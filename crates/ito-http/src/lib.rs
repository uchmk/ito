//! A small HTTP listener an app runs on its own PC so another system can start something or read a state
//! (an inbound webhook, a local REST hook for scripts).
//!
//! - **Loopback by default.** It binds `127.0.0.1` (and `::1` when asked). With the feature `tls`, [`Bind::Lan`] opens
//!   it to the network, and then **only over TLS**: plain HTTP cannot leave the PC. The certificate is self-signed,
//!   made on first use in a folder the app names, and known by its fingerprint ([`tls`]); a client pins that
//!   fingerprint ([`client`]).
//! - **A bearer token on every request**, compared in constant time; a missing or wrong one is a 401 before the body
//!   is read and before the handler is called. The token is never logged ([`token_from_secret`] reads it from
//!   `ito-secret`).
//! - **Limits:** the head, the body (`max_body`, 413 before it is read), connections at once (`max_connections`, 503),
//!   and a read timeout per connection.
//! - JSON in and out, HTTP/1.1, one request per connection (`Connection: close`), no chunked bodies.
//! - Each request is reported to a log callback as method, path (no query, which may carry a secret), status and
//!   milliseconds.
//!
//! It runs on its own threads; the handler is called on a connection thread, so it must be `Send + Sync` and quick
//! to answer (or hand the work to the app and answer "accepted").

#[cfg(feature = "tls")]
pub mod client;
#[cfg(feature = "tls")]
pub mod tls;

#[cfg(feature = "tls")]
pub use tls::TlsCfg;

use std::collections::HashMap;
use std::io::{self, Read, Write};
#[cfg(feature = "tls")]
use std::net::IpAddr;
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MAX_HEAD: usize = 16 * 1024;

/// Where the listener is reachable from.
#[derive(Clone, Debug, Default)]
pub enum Bind {
    /// `127.0.0.1` (and `::1` with `Config::ipv6`), plain HTTP.
    #[default]
    Loopback,
    /// The network, over TLS only: every address (`addr: None`, `0.0.0.0`) or the one given. The certificate is made
    /// in `tls.cert_dir` on first use; `Config::ipv6` is ignored (give `addr` an IPv6 address to listen on one).
    #[cfg(feature = "tls")]
    Lan { addr: Option<IpAddr>, tls: TlsCfg },
}

/// How the listener is set up. [`Config::new`] gives the defaults.
#[derive(Clone)]
pub struct Config {
    /// Loopback (the default) or the LAN with TLS.
    pub bind: Bind,
    /// Port to listen on; `0` lets the system pick (see [`Server::port`]).
    pub port: u16,
    /// Also listen on `::1` (same port). A machine without IPv6 loopback just goes without.
    pub ipv6: bool,
    /// The bearer token every request must send (its name, in [`Request::who`], is `"default"`). May be empty only
    /// when `tokens` has one.
    pub token: String,
    /// More tokens, each with a name the handler learns through [`Request::who`] (a role, an audit trail).
    /// Add them with [`Config::token_named`]. The name must not be empty.
    pub tokens: Vec<(String, String)>,
    /// Exact paths a `GET` may fetch with no token (the page that signs in: `/`, `/app.js`). The handler is called
    /// with `who` `None`; anything with data behind it must check `who` itself.
    pub public: Vec<String>,
    /// Browser sessions: `POST /session` with a good bearer token sets an `HttpOnly; SameSite=Strict` cookie
    /// (`Secure` on TLS) that stands in for the header afterwards; `GET /session` says who; `DELETE /session` signs
    /// out. A request that has only the cookie and changes something (not `GET`/`HEAD`) must send `X-Ito-Csrf: 1`.
    pub sessions: bool,
    /// How long a session lasts (default 8 hours).
    pub session_ttl: Duration,
    /// Largest body accepted, in bytes (default 64 KiB).
    pub max_body: usize,
    /// Most connections served at once (default 8); one more gets a 503.
    pub max_connections: usize,
    /// Longest wait for the client to send, or to take, a piece of the request (default 10 s).
    pub timeout: Duration,
}

impl Config {
    pub fn new(port: u16, token: impl Into<String>) -> Self {
        Config {
            bind: Bind::Loopback,
            port,
            ipv6: false,
            token: token.into(),
            tokens: Vec::new(),
            public: Vec::new(),
            sessions: false,
            session_ttl: Duration::from_secs(8 * 3600),
            max_body: 64 * 1024,
            max_connections: 8,
            timeout: Duration::from_secs(10),
        }
    }

    /// One more token, known to the handler as `name`.
    pub fn token_named(mut self, name: impl Into<String>, token: impl Into<String>) -> Self {
        self.tokens.push((name.into(), token.into()));
        self
    }

    /// A path a `GET` may fetch with no token.
    pub fn public_path(mut self, path: impl Into<String>) -> Self {
        self.public.push(path.into());
        self
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("bind", &self.bind)
            .field("port", &self.port)
            .field("ipv6", &self.ipv6)
            .field("token", &"***")
            .field("tokens", &self.tokens.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>())
            .finish_non_exhaustive()
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
    /// The name of the token (or session) that let it in: `"default"` for `Config::token`, else the name from
    /// [`Config::token_named`]. `None` only for a [`Config::public`] path.
    pub who: Option<String>,
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

/// An answer: a status and a JSON value, or bytes with a content type ([`Response::bytes`]).
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub json: serde_json::Value,
    /// When set, sent instead of `json`: the content type and the bytes.
    pub raw: Option<(String, Vec<u8>)>,
    /// Headers to add (`Set-Cookie`, `Location`…). Names and values must not hold a line break.
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn new(status: u16, json: serde_json::Value) -> Self {
        Response { status, json, raw: None, headers: Vec::new() }
    }

    /// Not JSON: a page, a script, a style sheet, an image (`content_type` such as `text/html; charset=utf-8`).
    /// Meant for files embedded in the exe with `include_bytes!`.
    pub fn bytes(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Response { raw: Some((content_type.to_string(), body.into())), ..Self::new(status, serde_json::Value::Null) }
    }

    /// One more header on the answer.
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
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
    /// Signed-in browsers: the random id in the cookie → who and until when.
    sessions: Mutex<HashMap<String, (String, Instant)>>,
    /// Set for a LAN bind: every connection is TLS.
    #[cfg(feature = "tls")]
    tls: Option<Arc<rustls::ServerConfig>>,
}

/// A running listener. Dropping it (or [`Server::stop`]) stops accepting; requests already being served finish.
pub struct Server {
    port: u16,
    #[cfg(feature = "tls")]
    fingerprint: Option<tls::Fingerprint>,
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
        if cfg.token.is_empty() && cfg.tokens.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the token is empty"));
        }
        if cfg.tokens.iter().any(|(name, token)| name.is_empty() || token.is_empty()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a named token has an empty name or value"));
        }
        #[cfg(feature = "tls")]
        let mut identity = None;
        let mut listeners;
        let port;
        match &cfg.bind {
            Bind::Loopback => {
                let v4 = TcpListener::bind((Ipv4Addr::LOCALHOST, cfg.port))?;
                port = v4.local_addr()?.port();
                listeners = vec![v4];
                if cfg.ipv6 {
                    if let Ok(l) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)) {
                        listeners.push(l);
                    }
                }
            }
            #[cfg(feature = "tls")]
            Bind::Lan { addr, tls: tls_cfg } => {
                let id = tls::load_or_create(tls_cfg)?;
                let l = TcpListener::bind((addr.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)), cfg.port))?;
                port = l.local_addr()?.port();
                listeners = vec![l];
                identity = Some(id);
            }
        }
        #[cfg(feature = "tls")]
        let (fingerprint, tls_config) = match &identity {
            Some(id) => (Some(id.fingerprint()), Some(id.server_config()?)),
            None => (None, None),
        };
        let shared = Arc::new(Shared {
            cfg,
            handler: Box::new(handler),
            log: Box::new(log),
            active: AtomicUsize::new(0),
            sessions: Mutex::new(HashMap::new()),
            #[cfg(feature = "tls")]
            tls: tls_config,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        for l in listeners {
            l.set_nonblocking(true)?;
            let (shared, stop) = (shared.clone(), stop.clone());
            threads.push(std::thread::spawn(move || accept_loop(l, shared, stop)));
        }
        Ok(Server {
            port,
            #[cfg(feature = "tls")]
            fingerprint,
            stop,
            threads,
        })
    }

    /// The port in use (what the system picked when `Config::port` was 0).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The certificate's fingerprint, to show the person (`tls::format`), for a LAN bind; `None` on loopback.
    #[cfg(feature = "tls")]
    pub fn fingerprint(&self) -> Option<tls::Fingerprint> {
        self.fingerprint
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
                if matches!(shared.cfg.bind, Bind::Loopback) && !peer.ip().is_loopback() {
                    continue;
                }
                if shared.active.fetch_add(1, Ordering::AcqRel) >= shared.cfg.max_connections {
                    shared.active.fetch_sub(1, Ordering::AcqRel);
                    // On the LAN the 503 would need a handshake first, which a slow client could stall this thread with.
                    #[cfg(feature = "tls")]
                    if shared.tls.is_some() {
                        continue;
                    }
                    let _ = respond(&mut Wire::new(s, &shared), &Response::error(503, "too many connections"));
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

/// A connection: plain on loopback, TLS on the LAN.
enum Wire {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>),
}

impl Wire {
    fn new(s: TcpStream, shared: &Shared) -> Wire {
        let _ = s.set_nonblocking(false);
        let _ = s.set_read_timeout(Some(shared.cfg.timeout));
        let _ = s.set_write_timeout(Some(shared.cfg.timeout));
        #[cfg(feature = "tls")]
        if let Some(config) = &shared.tls {
            return match rustls::ServerConnection::new(config.clone()) {
                Ok(c) => Wire::Tls(Box::new(rustls::StreamOwned::new(c, s))),
                Err(_) => Wire::Plain(s),
            };
        }
        Wire::Plain(s)
    }

    /// Say goodbye properly (TLS close_notify) and close our side.
    fn finish(&mut self) {
        match self {
            Wire::Plain(s) => {
                let _ = s.shutdown(std::net::Shutdown::Write);
            }
            #[cfg(feature = "tls")]
            Wire::Tls(t) => {
                t.conn.send_close_notify();
                let _ = t.flush();
                let _ = t.sock.shutdown(std::net::Shutdown::Write);
            }
        }
    }
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Wire::Plain(s) => s.read(buf),
            #[cfg(feature = "tls")]
            Wire::Tls(t) => t.read(buf),
        }
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Wire::Plain(s) => s.write(buf),
            #[cfg(feature = "tls")]
            Wire::Tls(t) => t.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Wire::Plain(s) => s.flush(),
            #[cfg(feature = "tls")]
            Wire::Tls(t) => t.flush(),
        }
    }
}

fn serve(s: TcpStream, shared: &Shared) {
    let start = Instant::now();
    let mut s = Wire::new(s, shared);
    let (method, path, resp) = match read_request(&mut s, shared) {
        Ok((req, auth)) => {
            let (m, p) = (req.method.clone(), req.path.clone());
            if auth == Auth::No {
                (m, p, Response::error(401, "unauthorized"))
            } else if let (true, "/session") = (shared.cfg.sessions, p.as_str()) {
                (m, p, session_call(shared, &req, auth))
            } else {
                let r = catch_unwind(AssertUnwindSafe(|| (shared.handler)(req)));
                (m, p, r.unwrap_or_else(|_| Response::error(500, "internal error")))
            }
        }
        Err(Refused { method, path, status, message }) => (method, path, Response::error(status, message)),
    };
    let _ = respond(&mut s, &resp);
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

/// How a request got in.
#[derive(Debug, Clone, PartialEq)]
enum Auth {
    /// Not at all: a 401 (the body was not read).
    No,
    /// A `GET` of a [`Config::public`] path.
    Public,
    /// A bearer token.
    Bearer,
    /// A session cookie, with its id.
    Cookie(String),
}

/// The name of the token `given` is, if any. Every token is compared, so the time does not tell which one is close.
fn token_name(cfg: &Config, given: &[u8]) -> Option<String> {
    let mut found = None;
    if !cfg.token.is_empty() && ct_eq(given, cfg.token.as_bytes()) {
        found = Some("default".to_string());
    }
    for (name, token) in &cfg.tokens {
        if ct_eq(given, token.as_bytes()) && found.is_none() {
            found = Some(name.clone());
        }
    }
    found
}

/// The value of cookie `name` in a `Cookie` header.
fn cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').filter_map(|p| p.trim().split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v)
}

const SESSION_COOKIE: &str = "ito_session";
const MAX_SESSIONS: usize = 256;

/// The session `id` → who, while it lasts.
fn session_who(shared: &Shared, id: &str) -> Option<String> {
    let mut map = shared.sessions.lock().ok()?;
    match map.get(id) {
        Some((who, until)) if *until > Instant::now() => Some(who.clone()),
        Some(_) => {
            map.remove(id);
            None
        }
        None => None,
    }
}

fn new_session(shared: &Shared, who: &str) -> io::Result<String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| io::Error::other(e.to_string()))?;
    let id: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let mut map = shared.sessions.lock().map_err(|_| io::Error::other("session table poisoned"))?;
    let now = Instant::now();
    map.retain(|_, (_, until)| *until > now);
    if map.len() >= MAX_SESSIONS {
        if let Some(oldest) = map.iter().min_by_key(|(_, (_, until))| *until).map(|(k, _)| k.clone()) {
            map.remove(&oldest);
        }
    }
    map.insert(id.clone(), (who.to_string(), now + shared.cfg.session_ttl));
    Ok(id)
}

fn cookie_header(shared: &Shared, value: &str, max_age: u64) -> String {
    #[cfg(feature = "tls")]
    let secure = if shared.tls.is_some() { "; Secure" } else { "" };
    #[cfg(not(feature = "tls"))]
    let secure = {
        let _ = shared;
        ""
    };
    format!("{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}")
}

/// `/session`: sign in (`POST`, with the bearer token), who am I (`GET`), sign out (`DELETE`).
fn session_call(shared: &Shared, req: &Request, auth: Auth) -> Response {
    let who = req.who.clone().unwrap_or_default();
    match (req.method.as_str(), auth) {
        ("POST", Auth::Bearer) => match new_session(shared, &who) {
            Ok(id) => Response::ok(serde_json::json!({ "who": who }))
                .with_header("Set-Cookie", &cookie_header(shared, &id, shared.cfg.session_ttl.as_secs())),
            Err(_) => Response::error(500, "could not make a session"),
        },
        ("POST", _) => Response::error(401, "sign in with the bearer token"),
        ("GET", _) => Response::ok(serde_json::json!({ "who": who })),
        ("DELETE", Auth::Cookie(id)) => {
            if let Ok(mut map) = shared.sessions.lock() {
                map.remove(&id);
            }
            Response::ok(serde_json::json!({ "ok": true })).with_header("Set-Cookie", &cookie_header(shared, "", 0))
        }
        ("DELETE", _) => Response::ok(serde_json::json!({ "ok": true })),
        _ => Response::error(405, "method not allowed"),
    }
}

/// Read and check one request. `Ok((request, auth))`: when `auth` is [`Auth::No`] the body was not read.
fn read_request(s: &mut Wire, shared: &Shared) -> Result<(Request, Auth), Refused> {
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
    let bearer = header("authorization")
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .and_then(|(_, t)| token_name(&shared.cfg, t.trim().as_bytes()));
    let (mut auth, mut who) = match bearer {
        Some(name) => (Auth::Bearer, Some(name)),
        None => (Auth::No, None),
    };
    if auth == Auth::No && shared.cfg.sessions {
        if let Some(id) = header("cookie").and_then(|c| cookie(c, SESSION_COOKIE)) {
            if let Some(name) = session_who(shared, id) {
                // The cookie rides along by itself, so a change needs a header a page of another site cannot add.
                if !matches!(method, "GET" | "HEAD" | "OPTIONS") && header("x-ito-csrf") != Some("1") {
                    return Err(named(403, "send X-Ito-Csrf: 1"));
                }
                (auth, who) = (Auth::Cookie(id.to_string()), Some(name));
            }
        }
    }
    if auth == Auth::No && method == "GET" && shared.cfg.public.contains(&path) {
        auth = Auth::Public;
    }
    let mut req = Request {
        who,
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
    if auth == Auth::No {
        return Ok((req, auth));
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
    Ok((req, auth))
}

fn respond(s: &mut Wire, r: &Response) -> io::Result<()> {
    let (content_type, body) = match &r.raw {
        Some((ct, bytes)) => (ct.replace(['\r', '\n'], ""), bytes.clone()),
        None => ("application/json; charset=utf-8".to_string(), serde_json::to_vec(&r.json).unwrap_or_else(|_| b"null".to_vec())),
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n",
        r.status,
        reason(r.status),
        content_type,
        body.len()
    );
    for (name, value) in &r.headers {
        head.push_str(&format!("{}: {}\r\n", name.replace(['\r', '\n', ':'], ""), value.replace(['\r', '\n'], "")));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())?;
    s.write_all(&body)?;
    s.flush()?;
    s.finish();
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

    /// The whole answer text.
    fn call_text(port: u16, raw: &str) -> String {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    fn start_with(cfg: Config) -> Server {
        Server::start(cfg, |r| Response::ok(serde_json::json!({ "who": r.who, "path": r.path })), |_| {}).unwrap()
    }

    #[test]
    fn named_tokens_tell_the_handler_who_came() {
        let cfg = Config::new(0, TOKEN).token_named("viewer", "v-token").token_named("admin", "a-token");
        let srv = start_with(cfg);
        for (t, who) in [(TOKEN, "default"), ("v-token", "viewer"), ("a-token", "admin")] {
            let (st, body) = call(srv.port(), &req("GET", "/x", Some(t), ""));
            assert_eq!((st, body["who"].as_str()), (200, Some(who)), "{t}");
        }
        assert_eq!(call(srv.port(), &req("GET", "/x", Some("v-tokeN"), "")).0, 401);
        assert!(!format!("{:?}", Config::new(0, TOKEN).token_named("viewer", "v-token")).contains("v-token"));
    }

    #[test]
    fn only_named_tokens_are_enough_and_empty_ones_are_refused() {
        let mut cfg = Config::new(0, "");
        cfg.tokens.push(("admin".into(), "a-token".into()));
        let srv = start_with(cfg);
        assert_eq!(call(srv.port(), &req("GET", "/", Some("a-token"), "")).0, 200);
        assert_eq!(call(srv.port(), &req("GET", "/", Some(""), "")).0, 401);
        assert!(Server::start(Config::new(0, ""), |_| Response::ok(serde_json::Value::Null), |_| {}).is_err());
        assert!(Server::start(Config::new(0, TOKEN).token_named("", "x"), |_| Response::ok(serde_json::Value::Null), |_| {}).is_err());
        assert!(Server::start(Config::new(0, TOKEN).token_named("n", ""), |_| Response::ok(serde_json::Value::Null), |_| {}).is_err());
    }

    #[test]
    fn bytes_are_sent_as_given_and_a_public_path_needs_no_token() {
        let cfg = Config::new(0, TOKEN).public_path("/");
        let srv = Server::start(
            cfg,
            |r| match r.path.as_str() {
                "/" => Response::bytes(200, "text/html; charset=utf-8", "<h1>hi</h1>").with_header("X-Frame-Options", "DENY"),
                _ => Response::ok(serde_json::json!({ "who": r.who })),
            },
            |_| {},
        )
        .unwrap();
        let out = call_text(srv.port(), "GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert!(out.contains("Content-Type: text/html; charset=utf-8\r\n"), "{out}");
        assert!(out.contains("X-Frame-Options: DENY\r\n") && out.contains("X-Content-Type-Options: nosniff\r\n"), "{out}");
        assert!(out.ends_with("<h1>hi</h1>"), "{out}");
        // Only a GET of that exact path; the rest still wants the token.
        assert_eq!(call(srv.port(), "POST / HTTP/1.1\r\nContent-Length: 0\r\n\r\n").0, 401);
        assert_eq!(call(srv.port(), "GET /data HTTP/1.1\r\n\r\n").0, 401);
        assert_eq!(call(srv.port(), "GET /?x=1 HTTP/1.1\r\n\r\n").0, 200);
    }

    #[test]
    fn a_header_value_cannot_split_the_answer() {
        let srv = Server::start(
            Config::new(0, TOKEN),
            |_| Response::ok(serde_json::Value::Null).with_header("X-A", "one\r\nSet-Cookie: evil=1"),
            |_| {},
        )
        .unwrap();
        let out = call_text(srv.port(), &req("GET", "/", Some(TOKEN), ""));
        assert!(!out.contains("\r\nSet-Cookie"), "{out}");
    }

    fn cookie_of(out: &str) -> String {
        let line = out.lines().find(|l| l.starts_with("Set-Cookie: ")).unwrap_or_else(|| panic!("no cookie in {out}"));
        line["Set-Cookie: ".len()..].split(';').next().unwrap().to_string()
    }

    #[test]
    fn a_session_stands_in_for_the_token_and_changes_need_the_csrf_header() {
        let mut cfg = Config::new(0, TOKEN).token_named("admin", "a-token");
        cfg.sessions = true;
        let srv = start_with(cfg);
        let port = srv.port();
        // No cookie, no token: 401. A wrong token cannot sign in.
        assert_eq!(call(port, &req("GET", "/session", None, "")).0, 401);
        assert_eq!(call(port, &req("POST", "/session", Some("nope"), "")).0, 401);
        let out = call_text(port, &req("POST", "/session", Some("a-token"), ""));
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        let set = out.lines().find(|l| l.starts_with("Set-Cookie: ")).unwrap();
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Strict") && !set.contains("Secure"), "{set}");
        assert!(!out.contains("a-token"));
        let cookie = cookie_of(&out);
        let with_cookie = |method: &str, extra: &str| {
            call(port, &format!("{method} /x HTTP/1.1\r\nCookie: {cookie}\r\n{extra}Content-Length: 0\r\n\r\n"))
        };
        let (st, body) = with_cookie("GET", "");
        assert_eq!((st, body["who"].as_str()), (200, Some("admin")));
        assert_eq!(with_cookie("POST", "").0, 403);
        assert_eq!(with_cookie("POST", "X-Ito-Csrf: 0\r\n").0, 403);
        assert_eq!(with_cookie("POST", "X-Ito-Csrf: 1\r\n").0, 200);
        // The bearer header never needs it.
        assert_eq!(call(port, &req("POST", "/x", Some("a-token"), "")).0, 200);
        // A forged id is no session.
        assert_eq!(call(port, "GET /x HTTP/1.1\r\nCookie: ito_session=00\r\n\r\n").0, 401);
        // Who am I, then sign out; the cookie is dead afterwards.
        let (st, body) = call(port, &format!("GET /session HTTP/1.1\r\nCookie: {cookie}\r\n\r\n"));
        assert_eq!((st, body["who"].as_str()), (200, Some("admin")));
        let out = call_text(port, &format!("DELETE /session HTTP/1.1\r\nCookie: {cookie}\r\nX-Ito-Csrf: 1\r\n\r\n"));
        assert!(out.starts_with("HTTP/1.1 200") && out.contains("Max-Age=0"), "{out}");
        assert_eq!(with_cookie("GET", "").0, 401);
    }

    #[test]
    fn without_sessions_the_cookie_and_the_session_path_do_nothing() {
        let srv = start_with(Config::new(0, TOKEN));
        let out = call_text(srv.port(), &req("POST", "/session", Some(TOKEN), ""));
        assert!(!out.contains("Set-Cookie"), "{out}");
        assert_eq!(call(srv.port(), "GET /x HTTP/1.1\r\nCookie: ito_session=00\r\n\r\n").0, 401);
    }

    #[test]
    fn an_expired_session_is_gone() {
        let mut cfg = Config::new(0, TOKEN);
        cfg.sessions = true;
        cfg.session_ttl = Duration::from_millis(100);
        let srv = start_with(cfg);
        let cookie = cookie_of(&call_text(srv.port(), &req("POST", "/session", Some(TOKEN), "")));
        let get = || call(srv.port(), &format!("GET /x HTTP/1.1\r\nCookie: {cookie}\r\n\r\n")).0;
        assert_eq!(get(), 200);
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(get(), 401);
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
