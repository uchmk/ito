//! PostgreSQL through tokio-postgres (rustls, ring).

use std::io::ErrorKind;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio_postgres::SimpleQueryMessage;

use crate::{Clock, ConnectKind, Error, Last, Outcome, Target, Tls};

pub(crate) fn run(target: &Target, batches: &[String], clock: &Clock, last: &mut Last, outcome: &mut Outcome) -> Result<(), Error> {
    let Target::Postgres { host, port, database, user, password, tls } = target else { unreachable!() };
    let mut config = tokio_postgres::Config::new();
    config.host(host).port(*port).dbname(database).user(user).password(password);
    // tokio-postgres' default is `Prefer`: a server (or a man in the middle) that answers `N` to the SSLRequest would be
    // used in plain text, sign-in included. Require / Verify must fail instead.
    config.ssl_mode(match tls {
        Tls::Disable => tokio_postgres::config::SslMode::Disable,
        Tls::Require | Tls::Verify => tokio_postgres::config::SslMode::Require,
    });
    clock.block_on(async {
        let client = match tls {
            Tls::Disable => {
                let (client, conn) = config.connect(tokio_postgres::NoTls).await.map_err(connect_error)?;
                tokio::spawn(conn);
                client
            }
            Tls::Require | Tls::Verify => {
                let connector = tokio_postgres_rustls::MakeRustlsConnect::new(client_config(*tls == Tls::Verify)?);
                let (client, conn) = config.connect(connector).await.map_err(connect_error)?;
                tokio::spawn(conn);
                client
            }
        };
        for batch in batches {
            let messages = client.simple_query(batch).await.map_err(sql_error)?;
            let mut described = false;
            for m in messages {
                match m {
                    SimpleQueryMessage::RowDescription(cols) => {
                        described = true;
                        let names: Vec<String> = cols.iter().map(|c| c.name().to_owned()).collect();
                        last.begin(&names);
                    }
                    SimpleQueryMessage::Row(row) => {
                        last.row((0..row.len()).map(|i| row.get(i).map(str::to_owned)));
                    }
                    // A SELECT also reports how many rows it returned; those are not changed rows.
                    SimpleQueryMessage::CommandComplete(n) => {
                        if !described {
                            outcome.rows_affected += n;
                        }
                        described = false;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })
}

fn connect_error(e: tokio_postgres::Error) -> Error {
    let kind = connect_kind(&e);
    match e.as_db_error() {
        Some(db) => Error::connect(kind, db.message()),
        None => Error::connect(kind, error_chain(&e)),
    }
}

// tokio-postgres 0.7 keeps its error kind private; these are the fixed `Display` texts of the kinds we tell apart. The
// tests pin them, so a version that changes them fails a test instead of silently classifying wrong.
const TLS_HANDSHAKE: &str = "error performing TLS handshake";
const AUTHENTICATION: &str = "authentication error";

/// Why a connect failed, from what tokio-postgres leaves public (see the order in the comments).
fn connect_kind(e: &tokio_postgres::Error) -> ConnectKind {
    // 1. The server answered with an error: its SQLSTATE says what.
    if let Some(db) = e.as_db_error() {
        let code = db.code().code();
        return if code.starts_with("28") {
            ConnectKind::SignIn
        } else if code == "3D000" {
            ConnectKind::Missing
        } else {
            ConnectKind::Other
        };
    }
    let top = e.to_string();
    let source = std::error::Error::source(e);
    // 2. A TLS handshake that broke on the transport (the peer closed or reset) is an unreachable server, but not when the
    // cause is rustls or the connector itself (an invalid host name is an `InvalidInput` io error from the connector).
    if top == TLS_HANDSHAKE {
        let transport = source.and_then(|s| s.downcast_ref::<std::io::Error>()).is_some_and(|io| {
            io.get_ref().is_none_or(|inner| !inner.is::<rustls::Error>())
                && matches!(
                    io.kind(),
                    ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted
                        | ErrorKind::ConnectionRefused
                        | ErrorKind::BrokenPipe
                        | ErrorKind::UnexpectedEof
                        | ErrorKind::NotConnected
                        | ErrorKind::TimedOut
                )
        });
        return if transport { ConnectKind::Unreachable } else { ConnectKind::Tls };
    }
    // 3. A rustls error anywhere in the chain.
    if source.is_some_and(has_rustls_error) {
        return ConnectKind::Tls;
    }
    // 4. A sign-in method we do not speak (Kerberos, GSS, SSPI) or a SCRAM failure.
    if top == AUTHENTICATION {
        return ConnectKind::SignIn;
    }
    // 5. The connection itself failed or closed.
    if e.is_closed() || source.is_some_and(has_io_error) {
        return ConnectKind::Unreachable;
    }
    ConnectKind::Other
}

/// A `rustls::Error` in the chain, also inside an `io::Error` (whose `source()` skips the error it wraps).
fn has_rustls_error(e: &(dyn std::error::Error + 'static)) -> bool {
    if e.is::<rustls::Error>() {
        return true;
    }
    if let Some(inner) = e.downcast_ref::<std::io::Error>().and_then(|io| io.get_ref()) {
        if has_rustls_error(inner) {
            return true;
        }
    }
    e.source().is_some_and(has_rustls_error)
}

fn has_io_error(e: &(dyn std::error::Error + 'static)) -> bool {
    e.is::<std::io::Error>() || e.source().is_some_and(has_io_error)
}

fn sql_error(e: tokio_postgres::Error) -> Error {
    match e.as_db_error() {
        Some(db) => Error::Sql(db.message().to_owned()),
        None => Error::Sql(error_chain(&e)),
    }
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(inner) = src {
        s.push_str(": ");
        s.push_str(&inner.to_string());
        src = inner.source();
    }
    s
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

fn client_config(verify: bool) -> Result<ClientConfig, Error> {
    let builder = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::connect(ConnectKind::Tls, format!("TLS: {e}")))?;
    Ok(if verify {
        let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(AnyCertificate(provider()))).with_no_client_auth()
    })
}

/// `sslmode=require`: encrypt, but do not check who answers.
#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use super::*;

    const FIVE: Duration = Duration::from_secs(5);

    /// A fake server for one connection: `handler` gets the accepted stream (read / write timeouts of 3 s). Nothing here
    /// waits without a bound: `accept` is polled until a deadline, and the handler ends on its first error.
    fn fake(handler: impl FnOnce(TcpStream) + Send + 'static) -> (u16, JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(10);
            while Instant::now() < end {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                        stream.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                        handler(stream);
                        return;
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        (port, h)
    }

    fn target(port: u16, tls: Tls) -> Target {
        Target::Postgres { host: "127.0.0.1".into(), port, database: "d".into(), user: "u".into(), password: "p".into(), tls }
    }

    fn connect_err(port: u16, tls: Tls) -> Error {
        crate::run(&target(port, tls), "select 1", None, &AtomicBool::new(false), FIVE).unwrap_err()
    }

    fn kind(e: &Error) -> ConnectKind {
        match e {
            Error::Connect { kind, .. } => *kind,
            other => panic!("not a connect error: {other:?}"),
        }
    }

    fn read_ssl_request(s: &mut TcpStream) {
        let mut b = [0u8; 8];
        s.read_exact(&mut b).unwrap();
        assert_eq!(&b[..4], &8u32.to_be_bytes());
    }

    fn read_startup(s: &mut TcpStream) {
        let mut len = [0u8; 4];
        s.read_exact(&mut len).unwrap();
        let mut body = vec![0u8; u32::from_be_bytes(len) as usize - 4];
        s.read_exact(&mut body).unwrap();
    }

    fn error_response(code: &str, message: &str) -> Vec<u8> {
        let mut fields = Vec::new();
        for (tag, text) in [(b'S', "FATAL"), (b'V', "FATAL"), (b'C', code), (b'M', message)] {
            fields.push(tag);
            fields.extend_from_slice(text.as_bytes());
            fields.push(0);
        }
        fields.push(0);
        let mut out = vec![b'E'];
        out.extend_from_slice(&((fields.len() + 4) as u32).to_be_bytes());
        out.extend_from_slice(&fields);
        out
    }

    #[test]
    fn nothing_listening_is_unreachable() {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        let e = connect_err(port, Tls::Disable);
        assert_eq!(kind(&e), ConnectKind::Unreachable, "{e:?}");
        assert!(e.retryable());
    }

    fn refuses_plain_text(tls: Tls) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let seen = got.clone();
        let (port, h) = fake(move |mut s| {
            read_ssl_request(&mut s);
            s.write_all(b"N").unwrap();
            // A client that went on in plain text would send its StartupMessage now; one that gave up closes.
            let mut buf = [0u8; 256];
            if let Ok(n) = s.read(&mut buf) {
                seen.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });
        let e = connect_err(port, tls);
        h.join().unwrap();
        assert_eq!(kind(&e), ConnectKind::Tls, "{e:?}");
        assert!(!e.retryable());
        assert!(e.to_string().contains("server does not support TLS"), "{e}");
        assert!(got.lock().unwrap().is_empty(), "the client sent {:?} after N", got.lock().unwrap());
    }

    #[test]
    fn require_does_not_fall_back_to_plain_text() {
        refuses_plain_text(Tls::Require);
    }

    #[test]
    fn verify_does_not_fall_back_to_plain_text() {
        refuses_plain_text(Tls::Verify);
    }

    #[test]
    fn tls_with_a_server_that_speaks_http_is_a_tls_failure() {
        let (port, h) = fake(|mut s| {
            read_ssl_request(&mut s);
            s.write_all(b"S").unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
            std::thread::sleep(Duration::from_millis(300));
        });
        let e = connect_err(port, Tls::Require);
        h.join().unwrap();
        assert_eq!(kind(&e), ConnectKind::Tls, "{e:?}");
        assert!(!e.retryable());
    }

    fn answers_error(code: &'static str) -> Error {
        let (port, h) = fake(move |mut s| {
            read_startup(&mut s);
            s.write_all(&error_response(code, "password authentication failed for user \"u\"")).unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });
        let e = connect_err(port, Tls::Disable);
        h.join().unwrap();
        e
    }

    #[test]
    fn sqlstates_are_told_apart() {
        for code in ["28P01", "28000"] {
            let e = answers_error(code);
            assert_eq!(kind(&e), ConnectKind::SignIn, "{code}: {e:?}");
            assert!(!e.retryable());
            assert!(e.to_string().contains("password authentication failed for user \"u\""), "{e}");
        }
        let e = answers_error("3D000");
        assert_eq!(kind(&e), ConnectKind::Missing, "{e:?}");
        assert!(!e.retryable());
        let e = answers_error("53300");
        assert_eq!(kind(&e), ConnectKind::Other, "{e:?}");
        assert!(e.retryable());
    }

    #[test]
    fn an_unsupported_sign_in_method_is_a_sign_in_failure() {
        let (port, h) = fake(|mut s| {
            read_startup(&mut s);
            let mut m = vec![b'R'];
            m.extend_from_slice(&8u32.to_be_bytes());
            m.extend_from_slice(&2u32.to_be_bytes());
            s.write_all(&m).unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });
        let e = connect_err(port, Tls::Disable);
        h.join().unwrap();
        assert_eq!(kind(&e), ConnectKind::SignIn, "{e:?}");
        assert!(e.to_string().contains(AUTHENTICATION), "{e}");
    }

    #[test]
    fn a_server_that_closes_at_once_is_unreachable() {
        let (port, h) = fake(drop);
        let e = connect_err(port, Tls::Disable);
        h.join().unwrap();
        assert_eq!(kind(&e), ConnectKind::Unreachable, "{e:?}");
        assert!(e.retryable());
    }

    #[test]
    fn closing_after_the_ssl_request_is_unreachable() {
        let (port, h) = fake(|mut s| read_ssl_request(&mut s));
        let e = connect_err(port, Tls::Require);
        h.join().unwrap();
        assert_eq!(kind(&e), ConnectKind::Unreachable, "{e:?}");
    }

    #[test]
    fn closing_during_the_tls_handshake_is_unreachable() {
        let (port, h) = fake(|mut s| {
            read_ssl_request(&mut s);
            s.write_all(b"S").unwrap();
        });
        let e = connect_err(port, Tls::Require);
        h.join().unwrap();
        assert_eq!(kind(&e), ConnectKind::Unreachable, "{e:?}");
        assert!(e.to_string().contains(TLS_HANDSHAKE), "{e}");
    }

    #[test]
    fn an_invalid_tls_host_name_is_a_tls_failure() {
        let (port, h) = fake(|mut s| {
            read_ssl_request(&mut s);
            s.write_all(b"S").unwrap();
            std::thread::sleep(Duration::from_millis(300));
        });
        let mut config = tokio_postgres::Config::new();
        config
            .host("not a valid name")
            .hostaddr(Ipv4Addr::LOCALHOST.into())
            .port(port)
            .user("u")
            .password("p")
            .ssl_mode(tokio_postgres::config::SslMode::Require);
        let connector = tokio_postgres_rustls::MakeRustlsConnect::new(client_config(false).unwrap());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let e = rt
            .block_on(async { tokio::time::timeout(FIVE, config.connect(connector)).await.unwrap() })
            .err()
            .expect("an invalid host name must not connect");
        h.join().unwrap();
        assert_eq!(e.to_string(), TLS_HANDSHAKE, "{e:?}");
        assert_eq!(connect_kind(&e), ConnectKind::Tls, "{e:?}");
    }
}
