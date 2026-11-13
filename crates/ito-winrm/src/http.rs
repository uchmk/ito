//! A small HTTP/1.1 client for one WinRM endpoint: a kept-alive connection (plain or TLS with a pinned certificate),
//! `POST` with a SOAP body, the reply read by `Content-Length` or chunked.

use crate::{fingerprint, ntlm, Error};
use base64::Engine;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Seen = Arc<Mutex<Option<[u8; 32]>>>;

enum Conn {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Conn {
    fn tcp(&self) -> &TcpStream {
        match self {
            Conn::Plain(s) => s,
            Conn::Tls(s) => &s.sock,
        }
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(buf),
            Conn::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            Conn::Tls(s) => s.flush(),
        }
    }
}

/// How to reach the endpoint.
pub(crate) struct Endpoint {
    pub host: String,
    pub port: u16,
    pub https: bool,
    pub path: String,
    /// The certificate's SHA-256, when pinned. `None` checks the certificate against the public roots.
    pub pin: Option<[u8; 32]>,
    pub connect_timeout: Duration,
    pub credential: Credential,
}

/// How requests sign in.
pub(crate) enum Credential {
    /// The `Authorization` header value, sent with every request.
    Basic(String),
    /// NTLMv2 once per connection (`Negotiate` / `NTLM` handshake), then nothing.
    Ntlm { user: String, password: String },
}

pub(crate) struct Http<'a> {
    ep: &'a Endpoint,
    stop: &'a AtomicBool,
    conn: Option<Conn>,
    /// The connection has signed in (NTLM): its requests carry no `Authorization`.
    signed: bool,
}

/// A reply: status, the `WWW-Authenticate` lines and body text.
pub(crate) struct Reply {
    pub status: u16,
    pub www_authenticate: Vec<String>,
    pub body: String,
}

pub(crate) fn basic(user: &str, password: &str) -> String {
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}")))
}

impl<'a> Http<'a> {
    pub fn new(ep: &'a Endpoint, stop: &'a AtomicBool) -> Self {
        Http { ep, stop, conn: None, signed: false }
    }

    /// Send `body` and read the reply, waiting up to `wait` for it. A kept connection that the server closed is
    /// opened again once.
    pub fn post(&mut self, body: &str, wait: Duration) -> Result<Reply, Error> {
        for attempt in 0..2 {
            let reused = self.conn.is_some();
            if self.conn.is_none() {
                self.conn = Some(connect(self.ep, self.stop)?);
                self.signed = false;
            }
            match self.sign_in_and_send(body, wait) {
                Ok(r) => return Ok(r),
                Err(Error::Lost(_)) if reused && attempt == 0 => self.conn = None,
                Err(e) => {
                    self.conn = None;
                    return Err(e);
                }
            }
        }
        Err(Error::Lost("the connection kept closing".to_owned()))
    }

    /// Basic: every request carries the header. NTLM: the first request of a connection goes through the handshake.
    fn sign_in_and_send(&mut self, body: &str, wait: Duration) -> Result<Reply, Error> {
        let (user, password) = match &self.ep.credential {
            Credential::Basic(h) => return self.round(Some(h), body, wait),
            Credential::Ntlm { user, password } => (user, password),
        };
        if self.signed {
            return self.round(None, body, wait);
        }
        let b64 = base64::engine::general_purpose::STANDARD;
        let first = self.round(Some(&format!("Negotiate {}", b64.encode(ntlm::negotiate()))), "", wait)?;
        let (scheme, token) = first
            .www_authenticate
            .iter()
            .find_map(|h| {
                let (s, t) = h.split_once(' ')?;
                (s.eq_ignore_ascii_case("negotiate") || s.eq_ignore_ascii_case("ntlm")).then(|| (s.to_owned(), t.trim().to_owned()))
            })
            .ok_or_else(|| Error::Other("the server does not offer NTLM or Negotiate".to_owned()))?;
        let challenge = b64
            .decode(token)
            .ok()
            .and_then(|m| ntlm::parse_challenge(&m))
            .ok_or_else(|| Error::Other("the server's NTLM challenge could not be read".to_owned()))?;
        let mut client = [0u8; 8];
        getrandom::fill(&mut client).map_err(|e| Error::Other(format!("no random numbers: {e}")))?;
        let token = b64.encode(ntlm::authenticate(&challenge, user, password, client, filetime()));
        let reply = self.round(Some(&format!("{scheme} {token}")), body, wait)?;
        self.signed = reply.status != 401;
        Ok(reply)
    }

    fn round(&mut self, authorization: Option<&str>, body: &str, wait: Duration) -> Result<Reply, Error> {
        let ep = self.ep;
        let conn = self.conn.as_mut().ok_or_else(|| Error::Lost("no connection".to_owned()))?;
        let auth = authorization.map(|a| format!("Authorization: {a}\r\n")).unwrap_or_default();
        let head = format!(
            "POST {} HTTP/1.1\r\nHost: {}:{}\r\n{auth}Content-Type: application/soap+xml;charset=UTF-8\r\n\
Content-Length: {}\r\nConnection: Keep-Alive\r\n\r\n",
            ep.path,
            ep.host,
            ep.port,
            body.len()
        );
        conn.tcp().set_write_timeout(Some(ep.connect_timeout)).map_err(lost)?;
        conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(body.as_bytes())).and_then(|()| conn.flush()).map_err(lost)?;
        read_reply(conn, wait, self.stop)
    }
}

/// Now as a Windows FILETIME (100 ns ticks since 1601-01-01).
fn filetime() -> u64 {
    let since_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (since_unix.as_nanos() / 100) as u64 + 116_444_736_000_000_000
}

fn lost(e: io::Error) -> Error {
    Error::Lost(e.to_string())
}

fn connect(ep: &Endpoint, stop: &AtomicBool) -> Result<Conn, Error> {
    // One budget for the lookup, every TCP try and the TLS handshake.
    let due = deadline(ep.connect_timeout);
    let tcp = tcp_connect(&ep.host, ep.port, due, stop)?;
    let _ = tcp.set_nodelay(true);
    if !ep.https {
        return Ok(Conn::Plain(tcp));
    }
    tls(tcp, &ep.host, ep.pin, ep.connect_timeout, due, stop).map(|(c, _)| Conn::Tls(Box::new(c)))
}

/// `timeout` from now; a budget too large to add (`Duration::MAX`) becomes a year.
fn deadline(timeout: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(timeout).or_else(|| now.checked_add(Duration::from_secs(365 * 24 * 3600))).unwrap_or(now)
}

/// The addresses of `host`. An IP literal answers at once; a name is looked up on a thread, waited for in slices of
/// at most 100 ms that see `stop` and `due`.
fn lookup(host: &str, port: u16, due: Instant, stop: &AtomicBool) -> Result<Vec<SocketAddr>, Error> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let host = host.to_owned();
    lookup_with(due, stop, move || (host.as_str(), port).to_socket_addrs().map(|a| a.collect()))
}

/// Run `work` (the system's name lookup, which cannot be cancelled) on a thread and wait for it until `due`.
/// A lookup still running at the deadline, or when `stop` is set, is left to end by itself when the system's lookup
/// ends: its answer goes nowhere.
fn lookup_with<F>(due: Instant, stop: &AtomicBool, work: F) -> Result<Vec<SocketAddr>, Error>
where
    F: FnOnce() -> io::Result<Vec<SocketAddr>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("ito-winrm-lookup".to_owned())
        .spawn(move || {
            let _ = tx.send(work());
        })
        .map_err(|e| Error::Connect(e.to_string()))?;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        let left = due.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::Timeout);
        }
        match rx.recv_timeout(left.min(Duration::from_millis(100))) {
            Ok(Ok(addrs)) => return Ok(addrs),
            Ok(Err(e)) => return Err(Error::Connect(e.to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Error::Connect("the name lookup ended without an answer".to_owned())),
        }
    }
}

fn tcp_connect(host: &str, port: u16, due: Instant, stop: &AtomicBool) -> Result<TcpStream, Error> {
    let addrs = lookup(host, port, due, stop)?;
    if addrs.is_empty() {
        return Err(Error::Connect(format!("{host} has no address")));
    }
    // Short tries, so a stop request is noticed while the SYN goes unanswered.
    loop {
        for a in &addrs {
            if stop.load(Ordering::Relaxed) {
                return Err(Error::Stopped);
            }
            let left = due.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            match TcpStream::connect_timeout(a, left.min(Duration::from_millis(250))) {
                Ok(s) => return Ok(s),
                Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(Error::Connect(e.to_string())),
            }
        }
    }
}

/// The socket as the TLS handshake sees it: every read and write waits at most 250 ms at a time and then looks at
/// `stop` and `due` again, so a server that trickles bytes (or says nothing) cannot hold the handshake past `due`.
struct Bounded<'a> {
    tcp: &'a mut TcpStream,
    due: Instant,
    stop: &'a AtomicBool,
}

impl Bounded<'_> {
    /// Check `stop` and `due`, then set the socket's timeouts to the time left, at most 250 ms.
    fn slice(&mut self) -> io::Result<()> {
        if self.stop.load(Ordering::Relaxed) {
            // Not `Interrupted` (rustls retries that). The caller reads the flag to tell this from a real failure.
            return Err(io::Error::other("stopped"));
        }
        let left = self.due.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let slice = Some(left.min(Duration::from_millis(250)));
        self.tcp.set_read_timeout(slice)?;
        self.tcp.set_write_timeout(slice)
    }
}

impl Read for Bounded<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            self.slice()?;
            match self.tcp.read(buf) {
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {}
                r => return r,
            }
        }
    }
}

impl Write for Bounded<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            self.slice()?;
            match self.tcp.write(buf) {
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {}
                r => return r,
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

/// Drive the handshake to its end by `due`. `stop` wins over every other outcome.
fn handshake(conn: &mut rustls::ClientConnection, tcp: &mut TcpStream, due: Instant, stop: &AtomicBool, seen: &Seen) -> Result<(), Error> {
    let mut io = Bounded { tcp, due, stop };
    while conn.is_handshaking() {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        if let Err(e) = conn.complete_io(&mut io) {
            if stop.load(Ordering::Relaxed) {
                return Err(Error::Stopped);
            }
            return Err(handshake_error(&e, seen));
        }
    }
    Ok(())
}

/// The handshake takes until `due` at most (the lookup and connect before it spent part of the same budget);
/// afterwards the socket's timeouts are `timeout`, for the requests.
pub(crate) fn tls(
    mut tcp: TcpStream,
    host: &str,
    pin: Option<[u8; 32]>,
    timeout: Duration,
    due: Instant,
    stop: &AtomicBool,
) -> Result<(rustls::StreamOwned<rustls::ClientConnection, TcpStream>, Seen), Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let seen: Seen = Arc::new(Mutex::new(None));
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Tls(e.to_string()))?;
    let config = match pin {
        Some(_) => builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pin {
                want: pin,
                seen: seen.clone(),
                algs: provider.signature_verification_algorithms,
            }))
            .with_no_client_auth(),
        None => {
            let mut roots = rustls::RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            builder.with_root_certificates(roots).with_no_client_auth()
        }
    };
    let name = ServerName::try_from(host.to_owned()).map_err(|e| Error::Tls(e.to_string()))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| Error::Tls(e.to_string()))?;
    handshake(&mut conn, &mut tcp, due, stop, &seen)?;
    tcp.set_read_timeout(Some(timeout)).map_err(lost)?;
    tcp.set_write_timeout(Some(timeout)).map_err(lost)?;
    Ok((rustls::StreamOwned::new(conn, tcp), seen))
}

/// Connect with any certificate accepted and say what its fingerprint is (nothing is sent to the server).
/// `timeout` bounds the whole call (lookup, connect and handshake).
pub(crate) fn peek(host: &str, port: u16, timeout: Duration, stop: &AtomicBool) -> Result<[u8; 32], Error> {
    let due = deadline(timeout);
    let mut tcp = tcp_connect(host, port, due, stop)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let seen: Seen = Arc::new(Mutex::new(None));
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pin {
            want: None,
            seen: seen.clone(),
            algs: provider.signature_verification_algorithms,
        }))
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_owned()).map_err(|e| Error::Tls(e.to_string()))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| Error::Tls(e.to_string()))?;
    match handshake(&mut conn, &mut tcp, due, stop, &seen) {
        Ok(()) => {}
        Err(Error::Stopped) => return Err(Error::Stopped),
        // A handshake that fails after the certificate was seen still has what the caller asked for.
        Err(e) => return seen.lock().ok().and_then(|g| *g).ok_or(e),
    }
    let got = seen.lock().ok().and_then(|g| *g);
    got.ok_or_else(|| Error::Tls("the server showed no certificate".to_owned()))
}

const DIFFERS: &str = "certificate fingerprint differs";

fn handshake_error(e: &io::Error, seen: &Seen) -> Error {
    let differs = e.to_string().contains(DIFFERS);
    match (differs, seen.lock().ok().and_then(|g| *g)) {
        (true, Some(got)) => Error::Fingerprint(fingerprint::format(&got)),
        _ if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => Error::Timeout,
        _ if matches!(e.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionAborted) => {
            Error::Connect(e.to_string())
        }
        _ => Error::Tls(e.to_string()),
    }
}

#[derive(Debug)]
struct Pin {
    /// `None` accepts any certificate (only [`peek`], which sends nothing).
    want: Option<[u8; 32]>,
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
        let got = fingerprint::sha256(end_entity);
        if let Ok(mut s) = self.seen.lock() {
            *s = Some(got);
        }
        match self.want {
            Some(want) if !fingerprint::ct_eq(&want, &got) => Err(rustls::Error::General(DIFFERS.to_owned())),
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

/// Read bytes with a deadline that the stop flag also cuts short (the socket wakes every 200 ms).
struct Reader<'a> {
    conn: &'a mut Conn,
    buf: Vec<u8>,
    deadline: Instant,
    stop: &'a AtomicBool,
}

impl Reader<'_> {
    fn fill(&mut self) -> Result<(), Error> {
        let mut chunk = [0u8; 8192];
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Err(Error::Stopped);
            }
            if Instant::now() >= self.deadline {
                return Err(Error::Timeout);
            }
            match self.conn.read(&mut chunk) {
                Ok(0) => return Err(Error::Lost("the server closed the connection".to_owned())),
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(());
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {}
                Err(e) => return Err(lost(e)),
            }
        }
    }

    fn until(&mut self, pat: &[u8]) -> Result<usize, Error> {
        loop {
            if let Some(i) = find(&self.buf, pat) {
                return Ok(i);
            }
            self.fill()?;
        }
    }

    fn take(&mut self, n: usize) -> Result<Vec<u8>, Error> {
        while self.buf.len() < n {
            self.fill()?;
        }
        Ok(self.buf.drain(..n).collect())
    }
}

fn find(hay: &[u8], pat: &[u8]) -> Option<usize> {
    hay.windows(pat.len()).position(|w| w == pat)
}

fn read_reply(conn: &mut Conn, wait: Duration, stop: &AtomicBool) -> Result<Reply, Error> {
    conn.tcp().set_read_timeout(Some(Duration::from_millis(200))).map_err(lost)?;
    let mut r = Reader { conn, buf: vec![], deadline: Instant::now() + wait, stop };
    let end = r.until(b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&r.take(end + 4)?[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| Error::Other("not an HTTP reply".to_owned()))?;
    let www_authenticate: Vec<String> = lines
        .clone()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("www-authenticate").then(|| v.trim().to_owned())
        })
        .collect();
    let header = |name: &str| {
        lines.clone().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
        })
    };
    let body = if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        let mut out = vec![];
        loop {
            let i = r.until(b"\r\n")?;
            let line = String::from_utf8_lossy(&r.take(i + 2)?[..i]).into_owned();
            let size =
                usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16).map_err(|_| Error::Other("bad chunk".to_owned()))?;
            if size == 0 {
                // Trailers, then the blank line.
                loop {
                    let i = r.until(b"\r\n")?;
                    let t = r.take(i + 2)?;
                    if i == 0 {
                        let _ = t;
                        break;
                    }
                }
                break;
            }
            out.extend(r.take(size)?);
            r.take(2)?;
        }
        out
    } else {
        let n = header("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        r.take(n)?
    };
    Ok(Reply { status, www_authenticate, body: String::from_utf8_lossy(&body).into_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// Accepts one connection, then runs `serve` on it; the thread ends by itself.
    fn peer(serve: impl FnOnce(TcpStream) + Send + 'static) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((s, _)) = l.accept() {
                serve(s);
            }
        });
        port
    }

    /// A TLS record header that promises a long record, then one byte every 150 ms (at most ~10 s, or until a write fails).
    fn trickle(mut s: TcpStream) {
        if s.write_all(&[0x16, 0x03, 0x03, 0x40, 0x00]).is_err() {
            return;
        }
        for _ in 0..66 {
            thread::sleep(Duration::from_millis(150));
            if s.write_all(&[0]).is_err() {
                return;
            }
        }
    }

    fn silent(s: TcpStream) {
        thread::sleep(Duration::from_secs(5));
        drop(s);
    }

    #[test]
    fn a_trickling_server_cannot_hold_peek_past_the_deadline() {
        let port = peer(trickle);
        let t = Instant::now();
        let r = peek("127.0.0.1", port, Duration::from_secs(1), &AtomicBool::new(false));
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }

    #[test]
    fn stop_ends_a_trickling_peek() {
        let port = peer(trickle);
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            s2.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let r = peek("127.0.0.1", port, Duration::from_secs(10), &stop);
        assert!(matches!(r, Err(Error::Stopped)), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn a_trickling_server_cannot_hold_the_login_handshake_past_the_deadline() {
        let port = peer(trickle);
        let stop = AtomicBool::new(false);
        let timeout = Duration::from_secs(1);
        let t = Instant::now();
        let due = deadline(timeout);
        let tcp = tcp_connect("127.0.0.1", port, due, &stop).unwrap();
        let r = tls(tcp, "127.0.0.1", None, timeout, due, &stop);
        assert!(matches!(r.err(), Some(Error::Timeout)));
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }

    #[test]
    fn a_silent_peer_times_out() {
        let port = peer(silent);
        let t = Instant::now();
        let r = peek("127.0.0.1", port, Duration::from_secs(1), &AtomicBool::new(false));
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }

    #[test]
    fn a_peer_that_closes_at_once_is_not_a_timeout() {
        let port = peer(drop);
        let t = Instant::now();
        let r = peek("127.0.0.1", port, Duration::from_secs(5), &AtomicBool::new(false));
        assert!(matches!(r, Err(Error::Connect(_))), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }

    #[test]
    fn extreme_budgets_do_not_panic() {
        let r = peek("127.0.0.1", 9, Duration::ZERO, &AtomicBool::new(false));
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        let port = peer(drop);
        let r = peek("127.0.0.1", port, Duration::MAX, &AtomicBool::new(false));
        assert!(matches!(r, Err(Error::Connect(_))), "{r:?}");
    }

    #[test]
    fn a_slow_lookup_ends_at_the_deadline() {
        let t = Instant::now();
        let due = deadline(Duration::from_millis(100));
        let r = lookup_with(due, &AtomicBool::new(false), || {
            thread::sleep(Duration::from_secs(3));
            Ok(vec![])
        });
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    }

    #[test]
    fn stop_ends_a_slow_lookup() {
        let t = Instant::now();
        let due = deadline(Duration::from_secs(10));
        let r = lookup_with(due, &AtomicBool::new(true), || {
            thread::sleep(Duration::from_secs(3));
            Ok(vec![])
        });
        assert!(matches!(r, Err(Error::Stopped)), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
    }

    #[test]
    fn a_lookup_error_is_a_connect_error() {
        let due = deadline(Duration::from_secs(5));
        let r = lookup_with(due, &AtomicBool::new(false), || Err(io::Error::other("no such host")));
        assert!(matches!(r, Err(Error::Connect(_))), "{r:?}");
    }

    #[test]
    fn an_ip_literal_is_answered_without_a_lookup() {
        let due = deadline(Duration::from_secs(5));
        let r = lookup("127.0.0.1", 80, due, &AtomicBool::new(false)).unwrap();
        assert_eq!(r, vec!["127.0.0.1:80".parse::<SocketAddr>().unwrap()]);
    }

    #[test]
    fn an_abandoned_lookup_ends_by_itself() {
        let done = Arc::new(AtomicBool::new(false));
        let d2 = done.clone();
        let due = deadline(Duration::from_millis(100));
        let r = lookup_with(due, &AtomicBool::new(false), move || {
            thread::sleep(Duration::from_millis(300));
            d2.store(true, Ordering::Relaxed);
            Ok(vec![])
        });
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        let end = Instant::now() + Duration::from_secs(3);
        while !done.load(Ordering::Relaxed) && Instant::now() < end {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(done.load(Ordering::Relaxed));
    }
}
