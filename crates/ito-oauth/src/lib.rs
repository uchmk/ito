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
    /// Whether the same call may work later: a timeout, a lost connection, 429 and 5xx.
    pub fn retryable(&self) -> bool {
        match self {
            Error::Network(_) | Error::Timeout => true,
            Error::Status { code, .. } => *code == 429 || *code >= 500,
            _ => false,
        }
    }
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
