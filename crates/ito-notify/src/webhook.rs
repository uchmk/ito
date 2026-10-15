//! A message sent elsewhere: an ntfy topic (the phone's notification), a
//! Slack or Discord incoming webhook, or any address that takes JSON
//! (`[notify] webhook`). Through the system's `curl`, so no HTTP client is
//! built in; each on a thread of its own, and a failure only goes to stderr.

/// What kind of notice it is: the ntfy tag and priority, and the JSON's
/// `state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Wants a person.
    Waiting,
    Error,
    Done,
    /// A threshold was crossed.
    Alert,
}

impl Level {
    pub fn word(self) -> &'static str {
        match self {
            Level::Waiting => "waiting",
            Level::Error => "error",
            Level::Done => "done",
            Level::Alert => "alert",
        }
    }
}

/// `text` as a JSON string, quotes included.
fn json_string(text: &str) -> String {
    let mut out = String::from('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The most a Discord message holds, in characters. It is counted here in
/// UTF-16 units, which are never fewer than characters, so the cut fits
/// whichever way Discord counts an emoji.
const DISCORD_MAX: usize = 2000;

/// `title` in bold over `message`, cut to what Discord takes (a longer
/// message is refused whole).
fn discord_content(title: &str, message: &str) -> String {
    let text = format!("**{title}**\n{message}");
    if text.encode_utf16().count() <= DISCORD_MAX {
        return text;
    }
    let mut cut = String::new();
    let mut units = 0;
    for c in text.chars() {
        units += c.len_utf16();
        if units > DISCORD_MAX - 1 {
            break;
        }
        cut.push(c);
    }
    cut.push('\u{2026}');
    cut
}

/// The `curl` arguments for one notice, before the address. The body goes
/// with `--data-raw`: with `-d`, a message starting `@` (a program's own
/// notification can say anything) has curl send the file it names (the
/// source review, 2026-10-07). `-f` makes an answer of 400 or more (a wrong
/// address, too many notices at once) a failure, so it is tried again and said on
/// stderr, where before only a network that was out was.
pub fn args(format: &str, level: Level, title: &str, message: &str, folder: &str) -> Vec<String> {
    let json = |fields: Vec<(&str, &str)>| {
        format!("{{{}}}", fields.iter().map(|(k, v)| format!("{}: {}", json_string(k), json_string(v))).collect::<Vec<_>>().join(", "))
    };
    let mut out: Vec<String> = ["-sS", "-f", "-m", "10", "-X", "POST"].iter().map(|s| (*s).to_owned()).collect();
    match format {
        "slack" => {
            out.extend([
                "-H".into(),
                "Content-Type: application/json".into(),
                "--data-raw".into(),
                json(vec![("text", &format!("*{title}*\n{message}"))]),
            ]);
        }
        // No mention pings anyone: what a program says could hold
        // `@everyone`.
        "discord" => {
            out.extend([
                "-H".into(),
                "Content-Type: application/json".into(),
                "--data-raw".into(),
                format!(r#"{{"content": {}, "allowed_mentions": {{"parse": []}}}}"#, json_string(&discord_content(title, message))),
            ]);
        }
        "json" => {
            out.extend([
                "-H".into(),
                "Content-Type: application/json".into(),
                "--data-raw".into(),
                json(vec![("title", title), ("message", message), ("folder", folder), ("state", level.word())]),
            ]);
        }
        // ntfy: the body is the message, the title a header.
        _ => {
            let (tag, priority) = match level {
                Level::Error => ("x", "high"),
                Level::Done => ("white_check_mark", "default"),
                Level::Alert => ("warning", "high"),
                Level::Waiting => ("hourglass", "default"),
            };
            out.extend([
                "-H".into(),
                format!("Title: {}", title.replace(['\r', '\n'], " ")),
                "-H".into(),
                format!("Tags: {tag}"),
                "-H".into(),
                format!("Priority: {priority}"),
                "--data-raw".into(),
                message.to_owned(),
            ]);
        }
    }
    out
}

/// Send one notice to `url`, on a thread; a failure is tried once more
/// after `RETRY_AFTER` (a network that was just out).
pub fn send(url: String, format: String, level: Level, title: String, message: String, folder: String) {
    let _ = std::thread::Builder::new().name("webhook".into()).spawn(move || {
        if !post(&url, &format, level, &title, &message, &folder) {
            std::thread::sleep(RETRY_AFTER);
            post(&url, &format, level, &title, &message, &folder);
        }
    });
}

const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

/// One try; whether it got through.
fn post(url: &str, format: &str, level: Level, title: &str, message: &str, folder: &str) -> bool {
    let mut cmd = std::process::Command::new("curl");
    cmd.args(args(format, level, title, message, folder))
        .arg("--url")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.output() {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            eprintln!("the webhook failed: {}", String::from_utf8_lossy(&out.stderr).trim());
            false
        }
        Err(e) => {
            eprintln!("curl did not run for the webhook: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_format_says_it_its_way() {
        let w = Level::Waiting;
        let ntfy = args("ntfy", w, "filer is waiting", "Claude needs your permission", "/home/u/filer");
        assert!(ntfy.contains(&"Title: filer is waiting".to_string()) && ntfy.last().unwrap() == "Claude needs your permission");
        // A message that starts with @ is sent as it is, not as a file.
        let at = args("ntfy", w, "t", "@/home/u/.ssh/id_rsa", "/");
        assert!(!at.iter().any(|a| a == "-d") && at[at.len() - 2] == "--data-raw", "{at:?}");
        let slack = args("slack", w, "filer is waiting", "say \"yes\"", "/f");
        assert!(slack.last().unwrap().contains(r#""text": "*filer is waiting*\nsay \"yes\"""#), "{slack:?}");
        let json = args("json", Level::Error, "t", "m", "C:\\dev\\x");
        assert!(json.last().unwrap().contains(r#""folder": "C:\\dev\\x""#) && json.last().unwrap().contains(r#""state": "error""#));
        let failed = args("ntfy", Level::Error, "t", "m", "");
        assert!(failed.contains(&"Priority: high".to_string()) && failed.contains(&"Tags: x".to_string()));
        assert!(args("ntfy", w, "a\nb", "m", "").contains(&"Title: a b".to_string()), "one header line");
        let discord = args("discord", Level::Done, "kura is done", "say \"@everyone\"", "/f");
        assert_eq!(
            discord.last().unwrap(),
            r#"{"content": "**kura is done**\nsay \"@everyone\"", "allowed_mentions": {"parse": []}}"#,
            "{discord:?}"
        );
        for f in crate::FORMATS {
            assert!(args(f, w, "t", "m", "").contains(&"-f".to_string()), "{f}: an HTTP error is a failure");
        }
    }

    #[test]
    fn a_long_discord_message_is_cut_to_what_it_takes() {
        let long = "\u{3042}".repeat(3000);
        let cut = discord_content("t", &long);
        assert_eq!(cut.chars().count(), DISCORD_MAX);
        assert!(cut.starts_with("**t**\n\u{3042}") && cut.ends_with('\u{2026}'));
        assert_eq!(discord_content("t", "m"), "**t**\nm");
        // An emoji is two UTF-16 units: the cut stays inside 2000 of those.
        let faces = discord_content("t", &"\u{1F600}".repeat(1500));
        assert!(faces.encode_utf16().count() <= DISCORD_MAX && faces.encode_utf16().count() >= DISCORD_MAX - 2, "{}", faces.len());
        assert!(faces.ends_with("\u{1F600}\u{2026}"));
        let just = "x".repeat(DISCORD_MAX - "**t**\n".len());
        assert_eq!(discord_content("t", &just), format!("**t**\n{just}"), "exactly 2000 is not cut");
    }

    /// The address answers `status` to one request and gives back what it
    /// was sent.
    fn answer_once(status: &str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let status = status.to_owned();
        let got = std::thread::spawn(move || {
            // Not for ever: a curl that never comes (a proxy in the way)
            // fails the test instead of hanging it.
            listener.set_nonblocking(true).unwrap();
            let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
            let mut s = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && std::time::Instant::now() < until => {
                        std::thread::sleep(std::time::Duration::from_millis(20))
                    }
                    Err(e) => panic!("no request came: {e}"),
                }
            };
            s.set_nonblocking(false).unwrap();
            s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            // The head, then as much body as it says.
            loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).into_owned();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + len || n == 0 {
                        break;
                    }
                } else if n == 0 {
                    break;
                }
            }
            let _ = s.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
            String::from_utf8_lossy(&buf).into_owned()
        });
        (url, got)
    }

    /// A refused notice (an HTTP error) counts as not got through, so it is
    /// tried again; an accepted one does not.
    #[test]
    fn an_http_error_is_a_failure() {
        if std::process::Command::new("curl").arg("--version").output().is_err() {
            eprintln!("no curl here; skipped");
            return;
        }
        let (url, got) = answer_once("204 No Content");
        assert!(post(&url, "discord", Level::Done, "kura is done", "m", ""), "204 got through");
        let sent = got.join().unwrap();
        assert!(sent.starts_with("POST /hook") && sent.contains(r#""content": "**kura is done**\nm""#), "{sent}");
        let (url, got) = answer_once("404 Not Found");
        assert!(!post(&url, "json", Level::Error, "t", "m", ""), "404 did not");
        got.join().unwrap();
    }
}
