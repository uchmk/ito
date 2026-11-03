//! A fake SMTP server. See [`SmtpServer`].

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rustls::{ServerConnection, StreamOwned};

use crate::tls::{self, Cert, Fingerprint};
use crate::{is_timeout, unique, Acceptor};

/// How the connection is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// No TLS at all; `STARTTLS` is not offered.
    Plain,
    /// Plaintext first, `STARTTLS` offered (like port 587). With credentials, `AUTH` is offered only after it.
    StartTls,
    /// TLS from the first byte (like port 465).
    ImplicitTls,
}

/// A message the server accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// `MAIL FROM`, without the angle brackets.
    pub from: String,
    /// The accepted `RCPT TO`, without the angle brackets.
    pub to: Vec<String>,
    /// The message as received: CRLF lines, dot-stuffing undone, no final `.` line.
    pub raw: Vec<u8>,
    /// The `.eml` file it was also written to.
    pub path: PathBuf,
}

impl Message {
    /// The message as text (lossy).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.raw).into_owned()
    }
}

#[derive(Default)]
struct Switches {
    reject: Vec<String>,
    cut_during_data: bool,
}

struct State {
    mode: Mode,
    credentials: Option<(String, String)>,
    cert: Option<Cert>,
    dir: PathBuf,
    switches: Mutex<Switches>,
    messages: Mutex<Vec<Message>>,
    log: Mutex<Vec<String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A fake SMTP server on `127.0.0.1`. Accepted messages are kept in memory ([`messages`](Self::messages)) and as
/// `.eml` files in [`eml_dir`](Self::eml_dir); everything said on the wire is in [`log`](Self::log). It stops, and
/// its folder goes, when it is dropped.
pub struct SmtpServer {
    state: Arc<State>,
    acceptor: Acceptor,
}

impl SmtpServer {
    /// Start a server in `mode` that takes mail with no login.
    pub fn start(mode: Mode) -> io::Result<SmtpServer> {
        Self::start_with(mode, None)
    }

    /// Start a server that wants `AUTH PLAIN` or `AUTH LOGIN` with this user and password before `MAIL` (535 for
    /// any other login, 530 for none).
    pub fn start_with_login(mode: Mode, user: &str, password: &str) -> io::Result<SmtpServer> {
        Self::start_with(mode, Some((user.to_owned(), password.to_owned())))
    }

    fn start_with(mode: Mode, credentials: Option<(String, String)>) -> io::Result<SmtpServer> {
        let dir = std::env::temp_dir().join(format!("ito-testserver-smtp-{}-{}", std::process::id(), unique()));
        std::fs::create_dir_all(&dir)?;
        let cert = (mode != Mode::Plain).then(Cert::generate);
        let state = Arc::new(State {
            mode,
            credentials,
            cert,
            dir,
            switches: Mutex::new(Switches::default()),
            messages: Mutex::new(Vec::new()),
            log: Mutex::new(Vec::new()),
        });
        let st = state.clone();
        let acceptor = Acceptor::start(move |stream, stop| {
            let _ = session(&st, stream, &stop);
        })?;
        Ok(SmtpServer { state, acceptor })
    }

    pub fn addr(&self) -> SocketAddr {
        self.acceptor.addr
    }

    pub fn port(&self) -> u16 {
        self.acceptor.addr.port()
    }

    pub fn mode(&self) -> Mode {
        self.state.mode
    }

    /// The SHA-256 of the certificate (`None` in [`Mode::Plain`]). Pin it with [`tls::pinned_client_config`].
    pub fn fingerprint(&self) -> Option<Fingerprint> {
        self.state.cert.as_ref().map(Cert::fingerprint)
    }

    /// [`fingerprint`](Self::fingerprint) as `AB:CD:...`.
    pub fn fingerprint_text(&self) -> Option<String> {
        self.fingerprint().map(|f| tls::format(&f))
    }

    /// The certificate (DER), for a client that wants to trust it by value.
    pub fn certificate_der(&self) -> Option<Vec<u8>> {
        self.state.cert.as_ref().map(|c| c.der().to_vec())
    }

    /// The folder the `.eml` files are written to (`0001.eml`, `0002.eml`, ...).
    pub fn eml_dir(&self) -> &Path {
        &self.state.dir
    }

    /// The messages accepted so far, oldest first.
    pub fn messages(&self) -> Vec<Message> {
        lock(&self.state.messages).clone()
    }

    /// Wait until at least `n` messages were accepted, at most `timeout`. The messages so far either way.
    pub fn wait_for_messages(&self, n: usize, timeout: Duration) -> Vec<Message> {
        let end = Instant::now() + timeout;
        loop {
            let got = self.messages();
            if got.len() >= n || Instant::now() >= end {
                return got;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// What was said, in order: `C: <line>` for a client line, `S: <line>` for a server reply, and `! <event>` for
    /// `tls`, `starttls`, `auth ok`, `auth failed`, `message`, `cut`. A password sent with `AUTH` is not logged.
    pub fn log(&self) -> Vec<String> {
        lock(&self.state.log).clone()
    }

    /// From now on, refuse `RCPT TO` this address (any case) with 550.
    pub fn reject_recipient(&self, address: &str) {
        lock(&self.state.switches).reject.push(address.to_ascii_lowercase());
    }

    /// From now on, cut the connection after the first line of `DATA` with no answer, or stop doing that.
    pub fn cut_during_data(&self, on: bool) {
        lock(&self.state.switches).cut_during_data = on;
    }
}

impl Drop for SmtpServer {
    fn drop(&mut self) {
        // The accept loop (a field) stops after this; the folder goes first.
        let _ = std::fs::remove_dir_all(&self.state.dir);
    }
}

enum Conn {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ServerConnection, TcpStream>>),
    Gone,
}

impl Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Tls(s) => s.read(buf),
            Conn::Gone => Ok(0),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Conn::Plain(s) => s.write_all(bytes).and_then(|_| s.flush()),
            Conn::Tls(s) => s.write_all(bytes).and_then(|_| s.flush()),
            Conn::Gone => Err(io::ErrorKind::BrokenPipe.into()),
        }
    }

    fn upgrade(&mut self, cert: &Cert) -> io::Result<()> {
        if let Conn::Plain(s) = std::mem::replace(self, Conn::Gone) {
            let conn = ServerConnection::new(cert.server_config()).map_err(io::Error::other)?;
            *self = Conn::Tls(Box::new(StreamOwned::new(conn, s)));
        }
        Ok(())
    }

    fn is_tls(&self) -> bool {
        matches!(self, Conn::Tls(_))
    }
}

struct Io<'a> {
    st: &'a State,
    conn: Conn,
    buf: Vec<u8>,
    stop: &'a AtomicBool,
}

impl Io<'_> {
    fn note(&self, line: String) {
        lock(&self.st.log).push(line);
    }

    /// One line without its line end; `None` when the peer is gone or the server stops.
    fn line(&mut self) -> Option<Vec<u8>> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=i).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Some(line);
            }
            if self.stop.load(Ordering::Relaxed) {
                return None;
            }
            let mut tmp = [0u8; 4096];
            match self.conn.read(&mut tmp) {
                Ok(0) => return None,
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e) if is_timeout(&e) => {}
                Err(_) => return None,
            }
        }
    }

    fn reply(&mut self, text: &str) -> io::Result<()> {
        for l in text.split("\r\n") {
            self.note(format!("S: {l}"));
        }
        self.conn.write_all(format!("{text}\r\n").as_bytes())
    }

    fn command(&mut self, secret: bool) -> Option<String> {
        let line = String::from_utf8_lossy(&self.line()?).into_owned();
        let mut words = line.split_whitespace();
        let logged = if secret {
            "C: <auth data>".to_owned()
        } else if words.next().is_some_and(|w| w.eq_ignore_ascii_case("AUTH")) {
            // An initial response carries the password (base64): keep the command and the mechanism only.
            format!("C: AUTH {}{}", words.next().unwrap_or(""), if words.next().is_some() { " <auth data>" } else { "" })
        } else {
            format!("C: {line}")
        };
        self.note(logged);
        Some(line)
    }
}

fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for b in s.trim().bytes().take_while(|&b| b != b'=') {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// `<a@b>` or `a@b` after `FROM:` / `TO:`, with the parameters dropped.
fn path_arg(rest: &str) -> String {
    let rest = rest.trim();
    let inner = match (rest.find('<'), rest.find('>')) {
        (Some(a), Some(b)) if a < b => &rest[a + 1..b],
        _ => rest.split_whitespace().next().unwrap_or(""),
    };
    inner.to_owned()
}

/// What follows the colon of `FROM:<a@b>`.
fn after_colon(rest: &str) -> &str {
    rest.find(':').map_or(rest, |i| &rest[i + 1..])
}

fn session(st: &State, stream: TcpStream, stop: &AtomicBool) -> io::Result<()> {
    let mut io = Io { st, conn: Conn::Plain(stream), buf: Vec::new(), stop };
    if st.mode == Mode::ImplicitTls {
        io.conn.upgrade(st.cert.as_ref().expect("a certificate"))?;
        io.note("! tls".into());
    }
    io.reply("220 ito-testserver ESMTP ready")?;
    let (mut authed, mut from, mut to): (bool, Option<String>, Vec<String>) = (st.credentials.is_none(), None, Vec::new());
    while let Some(cmd) = io.command(false) {
        let verb = cmd.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
        let rest = cmd.trim_start().get(verb.len()..).unwrap_or("").trim();
        match verb.as_str() {
            "EHLO" | "HELO" => {
                let mut lines = vec!["250-ito-testserver".to_owned(), "250-8BITMIME".to_owned(), "250-SIZE 52428800".to_owned()];
                if st.mode == Mode::StartTls && !io.conn.is_tls() {
                    lines.push("250-STARTTLS".into());
                }
                if st.credentials.is_some() && (st.mode != Mode::StartTls || io.conn.is_tls()) {
                    lines.push("250-AUTH PLAIN LOGIN".into());
                }
                lines.push("250 HELP".into());
                io.reply(&lines.join("\r\n"))?;
            }
            "STARTTLS" if st.mode == Mode::StartTls && !io.conn.is_tls() => {
                io.reply("220 2.0.0 Ready to start TLS")?;
                io.buf.clear();
                io.conn.upgrade(st.cert.as_ref().expect("a certificate"))?;
                io.note("! starttls".into());
                (authed, from, to) = (st.credentials.is_none(), None, Vec::new());
            }
            "AUTH" => {
                let Some((user, pass)) = st.credentials.as_ref() else {
                    io.reply("503 5.5.1 AUTH is not offered")?;
                    continue;
                };
                if st.mode == Mode::StartTls && !io.conn.is_tls() {
                    io.reply("530 5.7.0 Must issue a STARTTLS command first")?;
                    continue;
                }
                let mut parts = rest.split_whitespace();
                let given = match parts.next().map(str::to_ascii_uppercase).as_deref() {
                    Some("PLAIN") => {
                        let data = match parts.next() {
                            Some(d) => d.to_owned(),
                            None => {
                                io.reply("334 ")?;
                                let Some(l) = io.command(true) else { break };
                                l
                            }
                        };
                        b64_decode(&data).map(|b| {
                            let mut f = b.split(|&c| c == 0);
                            let (_authzid, u, p) = (f.next(), f.next().unwrap_or(&[]), f.next().unwrap_or(&[]));
                            (String::from_utf8_lossy(u).into_owned(), String::from_utf8_lossy(p).into_owned())
                        })
                    }
                    Some("LOGIN") => {
                        let u = match parts.next() {
                            Some(u) => u.to_owned(),
                            None => {
                                io.reply(&format!("334 {}", b64_encode(b"Username:")))?;
                                let Some(l) = io.command(true) else { break };
                                l
                            }
                        };
                        io.reply(&format!("334 {}", b64_encode(b"Password:")))?;
                        let Some(p) = io.command(true) else { break };
                        let dec = |s: &str| b64_decode(s).map(|b| String::from_utf8_lossy(&b).into_owned());
                        dec(&u).zip(dec(&p))
                    }
                    _ => {
                        io.reply("504 5.5.4 Unrecognized authentication type")?;
                        continue;
                    }
                };
                if given.as_ref().is_some_and(|(u, p)| u == user && p == pass) {
                    authed = true;
                    io.note("! auth ok".into());
                    io.reply("235 2.7.0 Authentication successful")?;
                } else {
                    io.note("! auth failed".into());
                    io.reply("535 5.7.8 Authentication credentials invalid")?;
                }
            }
            "MAIL" => {
                if !authed {
                    io.reply("530 5.7.0 Authentication required")?;
                    continue;
                }
                from = Some(path_arg(after_colon(rest)));
                to.clear();
                io.reply("250 2.1.0 Ok")?;
            }
            "RCPT" => {
                if from.is_none() {
                    io.reply("503 5.5.1 MAIL first")?;
                    continue;
                }
                let addr = path_arg(after_colon(rest));
                if lock(&st.switches).reject.contains(&addr.to_ascii_lowercase()) {
                    io.reply(&format!("550 5.1.1 <{addr}> Mailbox unavailable"))?;
                } else {
                    to.push(addr);
                    io.reply("250 2.1.5 Ok")?;
                }
            }
            "DATA" => {
                if from.is_none() || to.is_empty() {
                    io.reply("503 5.5.1 MAIL and RCPT first")?;
                    continue;
                }
                io.reply("354 End data with <CR><LF>.<CR><LF>")?;
                let mut raw = Vec::new();
                loop {
                    let Some(line) = io.line() else { return Ok(()) };
                    if lock(&st.switches).cut_during_data {
                        io.note("! cut".into());
                        return Ok(());
                    }
                    if line == b"." {
                        break;
                    }
                    raw.extend_from_slice(if line.starts_with(b"..") { &line[1..] } else { &line });
                    raw.extend_from_slice(b"\r\n");
                }
                let mut messages = lock(&st.messages);
                let path = st.dir.join(format!("{:04}.eml", messages.len() + 1));
                std::fs::write(&path, &raw)?;
                messages.push(Message { from: from.take().unwrap_or_default(), to: std::mem::take(&mut to), raw, path });
                drop(messages);
                io.note("! message".into());
                io.reply("250 2.0.0 Ok: queued")?;
            }
            "RSET" => {
                (from, to) = (None, Vec::new());
                io.reply("250 2.0.0 Ok")?;
            }
            "NOOP" => io.reply("250 2.0.0 Ok")?,
            "QUIT" => {
                io.reply("221 2.0.0 Bye")?;
                break;
            }
            _ => io.reply("502 5.5.2 Command not recognized")?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for s in ["", "a", "ab", "abc", "\0user\0pass", "Username:"] {
            assert_eq!(b64_decode(&b64_encode(s.as_bytes())).as_deref(), Some(s.as_bytes()));
        }
        assert_eq!(b64_encode(b"Username:"), "VXNlcm5hbWU6");
    }

    #[test]
    fn addresses_come_out_of_the_angle_brackets() {
        assert_eq!(path_arg(after_colon("FROM:<a@b.c> SIZE=10")), "a@b.c");
        assert_eq!(path_arg("a@b.c"), "a@b.c");
        assert_eq!(path_arg("<>"), "");
    }
}
