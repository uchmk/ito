//! Test hooks (feature `test-hook`): a tray with no icon, whose state is written to a file and whose clicks come
//! from a script. A build without the feature never reads the variables.
//!
//! - `ITO_TRAY_STATE=<file>`: rewritten (TOML) at every change: `state`, `tip` (as given, no mark), `[[menu]]`
//!   with `id`, `label`, `enabled`, `checked` (a separator has `separator = true`).
//! - `ITO_TRAY_KEYS=<script>`: `<TrayClick>`, `<TrayMenu:id>` and `<Wait:ms>`, separated by white space. They
//!   arrive on [`crate::Tray::events`] like the real ones. At the end `<script>.done` is written (`ok`, or
//!   `error: …` when the script cannot be read).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{Event, Item, State};

pub const STATE_ENV: &str = "ITO_TRAY_STATE";
pub const KEYS_ENV: &str = "ITO_TRAY_KEYS";

/// One step of the script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Click,
    Menu(String),
    Wait(u64),
}

/// Read the script. An unknown word is an error that names it, so a typo does not pass silently.
pub fn parse(script: &str) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    for w in script.split_whitespace() {
        let inner = w.strip_prefix('<').and_then(|w| w.strip_suffix('>')).ok_or_else(|| format!("not a <step>: {w}"))?;
        let (name, arg) = inner.split_once(':').map_or((inner, None), |(n, a)| (n, Some(a)));
        out.push(match (name, arg) {
            ("TrayClick", None) => Step::Click,
            ("TrayMenu", Some(id)) if !id.is_empty() => Step::Menu(id.to_owned()),
            ("Wait", Some(ms)) => Step::Wait(ms.parse().map_err(|_| format!("bad number: {w}"))?),
            _ => return Err(format!("unknown step: {w}")),
        });
    }
    Ok(out)
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
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The state file's text.
pub fn render(state: Option<State>, tip: &str, menu: &[Item]) -> String {
    let mut s = String::new();
    match state {
        Some(st) => {
            let _ = writeln!(s, "state = {}", quote(&format!("{st:?}")));
        }
        None => s.push_str("state = \"\"\n"),
    }
    let _ = writeln!(s, "tip = {}", quote(tip));
    for it in menu {
        s.push_str("\n[[menu]]\n");
        match it {
            Item::Separator => s.push_str("separator = true\n"),
            Item::Entry { id, label, enabled, checked } => {
                let _ = write!(s, "id = {}\nlabel = {}\nenabled = {enabled}\nchecked = {checked}\n", quote(id), quote(label));
            }
        }
    }
    s
}

struct Shadow {
    state: Option<State>,
    tip: String,
    menu: Vec<Item>,
}

/// The tray that has no icon.
pub struct Fake {
    shadow: Arc<Mutex<Shadow>>,
    file: Option<PathBuf>,
    rx: Receiver<Event>,
    stop: Arc<AtomicBool>,
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn write_file(file: &Path, text: &str) {
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, file).is_err() {
        let _ = std::fs::write(file, text);
    }
}

/// True when either variable is set.
pub fn wanted() -> bool {
    std::env::var_os(STATE_ENV).is_some() || std::env::var_os(KEYS_ENV).is_some()
}

impl Fake {
    pub fn new(tip: &str, wake: Box<dyn Fn() + Send>) -> Fake {
        let file = std::env::var_os(STATE_ENV).map(PathBuf::from);
        let script = std::env::var_os(KEYS_ENV).map(PathBuf::from);
        let shadow = Arc::new(Mutex::new(Shadow { state: None, tip: tip.to_owned(), menu: Vec::new() }));
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let fake = Fake { shadow, file, rx, stop: stop.clone() };
        fake.flush();
        if let Some(script) = script {
            let _ = std::thread::Builder::new().name("tray-hook".into()).spawn(move || run(&script, tx, wake, &stop));
        }
        fake
    }

    fn shadow(&self) -> std::sync::MutexGuard<'_, Shadow> {
        self.shadow.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn flush(&self) {
        if let Some(f) = &self.file {
            let s = self.shadow();
            write_file(f, &render(s.state, &s.tip, &s.menu));
        }
    }

    pub fn set_state(&self, state: Option<State>) {
        self.shadow().state = state;
        self.flush();
    }

    pub fn set_tip(&self, tip: &str) {
        self.shadow().tip = tip.to_owned();
        self.flush();
    }

    pub fn set_menu(&self, items: Vec<Item>) {
        self.shadow().menu = items;
        self.flush();
    }

    pub fn events(&self) -> &Receiver<Event> {
        &self.rx
    }
}

fn run(script: &Path, tx: Sender<Event>, wake: Box<dyn Fn() + Send>, stop: &AtomicBool) {
    let mut done = script.as_os_str().to_owned();
    done.push(".done");
    let done = PathBuf::from(done);
    let result = std::fs::read_to_string(script).map_err(|e| e.to_string()).and_then(|t| parse(&t));
    match result {
        Ok(steps) => {
            for step in steps {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                match step {
                    Step::Wait(ms) => std::thread::sleep(Duration::from_millis(ms)),
                    Step::Click => {
                        let _ = tx.send(Event::Click);
                        wake();
                    }
                    Step::Menu(id) => {
                        let _ = tx.send(Event::Menu(id));
                        wake();
                    }
                }
            }
            let _ = std::fs::write(&done, "ok\n");
        }
        Err(e) => {
            let _ = std::fs::write(&done, format!("error: {e}\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn the_script_is_read_and_typos_are_named() {
        assert_eq!(parse("<TrayClick> <Wait:50>\n<TrayMenu:quit>").unwrap(), vec![Step::Click, Step::Wait(50), Step::Menu("quit".into())]);
        assert!(parse("<TrayClik>").unwrap_err().contains("TrayClik"));
        assert!(parse("<Wait:x>").is_err());
        assert!(parse("<TrayMenu:>").is_err());
        assert!(parse("TrayClick").is_err());
        assert_eq!(parse("").unwrap(), vec![]);
    }

    #[test]
    fn the_state_file_lists_state_tip_and_menu() {
        let menu = vec![
            Item::Entry { id: "open".into(), label: "Open \"it\"".into(), enabled: true, checked: false },
            Item::Separator,
            Item::Entry { id: "mute".into(), label: "Mute".into(), enabled: false, checked: true },
        ];
        let t = render(Some(State::Waiting), "3 jobs", &menu);
        assert!(t.starts_with("state = \"Waiting\"\ntip = \"3 jobs\"\n"), "{t}");
        assert!(t.contains("label = \"Open \\\"it\\\"\""), "{t}");
        assert!(t.contains("[[menu]]\nseparator = true\n"), "{t}");
        assert!(t.contains("id = \"mute\"\nlabel = \"Mute\"\nenabled = false\nchecked = true\n"), "{t}");
        assert!(render(None, "x", &[]).starts_with("state = \"\"\n"));
    }

    #[test]
    fn a_fake_tray_follows_its_script_and_writes_its_state() {
        let d = tempfile::tempdir().unwrap();
        let (state, script) = (d.path().join("tray.toml"), d.path().join("keys.txt"));
        std::fs::write(&script, "<TrayClick> <Wait:20> <TrayMenu:quit>").unwrap();
        // The variables are process-wide: this is the only test that sets them.
        std::env::set_var(STATE_ENV, &state);
        std::env::set_var(KEYS_ENV, &script);
        let woke = Arc::new(AtomicUsize::new(0));
        let w = woke.clone();
        let fake = Fake::new(
            "hello",
            Box::new(move || {
                w.fetch_add(1, Ordering::SeqCst);
            }),
        );
        std::env::remove_var(STATE_ENV);
        std::env::remove_var(KEYS_ENV);
        fake.set_state(Some(State::Waiting));
        fake.set_menu(vec![Item::entry("quit", "Quit")]);
        assert_eq!(fake.events().recv_timeout(Duration::from_secs(5)).unwrap(), Event::Click);
        assert_eq!(fake.events().recv_timeout(Duration::from_secs(5)).unwrap(), Event::Menu("quit".into()));
        let done = d.path().join("keys.txt.done");
        for _ in 0..100 {
            if done.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(std::fs::read_to_string(&done).unwrap(), "ok\n");
        assert_eq!(woke.load(Ordering::SeqCst), 2);
        let t = std::fs::read_to_string(&state).unwrap();
        assert!(t.contains("state = \"Waiting\"") && t.contains("id = \"quit\""), "{t}");
    }
}
