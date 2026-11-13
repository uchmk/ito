//! One plain HTTP or HTTPS request to an ordinary server, for a file or a small call: an update check, a published
//! table, a chore that calls a web address.
//!
//! - [`get`] and [`request`] block. Call them from a worker thread, never from the UI thread.
//! - Any HTTP status comes back as a [`Reply`] (404 and 500 too). Only "no answer" is an [`Error`].
//! - **Stop and deadline.** The whole call runs on one helper thread with its own agent; the caller waits and polls the
//!   stop flag and the deadline every 50 ms or less. When either fires, the helper is told to stop, its connections are
//!   shut down (which interrupts a read, a write, a handshake and a body in flight) and the caller returns
//!   [`Error::Stopped`] or [`Error::Timeout`] at once, without joining. Name lookup and a TCP connect cannot be
//!   interrupted: the caller still returns, and the helper ends by itself at the deadline (every hop has
//!   `timeout_global(remaining)`). A request already sent may have reached the server, so a stopped chore may have run.
//! - **Redirects** are followed here, not by ureq: 301, 302, 303 become GET (HEAD stays HEAD) without a body; 307 and 308
//!   keep the method and body but never send a body to another origin. https to http is always refused. On a change of
//!   origin every secret header and `Authorization` / `Cookie` are dropped for the rest of the call.
//! - **Proxy.** [`Proxy::Env`] reads `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` (and lower case) per hop by its scheme, and
//!   `NO_PROXY`. There is no PAC and no Windows proxy setting. With an `http://` proxy, an `http://` request (its
//!   headers, secrets included) goes to the proxy in clear; an `https://` request goes through a CONNECT tunnel, so the
//!   proxy sees only the host and port.
//! - **Roots.** [`Roots::Os`]: on Windows and macOS the OS verifies the chain. On Linux (and other Unix) no OS verifier
//!   exists: `rustls-platform-verifier` loads the system's CA files with `rustls-native-certs` (it honours
//!   `SSL_CERT_FILE` / `SSL_CERT_DIR` and the usual distribution paths) and checks the chain with webpki; if none can be
//!   loaded the TLS setup fails (`Error::Tls`) and there is no fall back to the bundled list. [`Roots::Bundled`]: the
//!   Mozilla list in the binary.
//! - **Secrets.** `Debug` of [`Header`], [`Request`], [`Reply`] and [`Error`] never shows a secret header value, a body, or
//!   a URL path or query; error texts name a host at most.
//! - **Logging.** ureq logs through the `log` crate and at `debug` or `trace` it can print URL paths and some header
//!   values. An app that installs a `log` logger should keep the targets `ureq` and `ureq_proto` at `info` or above.
//! - HTTP/1.1 only. No cookies, no compression (no `Accept-Encoding` is sent), no retries (the caller retries when
//!   [`Error::retryable`]).
//! - Not pure Rust: TLS is rustls with `ring`, which needs a C compiler on Linux and macOS targets.

mod error;
mod proxy;
mod redirect;
mod run;

use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub use error::{Error, Redirect};

/// The request method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    fn name(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// A request header. A `secret` one never shows in `Debug` or in an error, and is not sent on a redirect to another
/// origin.
///
/// These names are refused (`Error::InvalidRequest`): `Host`, `Content-Length`, `Transfer-Encoding`, `Connection`,
/// `Proxy-Authorization`. A `Content-Type` header is ignored when the request has a [`Body`] (the body's wins).
#[derive(Clone)]
pub struct Header {
    pub name: String,
    pub value: String,
    pub secret: bool,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Header {
        Header { name: name.into(), value: value.into(), secret: false }
    }

    /// A header whose value is a secret (a token, a key).
    pub fn secret(name: impl Into<String>, value: impl Into<String>) -> Header {
        Header { name: name.into(), value: value.into(), secret: true }
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Header");
        d.field("name", &self.name);
        if self.secret {
            d.field("value", &"***");
        } else {
            d.field("value", &self.value);
        }
        d.finish()
    }
}

/// A request body, sent as is with its `Content-Type`. `Content-Length` is set from the bytes.
#[derive(Clone)]
pub struct Body {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Body").field("len", &self.bytes.len()).field("content_type", &self.content_type).finish()
    }
}

/// One request. A body is allowed on POST, PUT, PATCH and DELETE, not on GET or HEAD (`Error::InvalidRequest`).
#[derive(Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: Option<Body>,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Request {
        Request { method: Method::Get, url: url.into(), headers: Vec::new(), body: None }
    }
}

fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_else(|| "?".to_owned())
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|h| h.name.as_str()).collect();
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("host", &host_of(&self.url))
            .field("headers", &names)
            .field("body_len", &self.body.as_ref().map(|b| b.bytes.len()))
            .finish()
    }
}

/// What happens when the body is longer than `max_reply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Over {
    /// `Error::TooLarge`.
    Fail,
    /// Keep the first `max_reply` bytes, set `Reply::cut`, stop reading.
    Cut,
}

/// Whose root certificates check an https server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Roots {
    /// The OS: its verifier on Windows and macOS, the system's CA files on Linux.
    Os,
    /// The Mozilla list built into the program.
    Bundled,
}

/// Which proxy to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proxy {
    /// The environment variables (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`, either case).
    Env,
    /// Connect directly.
    None,
}

/// How a call behaves.
#[derive(Clone, Debug)]
pub struct Options {
    /// The whole call: lookup, connect, TLS, every redirect hop and the body. Default 30 s.
    pub deadline: Duration,
    /// Body bytes kept. Default 8 MiB.
    pub max_reply: usize,
    /// What happens past `max_reply`. Default `Over::Fail`.
    pub over: Over,
    /// Refuse an `http://` URL at the start. Default false. (A redirect from https to http is always refused.)
    pub https_only: bool,
    /// Hops followed. Default 5; 0 returns the 3xx as a `Reply`.
    pub max_redirects: u8,
    pub roots: Roots,
    pub proxy: Proxy,
    /// Default `ito-fetch/<version>`: no app, user or machine name.
    pub user_agent: String,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            deadline: Duration::from_secs(30),
            max_reply: 8 << 20,
            over: Over::Fail,
            https_only: false,
            max_redirects: 5,
            roots: Roots::Os,
            proxy: Proxy::Env,
            user_agent: concat!("ito-fetch/", env!("CARGO_PKG_VERSION")).to_owned(),
        }
    }
}

/// What the server answered.
#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    /// The final URL after redirects.
    pub url: String,
    pub content_type: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The body was cut at `max_reply` (`Over::Cut`).
    pub cut: bool,
}

impl Reply {
    /// Status 200 to 299.
    pub fn success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    fn body_text(&self) -> &[u8] {
        self.body.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&self.body)
    }

    /// The body as UTF-8 (a BOM is skipped), or `None` when it is not valid UTF-8 (Shift_JIS, a cut mid-character).
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(self.body_text()).ok()
    }

    /// The body as UTF-8 with bad bytes replaced, for a log of a capped body.
    pub fn text_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.body_text())
    }
}

impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reply")
            .field("status", &self.status)
            .field("host", &host_of(&self.url))
            .field("content_type", &self.content_type)
            .field("body_len", &self.body.len())
            .field("cut", &self.cut)
            .finish()
    }
}

/// Send `req`, follow redirects, read the body up to `opts.max_reply`. Blocks: call it from a worker thread.
///
/// Returns within about 50 ms after `stop` is set or `opts.deadline` passes (see the crate doc). The deadline starts at
/// the call, before the request is checked. The request, body included, is copied once to the helper thread.
pub fn request(req: &Request, opts: &Options, stop: &AtomicBool) -> Result<Reply, Error> {
    run::request(req, opts, stop)
}

/// `request` with a plain GET of `url`.
pub fn get(url: &str, opts: &Options, stop: &AtomicBool) -> Result<Reply, Error> {
    request(&Request::get(url), opts, stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_secrets_and_paths() {
        let req = Request {
            method: Method::Post,
            url: "https://api.example/secret/path?token=abc".into(),
            headers: vec![Header::new("Accept", "text/plain"), Header::secret("Authorization", "Bearer s3cret")],
            body: Some(Body { bytes: b"private body".to_vec(), content_type: "text/plain".into() }),
        };
        let shown = format!("{req:?} {:?} {:?}", req.headers[1], req.body);
        for hidden in ["s3cret", "secret/path", "token=abc", "private body"] {
            assert!(!shown.contains(hidden), "{shown}");
        }
        assert!(shown.contains("api.example") && shown.contains("Authorization"), "{shown}");
        assert!(format!("{:?}", req.headers[0]).contains("text/plain"));
    }

    #[test]
    fn text_skips_bom_and_refuses_other_encodings() {
        let mut r = Reply { status: 200, url: "http://h/".into(), content_type: None, headers: vec![], body: vec![], cut: false };
        r.body = [&[0xEF, 0xBB, 0xBF][..], "ab\u{3042}".as_bytes()].concat();
        assert_eq!(r.text(), Some("ab\u{3042}"));
        r.body = vec![0x93, 0xFA, 0x96, 0x7B]; // Shift_JIS
        assert_eq!(r.text(), None);
        assert!(r.text_lossy().contains('\u{FFFD}'));
        assert!(r.success());
    }

    #[test]
    fn defaults() {
        let o = Options::default();
        assert_eq!((o.deadline, o.max_reply, o.max_redirects), (Duration::from_secs(30), 8 << 20, 5));
        assert!(o.user_agent.starts_with("ito-fetch/"));
    }
}
