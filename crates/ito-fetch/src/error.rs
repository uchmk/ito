//! The error type. No text in it carries a header value, a body, or a URL's path, query or user information.

use std::fmt;

pub(crate) const BAD_ADDRESS: &str = "not a valid address";
pub(crate) const BAD_SCHEME: &str = "only http and https addresses are supported";
pub(crate) const NO_HOST: &str = "the address has no host";
pub(crate) const HTTPS_REQUIRED: &str = "https is required";
pub(crate) const PROXY_UNUSABLE: &str = "the proxy setting is not usable";
pub(crate) const PROXY_CONNECT: &str = "could not connect to the proxy";
pub(crate) const PROXY_REFUSED: &str = "the proxy refused the connection";

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

    /// A short phrase for the person, in `lang` (Japanese when it starts with `ja`, as `ja-JP`; anything else is
    /// English), meant to follow a colon in the app's own line. Like `Display`, it names at most a host: never a path,
    /// a query, a header value or a body.
    pub fn words(&self, lang: &str) -> String {
        let ja = is_ja(lang);
        let pick = |j: &str, e: &str| if ja { j.to_string() } else { e.to_string() };
        match self {
            Error::BadUrl(why) => match why.as_str() {
                BAD_SCHEME => pick("http か https のアドレスだけ使えます", "only http and https addresses work"),
                NO_HOST => pick("アドレスにホストがありません", "the address has no host"),
                HTTPS_REQUIRED => pick("https のアドレスが要ります", "an https address is required"),
                _ => pick("アドレスが正しくありません", "the address is not valid"),
            },
            Error::InvalidRequest(_) => pick("要求の形が正しくありません", "the request is not valid"),
            Error::Name(host) => pick(&format!("{host} が見つかりません"), &format!("cannot find {host}")),
            Error::Connect(host) => pick(&format!("{host} につながりません"), &format!("cannot connect to {host}")),
            Error::Tls(host) => pick(&format!("{host} と安全につなげません"), &format!("cannot connect securely to {host}")),
            Error::Proxy(why) => match why.as_str() {
                PROXY_UNUSABLE => pick("プロキシの設定を使えません", "the proxy setting is not usable"),
                PROXY_CONNECT => pick("プロキシにつながりません", "cannot connect to the proxy"),
                PROXY_REFUSED => pick("プロキシが接続を断りました", "the proxy refused the connection"),
                _ => pick("プロキシを使えません", "cannot use the proxy"),
            },
            Error::Timeout => pick("時間内に終わりませんでした", "timed out"),
            Error::Stopped => pick("止めました", "stopped"),
            Error::TooLarge { limit } => {
                let size = size_text(*limit);
                pick(&format!("大きすぎます（{size} まで）"), &format!("too large (the limit is {size})"))
            }
            Error::Redirect(r) => match r {
                Redirect::TooMany => pick("転送が多すぎます", "too many redirects"),
                Redirect::ToHttp => pick("https から http への転送は断りました", "refused a redirect from https to http"),
                Redirect::BadLocation => pick("転送先のアドレスが正しくありません", "the redirect points to an unusable address"),
                Redirect::BodyElsewhere => pick("別のサイトへの転送には本文を送りません", "will not send the body to another site"),
            },
            Error::Broken { .. } => pick("途中で切れました", "the reply ended early"),
            Error::Other(_) => pick("通信に失敗しました", "the request failed"),
        }
    }
}

/// True when `lang` starts with "ja", ignoring case ("ja", "JA", "ja-JP").
fn is_ja(lang: &str) -> bool {
    lang.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("ja"))
}

/// A byte count as a person reads it: `512 B`, `64 KiB`, `1.5 MiB`. Rounds down, one decimal unless whole.
fn size_text(bytes: usize) -> String {
    const UNITS: [(&str, u64); 3] = [("GiB", 1 << 30), ("MiB", 1 << 20), ("KiB", 1 << 10)];
    let n = bytes as u64;
    for (name, unit) in UNITS {
        if n >= unit {
            // No overflow even for a limit near u64::MAX.
            let tenths = n / unit * 10 + n % unit * 10 / unit;
            return if tenths.is_multiple_of(10) {
                format!("{} {name}", tenths / 10)
            } else {
                format!("{}.{} {name}", tenths / 10, tenths % 10)
            };
        }
    }
    format!("{n} B")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn both(e: &Error, ja: &str, en: &str) {
        assert_eq!(e.words("ja"), ja, "{e:?}");
        assert_eq!(e.words("en"), en, "{e:?}");
    }

    #[test]
    fn table() {
        let b = |t: &str| Error::BadUrl(t.into());
        both(&b(BAD_ADDRESS), "アドレスが正しくありません", "the address is not valid");
        both(&b(BAD_SCHEME), "http か https のアドレスだけ使えます", "only http and https addresses work");
        both(&b(NO_HOST), "アドレスにホストがありません", "the address has no host");
        both(&b(HTTPS_REQUIRED), "https のアドレスが要ります", "an https address is required");
        both(&b("zzz"), "アドレスが正しくありません", "the address is not valid");
        both(&Error::InvalidRequest("x".into()), "要求の形が正しくありません", "the request is not valid");
        both(&Error::Name("h.example".into()), "h.example が見つかりません", "cannot find h.example");
        both(&Error::Connect("h.example".into()), "h.example につながりません", "cannot connect to h.example");
        both(&Error::Tls("h.example".into()), "h.example と安全につなげません", "cannot connect securely to h.example");
        let p = |t: &str| Error::Proxy(t.into());
        both(&p(PROXY_UNUSABLE), "プロキシの設定を使えません", "the proxy setting is not usable");
        both(&p(PROXY_CONNECT), "プロキシにつながりません", "cannot connect to the proxy");
        both(&p(PROXY_REFUSED), "プロキシが接続を断りました", "the proxy refused the connection");
        both(&p("zzz"), "プロキシを使えません", "cannot use the proxy");
        both(&Error::Timeout, "時間内に終わりませんでした", "timed out");
        both(&Error::Stopped, "止めました", "stopped");
        both(&Error::TooLarge { limit: 65536 }, "大きすぎます（64 KiB まで）", "too large (the limit is 64 KiB)");
        both(&Error::Redirect(Redirect::TooMany), "転送が多すぎます", "too many redirects");
        both(&Error::Redirect(Redirect::ToHttp), "https から http への転送は断りました", "refused a redirect from https to http");
        both(&Error::Redirect(Redirect::BadLocation), "転送先のアドレスが正しくありません", "the redirect points to an unusable address");
        both(&Error::Redirect(Redirect::BodyElsewhere), "別のサイトへの転送には本文を送りません", "will not send the body to another site");
        both(&Error::Broken { status: 200 }, "途中で切れました", "the reply ended early");
        both(&Error::Other("x".into()), "通信に失敗しました", "the request failed");
    }

    #[test]
    fn other_languages_are_english() {
        for lang in ["", "en", "fr", "jp"] {
            assert_eq!(Error::Timeout.words(lang), "timed out", "{lang}");
        }
        for lang in ["ja", "JA", "ja-JP"] {
            assert_eq!(Error::Timeout.words(lang), "時間内に終わりませんでした", "{lang}");
        }
    }

    #[test]
    fn display_of_constants_unchanged() {
        assert_eq!(Error::BadUrl(BAD_ADDRESS.into()).to_string(), "bad URL: not a valid address");
        assert_eq!(Error::BadUrl(BAD_SCHEME.into()).to_string(), "bad URL: only http and https addresses are supported");
        assert_eq!(Error::BadUrl(NO_HOST.into()).to_string(), "bad URL: the address has no host");
        assert_eq!(Error::BadUrl(HTTPS_REQUIRED.into()).to_string(), "bad URL: https is required");
        assert_eq!(Error::Proxy(PROXY_UNUSABLE.into()).to_string(), "proxy: the proxy setting is not usable");
        assert_eq!(Error::Proxy(PROXY_CONNECT.into()).to_string(), "proxy: could not connect to the proxy");
        assert_eq!(Error::Proxy(PROXY_REFUSED.into()).to_string(), "proxy: the proxy refused the connection");
    }

    #[test]
    fn sizes() {
        let cases: [(usize, &str); 9] = [
            (0, "0 B"),
            (1023, "1023 B"),
            (1024, "1 KiB"),
            (1536, "1.5 KiB"),
            (1048576, "1 MiB"),
            (10485760, "10 MiB"),
            (1610612736, "1.5 GiB"),
            (1048575, "1023.9 KiB"),
            (usize::MAX, if usize::BITS == 64 { "17179869183.9 GiB" } else { "3.9 GiB" }),
        ];
        for (n, want) in cases {
            assert_eq!(size_text(n), want, "{n}");
        }
    }

    #[test]
    fn only_the_host() {
        for lang in ["ja", "en"] {
            let w = Error::Name("x.example".into()).words(lang);
            assert!(w.contains("x.example") && !w.contains('/') && !w.contains('?'), "{w}");
        }
    }
}
