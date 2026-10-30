//! A message sent by mail (SMTP): `[[notify.mails]]`, each a [`Mail`]. Through the system's `curl`, so no
//! mail library is built in; each on a thread of its own, and a failure only goes to stderr (and to
//! [`on_failure`], for an app that shows it).
//!
//! The connection is always encrypted: port 465 is TLS from the start (`smtps://`), any other port must
//! upgrade with STARTTLS (`--ssl-reqd`). The password is only ever a `secret:` reference to the OS
//! credential store (see `ito_secret`); a password written in the file is a problem and nothing is sent.
//! It reaches `curl` on its standard input, so it is not in the process list.

use crate::webhook::{reference, Level};
use std::sync::OnceLock;

/// One place notices go by mail, a `[[notify.mails]]` of the settings.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Mail {
    /// What the app calls it (`ops`), so a notice can go to some places only. Empty: no name.
    pub name: String,
    pub host: String,
    /// 465: TLS from the start; any other: STARTTLS.
    pub port: u16,
    /// Empty: the server takes mail with no login.
    pub user: String,
    /// As written: `secret:mail-ops`. See [`reveal`].
    pub password: String,
    /// `jobs@example.com` or `koyomi <jobs@example.com>`.
    pub from: String,
    pub to: Vec<String>,
    /// The events sent here, by the app's words.
    pub states: Vec<String>,
    /// The password read back from the credential store by [`reveal`].
    pub secret: Option<String>,
}

impl std::fmt::Debug for Mail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mail")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password)
            .field("from", &self.from)
            .field("to", &self.to)
            .field("states", &self.states)
            .field("secret", &self.secret.as_ref().map(|_| "***"))
            .finish()
    }
}

impl Mail {
    /// Whether a notice of `state` goes here: it can be sent at all, and takes that event.
    pub fn takes(&self, state: &str) -> bool {
        self.ready() && self.states.iter().any(|s| s == state)
    }

    /// Whether a mail can be sent from this: a host, a port, good addresses, and the login (if any) read back.
    pub fn ready(&self) -> bool {
        !self.host.trim().is_empty()
            && self.port != 0
            && address(&self.from).is_some()
            && !self.to.is_empty()
            && self.to.iter().all(|t| address(t).is_some())
            && (self.user.trim().is_empty() || self.secret.as_deref().is_some_and(|s| !s.is_empty() && !s.contains(['\r', '\n'])))
    }

    /// What is wrong with it, for the settings' warnings (the app puts the file and the table in front):
    /// no host, a bad port, a bad or missing address, a password written in the file, a state not in `states`.
    /// A table with no host at all is only the one not filled in yet: it is told, and nothing is sent.
    pub fn problems(&self, states: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        if self.host.trim().is_empty() {
            out.push("`host` is empty".to_owned());
        }
        if self.port == 0 {
            out.push("`port` is not 1 to 65535".to_owned());
        }
        if address(&self.from).is_none() {
            out.push(format!("`from` `{}` is not a mail address", self.from));
        }
        if self.to.is_empty() {
            out.push("`to` has no address".to_owned());
        }
        for t in self.to.iter().filter(|t| address(t).is_none()) {
            out.push(format!("`to` `{t}` is not a mail address"));
        }
        let pw = self.password.trim();
        if !pw.is_empty() && reference(pw).is_none() {
            out.push("`password` is written in the file: keep it in the credential store and write `secret:<name>`".to_owned());
        }
        if self.user.trim().is_empty() && !pw.is_empty() {
            out.push("`password` needs a `user`".to_owned());
        }
        if !self.user.trim().is_empty() && pw.is_empty() {
            out.push("`user` needs a `password` (`secret:<name>`)".to_owned());
        }
        for s in self.states.iter().filter(|s| !states.contains(&s.as_str())) {
            out.push(format!("`states` `{s}` is not {}", states.join(", ")));
        }
        out
    }
}

/// `mails` with each `secret:` password read back from the store of `app`. What cannot be read (gone,
/// locked, no store) stays unread, so nothing is sent there, and comes back as a line for the settings'
/// warnings. Blocks: call it from a worker thread.
pub fn reveal(app: &str, mails: &[Mail]) -> (Vec<Mail>, Vec<String>) {
    let mut warnings = Vec::new();
    let out = mails
        .iter()
        .map(|m| {
            let Some(name) = reference(&m.password) else { return Mail { secret: None, ..m.clone() } };
            let secret = match ito_secret::get(app, name) {
                Ok(Some(p)) => Some(p),
                Ok(None) => {
                    warnings.push(format!("`password` `{}` is not in the credential store", m.password));
                    None
                }
                Err(e) => {
                    warnings.push(format!("`password` `{}` cannot be read from the credential store: {e}", m.password));
                    None
                }
            };
            Mail { secret, ..m.clone() }
        })
        .collect();
    (out, warnings)
}

/// The bare address in `text`: `jobs@example.com` from either `jobs@example.com` or `koyomi <jobs@example.com>`.
/// `None` when it is not one (no `@`, nothing either side, a space or a control character, a second `@`).
pub fn address(text: &str) -> Option<&str> {
    let t = text.trim();
    let a = match (t.rfind('<'), t.ends_with('>')) {
        (Some(i), true) => &t[i + 1..t.len() - 1],
        (None, false) => t,
        _ => return None,
    }
    .trim();
    let (local, domain) = a.split_once('@')?;
    let ok = !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !a.chars().any(|c| c.is_whitespace() || c.is_control() || "<>\",;".contains(c));
    ok.then_some(a)
}

/// Send the notice of `state` to every mail place that takes it, each on a thread of its own (see [`send`]).
pub fn send_all(mails: &[Mail], state: &str, level: Level, title: &str, message: &str) {
    for m in mails.iter().filter(|m| m.takes(state)) {
        send(m.clone(), level, title.to_owned(), message.to_owned());
    }
}

type Failed = Box<dyn Fn(&str) + Send + Sync>;
static FAILED: OnceLock<Failed> = OnceLock::new();

/// Call `f` with a line (`mail "ops" failed: …`) when a mail could not be sent even after the second try, for an
/// app that shows it (its notices, its log). Called on the sending thread; set once, the first call wins.
pub fn on_failure(f: impl Fn(&str) + Send + Sync + 'static) {
    let _ = FAILED.set(Box::new(f));
}

const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

/// Send one notice by mail, on a thread; a failure is tried once more after 30 seconds.
pub fn send(mail: Mail, level: Level, title: String, message: String) {
    let _ = std::thread::Builder::new().name("mail".into()).spawn(move || {
        let mut result = post(&mail, level, &title, &message);
        if result.is_err() {
            std::thread::sleep(RETRY_AFTER);
            result = post(&mail, level, &title, &message);
        }
        if let Err(e) = result {
            let line = format!("mail \"{}\" failed: {e}", mail.name);
            eprintln!("{line}");
            if let Some(f) = FAILED.get() {
                f(&line);
            }
        }
    });
}

/// The `curl` arguments for one mail, before `--upload-file`: the address (TLS from the start on 465, else STARTTLS
/// required), the envelope, and the login read from standard input (`--config -`).
pub fn args(mail: &Mail) -> Vec<String> {
    let url =
        if mail.port == 465 { format!("smtps://{}:465", mail.host.trim()) } else { format!("smtp://{}:{}", mail.host.trim(), mail.port) };
    let mut out: Vec<String> = ["-sS", "-m", "60"].iter().map(|s| (*s).to_owned()).collect();
    if mail.port != 465 {
        out.push("--ssl-reqd".into());
    }
    out.extend(["--url".into(), url, "--mail-from".into(), address(&mail.from).unwrap_or_default().to_owned()]);
    for t in &mail.to {
        out.extend(["--mail-rcpt".into(), address(t).unwrap_or_default().to_owned()]);
    }
    if !mail.user.trim().is_empty() {
        out.extend(["--config".into(), "-".into()]);
    }
    out
}

/// What goes to `curl`'s standard input: the login, as a config line (`user = "…"`).
fn login(mail: &Mail) -> String {
    let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("user = \"{}:{}\"\n", q(mail.user.trim()), q(mail.secret.as_deref().unwrap_or_default()))
}

fn post(mail: &Mail, level: Level, title: &str, message: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let dir = std::env::temp_dir();
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let file = dir.join(format!("ito-mail-{}-{}.eml", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::write(&file, body(mail, level, title, message, now())).map_err(|e| format!("cannot write the message: {e}"))?;
    let mut cmd = Command::new("curl");
    cmd.args(args(mail)).arg("--upload-file").arg(&file).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = (|| {
        let mut child = cmd.spawn().map_err(|e| format!("curl did not run: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            if !mail.user.trim().is_empty() {
                let _ = stdin.write_all(login(mail).as_bytes());
            }
        }
        child.wait_with_output().map_err(|e| format!("curl did not finish: {e}"))
    })();
    let _ = std::fs::remove_file(&file);
    let out = out?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// Seconds since 1970 (UTC).
fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The mail itself (CRLF lines): headers with the non-ASCII words encoded, and the message in UTF-8 as base64.
pub fn body(mail: &Mail, level: Level, title: &str, message: &str, at: i64) -> String {
    let (name, addr) = split_from(&mail.from);
    let from = if name.is_empty() { addr.to_owned() } else { format!("{} <{addr}>", word(&name)) };
    let to = mail.to.iter().filter_map(|t| address(t)).collect::<Vec<_>>().join(", ");
    let flat = |s: &str| s.replace(['\r', '\n'], " ");
    let mut out = String::new();
    out.push_str(&format!("From: {from}\r\nTo: {to}\r\nSubject: {}\r\nDate: {}\r\n", word(&flat(title)), date(at)));
    out.push_str(&format!("X-Notice-State: {}\r\n", level.word()));
    out.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    let b = base64(message.replace("\r\n", "\n").replace('\n', "\r\n").as_bytes());
    for line in b.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(line).unwrap_or_default());
        out.push_str("\r\n");
    }
    out
}

/// The display name (without quotes) and the address of a `from`.
fn split_from(from: &str) -> (String, &str) {
    let addr = address(from).unwrap_or_default();
    let name = from.trim().rfind('<').map_or("", |i| from.trim()[..i].trim()).trim_matches('"').trim();
    (name.replace(['\r', '\n', '"', '\\'], " "), addr)
}

/// `text` as a header word: as it is when plain ASCII, else UTF-8 encoded words (RFC 2047) of at most 75 characters.
fn word(text: &str) -> String {
    if text.is_ascii() && text.len() <= 900 && !text.chars().any(|c| c.is_control()) {
        return text.to_owned();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut chunk = String::new();
    for c in text.chars().filter(|c| !c.is_control()) {
        if chunk.len() + c.len_utf8() > 36 {
            parts.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    if !chunk.is_empty() {
        parts.push(chunk);
    }
    parts.iter().map(|p| format!("=?UTF-8?B?{}?=", base64(p.as_bytes()))).collect::<Vec<_>>().join("\r\n ")
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// `Thu, 29 Oct 2026 12:00:00 +0000` for the seconds since 1970.
fn date(at: i64) -> String {
    let (days, secs) = (at.div_euclid(86400), at.rem_euclid(86400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][m as usize - 1];
    format!("{wd}, {d} {mon} {y} {:02}:{:02}:{:02} +0000", secs / 3600, secs % 3600 / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops() -> Mail {
        Mail {
            name: "ops".into(),
            host: "smtp.example.com".into(),
            port: 587,
            user: "jobs@example.com".into(),
            password: "secret:mail-ops".into(),
            from: "koyomi <jobs@example.com>".into(),
            to: vec!["ops@example.com".into(), "Boss <boss@example.com>".into()],
            states: vec!["failed".into()],
            secret: Some("p\"w\\d".into()),
        }
    }

    #[test]
    fn an_address_is_read_from_either_form_and_nothing_else() {
        assert_eq!(address(" jobs@example.com "), Some("jobs@example.com"));
        assert_eq!(address("koyomi <jobs@example.com>"), Some("jobs@example.com"));
        assert_eq!(address("\"A, B\" <a@b.c>"), Some("a@b.c"));
        for bad in ["", "jobs", "@x.y", "a@", "a@b@c", "a b@c.d", "<a@b.c", "a@b.c>", "a@b.c\r\nBcc: x@y.z"] {
            assert_eq!(address(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_good_place_is_ready_and_takes_only_its_states() {
        let m = ops();
        assert!(m.problems(&["done", "failed"]).is_empty());
        assert!(m.ready() && m.takes("failed") && !m.takes("done"));
    }

    #[test]
    fn until_the_password_is_read_back_nothing_is_sent() {
        let m = Mail { secret: None, ..ops() };
        assert!(!m.ready() && !m.takes("failed"));
        let open = Mail { user: String::new(), password: String::new(), secret: None, ..ops() };
        assert!(open.ready(), "a server with no login needs none");
    }

    #[test]
    fn a_password_in_the_file_is_a_problem_and_never_shown() {
        let m = Mail { password: "hunter2".into(), secret: None, ..ops() };
        let p = m.problems(&["failed"]);
        assert_eq!(p.len(), 1);
        assert!(p[0].contains("credential store"));
        assert!(!m.ready());
        assert!(!format!("{:?}", ops()).contains("p\\\"w"), "the revealed password stays out of Debug");
    }

    #[test]
    fn each_mistake_is_told() {
        let m = Mail { host: " ".into(), port: 0, from: "x".into(), to: vec!["y".into()], states: vec!["nope".into()], ..ops() };
        assert_eq!(m.problems(&["done"]).len(), 5);
        let none = Mail { to: vec![], ..ops() };
        assert!(none.problems(&["failed"]).iter().any(|p| p.contains("`to` has no address")));
    }

    #[test]
    fn the_port_picks_how_it_is_encrypted_and_the_password_is_not_an_argument() {
        let a = args(&ops());
        assert!(a.contains(&"--ssl-reqd".to_owned()) && a.contains(&"smtp://smtp.example.com:587".to_owned()));
        assert_eq!(a.iter().filter(|x| *x == "--mail-rcpt").count(), 2);
        assert!(a.contains(&"boss@example.com".to_owned()), "the envelope holds the bare address");
        assert!(!a.iter().any(|x| x.contains("p\"w")), "no password in the process list");
        let tls = args(&Mail { port: 465, ..ops() });
        assert!(tls.contains(&"smtps://smtp.example.com:465".to_owned()) && !tls.contains(&"--ssl-reqd".to_owned()));
        assert_eq!(login(&ops()), "user = \"jobs@example.com:p\\\"w\\\\d\"\n");
        assert!(!args(&Mail { user: String::new(), ..ops() }).contains(&"--config".to_owned()));
    }

    #[test]
    fn the_message_is_utf8_with_encoded_headers() {
        let b = body(&ops(), Level::Error, "ジョブが失敗しました", "line1\nline2", 1_793_361_600);
        assert!(b.contains("Subject: =?UTF-8?B?"));
        assert!(!b.contains("From: =?UTF-8?B?") && b.contains("From: koyomi <jobs@example.com>\r\n"));
        assert!(b.contains("To: ops@example.com, boss@example.com\r\n"));
        assert!(b.contains("Date: Fri, 30 Oct 2026 12:00:00 +0000\r\n"));
        assert!(b.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(b.ends_with("bGluZTENCmxpbmUy\r\n"), "line1 CRLF line2 in base64");
        assert!(b.lines().all(|l| l.len() <= 78));
    }

    #[test]
    fn a_title_cannot_add_a_header_and_a_long_one_is_folded() {
        let b = body(&ops(), Level::Done, "x\r\nBcc: evil@example.com", "m", 0);
        assert!(!b.contains("\r\nBcc:"));
        let long = body(&ops(), Level::Done, &"長い題名".repeat(30), "m", 0);
        assert!(long.lines().all(|l| l.len() <= 78), "every line is within the limit");
        assert!(long.contains("Date: Thu, 1 Jan 1970 00:00:00 +0000"));
    }

    #[test]
    fn base64_matches_the_known_forms() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
