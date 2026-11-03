use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ito_testserver::winrm::{Mode, Output, WinrmServer};
use ito_winrm::{exec, Error, ExecOptions, Stream, Target};

fn target(server: &WinrmServer, user: &str, password: &str) -> Target {
    let mut t = match server.mode() {
        Mode::Http => Target::http("127.0.0.1", user, password),
        Mode::Https => Target::https("127.0.0.1", user, password, server.fingerprint()),
    };
    t.port = server.port();
    t.connect_timeout = Duration::from_secs(5);
    t
}

fn run(server: &WinrmServer, command: &str, opts: &ExecOptions) -> (Result<Option<u32>, Error>, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let stop = AtomicBool::new(false);
    let r = exec(&target(server, "admin", "pw"), command, opts, &stop, &mut |s, b| match s {
        Stream::Stdout => out.extend_from_slice(b),
        Stream::Stderr => err.extend_from_slice(b),
    });
    (r.map(|e| e.status), String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned())
}

#[test]
fn output_and_the_exit_code_come_back_over_http_and_https() {
    for mode in [Mode::Http, Mode::Https] {
        let server = WinrmServer::start_with_login(mode, "admin", "pw").unwrap();
        server.on_command("ver", Output::new().stdout("Microsoft Windows\r\n").stderr("warn\r\n").stdout("more\r\n").exit(3));
        let (r, out, err) = run(&server, "ver & echo x", &ExecOptions::default());
        assert_eq!(r, Ok(Some(3)), "{mode:?}");
        assert_eq!(out, "Microsoft Windows\r\nmore\r\n");
        assert_eq!(err, "warn\r\n");
        assert_eq!(server.commands(), vec!["ver & echo x".to_owned()]);
        let actions = server.actions();
        assert_eq!(actions.first().map(String::as_str), Some("Create"));
        assert_eq!(actions[1], "Command");
        assert!(actions.contains(&"Receive".to_owned()));
        assert_eq!(actions.last().map(String::as_str), Some("Delete"));
        assert_eq!(server.shells_deleted(), 1);
        assert_eq!(server.shells_open(), 0);
        assert!(!server.signalled());
        assert!(!server.log().iter().any(|l| l.contains("pw")));
    }
}

#[test]
fn an_unscripted_command_fails_with_exit_1() {
    let server = WinrmServer::start(Mode::Http).unwrap();
    let (r, out, err) = run(&server, "nope", &ExecOptions::default());
    assert_eq!(r, Ok(Some(1)));
    assert!(out.is_empty());
    assert!(err.contains("'nope' is not recognized"), "{err}");
}

#[test]
fn environment_goes_in_the_create_request() {
    let server = WinrmServer::start(Mode::Http).unwrap();
    server.on_command("echo", Output::new().stdout("ok"));
    let opts = ExecOptions { env: vec![("GREETING".into(), "a<b".into())], timeout: None };
    assert_eq!(run(&server, "echo %GREETING%", &opts).0, Ok(Some(0)));
    let create = &server.requests()[0];
    assert!(create.body.contains("Name=\"GREETING\">a&lt;b<"), "{}", create.body);
}

#[test]
fn a_wrong_password_is_refused() {
    let server = WinrmServer::start_with_login(Mode::Http, "admin", "pw").unwrap();
    let stop = AtomicBool::new(false);
    let r = exec(&target(&server, "admin", "bad"), "ver", &ExecOptions::default(), &stop, &mut |_, _| {});
    assert_eq!(r.err(), Some(Error::AuthRefused));
    assert!(server.log().iter().any(|l| l == "! 401"));
    assert!(server.commands().is_empty());
}

#[test]
fn a_wrong_pin_is_refused() {
    let server = WinrmServer::start(Mode::Https).unwrap();
    let mut t = target(&server, "admin", "pw");
    t.pin = Some([7u8; 32]);
    let stop = AtomicBool::new(false);
    let r = exec(&t, "ver", &ExecOptions::default(), &stop, &mut |_, _| {});
    assert!(matches!(r, Err(Error::Fingerprint(_) | Error::Tls(_))), "{r:?}");
    assert!(server.commands().is_empty());
}

#[test]
fn a_slow_command_waits_through_the_nothing_yet_fault() {
    let server = WinrmServer::start(Mode::Http).unwrap();
    server.on_command("slow", Output::new().stdout("done").delay(Duration::from_millis(3500)).exit(5));
    let started = Instant::now();
    let (r, out, _) = run(&server, "slow", &ExecOptions::default());
    assert_eq!(r, Ok(Some(5)));
    assert_eq!(out, "done");
    assert!(started.elapsed() >= Duration::from_millis(3400));
    // More than one Receive: the first waits out its 3 s.
    assert!(server.actions().iter().filter(|a| *a == "Receive").count() >= 2);
}

#[test]
fn a_timeout_signals_the_command_and_deletes_the_shell() {
    let server = WinrmServer::start(Mode::Http).unwrap();
    server.on_command("forever", Output::new().stdout("started").hang());
    let opts = ExecOptions { env: vec![], timeout: Some(Duration::from_millis(500)) };
    let (r, out, _) = run(&server, "forever", &opts);
    assert_eq!(r, Err(Error::Timeout));
    assert_eq!(out, "started");
    assert!(server.signalled());
    let signal = server.requests().into_iter().find(|r| r.action == "Signal").unwrap();
    assert!(signal.detail.unwrap().ends_with("/signal/terminate"));
    assert!(server.wait_for_action("Delete", Duration::from_secs(5)));
    assert_eq!(server.shells_open(), 0);
}

#[test]
fn the_stop_flag_ends_a_running_command() {
    let server = WinrmServer::start(Mode::Http).unwrap();
    server.on_command("forever", Output::new().hang());
    let stop = AtomicBool::new(false);
    let t = target(&server, "admin", "pw");
    let r = std::thread::scope(|s| {
        let h = s.spawn(|| exec(&t, "forever", &ExecOptions::default(), &stop, &mut |_, _| {}));
        assert!(server.wait_for_action("Receive", Duration::from_secs(5)));
        stop.store(true, Ordering::Relaxed);
        h.join().unwrap()
    });
    assert_eq!(r.err(), Some(Error::Stopped));
    assert!(server.signalled());
    assert!(server.wait_for_action("Delete", Duration::from_secs(5)));
}
