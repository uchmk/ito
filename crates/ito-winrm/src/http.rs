//! A small HTTP/1.1 client for one WinRM endpoint: a kept-alive connection (plain or TLS with a pinned certificate),
//! `POST` with a SOAP body, the reply read by `Content-Length` or chunked.

use crate::{fingerprint, ntlm, Error};
use base64::Engine;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
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
    let tcp = tcp_connect(&ep.host, ep.port, ep.connect_timeout, stop)?;
    let _ = tcp.set_nodelay(true);
    if !ep.https {
        return Ok(Conn::Plain(tcp));
    }
    tls(tcp, &ep.host, ep.pin, ep.connect_timeout, stop).map(|(c, _)| Conn::Tls(Box::new(c)))
}

fn tcp_connect(host: &str, port: u16, timeout: Duration, stop: &AtomicBool) -> Result<TcpStream, Error> {
    let addrs: Vec<_> = (host, port).to_socket_addrs().map_err(|e| Error::Connect(e.to_string()))?.collect();
    let deadline = Instant::now() + timeout;
    let mut last = None;
    // Short tries, so a stop request is noticed while the SYN goes unanswered.
    while Instant::now() < deadline {
        for a in &addrs {
            if stop.load(Ordering::Relaxed) {
                return Err(Error::Stopped);
            }
            let slice = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(250));
            match TcpStream::connect_timeout(a, slice.max(Duration::from_millis(1))) {
                Ok(s) => return Ok(s),
                Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(Error::Connect(e.to_string())),
            }
            last = Some(io::Error::new(io::ErrorKind::TimedOut, "timed out"));
        }
    }
    Err(last.map_or_else(|| Error::Connect(format!("{host} has no address")), |_| Error::Timeout))
}

pub(crate) fn tls(
    mut tcp: TcpStream,
    host: &str,
    pin: Option<[u8; 32]>,
    timeout: Duration,
    stop: &AtomicBool,
) -> Result<(rustls::StreamOwned<rustls::ClientConnection, TcpStream>, Seen), Error> {
    tcp.set_read_timeout(Some(timeout)).map_err(lost)?;
    tcp.set_write_timeout(Some(timeout)).map_err(lost)?;
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
    while conn.is_handshaking() {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        if let Err(e) = conn.complete_io(&mut tcp) {
            return Err(handshake_error(&e, &seen));
        }
    }
    Ok((rustls::StreamOwned::new(conn, tcp), seen))
}

/// Connect with any certificate accepted and say what its fingerprint is (nothing is sent to the server).
pub(crate) fn peek(host: &str, port: u16, timeout: Duration, stop: &AtomicBool) -> Result<[u8; 32], Error> {
    let tcp = tcp_connect(host, port, timeout, stop)?;
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
    let mut tcp = tcp;
    tcp.set_read_timeout(Some(timeout)).map_err(lost)?;
    while conn.is_handshaking() {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        if let Err(e) = conn.complete_io(&mut tcp) {
            if let Some(got) = seen.lock().ok().and_then(|g| *g) {
                return Ok(got);
            }
            return Err(handshake_error(&e, &seen));
        }
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
