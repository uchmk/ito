//! A small HTTP listener an app runs on its own PC so another system can start something or read a state
//! (an inbound webhook, a local REST hook for scripts).
//!
//! - **Loopback only.** It binds `127.0.0.1` (and `::1` when asked); there is no setting for anything else.
//! - **A bearer token on every request**, compared in constant time; a missing or wrong one is a 401 before the body
//!   is read and before the handler is called. The token is never logged ([`token_from_secret`] reads it from
//!   `ito-secret`).
//! - **Limits:** the head, the body (`max_body`, 413 before it is read), connections at once (`max_connections`, 503),
//!   and a read timeout per connection.
//! - JSON in and out, HTTP/1.1, one request per connection (`Connection: close`), no TLS (it never leaves the PC),
//!   no chunked bodies.
//! - Each request is reported to a log callback as method, path (no query, which may carry a secret), status and
//!   milliseconds.
//!
//! It runs on its own threads; the handler is called on a connection thread, so it must be `Send + Sync` and quick
//! to answer (or hand the work to the app and answer "accepted").

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MAX_HEAD: usize = 16 * 1024;

/// How the listener is set up. [`Config::new`] gives the defaults.
#[derive(Clone)]
pub struct Config {
    /// Port to listen on; `0` lets the system pick (see [`Server::port`]).
    pub port: u16,
    /// Also listen on `::1` (same port). A machine without IPv6 loopback just goes without.
    pub ipv6: bool,
    /// The bearer token every request must send. Must not be empty.
    pub token: String,
    /// Largest body accepted, in bytes (default 64 KiB).
    pub max_body: usize,
    /// Most connections served at once (default 8); one more gets a 503.
    pub max_connections: usize,
    /// Longest wait for the client to send, or to take, a piece of the request (default 10 s).
    pub timeout: Duration,
}

impl Config {
    pub fn new(port: u16, token: impl Into<String>) -> Self {
        Config { port, ipv6: false, token: token.into(), max_body: 64 * 1024, max_connections: 8, timeout: Duration::from_secs(10) }
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config").field("port", &self.port).field("ipv6", &self.ipv6).field("token", &"***").finish_non_exhaustive()
    }
}

/// The token kept as `name` for `app` in the OS credential store. `Err` when the store fails or there is none
/// (the message says which); blocks, so call it off the UI thread.
pub fn token_from_secret(app: &str, name: &str) -> io::Result<String> {
    match ito_secret::get(app, name)? {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(io::Error::new(io::ErrorKind::NotFound, format!("no secret `{name}` for {app}"))),
    }
}

/// A request, after the token was checked.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// The path, percent-decoded (UTF-8), without the query.
    pub path: String,
    /// The query as decoded pairs (`+` is a space).
    pub query: Vec<(String, String)>,
    /// Header names in lower case, in the order sent.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// The first value of query key `key`.
    pub fn query(&self, key: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// The body as JSON; `None` when there is none, `Err` (for a 400) when it does not parse.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        if self.body.iter().all(u8::is_ascii_whitespace) {
            return Ok(None);
        }
        serde_json::from_slice(&self.body).map(Some)
    }
}

/// An answer: a status and a JSON value.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub json: serde_json::Value,
}

impl Response {
    pub fn new(status: u16, json: serde_json::Value) -> Self {
        Response { status, json }
    }

    pub fn ok(json: serde_json::Value) -> Self {
        Self::new(200, json)
    }

    /// `{"error": message}` with `status`.
    pub fn error(status: u16, message: &str) -> Self {
        Self::new(status, serde_json::json!({ "error": message }))
    }
}

/// One served request, for the app's log. Nothing secret is in it.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub millis: u64,
}

type Handler = dyn Fn(Request) -> Response + Send + Sync;
type Log = dyn Fn(&LogLine) + Send + Sync;

struct Shared {
    cfg: Config,
    handler: Box<Handler>,
    log: Box<Log>,
    active: AtomicUsize,
}

/// A running listener. Dropping it (or [`Server::stop`]) stops accepting; requests already being served finish.
pub struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Server {
    /// Start listening. `Err` for an empty token or when the port cannot be taken.
    pub fn start(
        cfg: Config,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
        log: impl Fn(&LogLine) + Send + Sync + 'static,
    ) -> io::Result<Server> {
        if cfg.token.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the token is empty"));
        }
        let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, cfg.port))?;
        let port = v4.local_addr()?.port();
        let mut listeners = vec![v4];
        if cfg.ipv6 {
            if let Ok(l) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
                listeners.push(l);
            }
        }
        let shared = Arc::new(Shared { cfg, handler: Box::new(handler), log: Box::new(log), active: AtomicUsize::new(0) });
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        for l in listeners {
            l.set_nonblocking(true)?;
            let (shared, stop) = (shared.clone(), stop.clone());
            threads.push(std::thread::spawn(move || accept_loop(l, shared, stop)));
        }
        Ok(Server { port, stop, threads })
    }

    /// The port in use (what the system picked when `Config::port` was 0).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stop accepting and wait for the listener threads.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(l: TcpListener, shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match l.accept() {
            Ok((s, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                if shared.active.fetch_add(1, Ordering::AcqRel) >= shared.cfg.max_connections {
                    shared.active.fetch_sub(1, Ordering::AcqRel);
                    let _ = respond(s, &Response::error(503, "too many connections"), shared.cfg.timeout);
                    continue;
                }
                let shared = shared.clone();
                std::thread::spawn(move || {
                    serve(s, &shared);
                    shared.active.fetch_sub(1, Ordering::AcqRel);
                });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn serve(mut s: TcpStream, shared: &Shared) {
    let start = Instant::now();
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(shared.cfg.timeout));
    let (method, path, resp) = match read_request(&mut s, shared) {
        Ok((req, token_ok)) => {
            let (m, p) = (req.method.clone(), req.path.clone());
            if !token_ok {
                (m, p, Response::error(401, "unauthorized"))
            } else {
                let r = catch_unwind(AssertUnwindSafe(|| (shared.handler)(req)));
                (m, p, r.unwrap_or_else(|_| Response::error(500, "internal error")))
            }
        }
        Err(Refused { method, path, status, message }) => (method, path, Response::error(status, message)),
    };
    let _ = respond(s, &resp, shared.cfg.timeout);
    (shared.log)(&LogLine { method, path, status: resp.status, millis: start.elapsed().as_millis() as u64 });
}

struct Refused {
    method: String,
    path: String,
    status: u16,
    message: &'static str,
}

fn refuse(status: u16, message: &'static str) -> Refused {
    Refused { method: String::new(), path: String::new(), status, message }
}

/// Read and check one request. `Ok((request, token_ok))`: when the token is wrong the body was not read.
fn read_request(s: &mut TcpStream, shared: &Shared) -> Result<(Request, bool), Refused> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(refuse(431, "request head too large"));
        }
        match s.read(&mut chunk) {
            Ok(0) => return Err(refuse(400, "closed before the request was complete")),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => return Err(refuse(408, "timed out")),
        }
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| refuse(400, "head is not UTF-8"))?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target, version) = (first.next().unwrap_or(""), first.next().unwrap_or(""), first.next().unwrap_or(""));
    if method.is_empty() || target.is_empty() || !version.starts_with("HTTP/1.") || first.next().is_some() {
        return Err(refuse(400, "bad request line"));
    }
    let (raw_path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let path = percent_decode(raw_path, false).ok_or_else(|| refuse(400, "bad path"))?;
    let named = |status, message| Refused { method: method.to_string(), path: path.clone(), status, message };
    let mut headers = Vec::new();
    for line in lines {
        let (k, v) = line.split_once(':').ok_or_else(|| named(400, "bad header"))?;
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    let token_ok = header("authorization")
        .and_then(|v| v.split_once(' '))
        .is_some_and(|(scheme, t)| scheme.eq_ignore_ascii_case("bearer") && ct_eq(t.trim().as_bytes(), shared.cfg.token.as_bytes()));
    let mut req = Request {
        method: method.to_string(),
        path: path.clone(),
        query: raw_query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (percent_decode(k, true).unwrap_or_default(), percent_decode(v, true).unwrap_or_default())
            })
            .collect(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    if !token_ok {
        return Ok((req, false));
    }
    if header("transfer-encoding").is_some() {
        return Err(named(501, "chunked bodies are not supported"));
    }
    let len = match header("content-length") {
        None => 0,
        Some(v) => v.parse::<usize>().map_err(|_| named(400, "bad Content-Length"))?,
    };
    if len > shared.cfg.max_body {
        return Err(named(413, "body too large"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    if body.len() > len {
        return Err(named(400, "more data than Content-Length"));
    }
    while body.len() < len {
        match s.read(&mut chunk) {
            Ok(0) => return Err(named(400, "closed before the body was complete")),
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(_) => return Err(named(408, "timed out")),
        }
        if body.len() > len {
            return Err(named(400, "more data than Content-Length"));
        }
    }
    req.headers = headers;
    req.body = body;
    Ok((req, true))
}

fn respond(mut s: TcpStream, r: &Response, timeout: Duration) -> io::Result<()> {
    let _ = s.set_write_timeout(Some(timeout));
    let body = serde_json::to_vec(&r.json).unwrap_or_else(|_| b"null".to_vec());
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        r.status,
        reason(r.status),
        body.len()
    );
    s.write_all(head.as_bytes())?;
    s.write_all(&body)?;
    s.flush()?;
    let _ = s.shutdown(std::net::Shutdown::Write);
    Ok(())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Equal bytes, in time that depends only on the longer length.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0)) as usize;
    }
    diff == 0
}

/// `%XX` decoded; `plus` also turns `+` into a space (a query). `None` when it is not valid UTF-8 or a `%` is cut.
fn percent_decode(s: &str, plus: bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
                continue;
            }
            b'+' if plus => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const TOKEN: &str = "s3cret-token";

    fn start(f: impl Fn(Request) -> Response + Send + Sync + 'static) -> (Server, Arc<Mutex<Vec<LogLine>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let l = lines.clone();
        let mut cfg = Config::new(0, TOKEN);
        cfg.max_body = 100;
        cfg.timeout = Duration::from_millis(800);
        (Server::start(cfg, f, move |x| l.lock().unwrap().push(x.clone())).unwrap(), lines)
    }

    /// Send `raw`, return (status, body json).
    fn call(port: u16, raw: &str) -> (u16, serde_json::Value) {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let status = out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let body = out.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        (status, serde_json::from_str(body).unwrap_or(serde_json::Value::Null))
    }

    fn req(method: &str, path: &str, token: Option<&str>, body: &str) -> String {
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Length: {}\r\n\r\n{body}", body.len())
    }

    #[test]
    fn a_good_request_reaches_the_handler_decoded() {
        let (srv, log) = start(|r| {
            assert_eq!(r.method, "POST");
            let net: serde_json::Value = r.json().unwrap().unwrap();
            Response::ok(serde_json::json!({ "path": r.path, "q": r.query("a b"), "net": net["net"] }))
        });
        let (st, body) = call(srv.port(), &req("POST", "/runs/%E7%B5%8C%E7%90%86?a+b=c%20d&x=1", Some(TOKEN), r#"{"net":"月次"}"#));
        assert_eq!(st, 200);
        assert_eq!(body, serde_json::json!({ "path": "/runs/経理", "q": "c d", "net": "月次" }));
        srv.stop();
        let l = log.lock().unwrap();
        assert_eq!((l[0].method.as_str(), l[0].path.as_str(), l[0].status), ("POST", "/runs/経理", 200));
    }

    #[test]
    fn without_the_right_token_the_handler_is_not_called() {
        let called = Arc::new(AtomicBool::new(false));
        let c = called.clone();
        let (srv, log) = start(move |_| {
            c.store(true, Ordering::Relaxed);
            Response::ok(serde_json::Value::Null)
        });
        for t in [None, Some("wrong"), Some("s3cret-toke"), Some("s3cret-token2")] {
            assert_eq!(call(srv.port(), &req("GET", "/runs/1", t, "")).0, 401, "{t:?}");
        }
        assert_eq!(call(srv.port(), "GET / HTTP/1.1\r\nAuthorization: Basic czNjcmV0\r\n\r\n").0, 401);
        assert!(!called.load(Ordering::Relaxed));
        srv.stop();
        assert!(log.lock().unwrap().iter().all(|l| l.status == 401));
    }

    #[test]
    fn the_token_never_reaches_the_log() {
        let (srv, log) = start(|_| Response::ok(serde_json::Value::Null));
        call(srv.port(), &req("GET", "/x?token=s3cret-token", Some(TOKEN), ""));
        srv.stop();
        let l = format!("{:?}", log.lock().unwrap());
        assert!(!l.contains("s3cret"), "{l}");
    }

    #[test]
    fn limits_and_bad_requests() {
        let (srv, _) = start(|_| Response::ok(serde_json::Value::Null));
        let big = "x".repeat(101);
        assert_eq!(call(srv.port(), &req("POST", "/", Some(TOKEN), &big)).0, 413);
        assert_eq!(call(srv.port(), "garbage\r\n\r\n").0, 400);
        assert_eq!(
            call(srv.port(), &format!("POST / HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\nTransfer-Encoding: chunked\r\n\r\n")).0,
            501
        );
        assert_eq!(call(srv.port(), &req("GET", "/%ZZ", Some(TOKEN), "")).0, 400);
        // A client that sends half a head and stops gets cut off by the timeout.
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, srv.port())).unwrap();
        s.write_all(b"GET / HTT").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.1 408"), "{out}");
    }

    #[test]
    fn too_many_connections_get_a_503() {
        let gate = Arc::new(Mutex::new(()));
        let held = gate.lock().unwrap();
        let g = gate.clone();
        let mut cfg = Config::new(0, TOKEN);
        cfg.max_connections = 2;
        let srv = Server::start(
            cfg,
            move |_| {
                drop(g.lock().unwrap());
                Response::ok(serde_json::Value::Null)
            },
            |_| {},
        )
        .unwrap();
        let port = srv.port();
        let slow: Vec<_> = (0..2).map(|_| std::thread::spawn(move || call(port, &req("GET", "/", Some(TOKEN), "")))).collect();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(call(port, &req("GET", "/", Some(TOKEN), "")).0, 503);
        drop(held);
        for t in slow {
            assert_eq!(t.join().unwrap().0, 200);
        }
    }

    #[test]
    fn a_panicking_handler_is_a_500_and_the_server_lives() {
        let (srv, _) = start(|r| if r.path == "/boom" { panic!("boom") } else { Response::ok(serde_json::Value::Null) });
        assert_eq!(call(srv.port(), &req("GET", "/boom", Some(TOKEN), "")).0, 500);
        assert_eq!(call(srv.port(), &req("GET", "/", Some(TOKEN), "")).0, 200);
    }

    #[test]
    fn an_empty_token_is_refused_and_debug_hides_it() {
        assert!(Server::start(Config::new(0, ""), |_| Response::ok(serde_json::Value::Null), |_| {}).is_err());
        assert!(!format!("{:?}", Config::new(1, TOKEN)).contains("s3cret"));
    }

    #[test]
    fn constant_time_compare_and_decoding() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd") && !ct_eq(b"abc", b"ab") && !ct_eq(b"", b"a"));
        assert_eq!(percent_decode("a%2Fb+c", false).as_deref(), Some("a/b+c"));
        assert_eq!(percent_decode("a+c", true).as_deref(), Some("a c"));
        assert_eq!(percent_decode("%E3%81", false), None);
        assert_eq!(percent_decode("%4", false), None);
    }
}
