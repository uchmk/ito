//! A fake IMAP server. See [`ImapServer`].

use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::stream::{b64_decode, Conn, Reader};
use crate::tls::{self, Cert, Fingerprint};
use crate::Acceptor;

/// How the connection is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// No TLS at all.
    Plain,
    /// TLS from the first byte (like port 993), with a self-signed certificate.
    ImplicitTls,
}

struct Mailbox {
    uid_validity: u32,
    next_uid: u32,
    messages: Vec<(u32, Vec<u8>)>,
}

struct State {
    mode: Mode,
    credentials: Option<(String, String)>,
    cert: Option<Cert>,
    mailbox: Mutex<Mailbox>,
    log: Mutex<Vec<String>>,
    /// Bumped by `drop_connections`; a session that started under an older number ends.
    generation: AtomicU32,
    connections: AtomicU32,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A fake IMAP4rev1 server on `127.0.0.1` with one folder, `INBOX`, that the test fills with
/// [`add_message`](Self::add_message). It speaks what a read-only watcher like `ito-mail` needs: `CAPABILITY`,
/// `LOGIN` and `AUTHENTICATE PLAIN`, `EXAMINE` / `SELECT` (with `UIDVALIDITY` and `UIDNEXT`), `NOOP` (it tells the
/// new `EXISTS`), `FETCH` / `UID FETCH` (`UID`, `FLAGS`, `RFC822.SIZE`, `BODY[]`, `BODY[HEADER]` and
/// `BODY[HEADER.FIELDS (...)]`, `PEEK` or not, as literals), `LIST`, `CLOSE`, `LOGOUT`. Nothing is ever changed by
/// a client (no `STORE`, no `EXPUNGE`: they get `BAD`). It stops when it is dropped.
pub struct ImapServer {
    state: Arc<State>,
    acceptor: Acceptor,
}

impl ImapServer {
    /// Start a server in `mode` that takes any login.
    pub fn start(mode: Mode) -> io::Result<ImapServer> {
        Self::start_with(mode, None)
    }

    /// Start a server that wants this user and password (`NO [AUTHENTICATIONFAILED]` for any other).
    pub fn start_with_login(mode: Mode, user: &str, password: &str) -> io::Result<ImapServer> {
        Self::start_with(mode, Some((user.to_owned(), password.to_owned())))
    }

    fn start_with(mode: Mode, credentials: Option<(String, String)>) -> io::Result<ImapServer> {
        let cert = (mode == Mode::ImplicitTls).then(Cert::generate);
        let state = Arc::new(State {
            mode,
            credentials,
            cert,
            mailbox: Mutex::new(Mailbox { uid_validity: 1, next_uid: 1, messages: Vec::new() }),
            log: Mutex::new(Vec::new()),
            generation: AtomicU32::new(0),
            connections: AtomicU32::new(0),
        });
        let st = state.clone();
        let acceptor = Acceptor::start(move |stream, stop| {
            st.connections.fetch_add(1, Ordering::Relaxed);
            let _ = session(&st, stream, &stop);
        })?;
        Ok(ImapServer { state, acceptor })
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

    /// The certificate (DER).
    pub fn certificate_der(&self) -> Option<Vec<u8>> {
        self.state.cert.as_ref().map(|c| c.der().to_vec())
    }

    /// Put a message (RFC 5322 bytes) at the end of the folder; its UID. UIDs count up from 1 and are never reused.
    pub fn add_message(&self, raw: impl Into<Vec<u8>>) -> u32 {
        let mut m = lock(&self.state.mailbox);
        let uid = m.next_uid;
        m.next_uid += 1;
        m.messages.push((uid, raw.into()));
        uid
    }

    /// [`add_message`](Self::add_message) with a plain header and a one-line body.
    pub fn add_mail(&self, from: &str, subject: &str) -> u32 {
        self.add_message(format!(
            "From: {from}\r\nTo: you@example.com\r\nSubject: {subject}\r\nDate: Tue, 03 Nov 2026 10:00:00 +0900\r\n\
Message-ID: <{}@ito-testserver>\r\n\r\nbody\r\n",
            crate::unique()
        ))
    }

    /// Remove a message, leaving a gap in the UIDs (as a delete and expunge by another client would). False if
    /// there is none with this UID.
    pub fn remove_message(&self, uid: u32) -> bool {
        let mut m = lock(&self.state.mailbox);
        let before = m.messages.len();
        m.messages.retain(|(u, _)| *u != uid);
        m.messages.len() != before
    }

    /// The messages as `(uid, raw)`, oldest first.
    pub fn messages(&self) -> Vec<(u32, Vec<u8>)> {
        lock(&self.state.mailbox).messages.clone()
    }

    /// The folder's `UIDVALIDITY` (1 at first).
    pub fn uid_validity(&self) -> u32 {
        lock(&self.state.mailbox).uid_validity
    }

    /// Change the `UIDVALIDITY` that the next `EXAMINE` tells; the messages and UIDs stay. Use
    /// [`renumber`](Self::renumber) for what a server really does when it renumbers.
    pub fn set_uid_validity(&self, value: u32) {
        lock(&self.state.mailbox).uid_validity = value;
    }

    /// Renumber the folder as a restored or recreated mailbox would: a new `UIDVALIDITY`, the messages numbered
    /// again from 1.
    pub fn renumber(&self, new_validity: u32) {
        let mut m = lock(&self.state.mailbox);
        m.uid_validity = new_validity;
        for (i, msg) in m.messages.iter_mut().enumerate() {
            msg.0 = i as u32 + 1;
        }
        m.next_uid = m.messages.len() as u32 + 1;
    }

    /// Close every connection now (the server stays up and takes new ones).
    pub fn drop_connections(&self) {
        self.state.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// How many connections were made so far.
    pub fn connections(&self) -> u32 {
        self.state.connections.load(Ordering::Relaxed)
    }

    /// What was said, in order: `C: <line>` for a client line (a password is `<password>`), `S: <line>` for a server
    /// line (a literal as `<n bytes>`), and `! tls`, `! login ok`, `! login failed`, `! dropped`.
    pub fn log(&self) -> Vec<String> {
        lock(&self.state.log).clone()
    }

    /// The client commands only (`LOGIN u <password>`, `EXAMINE "INBOX"`, `UID FETCH 3:203 (...)`), without their tags.
    pub fn commands(&self) -> Vec<String> {
        self.log().iter().filter_map(|l| l.strip_prefix("C: ")).map(|l| l.split_once(' ').map_or(l, |x| x.1).to_owned()).collect()
    }

    /// Wait until a client line contains `needle`, at most `timeout`.
    pub fn wait_for_command(&self, needle: &str, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            if self.commands().iter().any(|c| c.contains(needle)) {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// One session's I/O.
struct Io<'a> {
    st: &'a State,
    r: Reader<'a>,
    generation: u32,
}

impl Io<'_> {
    fn note(&self, line: String) {
        lock(&self.st.log).push(line);
    }

    fn line(&mut self) -> Option<String> {
        let (st, g) = (self.st, self.generation);
        let l = self.r.line(&|| st.generation.load(Ordering::Relaxed) != g);
        if l.is_none() && st.generation.load(Ordering::Relaxed) != g {
            self.note("! dropped".into());
        }
        l
    }

    fn say(&mut self, text: &str) -> io::Result<()> {
        self.note(format!("S: {text}"));
        self.r.conn.write_all(format!("{text}\r\n").as_bytes())
    }

    /// Raw bytes with a note of what they are.
    fn send(&mut self, bytes: &[u8], note: String) -> io::Result<()> {
        self.note(note);
        self.r.conn.write_all(bytes)
    }
}

fn session(st: &State, stream: TcpStream, stop: &AtomicBool) -> io::Result<()> {
    let generation = st.generation.load(Ordering::Relaxed);
    let conn = Conn::new(stream, st.cert.as_ref())?;
    let mut io = Io { st, r: Reader::new(conn, stop), generation };
    if st.cert.is_some() {
        io.note("! tls".into());
    }
    io.say("* OK [CAPABILITY IMAP4rev1 AUTH=PLAIN] ito-testserver ready")?;
    let mut authed = false;
    let mut selected = false;
    while let Some(line) = io.line() {
        let (tag, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let (verb, args) = rest.trim_start().split_once(' ').unwrap_or((rest.trim(), ""));
        let verb = verb.to_ascii_uppercase();
        let tag = tag.to_owned();
        let args = args.trim().to_owned();
        let logged = match verb.as_str() {
            "LOGIN" => format!("C: {tag} LOGIN {} <password>", words(&args).first().cloned().unwrap_or_default()),
            "AUTHENTICATE" => format!("C: {tag} AUTHENTICATE {}", args.split_whitespace().next().unwrap_or("")),
            _ => format!("C: {line}"),
        };
        io.note(logged);
        match verb.as_str() {
            "CAPABILITY" => {
                io.say("* CAPABILITY IMAP4rev1 AUTH=PLAIN")?;
                io.say(&format!("{tag} OK CAPABILITY completed"))?;
            }
            "NOOP" => {
                if selected {
                    let n = lock(&st.mailbox).messages.len();
                    io.say(&format!("* {n} EXISTS"))?;
                }
                io.say(&format!("{tag} OK NOOP completed"))?;
            }
            "LOGIN" => {
                let w = words(&args);
                let good = match (&st.credentials, w.as_slice()) {
                    (None, [_, _]) => true,
                    (Some((u, p)), [wu, wp]) => wu == u && wp == p,
                    _ => false,
                };
                if good {
                    authed = true;
                    io.note("! login ok".into());
                    io.say(&format!("{tag} OK LOGIN completed"))?;
                } else {
                    io.note("! login failed".into());
                    io.say(&format!("{tag} NO [AUTHENTICATIONFAILED] invalid credentials"))?;
                }
            }
            "AUTHENTICATE" => {
                let mut it = args.split_whitespace();
                if !it.next().is_some_and(|m| m.eq_ignore_ascii_case("PLAIN")) {
                    io.say(&format!("{tag} NO unsupported mechanism"))?;
                    continue;
                }
                let initial = match it.next() {
                    Some(x) => x.to_owned(),
                    None => {
                        io.say("+ ")?;
                        let Some(l) = io.line() else { break };
                        io.note("C: <auth data>".into());
                        l
                    }
                };
                // authzid NUL user NUL password
                let parts =
                    b64_decode(&initial).map(|b| b.split(|&c| c == 0).map(|p| String::from_utf8_lossy(p).into_owned()).collect::<Vec<_>>());
                let good = match (&st.credentials, parts.as_deref()) {
                    (None, Some([_, _, _])) => true,
                    (Some((u, p)), Some([_, wu, wp])) => wu == u && wp == p,
                    _ => false,
                };
                if good {
                    authed = true;
                    io.note("! login ok".into());
                    io.say(&format!("{tag} OK AUTHENTICATE completed"))?;
                } else {
                    io.note("! login failed".into());
                    io.say(&format!("{tag} NO [AUTHENTICATIONFAILED] invalid credentials"))?;
                }
            }
            "EXAMINE" | "SELECT" => {
                if !authed {
                    io.say(&format!("{tag} BAD log in first"))?;
                    continue;
                }
                let folder = words(&args).into_iter().next().unwrap_or_default();
                if !folder.eq_ignore_ascii_case("INBOX") {
                    selected = false;
                    io.say(&format!("{tag} NO [NONEXISTENT] no such folder"))?;
                    continue;
                }
                let (n, validity, next) = {
                    let m = lock(&st.mailbox);
                    (m.messages.len(), m.uid_validity, m.next_uid)
                };
                selected = true;
                io.say("* FLAGS (\\Seen \\Answered \\Flagged \\Deleted \\Draft)")?;
                io.say(&format!("* {n} EXISTS"))?;
                io.say("* 0 RECENT")?;
                io.say(&format!("* OK [UIDVALIDITY {validity}] UIDs valid"))?;
                io.say(&format!("* OK [UIDNEXT {next}] predicted next UID"))?;
                if verb == "EXAMINE" {
                    io.say(&format!("{tag} OK [READ-ONLY] EXAMINE completed"))?;
                } else {
                    io.say(&format!("{tag} OK [READ-WRITE] SELECT completed"))?;
                }
            }
            "FETCH" | "UID" => {
                let (by_uid, args) = if verb == "UID" {
                    let (sub, a) = args.split_once(' ').unwrap_or((args.as_str(), ""));
                    if !sub.eq_ignore_ascii_case("FETCH") {
                        io.say(&format!("{tag} BAD only UID FETCH is supported"))?;
                        continue;
                    }
                    (true, a.trim().to_owned())
                } else {
                    (false, args)
                };
                if !selected {
                    io.say(&format!("{tag} BAD select a folder first"))?;
                    continue;
                }
                let (set, items) = args.split_once(' ').unwrap_or((args.as_str(), ""));
                let items = parse_items(items);
                let found = pick(&lock(&st.mailbox).messages, set, by_uid);
                for (seq, uid, raw) in found {
                    let (bytes, note) = fetch_response(seq, uid, &raw, &items, by_uid);
                    io.send(&bytes, note)?;
                }
                io.say(&format!("{tag} OK {}FETCH completed", if by_uid { "UID " } else { "" }))?;
            }
            "LIST" | "LSUB" => {
                io.say(&format!("* {verb} () \"/\" \"INBOX\""))?;
                io.say(&format!("{tag} OK {verb} completed"))?;
            }
            "CLOSE" => {
                selected = false;
                io.say(&format!("{tag} OK CLOSE completed"))?;
            }
            "LOGOUT" => {
                io.say("* BYE ito-testserver closing")?;
                io.say(&format!("{tag} OK LOGOUT completed"))?;
                break;
            }
            _ => io.say(&format!("{tag} BAD command not supported"))?,
        }
    }
    Ok(())
}

/// The arguments of a command: bare words and quoted strings (`\"` and `\\` undone).
fn words(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = args.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut s = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => s.extend(chars.next()),
                    '"' => break,
                    c => s.push(c),
                }
            }
            out.push(s);
        } else {
            let mut s = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                s.push(c);
                chars.next();
            }
            out.push(s);
        }
    }
    out
}

/// The fetch items: `UID BODY.PEEK[HEADER.FIELDS (FROM SUBJECT)]` or the same in parentheses, split at the top level.
fn parse_items(text: &str) -> Vec<String> {
    let t = text.trim();
    let t = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')).unwrap_or(t);
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for c in t.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The messages in a set (`1`, `3:5`, `7:*`, `1,4:6`) as `(seq, uid, raw)`. As RFC 3501 says, a range that holds
/// `*` and misses everything still gives the last message.
fn pick(messages: &[(u32, Vec<u8>)], set: &str, by_uid: bool) -> Vec<(u32, u32, Vec<u8>)> {
    let key = |i: usize| if by_uid { messages[i].0 } else { i as u32 + 1 };
    let max = if messages.is_empty() { 0 } else { key(messages.len() - 1) };
    let mut ranges = Vec::new();
    let mut star = false;
    for part in set.split(',') {
        let num = |s: &str, star: &mut bool| {
            if s == "*" {
                *star = true;
                Some(max)
            } else {
                s.parse::<u32>().ok()
            }
        };
        let (a, b) = part.split_once(':').unwrap_or((part, part));
        if let (Some(a), Some(b)) = (num(a, &mut star), num(b, &mut star)) {
            ranges.push((a.min(b), a.max(b)));
        }
    }
    let mut out: Vec<(u32, u32, Vec<u8>)> = (0..messages.len())
        .filter(|&i| ranges.iter().any(|&(a, b)| (a..=b).contains(&key(i))))
        .map(|i| (i as u32 + 1, messages[i].0, messages[i].1.clone()))
        .collect();
    if out.is_empty() && star && !messages.is_empty() {
        let i = messages.len() - 1;
        out.push((i as u32 + 1, messages[i].0, messages[i].1.clone()));
    }
    out
}

/// The header of a message as CRLF lines with the blank line after, optionally only some fields (a folded field
/// is kept whole).
fn header_part(raw: &[u8], fields: Option<&[String]>) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    let mut groups: Vec<Vec<&str>> = Vec::new();
    for l in text.split('\n') {
        let l = l.trim_end_matches('\r');
        if l.is_empty() {
            break;
        }
        if l.starts_with([' ', '\t']) {
            if let Some(g) = groups.last_mut() {
                g.push(l);
            }
        } else {
            groups.push(vec![l]);
        }
    }
    let mut out = String::new();
    for g in groups {
        let name = g[0].split(':').next().unwrap_or("").trim().to_ascii_uppercase();
        if fields.is_none_or(|f| f.iter().any(|x| x.eq_ignore_ascii_case(&name))) {
            for l in g {
                out.push_str(l);
                out.push_str("\r\n");
            }
        }
    }
    out.push_str("\r\n");
    out.into_bytes()
}

fn fetch_response(seq: u32, uid: u32, raw: &[u8], items: &[String], by_uid: bool) -> (Vec<u8>, String) {
    let mut out = format!("* {seq} FETCH (").into_bytes();
    let mut note = format!("S: * {seq} FETCH (");
    let mut first = true;
    let mut push = |out: &mut Vec<u8>, note: &mut String, text: &str, literal: Option<&[u8]>| {
        if !first {
            out.push(b' ');
            note.push(' ');
        }
        first = false;
        out.extend_from_slice(text.as_bytes());
        note.push_str(text);
        if let Some(l) = literal {
            out.extend_from_slice(format!(" {{{}}}\r\n", l.len()).as_bytes());
            out.extend_from_slice(l);
            note.push_str(&format!(" <{} bytes>", l.len()));
        }
    };
    let mut want = items.iter().map(|i| i.to_ascii_uppercase()).collect::<Vec<_>>();
    if by_uid && !want.iter().any(|i| i == "UID") {
        want.insert(0, "UID".into());
    }
    for item in &want {
        match item.as_str() {
            "UID" => push(&mut out, &mut note, &format!("UID {uid}"), None),
            "FLAGS" => push(&mut out, &mut note, "FLAGS ()", None),
            "RFC822.SIZE" => push(&mut out, &mut note, &format!("RFC822.SIZE {}", raw.len()), None),
            "RFC822" => push(&mut out, &mut note, "RFC822", Some(raw)),
            "BODY[]" | "BODY.PEEK[]" => push(&mut out, &mut note, "BODY[]", Some(raw)),
            "RFC822.HEADER" => push(&mut out, &mut note, "RFC822.HEADER", Some(&header_part(raw, None))),
            "BODY[HEADER]" | "BODY.PEEK[HEADER]" => push(&mut out, &mut note, "BODY[HEADER]", Some(&header_part(raw, None))),
            other => {
                let inner = other.strip_prefix("BODY.PEEK[").or_else(|| other.strip_prefix("BODY[")).and_then(|r| r.strip_suffix(']'));
                if let Some(spec) = inner.and_then(|s| s.strip_prefix("HEADER.FIELDS")) {
                    let list: Vec<String> =
                        spec.trim().trim_start_matches('(').trim_end_matches(')').split_whitespace().map(str::to_owned).collect();
                    let name = format!("BODY[HEADER.FIELDS ({})]", list.join(" "));
                    push(&mut out, &mut note, &name, Some(&header_part(raw, Some(&list))));
                }
            }
        }
    }
    out.extend_from_slice(b")\r\n");
    note.push(')');
    (out, note)
}
