//! A small HTTP listener an app runs on its own PC so another system can start something or read a state
//! (an inbound webhook, a local REST hook for scripts).
//!
//! - **Loopback by default.** It binds `127.0.0.1` (and `::1` when asked). With the feature `tls`, [`Bind::Lan`] opens
//!   it to the network, and then **only over TLS**: plain HTTP cannot leave the PC. The certificate is self-signed,
//!   made on first use in a folder the app names, and known by its fingerprint ([`tls`]); a client pins that
//!   fingerprint ([`client`]).
//! - **A bearer token on every request**, compared in constant time; a missing or wrong one is a 401 before the body
//!   is read and before the handler is called. The token is never logged ([`token_from_secret`] reads it from
//!   `ito-secret`). A token may be kept only as its SHA-256 ([`Config::token_hash_named`], [`new_token`], [`token_hash`]).
//! - **Who may come in can change while it runs:** [`Server::set_access`] swaps the tokens and the tailnet logins and tags
//!   ([`Access`]) without closing a socket; a browser session stays only while its token still signs in as the same name.
//! - **Limits:** the head, the body (`max_body`, 413 before it is read), connections at once (`max_connections`, 503),
//!   and a read timeout per connection.
//! - JSON in and out, HTTP/1.1, one request per connection (`Connection: close`), no chunked bodies.
//! - **On the tailnet, no token:** [`Config::tailnet`] adds a plain-HTTP listener on tailnet addresses only, where Tailscale's
//!   whois decides `who` ([`tailnet`]); a token still works there.
//! - Each request is reported to a log callback as method, path (no query, which may carry a secret), status and
//!   milliseconds.
//!
//! It runs on its own threads; the handler is called on a connection thread, so it must be `Send + Sync` and quick
//! to answer (or hand the work to the app and answer "accepted").

#[cfg(feature = "tls")]
pub mod client;
pub mod tailnet;
#[cfg(feature = "tls")]
pub mod tls;

#[cfg(feature = "tls")]
pub use tls::TlsCfg;

pub use tailnet::Tailnet;

use std::collections::HashMap;
use std::io::{self, Read, Write};
#[cfg(any(feature = "tls", test))]
use std::net::IpAddr;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
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
    /// when `tokens` or `token_hashes` has one.
    pub token: String,
    /// More tokens, each with a name the handler learns through [`Request::who`] (a role, an audit trail).
    /// Add them with [`Config::token_named`]. The name must not be empty.
    pub tokens: Vec<(String, String)>,
    /// Tokens kept only as their SHA-256 ([`token_hash`]: 64 lowercase hex characters), each with a name the handler
    /// learns through [`Request::who`], like `tokens`. Add them with [`Config::token_hash_named`]. Only for tokens made
    /// by [`new_token`]. The name must not be empty.
    pub token_hashes: Vec<(String, String)>,
    /// Exact paths a `GET` may fetch with no token (the page that signs in: `/`, `/app.js`). The handler is called
    /// with `who` `None`; anything with data behind it must check `who` itself.
    pub public: Vec<String>,
    /// Browser sessions: `POST /session` with a good bearer token sets an `HttpOnly; SameSite=Strict` cookie
    /// (`Secure` on TLS) that stands in for the header afterwards; `GET /session` says who; `DELETE /session` signs
    /// out. A request that has only the cookie and changes something (not `GET`/`HEAD`) must send `X-Ito-Csrf: 1`.
    pub sessions: bool,
    /// How long a session lasts (default 8 hours).
    pub session_ttl: Duration,
    /// A second listener on the tailnet only, where Tailscale's whois says who is calling ([`tailnet`]). `None`
    /// (the default) opens nothing. With it, `token`, `tokens` and `token_hashes` may all be empty.
    pub tailnet: Option<Tailnet>,
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
            token_hashes: Vec::new(),
            public: Vec::new(),
            sessions: false,
            session_ttl: Duration::from_secs(8 * 3600),
            tailnet: None,
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

    /// One more token, given only as its [`token_hash`], known to the handler as `name`.
    pub fn token_hash_named(mut self, name: impl Into<String>, hash: impl Into<String>) -> Self {
        self.token_hashes.push((name.into(), hash.into()));
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
            .field("token_hashes", &self.token_hashes.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>())
            .field("tailnet", &self.tailnet)
            .finish_non_exhaustive()
    }
}

/// Who may come in: the part of [`Config`] a running [`Server`] can swap ([`Server::set_access`]).
#[derive(Clone, Default)]
pub struct Access {
    /// As [`Config::token`] (its name is `"default"`).
    pub token: String,
    /// As [`Config::tokens`]: name and token.
    pub tokens: Vec<(String, String)>,
    /// As [`Config::token_hashes`]: name and SHA-256 hex.
    pub token_hashes: Vec<(String, String)>,
    /// As [`Tailnet::logins`]. Only for a server started with [`Config::tailnet`].
    pub tailnet_logins: Vec<(String, String)>,
    /// As [`Tailnet::tags`]. Only for a server started with [`Config::tailnet`].
    pub tailnet_tags: Vec<(String, String)>,
}

impl Access {
    /// The access `cfg` would start a server with: a copy of its tokens, token hashes and (with [`Config::tailnet`])
    /// the tailnet logins and tags. Nothing is checked here; [`Server::set_access`] checks.
    pub fn of(cfg: &Config) -> Access {
        let (tailnet_logins, tailnet_tags) = match &cfg.tailnet {
            Some(t) => (t.logins.clone(), t.tags.clone()),
            None => (Vec::new(), Vec::new()),
        };
        Access {
            token: cfg.token.clone(),
            tokens: cfg.tokens.clone(),
            token_hashes: cfg.token_hashes.clone(),
            tailnet_logins,
            tailnet_tags,
        }
    }
}

impl std::fmt::Debug for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Access")
            .field("token", &if self.token.is_empty() { "" } else { "***" })
            .field("tokens", &self.tokens.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>())
            .field("token_hashes", &self.token_hashes.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>())
            .field("tailnet_logins", &self.tailnet_logins)
            .field("tailnet_tags", &self.tailnet_tags)
            .finish()
    }
}

/// The checks on who may come in, for [`Server::start`] and [`Server::set_access`]. `tailnet`: the server has (was
/// started with) a tailnet listener.
fn check_access(access: &Access, tailnet: bool) -> io::Result<()> {
    let invalid = |msg: String| Err(io::Error::new(io::ErrorKind::InvalidInput, msg));
    if !tailnet && (!access.tailnet_logins.is_empty() || !access.tailnet_tags.is_empty()) {
        return invalid("this server has no tailnet listener".into());
    }
    if access.token.is_empty() && access.tokens.is_empty() && access.token_hashes.is_empty() && !tailnet {
        return invalid("the token is empty".into());
    }
    if access.tokens.iter().any(|(name, token)| name.is_empty() || token.is_empty()) {
        return invalid("a named token has an empty name or value".into());
    }
    let empty_hash = token_hash("");
    for (name, hash) in &access.token_hashes {
        if name.is_empty() {
            return invalid("a token hash has an empty name".into());
        } else if hash.len() != 64 || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return invalid(format!("the token hash for `{name}` is not 64 lowercase hex characters"));
        } else if *hash == empty_hash {
            return invalid(format!("the token hash for `{name}` is the hash of an empty token"));
        }
    }
    if tailnet {
        if access.tailnet_logins.is_empty() && access.tailnet_tags.is_empty() {
            return invalid("the tailnet listener has no login or tag to let in".into());
        }
        if access.tailnet_logins.iter().chain(&access.tailnet_tags).any(|(k, w)| k.is_empty() || w.is_empty()) {
            return invalid("a tailnet login or tag has an empty name".into());
        }
    }
    Ok(())
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

/// How a request was let in, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// Not at all (a 401, or the request was refused before it was judged).
    Denied,
    /// A bearer token.
    Token,
    /// A session cookie.
    Session,
    /// Tailscale's whois on the tailnet listener.
    Tailscale,
    /// A [`Config::public`] path.
    Public,
}

/// One served request, for the app's log. Nothing secret is in it.
#[derive(Debug, Clone)]
pub struct LogLine {
    /// Who was let in (`None` when nobody was) and how.
    pub who: Option<String>,
    pub via: Via,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub millis: u64,
}

/// The tailnet listener's state, read by [`Server::tailnet_port`] and [`Server::tailnet_error`].
#[derive(Default)]
struct TailState {
    /// 0 until the listener is open.
    port: AtomicU32,
    /// Why it is not open yet.
    error: Mutex<Option<String>>,
}

type Handler = dyn Fn(Request) -> Response + Send + Sync;
type Log = dyn Fn(&LogLine) + Send + Sync;

/// A signed-in browser.
struct Session {
    who: String,
    /// The SHA-256 of the bearer token that signed it in: it is kept only while that token still signs in as `who`.
    hash: String,
    until: Instant,
}

struct Shared {
    /// The listener's setup. Its tokens and the tailnet's logins and tags are cleared: they live in `access`.
    cfg: Config,
    /// Who may come in. Lock order: `sessions` first, then this. Never take `sessions` while holding a guard of this.
    access: RwLock<Arc<Access>>,
    handler: Box<Handler>,
    log: Box<Log>,
    active: AtomicUsize,
    /// Signed-in browsers: the random id in the cookie → the session.
    sessions: Mutex<HashMap<String, Session>>,
    /// What the `Host` header may be on the tailnet listener: this PC's addresses and names.
    hosts: RwLock<Vec<String>>,
    /// How the tailnet listener is doing (it opens in the background when Tailscale is not up yet).
    tail: TailState,
    /// Set for a LAN bind: every connection is TLS.
    #[cfg(feature = "tls")]
    tls: Option<Arc<rustls::ServerConfig>>,
}

/// A running listener. Dropping it (or [`Server::stop`]) stops accepting; requests already being served finish.
pub struct Server {
    port: u16,
    shared: Arc<Shared>,
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
        let mut cfg = cfg;
        let access = Access::of(&cfg);
        check_access(&access, cfg.tailnet.is_some())?;
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
        if let Some(t) = &cfg.tailnet {
            if let Some(bad) = t.addrs.iter().find(|a| !tailnet::is_tailnet_ip(**a)) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{bad} is not a tailnet address")));
            }
        }
        #[cfg(feature = "tls")]
        let (fingerprint, tls_config) = match &identity {
            Some(id) => (Some(id.fingerprint()), Some(id.server_config()?)),
            None => (None, None),
        };
        // The access lives in `Shared::access` only, so no code can read a stale copy from the config.
        cfg.token = String::new();
        cfg.tokens = Vec::new();
        cfg.token_hashes = Vec::new();
        if let Some(t) = &mut cfg.tailnet {
            t.logins = Vec::new();
            t.tags = Vec::new();
        }
        let shared = Arc::new(Shared {
            cfg,
            access: RwLock::new(Arc::new(access)),
            handler: Box::new(handler),
            log: Box::new(log),
            active: AtomicUsize::new(0),
            sessions: Mutex::new(HashMap::new()),
            hosts: RwLock::new(Vec::new()),
            tail: TailState::default(),
            #[cfg(feature = "tls")]
            tls: tls_config,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        for l in listeners {
            l.set_nonblocking(true)?;
            let (shared, stop) = (shared.clone(), stop.clone());
            threads.push(std::thread::spawn(move || accept_loop(l, shared, stop, Kind::Main)));
        }
        if shared.cfg.tailnet.is_some() {
            // The first try is made here so a ready Tailscale gives `tailnet_port()` at once; otherwise a thread keeps trying.
            let first = open_tailnet(&shared);
            if let Err(e) = &first {
                if let Ok(mut slot) = shared.tail.error.lock() {
                    *slot = Some(e.to_string());
                }
            }
            let (shared, stop) = (shared.clone(), stop.clone());
            threads.push(std::thread::spawn(move || tailnet_thread(first, shared, stop)));
        }
        Ok(Server {
            port,
            shared,
            #[cfg(feature = "tls")]
            fingerprint,
            stop,
            threads,
        })
    }

    /// Swap who may come in, without closing a socket. It replaces the whole policy (it is not merged into the old
    /// one), and is checked like [`Server::start`] checks the config: `Err` (`InvalidInput`) for an empty token set
    /// on a server without a tailnet listener, a bad named token or token hash, bad tailnet logins or tags, or tailnet
    /// logins or tags for a server started without [`Config::tailnet`]. On `Err` nothing changes: the old access and
    /// every session stay. Calls are serialized.
    ///
    /// Requests already let in are not cancelled: they finish with the access that let them in. The sockets, port,
    /// TLS, tailnet addresses and the rest of the config are not touched. A browser session stays only while its
    /// token still signs in as the same name under the new access (a removed name or a remade token ends it).
    pub fn set_access(&self, access: Access) -> io::Result<()> {
        check_access(&access, self.shared.cfg.tailnet.is_some())?;
        let access = Arc::new(access);
        let poisoned = |what: &str| io::Error::other(format!("{what} lock poisoned"));
        // Lock order: sessions, then access. Both are held before anything changes.
        let mut sessions = self.shared.sessions.lock().map_err(|_| poisoned("session table"))?;
        let mut slot = self.shared.access.write().map_err(|_| poisoned("access"))?;
        *slot = access.clone();
        sessions.retain(|_, s| name_of_hash(&access, &s.hash).as_deref() == Some(s.who.as_str()));
        Ok(())
    }

    /// The port in use (what the system picked when `Config::port` was 0).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The tailnet listener's port once it is open. `None` without [`Config::tailnet`], and while Tailscale cannot be
    /// reached: the main listener is up regardless, and the tailnet one is retried in the background (see
    /// [`Server::tailnet_error`]).
    pub fn tailnet_port(&self) -> Option<u16> {
        Some(self.shared.tail.port.load(Ordering::Relaxed) as u16).filter(|p| *p != 0)
    }

    /// Why the tailnet listener is not open yet (`tailscaled` not running, no tailnet address, the port taken), or `None`
    /// when it is open or [`Config::tailnet`] is not set. It is cleared once the listener opens.
    pub fn tailnet_error(&self) -> Option<String> {
        self.shared.tail.error.lock().ok().and_then(|e| e.clone())
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

/// How long to wait between tries at the tailnet listener: 2 s, doubling to a minute.
fn tail_wait(tries: u32) -> Duration {
    Duration::from_secs((2u64 << tries.min(5)).min(60))
}

/// Open the tailnet listeners (all of this PC's tailnet addresses on one port); records the hosts the `Host` header may
/// carry. Nothing is left open on `Err`.
fn open_tailnet(shared: &Shared) -> io::Result<Vec<TcpListener>> {
    let Some(t) = &shared.cfg.tailnet else { return Ok(Vec::new()) };
    let status = t.whois.status();
    let addrs = tailnet::bind_addrs(t, &status)?;
    let mut hosts = t.hosts.clone();
    hosts.extend(addrs.iter().map(|a| a.to_string()));
    if let Ok(st) = &status {
        hosts.extend(st.names.iter().cloned());
    }
    let (mut listeners, mut port) = (Vec::new(), t.port);
    for a in addrs {
        let l = TcpListener::bind((a, port))?;
        port = l.local_addr()?.port();
        l.set_nonblocking(true)?;
        listeners.push(l);
    }
    if let Ok(mut h) = shared.hosts.write() {
        *h = hosts;
    }
    shared.tail.port.store(u32::from(port), Ordering::Relaxed);
    Ok(listeners)
}

/// Opens the tailnet listener (trying again until it works), then serves it until `stop`.
fn tailnet_thread(first: io::Result<Vec<TcpListener>>, shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    let mut next = first;
    let mut tries = 0u32;
    let listeners = loop {
        match next {
            Ok(l) => break l,
            Err(e) => {
                if let Ok(mut slot) = shared.tail.error.lock() {
                    *slot = Some(e.to_string());
                }
            }
        }
        let until = Instant::now() + tail_wait(tries);
        tries += 1;
        while Instant::now() < until {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        next = open_tailnet(&shared);
    };
    if let Ok(mut slot) = shared.tail.error.lock() {
        *slot = None;
    }
    std::thread::scope(|s| {
        for l in listeners {
            let (shared, stop) = (shared.clone(), stop.clone());
            s.spawn(move || accept_loop(l, shared, stop, Kind::Tailnet));
        }
    });
}

/// Which listener a connection came in on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Loopback or the LAN (TLS).
    Main,
    /// The tailnet: plain HTTP, whois decides who.
    Tailnet,
}

fn accept_loop(l: TcpListener, shared: Arc<Shared>, stop: Arc<AtomicBool>, kind: Kind) {
    while !stop.load(Ordering::Relaxed) {
        match l.accept() {
            Ok((s, peer)) => {
                let allowed = match kind {
                    Kind::Main => !matches!(shared.cfg.bind, Bind::Loopback) || peer.ip().is_loopback(),
                    Kind::Tailnet => tailnet::is_tailnet_ip(peer.ip()),
                };
                if !allowed {
                    continue;
                }
                if shared.active.fetch_add(1, Ordering::AcqRel) >= shared.cfg.max_connections {
                    shared.active.fetch_sub(1, Ordering::AcqRel);
                    // On the LAN the 503 would need a handshake first, which a slow client could stall this thread with.
                    #[cfg(feature = "tls")]
                    if kind == Kind::Main && shared.tls.is_some() {
                        continue;
                    }
                    let _ = respond(&mut Wire::new(s, &shared, kind), &Response::error(503, "too many connections"));
                    continue;
                }
                let shared = shared.clone();
                std::thread::spawn(move || {
                    serve(s, &shared, kind, peer);
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
    fn new(s: TcpStream, shared: &Shared, kind: Kind) -> Wire {
        let _ = s.set_nonblocking(false);
        let _ = s.set_read_timeout(Some(shared.cfg.timeout));
        let _ = s.set_write_timeout(Some(shared.cfg.timeout));
        #[cfg(not(feature = "tls"))]
        let _ = kind;
        #[cfg(feature = "tls")]
        if let (Kind::Main, Some(config)) = (kind, &shared.tls) {
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

fn serve(s: TcpStream, shared: &Shared, kind: Kind, peer: SocketAddr) {
    let start = Instant::now();
    let mut s = Wire::new(s, shared, kind);
    let (who, via, method, path, resp) = match read_request(&mut s, shared, kind, peer) {
        Ok((req, auth)) => {
            let (m, p, who) = (req.method.clone(), req.path.clone(), req.who.clone());
            let via = match &auth {
                Auth::No => Via::Denied,
                Auth::Public => Via::Public,
                Auth::Bearer(_) => Via::Token,
                Auth::Cookie(_) => Via::Session,
                Auth::Tailnet => Via::Tailscale,
            };
            if auth == Auth::No {
                (None, via, m, p, Response::error(401, "unauthorized"))
            } else if let (true, "/session") = (shared.cfg.sessions, p.as_str()) {
                (who, via, m, p, session_call(shared, &req, auth, kind))
            } else {
                let r = catch_unwind(AssertUnwindSafe(|| (shared.handler)(req)));
                (who, via, m, p, r.unwrap_or_else(|_| Response::error(500, "internal error")))
            }
        }
        Err(Refused { method, path, status, message }) => (None, Via::Denied, method, path, Response::error(status, message)),
    };
    let _ = respond(&mut s, &resp);
    (shared.log)(&LogLine { who, via, method, path, status: resp.status, millis: start.elapsed().as_millis() as u64 });
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
#[derive(Clone, PartialEq)]
enum Auth {
    /// Not at all: a 401 (the body was not read).
    No,
    /// A `GET` of a [`Config::public`] path.
    Public,
    /// A bearer token, with its SHA-256 hex.
    Bearer(String),
    /// A session cookie, with its id.
    Cookie(String),
    /// Tailscale's whois, on the tailnet listener.
    Tailnet,
}

// By hand: the token's hash and the cookie id are credentials.
impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::No => f.write_str("No"),
            Auth::Public => f.write_str("Public"),
            Auth::Bearer(_) => f.write_str("Bearer(..)"),
            Auth::Cookie(_) => f.write_str("Cookie(..)"),
            Auth::Tailnet => f.write_str("Tailnet"),
        }
    }
}

/// The name of the token `given` is, if any. Every token is compared, so the time does not tell which one is close.
/// A hashed token is compared the same way, as the SHA-256 of `given` against each stored hash.
fn token_name(access: &Access, given: &[u8]) -> Option<String> {
    let mut found = None;
    if !access.token.is_empty() && ct_eq(given, access.token.as_bytes()) {
        found = Some("default".to_string());
    }
    for (name, token) in &access.tokens {
        if ct_eq(given, token.as_bytes()) && found.is_none() {
            found = Some(name.clone());
        }
    }
    if !access.token_hashes.is_empty() {
        let given_hash = token_hash_bytes(given);
        for (name, hash) in &access.token_hashes {
            if ct_eq(given_hash.as_bytes(), hash.as_bytes()) && found.is_none() {
                found = Some(name.clone());
            }
        }
    }
    found
}

/// The name a token with SHA-256 `hash` signs in as under `access`: [`token_name`]'s order (the default token, the
/// named tokens, the hashes), each compared.
fn name_of_hash(access: &Access, hash: &str) -> Option<String> {
    let mut found = None;
    if !access.token.is_empty() && ct_eq(token_hash(&access.token).as_bytes(), hash.as_bytes()) {
        found = Some("default".to_string());
    }
    for (name, token) in &access.tokens {
        if ct_eq(token_hash(token).as_bytes(), hash.as_bytes()) && found.is_none() {
            found = Some(name.clone());
        }
    }
    for (name, h) in &access.token_hashes {
        if ct_eq(h.as_bytes(), hash.as_bytes()) && found.is_none() {
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
        Some(s) if s.until > Instant::now() => Some(s.who.clone()),
        Some(_) => {
            map.remove(id);
            None
        }
        None => None,
    }
}

/// A new token: 32 random bytes from the OS, as 64 lowercase hex characters. `Err` when the OS gives no randomness.
pub fn new_token() -> io::Result<String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

/// The SHA-256 of `token` as 64 lowercase hex characters: what [`Config::token_hash_named`] takes, and what the
/// listener compares. A plain SHA-256 is safe for a token from [`new_token`] (256 random bits); it is not for a
/// password a person chose, which needs a salted, slow hash.
pub fn token_hash(token: &str) -> String {
    token_hash_bytes(token.as_bytes())
}

fn token_hash_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A session for `who`, signed in with the token whose SHA-256 is `hash`. `Ok(None)`: that token no longer signs in as
/// `who` (the access changed meanwhile), so nothing was added or evicted.
fn new_session(shared: &Shared, who: &str, hash: &str) -> io::Result<Option<String>> {
    let id = new_token()?;
    // Lock order: sessions, then access (set_access holds both while it swaps).
    let mut map = shared.sessions.lock().map_err(|_| io::Error::other("session table poisoned"))?;
    let current = shared.access.read().map(|a| Arc::clone(&a)).map_err(|_| io::Error::other("access lock poisoned"))?;
    if name_of_hash(&current, hash).as_deref() != Some(who) {
        return Ok(None);
    }
    let now = Instant::now();
    map.retain(|_, s| s.until > now);
    if map.len() >= MAX_SESSIONS {
        if let Some(oldest) = map.iter().min_by_key(|(_, s)| s.until).map(|(k, _)| k.clone()) {
            map.remove(&oldest);
        }
    }
    map.insert(id.clone(), Session { who: who.to_string(), hash: hash.to_string(), until: now + shared.cfg.session_ttl });
    Ok(Some(id))
}

fn cookie_header(secure: bool, value: &str, max_age: u64) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}")
}

/// `/session`: sign in (`POST`, with the bearer token), who am I (`GET`), sign out (`DELETE`).
fn session_call(shared: &Shared, req: &Request, auth: Auth, kind: Kind) -> Response {
    #[cfg(feature = "tls")]
    let secure = kind == Kind::Main && shared.tls.is_some();
    #[cfg(not(feature = "tls"))]
    let secure = {
        let _ = kind;
        false
    };
    let who = req.who.clone().unwrap_or_default();
    match (req.method.as_str(), auth) {
        ("POST", Auth::Bearer(hash)) => match new_session(shared, &who, &hash) {
            Ok(Some(id)) => Response::ok(serde_json::json!({ "who": who }))
                .with_header("Set-Cookie", &cookie_header(secure, &id, shared.cfg.session_ttl.as_secs())),
            // The token was taken away between the check and now.
            Ok(None) => Response::error(401, "sign in with the bearer token"),
            Err(_) => Response::error(500, "could not make a session"),
        },
        ("POST", _) => Response::error(401, "sign in with the bearer token"),
        ("GET", _) => Response::ok(serde_json::json!({ "who": who })),
        ("DELETE", Auth::Cookie(id)) => {
            if let Ok(mut map) = shared.sessions.lock() {
                map.remove(&id);
            }
            Response::ok(serde_json::json!({ "ok": true })).with_header("Set-Cookie", &cookie_header(secure, "", 0))
        }
        ("DELETE", _) => Response::ok(serde_json::json!({ "ok": true })),
        _ => Response::error(405, "method not allowed"),
    }
}

/// Read and check one request. `Ok((request, auth))`: when `auth` is [`Auth::No`] the body was not read.
fn read_request(s: &mut Wire, shared: &Shared, kind: Kind, peer: SocketAddr) -> Result<(Request, Auth), Refused> {
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
    // One snapshot of who may come in, for the whole request (a slow whois or body read keeps the access the request
    // started with). The guard is gone at the end of this statement. A poisoned lock lets nobody in by token or whois.
    let access: Option<Arc<Access>> = shared.access.read().map(|a| Arc::clone(&a)).ok();
    let bearer = header("authorization")
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .and_then(|(_, t)| {
            let name = token_name(access.as_deref()?, t.trim().as_bytes())?;
            Some((name, token_hash_bytes(t.trim().as_bytes())))
        });
    let (mut auth, mut who) = match bearer {
        Some((name, hash)) => (Auth::Bearer(hash), Some(name)),
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
    if auth == Auth::No && kind == Kind::Tailnet {
        if let (Some(t), Some(access)) = (&shared.cfg.tailnet, &access) {
            // Failing to ask, not knowing the peer and not listing it all end the same way: no `who`, a 401.
            let found = tailnet::whois_within(&t.whois, peer, Duration::from_secs(3))
                .ok()
                .flatten()
                .and_then(|id| tailnet::who_of(&access.tailnet_logins, &access.tailnet_tags, &id));
            if let Some(name) = found {
                // The peer's address says who it is, so any page the person has open could make it call: the same
                // two guards as a cookie, and the Host must be this PC's (a rebound name would carry another).
                if !header("host").is_some_and(|h| shared.hosts.read().is_ok_and(|hs| tailnet::host_allowed(h, &hs))) {
                    return Err(named(403, "host not allowed"));
                }
                if !matches!(method, "GET" | "HEAD" | "OPTIONS") && header("x-ito-csrf") != Some("1") {
                    return Err(named(403, "send X-Ito-Csrf: 1"));
                }
                (auth, who) = (Auth::Tailnet, Some(name));
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
    fn token_hash_and_new_token() {
        assert_eq!(token_hash("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(token_hash(""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')));
        assert_ne!(a, b);
    }

    #[test]
    fn a_hashed_token_lets_its_owner_in_and_nobody_else() {
        let t = new_token().unwrap();
        let hash = token_hash(&t);
        let srv = start_with(Config::new(0, "").token_hash_named("alice", hash.clone()));
        let (st, body) = call(srv.port(), &req("GET", "/x", Some(&t), ""));
        assert_eq!((st, body["who"].as_str()), (200, Some("alice")));
        assert_eq!(call(srv.port(), &req("GET", "/x", Some("wrong"), "")).0, 401);
        assert_eq!(call(srv.port(), &req("GET", "/x", None, "")).0, 401);
        assert_eq!(call(srv.port(), "GET /x HTTP/1.1\r\nAuthorization: Bearer \r\n\r\n").0, 401);
        assert_eq!(call(srv.port(), &req("GET", "/x", Some(&hash), "")).0, 401);
    }

    #[test]
    fn plain_and_hashed_tokens_each_have_their_own_name() {
        let hashed = new_token().unwrap();
        let cfg = Config::new(0, "plain-one").token_named("bob", "b-token").token_hash_named("alice", token_hash(&hashed));
        let srv = start_with(cfg);
        for (t, who) in [("plain-one", "default"), ("b-token", "bob"), (hashed.as_str(), "alice")] {
            let (st, body) = call(srv.port(), &req("GET", "/x", Some(t), ""));
            assert_eq!((st, body["who"].as_str()), (200, Some(who)), "{t}");
        }
    }

    #[test]
    fn bad_token_hashes_are_refused_without_naming_the_hash() {
        let good = token_hash("abc");
        let cases = [
            ("alice", good[..63].to_string()),
            ("alice", format!("{good}0")),
            ("alice", good.to_uppercase()),
            ("alice", format!("g{}", &good[1..])),
            ("alice", token_hash("")),
            ("", good.clone()),
        ];
        for (name, hash) in cases {
            let cfg = Config::new(0, "").token_hash_named(name, hash.clone());
            let err =
                Server::start(cfg, |_| Response::ok(serde_json::Value::Null), |_| {}).err().unwrap_or_else(|| panic!("accepted {name:?}"));
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
            assert!(!err.to_string().contains(&hash), "{err}");
        }
        let ok = Server::start(Config::new(0, "").token_hash_named("alice", good), |_| Response::ok(serde_json::Value::Null), |_| {});
        assert!(ok.is_ok());
    }

    #[test]
    fn debug_shows_the_name_of_a_hashed_token_not_the_hash() {
        let hash = token_hash("abc");
        let shown = format!("{:?}", Config::new(0, TOKEN).token_hash_named("alice", hash.clone()));
        assert!(shown.contains("alice") && !shown.contains(&hash), "{shown}");
    }

    #[test]
    fn token_name_takes_plain_tokens_first_and_never_matches_an_empty_one() {
        let t = new_token().unwrap();
        let cfg = Config::new(0, t.clone()).token_named("bob", t.clone()).token_hash_named("alice", token_hash(&t));
        assert_eq!(token_name(&Access::of(&cfg), t.as_bytes()).as_deref(), Some("default"));
        let cfg = Config::new(0, "").token_named("bob", t.clone()).token_hash_named("alice", token_hash(&t));
        assert_eq!(token_name(&Access::of(&cfg), t.as_bytes()).as_deref(), Some("bob"));
        let cfg = Config::new(0, "").token_hash_named("carol", token_hash("x")).token_hash_named("alice", token_hash(&t));
        assert_eq!(token_name(&Access::of(&cfg), t.as_bytes()).as_deref(), Some("alice"));
        assert_eq!(token_name(&Access::of(&cfg), b""), None);
        assert_eq!(token_name(&Access::of(&cfg), token_hash(&t).as_bytes()), None);
    }

    #[test]
    fn a_session_can_be_signed_in_with_a_hashed_token() {
        let t = new_token().unwrap();
        let mut cfg = Config::new(0, "").token_hash_named("alice", token_hash(&t));
        cfg.sessions = true;
        let srv = start_with(cfg);
        let out = call_text(srv.port(), &req("POST", "/session", Some(&t), ""));
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        let cookie = cookie_of(&out);
        let (st, body) = call(srv.port(), &format!("GET /x HTTP/1.1\r\nCookie: {cookie}\r\n\r\n"));
        assert_eq!((st, body["who"].as_str()), (200, Some("alice")));
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

    /// A whois that answers one fixed way; the status is the real PC's (the tests skip without tailscaled).
    struct Fake(Option<tailnet::Identity>, bool);
    impl tailnet::Whois for Fake {
        fn whois(&self, _: SocketAddr) -> io::Result<Option<tailnet::Identity>> {
            if self.1 {
                Err(io::Error::other("tailscaled is down"))
            } else {
                Ok(self.0.clone())
            }
        }
        fn status(&self) -> io::Result<tailnet::Status> {
            tailnet::LocalApi::default().status()
        }
    }

    type TailRig = (Server, IpAddr, Arc<Mutex<Vec<LogLine>>>);

    fn tail_server(w: Fake, f: impl Fn(Request) -> Response + Send + Sync + 'static) -> Option<TailRig> {
        let st = tailnet::LocalApi::default().status().ok()?;
        let ip = *st.addrs.iter().find(|a| a.is_ipv4())?;
        let mut t = Tailnet::new(0).login("alice@example.com", "owner").tag("tag:ha", "standby");
        t.addrs = vec![ip];
        t.whois = Arc::new(w);
        let mut cfg = Config::new(0, TOKEN);
        cfg.tailnet = Some(t);
        cfg.timeout = Duration::from_millis(800);
        let lines = Arc::new(Mutex::new(Vec::new()));
        let l = lines.clone();
        Some((Server::start(cfg, f, move |x| l.lock().unwrap().push(x.clone())).unwrap(), ip, lines))
    }

    fn tcall(ip: IpAddr, port: u16, raw: &str) -> u16 {
        let mut s = TcpStream::connect((ip, port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
    }

    fn alice() -> tailnet::Identity {
        tailnet::Identity { login: Some("alice@example.com".into()), ..Default::default() }
    }

    #[test]
    fn tailnet_lets_a_listed_login_in_without_a_token() {
        let Some((srv, ip, log)) = tail_server(Fake(Some(alice()), false), |r| Response::ok(serde_json::json!({ "who": r.who }))) else {
            return;
        };
        let port = srv.tailnet_port().unwrap();
        let host = format!("Host: {ip}:{port}\r\n");
        assert_eq!(tcall(ip, port, &format!("GET /x HTTP/1.1\r\n{host}\r\n")), 200);
        // A change needs the CSRF header; a wrong Host is refused (DNS rebinding).
        assert_eq!(tcall(ip, port, &format!("POST /x HTTP/1.1\r\n{host}Content-Length: 0\r\n\r\n")), 403);
        assert_eq!(tcall(ip, port, &format!("POST /x HTTP/1.1\r\n{host}X-Ito-Csrf: 1\r\nContent-Length: 0\r\n\r\n")), 200);
        assert_eq!(tcall(ip, port, "GET /x HTTP/1.1\r\nHost: evil.example\r\n\r\n"), 403);
        assert_eq!(tcall(ip, port, "GET /x HTTP/1.1\r\n\r\n"), 403);
        // The token still works, with any Host (it is not ambient).
        assert_eq!(
            tcall(
                ip,
                port,
                &format!("POST /x HTTP/1.1\r\nHost: evil.example\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 0\r\n\r\n")
            ),
            200
        );
        srv.stop();
        let l = log.lock().unwrap();
        assert_eq!((l[0].via, l[0].who.as_deref()), (Via::Tailscale, Some("owner")));
        assert_eq!((l[1].via, l[1].status), (Via::Denied, 403));
        assert_eq!((l[5].via, l[5].who.as_deref()), (Via::Token, Some("default")));
    }

    #[test]
    fn tailnet_shuts_out_the_unlisted_and_the_unreachable() {
        let bob = tailnet::Identity { login: Some("bob@example.com".into()), ..Default::default() };
        let tagged_other = tailnet::Identity { login: None, tags: vec!["tag:other".into()], node: String::new() };
        let cases = [Fake(None, false), Fake(Some(bob), false), Fake(Some(tagged_other), false), Fake(Some(alice()), true)];
        for w in cases {
            let Some((srv, ip, _)) = tail_server(w, |_| panic!("the handler must not run")) else { return };
            let port = srv.tailnet_port().unwrap();
            assert_eq!(tcall(ip, port, &format!("GET /x HTTP/1.1\r\nHost: {ip}\r\n\r\n")), 401);
            srv.stop();
        }
    }

    #[test]
    fn tailnet_tag_gets_its_own_name_and_loopback_ignores_whois() {
        let ha = tailnet::Identity { login: None, tags: vec!["tag:ha".into()], node: String::new() };
        let Some((srv, ip, _)) = tail_server(Fake(Some(ha), false), |r| Response::ok(serde_json::json!({ "who": r.who }))) else { return };
        let (tp, lp) = (srv.tailnet_port().unwrap(), srv.port());
        assert_eq!(tcall(ip, tp, &format!("GET /x HTTP/1.1\r\nHost: {ip}\r\n\r\n")), 200);
        // The main (loopback) listener never asks Tailscale: no token, no entry.
        assert_eq!(call(lp, "GET /x HTTP/1.1\r\nHost: localhost\r\n\r\n").0, 401);
        srv.stop();
    }

    /// A Tailscale that is not up until `up` is set.
    struct Late(Arc<AtomicBool>, IpAddr);
    impl tailnet::Whois for Late {
        fn whois(&self, _: SocketAddr) -> io::Result<Option<tailnet::Identity>> {
            Ok(Some(alice()))
        }
        fn status(&self) -> io::Result<tailnet::Status> {
            if self.0.load(Ordering::Relaxed) {
                Ok(tailnet::Status { addrs: vec![self.1], names: Vec::new() })
            } else {
                Err(io::Error::new(io::ErrorKind::NotFound, "tailscaled is not running"))
            }
        }
    }

    #[test]
    fn the_main_listener_opens_while_tailscale_is_down_and_the_tailnet_one_follows() {
        let Some(ip) = tailnet::LocalApi::default().status().ok().and_then(|s| s.addrs.into_iter().find(|a| a.is_ipv4())) else {
            return;
        };
        let up = Arc::new(AtomicBool::new(false));
        let mut t = Tailnet::new(0).login("alice@example.com", "owner");
        t.whois = Arc::new(Late(up.clone(), ip));
        let mut cfg = Config::new(0, TOKEN);
        cfg.tailnet = Some(t);
        let srv = Server::start(cfg, |_| Response::ok(serde_json::Value::Null), |_| {}).unwrap();
        assert!(srv.port() != 0);
        assert_eq!(srv.tailnet_port(), None);
        assert!(srv.tailnet_error().unwrap().contains("tailscaled is not running"));
        // The main listener answers meanwhile.
        assert_eq!(call(srv.port(), &req("GET", "/x", Some(TOKEN), "")).0, 200);
        up.store(true, Ordering::Relaxed);
        let end = Instant::now() + Duration::from_secs(10);
        while srv.tailnet_port().is_none() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(100));
        }
        let port = srv.tailnet_port().expect("the tailnet listener opens once Tailscale is up");
        assert_eq!(srv.tailnet_error(), None);
        assert_eq!(tcall(ip, port, &format!("GET /x HTTP/1.1\r\nHost: {ip}:{port}\r\n\r\n")), 200);
        srv.stop();
    }

    #[test]
    fn tailnet_refuses_to_start_on_a_bad_setup() {
        let mut cfg = Config::new(0, TOKEN);
        cfg.tailnet = Some(Tailnet::new(0)); // nobody listed
        assert!(Server::start(cfg, |_| Response::ok(serde_json::Value::Null), |_| {}).is_err());
        let mut t = Tailnet::new(0).login("a@b.c", "owner");
        t.addrs = vec!["0.0.0.0".parse().unwrap()]; // not a tailnet address
        let mut cfg = Config::new(0, TOKEN);
        cfg.tailnet = Some(t);
        assert!(Server::start(cfg, |_| Response::ok(serde_json::Value::Null), |_| {}).is_err());
    }

    fn sessions_server(cfg: Config) -> Server {
        let mut cfg = cfg;
        cfg.sessions = true;
        cfg.timeout = Duration::from_secs(3);
        start_with(cfg)
    }

    fn sign_in(port: u16, token: &str) -> String {
        let out = call_text(port, &req("POST", "/session", Some(token), ""));
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        cookie_of(&out)
    }

    fn with_cookie(port: u16, cookie: &str) -> (u16, serde_json::Value) {
        call(port, &format!("GET /x HTTP/1.1\r\nCookie: {cookie}\r\n\r\n"))
    }

    fn who_with(port: u16, token: &str) -> (u16, Option<String>) {
        let (st, body) = call(port, &req("GET", "/x", Some(token), ""));
        (st, body["who"].as_str().map(String::from))
    }

    fn status_of(out: &str) -> u16 {
        out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
    }

    #[test]
    fn set_access_swaps_who_may_come_in_and_keeps_the_port() {
        let (old_hash_token, new_hash_token) = (new_token().unwrap(), new_token().unwrap());
        let cfg = Config::new(0, "old-default").token_named("n1", "n1-tok").token_hash_named("h1", token_hash(&old_hash_token));
        let srv = start_with(cfg);
        let port = srv.port();
        assert_eq!(who_with(port, "old-default"), (200, Some("default".into())));
        assert_eq!(who_with(port, &old_hash_token), (200, Some("h1".into())));
        let access = Access {
            token: "new-default".into(),
            tokens: vec![("n1".into(), "n1-tok".into()), ("n2".into(), "n2-tok".into())],
            token_hashes: vec![("h2".into(), token_hash(&new_hash_token))],
            ..Access::default()
        };
        srv.set_access(access).unwrap();
        assert_eq!(srv.port(), port);
        assert_eq!(who_with(port, "old-default").0, 401);
        assert_eq!(who_with(port, &old_hash_token).0, 401);
        assert_eq!(who_with(port, "new-default"), (200, Some("default".into())));
        assert_eq!(who_with(port, "n1-tok"), (200, Some("n1".into())));
        assert_eq!(who_with(port, "n2-tok"), (200, Some("n2".into())));
        assert_eq!(who_with(port, &new_hash_token), (200, Some("h2".into())));
    }

    #[test]
    fn a_bad_access_is_refused_and_nothing_changes() {
        let srv = sessions_server(Config::new(0, TOKEN));
        let port = srv.port();
        let cookie = sign_in(port, TOKEN);
        let good = token_hash("abc");
        let hash_case = |name: &str, hash: String| Access { token_hashes: vec![(name.into(), hash)], ..Access::default() };
        let named = "a named token has an empty name or value";
        let no_tailnet = "this server has no tailnet listener";
        let cases: Vec<(Access, &str)> = vec![
            (Access::default(), "the token is empty"),
            (Access { tokens: vec![("".into(), "x".into())], ..Access::default() }, named),
            (Access { tokens: vec![("n".into(), "".into())], ..Access::default() }, named),
            (hash_case("", good.clone()), "a token hash has an empty name"),
            (hash_case("alice", good[..63].to_string()), "not 64 lowercase hex characters"),
            (hash_case("alice", good.to_uppercase()), "not 64 lowercase hex characters"),
            (hash_case("alice", token_hash("")), "the hash of an empty token"),
            (Access { token: TOKEN.into(), tailnet_logins: vec![("a@b.c".into(), "owner".into())], ..Access::default() }, no_tailnet),
            (Access { token: TOKEN.into(), tailnet_tags: vec![("tag:x".into(), "x".into())], ..Access::default() }, no_tailnet),
        ];
        for (access, message) in cases {
            let err = srv.set_access(access.clone()).expect_err(message);
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{message}");
            assert!(err.to_string().contains(message), "{err}");
            for (_, h) in &access.token_hashes {
                assert!(!err.to_string().contains(h.as_str()), "{err}");
            }
            assert_eq!(who_with(port, TOKEN), (200, Some("default".into())), "{message}");
            assert_eq!(with_cookie(port, &cookie).0, 200, "{message}");
        }
    }

    #[test]
    fn sessions_stay_only_while_their_token_signs_in_as_the_same_name() {
        let carol = new_token().unwrap();
        let cfg = Config::new(0, "dan-tok")
            .token_named("alice", "a-tok")
            .token_named("bob", "b-tok")
            .token_named("erin", "e-tok")
            .token_hash_named("carol", token_hash(&carol));
        let srv = sessions_server(cfg);
        let port = srv.port();
        let signed: [(&str, &str); 5] =
            [("alice", "a-tok"), ("bob", "b-tok"), ("carol", carol.as_str()), ("dan", "dan-tok"), ("erin", "e-tok")];
        let cookies: Vec<(&str, String)> = signed.iter().map(|(who, tok)| (*who, sign_in(port, tok))).collect();
        let access = Access {
            token: "dan-tok".into(),
            tokens: vec![("alice".into(), "a-tok".into()), ("bob".into(), "b-new".into())],
            token_hashes: vec![("erin".into(), token_hash("e-tok"))],
            ..Access::default()
        };
        srv.set_access(access).unwrap();
        let status = |who: &str| with_cookie(port, &cookies.iter().find(|(w, _)| *w == who).unwrap().1).0;
        assert_eq!((status("alice"), status("dan"), status("erin")), (200, 200, 200));
        assert_eq!((status("bob"), status("carol")), (401, 401));
        // A kept session still stands for the same name.
        let (_, body) = with_cookie(port, &cookies[4].1);
        assert_eq!(body["who"].as_str(), Some("erin"));
        // Bob's old token is refused; he signs in again with the new one.
        assert_eq!(status_of(&call_text(port, &req("POST", "/session", Some("b-tok"), ""))), 401);
        let again = sign_in(port, "b-new");
        assert_eq!(with_cookie(port, &again).0, 200);
    }

    #[test]
    fn a_session_ends_when_its_token_now_signs_in_under_another_name() {
        let srv = sessions_server(Config::new(0, "").token_named("alice", "t-tok"));
        let port = srv.port();
        let cookie = sign_in(port, "t-tok");
        assert_eq!(with_cookie(port, &cookie).0, 200);
        // The same token, now also the default token, which comes first: it signs in as "default".
        let access = Access { token: "t-tok".into(), tokens: vec![("alice".into(), "t-tok".into())], ..Access::default() };
        srv.set_access(access).unwrap();
        assert_eq!(with_cookie(port, &cookie).0, 401);
        assert_eq!(who_with(port, "t-tok"), (200, Some("default".into())));
    }

    #[test]
    fn a_sign_in_whose_token_was_taken_away_makes_no_session() {
        let srv = sessions_server(Config::new(0, "").token_named("alice", "a-tok"));
        let port = srv.port();
        let cookie = sign_in(port, "a-tok");
        // (a) a session, then an access without its name: the cookie is dead.
        srv.set_access(Access { tokens: vec![("bob".into(), "b-tok".into())], ..Access::default() }).unwrap();
        assert_eq!(with_cookie(port, &cookie).0, 401);
        srv.set_access(Access { tokens: vec![("alice".into(), "a-tok".into())], ..Access::default() }).unwrap();
        // (b) a (who, hash) that the current access does not sign in as: nothing is added and nothing evicted.
        let sh = &srv.shared;
        {
            let mut map = sh.sessions.lock().unwrap();
            map.clear();
            let base = Instant::now() + Duration::from_secs(3600);
            for i in 0..MAX_SESSIONS {
                let until = base + Duration::from_secs(i as u64);
                map.insert(format!("id{i}"), Session { who: "alice".into(), hash: token_hash("a-tok"), until });
            }
        }
        assert_eq!(new_session(sh, "alice", &token_hash("other")).unwrap(), None);
        assert_eq!(new_session(sh, "bob", &token_hash("a-tok")).unwrap(), None);
        {
            let map = sh.sessions.lock().unwrap();
            assert_eq!(map.len(), MAX_SESSIONS);
            assert!(map.contains_key("id0"));
        }
        // A valid pair still gets in (and evicts the oldest, as before).
        assert!(new_session(sh, "alice", &token_hash("a-tok")).unwrap().is_some());
        let map = sh.sessions.lock().unwrap();
        assert_eq!(map.len(), MAX_SESSIONS);
        assert!(!map.contains_key("id0"));
    }

    #[test]
    fn a_request_in_flight_finishes_with_the_access_that_let_it_in() {
        let mut cfg = Config::new(0, "").token_named("old", "old-tok").token_named("keep", "keep-tok");
        cfg.timeout = Duration::from_secs(5);
        let srv = start_with(cfg);
        let port = srv.port();
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s.write_all(b"POST /x HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer old-tok\r\nContent-Length: 10\r\n\r\n12345").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        srv.set_access(Access { tokens: vec![("keep".into(), "keep-tok".into())], ..Access::default() }).unwrap();
        s.write_all(b"67890").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert!(out.contains(r#""who":"old""#), "{out}");
        assert_eq!(who_with(port, "old-tok").0, 401);
        assert_eq!(who_with(port, "keep-tok"), (200, Some("keep".into())));
    }

    fn tcall_body(ip: IpAddr, port: u16, raw: &str) -> (u16, serde_json::Value) {
        let mut s = TcpStream::connect((ip, port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let body = out.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        (status_of(&out), serde_json::from_str(body).unwrap_or(serde_json::Value::Null))
    }

    fn tail_access(logins: &[(&str, &str)], tags: &[(&str, &str)]) -> Access {
        let own = |l: &[(&str, &str)]| l.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        Access { token: TOKEN.into(), tailnet_logins: own(logins), tailnet_tags: own(tags), ..Access::default() }
    }

    #[test]
    fn set_access_swaps_the_tailnet_logins_and_tags() {
        let who_handler = |r: Request| Response::ok(serde_json::json!({ "who": r.who }));
        // A login that is let in, then removed; the listener stays where it is.
        let Some((srv, ip, _)) = tail_server(Fake(Some(alice()), false), who_handler) else { return };
        let port = srv.tailnet_port().unwrap();
        let get = format!("GET /x HTTP/1.1\r\nHost: {ip}:{port}\r\n\r\n");
        assert_eq!(tcall_body(ip, port, &get).1["who"].as_str(), Some("owner"));
        srv.set_access(tail_access(&[], &[("tag:ha", "standby")])).unwrap();
        assert_eq!(tcall(ip, port, &get), 401);
        assert_eq!(srv.tailnet_port(), Some(port));
        // The token still works there, and logins and tags both empty is refused.
        let with_token = format!("GET /x HTTP/1.1\r\nHost: {ip}\r\nAuthorization: Bearer {TOKEN}\r\n\r\n");
        assert_eq!(tcall(ip, port, &with_token), 200);
        let err = srv.set_access(tail_access(&[], &[])).unwrap_err().to_string();
        assert!(err.contains("the tailnet listener has no login or tag to let in"), "{err}");
        let err = srv.set_access(tail_access(&[("", "x")], &[])).unwrap_err().to_string();
        assert!(err.contains("a tailnet login or tag has an empty name"), "{err}");
        assert_eq!(tcall(ip, port, &with_token), 200);
        srv.stop();

        // A tag renamed to another `who`.
        let ha = tailnet::Identity { login: None, tags: vec!["tag:ha".into()], node: String::new() };
        let Some((srv, ip, _)) = tail_server(Fake(Some(ha), false), who_handler) else { return };
        let port = srv.tailnet_port().unwrap();
        let get = format!("GET /x HTTP/1.1\r\nHost: {ip}:{port}\r\n\r\n");
        assert_eq!(tcall_body(ip, port, &get).1["who"].as_str(), Some("standby"));
        srv.set_access(tail_access(&[], &[("tag:ha", "renamed")])).unwrap();
        assert_eq!(tcall_body(ip, port, &get).1["who"].as_str(), Some("renamed"));
        srv.stop();

        // A tagged node whose login is also listed is still judged by its tags only.
        let both = tailnet::Identity { login: Some("alice@example.com".into()), tags: vec!["tag:other".into()], node: String::new() };
        let Some((srv, ip, _)) = tail_server(Fake(Some(both), false), who_handler) else { return };
        let port = srv.tailnet_port().unwrap();
        srv.set_access(tail_access(&[("alice@example.com", "owner")], &[("tag:ha", "standby")])).unwrap();
        assert_eq!(tcall(ip, port, &format!("GET /x HTTP/1.1\r\nHost: {ip}:{port}\r\n\r\n")), 401);
        srv.stop();
    }

    #[test]
    fn access_config_and_auth_debug_hide_the_secrets() {
        let (hash, cookie) = (token_hash("sentinel"), "SENTINEL-COOKIE");
        let access = Access {
            token: "SENTINEL-DEFAULT".into(),
            tokens: vec![("viewer".into(), "SENTINEL-NAMED".into())],
            token_hashes: vec![("alice".into(), hash.clone())],
            tailnet_logins: vec![("a@b.c".into(), "owner".into())],
            tailnet_tags: vec![("tag:x".into(), "standby".into())],
        };
        let shown = format!("{access:?}");
        for secret in ["SENTINEL-DEFAULT", "SENTINEL-NAMED", hash.as_str()] {
            assert!(!shown.contains(secret), "{shown}");
        }
        for name in ["viewer", "alice", "a@b.c", "owner", "tag:x", "standby"] {
            assert!(shown.contains(name), "{shown}");
        }
        let cfg = Config::new(0, "SENTINEL-DEFAULT").token_named("viewer", "SENTINEL-NAMED").token_hash_named("alice", hash.clone());
        let shown = format!("{cfg:?}");
        assert!(!shown.contains("SENTINEL") && !shown.contains(&hash), "{shown}");
        let shown = format!("{:?} {:?}", Auth::Bearer(hash.clone()), Auth::Cookie(cookie.into()));
        assert!(!shown.contains(&hash) && !shown.contains(cookie), "{shown}");
    }
}
