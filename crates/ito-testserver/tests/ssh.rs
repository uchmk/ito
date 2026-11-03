use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ito_sftp::{exec, fingerprint, Auth, Error, ExecOptions, Options, Session, Stream, Target, Verify};
use ito_testserver::ssh::{Config, KeyPair, Output, SshServer};

fn target(server: &SshServer, user: &str, auth: Auth) -> Target {
    let mut t = Target::new("127.0.0.1", user, auth, server.fingerprint());
    t.port = server.port();
    t.connect_timeout = Duration::from_secs(5);
    t.io_timeout = Duration::from_secs(5);
    t
}

fn pw(server: &SshServer) -> Target {
    target(server, "ito", Auth::Password("secret".into()))
}

fn config() -> Config {
    Config { login: Some(("ito".into(), "secret".into())), ..Default::default() }
}

fn run(server: &SshServer, command: &str, opts: &ExecOptions) -> (Result<ito_sftp::Exit, Error>, Vec<u8>, Vec<u8>) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let stop = AtomicBool::new(false);
    let r = exec(&pw(server), command, opts, &stop, &mut |s, b| match s {
        Stream::Stdout => out.extend_from_slice(b),
        Stream::Stderr => err.extend_from_slice(b),
    });
    (r, out, err)
}

fn put(session: &Session, local: &std::path::Path, remote: &str, verify: Verify) -> Result<Vec<ito_sftp::Moved>, Error> {
    session.put(local, remote, Options { overwrite: true, verify }, &AtomicBool::new(false), &mut |_, _, _| {})
}

#[test]
fn the_pinned_fingerprint_is_the_one_the_server_reports() {
    let server = SshServer::start().unwrap();
    assert!(server.fingerprint().starts_with("SHA256:"));
    assert_eq!(fingerprint("127.0.0.1", server.port(), Duration::from_secs(5)).unwrap(), server.fingerprint());
    let host = KeyPair::generate();
    let given = SshServer::start_with(Config { host_key: Some(host.clone()), ..Default::default() }).unwrap();
    assert_eq!(given.fingerprint(), host.fingerprint());
}

#[test]
fn a_wrong_fingerprint_is_refused() {
    let server = SshServer::start_with(config()).unwrap();
    let mut t = pw(&server);
    t.host_key = KeyPair::generate().fingerprint();
    assert!(matches!(Session::connect(&t), Err(Error::HostKeyMismatch { .. })));
    assert!(server.logins().is_empty());
}

#[test]
fn password_login_works_and_a_wrong_password_is_refused() {
    let server = SshServer::start_with(config()).unwrap();
    Session::connect(&pw(&server)).unwrap();
    let bad = target(&server, "ito", Auth::Password("nope".into()));
    assert!(matches!(Session::connect(&bad), Err(Error::AuthRefused)));
    let logins = server.logins();
    assert_eq!(logins.len(), 2);
    assert!(logins[0].accepted && logins[0].method == "password" && logins[0].user == "ito");
    assert!(!logins[1].accepted);
    assert!(!server.log().iter().any(|l| l.contains("secret") || l.contains("nope")));
}

#[test]
fn public_key_login_needs_an_allowed_key() {
    let good = KeyPair::generate();
    let other = KeyPair::generate();
    let server = SshServer::start_with(Config { allowed_keys: vec![good.public_openssh.clone()], ..Default::default() }).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, k: &KeyPair| {
        let p = dir.path().join(name);
        std::fs::write(&p, &k.private_openssh).unwrap();
        p
    };
    let key = |p| Auth::Key { path: p, passphrase: None };
    Session::connect(&target(&server, "ito", key(write("good", &good)))).unwrap();
    let r = Session::connect(&target(&server, "ito", key(write("other", &other))));
    assert!(matches!(r, Err(Error::AuthRefused)), "{:?}", r.err());
    let logins = server.logins();
    assert!(logins.iter().any(|l| l.method == "publickey" && l.accepted && l.key == good.fingerprint()));
    assert!(logins.iter().any(|l| l.method == "publickey" && !l.accepted));
}

#[test]
fn reject_all_refuses_every_login() {
    let server = SshServer::start_with(Config { reject_all: true, ..Default::default() }).unwrap();
    assert!(matches!(Session::connect(&pw(&server)), Err(Error::AuthRefused)));
    assert!(server.logins().iter().all(|l| !l.accepted));
}

#[test]
fn exec_returns_output_chunks_and_the_exit_code() {
    let server = SshServer::start_with(config()).unwrap();
    let sjis = [0x83, 0x65, 0x83, 0x58, 0x83, 0x67, b'\n'];
    server.on_command("build", Output::new().stdout("one\n").stderr("warn\n").stdout(sjis).exit(3));
    let (r, out, err) = run(&server, "build --all", &ExecOptions::default());
    let exit = r.unwrap();
    assert_eq!(exit.status, Some(3));
    assert_eq!(exit.signal, None);
    assert_eq!(&out[..4], b"one\n");
    assert_eq!(&out[4..], &sjis);
    assert_eq!(err, b"warn\n");
    assert_eq!(server.commands(), vec!["build --all".to_owned()]);
}

#[test]
fn an_unscripted_command_is_not_found() {
    let server = SshServer::start_with(config()).unwrap();
    let (r, _, err) = run(&server, "nope", &ExecOptions::default());
    assert_eq!(r.unwrap().status, Some(127));
    assert!(String::from_utf8_lossy(&err).contains("command not found"));
}

#[test]
fn a_command_can_end_by_a_signal() {
    let server = SshServer::start_with(config()).unwrap();
    server.on_command("crash", Output::new().stdout("x").signal("KILL"));
    let (r, out, _) = run(&server, "crash", &ExecOptions::default());
    let exit = r.unwrap();
    assert_eq!(exit.status, None);
    assert!(exit.signal.as_deref().is_some_and(|s| s.starts_with("KILL")), "{exit:?}");
    assert_eq!(out, b"x");
}

#[test]
fn env_requests_are_recorded_and_can_be_refused() {
    let server = SshServer::start_with(config()).unwrap();
    server.on_command("env", Output::new().exit(0));
    let opts = ExecOptions { env: vec![("LANG".into(), "C".into())], timeout: None };
    let (r, _, _) = run(&server, "env", &opts);
    assert!(r.unwrap().env_refused.is_empty());
    server.set_accept_env(false);
    let (r, _, _) = run(&server, "env", &opts);
    assert_eq!(r.unwrap().env_refused, vec!["LANG".to_owned()]);
    assert_eq!(server.envs(), vec![("LANG".to_owned(), "C".to_owned()); 2]);
}

#[test]
fn a_timeout_sends_term_and_closes_the_channel() {
    let server = SshServer::start_with(config()).unwrap();
    server.on_command(
        "sleep",
        Output::new()
            .stdout(
                "started
",
            )
            .hang(),
    );
    // A Session keeps the connection open, so the TERM and the close reach the wire; the one-shot `exec` drops its
    // runtime right after queuing them, and the server may never see them.
    let session = Session::connect(&pw(&server)).unwrap();
    let opts = ExecOptions { env: vec![], timeout: Some(Duration::from_millis(300)) };
    let started = Instant::now();
    let mut out = Vec::new();
    let r = session.exec("sleep 99", &opts, &AtomicBool::new(false), &mut |_, b| out.extend_from_slice(b));
    assert_eq!(r.unwrap_err(), Error::Timeout);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        out,
        b"started
"
    );
    assert!(server.wait_until(Duration::from_secs(5), |s| s.signals().contains(&"TERM".to_owned()) && s.channels_closed() >= 1));
}

#[test]
fn the_stop_flag_ends_a_hanging_command() {
    let server = SshServer::start_with(config()).unwrap();
    server.on_command("sleep", Output::new().hang());
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let session = Session::connect(&pw(&server)).unwrap();
    let worker = std::thread::spawn(move || {
        let r = session.exec("sleep 99", &ExecOptions::default(), &flag, &mut |_, _| {});
        (r, session)
    });
    assert!(server.wait_until(Duration::from_secs(5), |s| !s.commands().is_empty()));
    stop.store(true, Ordering::Relaxed);
    let (r, _session) = worker.join().unwrap();
    assert_eq!(r.unwrap_err(), Error::Stopped);
    assert!(server.wait_until(Duration::from_secs(5), |s| s.signals().contains(&"TERM".to_owned()) && s.channels_closed() >= 1));
}

#[test]
fn a_delayed_command_still_finishes() {
    let server = SshServer::start_with(config()).unwrap();
    server
        .on_command("slow", Output::new().delay(Duration::from_millis(200)).stdout("a").chunk_delay(Duration::from_millis(50)).stdout("b"));
    let (r, out, _) = run(&server, "slow", &ExecOptions::default());
    assert!(r.unwrap().success());
    assert_eq!(out, b"ab");
}

#[test]
fn sftp_put_get_list_and_the_part_rename() {
    let server = SshServer::start_with(config()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.txt");
    std::fs::write(&src, b"hello sftp").unwrap();
    std::fs::create_dir(server.local("/up")).unwrap();
    let s = Session::connect(&pw(&server)).unwrap();
    let moved = put(&s, &src, "/up/a.txt", Verify::None).unwrap();
    assert_eq!(moved[0].bytes, 10);
    assert_eq!(std::fs::read(server.local("/up/a.txt")).unwrap(), b"hello sftp");
    let names: Vec<_> = s.list("/up").unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, vec!["a.txt".to_owned()]);
    let back = dir.path().join("b.txt");
    s.get("/up/a.txt", &back, Options::default(), &AtomicBool::new(false), &mut |_, _, _| {}).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"hello sftp");
    // The transfer went to a `.part` first and was renamed over the final name.
    let log = server.log();
    let write = log.iter().position(|l| l.starts_with("sftp write /up/a.txt.part")).expect("a .part write");
    let rename = log.iter().position(|l| l.starts_with("sftp rename /up/a.txt.part")).expect("a rename");
    assert!(write < rename);
    s.remove("/up/a.txt").unwrap();
    assert!(!server.local("/up/a.txt").exists());
}

#[test]
fn a_denied_path_answers_permission_denied() {
    let server = SshServer::start_with(config()).unwrap();
    std::fs::create_dir(server.local("/locked")).unwrap();
    server.deny_write("/locked");
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.txt");
    std::fs::write(&src, b"x").unwrap();
    let s = Session::connect(&pw(&server)).unwrap();
    let r = put(&s, &src, "/locked/a.txt", Verify::None);
    assert!(matches!(r, Err(Error::PermissionDenied(_))), "{r:?}");
    assert!(s.list("/locked").unwrap().is_empty());
    server.allow_write();
    put(&s, &src, "/locked/a.txt", Verify::None).unwrap();
}

#[test]
fn verification_by_exec_and_by_readback_catches_damage() {
    let server = SshServer::start_with(config()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.bin");
    std::fs::write(&src, b"some bytes to hash").unwrap();
    let s = Session::connect(&pw(&server)).unwrap();
    for verify in [Verify::Exec, Verify::Readback] {
        let moved = put(&s, &src, "/ok.bin", verify).unwrap();
        assert!(moved[0].sha256.is_some(), "{verify:?}");
    }
    assert!(server.commands().iter().any(|c| c.contains("sha256sum")));
    // A link that flips a byte as it writes: the hash far away differs, and the `.part` is deleted.
    server.set_damage(true, false);
    for verify in [Verify::Exec, Verify::Readback] {
        let r = put(&s, &src, "/bad.bin", verify);
        assert!(matches!(r, Err(Error::Mismatch { .. })), "{verify:?}: {r:?}");
        assert!(!server.local("/bad.bin.part").exists());
    }
}

#[test]
fn a_silent_server_times_out() {
    let server = SshServer::start_with(Config { silent: true, ..Default::default() }).unwrap();
    let mut t = pw(&server);
    t.connect_timeout = Duration::from_millis(300);
    let started = Instant::now();
    assert!(matches!(Session::connect(&t), Err(Error::Connect(_))));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn the_one_shot_exec_delivers_term_before_it_returns() {
    for _ in 0..12 {
        let server = SshServer::start_with(config()).unwrap();
        server.on_command("sleep", Output::new().hang());
        let opts = ExecOptions { env: vec![], timeout: Some(Duration::from_millis(200)) };
        let r = exec(&pw(&server), "sleep 99", &opts, &AtomicBool::new(false), &mut |_, _| {});
        assert_eq!(r.unwrap_err(), Error::Timeout);
        assert!(server.wait_until(Duration::from_secs(5), |s| s.signals().contains(&"TERM".to_owned()) && s.channels_closed() >= 1));
    }
}
