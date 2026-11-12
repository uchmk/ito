//! What an app keeps between runs in a file of its state folder
//! ([`crate::state_dir`]): `state.toml`, a session, a list of recent folders.
//! Reading one is [`read`]; writing one is a [`Writer`], which does it on a
//! thread of its own so the UI never waits on the disk, lets the newest save
//! win when two are in flight, and writes nothing at all for a run a script
//! drives (`--keys`). [`copy_old`] brings a file from where an older build
//! kept it.
//!
//! One [`Writer`] is for one file: the order is kept per writer.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// The value kept in `path`, or `None` when the file is not there or does not
/// read (a state file is never worth stopping for).
pub fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Copy what an older build kept at `old` to `new`, once: only when `old` is
/// a file and `new` is not, so nothing is written over. `Ok(true)` when it
/// copied; `old` stays (an older build still running keeps reading it, and
/// the owner can delete it). For a move, [`crate::move_old`].
pub fn copy_old(old: &Path, new: &Path) -> Result<bool, String> {
    if !old.is_file() || new.exists() {
        return Ok(false);
    }
    if let Some(dir) = new.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::copy(old, new).map_err(|e| format!("could not copy {} to {}: {e}", old.display(), new.display()))?;
    Ok(true)
}

/// The waits before each new try of a save that failed.
const SAVE_RETRY_WAITS: [Duration; 3] = [Duration::from_millis(250), Duration::from_secs(1), Duration::from_secs(4)];

/// Writes one file, newest save first. Cloning shares the order, so the
/// clones are one writer.
#[derive(Clone, Debug)]
pub struct Writer {
    path: PathBuf,
    on: bool,
    /// The number the next save takes.
    next: Arc<AtomicU64>,
    /// The number of the newest save that is on the disk (a failed one does
    /// not count). Held while writing, so two saves never write at once and
    /// a late older one finds a newer one done and gives up.
    done: Arc<Mutex<u64>>,
}

impl Writer {
    /// A writer of `path`. `on` is false for a run that keeps nothing (the
    /// `--keys` script, a window torn off another): every save is then a no-op.
    pub fn new(path: impl Into<PathBuf>, on: bool) -> Self {
        Self { path: path.into(), on, next: Arc::new(AtomicU64::new(1)), done: Arc::new(Mutex::new(0)) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write `text` on a thread of its own. If a save made after this one is
    /// written first, this one is dropped. A save that cannot be written is
    /// tried three more times, after waits of 0.25 s, 1 s and 4 s, unless a
    /// newer one is written first. So the file ends as the last call made it,
    /// or, if that one could not be written in time (or the app ended first),
    /// as the newest one that was.
    pub fn save(&self, text: String) {
        if !self.on {
            return;
        }
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let w = self.clone();
        let _ = std::thread::Builder::new().name("ito-kept".into()).spawn(move || {
            let _ = w.write_retrying(n, &text, &mut |attempt| match SAVE_RETRY_WAITS.get(attempt) {
                Some(d) => {
                    std::thread::sleep(*d);
                    true
                }
                None => false,
            });
        });
    }

    /// [`save`](Self::save) without the thread, for the app closing: the
    /// file is written when this returns. A failure is returned and not tried
    /// again here.
    pub fn save_now(&self, text: String) -> Result<(), String> {
        if !self.on {
            return Ok(());
        }
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        self.write(n, &text)
    }

    /// `value` as TOML, [`save`](Self::save)d. A value that does not
    /// serialize is not written.
    pub fn save_toml(&self, value: &impl Serialize) {
        if let Ok(text) = toml::to_string(value) {
            self.save(text);
        }
    }

    fn write(&self, n: u64, text: &str) -> Result<(), String> {
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        if n < *done {
            return Ok(());
        }
        crate::write_atomic(&self.path, text)?;
        *done = n;
        Ok(())
    }

    /// [`write`](Self::write), and again after each `wait(attempt)` (0, 1,
    /// ...) that returns true, until it is `Ok` (which includes a newer save
    /// being on the disk) or `wait` returns false. The lock is not held while
    /// waiting.
    fn write_retrying(&self, n: u64, text: &str, wait: &mut dyn FnMut(usize) -> bool) -> Result<(), String> {
        let mut attempt = 0;
        loop {
            match self.write(n, text) {
                Err(_) if wait(attempt) => attempt += 1,
                done => return done,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-kept-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    struct S {
        n: u32,
    }

    #[test]
    fn reads_what_was_saved_and_nothing_for_a_missing_or_bad_file() {
        let d = dir("read");
        let w = Writer::new(d.join("sub").join("state.toml"), true);
        assert_eq!(read::<S>(w.path()), None);
        w.save_now("n = 3\n".into()).unwrap();
        assert_eq!(read::<S>(w.path()), Some(S { n: 3 }));
        w.save_now("not toml {".into()).unwrap();
        assert_eq!(read::<S>(w.path()), None);
    }

    #[test]
    fn the_newest_save_wins_and_an_off_writer_writes_nothing() {
        let d = dir("order");
        let w = Writer::new(d.join("state.toml"), true);
        // A save that was numbered first but arrives after a newer one.
        let (old, new) = (w.next.fetch_add(1, Ordering::SeqCst), w.next.fetch_add(1, Ordering::SeqCst));
        w.write(new, "n = 2\n").unwrap();
        w.write(old, "n = 1\n").unwrap();
        assert_eq!(read::<S>(w.path()), Some(S { n: 2 }));
        // Many threads: the last call is what stays once they are done.
        for i in 0..50 {
            w.save_toml(&S { n: 100 + i });
        }
        w.save_now("n = 999\n".into()).unwrap();
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(read::<S>(w.path()), Some(S { n: 999 }));
        let off = Writer::new(d.join("off.toml"), false);
        off.save("n = 1\n".into());
        assert!(off.save_now("n = 1\n".into()).is_ok());
        assert!(!off.path().exists());
    }

    /// A writer whose folder is a file, so every write fails until `fix`.
    fn blocked(name: &str) -> (Writer, PathBuf) {
        let d = dir(name);
        std::fs::create_dir_all(&d).unwrap();
        let b = d.join("blocker");
        std::fs::write(&b, "x").unwrap();
        (Writer::new(b.join("state.toml"), true), b)
    }

    fn fix(b: &Path) {
        let _ = std::fs::remove_file(b);
        let _ = std::fs::create_dir_all(b);
    }

    #[test]
    fn a_failed_save_leaves_the_order_open() {
        let (w, b) = blocked("failed-order");
        let (old, new) = (w.next.fetch_add(1, Ordering::SeqCst), w.next.fetch_add(1, Ordering::SeqCst));
        assert!(w.write(new, "n = 2\n").is_err());
        assert_eq!(*w.done.lock().unwrap(), 0);
        fix(&b);
        w.write(old, "n = 1\n").unwrap();
        assert_eq!(read::<S>(w.path()), Some(S { n: 1 }));
        w.write(new, "n = 2\n").unwrap();
        assert_eq!(read::<S>(w.path()), Some(S { n: 2 }));
        w.write(old, "n = 1\n").unwrap();
        assert_eq!(read::<S>(w.path()), Some(S { n: 2 }));
    }

    #[test]
    fn a_save_that_fails_is_tried_again() {
        let (w, b) = blocked("retry");
        let n = w.next.fetch_add(1, Ordering::SeqCst);
        let mut calls = 0;
        let r = w.write_retrying(n, "n = 7\n", &mut |i| {
            calls += 1;
            fix(&b);
            i < 3
        });
        assert_eq!(r, Ok(()));
        assert_eq!(calls, 1);
        assert_eq!(read::<S>(w.path()), Some(S { n: 7 }));
    }

    #[test]
    fn a_retry_gives_way_to_a_newer_save() {
        let (w, b) = blocked("retry-newer");
        let (old, new) = (w.next.fetch_add(1, Ordering::SeqCst), w.next.fetch_add(1, Ordering::SeqCst));
        let mut calls = 0;
        let r = w.write_retrying(old, "n = 1\n", &mut |_| {
            calls += 1;
            if calls == 1 {
                fix(&b);
                w.write(new, "n = 2\n").unwrap();
            }
            true
        });
        assert_eq!(r, Ok(()));
        assert_eq!(read::<S>(w.path()), Some(S { n: 2 }));
    }

    #[test]
    fn a_retry_gives_up_when_the_waits_run_out() {
        let (w, _b) = blocked("retry-out");
        let n = w.next.fetch_add(1, Ordering::SeqCst);
        let mut calls = 0;
        let r = w.write_retrying(n, "n = 1\n", &mut |_| {
            calls += 1;
            calls < 3
        });
        assert!(r.is_err());
        assert_eq!(calls, 3);
        assert_eq!(*w.done.lock().unwrap(), 0);
    }

    #[test]
    fn copies_once_and_leaves_the_old_file() {
        let d = dir("copy");
        std::fs::create_dir_all(&d).unwrap();
        let (old, new) = (d.join("old.toml"), d.join("new").join("state.toml"));
        assert_eq!(copy_old(&old, &new), Ok(false), "nothing there");
        std::fs::write(&old, "n = 1\n").unwrap();
        assert_eq!(copy_old(&old, &new), Ok(true));
        assert!(old.exists());
        std::fs::write(&old, "n = 2\n").unwrap();
        assert_eq!(copy_old(&old, &new), Ok(false), "both there: the new one is kept");
        assert_eq!(read::<S>(&new), Some(S { n: 1 }));
    }
}
