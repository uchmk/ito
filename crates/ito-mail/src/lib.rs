//! Look at a mailbox over IMAP for new mail, and never change it.
//!
//! [`Inbox::poll`] lists the messages that came since the last call (their sender, subject and date), the
//! way `ito-eventlog::Subscription::poll` lists new records. The folder is opened with `EXAMINE` and the
//! headers are fetched with `BODY.PEEK`, so nothing is marked read, moved or deleted. The connection is
//! TLS from the start (port 993), pure-Rust rustls with the bundled web roots, and every connect and
//! read has a timeout, so a stuck server cannot hang the caller's thread. A dropped connection is made
//! again on the next call.
//!
//! The app keeps [`Inbox::position`] with its state and passes it back as [`From::After`] after a
//! restart, so mail is neither delivered twice nor lost while the app was down. If the server renumbers
//! the folder (`UIDVALIDITY` changed) the old position means nothing: the inbox starts from the end again.
//!
//! Subjects and names are decoded from RFC 2047 (`=?UTF-8?B?…?=`, `=?ISO-2022-JP?B?…?=`, `Q` too).
//! Folder names are sent as they are (ASCII; modified UTF-7 is not encoded).

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, FixedOffset};

/// How long a connect, and each read or write, may take.
const TIMEOUT: Duration = Duration::from_secs(20);
/// A response line longer than this is an error, so a broken server cannot fill the memory.
const LONGEST_LINE: usize = 1 << 20;
/// A literal (a fetched header block) larger than this is an error.
const LONGEST_LITERAL: usize = 1 << 20;
/// At most this many UIDs are asked for per [`Inbox::poll`]; the rest comes on the next call.
const PER_POLL: u32 = 200;

/// Where the mailbox is and whose it is. The password is passed to [`Inbox::open`], not kept here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub host: String,
    /// 993 for IMAP over TLS.
    pub port: u16,
    pub user: String,
    /// `"INBOX"` for the main one.
    pub folder: String,
}

impl Account {
    /// `host` on port 993, the user's `INBOX`.
    pub fn new(host: &str, user: &str) -> Self {
        Self { host: host.to_owned(), port: 993, user: user.to_owned(), folder: "INBOX".to_owned() }
    }
}

/// Where in a folder the last look ended. Keep it and pass it back as [`From::After`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub uid_validity: u32,
    pub uid: u32,
}

/// Where to begin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    /// From the newest message: only mail that comes after [`Inbox::open`] is returned.
    End,
    /// After this position (from [`Inbox::position`]).
    After(Position),
}

/// One message, as far as the headers say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mail {
    pub uid: u32,
    /// The sender's address (`a@example.com`).
    pub from: String,
    /// The sender's display name, decoded.
    pub name: Option<String>,
    /// Decoded; empty when there is none.
    pub subject: String,
    pub date: Option<DateTime<FixedOffset>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The host is not reachable, or the connection broke.
    Connect(String),
    /// The TLS handshake failed (bad certificate, not a TLS port).
    Tls(String),
    /// Wrong user or password.
    Login,
    /// The folder does not exist.
    NoFolder,
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connect(s) => write!(f, "cannot reach the mail server: {s}"),
            Error::Tls(s) => write!(f, "TLS failed: {s}"),
            Error::Login => f.write_str("wrong user or password"),
            Error::NoFolder => f.write_str("no such folder"),
            Error::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Error {}

fn io_err(e: io::Error) -> Error {
    Error::Connect(e.to_string())
}

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

type Connector = Box<dyn FnMut() -> Result<Box<dyn Stream>, Error> + Send>;

/// A mailbox being looked at.
pub struct Inbox {
    connect: Connector,
    user: String,
    password: String,
    folder: String,
    conn: Option<Conn>,
    /// Set until the first connection decides where to start.
    start: Option<From>,
    pos: Position,
}

impl Inbox {
    /// Connect, log in and open the folder read-only. Fails now if the server, the login or the folder is wrong.
    pub fn open(account: &Account, password: &str, from: From) -> Result<Inbox, Error> {
        let (host, port) = (account.host.clone(), account.port);
        Self::with(Box::new(move || tls(&host, port)), account, password, from)
    }

    fn with(connect: Connector, account: &Account, password: &str, from: From) -> Result<Inbox, Error> {
        let mut inbox = Inbox {
            connect,
            user: account.user.clone(),
            password: password.to_owned(),
            folder: account.folder.clone(),
            conn: None,
            start: Some(from),
            pos: Position::default(),
        };
        inbox.attach()?;
        Ok(inbox)
    }

    /// Messages that came since the last call (UID above the last one seen), oldest first, at most
    /// 200 UIDs' worth at a time. Reconnects when the connection dropped.
    pub fn poll(&mut self) -> Result<Vec<Mail>, Error> {
        let result = self.look();
        if matches!(result, Err(Error::Connect(_) | Error::Tls(_))) {
            self.conn = None;
        }
        result
    }

    /// Where the last look ended; keep it and pass it to [`Inbox::open`] as [`From::After`].
    pub fn position(&self) -> Position {
        self.pos
    }

    fn look(&mut self) -> Result<Vec<Mail>, Error> {
        self.attach()?;
        let last = self.pos.uid;
        let to = last.saturating_add(PER_POLL);
        let conn = self.conn.as_mut().expect("attached");
        // A NOOP lets the server tell about mail that came since EXAMINE.
        let news = conn.run("NOOP")?;
        for r in &news {
            if let Some(n) = exists_of(&r.text) {
                conn.exists = n;
            }
        }
        let reply = conn.run(&format!("UID FETCH {}:{to} (UID BODY.PEEK[HEADER.FIELDS (FROM SUBJECT DATE)])", last + 1))?;
        // A range asked for by number gives only what exists in it, but some servers add the last message.
        let mut mails: Vec<Mail> = reply.iter().filter_map(mail_of).filter(|m| m.uid > last).collect();
        mails.sort_by_key(|m| m.uid);
        mails.dedup_by_key(|m| m.uid);
        let newest = mails.last().map_or(last, |m| m.uid);
        // A window that came up short has been looked through to its end, so the gaps left by deleted
        // mail are hopped over; a full one goes on from its last message.
        self.pos.uid = if mails.len() < PER_POLL as usize { newest.max(to.min(conn.uid_next.saturating_sub(1))) } else { newest };
        Ok(mails)
    }

    /// Make sure there is a logged-in connection with the folder open, and decide the start on the first one.
    fn attach(&mut self) -> Result<(), Error> {
        if self.conn.is_some() {
            return Ok(());
        }
        let stream = (self.connect)()?;
        let mut conn = Conn::new(stream)?;
        conn.login(&self.user, &self.password)?;
        conn.examine(&self.folder)?;
        let from = self.start.take().unwrap_or(From::After(self.pos));
        let end = Position { uid_validity: conn.uid_validity, uid: conn.uid_next.saturating_sub(1) };
        self.pos = match from {
            From::After(p) if p.uid_validity == conn.uid_validity && p.uid <= end.uid => p,
            // New numbering, or a position beyond what the folder has: it means nothing here.
            _ => end,
        };
        self.conn = Some(conn);
        Ok(())
    }
}

fn tls(host: &str, port: u16) -> Result<Box<dyn Stream>, Error> {
    let addrs = (host, port).to_socket_addrs().map_err(io_err)?;
    let mut last = None;
    let mut tcp = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, TIMEOUT) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let mut tcp = tcp.ok_or_else(|| last.map_or_else(|| Error::Connect(format!("{host} has no address")), io_err))?;
    tcp.set_read_timeout(Some(TIMEOUT)).map_err(io_err)?;
    tcp.set_write_timeout(Some(TIMEOUT)).map_err(io_err)?;
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Tls(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|e| Error::Tls(e.to_string()))?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| Error::Tls(e.to_string()))?;
    // Finish the handshake here so a certificate error is a Tls error, not a Connect one on the greeting.
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp).map_err(|e| Error::Tls(e.to_string()))?;
    }
    Ok(Box::new(rustls::StreamOwned::new(conn, tcp)))
}

/// One untagged response: its text with each literal cut out, and the literals.
#[derive(Debug, Default)]
struct Reply {
    text: String,
    literals: Vec<Vec<u8>>,
}

struct Conn {
    r: BufReader<Box<dyn Stream>>,
    n: u32,
    uid_validity: u32,
    uid_next: u32,
    exists: u32,
}

impl Conn {
    fn new(stream: Box<dyn Stream>) -> Result<Conn, Error> {
        let mut c = Conn { r: BufReader::new(stream), n: 0, uid_validity: 0, uid_next: 0, exists: 0 };
        let greeting = c.line()?;
        let g = greeting.to_ascii_uppercase();
        if !(g.starts_with("* OK") || g.starts_with("* PREAUTH")) {
            return Err(Error::Other(format!("the server said: {}", greeting.trim())));
        }
        Ok(c)
    }

    fn login(&mut self, user: &str, password: &str) -> Result<(), Error> {
        for s in [user, password] {
            if s.contains(['\r', '\n', '\0']) {
                return Err(Error::Login);
            }
        }
        match self.run(&format!("LOGIN {} {}", quoted(user), quoted(password))) {
            Err(Error::Other(_)) => Err(Error::Login),
            other => other.map(|_| ()),
        }
    }

    fn examine(&mut self, folder: &str) -> Result<(), Error> {
        if folder.contains(['\r', '\n', '\0']) {
            return Err(Error::NoFolder);
        }
        let replies = match self.run(&format!("EXAMINE {}", quoted(folder))) {
            Err(Error::Other(_)) => return Err(Error::NoFolder),
            other => other?,
        };
        for r in &replies {
            if let Some(n) = code(&r.text, "UIDVALIDITY") {
                self.uid_validity = n;
            }
            if let Some(n) = code(&r.text, "UIDNEXT") {
                self.uid_next = n;
            }
            if let Some(n) = exists_of(&r.text) {
                self.exists = n;
            }
        }
        if self.uid_validity == 0 {
            return Err(Error::Other("the server did not say UIDVALIDITY".to_owned()));
        }
        if self.uid_next == 0 {
            // Rare: ask for the newest message's UID instead.
            let replies = if self.exists == 0 { Vec::new() } else { self.run("UID FETCH * (UID)")? };
            let newest = replies.iter().filter_map(|r| uid_of(&r.text)).max().unwrap_or(0);
            self.uid_next = newest + 1;
        }
        Ok(())
    }

    /// Send a command and read up to its tagged answer. `NO` and `BAD` are `Error::Other` with the server's words.
    fn run(&mut self, command: &str) -> Result<Vec<Reply>, Error> {
        self.n += 1;
        let tag = format!("a{}", self.n);
        let w = self.r.get_mut();
        w.write_all(format!("{tag} {command}\r\n").as_bytes()).map_err(io_err)?;
        w.flush().map_err(io_err)?;
        let mut replies = Vec::new();
        loop {
            let mut reply = Reply::default();
            let mut line = self.line()?;
            // `{n}` at the end of a line announces n bytes, and the line goes on after them.
            loop {
                match literal_len(&line) {
                    Some(n) if n <= LONGEST_LITERAL => {
                        reply.text.push_str(line[..line.rfind('{').unwrap_or(0)].trim_end());
                        reply.text.push(' ');
                        let mut buf = vec![0; n];
                        self.r.read_exact(&mut buf).map_err(io_err)?;
                        reply.literals.push(buf);
                        line = self.line()?;
                    }
                    Some(_) => return Err(Error::Other("the server sent too much".to_owned())),
                    None => {
                        reply.text.push_str(line.trim_end());
                        break;
                    }
                }
            }
            if let Some(rest) = reply.text.strip_prefix(tag.as_str()).and_then(|r| r.strip_prefix(' ')) {
                let upper = rest.to_ascii_uppercase();
                return if upper.starts_with("OK") { Ok(replies) } else { Err(Error::Other(rest.to_owned())) };
            }
            replies.push(reply);
        }
    }

    fn line(&mut self) -> Result<String, Error> {
        let mut buf = Vec::new();
        let n = (&mut self.r).take(LONGEST_LINE as u64 + 1).read_until(b'\n', &mut buf).map_err(io_err)?;
        if n == 0 {
            return Err(Error::Connect("the server closed the connection".to_owned()));
        }
        if buf.len() > LONGEST_LINE {
            return Err(Error::Other("the server sent a line that is too long".to_owned()));
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// `"text"` with `\` and `"` escaped.
fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// n of a `{n}` at the end of the line.
fn literal_len(line: &str) -> Option<usize> {
    let t = line.trim_end().strip_suffix('}')?;
    t[t.rfind('{')? + 1..].trim_end_matches('+').parse().ok()
}

/// n of `[NAME n]` in a response.
fn code(text: &str, name: &str) -> Option<u32> {
    let at = text.find(&format!("[{name} "))? + name.len() + 2;
    text[at..].split(']').next()?.trim().parse().ok()
}

/// n of `* n EXISTS`.
fn exists_of(text: &str) -> Option<u32> {
    let mut w = text.split_whitespace();
    (w.next()? == "*").then_some(())?;
    let n = w.next()?.parse().ok()?;
    w.next()?.eq_ignore_ascii_case("EXISTS").then_some(n)
}

/// The number after the word `UID` in a FETCH response.
fn uid_of(text: &str) -> Option<u32> {
    let mut words = text.split(|c: char| c.is_whitespace() || c == '(' || c == ')').filter(|w| !w.is_empty());
    while let Some(w) = words.next() {
        if w.eq_ignore_ascii_case("UID") {
            return words.next()?.parse().ok();
        }
    }
    None
}

fn mail_of(r: &Reply) -> Option<Mail> {
    let uid = uid_of(&r.text)?;
    let headers = headers(r.literals.first()?);
    let get = |name: &str| headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    let (name, from) = get("From").map(address).unwrap_or_default();
    Some(Mail {
        uid,
        from,
        name,
        subject: get("Subject").map(|s| decode_words(s.trim())).unwrap_or_default(),
        date: get("Date").and_then(|d| DateTime::parse_from_rfc2822(d.trim()).ok()),
    })
}

/// Header fields with their folded lines joined.
fn headers(raw: &[u8]) -> Vec<(String, String)> {
    let text = match std::str::from_utf8(raw) {
        Ok(s) => s.to_owned(),
        // 8-bit headers in an old encoding: keep the bytes visible rather than failing.
        Err(_) => raw.iter().map(|&b| b as char).collect(),
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = out.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
        } else if let Some((n, v)) = line.split_once(':') {
            out.push((n.trim().to_owned(), v.trim().to_owned()));
        }
    }
    out
}

/// (display name, address) of the first address in a From header.
fn address(v: &str) -> (Option<String>, String) {
    let v = v.trim();
    let (name, addr) = match (v.rfind('<'), v.rfind('>')) {
        (Some(a), Some(b)) if a < b => (v[..a].trim(), v[a + 1..b].trim()),
        _ => ("", v),
    };
    let name = decode_words(name.trim().trim_matches('"').trim());
    ((!name.is_empty()).then_some(name), addr.to_owned())
}

/// RFC 2047: `=?charset?B|Q?text?=` words; whitespace between two encoded words is dropped.
fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut after_word = false;
    while !rest.is_empty() {
        if let Some((decoded, len)) = encoded_word(rest) {
            out.push_str(&decoded);
            rest = &rest[len..];
            after_word = true;
            continue;
        }
        let space = rest.len() - rest.trim_start().len();
        if after_word && space > 0 && encoded_word(&rest[space..]).is_some() {
            rest = &rest[space..];
            continue;
        }
        let c = rest.chars().next().expect("not empty");
        out.push(c);
        rest = &rest[c.len_utf8()..];
        after_word = false;
    }
    out
}

/// The decoded text and the length used, of an encoded word at the start of `s`.
fn encoded_word(s: &str) -> Option<(String, usize)> {
    let body = s.strip_prefix("=?")?;
    let end = body.find("?=")?;
    let mut parts = body[..end].splitn(3, '?');
    let (charset, enc, text) = (parts.next()?, parts.next()?, parts.next()?);
    // RFC 2231 may add `*language` to the charset.
    let charset = charset.split('*').next()?;
    let bytes = match enc {
        "B" | "b" => base64(text)?,
        "Q" | "q" => q(text)?,
        _ => return None,
    };
    let encoding = encoding_rs::Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    Some((encoding.decode_without_bom_handling(&bytes).0.into_owned(), 2 + end + 2))
}

fn q(text: &str) -> Option<Vec<u8>> {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'_' => out.push(b' '),
            b'=' => {
                let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    Some(out)
}

fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = acc << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[test]
    fn subjects_are_decoded_from_every_form_of_encoded_word() {
        assert_eq!(decode_words("=?UTF-8?B?44GT44KT44Gr44Gh44Gv?="), "こんにちは");
        assert_eq!(decode_words("=?UTF-8?Q?caf=C3=A9_au_lait?="), "café au lait");
        // Japanese mail: ISO-2022-JP, and a long subject split in two words with a space between.
        assert_eq!(decode_words("=?ISO-2022-JP?B?GyRCJDMkcyRLJEEkTxsoQg==?="), "こんにちは");
        assert_eq!(decode_words("=?UTF-8?B?44GT44KT?= =?UTF-8?B?44Gr44Gh44Gv?="), "こんにちは");
        // Text around a word keeps its spaces; plain text and a broken word stay as they are.
        assert_eq!(decode_words("Re: =?UTF-8?Q?a?= b"), "Re: a b");
        assert_eq!(decode_words("plain =?nope"), "plain =?nope");
        assert_eq!(decode_words("=?UTF-8?X?a?="), "=?UTF-8?X?a?=");
    }

    #[test]
    fn a_from_header_gives_the_address_and_the_name() {
        assert_eq!(address("alice@example.com"), (None, "alice@example.com".to_owned()));
        assert_eq!(address("Alice <alice@example.com>"), (Some("Alice".to_owned()), "alice@example.com".to_owned()));
        assert_eq!(address("\"Smith, Bob\" <b@example.com>"), (Some("Smith, Bob".to_owned()), "b@example.com".to_owned()));
        assert_eq!(address("=?UTF-8?B?44GC?= <a@example.jp>"), (Some("あ".to_owned()), "a@example.jp".to_owned()));
        assert_eq!(address("<x@example.com>"), (None, "x@example.com".to_owned()));
    }

    #[test]
    fn headers_unfold_and_the_date_parses() {
        let r = Reply {
            text: "* 3 FETCH (UID 42 BODY[HEADER.FIELDS (FROM SUBJECT DATE)] )".to_owned(),
            literals: vec![b"From: A <a@x.org>\r\nSubject: one\r\n two\r\nDate: Thu, 29 Oct 2026 09:15:00 +0900 (JST)\r\n\r\n".to_vec()],
        };
        let m = mail_of(&r).unwrap();
        assert_eq!((m.uid, m.from.as_str(), m.subject.as_str()), (42, "a@x.org", "one two"));
        assert_eq!(m.date.unwrap().to_rfc3339(), "2026-10-29T09:15:00+09:00");
    }

    #[test]
    fn the_small_parsers() {
        assert_eq!(literal_len("* 1 FETCH (BODY[] {123}\r\n"), Some(123));
        assert_eq!(literal_len("* 1 FETCH (UID 3)\r\n"), None);
        assert_eq!(code("* OK [UIDVALIDITY 3857529045] UIDs valid", "UIDVALIDITY"), Some(3857529045));
        assert_eq!(exists_of("* 5 EXISTS"), Some(5));
        assert_eq!(exists_of("* 5 FETCH (UID 1)"), None);
        assert_eq!(uid_of("* 2 FETCH (UID 7 BODY[HEADER] )"), Some(7));
        assert_eq!(uid_of("* 2 FETCH (BODY[HEADER.FIELDS (FROM)] UID 9)"), Some(9));
        assert_eq!(quoted("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    type Answer = Box<dyn Fn(&str) -> String + Send>;

    /// A scripted server: one answer per command it is sent, and a record of the commands.
    struct Script {
        said: Arc<Mutex<Vec<String>>>,
        out: VecDeque<u8>,
        answers: VecDeque<Answer>,
    }

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.out.len());
            for b in buf.iter_mut().take(n) {
                *b = self.out.pop_front().unwrap();
            }
            Ok(n)
        }
    }

    impl Write for Script {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let line = String::from_utf8_lossy(buf).trim_end().to_owned();
            self.said.lock().unwrap().push(line.clone());
            let answer = self.answers.pop_front().expect("an unexpected command")(&line);
            self.out.extend(answer.into_bytes());
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn tagged(line: &str, ok: &str) -> String {
        format!("{} {ok}\r\n", line.split(' ').next().unwrap())
    }

    fn server(connections: Vec<Vec<Answer>>) -> (Connector, Arc<Mutex<Vec<String>>>) {
        let said = Arc::new(Mutex::new(Vec::new()));
        let mut queue = VecDeque::from(connections);
        let said2 = said.clone();
        let connect: Connector = Box::new(move || {
            let answers = queue.pop_front().ok_or_else(|| Error::Connect("refused".to_owned()))?;
            Ok(Box::new(Script { said: said2.clone(), out: b"* OK ready\r\n".iter().copied().collect(), answers: answers.into() }))
        });
        (connect, said)
    }

    fn login_ok() -> Answer {
        Box::new(|l| tagged(l, "OK logged in"))
    }

    fn examine(validity: u32, next: u32) -> Answer {
        Box::new(move |l| {
            format!("* 5 EXISTS\r\n* OK [UIDVALIDITY {validity}] x\r\n* OK [UIDNEXT {next}] x\r\n{}", tagged(l, "OK [READ-ONLY] done"))
        })
    }

    fn noop() -> Answer {
        Box::new(|l| tagged(l, "OK"))
    }

    fn fetch(items: &'static [(u32, &'static str)]) -> Answer {
        Box::new(move |l| {
            let mut out = String::new();
            for (uid, headers) in items {
                out += &format!("* {uid} FETCH (UID {uid} BODY[HEADER.FIELDS (FROM SUBJECT DATE)] {{{}}}\r\n{headers})\r\n", headers.len());
            }
            out + &tagged(l, "OK done")
        })
    }

    fn account() -> Account {
        Account::new("imap.example.com", "me@example.com")
    }

    #[test]
    fn it_looks_read_only_and_returns_only_new_mail_oldest_first() {
        let (connect, said) = server(vec![vec![
            login_ok(),
            examine(7, 11),
            noop(),
            fetch(&[(12, "From: B <b@x.org>\r\nSubject: =?UTF-8?B?44GC?=\r\n\r\n"), (11, "From: a@x.org\r\nSubject: first\r\n\r\n")]),
            noop(),
            // Some servers add the last message to a range that is past it; it is not new.
            fetch(&[(12, "From: B <b@x.org>\r\n\r\n")]),
        ]]);
        let mut inbox = Inbox::with(connect, &account(), "pw\"d", From::End).unwrap();
        assert_eq!(inbox.position(), Position { uid_validity: 7, uid: 10 });
        let mails = inbox.poll().unwrap();
        assert_eq!(mails.iter().map(|m| (m.uid, m.subject.as_str())).collect::<Vec<_>>(), [(11, "first"), (12, "あ")]);
        assert_eq!(inbox.position(), Position { uid_validity: 7, uid: 12 });
        assert!(inbox.poll().unwrap().is_empty());
        let said = said.lock().unwrap().join("\n");
        assert!(said.contains("LOGIN \"me@example.com\" \"pw\\\"d\""), "{said}");
        assert!(said.contains("EXAMINE \"INBOX\""), "{said}");
        assert!(said.contains("BODY.PEEK[HEADER.FIELDS"), "{said}");
        // Nothing that could change the mailbox.
        for word in ["STORE", "COPY", "MOVE", "EXPUNGE", "DELETE", "SELECT", " BODY["] {
            assert!(!said.contains(word), "{word} in {said}");
        }
    }

    #[test]
    fn a_saved_position_resumes_and_a_new_uidvalidity_starts_over() {
        let (connect, _) = server(vec![vec![login_ok(), examine(7, 20), noop(), fetch(&[(15, "Subject: s\r\n\r\n")])]]);
        let mut inbox = Inbox::with(connect, &account(), "p", From::After(Position { uid_validity: 7, uid: 14 })).unwrap();
        assert_eq!(inbox.poll().unwrap().iter().map(|m| m.uid).collect::<Vec<_>>(), [15]);

        let (connect, _) = server(vec![vec![login_ok(), examine(8, 20)]]);
        let inbox = Inbox::with(connect, &account(), "p", From::After(Position { uid_validity: 7, uid: 14 })).unwrap();
        assert_eq!(inbox.position(), Position { uid_validity: 8, uid: 19 });
    }

    #[test]
    fn a_dropped_connection_is_made_again_without_losing_the_place() {
        let (connect, _) = server(vec![
            vec![login_ok(), examine(7, 11), Box::new(|_| String::new())],
            vec![login_ok(), examine(7, 12), noop(), fetch(&[(11, "Subject: late\r\n\r\n")])],
        ]);
        let mut inbox = Inbox::with(connect, &account(), "p", From::End).unwrap();
        assert!(matches!(inbox.poll(), Err(Error::Connect(_))));
        let mails = inbox.poll().unwrap();
        assert_eq!(mails.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(), ["late"]);
    }

    #[test]
    fn a_wrong_password_and_a_missing_folder_are_told_apart() {
        let (connect, _) = server(vec![vec![Box::new(|l| tagged(l, "NO [AUTHENTICATIONFAILED] bad"))]]);
        assert_eq!(Inbox::with(connect, &account(), "p", From::End).err(), Some(Error::Login));
        let (connect, _) = server(vec![vec![login_ok(), Box::new(|l| tagged(l, "NO no such mailbox"))]]);
        assert_eq!(Inbox::with(connect, &account(), "p", From::End).err(), Some(Error::NoFolder));
    }
}
