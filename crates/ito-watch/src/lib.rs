//! Watching folders for changes as they happen, not by looking every few
//! seconds (`ito-common::watch` is the polling one, for a few files).
//!
//! Starting to watch a folder opens a handle on it (`ReadDirectoryChangesW`
//! on Windows), which can hang on a network drive, so the watcher lives on a
//! thread of its own: the caller only hands it the folders it wants. A folder
//! on a network share (`\\server\share`) is not watched, and a folder that
//! cannot be watched says so ([`Event::Watching`]`(false)`): the caller falls
//! back to looking every few seconds.
//!
//! Only the newest wish counts: asking again before the thread has finished
//! the last one skips straight to the latest set of folders.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;

use notify::{RecursiveMode, Watcher as _};

/// What the watcher says.
#[derive(Debug)]
pub enum Event {
    /// These paths changed (or were made, or went).
    Paths(Vec<PathBuf>),
    /// Something changed, it is not known where (too many changes at once).
    Lost,
    /// Whether every folder asked for is watched now.
    Watching(bool),
}

/// The handle: dropping it ends the thread.
pub struct Watcher {
    tx: Option<Sender<Vec<PathBuf>>>,
    /// What was asked for last.
    sent: Vec<PathBuf>,
}

impl Watcher {
    /// Starts the watcher's thread; what it sees goes to `on` (called from
    /// that thread, so wake the UI there rather than touching it).
    pub fn start(on: impl Fn(Event) + Send + Sync + 'static) -> Watcher {
        let (tx, rx) = mpsc::channel::<Vec<PathBuf>>();
        let spawned = std::thread::Builder::new().name("ito-watch".into()).spawn(move || run(&rx, on));
        Watcher { tx: spawned.is_ok().then_some(tx), sent: Vec::new() }
    }

    /// Whether the thread is running (it is not when it could not be
    /// started, or when the notify backend failed to open).
    pub fn is_running(&self) -> bool {
        self.tx.is_some()
    }

    /// Watch exactly `roots`, each with everything below it. Returns whether
    /// that is new to the watcher (it says [`Event::Watching`] once done).
    pub fn watch(&mut self, roots: Vec<PathBuf>) -> bool {
        let Some(tx) = &self.tx else { return false };
        if roots == self.sent {
            return false;
        }
        if tx.send(roots.clone()).is_err() {
            self.tx = None;
            return false;
        }
        self.sent = roots;
        true
    }
}

/// The watcher's thread, until the [`Watcher`] is dropped.
fn run(rx: &mpsc::Receiver<Vec<PathBuf>>, on: impl Fn(Event) + Send + Sync + 'static) {
    let on = Arc::new(on);
    let tell = on.clone();
    let made = notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
        Ok(ev) if matches!(ev.kind, notify::EventKind::Access(_)) => {}
        Ok(ev) if ev.need_rescan() || ev.paths.is_empty() => tell(Event::Lost),
        Ok(ev) => tell(Event::Paths(ev.paths)),
        Err(_) => tell(Event::Lost),
    });
    let mut inner = match made {
        Ok(w) => w,
        Err(_) => {
            on(Event::Watching(false));
            return;
        }
    };
    let mut watched: Vec<PathBuf> = Vec::new();
    while let Ok(mut wanted) = rx.recv() {
        while let Ok(newer) = rx.try_recv() {
            wanted = newer;
        }
        for gone in watched.iter().filter(|p| !wanted.contains(p)) {
            let _ = inner.unwatch(gone);
        }
        watched.retain(|p| wanted.contains(p));
        let mut all = true;
        for p in &wanted {
            if watched.contains(p) {
                continue;
            }
            if on_share(p) {
                all = false;
                continue;
            }
            match inner.watch(p, RecursiveMode::Recursive) {
                Ok(()) => watched.push(p.clone()),
                Err(_) => all = false,
            }
        }
        on(Event::Watching(all));
    }
}

/// Whether `p` is on a network share, where a change may never be told.
pub fn on_share(p: &Path) -> bool {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/UNC/").map(|r| format!("//{r}")).unwrap_or(s);
    s.starts_with("//") && !s.starts_with("//?/") && !s.starts_with("//./")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    #[test]
    fn shares_are_told_from_local_folders() {
        assert!(on_share(Path::new(r"\\server\share\repo")));
        assert!(on_share(Path::new("//server/share/repo")));
        assert!(on_share(Path::new(r"\\?\UNC\server\share\repo")));
        assert!(!on_share(Path::new(r"\\?\C:\dev\repo")));
        assert!(!on_share(Path::new("C:/dev/repo")));
        assert!(!on_share(Path::new("/home/u/repo")));
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Waits for an event that `want` takes, up to a few seconds.
    fn wait_for(rx: &mpsc::Receiver<Event>, mut want: impl FnMut(&Event) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(10);
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(e) if want(&e) => return true,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
        false
    }

    #[test]
    fn a_file_made_below_a_watched_folder_is_told() {
        let dir = temp("make");
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let mut w = Watcher::start(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        assert!(w.is_running());
        assert!(w.watch(vec![dir.clone()]));
        assert!(!w.watch(vec![dir.clone()]), "the same wish is not sent again");
        assert!(wait_for(&rx, |e| matches!(e, Event::Watching(true))), "the folder is watched");
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("a.txt"), "x").unwrap();
        assert!(wait_for(&rx, |e| matches!(e, Event::Paths(p) if p.iter().any(|p| p.ends_with("a.txt")))), "the file is told");
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_is_not_there_says_it_is_not_watched() {
        let dir = temp("gone");
        let missing = dir.join("nope");
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let mut w = Watcher::start(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        w.watch(vec![missing]);
        assert!(wait_for(&rx, |e| matches!(e, Event::Watching(false))));
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
