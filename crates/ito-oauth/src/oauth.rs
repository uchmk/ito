//! The installed-app flow (RFC 8252) with PKCE, and a session that keeps the refresh token in the OS credential store.

use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::https::{self, io_err};
use crate::{text, Error};

/// How long before the end an access token counts as spent.
const SKEW: Duration = Duration::from_secs(60);

/// Where a provider keeps its endpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provider {
    /// Short name, part of the key in the credential store.
    pub name: &'static str,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub revoke_url: Option<&'static str>,
    /// Added to the consent address (Google needs `access_type=offline` and `prompt=consent` to give a refresh token).
    pub auth_params: &'static [(&'static str, &'static str)],
}

pub const GOOGLE: Provider = Provider {
    name: "google",
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    revoke_url: Some("https://oauth2.googleapis.com/revoke"),
    auth_params: &[("access_type", "offline"), ("prompt", "consent")],
};

/// The app's registration. A desktop client's "secret" is not secret (Google asks for it all the same); `None` if the
/// provider does not want one.
#[derive(Clone, PartialEq, Eq)]
pub struct Client {
    pub id: String,
    pub secret: Option<String>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client").field("id", &self.id).field("secret", &self.secret.as_ref().map(|_| "…")).finish()
    }
}

/// The two small pages the browser is left on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub done: String,
    pub failed: String,
}

impl Default for Page {
    fn default() -> Self {
        Self { done: "Signed in. You can close this tab.".into(), failed: "Sign-in failed. You can close this tab.".into() }
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    /// How long to wait for the person (default 5 minutes).
    pub timeout: Duration,
    /// Bounds each call to the provider (default 30 seconds).
    pub request_timeout: Duration,
    pub page: Page,
    /// Fills the account box on the consent page.
    pub login_hint: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self { timeout: Duration::from_secs(300), request_timeout: Duration::from_secs(30), page: Page::default(), login_hint: None }
    }
}

/// What the provider handed out. `Debug` hides the tokens.
#[derive(Clone)]
pub struct Tokens {
    pub access: String,
    /// Absent on a refresh, and when the provider was not asked for offline access.
    pub refresh: Option<String>,
    pub expires_at: Instant,
    pub scope: String,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tokens").field("scope", &self.scope).field("has_refresh", &self.refresh.is_some()).finish_non_exhaustive()
    }
}

/// Sign in through the browser. `open` gets the consent address and shows it (open the browser, or print it). Blocks until
/// the person answers, `stop` is raised ([`Error::Cancelled`]) or `options.timeout` passes ([`Error::Timeout`]).
pub fn sign_in<F: FnOnce(&str)>(
    provider: &Provider,
    client: &Client,
    scopes: &[&str],
    open: F,
    stop: &AtomicBool,
    options: &Options,
) -> Result<Tokens, Error> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io_err)?;
    listener.set_nonblocking(true).map_err(io_err)?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(io_err)?.port());
    let verifier = random(32)?;
    let state = random(16)?;
    let challenge = text::b64url(&Sha256::digest(verifier.as_bytes()));
    let scope = scopes.join(" ");
    let mut pairs: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", &client.id),
        ("redirect_uri", &redirect),
        ("scope", &scope),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
    ];
    pairs.extend_from_slice(provider.auth_params);
    if let Some(h) = &options.login_hint {
        pairs.push(("login_hint", h));
    }
    let sep = if provider.auth_url.contains('?') { '&' } else { '?' };
    open(&format!("{}{sep}{}", provider.auth_url, text::form(&pairs)));

    let deadline = Instant::now() + options.timeout;
    let code = loop {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        match listener.accept() {
            Ok((mut s, _)) => {
                if let Some(code) = answer(&mut s, &state, &options.page)? {
                    break code;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => return Err(io_err(e)),
        }
    };
    token_call(
        provider,
        client,
        &[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", &redirect), ("code_verifier", &verifier)],
        options.request_timeout,
    )
}

/// Handle one request on the loopback. `Some(code)` when it is the redirect with our state; `None` for anything else
/// (a favicon, a stray or forged request), which does not end the wait.
fn answer(s: &mut TcpStream, state: &str, page: &Page) -> Result<Option<String>, Error> {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut head = Vec::new();
    let mut buf = [0u8; 2048];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < 16 * 1024 {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let target = head.lines().next().and_then(|l| l.strip_prefix("GET ")).and_then(|l| l.split(' ').next()).unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = text::parse_query(query);
    let get = |k: &str| query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if path != "/" || get("state") != Some(state) {
        reply(s, 404, "Not Found", "");
        return Ok(None);
    }
    if let Some(e) = get("error") {
        reply(s, 200, "OK", &page.failed);
        return Err(Error::Denied(e.to_owned()));
    }
    match get("code") {
        Some(c) if !c.is_empty() => {
            reply(s, 200, "OK", &page.done);
            Ok(Some(c.to_owned()))
        }
        _ => {
            reply(s, 400, "Bad Request", &page.failed);
            Ok(None)
        }
    }
}

fn reply(s: &mut TcpStream, code: u16, reason: &str, message: &str) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{m}</title><body style=\"font-family:sans-serif;margin:3em\"><p>{m}</p>",
        m = text::html_escape(message)
    );
    let out = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(out.as_bytes());
    let _ = s.flush();
}

fn random(bytes: usize) -> Result<String, Error> {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).map_err(|e| Error::Bad(format!("no randomness: {e}")))?;
    Ok(text::b64url(&b))
}

#[derive(Deserialize)]
struct Raw {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
}

fn grant_refused(e: Error) -> Error {
    match &e {
        Error::Status { code: 400 | 401, body, .. } if body.contains("invalid_grant") => Error::Refused,
        _ => e,
    }
}

fn token_call(provider: &Provider, client: &Client, pairs: &[(&str, &str)], timeout: Duration) -> Result<Tokens, Error> {
    let mut all = pairs.to_vec();
    all.push(("client_id", &client.id));
    if let Some(s) = &client.secret {
        all.push(("client_secret", s));
    }
    let body = text::form(&all);
    let r = https::request("POST", provider.token_url, None, Some(("application/x-www-form-urlencoded", body.as_bytes())), timeout)
        .map_err(grant_refused)?;
    let raw: Raw = r.json()?;
    let access = raw.access_token.filter(|a| !a.is_empty()).ok_or_else(|| Error::Bad("no access_token in the answer".to_owned()))?;
    Ok(Tokens {
        access,
        refresh: raw.refresh_token.filter(|r| !r.is_empty()),
        expires_at: Instant::now() + Duration::from_secs(raw.expires_in.unwrap_or(3600)),
        scope: raw.scope.unwrap_or_default(),
    })
}

/// Where the refresh token lives.
pub(crate) trait Store: Send {
    fn get(&self, key: &str) -> std::io::Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> std::io::Result<()>;
    fn delete(&self, key: &str) -> std::io::Result<()>;
}

/// The OS credential store, through `ito-secret`.
struct Os(String);

impl Store for Os {
    fn get(&self, key: &str) -> std::io::Result<Option<String>> {
        ito_secret::get(&self.0, key)
    }
    fn set(&self, key: &str, value: &str) -> std::io::Result<()> {
        ito_secret::set(&self.0, key, value)
    }
    fn delete(&self, key: &str) -> std::io::Result<()> {
        ito_secret::delete(&self.0, key)
    }
}

/// A signed-in account: the refresh token is in the credential store, the access token in memory. `Debug` hides both.
pub struct Session {
    key: String,
    provider: Provider,
    client: Client,
    store: Box<dyn Store>,
    access: String,
    expires_at: Instant,
    refresh: String,
    timeout: Duration,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session").field("key", &self.key).finish_non_exhaustive()
    }
}

fn secret_err(e: std::io::Error) -> Error {
    Error::Secret(e.to_string())
}

impl Session {
    /// Keep a new sign-in: the refresh token goes to the credential store under `app` (the same name `ito-secret` uses).
    /// Without a refresh token (the provider was not asked for offline access) it is [`Error::Bad`].
    pub fn start(app: &str, account: &str, provider: Provider, client: Client, tokens: Tokens) -> Result<Session, Error> {
        Self::start_in(Box::new(Os(app.to_owned())), account, provider, client, tokens)
    }

    /// The account signed in before, or `None` when nothing is stored (sign in). The first [`Session::token`] refreshes.
    pub fn resume(app: &str, account: &str, provider: Provider, client: Client) -> Result<Option<Session>, Error> {
        Self::resume_in(Box::new(Os(app.to_owned())), account, provider, client)
    }

    pub(crate) fn start_in(
        store: Box<dyn Store>,
        account: &str,
        provider: Provider,
        client: Client,
        tokens: Tokens,
    ) -> Result<Session, Error> {
        let refresh = tokens.refresh.ok_or_else(|| Error::Bad("no refresh token (ask the provider for offline access)".to_owned()))?;
        let key = format!("oauth-{}-{account}", provider.name);
        store.set(&key, &refresh).map_err(secret_err)?;
        Ok(Session {
            key,
            provider,
            client,
            store,
            access: tokens.access,
            expires_at: tokens.expires_at,
            refresh,
            timeout: Duration::from_secs(30),
        })
    }

    pub(crate) fn resume_in(store: Box<dyn Store>, account: &str, provider: Provider, client: Client) -> Result<Option<Session>, Error> {
        let key = format!("oauth-{}-{account}", provider.name);
        Ok(store.get(&key).map_err(secret_err)?.filter(|r| !r.is_empty()).map(|refresh| Session {
            key,
            provider,
            client,
            store,
            access: String::new(),
            expires_at: Instant::now(),
            refresh,
            timeout: Duration::from_secs(30),
        }))
    }

    /// An access token good for at least a minute, refreshed when needed. [`Error::Refused`] means the grant is gone
    /// (revoked, expired): the stored secret has been deleted, sign in again.
    pub fn token(&mut self) -> Result<String, Error> {
        if !self.access.is_empty() && Instant::now() + SKEW < self.expires_at {
            return Ok(self.access.clone());
        }
        let fresh =
            token_call(&self.provider, &self.client, &[("grant_type", "refresh_token"), ("refresh_token", &self.refresh)], self.timeout);
        match fresh {
            Ok(t) => {
                if let Some(r) = t.refresh.filter(|r| *r != self.refresh) {
                    self.store.set(&self.key, &r).map_err(secret_err)?;
                    self.refresh = r;
                }
                self.access = t.access;
                self.expires_at = t.expires_at;
                Ok(self.access.clone())
            }
            Err(Error::Refused) => {
                let _ = self.store.delete(&self.key);
                self.access.clear();
                Err(Error::Refused)
            }
            Err(e) => Err(e),
        }
    }

    /// Sign out: delete the stored refresh token, then ask the provider to revoke it. The secret is gone even when the
    /// revoke fails; that failure (other than "already invalid") is returned so the app can say so.
    pub fn sign_out(self) -> Result<(), Error> {
        self.store.delete(&self.key).map_err(secret_err)?;
        let Some(url) = self.provider.revoke_url else { return Ok(()) };
        let body = text::form(&[("token", &self.refresh)]);
        match https::request("POST", url, None, Some(("application/x-www-form-urlencoded", body.as_bytes())), self.timeout) {
            Ok(_) | Err(Error::Status { code: 400, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<HashMap<String, String>>>);

    impl Store for Fake {
        fn get(&self, k: &str) -> std::io::Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn set(&self, k: &str, v: &str) -> std::io::Result<()> {
            self.0.lock().unwrap().insert(k.to_owned(), v.to_owned());
            Ok(())
        }
        fn delete(&self, k: &str) -> std::io::Result<()> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }

    fn client() -> Client {
        Client { id: "cid".into(), secret: Some("csec".into()) }
    }

    /// Answer each connection in turn with the next canned answer; the thread returns the requests (head and body).
    fn serve(answers: Vec<&'static str>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for a in answers {
                let (mut s, _) = l.accept().unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = s.read(&mut buf).unwrap();
                    raw.extend_from_slice(&buf[..n]);
                    let t = String::from_utf8_lossy(&raw).into_owned();
                    if let Some(i) = t.find("\r\n\r\n") {
                        let want =
                            t.lines().find_map(|l| l.strip_prefix("Content-Length: ")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                        if raw.len() >= i + 4 + want || n == 0 {
                            break;
                        }
                    } else if n == 0 {
                        break;
                    }
                }
                seen.push(String::from_utf8_lossy(&raw).into_owned());
                s.write_all(a.as_bytes()).unwrap();
            }
            seen
        });
        (url, h)
    }

    fn json(body: &'static str) -> String {
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())
    }

    fn leak(s: String) -> &'static str {
        Box::leak(s.into_boxed_str())
    }

    fn provider(token: &str, revoke: Option<String>) -> Provider {
        Provider {
            name: "test",
            auth_url: "http://127.0.0.1:1/auth",
            token_url: leak(format!("{token}/token")),
            revoke_url: revoke.map(|r| leak(format!("{r}/revoke"))),
            auth_params: &[("access_type", "offline")],
        }
    }

    fn query_of(url: &str) -> Vec<(String, String)> {
        text::parse_query(url.split_once('?').unwrap().1)
    }

    fn value(q: &[(String, String)], k: &str) -> String {
        q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default()
    }

    #[test]
    fn pkce_challenge_matches_the_rfc_example() {
        let c = text::b64url(&Sha256::digest(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"));
        assert_eq!(c, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn sign_in_sends_pkce_and_exchanges_the_code() {
        let (srv, seen) = serve(vec![leak(json(r#"{"access_token":"AT","refresh_token":"RT","expires_in":3600,"scope":"s1 s2"}"#))]);
        let p = provider(&srv, None);
        let stop = AtomicBool::new(false);
        let shown = Arc::new(Mutex::new(String::new()));
        let shown2 = shown.clone();
        let t = sign_in(
            &p,
            &client(),
            &["s1", "s2"],
            move |url| {
                *shown2.lock().unwrap() = url.to_owned();
                let q = query_of(url);
                let hit = format!("{}/?code=the%20code&state={}", value(&q, "redirect_uri"), text::encode(&value(&q, "state")));
                std::thread::spawn(move || {
                    // a favicon and a forged state first: neither ends the wait
                    let base = hit.split("/?").next().unwrap().to_owned();
                    assert!(https::request("GET", &format!("{base}/favicon.ico"), None, None, Duration::from_secs(3)).is_err());
                    assert!(https::request("GET", &format!("{base}/?code=evil&state=nope"), None, None, Duration::from_secs(3)).is_err());
                    let r = https::request("GET", &hit, None, None, Duration::from_secs(3)).unwrap();
                    assert!(r.text().contains("Signed in"));
                });
            },
            &stop,
            &Options::default(),
        )
        .unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_deref(), t.scope.as_str()), ("AT", Some("RT"), "s1 s2"));
        let q = query_of(&shown.lock().unwrap());
        assert_eq!(value(&q, "response_type"), "code");
        assert_eq!(value(&q, "scope"), "s1 s2");
        assert_eq!(value(&q, "code_challenge_method"), "S256");
        assert_eq!(value(&q, "access_type"), "offline");
        assert!(value(&q, "redirect_uri").starts_with("http://127.0.0.1:"));
        let req = &seen.join().unwrap()[0];
        let form = text::parse_query(req.split("\r\n\r\n").nth(1).unwrap());
        assert_eq!(value(&form, "code"), "the code");
        assert_eq!(value(&form, "grant_type"), "authorization_code");
        assert_eq!(value(&form, "client_secret"), "csec");
        assert_eq!(text::b64url(&Sha256::digest(value(&form, "code_verifier").as_bytes())), value(&q, "code_challenge"));
        assert_eq!(value(&form, "redirect_uri"), value(&q, "redirect_uri"));
    }

    #[test]
    fn a_no_in_the_browser_is_denied() {
        let p = provider("http://127.0.0.1:1", None);
        let stop = AtomicBool::new(false);
        let e = sign_in(
            &p,
            &client(),
            &["s"],
            |url| {
                let q = query_of(url);
                let hit = format!("{}/?error=access_denied&state={}", value(&q, "redirect_uri"), text::encode(&value(&q, "state")));
                std::thread::spawn(move || {
                    let _ = https::request("GET", &hit, None, None, Duration::from_secs(3));
                });
            },
            &stop,
            &Options::default(),
        )
        .unwrap_err();
        assert_eq!(e, Error::Denied("access_denied".to_owned()));
    }

    #[test]
    fn the_wait_ends_on_stop_and_on_timeout() {
        let p = provider("http://127.0.0.1:1", None);
        let stop = AtomicBool::new(true);
        assert_eq!(sign_in(&p, &client(), &["s"], |_| {}, &stop, &Options::default()).unwrap_err(), Error::Cancelled);
        let stop = AtomicBool::new(false);
        let o = Options { timeout: Duration::from_millis(150), ..Options::default() };
        assert_eq!(sign_in(&p, &client(), &["s"], |_| {}, &stop, &o).unwrap_err(), Error::Timeout);
    }

    fn tokens(refresh: Option<&str>, life: Duration) -> Tokens {
        Tokens { access: "A0".into(), refresh: refresh.map(str::to_owned), expires_at: Instant::now() + life, scope: String::new() }
    }

    #[test]
    fn start_stores_the_refresh_token_and_needs_one() {
        let store = Fake::default();
        let p = provider("http://127.0.0.1:1", None);
        assert!(matches!(
            Session::start_in(Box::new(store.clone()), "me", p, client(), tokens(None, Duration::from_secs(10))),
            Err(Error::Bad(_))
        ));
        let s = Session::start_in(Box::new(store.clone()), "me", p, client(), tokens(Some("RT"), Duration::from_secs(10))).unwrap();
        assert_eq!(store.get("oauth-test-me").unwrap().as_deref(), Some("RT"));
        let shown = format!("{s:?} {:?}", tokens(Some("RT"), Duration::ZERO));
        assert!(!shown.contains("RT") && !shown.contains("A0"), "{shown}");
        assert!(Session::resume_in(Box::new(store.clone()), "other", p, client()).unwrap().is_none());
        assert!(Session::resume_in(Box::new(store), "me", p, client()).unwrap().is_some());
    }

    #[test]
    fn a_fresh_token_is_reused_and_an_old_one_is_refreshed() {
        let (srv, seen) = serve(vec![
            leak(json(r#"{"access_token":"A1","expires_in":3600}"#)),
            leak(json(r#"{"access_token":"A2","refresh_token":"RT2","expires_in":3600}"#)),
        ]);
        let store = Fake::default();
        let p = provider(&srv, None);
        let mut s = Session::start_in(Box::new(store.clone()), "me", p, client(), tokens(Some("RT"), Duration::from_secs(3600))).unwrap();
        assert_eq!(s.token().unwrap(), "A0", "no call while fresh");
        s.expires_at = Instant::now() + Duration::from_secs(30);
        assert_eq!(s.token().unwrap(), "A1", "inside the skew it refreshes");
        assert_eq!(store.get("oauth-test-me").unwrap().as_deref(), Some("RT"), "no new refresh token: keep the old");
        s.expires_at = Instant::now();
        assert_eq!(s.token().unwrap(), "A2");
        assert_eq!(store.get("oauth-test-me").unwrap().as_deref(), Some("RT2"), "a rotated refresh token is stored");
        let reqs = seen.join().unwrap();
        let form = text::parse_query(reqs[0].split("\r\n\r\n").nth(1).unwrap());
        assert_eq!((value(&form, "grant_type").as_str(), value(&form, "refresh_token").as_str()), ("refresh_token", "RT"));
    }

    #[test]
    fn invalid_grant_is_refused_and_forgets_the_secret() {
        let (srv, _) = serve(vec!["HTTP/1.1 400 Bad Request\r\nContent-Length: 31\r\n\r\n{\"error\":\"invalid_grant\",\"x\":1}\n"]);
        let store = Fake::default();
        let mut s =
            Session::start_in(Box::new(store.clone()), "me", provider(&srv, None), client(), tokens(Some("RT"), Duration::ZERO)).unwrap();
        assert_eq!(s.token().unwrap_err(), Error::Refused);
        assert!(store.get("oauth-test-me").unwrap().is_none());
    }

    #[test]
    fn a_server_error_is_retryable_and_keeps_the_secret() {
        let (srv, _) = serve(vec!["HTTP/1.1 503 Unavailable\r\nRetry-After: 5\r\nContent-Length: 0\r\n\r\n"]);
        let store = Fake::default();
        let mut s =
            Session::start_in(Box::new(store.clone()), "me", provider(&srv, None), client(), tokens(Some("RT"), Duration::ZERO)).unwrap();
        let e = s.token().unwrap_err();
        assert!(e.retryable(), "{e:?}");
        assert!(store.get("oauth-test-me").unwrap().is_some());
    }

    #[test]
    fn sign_out_deletes_then_revokes() {
        let (srv, seen) = serve(vec!["HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"]);
        let store = Fake::default();
        let s = Session::start_in(
            Box::new(store.clone()),
            "me",
            provider("http://127.0.0.1:1", Some(srv)),
            client(),
            tokens(Some("RT"), Duration::ZERO),
        )
        .unwrap();
        s.sign_out().unwrap();
        assert!(store.get("oauth-test-me").unwrap().is_none());
        let req = &seen.join().unwrap()[0];
        assert!(req.starts_with("POST /revoke "), "{req}");
        assert!(req.ends_with("token=RT"), "{req}");
    }

    #[test]
    fn sign_out_deletes_the_secret_even_when_the_revoke_fails() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let dead = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        drop(l);
        let store = Fake::default();
        let s = Session::start_in(
            Box::new(store.clone()),
            "me",
            provider("http://127.0.0.1:1", Some(dead)),
            client(),
            tokens(Some("RT"), Duration::ZERO),
        )
        .unwrap();
        assert!(s.sign_out().is_err());
        assert!(store.get("oauth-test-me").unwrap().is_none());
    }
}
