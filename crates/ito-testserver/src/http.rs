//! A scripted HTTP server. See [`HttpServer`].

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::{is_timeout, Acceptor};

/// The largest request body kept (16 MiB); a bigger one is answered with 413.
const MAX_BODY: usize = 16 << 20;

/// What a route answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Waited before the answer is sent (a slow server, a client's timeout).
    pub delay: Duration,
}

impl Reply {
    pub fn new(status: u16) -> Reply {
        Reply { status, headers: Vec::new(), body: Vec::new(), delay: Duration::ZERO }
    }

    pub fn header(mut self, name: &str, value: &str) -> Reply {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Reply {
        self.body = body.into();
        self
    }

    /// A JSON body (the text is sent as is) with `Content-Type: application/json`.
    pub fn json(self, text: &str) -> Reply {
        self.header("Content-Type", "application/json").body(text.as_bytes())
    }

    pub fn delay(mut self, delay: Duration) -> Reply {
        self.delay = delay;
        self
    }
}

/// A request the server received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Upper case: `GET`.
    pub method: String,
    /// The path, with no query: `/a/b`.
    pub path: String,
    /// What follows the `?`, if anything.
    pub query: String,
    /// In the order received, names as sent.
    pub headers: Vec<(String, String)>,
    /// The body (a chunked body is joined).
    pub body: Vec<u8>,
}

impl Request {
    /// The first header called `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

struct State {
    routes: Mutex<Vec<(String, String, Reply)>>,
    requests: Mutex<Vec<Request>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A scripted HTTP/1.1 server on `127.0.0.1` (plain HTTP, one request per connection). A route is a method and a
/// path; a request that matches none is answered 404 with an empty body. Every request is recorded, matched or not.
/// It stops when it is dropped.
pub struct HttpServer {
    state: Arc<State>,
    acceptor: Acceptor,
}

impl HttpServer {
    pub fn start() -> io::Result<HttpServer> {
        let state = Arc::new(State { routes: Mutex::new(Vec::new()), requests: Mutex::new(Vec::new()) });
        let st = state.clone();
        let acceptor = Acceptor::start(move |stream, stop| {
            let _ = serve(&st, stream, &stop);
        })?;
        Ok(HttpServer { state, acceptor })
    }

    pub fn addr(&self) -> SocketAddr {
        self.acceptor.addr
    }

    pub fn port(&self) -> u16 {
        self.acceptor.addr.port()
    }

    /// `http://127.0.0.1:PORT`, no trailing slash.
    pub fn url(&self) -> String {
        format!("http://{}", self.acceptor.addr)
    }

    /// Answer `method` (any case) `path` (no query) with `reply`. A later route for the same method and path
    /// replaces an earlier one.
    pub fn route(&self, method: &str, path: &str, reply: Reply) -> &Self {
        let mut routes = lock(&self.state.routes);
        routes.retain(|(m, p, _)| !(m.eq_ignore_ascii_case(method) && p == path));
        routes.push((method.to_ascii_uppercase(), path.to_owned(), reply));
        self
    }

    /// The requests received so far, oldest first.
    pub fn requests(&self) -> Vec<Request> {
        lock(&self.state.requests).clone()
    }

    /// Wait until at least `n` requests came in, at most `timeout`. The requests so far either way.
    pub fn wait_for_requests(&self, n: usize, timeout: Duration) -> Vec<Request> {
        let end = Instant::now() + timeout;
        loop {
            let got = self.requests();
            if got.len() >= n || Instant::now() >= end {
                return got;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct Reader<'a> {
    stream: TcpStream,
    buf: Vec<u8>,
    stop: &'a AtomicBool,
}

impl Reader<'_> {
    fn fill(&mut self) -> Option<()> {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return None;
            }
            let mut tmp = [0u8; 8192];
            match self.stream.read(&mut tmp) {
                Ok(0) => return None,
                Ok(n) => {
                    self.buf.extend_from_slice(&tmp[..n]);
                    return Some(());
                }
                Err(e) if is_timeout(&e) => {}
                Err(_) => return None,
            }
        }
    }

    fn line(&mut self) -> Option<String> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=i).collect();
                return Some(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_owned());
            }
            self.fill()?;
        }
    }

    fn take(&mut self, n: usize) -> Option<Vec<u8>> {
        while self.buf.len() < n {
            self.fill()?;
        }
        Some(self.buf.drain(..n).collect())
    }
}

fn serve(st: &State, stream: TcpStream, stop: &AtomicBool) -> io::Result<()> {
    let out = stream.try_clone()?;
    let mut r = Reader { stream, buf: Vec::new(), stop };
    let Some(request_line) = r.line() else { return Ok(()) };
    let mut it = request_line.split_whitespace();
    let (method, target) = (it.next().unwrap_or("").to_ascii_uppercase(), it.next().unwrap_or("/").to_owned());
    let mut headers = Vec::new();
    loop {
        let Some(l) = r.line() else { return Ok(()) };
        if l.is_empty() {
            break;
        }
        if let Some((n, v)) = l.split_once(':') {
            headers.push((n.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let find = |name: &str| headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    if find("Expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
        (&out).write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let mut body = Vec::new();
    let mut too_big = false;
    if find("Transfer-Encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
        loop {
            let Some(size) = r.line() else { return Ok(()) };
            let n = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
            if n == 0 {
                while r.line().is_some_and(|l| !l.is_empty()) {}
                break;
            }
            let Some(chunk) = r.take(n + 2) else { return Ok(()) };
            body.extend_from_slice(&chunk[..n]);
            too_big |= body.len() > MAX_BODY;
            if too_big {
                break;
            }
        }
    } else if let Some(n) = find("Content-Length").and_then(|v| v.parse::<usize>().ok()) {
        if n > MAX_BODY {
            too_big = true;
        } else {
            let Some(b) = r.take(n) else { return Ok(()) };
            body = b;
        }
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_owned(), q.to_owned()),
        None => (target, String::new()),
    };
    let reply = if too_big {
        Reply::new(413)
    } else {
        let routes = lock(&st.routes);
        routes.iter().find(|(m, p, _)| *m == method && *p == path).map(|(_, _, r)| r.clone()).unwrap_or_else(|| Reply::new(404))
    };
    lock(&st.requests).push(Request { method: method.clone(), path, query, headers, body });
    let end = Instant::now() + reply.delay;
    while Instant::now() < end && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10).min(end - Instant::now()));
    }
    let mut head = format!("HTTP/1.1 {} {}\r\n", reply.status, reason(reply.status));
    for (n, v) in &reply.headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", reply.body.len()));
    let mut out = out;
    out.write_all(head.as_bytes())?;
    if method != "HEAD" {
        out.write_all(&reply.body)?;
    }
    out.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}
