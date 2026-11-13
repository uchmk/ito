//! The call: validation, the helper thread, the hop loop and the body reader.

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ureq::http::{self, HeaderName, HeaderValue};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, AsSendBody};
use url::Url;

use crate::error::{BAD_ADDRESS, BAD_SCHEME, HTTPS_REQUIRED, NO_HOST, PROXY_CONNECT, PROXY_REFUSED, PROXY_UNUSABLE};
use crate::redirect::{self, Hop};
use crate::{proxy, Error, Method, Options, Over, Proxy, Reply, Request, Roots};

/// How often the caller looks at the stop flag and the deadline.
const POLL: Duration = Duration::from_millis(50);
const CHUNK: usize = 16 * 1024;
const RESERVED: [&str; 5] = ["host", "content-length", "transfer-encoding", "connection", "proxy-authorization"];
/// Dropped for the rest of the call once the origin changed (secret headers go too).
const ORIGIN_BOUND: [&str; 3] = ["authorization", "cookie", "proxy-authorization"];

pub(crate) fn root_certs(roots: Roots) -> RootCerts {
    match roots {
        Roots::Os => RootCerts::PlatformVerifier,
        Roots::Bundled => RootCerts::WebPki,
    }
}

/// Check the request and return the start URL (no fragment).
fn validate(req: &Request, opts: &Options) -> Result<Url, Error> {
    let url = Url::parse(&req.url).map_err(|_| Error::BadUrl(BAD_ADDRESS.into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::BadUrl(BAD_SCHEME.into()));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(Error::BadUrl(NO_HOST.into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InvalidRequest("the URL has user information; use a header".into()));
    }
    if opts.https_only && url.scheme() != "https" {
        return Err(Error::BadUrl(HTTPS_REQUIRED.into()));
    }
    if req.body.is_some() && matches!(req.method, Method::Get | Method::Head) {
        return Err(Error::InvalidRequest(format!("a body is not allowed on {}", req.method.name())));
    }
    for h in &req.headers {
        let lower = h.name.to_ascii_lowercase();
        if HeaderName::from_bytes(h.name.as_bytes()).is_err() {
            return Err(Error::InvalidRequest(format!("bad header name {}", h.name)));
        }
        if RESERVED.contains(&lower.as_str()) {
            return Err(Error::InvalidRequest(format!("the header {} is set by the client", h.name)));
        }
        if HeaderValue::from_str(&h.value).is_err() {
            return Err(Error::InvalidRequest(format!("bad value for the header {}", h.name)));
        }
    }
    if let Some(b) = &req.body {
        if HeaderValue::from_str(&b.content_type).is_err() {
            return Err(Error::InvalidRequest("bad value for the header Content-Type".into()));
        }
    }
    redirect::clean(url).ok_or_else(|| Error::BadUrl(BAD_ADDRESS.into()))
}

fn build_agent(opts: &Options) -> Agent {
    let tls = TlsConfig::builder().provider(TlsProvider::Rustls).root_certs(root_certs(opts.roots)).build();
    let config = Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .https_only(false)
        .proxy(None)
        .tls_config(tls)
        .user_agent(opts.user_agent.as_str())
        .accept_encoding("")
        .build();
    Agent::new_with_config(config)
}

pub(crate) fn request(req: &Request, opts: &Options, stop: &AtomicBool) -> Result<Reply, Error> {
    let now = Instant::now();
    let end = now.checked_add(opts.deadline).unwrap_or(now + Duration::from_secs(365 * 24 * 3600));
    let first = validate(req, opts)?;
    if stop.load(Ordering::Relaxed) {
        return Err(Error::Stopped);
    }
    let agent = build_agent(opts);
    let cancel = Arc::new(AtomicBool::new(false));
    let job = Job {
        req: req.clone(),
        opts: opts.clone(),
        first,
        env: (opts.proxy == Proxy::Env).then(proxy::capture),
        end,
        cancel: cancel.clone(),
        agent: agent.clone(),
    };
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("ito-fetch".into())
        .spawn(move || {
            let _ = tx.send(job.run());
        })
        .map_err(|_| Error::Other("could not start a thread".into()))?;
    loop {
        let now = Instant::now();
        let give_up = if stop.load(Ordering::Relaxed) {
            Some(Error::Stopped)
        } else if now >= end {
            Some(Error::Timeout)
        } else {
            None
        };
        if let Some(e) = give_up {
            cancel.store(true, Ordering::Relaxed);
            agent.shutdown();
            return Err(e);
        }
        match rx.recv_timeout((end - now).min(POLL)) {
            Ok(r) => return r,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(Error::Other("the worker ended".into())),
        }
    }
}

struct Job {
    req: Request,
    opts: Options,
    first: Url,
    env: Option<HashMap<String, String>>,
    end: Instant,
    cancel: Arc<AtomicBool>,
    agent: Agent,
}

impl Job {
    fn check(&self) -> Result<Duration, Error> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        let left = self.end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::Timeout);
        }
        Ok(left)
    }

    fn run(&self) -> Result<Reply, Error> {
        let (mut url, mut method) = (self.first.clone(), self.req.method);
        let mut with_body = self.req.body.is_some();
        let mut stripped = false;
        let mut hops = 0u8;
        loop {
            let left = self.check()?;
            let host = url.host_str().unwrap_or("?").to_owned();
            let proxy = match self.env.as_ref().and_then(|env| proxy::choose(env, &url)) {
                Some(p) => Some(ureq::Proxy::new(&p).map_err(|_| Error::Proxy(PROXY_UNUSABLE.into()))?),
                None => None,
            };
            let proxied = proxy.is_some();
            let resp = self.send(&url, method, with_body, stripped, proxy, left).map_err(|e| map_err(e, &host, proxied))?;
            let status = resp.status().as_u16();
            if self.opts.max_redirects > 0 {
                let location = match resp.headers().get("location") {
                    Some(v) => match v.to_str() {
                        Ok(t) => Some(t),
                        Err(_) if redirect::is_redirect(status) => return Err(Error::Redirect(crate::Redirect::BadLocation)),
                        Err(_) => None,
                    },
                    None => None,
                };
                match redirect::next_hop(method, with_body, status, &url, location)? {
                    Hop::Done => {}
                    Hop::Follow { url: next, method: m, keep_body, other_origin } => {
                        if hops >= self.opts.max_redirects {
                            return Err(Error::Redirect(crate::Redirect::TooMany));
                        }
                        hops += 1;
                        (url, method, with_body, stripped) = (next, m, keep_body, stripped || other_origin);
                        continue;
                    }
                }
            }
            return self.read_reply(resp, &url, method);
        }
    }

    fn send(
        &self,
        url: &Url,
        method: Method,
        with_body: bool,
        stripped: bool,
        proxy: Option<ureq::Proxy>,
        left: Duration,
    ) -> Result<http::Response<ureq::Body>, ureq::Error> {
        let m = http::Method::from_bytes(method.name().as_bytes()).map_err(|_| ureq::Error::Other("method".into()))?;
        let mut b = http::Request::builder().method(m).uri(url.as_str());
        let dropped_body = self.req.body.is_some() && !with_body;
        for h in &self.req.headers {
            let lower = h.name.to_ascii_lowercase();
            let content =
                lower == "content-type" || lower == "content-encoding" || lower == "content-language" || lower == "content-location";
            // A redirect dropped the body: its content headers go with it. While it is sent, a non-empty `Body::content_type` wins.
            let body_type_wins = lower == "content-type" && with_body && self.req.body.as_ref().is_some_and(|b| !b.content_type.is_empty());
            if (content && dropped_body) || body_type_wins || (stripped && (h.secret || ORIGIN_BOUND.contains(&lower.as_str()))) {
                continue;
            }
            let mut v = HeaderValue::from_str(&h.value).map_err(|_| ureq::Error::Other("header".into()))?;
            v.set_sensitive(h.secret);
            b = b.header(h.name.as_str(), v);
        }
        match self.req.body.as_ref().filter(|_| with_body) {
            Some(body) => {
                if !body.content_type.is_empty() {
                    b = b.header("content-type", body.content_type.as_str());
                }
                self.run_one(b.body(body.bytes.as_slice())?, proxy, left)
            }
            None => self.run_one(b.body(())?, proxy, left),
        }
    }

    fn run_one<B: AsSendBody>(
        &self,
        req: http::Request<B>,
        proxy: Option<ureq::Proxy>,
        left: Duration,
    ) -> Result<http::Response<ureq::Body>, ureq::Error> {
        let req = self.agent.configure_request(req).timeout_global(Some(left)).proxy(proxy).build();
        self.agent.run(req)
    }

    fn read_reply(&self, resp: http::Response<ureq::Body>, url: &Url, method: Method) -> Result<Reply, Error> {
        let (parts, mut body) = resp.into_parts();
        let status = parts.status.as_u16();
        let text = |v: &HeaderValue| String::from_utf8_lossy(v.as_bytes()).into_owned();
        let headers: Vec<(String, String)> = parts.headers.iter().map(|(n, v)| (n.as_str().to_owned(), text(v))).collect();
        let content_type = parts.headers.get("content-type").map(text);
        let mut reply = Reply { status, url: url.to_string(), content_type, headers, body: Vec::new(), cut: false };
        let max = self.opts.max_reply;
        if method == Method::Head {
            return Ok(reply);
        }
        if self.opts.over == Over::Fail && body.content_length().is_some_and(|n| n > max as u64) {
            return Err(Error::TooLarge { limit: max });
        }
        let mut reader = body.as_reader();
        let mut chunk = vec![0u8; CHUNK];
        let mut buf: Vec<u8> = Vec::new();
        // One byte more than `max` tells there is more.
        let wanted = max.saturating_add(1);
        while buf.len() < wanted {
            self.check()?;
            let n = chunk.len().min(wanted - buf.len());
            match reader.read(&mut chunk[..n]) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) => return Err(self.map_body_err(e, status)),
            }
        }
        if buf.len() > max {
            if self.opts.over == Over::Fail {
                return Err(Error::TooLarge { limit: max });
            }
            buf.truncate(max);
            reply.cut = true;
        }
        reply.body = buf;
        Ok(reply)
    }

    fn map_body_err(&self, e: std::io::Error, status: u16) -> Error {
        if self.cancel.load(Ordering::Relaxed) {
            return Error::Stopped;
        }
        let timed_out = e.kind() == std::io::ErrorKind::TimedOut
            || matches!(e.get_ref().and_then(|x| x.downcast_ref::<ureq::Error>()), Some(ureq::Error::Timeout(_)));
        if timed_out || Instant::now() >= self.end {
            Error::Timeout
        } else {
            Error::Broken { status }
        }
    }
}

/// Map a ureq error to ours. Only the kind and the host are used: ureq's own texts can hold the URL.
fn map_err(e: ureq::Error, host: &str, proxied: bool) -> Error {
    use std::io::ErrorKind as K;
    use ureq::Error as U;
    let host = host.to_owned();
    if proxied {
        // The connect stage failed on the way to the proxy, not to the origin. The text names no proxy URL (it may hold a password).
        let to_proxy = match &e {
            U::HostNotFound | U::ConnectionFailed => true,
            U::Io(e) => {
                matches!(e.kind(), K::ConnectionRefused | K::NotFound | K::HostUnreachable | K::NetworkUnreachable | K::AddrNotAvailable)
            }
            _ => false,
        };
        if to_proxy {
            return Error::Proxy(PROXY_CONNECT.into());
        }
    }
    match e {
        U::Timeout(_) => Error::Timeout,
        U::HostNotFound => Error::Name(host),
        U::ConnectionFailed if proxied => Error::Proxy(PROXY_CONNECT.into()),
        U::ConnectionFailed => Error::Connect(host),
        U::InvalidProxyUrl => Error::Proxy(PROXY_UNUSABLE.into()),
        U::ConnectProxyFailed(_) => Error::Proxy(PROXY_REFUSED.into()),
        U::Tls(_) | U::Pem(_) | U::Rustls(_) | U::TlsRequired => Error::Tls(host),
        U::BadUri(_) | U::Http(_) => Error::BadUrl(BAD_ADDRESS.into()),
        U::Io(e) => match e.kind() {
            K::TimedOut | K::WouldBlock => Error::Timeout,
            K::InvalidData => {
                if e.get_ref().is_some_and(|x| x.is::<ureq::Error>()) {
                    Error::Other("protocol error".into())
                } else {
                    Error::Tls(host)
                }
            }
            K::NotFound => Error::Name(host),
            K::ConnectionRefused
            | K::ConnectionReset
            | K::ConnectionAborted
            | K::NotConnected
            | K::BrokenPipe
            | K::UnexpectedEof
            | K::HostUnreachable
            | K::NetworkUnreachable
            | K::AddrNotAvailable => Error::Connect(host),
            _ => Error::Other("network error".into()),
        },
        _ => Error::Other("protocol error".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Body, Header};
    use std::io::{Error as IoError, ErrorKind as K};

    fn io(k: K) -> ureq::Error {
        ureq::Error::Io(IoError::from(k))
    }

    #[test]
    fn unreachable_networks_are_connect_errors() {
        for k in [K::HostUnreachable, K::NetworkUnreachable] {
            assert_eq!(map_err(io(k), "h.example", false), Error::Connect("h.example".into()), "{k:?}");
        }
        assert!(map_err(io(K::NetworkUnreachable), "h", false).retryable());
    }

    #[test]
    fn a_proxied_connect_failure_names_the_proxy() {
        let cases = [
            ureq::Error::HostNotFound,
            ureq::Error::ConnectionFailed,
            io(K::ConnectionRefused),
            io(K::NotFound),
            io(K::HostUnreachable),
            io(K::NetworkUnreachable),
            io(K::AddrNotAvailable),
        ];
        for e in cases {
            let shown = format!("{e:?}");
            let got = map_err(e, "origin.example", true);
            assert!(matches!(got, Error::Proxy(_)), "{shown} -> {got:?}");
            assert!(!got.to_string().contains("origin.example"));
            assert!(got.retryable());
        }
        assert_eq!(map_err(ureq::Error::Timeout(ureq::Timeout::Global), "h", true), Error::Timeout);
        assert_eq!(map_err(io(K::TimedOut), "h", true), Error::Timeout);
        // Not proxied: the origin is named.
        assert_eq!(map_err(ureq::Error::HostNotFound, "h.example", false), Error::Name("h.example".into()));
        assert_eq!(map_err(io(K::ConnectionRefused), "h.example", false), Error::Connect("h.example".into()));
        assert_eq!(map_err(io(K::NotFound), "h.example", false), Error::Name("h.example".into()));
    }

    fn bad(req: &Request, opts: &Options) -> Error {
        validate(req, opts).unwrap_err()
    }

    #[test]
    fn roots_map_to_the_matching_verifier() {
        assert!(matches!(root_certs(Roots::Os), RootCerts::PlatformVerifier));
        assert!(matches!(root_certs(Roots::Bundled), RootCerts::WebPki));
    }

    #[test]
    fn urls_are_checked() {
        let o = Options::default();
        for u in ["ftp://x.example/f", "not a url", "", "file:///c:/x", "https://"] {
            assert!(matches!(bad(&Request::get(u), &o), Error::BadUrl(_)), "{u}");
        }
        let https = Options { https_only: true, ..Options::default() };
        assert!(matches!(bad(&Request::get("http://x.example/"), &https), Error::BadUrl(_)));
        assert!(validate(&Request::get("https://x.example/p#f"), &https).unwrap().fragment().is_none());
        assert!(matches!(bad(&Request::get("https://u:p@x.example/"), &o), Error::InvalidRequest(_)));
    }

    #[test]
    fn requests_are_checked() {
        let o = Options::default();
        let body = Body { bytes: vec![1], content_type: "text/plain".into() };
        let mut r = Request::get("http://x.example/");
        r.body = Some(body.clone());
        assert!(matches!(bad(&r, &o), Error::InvalidRequest(_)));
        r.method = Method::Delete;
        assert!(validate(&r, &o).is_ok());
        for name in ["Host", "content-LENGTH", "Transfer-Encoding", "Connection", "Proxy-Authorization"] {
            let mut r = Request::get("http://x.example/");
            r.headers.push(Header::secret(name, "TOPSECRET"));
            let e = bad(&r, &o);
            assert!(matches!(e, Error::InvalidRequest(_)), "{name}");
            assert!(!e.to_string().contains("TOPSECRET"));
        }
        let mut r = Request::get("http://x.example/");
        r.headers.push(Header::secret("X-Key", "line\nbreak-SECRET"));
        let e = bad(&r, &o);
        assert!(matches!(e, Error::InvalidRequest(_)) && e.to_string().contains("X-Key") && !e.to_string().contains("SECRET"));
        let mut r = Request::get("http://x.example/");
        r.headers.push(Header::new("bad name", "v"));
        assert!(matches!(bad(&r, &o), Error::InvalidRequest(_)));
    }
}
