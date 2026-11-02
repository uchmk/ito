//! OAuth 2.0 for a desktop app, and the HTTPS client it needs. Pure Rust (rustls + ring, webpki roots), blocking: call
//! it from a worker thread.
//!
//! - [`https::request`]: GET / POST / PUT / DELETE against a public host with a Bearer token and a JSON or raw body. Only
//!   2xx comes back as `Ok`; anything else is [`Error::Status`] with the code, the `Retry-After` and the body, so the
//!   caller can tell 429 / 5xx (back off) from 4xx.
//! - [`sign_in`]: the installed-app flow of RFC 8252 — open the consent page in the browser, take the code on
//!   `http://127.0.0.1:<random port>`, exchange it with PKCE (S256). A stop flag and a timeout end the wait.
//! - [`Session`]: keeps the refresh token in `ito-secret` (never a file), hands out an access token (refreshing it when
//!   it is near its end), and on sign-out revokes the token and deletes the secret.
//!
//! The access token and the refresh token never reach a file or a log: the `Debug` of [`Tokens`] and [`Session`] hides them.
//!
//! ```no_run
//! use std::sync::atomic::AtomicBool;
//! use ito_oauth::{sign_in, Client, Options, Session, GOOGLE};
//! let client = Client { id: "…apps.googleusercontent.com".into(), secret: None };
//! let stop = AtomicBool::new(false);
//! let tokens = sign_in(&GOOGLE, &client, &["https://www.googleapis.com/auth/gmail.readonly"], |url| println!("{url}"), &stop, &Options::default())?;
//! let mut session = Session::start("fumi", "me@example.com", GOOGLE, client, tokens)?;
//! let bearer = session.token()?;
//! # Ok::<(), ito_oauth::Error>(())
//! ```

pub mod https;
mod oauth;
mod text;

use std::fmt;
use std::time::Duration;

pub use oauth::{sign_in, Client, Options, Page, Provider, Session, Tokens, GOOGLE};

/// The client built into the app: reads `<PREFIX>_CLIENT_ID` and `<PREFIX>_CLIENT_SECRET` from the environment **of the
/// crate being compiled** (`option_env!`, so cargo rebuilds it when they change) and gives an `Option<Client>`. Without
/// them, or with blank ones (a local build, a fork's pull request), it is `None`.
///
/// Order of choice: the client the person imported > this one > none (ask them to import one): `Client::choose`.
///
/// ```
/// let built_in: Option<ito_oauth::Client> = ito_oauth::built_in_client!("UCHMK_GOOGLE");
/// let client = ito_oauth::Client::choose(None, built_in);
/// # let _ = client;
/// ```
#[macro_export]
macro_rules! built_in_client {
    ($prefix:literal) => {
        $crate::Client::from_build(
            ::core::option_env!(::core::concat!($prefix, "_CLIENT_ID")),
            ::core::option_env!(::core::concat!($prefix, "_CLIENT_SECRET")),
        )
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The address is not `https://host[:port]/path` (or `http://` to the loopback).
    Url(String),
    /// The host is not reachable, or the connection broke.
    Network(String),
    /// The TLS handshake failed (including a certificate that does not check out).
    Tls(String),
    /// The answer is not an HTTP/1.1 answer, or is too large, or lacks what OAuth needs.
    Bad(String),
    /// The server answered with something other than 2xx. `retry_after` is its `Retry-After` in seconds, when it gave one.
    Status { code: u16, retry_after: Option<Duration>, body: String },
    /// The grant is no longer good (`invalid_grant`): the person has to sign in again. The stored secret is deleted.
    Refused,
    /// The person said no in the browser (`access_denied`), or the provider reported another error.
    Denied(String),
    /// The stop flag was raised.
    Cancelled,
    /// Nothing happened in time.
    Timeout,
    /// The OS credential store failed.
    Secret(String),
}

impl Error {
    /// Whether the same call may work later: a timeout, a lost connection, 429, 5xx, and Google's quota 403
    /// (see [`Error::rate_limited`]).
    pub fn retryable(&self) -> bool {
        match self {
            Error::Network(_) | Error::Timeout => true,
            Error::Status { code, .. } => *code == 429 || *code >= 500 || self.rate_limited().is_some(),
            _ => false,
        }
    }

    /// `Some(wait)` when the server said "too many requests": 429, or a 403 whose JSON body carries Google's
    /// `rateLimitExceeded` / `userRateLimitExceeded` / `RATE_LIMIT_EXCEEDED` (Gmail answers a per-user quota with 403).
    /// `wait` is the `Retry-After` when there was one, otherwise 60 s (the quotas are per minute).
    pub fn rate_limited(&self) -> Option<Duration> {
        let Error::Status { code, retry_after, body } = self else { return None };
        if *code == 429 || (*code == 403 && google_rate_limit(body)) {
            Some(retry_after.unwrap_or(Duration::from_secs(60)))
        } else {
            None
        }
    }
}

/// Whether a Google error body names a rate-limit reason (`error.errors[].reason` or `error.details[].reason`).
fn google_rate_limit(body: &str) -> bool {
    const REASONS: [&str; 3] = ["rateLimitExceeded", "userRateLimitExceeded", "RATE_LIMIT_EXCEEDED"];
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { return false };
    ["errors", "details"]
        .iter()
        .any(|k| v["error"][k].as_array().is_some_and(|a| a.iter().any(|e| e["reason"].as_str().is_some_and(|r| REASONS.contains(&r)))))
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Url(s) => write!(f, "bad address: {s}"),
            Error::Network(s) => write!(f, "cannot reach the server: {s}"),
            Error::Tls(s) => write!(f, "TLS failed: {s}"),
            Error::Bad(s) => write!(f, "bad answer: {s}"),
            Error::Status { code, body, .. } => write!(f, "the server answered {code}: {body}"),
            Error::Refused => write!(f, "the sign-in is no longer valid; sign in again"),
            Error::Denied(s) => write!(f, "sign-in was not allowed: {s}"),
            Error::Cancelled => write!(f, "cancelled"),
            Error::Timeout => write!(f, "timed out"),
            Error::Secret(s) => write!(f, "credential store: {s}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16, retry_after: Option<u64>, body: &str) -> Error {
        Error::Status { code, retry_after: retry_after.map(Duration::from_secs), body: body.to_owned() }
    }

    #[test]
    fn rate_limited_reads_the_google_reason() {
        let errors = r#"{"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}"#;
        let details = r#"{"error":{"details":[{"reason":"RATE_LIMIT_EXCEEDED"}]}}"#;
        assert_eq!(status(403, None, errors).rate_limited(), Some(Duration::from_secs(60)));
        assert_eq!(status(403, Some(5), details).rate_limited(), Some(Duration::from_secs(5)));
        assert_eq!(status(429, None, "").rate_limited(), Some(Duration::from_secs(60)));
        assert!(status(403, None, errors).retryable());
    }

    #[test]
    fn other_403s_are_not_rate_limits() {
        assert_eq!(status(403, None, r#"{"error":{"errors":[{"reason":"forbidden"}]}}"#).rate_limited(), None);
        assert_eq!(status(403, None, "rateLimitExceeded").rate_limited(), None);
        assert_eq!(status(500, None, r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#).rate_limited(), None);
        assert!(!status(403, None, "").retryable());
    }
}
