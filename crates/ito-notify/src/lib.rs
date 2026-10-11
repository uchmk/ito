//! Telling a person from an app, three ways: the system's notification
//! ([`toast`]), a sound ([`sound`]) and a message sent elsewhere
//! ([`webhook`]: an ntfy topic, a Slack incoming webhook, or any address that
//! takes JSON). Each runs on a thread of its own and a failure only goes to
//! stderr, so a caller never waits on the system. No HTTP client is built in:
//! the webhook goes through the system's `curl`.
//!
//! The words of the settings (`[notify] sound_waiting`, `webhook_format`)
//! are the same in every app that uses this: see [`SOUNDS`] and [`FORMATS`].

pub mod notifier;
pub mod sound;
pub mod toast;
pub mod webhook;

/// The sounds [`sound::play`] knows, by their words.
pub const SOUNDS: [&str; 4] = ["chime", "low", "alert", "default"];

/// The addresses [`webhook::send`] speaks to, by their words.
pub const FORMATS: [&str; 3] = ["ntfy", "slack", "json"];

/// Whether `url` is an address a webhook may be sent to.
pub fn is_http(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("http://") || u.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_addresses_are_sent_to() {
        assert!(is_http("https://ntfy.sh/topic") && is_http(" http://localhost:8080/x"));
        assert!(!is_http("file:///etc/passwd") && !is_http("") && !is_http("ftp://x"));
    }
}
