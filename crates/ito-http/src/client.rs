//! An HTTPS client that trusts one certificate: the one whose SHA-256 the person was shown. No CA is asked, and the
//! name in the address is not checked; the fingerprint is the whole identity. Every connect, read and write has a
//! timeout, and a whole call can have a deadline. One request per connection, JSON in and out, a bearer token.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::tls::{self, Fingerprint};

/// The default for [`Client::max_reply`].
const MAX_BODY: usize = 4 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The address is not `https://host[:port]`.
    Url(String),
    /// The host is not reachable, or the connection broke or timed out.
    Connect(String),
    /// The TLS handshake failed for a reason other than the fingerprint.
    Tls(String),
    /// The server's certificate is not the pinned one. The fingerprint it showed is here, for the person to compare.
    Fingerprint(String),
    /// The answer is not an HTTP/1.1 answer, or is too large.
    Bad(String),
    /// The call's [`Client::deadline`] passed before it finished. A request that changes something may or may not have been
    /// carried out.
    Deadline,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Url(s) => write!(f, "bad address: {s}"),
            Error::Connect(s) => write!(f, "cannot reach the server: {s}"),
            Error::Tls(s) => write!(f, "TLS failed: {s}"),
            Error::Fingerprint(got) => write!(f, "the server's certificate is not the one trusted (it is {got})"),
            Error::Bad(s) => write!(f, "bad answer: {s}"),
            Error::Deadline => f.write_str("the call did not finish in time; whether the server acted on it is unknown"),
        }
    }
}

impl std::error::Error for Error {}

/// An answer: the status and the body parsed as JSON (`Null` when there is no body or it is not JSON).
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub json: serde_json::Value,
}

/// A server whose certificate is pinned.
#[derive(Clone)]
pub struct Client {
    host: String,
    port: u16,
    fingerprint: Fingerprint,
    token: String,
    /// Longest wait to connect, and for each read or write (default 10 s).
    pub timeout: Duration,
    /// The longest the whole call may wait: name lookup, connect, TLS, sending and receiving. `None` (the default) means only
    /// `timeout` on each step. Turning the JSON into bytes and back has no wait in it and is not cut off.
    pub deadline: Option<Duration>,
    /// Largest answer body accepted (default 4 MiB). The whole answer is held in memory, so raise it only for what is known to answer more.
    pub max_reply: usize,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client").field("host", &self.host).field("port", &self.port).field("token", &"***").finish_non_exhaustive()
    }
}

impl Client {
    /// `url` is `https://host[:port]` (443 by default); `fingerprint` is written as [`tls::format`] does (colons, spaces
    /// and case do not matter).
    pub fn new(url: &str, fingerprint: &str, token: impl Into<String>) -> Result<Client, Error> {
        let (host, port) = split_url(url)?;
        let fingerprint = tls::parse(fingerprint).ok_or_else(|| Error::Url("the fingerprint is not 32 bytes of hex".to_owned()))?;
        Ok(Client { host, port, fingerprint, token: token.into(), timeout: Duration::from_secs(10), deadline: None, max_reply: MAX_BODY })
    }

    pub fn get(&self, path: &str) -> Result<Reply, Error> {
        self.send("GET", path, None)
    }

    pub fn post_json(&self, path: &str, json: &serde_json::Value) -> Result<Reply, Error> {
        self.send("POST", path, Some(json))
    }

    fn send(&self, method: &str, path: &str, json: Option<&serde_json::Value>) -> Result<Reply, Error> {
        if path.contains(['\r', '\n', ' ']) || !path.starts_with('/') {
            return Err(Error::Url("the path must start with / and have no spaces".to_owned()));
        }
        let body = json.map(|j| serde_json::to_vec(j).unwrap_or_default());
        // An absurdly long deadline is no deadline.
        let due = self.deadline.and_then(|d| Instant::now().checked_add(d));
        let (mut stream, _) = connect(&self.host, self.port, Some(self.fingerprint), self.timeout, due)?;
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\nAccept: application/json\r\n",
            self.host, self.port, self.token
        );
        if let Some(b) = &body {
            head += &format!("Content-Type: application/json\r\nContent-Length: {}\r\n", b.len());
        }
        head += "\r\n";
        let wrote =
            stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body.as_deref().unwrap_or(&[]))).and_then(|_| stream.flush());
        if let Err(e) = wrote {
            return Err(from_io(e, due));
        }
        let mut raw = Vec::new();
        let mut limited = (&mut stream).take((self.max_reply as u64).saturating_add(16 * 1024));
        match limited.read_to_end(&mut raw) {
            Ok(_) => {}
            // A server that closes without close_notify after a complete answer is still a complete answer.
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof && !raw.is_empty() => {}
            Err(e) => return Err(from_io(e, due)),
        }
        parse_reply(&raw, self.max_reply)
    }
}

/// The fingerprint of whatever certificate `url` (`https://host[:port]`) shows, to put in front of the person once
/// (trust on first use). Nothing is sent after the handshake.
pub fn peek_fingerprint(url: &str, timeout: Duration) -> Result<String, Error> {
    let (host, port) = split_url(url)?;
    let (_, seen) = connect(&host, port, None, timeout, None)?;
    let fp = seen.lock().ok().and_then(|g| *g).ok_or_else(|| Error::Tls("the server showed no certificate".to_owned()))?;
    Ok(tls::format(&fp))
}

type Seen = Arc<Mutex<Option<Fingerprint>>>;
type Stream = rustls::StreamOwned<rustls::ClientConnection, Timed>;

/// A socket wait armed to the deadline may end a timer tick early.
const DEADLINE_SLACK: Duration = Duration::from_millis(50);

/// True when `e` is a wait that ran out with the deadline at hand. Only a timeout counts: a refused connection stays what it is,
/// however late it comes.
fn ran_out(e: &io::Error, due: Option<Instant>) -> bool {
    matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) && due.is_some_and(|d| Instant::now() + DEADLINE_SLACK >= d)
}

/// How long the next step may wait: `timeout`, or less when the deadline is nearer. `Err(Error::Deadline)` once `due` is past.
fn step(timeout: Duration, due: Option<Instant>) -> Result<Duration, Error> {
    match due {
        None => Ok(timeout),
        Some(d) => {
            let left = d.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(Error::Deadline)
            } else {
                Ok(timeout.min(left))
            }
        }
    }
}

/// `work` on a thread of its own, because the standard library cannot limit a name lookup. A lookup still stuck at the deadline is
/// left to end by itself when the system's own lookup timeout ends it; its answer goes nowhere.
fn within<T: Send + 'static>(due: Instant, work: impl FnOnce() -> T + Send + 'static) -> Result<T, Error> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("ito-http-lookup".to_owned())
        .spawn(move || {
            let _ = tx.send(work());
        })
        .map_err(|e| Error::Connect(e.to_string()))?;
    match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
        Ok(v) => Ok(v),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::Deadline),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Connect("the name lookup ended without an answer".to_owned())),
    }
}

/// The socket under TLS. With a deadline, each read and write first arms its socket timeout to what is left of it.
struct Timed {
    tcp: TcpStream,
    timeout: Duration,
    due: Option<Instant>,
}

impl Timed {
    /// The wait for the next step, or `None` when no deadline is set (the timeouts set in `connect` stand).
    fn left(&self) -> io::Result<Option<Duration>> {
        match self.due {
            None => Ok(None),
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    Err(io::ErrorKind::TimedOut.into())
                } else {
                    Ok(Some(self.timeout.min(left)))
                }
            }
        }
    }
}

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(t) = self.left()? {
            self.tcp.set_read_timeout(Some(t))?;
        }
        self.tcp.read(buf)
    }
}

impl Write for Timed {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(t) = self.left()? {
            self.tcp.set_write_timeout(Some(t))?;
        }
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

fn split_url(url: &str) -> Result<(String, u16), Error> {
    let bad = || Error::Url(format!("{url} is not https://host[:port]"));
    let rest = url.strip_prefix("https://").ok_or_else(bad)?.trim_end_matches('/');
    if rest.is_empty() || rest.contains(['/', '?', '#', '@', ' ']) {
        return Err(bad());
    }
    // `[::1]:8443`, `host:8443`, `host`.
    if let Some(v6) = rest.strip_prefix('[') {
        let (host, tail) = v6.split_once(']').ok_or_else(bad)?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| bad())?,
            None if tail.is_empty() => 443,
            None => return Err(bad()),
        };
        return Ok((host.to_owned(), port));
    }
    match rest.rsplit_once(':') {
        Some((host, p)) if !host.is_empty() => Ok((host.to_owned(), p.parse().map_err(|_| bad())?)),
        _ => Ok((rest.to_owned(), 443)),
    }
}

fn connect(host: &str, port: u16, pin: Option<Fingerprint>, timeout: Duration, due: Option<Instant>) -> Result<(Stream, Seen), Error> {
    let looked_up: io::Result<Vec<SocketAddr>> = match due {
        Some(d) => {
            let host = host.to_owned();
            within(d, move || (host.as_str(), port).to_socket_addrs().map(Iterator::collect))?
        }
        None => (host, port).to_socket_addrs().map(Iterator::collect),
    };
    let addrs = looked_up.map_err(|e| if ran_out(&e, due) { Error::Deadline } else { Error::Connect(e.to_string()) })?;
    let mut last = None;
    let mut tcp = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, step(timeout, due)?) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let Some(tcp) = tcp else {
        return Err(match last {
            Some(e) if ran_out(&e, due) => Error::Deadline,
            Some(e) => Error::Connect(e.to_string()),
            None => Error::Connect(format!("{host} has no address")),
        });
    };
    tcp.set_read_timeout(Some(timeout)).map_err(|e| Error::Connect(e.to_string()))?;
    tcp.set_write_timeout(Some(timeout)).map_err(|e| Error::Connect(e.to_string()))?;
    let provider = tls::provider();
    let seen: Seen = Arc::new(Mutex::new(None));
    let verifier = Arc::new(Pin { want: pin, seen: seen.clone(), algs: provider.signature_verification_algorithms });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_owned()).map_err(|e| Error::Tls(e.to_string()))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| Error::Tls(e.to_string()))?;
    let mut tcp = Timed { tcp, timeout, due };
    while conn.is_handshaking() {
        if let Err(e) = conn.complete_io(&mut tcp) {
            return Err(handshake_error(e, &seen, due));
        }
    }
    Ok((rustls::StreamOwned::new(conn, tcp), seen))
}

/// A handshake that stopped because the fingerprint differs says so, with what the server showed.
fn handshake_error(e: io::Error, seen: &Seen, due: Option<Instant>) -> Error {
    if ran_out(&e, due) {
        return Error::Deadline;
    }
    let differs = e.to_string().contains(DIFFERS);
    match (differs, seen.lock().ok().and_then(|g| *g)) {
        (true, Some(got)) => Error::Fingerprint(tls::format(&got)),
        _ if matches!(
            e.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof
        ) =>
        {
            Error::Connect(e.to_string())
        }
        _ => Error::Tls(e.to_string()),
    }
}

fn from_io(e: io::Error, due: Option<Instant>) -> Error {
    if ran_out(&e, due) {
        return Error::Deadline;
    }
    Error::Connect(e.to_string())
}

const DIFFERS: &str = "certificate fingerprint differs";

#[derive(Debug)]
struct Pin {
    /// `None` accepts any certificate (only [`peek_fingerprint`], which sends nothing).
    want: Option<Fingerprint>,
    seen: Seen,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let got = tls::sha256(end_entity);
        if let Ok(mut s) = self.seen.lock() {
            *s = Some(got);
        }
        match self.want {
            Some(want) if !ct_eq(&want, &got) => Err(rustls::Error::General(DIFFERS.to_owned())),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn parse_reply(raw: &[u8], max: usize) -> Result<Reply, Error> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| Error::Bad("no end of the head".to_owned()))?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| Error::Bad("the head is not UTF-8".to_owned()))?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| Error::Bad("no status".to_owned()))?;
    let mut body = &raw[end + 4..];
    let length = head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().ok()).flatten()
    });
    let too_large = || Error::Bad(format!("the body is too large (over {max} bytes)"));
    if let Some(n) = length {
        if n > max {
            return Err(too_large());
        }
        if body.len() < n {
            return Err(Error::Bad("the body was cut short".to_owned()));
        }
        body = &body[..n];
    }
    if body.len() > max {
        return Err(too_large());
    }
    Ok(Reply { status, json: serde_json::from_slice(body).unwrap_or(serde_json::Value::Null) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_split_into_host_and_port() {
        assert_eq!(split_url("https://pc1.lan:8443").unwrap(), ("pc1.lan".to_owned(), 8443));
        assert_eq!(split_url("https://192.168.0.5/").unwrap(), ("192.168.0.5".to_owned(), 443));
        assert_eq!(split_url("https://[::1]:9").unwrap(), ("::1".to_owned(), 9));
        for bad in ["http://x:1", "https://", "https://x/path", "https://u@x", "https://x:notaport", "x:1"] {
            assert!(matches!(split_url(bad), Err(Error::Url(_))), "{bad}");
        }
    }

    #[test]
    fn replies_are_read_by_content_length() {
        let r = parse_reply(b"HTTP/1.1 201 Created\r\nContent-Length: 8\r\n\r\n{\"a\":1}\n??", MAX_BODY).unwrap();
        assert_eq!((r.status, r.json), (201, serde_json::json!({ "a": 1 })));
        assert!(parse_reply(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n{}", MAX_BODY).is_err());
        assert_eq!(parse_reply(b"HTTP/1.1 204 No Content\r\n\r\n", MAX_BODY).unwrap().json, serde_json::Value::Null);
        assert!(parse_reply(b"garbage", MAX_BODY).is_err());
    }

    use crate::{Bind, Config, Request, Response, Server, TlsCfg};
    use std::net::{IpAddr, Ipv4Addr};
    use std::path::PathBuf;

    const TOKEN: &str = "lan-token";

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-http-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn lan(cert_dir: &PathBuf, f: impl Fn(Request) -> Response + Send + Sync + 'static) -> Server {
        let mut cfg = Config::new(0, TOKEN);
        cfg.bind = Bind::Lan { addr: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)), tls: TlsCfg::new(cert_dir) };
        cfg.timeout = Duration::from_secs(3);
        Server::start(cfg, f, |_| {}).unwrap()
    }

    #[test]
    fn a_client_that_pins_the_fingerprint_talks_to_the_lan_listener() {
        let d = dir("pin");
        let srv = lan(&d, |r| Response::ok(serde_json::json!({ "path": r.path, "echo": r.json::<serde_json::Value>().unwrap() })));
        let fp = tls::format(&srv.fingerprint().unwrap());
        let url = format!("https://127.0.0.1:{}", srv.port());

        let c = Client::new(&url, &fp, TOKEN).unwrap();
        let r = c.get("/health").unwrap();
        assert_eq!((r.status, r.json["path"].as_str()), (200, Some("/health")));
        let r = c.post_json("/runs", &serde_json::json!({ "net": "月次" })).unwrap();
        assert_eq!(r.json["echo"], serde_json::json!({ "net": "月次" }));
        // The person may type the fingerprint any way.
        assert!(Client::new(&url, &fp.replace(':', "").to_lowercase(), TOKEN).unwrap().get("/").is_ok());

        // The token still counts over TLS.
        assert_eq!(Client::new(&url, &fp, "wrong").unwrap().get("/").unwrap().status, 401);
        // A certificate that is not the pinned one is refused, and the one shown is reported.
        let other = Client::new(&url, &"00".repeat(32), TOKEN).unwrap();
        assert_eq!(other.get("/").unwrap_err(), Error::Fingerprint(fp.clone()));
        // First use: ask what the server shows.
        assert_eq!(peek_fingerprint(&url, Duration::from_secs(3)).unwrap(), fp);
        srv.stop();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_certificate_is_made_once_and_kept() {
        let d = dir("kept");
        let first = tls::fingerprint(&d).unwrap();
        assert_eq!(tls::fingerprint(&d).unwrap(), first);
        let srv = lan(&d, |_| Response::ok(serde_json::Value::Null));
        assert_eq!(srv.fingerprint(), Some(first));
        srv.stop();
        // Without its key the certificate is made again.
        std::fs::remove_file(d.join("ito-http-key.der")).unwrap();
        assert_ne!(tls::fingerprint(&d).unwrap(), first);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plain_http_to_the_tls_port_gets_nothing() {
        let d = dir("plain");
        let called = Arc::new(Mutex::new(false));
        let c = called.clone();
        let srv = lan(&d, move |_| {
            *c.lock().unwrap() = true;
            Response::ok(serde_json::Value::Null)
        });
        let mut s = TcpStream::connect(("127.0.0.1", srv.port())).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = s.write_all(format!("GET / HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n").as_bytes());
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        assert!(!String::from_utf8_lossy(&out).contains("HTTP/1.1 200"));
        assert!(!*called.lock().unwrap());
        srv.stop();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn fingerprints_are_written_and_read_both_ways() {
        let fp = tls::sha256(b"x");
        assert_eq!(tls::parse(&tls::format(&fp)), Some(fp));
        assert_eq!(tls::parse(&"ab".repeat(32)), tls::parse(&"AB:".repeat(32)[..95]));
        assert_eq!(tls::parse("ab"), None);
        assert_eq!(tls::parse(&"zz".repeat(32)), None);
    }

    #[test]
    fn a_bad_fingerprint_is_refused_up_front() {
        assert!(Client::new("https://x:1", "nothex", "t").is_err());
        assert!(Client::new("https://x:1", &"ab".repeat(32), "t").is_ok());
    }

    #[test]
    fn the_reply_limit_is_exact_and_names_itself() {
        let ok = |n: usize| format!("HTTP/1.1 200 OK\r\nContent-Length: {n}\r\n\r\n{}", "a".repeat(n));
        let plain = |n: usize| format!("HTTP/1.1 200 OK\r\n\r\n{}", "a".repeat(n));
        assert!(parse_reply(ok(10).as_bytes(), 10).is_ok());
        assert!(parse_reply(plain(10).as_bytes(), 10).is_ok());
        for raw in [ok(11), plain(11)] {
            match parse_reply(raw.as_bytes(), 10) {
                Err(Error::Bad(m)) => assert!(m.contains("too large") && m.contains("10"), "{m}"),
                other => panic!("{other:?}"),
            }
        }
        // A Content-Length over the limit is "too large" even when the body that came is short.
        match parse_reply(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\nabc", 10) {
            Err(Error::Bad(m)) => assert!(m.contains("too large"), "{m}"),
            other => panic!("{other:?}"),
        }
        match parse_reply(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nabc", 10) {
            Err(Error::Bad(m)) => assert!(m.contains("cut short"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_big_answer_needs_max_reply() {
        let d = dir("big");
        let size: usize = 5 << 20;
        let srv = lan(&d, move |_| Response::ok(serde_json::json!({ "x": "a".repeat(size) })));
        let url = format!("https://127.0.0.1:{}", srv.port());
        let fp = tls::format(&srv.fingerprint().unwrap());
        let mut c = Client::new(&url, &fp, TOKEN).unwrap();
        assert_eq!(c.max_reply, MAX_BODY);
        match c.get("/") {
            Err(Error::Bad(m)) => assert!(m.contains("too large"), "{m}"),
            other => panic!("{other:?}"),
        }
        c.max_reply = 8 << 20;
        let r = c.get("/").unwrap();
        assert_eq!(r.json["x"].as_str().map(str::len), Some(size));
        srv.stop();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn within_returns_the_value_or_the_deadline() {
        let now = Instant::now();
        assert_eq!(within(now + Duration::from_secs(5), || 7).unwrap(), 7);
        let t = Instant::now();
        let r = within(Instant::now() + Duration::from_millis(100), || std::thread::sleep(Duration::from_secs(5)));
        assert_eq!(r.unwrap_err(), Error::Deadline);
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn ran_out_counts_only_timeouts_at_the_deadline() {
        let past = Some(Instant::now() - Duration::from_secs(1));
        let far = Some(Instant::now() + Duration::from_secs(60));
        let refused = io::Error::from(io::ErrorKind::ConnectionRefused);
        let timed = || io::Error::from(io::ErrorKind::TimedOut);
        assert!(!ran_out(&refused, past));
        assert!(ran_out(&timed(), past));
        assert!(!ran_out(&timed(), None));
        assert!(!ran_out(&timed(), far));
    }

    #[test]
    fn a_trickling_server_cannot_hold_a_read_past_the_deadline() {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let writer = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            // Ends with a write error once the reader is dropped.
            while s.write_all(b"x").is_ok() {
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut one = [0u8; 1];
        tcp.read_exact(&mut one).unwrap();
        let t = Instant::now();
        let mut timed = Timed { tcp, timeout: Duration::from_secs(10), due: Some(Instant::now() + Duration::from_millis(200)) };
        let mut buf = [0u8; 64];
        let err = loop {
            if let Err(e) = timed.read(&mut buf) {
                break e;
            }
        };
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(t.elapsed() < Duration::from_secs(5));
        drop(timed);
        writer.join().unwrap();
    }

    #[test]
    fn a_server_that_never_answers_ends_at_the_deadline() {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut c = Client::new(&format!("https://127.0.0.1:{}", l.local_addr().unwrap().port()), &"ab".repeat(32), TOKEN).unwrap();
        c.timeout = Duration::from_secs(30);
        c.deadline = Some(Duration::from_millis(300));
        let t = Instant::now();
        assert_eq!(c.get("/").unwrap_err(), Error::Deadline);
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_connection_is_closed_when_the_deadline_ends_a_call() {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut buf = [0u8; 1024];
            loop {
                match s.read(&mut buf) {
                    Ok(0) => return true,
                    Ok(_) => {}
                    Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => return false,
                    // A reset or an abort is the peer closing too.
                    Err(_) => return true,
                }
            }
        });
        let mut c = Client::new(&format!("https://127.0.0.1:{port}"), &"ab".repeat(32), TOKEN).unwrap();
        c.timeout = Duration::from_secs(30);
        c.deadline = Some(Duration::from_millis(300));
        assert_eq!(c.get("/").unwrap_err(), Error::Deadline);
        assert!(server.join().unwrap(), "the client left the connection open after the deadline");
    }

    #[test]
    fn a_slow_handler_ends_at_the_deadline_and_the_server_still_stops() {
        let d = dir("slow");
        let srv = lan(&d, |_| {
            std::thread::sleep(Duration::from_millis(1500));
            Response::ok(serde_json::Value::Null)
        });
        let url = format!("https://127.0.0.1:{}", srv.port());
        let mut c = Client::new(&url, &tls::format(&srv.fingerprint().unwrap()), TOKEN).unwrap();
        c.deadline = Some(Duration::from_millis(300));
        let t = Instant::now();
        assert_eq!(c.get("/").unwrap_err(), Error::Deadline);
        assert!(t.elapsed() < Duration::from_secs(5));
        srv.stop();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_closed_port_is_not_a_deadline() {
        let port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            l.local_addr().unwrap().port()
        };
        let mut c = Client::new(&format!("https://127.0.0.1:{port}"), &"ab".repeat(32), TOKEN).unwrap();
        c.deadline = Some(Duration::from_secs(30));
        let e = c.get("/").unwrap_err();
        assert!(matches!(e, Error::Connect(_) | Error::Tls(_)), "{e:?}");
    }

    #[test]
    fn a_long_deadline_changes_nothing_for_a_normal_call() {
        let d = dir("long");
        let srv = lan(&d, |_| Response::ok(serde_json::json!({ "ok": true })));
        let url = format!("https://127.0.0.1:{}", srv.port());
        let mut c = Client::new(&url, &tls::format(&srv.fingerprint().unwrap()), TOKEN).unwrap();
        for dl in [Duration::from_secs(30), Duration::MAX] {
            c.deadline = Some(dl);
            assert_eq!(c.get("/").unwrap().json["ok"], serde_json::json!(true));
        }
        srv.stop();
        let _ = std::fs::remove_dir_all(&d);
    }
}
