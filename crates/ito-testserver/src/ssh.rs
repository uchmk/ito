//! A fake SSH server (russh) with exec and SFTP. See [`SshServer`].

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};
use russh::server::{Auth, ChannelOpenHandle, Msg, Server as _, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId, Sig};
use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::unique;

/// An OpenSSH key pair as text, to give the server as its host key, to put in a client's key file, or to allow.
#[derive(Clone, Debug)]
pub struct KeyPair {
    /// The private key, an OpenSSH file's content (write it to a file for `Auth::Key`).
    pub private_openssh: String,
    /// The public key, one `ssh-ed25519 AAAA...` line (for [`Config::allowed_keys`]).
    pub public_openssh: String,
}

impl KeyPair {
    /// A new Ed25519 key pair.
    pub fn generate() -> KeyPair {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).expect("a key");
        KeyPair {
            private_openssh: key.to_openssh(russh::keys::ssh_key::LineEnding::LF).expect("openssh text").to_string(),
            public_openssh: key.public_key().to_openssh().expect("openssh public key"),
        }
    }

    /// `SHA256:...`, the format `ssh-keygen -l` prints and `ito-sftp` pins.
    pub fn fingerprint(&self) -> String {
        PublicKey::from_openssh(&self.public_openssh).expect("a public key").fingerprint(HashAlg::Sha256).to_string()
    }
}

/// What a server is started with.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// The host key. A new one is made when this is `None`.
    pub host_key: Option<KeyPair>,
    /// The `(user, password)` that password login wants. `None` takes any user and password.
    pub login: Option<(String, String)>,
    /// Public keys (OpenSSH lines) that may log in. With none, public-key login is refused. The user name must be
    /// `login`'s user when `login` is set, and is not checked otherwise.
    pub allowed_keys: Vec<String>,
    /// Refuse every login, whatever it offers.
    pub reject_all: bool,
    /// Accept the TCP connection and say nothing: no banner, no key exchange (for timeout tests).
    pub silent: bool,
}

/// What a scripted command does.
#[derive(Clone, Debug, Default)]
pub struct Output {
    /// Pieces of output, in the order they are sent: `(is_stderr, bytes)`. Each is its own channel message.
    pub chunks: Vec<(bool, Vec<u8>)>,
    /// The exit code the command ends with.
    pub exit: u32,
    /// End by `exit-signal` (`KILL`, ...) instead of an exit code.
    pub signal: Option<String>,
    /// How long after the command starts the first chunk is sent.
    pub delay: Duration,
    /// How long between chunks.
    pub chunk_delay: Duration,
    /// Never end by itself: only a `signal` request or the client closing the channel ends it.
    pub hang: bool,
}

impl Output {
    pub fn new() -> Output {
        Output::default()
    }

    /// A piece of stdout; text or raw bytes (for CP932 pass the encoded bytes).
    pub fn stdout(mut self, bytes: impl AsRef<[u8]>) -> Output {
        self.chunks.push((false, bytes.as_ref().to_vec()));
        self
    }

    pub fn stderr(mut self, bytes: impl AsRef<[u8]>) -> Output {
        self.chunks.push((true, bytes.as_ref().to_vec()));
        self
    }

    pub fn exit(mut self, code: u32) -> Output {
        self.exit = code;
        self
    }

    /// End by the signal `name` (`"KILL"`) with no exit code.
    pub fn signal(mut self, name: &str) -> Output {
        self.signal = Some(name.to_owned());
        self
    }

    pub fn delay(mut self, delay: Duration) -> Output {
        self.delay = delay;
        self
    }

    pub fn chunk_delay(mut self, delay: Duration) -> Output {
        self.chunk_delay = delay;
        self
    }

    pub fn hang(mut self) -> Output {
        self.hang = true;
        self
    }
}

/// One login attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Login {
    pub user: String,
    /// `"password"` or `"publickey"`.
    pub method: String,
    /// For a public key, its `SHA256:...` fingerprint; empty for a password (which is never kept).
    pub key: String,
    pub accepted: bool,
}

struct State {
    config: Config,
    accept_env: AtomicBool,
    damage_writes: AtomicBool,
    damage_reads: AtomicBool,
    root: PathBuf,
    allowed: Vec<PublicKey>,
    scripts: Mutex<Vec<(String, Output)>>,
    denied: Mutex<Vec<String>>,
    logins: Mutex<Vec<Login>>,
    commands: Mutex<Vec<String>>,
    envs: Mutex<Vec<(String, String)>>,
    signals: Mutex<Vec<String>>,
    closed: Mutex<u32>,
    log: Mutex<Vec<String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl State {
    fn note(&self, line: String) {
        lock(&self.log).push(line);
    }
}

/// A fake SSH server on `127.0.0.1` for `ito-sftp`: password and public-key login, `exec` of scripted commands, `env`
/// requests (accepted or refused), and an SFTP subsystem over a temp folder. It records the logins, command lines,
/// environment variables, signals and closed channels. It stops, and removes the folder, when it is dropped.
///
/// A command is scripted by a part of its command line ([`on_command`](Self::on_command)); one that no script matches
/// gets `sha256sum -- '<path>'` answered from the SFTP folder (so `Verify::Exec` works) and otherwise
/// `<cmd>: command not found` with exit code 127.
pub struct SshServer {
    state: Arc<State>,
    addr: SocketAddr,
    fingerprint: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SshServer {
    /// Start a server with a new host key that takes any password login and refuses public keys.
    pub fn start() -> io::Result<SshServer> {
        Self::start_with(Config::default())
    }

    /// Start a server with `config`.
    pub fn start_with(config: Config) -> io::Result<SshServer> {
        let host = config.host_key.clone().unwrap_or_else(KeyPair::generate);
        let host_key = PrivateKey::from_openssh(&host.private_openssh).map_err(io::Error::other)?;
        let fingerprint = host.fingerprint();
        let allowed =
            config.allowed_keys.iter().map(|k| PublicKey::from_openssh(k).map_err(io::Error::other)).collect::<io::Result<Vec<_>>>()?;
        let root = std::env::temp_dir().join(format!("ito-testserver-ssh-{}-{}", std::process::id(), unique()));
        std::fs::create_dir_all(&root)?;
        let silent = config.silent;
        let state = Arc::new(State {
            config,
            accept_env: AtomicBool::new(true),
            damage_writes: AtomicBool::new(false),
            damage_reads: AtomicBool::new(false),
            root,
            allowed,
            scripts: Mutex::new(Vec::new()),
            denied: Mutex::new(Vec::new()),
            logins: Mutex::new(Vec::new()),
            commands: Mutex::new(Vec::new()),
            envs: Mutex::new(Vec::new()),
            signals: Mutex::new(Vec::new()),
            closed: Mutex::new(0),
            log: Mutex::new(Vec::new()),
        });
        let russh_config = russh::server::Config {
            auth_rejection_time: Duration::from_millis(10),
            auth_rejection_time_initial: Some(Duration::ZERO),
            keys: vec![host_key],
            ..Default::default()
        };
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let (tx, rx) = std::sync::mpsc::channel();
        let st = state.clone();
        let thread = std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(rt) => rt,
                Err(e) => return drop(tx.send(Err(e))),
            };
            rt.block_on(async move {
                let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
                    Ok(l) => l,
                    Err(e) => return drop(tx.send(Err(e))),
                };
                let _ = tx.send(listener.local_addr());
                if silent {
                    // Hold the sockets open, unanswered, until the server is dropped.
                    let mut held = Vec::new();
                    let mut stopped = stopped;
                    loop {
                        tokio::select! {
                            _ = &mut stopped => break,
                            r = listener.accept() => match r {
                                Ok((s, _)) => held.push(s),
                                Err(_) => break,
                            },
                        }
                    }
                } else {
                    let mut srv = Srv { state: st };
                    tokio::select! {
                        _ = stopped => {}
                        _ = srv.run_on_socket(Arc::new(russh_config), &listener) => {}
                    }
                }
            });
            rt.shutdown_timeout(Duration::from_secs(1));
        });
        let addr = rx.recv().map_err(|_| io::Error::other("the server thread ended"))??;
        Ok(SshServer { state, addr, fingerprint, stop: Some(stop), thread: Some(thread) })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The host key's `SHA256:...`, the format `ito-sftp` pins.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The folder the SFTP subsystem serves; `/` over SFTP is this folder.
    pub fn root(&self) -> &std::path::Path {
        &self.state.root
    }

    /// The local path of a path on the SFTP side (`/up/a.txt`).
    pub fn local(&self, remote: &str) -> PathBuf {
        self.state.root.join(remote.trim_start_matches('/'))
    }

    /// Script a command: when a command line contains `needle`, it does `output`. The first script that matches wins.
    pub fn on_command(&self, needle: &str, output: Output) {
        lock(&self.state.scripts).push((needle.to_owned(), output));
    }

    /// Accept (the default) or refuse `env` requests.
    pub fn set_accept_env(&self, accept: bool) {
        self.state.accept_env.store(accept, Ordering::Relaxed);
    }

    /// Make a path (and everything under it) unwritable over SFTP: opening for write, `mkdir`, `rmdir`, `remove` and
    /// `rename` answer permission denied. Reading and listing still work.
    pub fn deny_write(&self, remote: &str) {
        lock(&self.state.denied).push(norm(remote));
    }

    /// Undo every [`deny_write`](Self::deny_write).
    pub fn allow_write(&self) {
        lock(&self.state.denied).clear();
    }

    /// Flip the first byte of every file as it is written / read, as a broken link or disk would (for `Options::verify`).
    pub fn set_damage(&self, writes: bool, reads: bool) {
        self.state.damage_writes.store(writes, Ordering::Relaxed);
        self.state.damage_reads.store(reads, Ordering::Relaxed);
    }

    /// Every login attempt, in order, with the method it used.
    pub fn logins(&self) -> Vec<Login> {
        lock(&self.state.logins).clone()
    }

    /// The command lines of the `exec` requests, in order.
    pub fn commands(&self) -> Vec<String> {
        lock(&self.state.commands).clone()
    }

    /// The `env` requests received (whether accepted or not), in order.
    pub fn envs(&self) -> Vec<(String, String)> {
        lock(&self.state.envs).clone()
    }

    /// The names of the `signal` requests received (`TERM`, ...), in order.
    pub fn signals(&self) -> Vec<String> {
        lock(&self.state.signals).clone()
    }

    /// How many channels the client closed.
    pub fn channels_closed(&self) -> u32 {
        *lock(&self.state.closed)
    }

    /// What happened, one line each: `login password ito ok`, `exec <cmd>`, `env K=V accepted`, `signal TERM`,
    /// `close`, `sftp <op> <path>`. A password is never logged.
    pub fn log(&self) -> Vec<String> {
        lock(&self.state.log).clone()
    }

    /// Wait until `done(self)` is true, at most `timeout`. For `signals()` / `channels_closed()` after a stop.
    pub fn wait_until(&self, timeout: Duration, done: impl Fn(&SshServer) -> bool) -> bool {
        let end = Instant::now() + timeout;
        loop {
            if done(self) {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for SshServer {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_dir_all(&self.state.root);
    }
}

fn norm(p: &str) -> String {
    format!("/{}", p.trim_matches('/'))
}

#[derive(Clone)]
struct Srv {
    state: Arc<State>,
}

impl russh::server::Server for Srv {
    type Handler = Ssh;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Ssh {
        Ssh { state: self.state.clone(), channels: HashMap::new(), ctl: HashMap::new() }
    }
}

#[derive(Debug)]
struct SrvError(#[allow(dead_code)] russh::Error);

impl From<russh::Error> for SrvError {
    fn from(e: russh::Error) -> Self {
        SrvError(e)
    }
}

/// What the task running a command waits on.
#[derive(Default)]
struct Ctl {
    /// Set to the signal's name (or `""` for a close) when the client ends the command.
    ended: Mutex<Option<String>>,
    wake: Notify,
}

impl Ctl {
    fn end(&self, how: &str) {
        let mut e = lock(&self.ended);
        if e.is_none() {
            *e = Some(how.to_owned());
        }
        drop(e);
        self.wake.notify_one();
    }

    fn ended(&self) -> Option<String> {
        lock(&self.ended).clone()
    }

    /// Sleep `d`, or until ended. True when ended.
    async fn sleep(&self, d: Duration) -> bool {
        if !d.is_zero() {
            tokio::select! {
                _ = tokio::time::sleep(d) => {}
                _ = self.wake.notified() => {}
            }
        }
        self.ended().is_some()
    }
}

struct Ssh {
    state: Arc<State>,
    channels: HashMap<ChannelId, Channel<Msg>>,
    ctl: HashMap<ChannelId, Arc<Ctl>>,
}

impl Ssh {
    fn login(&self, user: &str, method: &str, key: String, accepted: bool) -> Auth {
        let accepted = accepted && !self.state.config.reject_all;
        lock(&self.state.logins).push(Login { user: user.to_owned(), method: method.to_owned(), key, accepted });
        self.state.note(format!("login {method} {user} {}", if accepted { "ok" } else { "refused" }));
        if accepted {
            Auth::Accept
        } else {
            Auth::reject()
        }
    }
}

impl russh::server::Handler for Ssh {
    type Error = SrvError;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, SrvError> {
        let ok = match &self.state.config.login {
            Some((u, p)) => u == user && p == password,
            None => true,
        };
        Ok(self.login(user, "password", String::new(), ok))
    }

    async fn auth_publickey(&mut self, user: &str, key: VerifiedPublicKey<'_>) -> Result<Auth, SrvError> {
        let user_ok = self.state.config.login.as_ref().is_none_or(|(u, _)| u == user);
        let known = self.state.allowed.iter().any(|k| k == key.public_key);
        let fp = key.public_key.fingerprint(HashAlg::Sha256).to_string();
        Ok(self.login(user, "publickey", fp, user_ok && known))
    }

    async fn channel_open_session(&mut self, channel: Channel<Msg>, reply: ChannelOpenHandle, _: &mut Session) -> Result<(), SrvError> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn channel_eof(&mut self, _: ChannelId, _: &mut Session) -> Result<(), SrvError> {
        Ok(())
    }

    async fn channel_close(&mut self, channel: ChannelId, _: &mut Session) -> Result<(), SrvError> {
        *lock(&self.state.closed) += 1;
        self.state.note("close".into());
        if let Some(c) = self.ctl.remove(&channel) {
            c.end("");
        }
        self.channels.remove(&channel);
        Ok(())
    }

    async fn signal(&mut self, channel: ChannelId, signal: Sig, _: &mut Session) -> Result<(), SrvError> {
        let name = match signal {
            Sig::Custom(c) => c,
            other => format!("{other:?}"),
        };
        lock(&self.state.signals).push(name.clone());
        self.state.note(format!("signal {name}"));
        if let Some(c) = self.ctl.get(&channel) {
            c.end(&name);
        }
        Ok(())
    }

    async fn env_request(&mut self, channel: ChannelId, name: &str, value: &str, session: &mut Session) -> Result<(), SrvError> {
        let accept = self.state.accept_env.load(Ordering::Relaxed);
        lock(&self.state.envs).push((name.to_owned(), value.to_owned()));
        self.state.note(format!("env {name}={value} {}", if accept { "accepted" } else { "refused" }));
        if accept {
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }

    async fn exec_request(&mut self, id: ChannelId, data: &[u8], session: &mut Session) -> Result<(), SrvError> {
        let command = String::from_utf8_lossy(data).into_owned();
        lock(&self.state.commands).push(command.clone());
        self.state.note(format!("exec {command}"));
        let script = lock(&self.state.scripts).iter().find(|(needle, _)| command.contains(needle.as_str())).map(|(_, o)| o.clone());
        let script = script.unwrap_or_else(|| default_answer(&self.state, &command));
        let ctl = Arc::new(Ctl::default());
        self.ctl.insert(id, ctl.clone());
        session.channel_success(id)?;
        let handle = session.handle();
        tokio::spawn(async move {
            run_script(handle, id, script, ctl).await;
        });
        Ok(())
    }

    async fn subsystem_request(&mut self, id: ChannelId, name: &str, session: &mut Session) -> Result<(), SrvError> {
        if name == "sftp" {
            let Some(channel) = self.channels.remove(&id) else { return Ok(session.channel_failure(id)?) };
            session.channel_success(id)?;
            self.state.note("subsystem sftp".into());
            let fs = Fs { state: self.state.clone(), files: HashMap::new(), dirs: HashMap::new(), next: 0 };
            russh_sftp::server::run(channel.into_stream(), fs).await;
        } else {
            session.channel_failure(id)?;
        }
        Ok(())
    }
}

/// `sha256sum -- '<path>'` from the SFTP folder, else "command not found".
fn default_answer(state: &State, command: &str) -> Output {
    let hash = command.strip_prefix("sha256sum -- '").and_then(|c| c.strip_suffix('\''));
    if let Some(p) = hash {
        return match std::fs::read(state.root.join(p.trim_start_matches('/'))) {
            Ok(body) => {
                let hex: String = Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect();
                Output::new().stdout(format!("{hex}  {p}\n"))
            }
            Err(_) => Output::new().stderr(format!("sha256sum: {p}: No such file or directory\n")).exit(1),
        };
    }
    Output::new().stderr(format!("{command}: command not found\n")).exit(127)
}

async fn run_script(handle: russh::server::Handle, id: ChannelId, script: Output, ctl: Arc<Ctl>) {
    if ctl.sleep(script.delay).await {
        return;
    }
    for (i, (is_err, bytes)) in script.chunks.iter().enumerate() {
        if i > 0 && ctl.sleep(script.chunk_delay).await {
            return;
        }
        let sent = if *is_err { handle.extended_data(id, 1, bytes.clone()).await } else { handle.data(id, bytes.clone()).await };
        if sent.is_err() {
            return;
        }
    }
    if script.hang {
        while ctl.ended().is_none() {
            ctl.wake.notified().await;
        }
        match ctl.ended() {
            // The client closed the channel: nothing more to say.
            Some(s) if s.is_empty() => return,
            // Killed by the signal it sent.
            Some(s) => {
                let _ = handle.exit_signal_request(id, Sig::Custom(s), false, String::new(), String::new()).await;
                let _ = handle.eof(id).await;
                let _ = handle.close(id).await;
                return;
            }
            None => {}
        }
    }
    match &script.signal {
        Some(s) => {
            let _ = handle.exit_signal_request(id, Sig::Custom(s.clone()), false, String::new(), String::new()).await;
        }
        None => {
            let _ = handle.exit_status_request(id, script.exit).await;
        }
    }
    let _ = handle.eof(id).await;
    let _ = handle.close(id).await;
}

// ---- SFTP ----

struct Fs {
    state: Arc<State>,
    files: HashMap<String, std::fs::File>,
    dirs: HashMap<String, Option<Vec<File>>>,
    next: u32,
}

fn code(e: io::Error) -> StatusCode {
    match e.kind() {
        io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

fn ok(id: u32) -> Status {
    Status { id, status_code: StatusCode::Ok, error_message: "Ok".into(), language_tag: "en-US".into() }
}

fn attrs(md: &std::fs::Metadata) -> FileAttributes {
    let mut a = FileAttributes::from(md);
    if md.is_dir() {
        a.set_dir(true);
    } else {
        a.set_regular(true);
    }
    a
}

impl Fs {
    fn path(&self, p: &str) -> PathBuf {
        self.state.root.join(p.trim_start_matches('/'))
    }

    fn handle(&mut self) -> String {
        self.next += 1;
        format!("h{}", self.next)
    }

    /// Note the operation, and refuse it when the path is under a denied one.
    fn write_op(&self, op: &str, path: &str) -> Result<(), StatusCode> {
        self.state.note(format!("sftp {op} {path}"));
        let p = norm(path);
        let hit = lock(&self.state.denied).iter().any(|d| p == *d || p.starts_with(&format!("{d}/")));
        if hit {
            Err(StatusCode::PermissionDenied)
        } else {
            Ok(())
        }
    }
}

impl russh_sftp::server::Handler for Fs {
    type Error = StatusCode;

    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }

    async fn open(&mut self, id: u32, filename: String, flags: OpenFlags, _: FileAttributes) -> Result<Handle, StatusCode> {
        if flags.contains(OpenFlags::WRITE) || flags.contains(OpenFlags::CREATE) {
            self.write_op("write", &filename)?;
        } else {
            self.state.note(format!("sftp read {filename}"));
        }
        let mut o = std::fs::OpenOptions::new();
        o.read(flags.contains(OpenFlags::READ) || !flags.contains(OpenFlags::WRITE))
            .write(flags.contains(OpenFlags::WRITE))
            .create(flags.contains(OpenFlags::CREATE))
            .truncate(flags.contains(OpenFlags::TRUNCATE));
        let f = o.open(self.path(&filename)).map_err(code)?;
        let handle = self.handle();
        self.files.insert(handle.clone(), f);
        Ok(Handle { id, handle })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, StatusCode> {
        self.files.remove(&handle);
        self.dirs.remove(&handle);
        Ok(ok(id))
    }

    async fn read(&mut self, id: u32, handle: String, offset: u64, len: u32) -> Result<Data, StatusCode> {
        let damage = self.state.damage_reads.load(Ordering::Relaxed);
        let f = self.files.get_mut(&handle).ok_or(StatusCode::Failure)?;
        f.seek(SeekFrom::Start(offset)).map_err(code)?;
        let mut data = vec![0; len as usize];
        let n = f.read(&mut data).map_err(code)?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(n);
        if damage && offset == 0 {
            data[0] ^= 0xff;
        }
        Ok(Data { id, data })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, mut data: Vec<u8>) -> Result<Status, StatusCode> {
        if self.state.damage_writes.load(Ordering::Relaxed) && offset == 0 && !data.is_empty() {
            data[0] ^= 0xff;
        }
        let f = self.files.get_mut(&handle).ok_or(StatusCode::Failure)?;
        f.seek(SeekFrom::Start(offset)).map_err(code)?;
        f.write_all(&data).map_err(code)?;
        Ok(ok(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        self.stat(id, path).await
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        let md = std::fs::metadata(self.path(&path)).map_err(code)?;
        Ok(Attrs { id, attrs: attrs(&md) })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, StatusCode> {
        let md = self.files.get(&handle).ok_or(StatusCode::Failure)?.metadata().map_err(code)?;
        Ok(Attrs { id, attrs: attrs(&md) })
    }

    async fn setstat(&mut self, id: u32, _: String, _: FileAttributes) -> Result<Status, StatusCode> {
        Ok(ok(id))
    }

    async fn fsetstat(&mut self, id: u32, _: String, _: FileAttributes) -> Result<Status, StatusCode> {
        Ok(ok(id))
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, StatusCode> {
        self.state.note(format!("sftp ls {path}"));
        let mut list = Vec::new();
        for e in std::fs::read_dir(self.path(&path)).map_err(code)? {
            let e = e.map_err(code)?;
            list.push(File::new(e.file_name().to_string_lossy(), attrs(&e.metadata().map_err(code)?)));
        }
        let handle = self.handle();
        self.dirs.insert(handle.clone(), Some(list));
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, StatusCode> {
        match self.dirs.get_mut(&handle).and_then(Option::take) {
            Some(files) => Ok(Name { id, files }),
            None => Err(StatusCode::Eof),
        }
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, StatusCode> {
        self.write_op("remove", &filename)?;
        std::fs::remove_file(self.path(&filename)).map_err(code)?;
        Ok(ok(id))
    }

    async fn mkdir(&mut self, id: u32, path: String, _: FileAttributes) -> Result<Status, StatusCode> {
        self.write_op("mkdir", &path)?;
        std::fs::create_dir(self.path(&path)).map_err(code)?;
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        self.write_op("rmdir", &path)?;
        std::fs::remove_dir(self.path(&path)).map_err(code)?;
        Ok(ok(id))
    }

    async fn rename(&mut self, id: u32, old: String, new: String) -> Result<Status, StatusCode> {
        self.write_op("rename", &old)?;
        self.write_op("rename", &new)?;
        std::fs::rename(self.path(&old), self.path(&new)).map_err(code)?;
        Ok(ok(id))
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, StatusCode> {
        let p = if path == "." || path.is_empty() { "/".to_string() } else { path };
        Ok(Name { id, files: vec![File::dummy(p)] })
    }
}
