//! The error type. No text in it carries a header value, a body, or a URL's path, query or user information.

use std::fmt;

/// Why a redirect was not followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Redirect {
    /// More hops than `Options::max_redirects`.
    TooMany,
    /// An `https://` page sent the client to `http://`. Never followed.
    ToHttp,
    /// The `Location` is not a usable http(s) URL (or has user information).
    BadLocation,
    /// A 307 or 308 with a request body points to another origin. The body is not sent there.
    BodyElsewhere,
}

/// Why a request did not give a [`Reply`](crate::Reply). An HTTP status (even 500) is never an error.
///
/// The text of each variant names at most a host.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The URL cannot be parsed, is not `http` or `https`, or is `http` with `https_only`. The text never holds the URL.
    BadUrl(String),
    /// The request itself is wrong (a body on GET, a reserved header, a malformed header, user information in the
    /// URL). The text names a header, never a value.
    InvalidRequest(String),
    /// Name lookup failed. The text is the host.
    Name(String),
    /// Refused, unreachable, or reset before a reply. The text is the host.
    Connect(String),
    /// The certificate was refused or the handshake failed. The text is the host.
    Tls(String),
    /// The proxy refused or could not be used.
    Proxy(String),
    /// The deadline passed.
    Timeout,
    /// The stop flag was set.
    Stopped,
    /// The body is longer than `max_reply` (with `Over::Fail`).
    TooLarge { limit: usize },
    /// A redirect was not followed.
    Redirect(Redirect),
    /// The status arrived but the body broke (reset, early end).
    Broken { status: u16 },
    /// Anything else. The text is a fixed category, never a ureq message.
    Other(String),
}

impl Error {
    /// True when trying again can help: `Name`, `Connect`, `Proxy`, `Timeout`.
    ///
    /// For `Connect` (a reset) and `Timeout`, a request that changes something may already have run on the
    /// server. The caller decides.
    pub fn retryable(&self) -> bool {
        matches!(self, Error::Name(_) | Error::Connect(_) | Error::Proxy(_) | Error::Timeout)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadUrl(why) => write!(f, "bad URL: {why}"),
            Error::InvalidRequest(why) => write!(f, "invalid request: {why}"),
            Error::Name(host) => write!(f, "could not look up {host}"),
            Error::Connect(host) => write!(f, "could not connect to {host}"),
            Error::Tls(host) => write!(f, "the secure connection to {host} failed (certificate or handshake)"),
            Error::Proxy(why) => write!(f, "proxy: {why}"),
            Error::Timeout => f.write_str("timed out"),
            Error::Stopped => f.write_str("stopped"),
            Error::TooLarge { limit } => write!(f, "the reply is larger than {limit} bytes"),
            Error::Redirect(r) => f.write_str(match r {
                Redirect::TooMany => "too many redirects",
                Redirect::ToHttp => "refused a redirect from https to http",
                Redirect::BadLocation => "the redirect has no usable location",
                Redirect::BodyElsewhere => "refused to send the request body to another origin on a redirect",
            }),
            Error::Broken { status } => write!(f, "the reply (status {status}) broke before its end"),
            Error::Other(why) => write!(f, "request failed: {why}"),
        }
    }
}

impl std::error::Error for Error {}
