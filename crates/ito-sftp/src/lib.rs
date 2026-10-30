//! SFTP for the apps, with a plain blocking API. The async runtime (tokio, for russh) lives inside
//! a [`Session`]; the caller never sees it. Call from a worker thread: every call blocks until it is
//! done, a timeout passes or the stop flag is set.
//!
//! - [`fingerprint`] asks a server for its host key without logging in, so a user can check it and pin it.
//! - [`Session::connect`] refuses a server whose key is not the pinned one ([`Error::HostKeyMismatch`]).
//! - [`Session::put`] / [`Session::get`] take `*` and `?` in the file name, write `<name>.part` and rename
//!   it when whole, and never leave a `.part` behind on failure or stop.
//!
//! Secrets are passed in by the caller (see `ito-secret`); this crate keeps none.

use std::fmt;
use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use russh::client;
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{OpenFlags, StatusCode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Why a call failed. `Display` is a short Japanese sentence fit for a log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The server could not be reached (name, port, network).
    Connect(String),
    /// The server refused the user name, password or key.
    AuthRefused,
    /// The private key file could not be read or decrypted (wrong passphrase).
    Key(String),
    /// The server's host key is not the pinned one.
    HostKeyMismatch {
        expected: String,
        actual: String,
    },
    /// No such file or directory (remote), or no local file matches.
    NotFound(String),
    PermissionDenied(String),
    /// The target already exists and `overwrite` is off.
    Exists(String),
    /// The connection went away in the middle.
    Lost(String),
    /// A step took longer than the timeout.
    Timeout,
    /// The stop flag was set.
    Stopped,
    /// A local file could not be read or written.
    Local(String),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connect(m) => write!(f, "接続できません: {m}"),
            Error::AuthRefused => f.write_str("ログインを断られました（ユーザー名・パスワード・鍵を確かめてください）"),
            Error::Key(m) => write!(f, "秘密鍵を読めません: {m}"),
            Error::HostKeyMismatch { expected, actual } => {
                write!(f, "サーバーのホスト鍵が登録と違います（登録 {expected}、実際 {actual}）")
            }
            Error::NotFound(p) => write!(f, "見つかりません: {p}"),
            Error::PermissionDenied(p) => write!(f, "権限がありません: {p}"),
            Error::Exists(p) => write!(f, "すでにあります: {p}"),
            Error::Lost(m) => write!(f, "接続が途中で切れました: {m}"),
            Error::Timeout => f.write_str("時間内に終わりませんでした"),
            Error::Stopped => f.write_str("止められました"),
            Error::Local(m) => write!(f, "手元のファイルを扱えません: {m}"),
            Error::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<russh_sftp::client::error::Error> for Error {
    fn from(e: russh_sftp::client::error::Error) -> Self {
        use russh_sftp::client::error::Error as E;
        match e {
            E::Status(s) => match s.status_code {
                StatusCode::NoSuchFile => Error::NotFound(s.error_message),
                StatusCode::PermissionDenied => Error::PermissionDenied(s.error_message),
                StatusCode::NoConnection | StatusCode::ConnectionLost => Error::Lost(s.error_message),
                _ => Error::Other(format!("{}: {}", s.status_code, s.error_message)),
            },
            E::Timeout => Error::Timeout,
            E::IO(m) => Error::Lost(m),
            other => Error::Other(other.to_string()),
        }
    }
}

/// An error out of a remote file's reader or writer.
fn remote_io(path: &str, e: std::io::Error) -> Error {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => Error::NotFound(path.to_string()),
        K::PermissionDenied => Error::PermissionDenied(path.to_string()),
        K::TimedOut => Error::Timeout,
        _ => Error::Lost(e.to_string()),
    }
}

fn local_err(path: &Path, e: std::io::Error) -> Error {
    Error::Local(format!("{}: {e}", path.display()))
}

/// How to log in. Secrets come from the caller.
#[derive(Clone)]
pub enum Auth {
    Password(String),
    /// A private key file (OpenSSH or PEM) and its passphrase, if it has one.
    Key {
        path: PathBuf,
        passphrase: Option<String>,
    },
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Auth::Password(_) => f.write_str("Auth::Password(***)"),
            Auth::Key { path, passphrase } => {
                f.debug_struct("Auth::Key").field("path", path).field("passphrase", &passphrase.as_ref().map(|_| "***")).finish()
            }
        }
    }
}

/// Where to connect and how.
#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Auth,
    /// The host key the server must present, as `SHA256:…` (what [`fingerprint`] returns and `ssh-keygen -l` prints).
    pub host_key: String,
    /// For the TCP connection, the key exchange and the login, each.
    pub connect_timeout: Duration,
    /// For each chunk read or written, and for each request such as a listing.
    pub io_timeout: Duration,
}

impl Target {
    pub fn new(host: impl Into<String>, user: impl Into<String>, auth: Auth, host_key: impl Into<String>) -> Self {
        Target {
            host: host.into(),
            port: 22,
            user: user.into(),
            auth,
            host_key: host_key.into(),
            connect_timeout: Duration::from_secs(15),
            io_timeout: Duration::from_secs(30),
        }
    }
}

/// How a transfer treats a target that exists.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub overwrite: bool,
}

/// One file sent or fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    pub name: String,
    pub bytes: u64,
}

/// One line of a remote listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub is_dir: bool,
}

/// `fn(name, bytes done, bytes total)`, called after each chunk.
pub type Progress<'a> = &'a mut dyn FnMut(&str, u64, u64);

const CHUNK: usize = 64 * 1024;

#[derive(Debug)]
struct HandlerError(#[allow(dead_code)] russh::Error);

impl From<russh::Error> for HandlerError {
    fn from(e: russh::Error) -> Self {
        HandlerError(e)
    }
}

struct Check {
    pin: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
}

impl client::Handler for Check {
    type Error = HandlerError;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let fp = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256).to_string(),
            PublicKeyOrCertificate::Certificate(c) => c.public_key().fingerprint(HashAlg::Sha256).to_string(),
        };
        let ok = self.pin.as_deref().is_some_and(|p| same_fingerprint(p, &fp));
        *self.seen.lock().unwrap() = Some(fp);
        Ok(ok)
    }
}

/// `SHA256:abc` equals `abc=` and `sha256:abc`: only the digest counts.
fn same_fingerprint(a: &str, b: &str) -> bool {
    fn digest(s: &str) -> &str {
        let s = s.trim();
        let s = s.split_once(':').filter(|(h, _)| h.eq_ignore_ascii_case("sha256")).map_or(s, |(_, d)| d);
        s.trim_end_matches('=')
    }
    !digest(a).is_empty() && digest(a) == digest(b)
}

fn runtime() -> Result<tokio::runtime::Runtime, Error> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name("ito-sftp")
        .enable_all()
        .build()
        .map_err(|e| Error::Other(format!("runtime: {e}")))
}

async fn open_ssh(
    host: &str,
    port: u16,
    pin: Option<String>,
    timeout: Duration,
    seen: Arc<Mutex<Option<String>>>,
) -> Result<client::Handle<Check>, Error> {
    let config = Arc::new(client::Config { inactivity_timeout: None, ..Default::default() });
    let handler = Check { pin: pin.clone(), seen: seen.clone() };
    let attempt = tokio::time::timeout(timeout, client::connect(config, (host.to_string(), port), handler)).await;
    match attempt {
        Err(_) => Err(Error::Connect("時間切れ".into())),
        Ok(Ok(h)) => Ok(h),
        Ok(Err(e)) => {
            let actual = seen.lock().unwrap().clone();
            match (pin, actual) {
                (Some(expected), Some(actual)) if !same_fingerprint(&expected, &actual) => Err(Error::HostKeyMismatch { expected, actual }),
                _ => Err(Error::Connect(format!("{e:?}"))),
            }
        }
    }
}

/// The server's host key as `SHA256:…`, fetched without logging in. For showing to the user, who pins it.
pub fn fingerprint(host: &str, port: u16, timeout: Duration) -> Result<String, Error> {
    let rt = runtime()?;
    let seen = Arc::new(Mutex::new(None));
    let r = rt.block_on(open_ssh(host, port, None, timeout, seen.clone()));
    let got = seen.lock().unwrap().clone();
    rt.shutdown_background();
    match (got, r) {
        (Some(fp), _) => Ok(fp),
        (None, Err(e)) => Err(e),
        (None, Ok(_)) => Err(Error::Other("ホスト鍵を受け取れませんでした".into())),
    }
}

/// An open SFTP session. Dropping it disconnects.
pub struct Session {
    rt: Option<tokio::runtime::Runtime>,
    sftp: SftpSession,
    _ssh: client::Handle<Check>,
    io_timeout: Duration,
}

impl Session {
    /// Connect, check the host key against `target.host_key`, log in and start the SFTP subsystem.
    pub fn connect(target: &Target) -> Result<Session, Error> {
        let rt = runtime()?;
        match rt.block_on(Self::open(target)) {
            Ok((sftp, ssh)) => Ok(Session { rt: Some(rt), sftp, _ssh: ssh, io_timeout: target.io_timeout }),
            Err(e) => {
                rt.shutdown_background();
                Err(e)
            }
        }
    }

    async fn open(t: &Target) -> Result<(SftpSession, client::Handle<Check>), Error> {
        let seen = Arc::new(Mutex::new(None));
        let mut ssh = open_ssh(&t.host, t.port, Some(t.host_key.clone()), t.connect_timeout, seen).await?;
        let login = async {
            let res = match &t.auth {
                Auth::Password(p) => ssh.authenticate_password(t.user.clone(), p.clone()).await,
                Auth::Key { path, passphrase } => {
                    let key = russh::keys::load_secret_key(path, passphrase.as_deref()).map_err(|e| Error::Key(e.to_string()))?;
                    ssh.authenticate_publickey(t.user.clone(), PrivateKeyWithHashAlg::new(Arc::new(key), Some(HashAlg::Sha256))).await
                }
            };
            match res {
                Ok(r) if r.success() => Ok(()),
                Ok(_) => Err(Error::AuthRefused),
                Err(e) => Err(Error::Connect(e.to_string())),
            }
        };
        tokio::time::timeout(t.connect_timeout, login).await.map_err(|_| Error::Connect("ログインが時間切れ".into()))??;
        let subsystem = async {
            let channel = ssh.channel_open_session().await.map_err(|e| Error::Connect(e.to_string()))?;
            channel.request_subsystem(true, "sftp").await.map_err(|e| Error::Connect(format!("sftp を始められません: {e}")))?;
            let cfg = russh_sftp::client::Config { request_timeout_secs: t.io_timeout.as_secs().max(1), ..Default::default() };
            SftpSession::new_with_config(channel.into_stream(), cfg).await.map_err(Error::from)
        };
        let sftp = tokio::time::timeout(t.connect_timeout, subsystem).await.map_err(|_| Error::Connect("sftp が時間切れ".into()))??;
        Ok((sftp, ssh))
    }

    fn run<T>(&self, f: impl Future<Output = Result<T, Error>>) -> Result<T, Error> {
        self.rt.as_ref().expect("runtime lives until drop").block_on(f)
    }

    /// The entries of a remote directory, by name.
    pub fn list(&self, dir: &str) -> Result<Vec<Entry>, Error> {
        let never = AtomicBool::new(false);
        self.run(self.entries(dir, &never))
    }

    async fn entries(&self, dir: &str, stop: &AtomicBool) -> Result<Vec<Entry>, Error> {
        let read = step(self.io_timeout, stop, self.sftp.read_dir(dir)).await?;
        let mut out: Vec<Entry> = read
            .filter(|e| e.file_name() != "." && e.file_name() != "..")
            .map(|e| {
                let m = e.metadata();
                Entry { name: e.file_name(), size: m.len(), modified: m.modified().ok(), is_dir: m.file_type().is_dir() }
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Delete one remote file.
    pub fn remove(&self, path: &str) -> Result<(), Error> {
        let never = AtomicBool::new(false);
        self.run(step(self.io_timeout, &never, self.sftp.remove_file(path)))
    }

    /// Send `local` to `remote`. If the last part of `local` has `*` or `?`, every matching file in its
    /// folder is sent and `remote` is a directory. Otherwise `remote` is the new file's path, or a
    /// directory (when it ends in `/` or already is one) to put the file in under its own name.
    pub fn put(&self, local: &Path, remote: &str, opts: Options, stop: &AtomicBool, progress: Progress) -> Result<Vec<Moved>, Error> {
        self.run(self.put_all(local, remote, opts, stop, progress))
    }

    async fn put_all(
        &self,
        local: &Path,
        remote: &str,
        opts: Options,
        stop: &AtomicBool,
        progress: Progress<'_>,
    ) -> Result<Vec<Moved>, Error> {
        let pattern = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut jobs: Vec<(PathBuf, String)> = Vec::new();
        if has_wildcard(&pattern) {
            let dir = match local.parent() {
                Some(p) if !p.as_os_str().is_empty() => p,
                _ => Path::new("."),
            };
            for e in std::fs::read_dir(dir).map_err(|e| local_err(dir, e))? {
                let e = e.map_err(|e| local_err(dir, e))?;
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().is_file() && glob_match(&pattern, &name, cfg!(windows)) {
                    jobs.push((e.path(), join(remote, &name)));
                }
            }
            jobs.sort();
            if jobs.is_empty() {
                return Err(Error::NotFound(local.display().to_string()));
            }
        } else {
            let into_dir = remote.ends_with('/') || self.is_dir(remote, stop).await?;
            jobs.push((local.to_path_buf(), if into_dir { join(remote, &pattern) } else { remote.to_string() }));
        }
        let mut done = Vec::new();
        for (from, to) in jobs {
            done.push(self.put_one(&from, &to, opts, stop, progress).await?);
        }
        Ok(done)
    }

    async fn is_dir(&self, path: &str, stop: &AtomicBool) -> Result<bool, Error> {
        match step(self.io_timeout, stop, self.sftp.metadata(path)).await {
            Ok(m) => Ok(m.file_type().is_dir()),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn exists(&self, path: &str, stop: &AtomicBool) -> Result<bool, Error> {
        match step(self.io_timeout, stop, self.sftp.metadata(path)).await {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn put_one(&self, from: &Path, to: &str, opts: Options, stop: &AtomicBool, progress: Progress<'_>) -> Result<Moved, Error> {
        let name = from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        let mut src = std::fs::File::open(from).map_err(|e| local_err(from, e))?;
        let total = src.metadata().map_err(|e| local_err(from, e))?.len();
        let exists = self.exists(to, stop).await?;
        if exists && !opts.overwrite {
            return Err(Error::Exists(to.to_string()));
        }
        let part = format!("{to}.part");
        let finished = match self.send_part(&mut src, from, &part, &name, total, stop, progress).await {
            Ok(()) => self.swap_in(&part, to, exists, stop).await,
            Err(e) => Err(e),
        };
        match finished {
            Ok(()) => Ok(Moved { name, bytes: total }),
            Err(e) => {
                // Best effort, and never blocked by the stop flag that may be why we are here.
                let _ = tokio::time::timeout(self.io_timeout, self.sftp.remove_file(&part)).await;
                Err(e)
            }
        }
    }

    async fn swap_in(&self, part: &str, to: &str, exists: bool, stop: &AtomicBool) -> Result<(), Error> {
        if exists {
            step(self.io_timeout, stop, self.sftp.remove_file(to)).await?;
        }
        step(self.io_timeout, stop, self.sftp.rename(part, to)).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_part(
        &self,
        src: &mut std::fs::File,
        from: &Path,
        part: &str,
        name: &str,
        total: u64,
        stop: &AtomicBool,
        progress: Progress<'_>,
    ) -> Result<(), Error> {
        let flags = OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE;
        let mut dst = step(self.io_timeout, stop, self.sftp.open_with_flags(part, flags)).await?;
        let (mut buf, mut done) = (vec![0u8; CHUNK], 0u64);
        progress(name, 0, total);
        loop {
            let n = src.read(&mut buf).map_err(|e| local_err(from, e))?;
            if n == 0 {
                break;
            }
            step_io(self.io_timeout, stop, part, dst.write_all(&buf[..n])).await?;
            done += n as u64;
            progress(name, done, total);
        }
        step_io(self.io_timeout, stop, part, dst.shutdown()).await
    }

    /// Fetch `remote` into `local`. If the last part of `remote` has `*` or `?`, every matching file in its
    /// directory is fetched and `local` is a folder (made if missing). Otherwise `local` is the new file's
    /// path, or a folder (when it ends in a separator or already is one) to put the file in.
    pub fn get(&self, remote: &str, local: &Path, opts: Options, stop: &AtomicBool, progress: Progress) -> Result<Vec<Moved>, Error> {
        self.run(self.get_all(remote, local, opts, stop, progress))
    }

    async fn get_all(
        &self,
        remote: &str,
        local: &Path,
        opts: Options,
        stop: &AtomicBool,
        progress: Progress<'_>,
    ) -> Result<Vec<Moved>, Error> {
        let (dir, pattern) = match remote.rfind('/') {
            Some(0) => ("/", &remote[1..]),
            Some(i) => (&remote[..i], &remote[i + 1..]),
            None => (".", remote),
        };
        let mut jobs: Vec<(String, PathBuf)> = Vec::new();
        if has_wildcard(pattern) {
            std::fs::create_dir_all(local).map_err(|e| local_err(local, e))?;
            for e in self.entries(dir, stop).await? {
                if !e.is_dir && glob_match(pattern, &e.name, false) {
                    jobs.push((join(dir, &e.name), local.join(&e.name)));
                }
            }
            if jobs.is_empty() {
                return Err(Error::NotFound(remote.to_string()));
            }
        } else {
            let text = local.to_string_lossy();
            let into_dir = local.is_dir() || text.ends_with('/') || text.ends_with('\\');
            if into_dir {
                std::fs::create_dir_all(local).map_err(|e| local_err(local, e))?;
            }
            jobs.push((remote.to_string(), if into_dir { local.join(pattern) } else { local.to_path_buf() }));
        }
        let mut done = Vec::new();
        for (from, to) in jobs {
            done.push(self.get_one(&from, &to, opts, stop, progress).await?);
        }
        Ok(done)
    }

    async fn get_one(&self, from: &str, to: &Path, opts: Options, stop: &AtomicBool, progress: Progress<'_>) -> Result<Moved, Error> {
        let name = from.rsplit('/').next().unwrap_or(from).to_string();
        if to.exists() && !opts.overwrite {
            return Err(Error::Exists(to.display().to_string()));
        }
        let mut src = step(self.io_timeout, stop, self.sftp.open(from)).await?;
        let total = step(self.io_timeout, stop, self.sftp.metadata(from)).await.map(|m| m.len()).unwrap_or(0);
        let mut part = to.as_os_str().to_owned();
        part.push(".part");
        let part = PathBuf::from(part);
        let got = self.fetch_part(&mut src, from, &part, &name, total, stop, progress).await;
        let finished = got.and_then(|n| std::fs::rename(&part, to).map(|_| n).map_err(|e| local_err(to, e)));
        match finished {
            Ok(bytes) => Ok(Moved { name, bytes }),
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Err(e)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_part(
        &self,
        src: &mut russh_sftp::client::fs::File,
        from: &str,
        part: &Path,
        name: &str,
        total: u64,
        stop: &AtomicBool,
        progress: Progress<'_>,
    ) -> Result<u64, Error> {
        let mut dst = std::fs::File::create(part).map_err(|e| local_err(part, e))?;
        let (mut buf, mut done) = (vec![0u8; CHUNK], 0u64);
        progress(name, 0, total);
        loop {
            let n = step_io(self.io_timeout, stop, from, src.read(&mut buf)).await?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).map_err(|e| local_err(part, e))?;
            done += n as u64;
            progress(name, done, total.max(done));
        }
        dst.flush().map_err(|e| local_err(part, e))?;
        Ok(done)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            // Closing politely may hang on a dead link; the connection ends with the runtime.
            let _ = rt.block_on(async { tokio::time::timeout(Duration::from_secs(2), self.sftp.close()).await });
            rt.shutdown_background();
        }
    }
}

/// Wait for `f`, at most `timeout`, giving up early when `stop` is set.
async fn step<T, E: Into<Error>>(timeout: Duration, stop: &AtomicBool, f: impl Future<Output = Result<T, E>>) -> Result<T, Error> {
    tokio::pin!(f);
    let end = tokio::time::Instant::now() + timeout;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        let left = end.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return Err(Error::Timeout);
        }
        if let Ok(r) = tokio::time::timeout(left.min(Duration::from_millis(100)), f.as_mut()).await {
            return r.map_err(Into::into);
        }
    }
}

/// [`step`] for the `io::Error` of a remote file's reader or writer.
async fn step_io<T>(timeout: Duration, stop: &AtomicBool, path: &str, f: impl Future<Output = std::io::Result<T>>) -> Result<T, Error> {
    step(timeout, stop, async { f.await.map_err(|e| remote_io(path, e)) }).await
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Whether `name` (a bare file name) has a wildcard.
pub fn has_wildcard(name: &str) -> bool {
    name.contains(['*', '?'])
}

/// `*` (any run, also empty) and `?` (one character) against a bare file name.
pub fn glob_match(pattern: &str, name: &str, ignore_case: bool) -> bool {
    let fold = |s: &str| -> Vec<char> {
        s.chars().flat_map(|c| if ignore_case { c.to_lowercase().collect::<Vec<_>>() } else { vec![c] }).collect()
    };
    let (p, n) = (fold(pattern), fold(name));
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(glob_match("*.log", "a.log", false));
        assert!(glob_match("*.log", ".log", false));
        assert!(!glob_match("*.log", "a.log.part", false));
        assert!(glob_match("a?c", "abc", false));
        assert!(!glob_match("a?c", "ac", false));
        assert!(glob_match("*a*b*", "xxaybz", false));
        assert!(!glob_match("A*", "abc", false));
        assert!(glob_match("A*", "abc", true));
        assert!(glob_match("*", "", false));
        assert!(has_wildcard("a?") && !has_wildcard("a.txt"));
    }

    #[test]
    fn fingerprints_compare_by_digest() {
        assert!(same_fingerprint("SHA256:abc", "SHA256:abc="));
        assert!(same_fingerprint("sha256:abc", "abc"));
        assert!(!same_fingerprint("SHA256:abc", "SHA256:abd"));
        assert!(!same_fingerprint("", ""));
    }

    #[test]
    fn secrets_stay_out_of_debug() {
        let a = Auth::Password("hunter2".into());
        assert!(!format!("{a:?}").contains("hunter2"));
        let k = Auth::Key { path: "k".into(), passphrase: Some("hunter2".into()) };
        assert!(!format!("{k:?}").contains("hunter2"));
    }

    #[test]
    fn joins() {
        assert_eq!(join("/a", "b"), "/a/b");
        assert_eq!(join("/a/", "b"), "/a/b");
        assert_eq!(join("", "b"), "b");
    }
}
