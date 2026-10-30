//! `ito-sftp` against a real russh SSH server on 127.0.0.1 whose SFTP side is a temp folder.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use ito_sftp::{fingerprint, Auth, Error, Options, Session, Target};
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};
use russh::server::{Auth as SrvAuth, ChannelOpenHandle, Msg, Server as _, Session as SrvSession, VerifiedPublicKey};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode};

const USER: &str = "ito";
const PASSWORD: &str = "open-sesame";

#[derive(Debug)]
struct SrvError(#[allow(dead_code)] russh::Error);

impl From<russh::Error> for SrvError {
    fn from(e: russh::Error) -> Self {
        SrvError(e)
    }
}

#[derive(Clone)]
struct Srv {
    root: PathBuf,
    allowed: Option<PublicKey>,
}

impl russh::server::Server for Srv {
    type Handler = Ssh;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Ssh {
        Ssh { srv: self.clone(), channels: HashMap::new() }
    }
}

struct Ssh {
    srv: Srv,
    channels: HashMap<ChannelId, Channel<Msg>>,
}

impl russh::server::Handler for Ssh {
    type Error = SrvError;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<SrvAuth, SrvError> {
        Ok(if user == USER && password == PASSWORD { SrvAuth::Accept } else { SrvAuth::reject() })
    }

    async fn auth_publickey(&mut self, user: &str, key: VerifiedPublicKey<'_>) -> Result<SrvAuth, SrvError> {
        Ok(if user == USER && self.srv.allowed.as_ref() == Some(key.public_key) { SrvAuth::Accept } else { SrvAuth::reject() })
    }

    async fn channel_open_session(&mut self, channel: Channel<Msg>, reply: ChannelOpenHandle, _: &mut SrvSession) -> Result<(), SrvError> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn channel_eof(&mut self, channel: ChannelId, session: &mut SrvSession) -> Result<(), SrvError> {
        session.close(channel)?;
        Ok(())
    }

    async fn subsystem_request(&mut self, id: ChannelId, name: &str, session: &mut SrvSession) -> Result<(), SrvError> {
        if name == "sftp" {
            let channel = self.channels.remove(&id).expect("channel opened first");
            session.channel_success(id)?;
            russh_sftp::server::run(
                channel.into_stream(),
                Fs { root: self.srv.root.clone(), files: HashMap::new(), dirs: HashMap::new(), next: 0 },
            )
            .await;
        } else {
            session.channel_failure(id)?;
        }
        Ok(())
    }
}

struct Fs {
    root: PathBuf,
    files: HashMap<String, std::fs::File>,
    dirs: HashMap<String, Option<Vec<File>>>,
    next: u32,
}

fn code(e: std::io::Error) -> StatusCode {
    match e.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
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
        self.root.join(p.trim_start_matches('/'))
    }

    fn handle(&mut self) -> String {
        self.next += 1;
        format!("h{}", self.next)
    }
}

impl russh_sftp::server::Handler for Fs {
    type Error = StatusCode;

    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }

    async fn open(&mut self, id: u32, filename: String, flags: OpenFlags, _: FileAttributes) -> Result<Handle, StatusCode> {
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
        let f = self.files.get_mut(&handle).ok_or(StatusCode::Failure)?;
        f.seek(SeekFrom::Start(offset)).map_err(code)?;
        let mut data = vec![0; len as usize];
        let n = f.read(&mut data).map_err(code)?;
        if n == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(n);
        Ok(Data { id, data })
    }

    async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, StatusCode> {
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

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, StatusCode> {
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
        std::fs::remove_file(self.path(&filename)).map_err(code)?;
        Ok(ok(id))
    }

    async fn rename(&mut self, id: u32, old: String, new: String) -> Result<Status, StatusCode> {
        std::fs::rename(self.path(&old), self.path(&new)).map_err(code)?;
        Ok(ok(id))
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, StatusCode> {
        let p = if path == "." || path.is_empty() { "/".to_string() } else { path };
        Ok(Name { id, files: vec![File::dummy(p)] })
    }
}

struct Server {
    port: u16,
    fingerprint: String,
    /// What the server's folder is.
    root: tempfile::TempDir,
    /// A private key the server accepts, as an OpenSSH file in `root`'s sibling folder.
    key_file: PathBuf,
    _keys: tempfile::TempDir,
}

fn start() -> Server {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("up")).unwrap();
    let keys = tempfile::tempdir().unwrap();
    let host = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let user = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let key_file = keys.path().join("id_ed25519");
    std::fs::write(&key_file, user.to_openssh(russh::keys::ssh_key::LineEnding::LF).unwrap().as_bytes()).unwrap();
    let fingerprint = host.public_key().fingerprint(HashAlg::Sha256).to_string();
    let mut srv = Srv { root: root.path().to_path_buf(), allowed: Some(user.public_key().clone()) };
    let config = russh::server::Config {
        auth_rejection_time: Duration::from_millis(10),
        auth_rejection_time_initial: Some(Duration::ZERO),
        keys: vec![host],
        ..Default::default()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap().port()).unwrap();
            let _ = srv.run_on_socket(Arc::new(config), &listener).await;
        });
    });
    Server { port: rx.recv().unwrap(), fingerprint, root, key_file, _keys: keys }
}

impl Server {
    fn target(&self) -> Target {
        let mut t = Target::new("127.0.0.1", USER, Auth::Password(PASSWORD.into()), self.fingerprint.clone());
        t.port = self.port;
        t.connect_timeout = Duration::from_secs(10);
        t.io_timeout = Duration::from_secs(10);
        t
    }

    fn session(&self) -> Session {
        Session::connect(&self.target()).expect("connect")
    }

    fn up(&self, name: &str) -> PathBuf {
        self.root.path().join("up").join(name)
    }
}

fn no_stop() -> AtomicBool {
    AtomicBool::new(false)
}

fn write(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

#[test]
fn wildcards_go_up_and_come_back_without_a_part_left() {
    let s = start();
    let local = tempfile::tempdir().unwrap();
    write(local.path(), "a.log", b"alpha");
    write(local.path(), "b.log", b"bravo");
    write(local.path(), "c.txt", b"charlie");
    let ses = s.session();
    let stop = no_stop();

    let sent = ses.put(&local.path().join("*.log"), "/up/", Options::default(), &stop, &mut |_, _, _| {}).unwrap();
    assert_eq!(sent.iter().map(|m| (m.name.as_str(), m.bytes)).collect::<Vec<_>>(), [("a.log", 5), ("b.log", 5)]);
    let names: Vec<_> = ses.list("/up").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["a.log", "b.log"]);
    assert_eq!(std::fs::read(s.up("b.log")).unwrap(), b"bravo");

    let out = tempfile::tempdir().unwrap();
    let got = ses.get("/up/*.log", out.path(), Options::default(), &stop, &mut |_, _, _| {}).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(std::fs::read(out.path().join("a.log")).unwrap(), b"alpha");
    let mut left: Vec<_> = std::fs::read_dir(out.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["a.log", "b.log"]);
}

#[test]
fn an_existing_target_needs_overwrite() {
    let s = start();
    let local = tempfile::tempdir().unwrap();
    let f = write(local.path(), "n.txt", b"first");
    let ses = s.session();
    let stop = no_stop();
    ses.put(&f, "/up/n.txt", Options::default(), &stop, &mut |_, _, _| {}).unwrap();

    std::fs::write(&f, b"second, longer").unwrap();
    let e = ses.put(&f, "/up/n.txt", Options::default(), &stop, &mut |_, _, _| {}).unwrap_err();
    assert!(matches!(e, Error::Exists(_)), "{e:?}");
    assert_eq!(std::fs::read(s.up("n.txt")).unwrap(), b"first");
    assert!(!s.up("n.txt.part").exists());

    ses.put(&f, "/up/n.txt", Options { overwrite: true }, &stop, &mut |_, _, _| {}).unwrap();
    assert_eq!(std::fs::read(s.up("n.txt")).unwrap(), b"second, longer");

    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("n.txt");
    std::fs::write(&dest, b"mine").unwrap();
    let e = ses.get("/up/n.txt", &dest, Options::default(), &stop, &mut |_, _, _| {}).unwrap_err();
    assert!(matches!(e, Error::Exists(_)), "{e:?}");
    assert_eq!(std::fs::read(&dest).unwrap(), b"mine");
    ses.get("/up/n.txt", &dest, Options { overwrite: true }, &stop, &mut |_, _, _| {}).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"second, longer");
}

#[test]
fn a_big_file_reports_progress_to_the_end() {
    let s = start();
    let local = tempfile::tempdir().unwrap();
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let f = write(local.path(), "big.bin", &body);
    let ses = s.session();
    let stop = no_stop();

    let mut last = (0, 0);
    ses.put(&f, "/up", Options::default(), &stop, &mut |name, done, total| {
        assert_eq!(name, "big.bin");
        assert!(done >= last.0);
        last = (done, total);
    })
    .unwrap();
    assert_eq!(last, (300_000, 300_000));
    assert_eq!(std::fs::read(s.up("big.bin")).unwrap(), body);

    let out = tempfile::tempdir().unwrap();
    let mut last = (0, 0);
    let got = ses.get("/up/big.bin", &out.path().join("copy.bin"), Options::default(), &stop, &mut |_, d, t| last = (d, t)).unwrap();
    assert_eq!(got[0].bytes, 300_000);
    assert_eq!(last, (300_000, 300_000));
    assert_eq!(std::fs::read(out.path().join("copy.bin")).unwrap(), body);
}

#[test]
fn list_and_remove() {
    let s = start();
    write(&s.root.path().join("up"), "x.txt", b"12345");
    std::fs::create_dir(s.root.path().join("up").join("sub")).unwrap();
    let ses = s.session();
    let list = ses.list("/up").unwrap();
    assert_eq!(
        list.iter().map(|e| (e.name.as_str(), e.size, e.is_dir)).collect::<Vec<_>>(),
        [("sub", list[0].size, true), ("x.txt", 5, false)]
    );
    assert!(list[1].modified.is_some());
    ses.remove("/up/x.txt").unwrap();
    assert!(!s.up("x.txt").exists());
    assert!(matches!(ses.remove("/up/x.txt"), Err(Error::NotFound(_))));
    assert!(matches!(ses.list("/nowhere"), Err(Error::NotFound(_))));
}

#[test]
fn a_missing_file_is_not_found_and_leaves_nothing() {
    let s = start();
    let out = tempfile::tempdir().unwrap();
    let ses = s.session();
    let e = ses.get("/up/none.txt", &out.path().join("none.txt"), Options::default(), &no_stop(), &mut |_, _, _| {}).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)), "{e:?}");
    let e = ses.get("/up/*.zip", out.path(), Options::default(), &no_stop(), &mut |_, _, _| {}).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)), "{e:?}");
    let e = ses.put(&out.path().join("*.zip"), "/up/", Options::default(), &no_stop(), &mut |_, _, _| {}).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)), "{e:?}");
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn the_stop_flag_ends_a_transfer_and_leaves_no_part() {
    let s = start();
    let local = tempfile::tempdir().unwrap();
    let f = write(local.path(), "s.bin", &vec![7u8; 500_000]);
    let ses = s.session();
    let stop = AtomicBool::new(false);
    let e = ses
        .put(&f, "/up/s.bin", Options::default(), &stop, &mut |_, done, _| {
            if done > 100_000 {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .unwrap_err();
    assert_eq!(e, Error::Stopped);
    assert!(!s.up("s.bin").exists() && !s.up("s.bin.part").exists());
}

#[test]
fn only_the_pinned_host_key_is_trusted() {
    let s = start();
    let seen = fingerprint("127.0.0.1", s.port, Duration::from_secs(10)).unwrap();
    assert_eq!(seen, s.fingerprint);
    assert!(seen.starts_with("SHA256:"));

    let mut t = s.target();
    t.host_key = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into();
    match Session::connect(&t) {
        Err(Error::HostKeyMismatch { actual, .. }) => assert_eq!(actual, s.fingerprint),
        other => panic!("{:?}", other.err()),
    }

    t.host_key = seen.trim_end_matches('=').to_string();
    Session::connect(&t).expect("the right key, any spelling of the padding");
}

#[test]
fn a_wrong_password_is_refused_and_a_key_file_logs_in() {
    let s = start();
    let mut t = s.target();
    t.auth = Auth::Password("nope".into());
    assert!(matches!(Session::connect(&t), Err(Error::AuthRefused)));

    t.auth = Auth::Key { path: s.key_file.clone(), passphrase: None };
    let ses = Session::connect(&t).expect("key login");
    assert!(ses.list("/up").unwrap().is_empty());

    t.auth = Auth::Key { path: s.key_file.with_file_name("missing"), passphrase: None };
    assert!(matches!(Session::connect(&t), Err(Error::Key(_))));
}

#[test]
fn nothing_listening_is_a_connect_error() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut t = Target::new("127.0.0.1", USER, Auth::Password("x".into()), "SHA256:x");
    t.port = port;
    t.connect_timeout = Duration::from_secs(3);
    assert!(matches!(Session::connect(&t), Err(Error::Connect(_))));
}
