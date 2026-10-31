//! A blocking HTTPS client for public hosts (certificates checked against the webpki roots). One request per connection,
//! `Connection: close`, no redirects. `http://` is accepted only for the loopback (a local test server).

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::Error;

/// The largest answer body read (32 MiB: Gmail sends a message with its attachments as base64 in JSON).
const MAX_BODY: usize = 32 << 20;
/// How much of an error body is kept in [`Error::Status`].
const MAX_ERROR_BODY: usize = 2000;

/// A 2xx answer.
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, Error> {
        serde_json::from_slice(&self.body).map_err(|e| Error::Bad(format!("not the JSON expected: {e}")))
    }
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response").field("status", &self.status).field("body", &format_args!("{} bytes", self.body.len())).finish()
    }
}

/// Send one request. `body` is the content type and the bytes. Only 2xx is `Ok`; the rest is [`Error::Status`].
/// `timeout` bounds the connect and every read and write.
pub fn request(method: &str, url: &str, bearer: Option<&str>, body: Option<(&str, &[u8])>, timeout: Duration) -> Result<Response, Error> {
    if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Error::Url(format!("{method} is not a method")));
    }
    if bearer.is_some_and(|b| b.contains(['\r', '\n'])) || body.is_some_and(|(t, _)| t.contains(['\r', '\n'])) {
        return Err(Error::Url("a header value has a line break".to_owned()));
    }
    let u = Url::parse(url)?;
    let mut head = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: ito-oauth\r\nAccept: */*\r\nConnection: close\r\n",
        u.target,
        u.host_header()
    );
    if let Some(b) = bearer {
        head += &format!("Authorization: Bearer {b}\r\n");
    }
    match body {
        Some((kind, bytes)) => head += &format!("Content-Type: {kind}\r\nContent-Length: {}\r\n", bytes.len()),
        None if matches!(method, "POST" | "PUT" | "PATCH") => head += "Content-Length: 0\r\n",
        None => {}
    }
    head += "\r\n";
    let mut stream = connect(&u, timeout)?;
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(body.map_or(&[][..], |(_, b)| b)))
        .and_then(|_| stream.flush())
        .map_err(io_err)?;
    let mut raw = Vec::new();
    match Read::by_ref(&mut stream).take(MAX_BODY as u64 + 64 * 1024).read_to_end(&mut raw) {
        Ok(_) => {}
        // A server that closes without close_notify after a complete answer is still a complete answer.
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof && !raw.is_empty() => {}
        Err(e) => return Err(io_err(e)),
    }
    let parsed = parse_response(&raw)?;
    if (200..300).contains(&parsed.status) {
        Ok(Response { status: parsed.status, body: parsed.body })
    } else {
        let retry_after = parsed.header("retry-after").and_then(|v| v.trim().parse::<u64>().ok()).map(Duration::from_secs);
        let mut body = String::from_utf8_lossy(&parsed.body).into_owned();
        if let Some((i, _)) = body.char_indices().nth(MAX_ERROR_BODY) {
            body.truncate(i);
        }
        Err(Error::Status { code: parsed.status, retry_after, body })
    }
}

pub(crate) fn io_err(e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Error::Timeout,
        _ => Error::Network(e.to_string()),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Url {
    https: bool,
    host: String,
    port: u16,
    /// Path and query, starting with `/`.
    target: String,
}

impl Url {
    fn parse(url: &str) -> Result<Url, Error> {
        let bad = || Error::Url(format!("{url} is not https://host[:port]/path"));
        if url.contains(['\r', '\n', ' ']) {
            return Err(bad());
        }
        let (https, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
            (Some(r), _) => (true, r),
            (_, Some(r)) => (false, r),
            _ => return Err(bad()),
        };
        let rest = rest.split('#').next().unwrap_or_default();
        let (authority, target) = match rest.find(['/', '?']) {
            Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_owned()),
            Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
            None => (rest, "/".to_owned()),
        };
        if authority.is_empty() || authority.contains('@') {
            return Err(bad());
        }
        let default = if https { 443 } else { 80 };
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let (host, tail) = v6.split_once(']').ok_or_else(bad)?;
            let port = match tail.strip_prefix(':') {
                Some(p) => p.parse().map_err(|_| bad())?,
                None if tail.is_empty() => default,
                None => return Err(bad()),
            };
            (host.to_owned(), port)
        } else {
            match authority.rsplit_once(':') {
                Some((host, p)) if !host.is_empty() => (host.to_owned(), p.parse().map_err(|_| bad())?),
                _ => (authority.to_owned(), default),
            }
        };
        if !https && !matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost") {
            return Err(Error::Url(format!("{url}: plain http is for the loopback only")));
        }
        Ok(Url { https, host, port, target })
    }

    fn host_header(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == if self.https { 443 } else { 80 } {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }
}

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn connect(u: &Url, timeout: Duration) -> Result<Box<dyn Stream>, Error> {
    let addrs: Vec<_> = (u.host.as_str(), u.port).to_socket_addrs().map_err(|e| Error::Network(e.to_string()))?.collect();
    let mut last = None;
    let mut tcp = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let mut tcp = tcp.ok_or_else(|| last.map_or_else(|| Error::Network(format!("{} has no address", u.host)), io_err))?;
    tcp.set_read_timeout(Some(timeout)).map_err(io_err)?;
    tcp.set_write_timeout(Some(timeout)).map_err(io_err)?;
    if !u.https {
        return Ok(Box::new(tcp));
    }
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Tls(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(u.host.clone()).map_err(|e| Error::Tls(e.to_string()))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| Error::Tls(e.to_string()))?;
    // Finish the handshake here so a certificate error is a Tls error, not a Network one on the first read.
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp).map_err(|e| match e.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Error::Timeout,
            _ => Error::Tls(e.to_string()),
        })?;
    }
    Ok(Box::new(rustls::StreamOwned::new(conn, tcp)))
}

struct Parsed {
    status: u16,
    /// Names in lower case.
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Parsed {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

fn parse_response(raw: &[u8]) -> Result<Parsed, Error> {
    let bad = |s: &str| Error::Bad(s.to_owned());
    let end = find(raw, b"\r\n\r\n").ok_or_else(|| bad("no end of the head"))?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| bad("the head is not UTF-8"))?;
    let mut lines = head.lines();
    let status = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).ok_or_else(|| bad("no status"))?;
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned())).collect();
    let rest = &raw[end + 4..];
    let find_h = |n: &str| headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
    let body = if find_h("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
        dechunk(rest)?
    } else if let Some(n) = find_h("content-length").and_then(|v| v.parse::<usize>().ok()) {
        if rest.len() < n {
            return Err(bad("the body was cut short"));
        }
        rest[..n].to_vec()
    } else {
        rest.to_vec()
    };
    if body.len() > MAX_BODY {
        return Err(bad("the body is too large"));
    }
    Ok(Parsed { status, headers, body })
}

fn dechunk(mut rest: &[u8]) -> Result<Vec<u8>, Error> {
    let bad = |s: &str| Error::Bad(s.to_owned());
    let mut out = Vec::new();
    loop {
        let eol = find(rest, b"\r\n").ok_or_else(|| bad("a chunk was cut short"))?;
        let size_text = std::str::from_utf8(&rest[..eol]).map_err(|_| bad("bad chunk size"))?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or_default().trim(), 16).map_err(|_| bad("bad chunk size"))?;
        rest = &rest[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if rest.len() < size + 2 {
            return Err(bad("a chunk was cut short"));
        }
        out.extend_from_slice(&rest[..size]);
        if out.len() > MAX_BODY {
            return Err(bad("the body is too large"));
        }
        rest = &rest[size + 2..];
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn addresses_are_split_and_plain_http_is_loopback_only() {
        let u = Url::parse("https://gmail.googleapis.com/gmail/v1/users/me?x=1#f").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.target.as_str()), (true, "gmail.googleapis.com", 443, "/gmail/v1/users/me?x=1"));
        assert_eq!(Url::parse("https://h:8443").unwrap().target, "/");
        assert_eq!(Url::parse("https://h?a=b").unwrap().target, "/?a=b");
        assert_eq!(Url::parse("http://127.0.0.1:9/t").unwrap().host_header(), "127.0.0.1:9");
        assert_eq!(Url::parse("http://[::1]:9/").unwrap().host_header(), "[::1]:9");
        for bad in ["http://example.com/", "ftp://x", "https://", "https://u@x/", "https://x y/", "https://x:nope/", "x"] {
            assert!(matches!(Url::parse(bad), Err(Error::Url(_))), "{bad}");
        }
    }

    #[test]
    fn replies_are_read_by_length_chunks_or_the_end() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcdef").unwrap();
        assert_eq!(r.body, b"abc");
        let r = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\n\r\n").unwrap();
        assert_eq!(r.body, b"Wikipedia");
        assert_eq!(parse_response(b"HTTP/1.1 200 OK\r\n\r\nrest").unwrap().body, b"rest");
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n{}").is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\nshort").is_err());
        assert!(parse_response(b"garbage").is_err());
    }

    /// Serve one answer on a loopback port and give back the request it saw.
    fn once(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 4096];
            while find(&seen, b"\r\n\r\n").is_none() {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            // Read a small body too, if the head says there is one.
            let text = String::from_utf8_lossy(&seen).into_owned();
            if let Some(n) = text.lines().find_map(|l| l.strip_prefix("Content-Length: ")).and_then(|v| v.parse::<usize>().ok()) {
                let have = seen.len() - find(&seen, b"\r\n\r\n").unwrap() - 4;
                let mut left = n.saturating_sub(have);
                while left > 0 {
                    let k = s.read(&mut buf).unwrap();
                    seen.extend_from_slice(&buf[..k]);
                    left = left.saturating_sub(k);
                }
            }
            s.write_all(answer.as_bytes()).unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        (url, h)
    }

    #[test]
    fn a_request_carries_the_bearer_and_the_body() {
        let (url, seen) = once("HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\n{\"a\":1}");
        let r = request("POST", &format!("{url}/x?y=1"), Some("tok"), Some(("application/json", b"{\"q\":2}")), Duration::from_secs(3))
            .unwrap();
        assert_eq!((r.status, r.json::<serde_json::Value>().unwrap()["a"].as_i64()), (200, Some(1)));
        let s = seen.join().unwrap();
        assert!(s.starts_with("POST /x?y=1 HTTP/1.1\r\n"), "{s}");
        assert!(s.contains("Authorization: Bearer tok\r\n") && s.contains("Content-Type: application/json\r\n"), "{s}");
        assert!(s.ends_with("{\"q\":2}"), "{s}");
    }

    #[test]
    fn a_post_without_a_body_says_so_and_a_get_does_not() {
        let (url, seen) = once("HTTP/1.1 204 No Content\r\n\r\n");
        assert_eq!(request("POST", &url, None, None, Duration::from_secs(3)).unwrap().status, 204);
        assert!(seen.join().unwrap().contains("Content-Length: 0\r\n"));
        let (url, seen) = once("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
        request("GET", &url, None, None, Duration::from_secs(3)).unwrap();
        assert!(!seen.join().unwrap().contains("Content-Length"));
    }

    #[test]
    fn other_than_2xx_is_a_status_with_retry_after() {
        let (url, _) = once("HTTP/1.1 429 Too Many Requests\r\nRetry-After: 17\r\nContent-Length: 4\r\n\r\nslow");
        let e = request("GET", &url, None, None, Duration::from_secs(3)).unwrap_err();
        assert_eq!(e, Error::Status { code: 429, retry_after: Some(Duration::from_secs(17)), body: "slow".to_owned() });
        assert!(e.retryable());
        let (url, _) = once("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
        let e = request("GET", &url, None, None, Duration::from_secs(3)).unwrap_err();
        assert!(!e.retryable());
    }

    #[test]
    fn a_header_value_with_a_line_break_is_refused() {
        assert!(matches!(request("GET", "http://127.0.0.1:1/", Some("a\r\nX: y"), None, Duration::from_secs(1)), Err(Error::Url(_))));
        assert!(matches!(request("get", "http://127.0.0.1:1/", None, None, Duration::from_secs(1)), Err(Error::Url(_))));
    }

    #[test]
    fn a_closed_port_is_a_network_error() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let e = request("GET", &format!("http://127.0.0.1:{port}/"), None, None, Duration::from_secs(2)).unwrap_err();
        assert!(matches!(e, Error::Network(_) | Error::Timeout), "{e:?}");
    }
}
