//! A message sent elsewhere: an ntfy topic (the phone's notification), a
//! Slack incoming webhook, or any address that takes JSON (`[notify]
//! webhook`). Through the system's `curl`, so no HTTP client is built in; each
//! on a thread of its own, and a failure only goes to stderr.

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

/// The `curl` arguments for one notice, before the address. The body goes
/// with `--data-raw`: with `-d`, a message starting `@` (a program's own
/// notification can say anything) has curl send the file it names (the
/// source review, 2026-10-07).
pub fn args(format: &str, level: Level, title: &str, message: &str, folder: &str) -> Vec<String> {
    let json = |fields: Vec<(&str, &str)>| {
        format!("{{{}}}", fields.iter().map(|(k, v)| format!("{}: {}", json_string(k), json_string(v))).collect::<Vec<_>>().join(", "))
    };
    let mut out: Vec<String> = ["-sS", "-m", "10", "-X", "POST"].iter().map(|s| (*s).to_owned()).collect();
    match format {
        "slack" => {
            out.extend([
                "-H".into(),
                "Content-Type: application/json".into(),
                "--data-raw".into(),
                json(vec![("text", &format!("*{title}*\n{message}"))]),
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
    }
}
