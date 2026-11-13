//! TLS: a local HTTPS listener with a self-signed certificate must be refused, with both root sets.
//! (ito-testserver has no HTTPS server for plain GETs and the brief says not to add one, so the listener is built here.)
//! A successful https GET against a real public server is the ignored test at the end (run by hand).

use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use ito_fetch::{get, Error, Options, Proxy, Roots};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

fn tls_server() -> String {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()]).unwrap();
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(made.signing_key.serialize_der()));
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(made.cert.der().to_vec())], key)
        .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("https://127.0.0.1:{}/", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut tcp) = stream else { return };
            let mut conn = rustls::ServerConnection::new(config.clone()).unwrap();
            // The client is expected to walk away at the certificate; if it does not, answer anyway.
            let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
            let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi");
        }
    });
    url
}

fn opts(roots: Roots) -> Options {
    Options { proxy: Proxy::None, roots, deadline: Duration::from_secs(10), ..Options::default() }
}

#[test]
fn a_self_signed_certificate_is_refused_with_os_roots() {
    let url = tls_server();
    let e = get(&url, &opts(Roots::Os), &AtomicBool::new(false)).unwrap_err();
    assert!(matches!(e, Error::Tls(_)), "{e:?}");
    assert!(!e.retryable());
}

#[test]
fn a_self_signed_certificate_is_refused_with_bundled_roots() {
    let url = tls_server();
    let e = get(&url, &opts(Roots::Bundled), &AtomicBool::new(false)).unwrap_err();
    assert!(matches!(e, Error::Tls(_)), "{e:?}");
}

#[test]
#[ignore = "needs the Internet: run by hand on a PC"]
fn the_cabinet_office_table_downloads_with_both_root_sets() {
    for roots in [Roots::Os, Roots::Bundled] {
        let o = Options { https_only: true, roots, proxy: Proxy::Env, ..Options::default() };
        let r = get("https://www8.cao.go.jp/chosei/shukujitsu/syukujitsu.csv", &o, &AtomicBool::new(false)).unwrap();
        assert_eq!(r.status, 200, "{roots:?}");
        assert!(r.body.len() > 10_000, "{roots:?}: {} bytes", r.body.len());
    }
}
