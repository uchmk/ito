//! A test sink for what would reach a person (feature `test-sink`).
//!
//! With `ITO_NOTIFY_SINK=<file>` set, [`toast::show`](crate::toast::show), [`notifier::Notifier`](crate::notifier::Notifier)
//! and [`sound::play`](crate::sound::play) put nothing on the screen and make no sound: each appends one table to the file instead
//!
//! ```toml
//! [[toast]]
//! n = 1                  # counted from 1, in the order shown, by the process
//! app = "uchmk.koyomi"
//! title = "…"
//! body = "…"
//! tag = "…"
//! launch = "open"
//! actions = ["Open=open"]   # label=arg of each button
//!
//! [[sound]]
//! name = "chime"
//! ```
//!
//! A click is a line in `<file>.click`, read about every 200 ms and then emptied: `3` clicks the 3rd toast itself (its `launch`),
//! `3:open` presses the button whose arg is `open`. The app's [`Click`](crate::notifier::Click) is called as for a real click.
//! A build without the feature never reads the variable.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// The environment variable that names the sink file.
pub const ENV: &str = "ITO_NOTIFY_SINK";

static LOCK: Mutex<()> = Mutex::new(());
static COUNT: AtomicU64 = AtomicU64::new(0);

/// The sink file, when the variable is set.
pub fn path() -> Option<PathBuf> {
    std::env::var_os(ENV).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn append(file: &std::path::Path, text: &str) {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = f.write_all(text.as_bytes());
    }
}

/// Record a toast; returns its number (from 1).
pub(crate) fn toast(file: &std::path::Path, app: &str, title: &str, body: &str, tag: &str, launch: &str, actions: &[String]) -> u64 {
    let n = COUNT.fetch_add(1, Ordering::SeqCst) + 1;
    let acts: Vec<String> = actions.iter().map(|a| quote(a)).collect();
    append(
        file,
        &format!(
            "[[toast]]\nn = {n}\napp = {}\ntitle = {}\nbody = {}\ntag = {}\nlaunch = {}\nactions = [{}]\n\n",
            quote(app),
            quote(title),
            quote(body),
            quote(tag),
            quote(launch),
            acts.join(", ")
        ),
    );
    n
}

/// Record a sound.
pub(crate) fn sound(file: &std::path::Path, name: &str) {
    append(file, &format!("[[sound]]\nname = {}\n\n", quote(name)));
}

/// Take the clicks waiting in `<file>.click`: `(n, arg)`, `arg` empty for the toast itself.
pub(crate) fn take_clicks(file: &std::path::Path) -> Vec<(u64, String)> {
    let mut name = file.as_os_str().to_owned();
    name.push(".click");
    let click = PathBuf::from(name);
    let Ok(text) = std::fs::read_to_string(&click) else { return Vec::new() };
    if text.is_empty() {
        return Vec::new();
    }
    let _ = std::fs::write(&click, "");
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (n, arg) = l.split_once(':').unwrap_or((l, ""));
            Some((n.trim().parse().ok()?, arg.trim().to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_quoted_so_a_toml_reader_gets_them_back() {
        let s = "a \"q\" \\ \n\ttab \u{1} é";
        let t = format!("v = {}", quote(s));
        assert_eq!(t, "v = \"a \\\"q\\\" \\\\ \\n\\ttab \\u0001 é\"");
    }

    #[test]
    fn clicks_are_taken_once() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("sink.toml");
        std::fs::write(d.path().join("sink.toml.click"), "2\n1:open\nx\n").unwrap();
        assert_eq!(take_clicks(&f), vec![(2, String::new()), (1, "open".to_owned())]);
        assert!(take_clicks(&f).is_empty());
    }

    #[test]
    fn toasts_are_written_and_numbered() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("sink.toml");
        let a = toast(&f, "uchmk.x", "T", "B", "tag", "open", &["Open=open".into()]);
        let b = toast(&f, "uchmk.x", "T2", "B2", "", "", &[]);
        assert_eq!(b, a + 1);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("title = \"T2\"") && text.contains("actions = [\"Open=open\"]"));
        sound(&f, "chime");
        assert!(std::fs::read_to_string(&f).unwrap().contains("[[sound]]\nname = \"chime\""));
    }
}
