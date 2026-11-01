//! A mock WinRM endpoint (plain and TLS) that answers the cmd-shell messages, to run `exec` against.

use ito_winrm::{exec, fingerprint, peek_fingerprint, Error, ExecOptions, Stream, Target};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RESP: &str = "<s:Envelope xmlns:s=\"x\" xmlns:rsp=\"y\"><s:Body>";
const END: &str = "</s:Body></s:Envelope>";

/// How the mock behaves.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Two polls of output (one empty wait first), then done with exit code 7.
    Normal,
    /// Never finishes: every Receive is an empty wait.
    Hang,
    /// Refuses the credentials.
    Deny,
}

#[derive(Default)]
struct Seen {
    actions: Vec<String>,
    auth: Vec<String>,
}

fn serve<S: Read + Write>(mut s: S, mode: Mode, seen: Arc<Mutex<Seen>>) {
    let mut receives = 0;
    loop {
        let mut buf = vec![];
        let mut b = [0u8; 1];
        while !buf.ends_with(b"\r\n\r\n") {
            if s.read(&mut b).unwrap_or(0) == 0 {
                return;
            }
            buf.push(b[0]);
        }
        let head = String::from_utf8_lossy(&buf).into_owned();
        let len: usize = head
            .lines()
            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse().ok()))
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        if s.read_exact(&mut body).is_err() {
            return;
        }
        let body = String::from_utf8_lossy(&body).into_owned();
        let auth = head.lines().find_map(|l| l.strip_prefix("Authorization: ")).unwrap_or("").to_owned();
        let action =
            body.split("<a:Action").nth(1).and_then(|r| r.split('>').nth(1)).and_then(|r| r.split('<').next()).unwrap_or("").to_owned();
        {
            let mut g = seen.lock().unwrap();
            g.auth.push(auth);
            g.actions.push(action.rsplit('/').next().unwrap_or("").to_owned());
        }
        let (status, text, chunked) = if mode == Mode::Deny {
            (401, String::new(), false)
        } else if action.ends_with("/Create") {
            (200, format!("{RESP}<rsp:Shell><rsp:ShellId>SHELL-1</rsp:ShellId></rsp:Shell>{END}"), false)
        } else if action.ends_with("/Command") {
            (200, format!("{RESP}<rsp:CommandResponse><rsp:CommandId>CMD-1</rsp:CommandId></rsp:CommandResponse>{END}"), false)
        } else if action.ends_with("/Receive") {
            receives += 1;
            if mode == Mode::Hang || receives == 1 {
                std::thread::sleep(Duration::from_millis(100));
                (500, "<s:Envelope><s:Body><s:Fault><s:Detail><f:WSManFault Code=\"2150858793\"/></s:Detail><s:Reason><s:Text>t</s:Text></s:Reason></s:Fault></s:Body></s:Envelope>".to_owned(), false)
            } else if receives == 2 {
                (200, format!("{RESP}<rsp:ReceiveResponse><rsp:Stream Name=\"stdout\" CommandId=\"CMD-1\">aGVsbG8=</rsp:Stream></rsp:ReceiveResponse>{END}"), true)
            } else {
                (200, format!(
                    "{RESP}<rsp:ReceiveResponse><rsp:Stream Name=\"stderr\" CommandId=\"CMD-1\">b29wcw==</rsp:Stream>\
<rsp:CommandState CommandId=\"CMD-1\" State=\"http://schemas.microsoft.com/wbem/wsman/1/windows/shell/CommandState/Done\"><rsp:ExitCode>7</rsp:ExitCode></rsp:CommandState></rsp:ReceiveResponse>{END}"), false)
            }
        } else {
            (200, format!("{RESP}{END}"), false)
        };
        let reply = if chunked {
            let (a, z) = text.split_at(text.len() / 2);
            format!("HTTP/1.1 {status} X\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{z}\r\n0\r\n\r\n", a.len(), z.len())
        } else {
            format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{text}", text.len())
        };
        if s.write_all(reply.as_bytes()).and_then(|()| s.flush()).is_err() {
            return;
        }
    }
}

fn plain(mode: Mode) -> (u16, Arc<Mutex<Seen>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let s = s2.clone();
            std::thread::spawn(move || serve(c, mode, s));
        }
    });
    (port, seen)
}

fn tls(mode: Mode) -> (u16, [u8; 32], Arc<Mutex<Seen>>) {
    let key = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let der = key.cert.der().to_vec();
    let fp = fingerprint::sha256(&der);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![key.cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::try_from(key.signing_key.serialize_der()).unwrap(),
        )
        .unwrap();
    let cfg = Arc::new(cfg);
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let (cfg, s) = (cfg.clone(), s2.clone());
            std::thread::spawn(move || {
                let conn = rustls::ServerConnection::new(cfg).unwrap();
                serve(rustls::StreamOwned::new(conn, c), mode, s);
            });
        }
    });
    (port, fp, seen)
}

fn run(t: &Target, opts: &ExecOptions, stop: &AtomicBool) -> (Result<ito_winrm::Exit, Error>, String, String) {
    let (mut out, mut err) = (vec![], vec![]);
    let r = exec(t, "echo hello", opts, stop, &mut |s, b| match s {
        Stream::Stdout => out.extend_from_slice(b),
        Stream::Stderr => err.extend_from_slice(b),
    });
    (r, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

fn local(port: u16, https: bool, pin: Option<[u8; 32]>) -> Target {
    let mut t = Target::https("127.0.0.1", "me", "pw", pin);
    t.port = port;
    t.https = https;
    t.connect_timeout = Duration::from_secs(5);
    t
}

#[test]
fn a_command_streams_its_output_and_ends_with_its_code() {
    let (port, seen) = plain(Mode::Normal);
    let (r, out, err) = run(&local(port, false, None), &ExecOptions::default(), &AtomicBool::new(false));
    assert_eq!(r.unwrap().status, Some(7));
    assert_eq!((out.as_str(), err.as_str()), ("hello", "oops"));
    let g = seen.lock().unwrap();
    assert_eq!(g.actions, ["Create", "Command", "Receive", "Receive", "Receive", "Delete"]);
    // Basic "me:pw"
    assert!(g.auth.iter().all(|a| a == "Basic bWU6cHc="));
}

#[test]
fn a_refused_password_is_auth_refused() {
    let (port, _) = plain(Mode::Deny);
    let (r, ..) = run(&local(port, false, None), &ExecOptions::default(), &AtomicBool::new(false));
    assert_eq!(r.unwrap_err(), Error::AuthRefused);
}

#[test]
fn the_stop_flag_ends_a_hanging_command_and_tidies_up() {
    let (port, seen) = plain(Mode::Hang);
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        s2.store(true, Ordering::Relaxed);
    });
    let (r, ..) = run(&local(port, false, None), &ExecOptions::default(), &stop);
    assert_eq!(r.unwrap_err(), Error::Stopped);
    let g = seen.lock().unwrap();
    assert!(g.actions.ends_with(&["Signal".to_owned(), "Delete".to_owned()]), "{:?}", g.actions);
}

#[test]
fn the_timeout_ends_a_hanging_command() {
    let (port, seen) = plain(Mode::Hang);
    let opts = ExecOptions { timeout: Some(Duration::from_millis(500)), ..Default::default() };
    let (r, ..) = run(&local(port, false, None), &opts, &AtomicBool::new(false));
    assert_eq!(r.unwrap_err(), Error::Timeout);
    assert!(seen.lock().unwrap().actions.contains(&"Signal".to_owned()));
}

#[test]
fn https_runs_with_the_pinned_fingerprint() {
    let (port, fp, _) = tls(Mode::Normal);
    let (r, out, _) = run(&local(port, true, Some(fp)), &ExecOptions::default(), &AtomicBool::new(false));
    assert_eq!(r.unwrap().status, Some(7));
    assert_eq!(out, "hello");
}

#[test]
fn another_certificate_is_refused_and_its_fingerprint_is_reported() {
    let (port, fp, _) = tls(Mode::Normal);
    let mut wrong = fp;
    wrong[0] ^= 1;
    let (r, ..) = run(&local(port, true, Some(wrong)), &ExecOptions::default(), &AtomicBool::new(false));
    assert_eq!(r.unwrap_err(), Error::Fingerprint(fingerprint::format(&fp)));
}

#[test]
fn peek_shows_the_certificate_fingerprint() {
    let (port, fp, _) = tls(Mode::Normal);
    let got = peek_fingerprint("127.0.0.1", port, Duration::from_secs(5), &AtomicBool::new(false)).unwrap();
    assert_eq!(got, fp);
    assert_eq!(fingerprint::parse(&fingerprint::format(&got)), Some(fp));
}

#[test]
fn nothing_listening_is_a_connect_error() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (r, ..) = run(&local(port, false, None), &ExecOptions::default(), &AtomicBool::new(false));
    assert!(matches!(r, Err(Error::Connect(_) | Error::Timeout)), "{r:?}");
    let _ = TcpStream::connect("127.0.0.1:1");
}
