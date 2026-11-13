//! Run a command on a Windows PC over WinRM (the WS-Management cmd shell), with the output streamed as it comes.
//!
//! Same shape as `ito_sftp::exec`: [`exec`] blocks (run it on a worker thread), calls `output` for each piece of
//! stdout / stderr, ends with the exit code, and stops when the `stop` flag is set or the timeout runs out (it then
//! asks the remote command to end and deletes the shell). Pure Rust (rustls + ring), so it runs from Linux and macOS
//! as well. Basic or NTLMv2 authentication ([`Auth`]), over HTTPS with a pinned certificate fingerprint ([`peek_fingerprint`] shows it the
//! first time) or over plain HTTP when the caller says so.

mod http;
mod ntlm;
mod soap;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// SHA-256 fingerprints of certificates.
pub mod fingerprint {
    use sha2::{Digest, Sha256};

    /// The SHA-256 of `der`.
    pub fn sha256(der: &[u8]) -> [u8; 32] {
        Sha256::digest(der).into()
    }

    /// `AB:CD:…`
    pub fn format(f: &[u8; 32]) -> String {
        f.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
    }

    /// Read `AB:CD:…` (also without colons, any case).
    pub fn parse(s: &str) -> Option<[u8; 32]> {
        let hex: String = s.chars().filter(|c| *c != ':' && !c.is_whitespace()).collect();
        if hex.len() != 64 || !hex.is_ascii() {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(out)
    }

    pub(crate) fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
        a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

/// Which stream a piece of output is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Called with each piece of output, in order.
pub type Output<'a> = &'a mut dyn FnMut(Stream, &[u8]);

/// How to sign in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Auth {
    /// User and password in every request. The server must allow Basic (off by default on Windows), and only
    /// local accounts work. Use it over HTTPS.
    #[default]
    Basic,
    /// NTLMv2 (the `Negotiate` / `NTLM` handshake once per connection), for local accounts (`name`) and domain
    /// accounts (`DOMAIN\name`). No message sealing, so use it over HTTPS: on plain HTTP a default WinRM refuses it.
    Ntlm,
}

/// Where and as whom.
#[derive(Debug, Clone)]
pub struct Target {
    pub host: String,
    /// 5986 (HTTPS) or 5985 (HTTP) by default, see [`Target::https`] / [`Target::http`].
    pub port: u16,
    pub https: bool,
    pub user: String,
    pub password: String,
    pub auth: Auth,
    /// The certificate's SHA-256 (HTTPS). `None` checks it against the public roots, which a PC's self-signed
    /// certificate will not pass.
    pub pin: Option<[u8; 32]>,
    /// The URL path, `/wsman` unless changed.
    pub path: String,
    /// Time allowed to connect (name lookup, TCP and the TLS handshake together, for each connection; a reconnect
    /// starts a fresh budget) and, separately, to send each request.
    pub connect_timeout: Duration,
}

impl Target {
    /// HTTPS on 5986.
    pub fn https(host: &str, user: &str, password: &str, pin: Option<[u8; 32]>) -> Target {
        Target {
            host: host.to_owned(),
            port: 5986,
            https: true,
            user: user.to_owned(),
            password: password.to_owned(),
            auth: Auth::Basic,
            pin,
            path: "/wsman".to_owned(),
            connect_timeout: Duration::from_secs(15),
        }
    }

    /// Plain HTTP on 5985. The password crosses the network in the clear (Basic): only for a network you trust.
    pub fn http(host: &str, user: &str, password: &str) -> Target {
        Target { port: 5985, https: false, pin: None, ..Target::https(host, user, password, None) }
    }

    /// The same target signing in as `auth`.
    pub fn with_auth(mut self, auth: Auth) -> Target {
        self.auth = auth;
        self
    }

    fn endpoint(&self) -> http::Endpoint {
        http::Endpoint {
            host: self.host.clone(),
            port: self.port,
            https: self.https,
            path: self.path.clone(),
            pin: self.pin,
            connect_timeout: self.connect_timeout,
            credential: match self.auth {
                Auth::Basic => http::Credential::Basic(http::basic(&self.user, &self.password)),
                Auth::Ntlm => http::Credential::Ntlm { user: self.user.clone(), password: self.password.clone() },
            },
        }
    }

    fn url(&self) -> String {
        format!("{}://{}:{}{}", if self.https { "https" } else { "http" }, self.host, self.port, self.path)
    }
}

/// Options of one [`exec`].
#[derive(Debug, Clone, Default)]
pub struct ExecOptions {
    /// Environment variables for the shell.
    pub env: Vec<(String, String)>,
    /// The whole run (not each wait). `None` waits as long as the command runs.
    pub timeout: Option<Duration>,
}

/// How the command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    pub status: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The host could not be reached.
    Connect(String),
    /// The user name or password was refused (HTTP 401), or the account may not use WinRM (403).
    AuthRefused,
    /// The certificate is not the pinned one; carries the fingerprint it showed (`AB:CD:…`).
    Fingerprint(String),
    /// TLS failed for another reason.
    Tls(String),
    /// The connection broke.
    Lost(String),
    /// The timeout ran out. The remote command was asked to end.
    Timeout,
    /// The `stop` flag was set. The remote command was asked to end.
    Stopped,
    /// The server answered with a fault.
    Fault(String),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Connect(e) => write!(f, "cannot connect: {e}"),
            Error::AuthRefused => f.write_str("the user name or password was refused"),
            Error::Fingerprint(got) => write!(f, "the certificate is not the pinned one (it is {got})"),
            Error::Tls(e) => write!(f, "TLS: {e}"),
            Error::Lost(e) => write!(f, "the connection was lost: {e}"),
            Error::Timeout => f.write_str("timed out"),
            Error::Stopped => f.write_str("stopped"),
            Error::Fault(e) => write!(f, "the server said: {e}"),
            Error::Other(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Error {}

/// The SHA-256 fingerprint of the certificate the host shows on `port`, to show the user once so they can pin it.
/// Nothing is sent to the server beyond the TLS handshake. `timeout` bounds the whole call (name lookup, connect and
/// handshake) and `stop` ends it within about 250 ms. A name lookup still running then is left to end by itself when
/// the system's lookup ends; its answer goes nowhere.
pub fn peek_fingerprint(host: &str, port: u16, timeout: Duration, stop: &AtomicBool) -> Result<[u8; 32], Error> {
    http::peek(host, port, timeout, stop)
}

/// How long one Receive waits on the server before it answers "nothing yet".
const POLL: u32 = 3;

fn check(reply: &http::Reply) -> Result<(), Error> {
    match reply.status {
        200 => Ok(()),
        401 | 403 => Err(Error::AuthRefused),
        _ => match soap::fault(&reply.body) {
            Some((_, text)) => Err(Error::Fault(text)),
            None => Err(Error::Other(format!("the server answered HTTP {}", reply.status))),
        },
    }
}

/// Run `command` (a `cmd.exe` command line) on `target`. Blocks until it ends, so call it from a worker thread.
pub fn exec(target: &Target, command: &str, opts: &ExecOptions, stop: &AtomicBool, output: Output) -> Result<Exit, Error> {
    let ep = target.endpoint();
    let url = target.url();
    let deadline = opts.timeout.map(|t| Instant::now() + t);
    let mut conn = http::Http::new(&ep, stop);
    let a_while = Duration::from_secs(30);

    let reply = conn.post(&soap::create(&url, &opts.env), a_while)?;
    check(&reply)?;
    let shell = soap::text_of(&reply.body, "ShellId").ok_or_else(|| Error::Other("no ShellId in the reply".to_owned()))?;

    let mut cmd_id = None;
    let run = run_command(&mut conn, &url, &shell, command, &mut cmd_id, deadline, stop, output);

    // Clean up even after a stop or a timeout: a fresh connection and a flag nobody sets.
    let never = AtomicBool::new(false);
    let mut tidy = http::Http::new(&ep, &never);
    if let (Err(Error::Stopped | Error::Timeout), Some(cmd)) = (&run, &cmd_id) {
        let _ = tidy.post(&soap::signal(&url, &shell, cmd, soap::SIGNAL_TERMINATE), Duration::from_secs(10));
    }
    let _ = tidy.post(&soap::delete(&url, &shell), Duration::from_secs(10));
    run
}

#[allow(clippy::too_many_arguments)]
fn run_command(
    conn: &mut http::Http,
    url: &str,
    shell: &str,
    command: &str,
    cmd_id: &mut Option<String>,
    deadline: Option<Instant>,
    stop: &AtomicBool,
    output: Output,
) -> Result<Exit, Error> {
    let reply = conn.post(&soap::command(url, shell, command), Duration::from_secs(30))?;
    check(&reply)?;
    let cmd = soap::text_of(&reply.body, "CommandId").ok_or_else(|| Error::Other("no CommandId in the reply".to_owned()))?;
    *cmd_id = Some(cmd.clone());
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        let wait = match deadline {
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(Error::Timeout);
                }
                POLL.min(left.as_secs() as u32 + 1)
            }
            None => POLL,
        };
        let reply = conn.post(&soap::receive(url, shell, &cmd, wait), Duration::from_secs(u64::from(wait) + 20))?;
        if reply.status != 200 {
            if let Some((Some(soap::FAULT_TIMEOUT), _)) = soap::fault(&reply.body) {
                continue;
            }
            check(&reply)?;
        }
        let got = soap::parse_receive(&reply.body);
        for (err, bytes) in &got.chunks {
            output(if *err { Stream::Stderr } else { Stream::Stdout }, bytes);
        }
        if let Some(code) = got.done {
            return Ok(Exit { status: Some(code) });
        }
    }
}
