//! Event-driven watching of the few directories a view shows (feature
//! `dirwatch`). Changes come back as "this directory is dirty"; the app
//! debounces and rescans.
//!
//! Registering a directory opens a handle to it (`ReadDirectoryChangesW` on
//! Windows), which can hang on a dead network share, so the `notify` watcher
//! lives on its own thread. The UI thread only posts the set of directories it
//! wants (newest wins); the thread applies the newest set it has been given.
//! Not recursive: a change in a subfolder dirties that subfolder only.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use notify::{RecursiveMode, Watcher as _};

pub struct DirWatcher {
    /// The wanted set, handed to the watcher thread.
    tx: Option<Sender<HashSet<PathBuf>>>,
    /// What was posted last, so a call every frame costs nothing.
    sent: HashSet<PathBuf>,
    /// The directories that changed (one entry per path of an event).
    pub rx: Receiver<PathBuf>,
}

impl DirWatcher {
    /// `wake` runs on the watcher thread after changes were queued on `rx`
    /// (to repaint the window).
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        let (ev_tx, ev_rx) = channel::<PathBuf>();
        let (cmd_tx, cmd_rx) = channel::<HashSet<PathBuf>>();
        let spawned = std::thread::Builder::new().name("dirwatch".into()).spawn(move || run(ev_tx, cmd_rx, wake)).is_ok();
        Self { tx: spawned.then_some(cmd_tx), sent: HashSet::new(), rx: ev_rx }
    }

    /// Ask for the watched set to become exactly `dirs`. Cheap to call every frame.
    pub fn sync<'a>(&mut self, dirs: impl IntoIterator<Item = &'a Path>) {
        let Some(tx) = self.tx.as_ref() else { return };
        let wanted: HashSet<PathBuf> = dirs.into_iter().map(Path::to_path_buf).collect();
        if wanted == self.sent {
            return;
        }
        if tx.send(wanted.clone()).is_ok() {
            self.sent = wanted;
        }
    }
}

/// The watcher thread: owns the `notify` watcher and every call that touches a directory handle.
fn run(ev_tx: Sender<PathBuf>, cmd_rx: Receiver<HashSet<PathBuf>>, wake: impl Fn() + Send + 'static) {
    let Ok(mut inner) = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        let mut sent = false;
        for p in ev.paths {
            let dir = if p.is_dir() { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or(p) };
            if ev_tx.send(dir).is_ok() {
                sent = true;
            }
        }
        if sent {
            wake();
        }
    }) else {
        return;
    };

    let mut watched: HashSet<PathBuf> = HashSet::new();
    while let Ok(mut wanted) = cmd_rx.recv() {
        // A slow `watch()` lets requests pile up; only the newest one matters.
        while let Ok(newer) = cmd_rx.try_recv() {
            wanted = newer;
        }
        let (drop, add) = plan(&watched, &wanted);
        for gone in drop {
            let _ = inner.unwatch(&gone);
            watched.remove(&gone);
        }
        for path in add {
            if inner.watch(&path, RecursiveMode::NonRecursive).is_ok() {
                watched.insert(path);
            }
        }
    }
}

/// Directories to stop watching, then ones to start watching.
fn plan(watched: &HashSet<PathBuf>, wanted: &HashSet<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    (watched.difference(wanted).cloned().collect(), wanted.difference(watched).cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn set<const N: usize>(paths: [&str; N]) -> HashSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn plan_only_moves_the_difference() {
        let (drop, add) = plan(&set(["a", "b"]), &set(["b", "c"]));
        assert_eq!(drop, vec![PathBuf::from("a")]);
        assert_eq!(add, vec![PathBuf::from("c")]);
        let (drop, add) = plan(&set(["a", "b"]), &set(["b", "a"]));
        assert!(drop.is_empty() && add.is_empty());
    }

    #[test]
    fn sync_posts_only_when_the_set_changes() {
        let (cmd_tx, cmd_rx) = channel::<HashSet<PathBuf>>();
        let (_ev_tx, ev_rx) = channel::<PathBuf>();
        let mut w = DirWatcher { tx: Some(cmd_tx), sent: HashSet::new(), rx: ev_rx };
        let (a, b) = (Path::new("a"), Path::new("b"));
        w.sync([a, b]);
        assert_eq!(cmd_rx.try_recv().unwrap(), set(["a", "b"]));
        w.sync([b, a]);
        assert!(cmd_rx.try_recv().is_err());
        w.sync([a]);
        assert_eq!(cmd_rx.try_recv().unwrap(), set(["a"]));
    }

    #[test]
    fn sync_keeps_trying_after_the_thread_is_gone() {
        let (cmd_tx, cmd_rx) = channel::<HashSet<PathBuf>>();
        let (_ev_tx, ev_rx) = channel::<PathBuf>();
        let mut w = DirWatcher { tx: Some(cmd_tx), sent: HashSet::new(), rx: ev_rx };
        drop(cmd_rx);
        w.sync([Path::new("a")]);
        assert!(w.sent.is_empty());
    }

    #[test]
    fn a_new_file_dirties_its_directory() {
        let dir = std::env::temp_dir().join(format!("ito-dirwatch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut w = DirWatcher::new(|| {});
        w.sync([dir.as_path()]);
        let mut got = false;
        // The thread registers the directory asynchronously: write until it hears.
        for i in 0..50 {
            std::fs::write(dir.join(format!("f{i}")), b"x").unwrap();
            if let Ok(p) = w.rx.recv_timeout(Duration::from_millis(100)) {
                got = p == dir;
                break;
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(got, "no event for the directory");
    }
}
