//! Copy text to the clipboard the same way in every app: one writer, CRLF on Windows, a fake under test, and the copy
//! record `--keys` reads.
//!
//! ```ignore
//! ito_clipboard::clipboard!().set_text("hello\nworld")?;
//! ```
//!
//! A crate's own unit tests never touch the machine's clipboard; integration tests and other builds enter a [`Fake`]
//! to stay off it: see [`clipboard!`].

mod fake;
#[doc(hidden)]
pub mod record;

pub use fake::{Entered, Fake};

use std::borrow::Cow;
use std::fmt;
use std::sync::Mutex;

/// Why a clipboard call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// Another program held the clipboard (arboard has already tried a few times).
    Occupied,
    /// No clipboard here: no display/session, or built without the `os` feature. The text says which.
    NotSupported(String),
    /// The text could not be handed to or read from the OS.
    Conversion,
    /// Anything else, with arboard's description.
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Occupied => write!(f, "clipboard: another program is using it"),
            Error::NotSupported(why) => write!(f, "clipboard: not available: {why}"),
            Error::Conversion => write!(f, "clipboard: the text could not be converted"),
            Error::Other(why) => write!(f, "clipboard: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// Record that the app copied `text` by its own path (not through egui's `CopyText`, e.g. straight to the OS clipboard),
/// so `<Clipboard:>` and `<ClipboardHas:>` see it like any other copy. Pass the text as the app means it, before any
/// change of line endings for the OS. Callable from any thread, with no `egui::Context`. Records only after
/// [`record::start`], which `ito_keyscript::observe` calls (a `--keys` run, or an app's own loop that calls it);
/// otherwise it does nothing, so a shipped app keeps nothing. The record is process-wide.
pub fn note_copy(text: &str) {
    record::note_when(record::started(), text);
}

/// `text` as the OS clipboard should hold it: on Windows every bare `\n` becomes `\r\n` (an existing `\r\n` stays one);
/// elsewhere every `\r\n` becomes `\n`. Borrowed when nothing changes. A lone `\r` is left alone.
pub fn line_endings(text: &str, windows: bool) -> Cow<'_, str> {
    if windows {
        if !text.contains('\n') {
            return Cow::Borrowed(text);
        }
        let mut out = String::with_capacity(text.len() + text.len() / 16);
        let mut prev = '\0';
        for c in text.chars() {
            if c == '\n' && prev != '\r' {
                out.push('\r');
            }
            out.push(c);
            prev = c;
        }
        Cow::Owned(out)
    } else if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// The clipboard for this call site: a [`Fake`] entered on this thread if there is one; else, in a crate's own unit-test
/// build (`cfg(test)` where the macro expands), this thread's own fake; else the OS clipboard.
///
/// A [`Clipboard`] keeps the backend it was picked with: pick it where the work starts and give the handle to the
/// worker. A worker that calls `clipboard!()` itself under test gets its own thread's fake.
#[macro_export]
macro_rules! clipboard {
    () => {
        $crate::__pick(cfg!(test))
    };
}

#[doc(hidden)]
pub fn __pick(test: bool) -> Clipboard {
    if let Some(f) = fake::entered_top() {
        return Clipboard(Backend::Fake(f));
    }
    if test {
        return Clipboard(Backend::Fake(fake::implicit()));
    }
    Clipboard(Backend::Os)
}

#[derive(Clone)]
enum Backend {
    Os,
    Fake(Fake),
}

/// A handle to the clipboard picked by [`clipboard!`]. Cheap to clone; can go to a worker thread.
#[derive(Clone)]
pub struct Clipboard(Backend);

/// What this app last wrote to the OS clipboard, as given.
static LAST_SET: Mutex<Option<String>> = Mutex::new(None);

impl Clipboard {
    /// Write `text` (as the app means it, with `\n`). OS: one long-lived handle, `line_endings(text, cfg!(windows))`.
    /// On success, and only then: remember it as `last_set` and give it to [`note_copy`] (the original `text`).
    /// Blocks briefly (the OS clipboard can be held by another program); callable from the UI thread.
    pub fn set_text(&self, text: &str) -> Result<(), Error> {
        match &self.0 {
            Backend::Os => os::set(text),
            Backend::Fake(f) => f.write(text),
        }
    }

    /// What is on the clipboard as text; `Ok(None)` when it is empty or holds no text.
    /// Can block for seconds on X11: call from a worker thread, never from the UI thread.
    pub fn get_text(&self) -> Result<Option<String>, Error> {
        let text = match &self.0 {
            Backend::Os => os::get()?,
            Backend::Fake(f) => f.contents(),
        };
        Ok(text.filter(|t| !t.is_empty()))
    }

    /// The last text this app wrote through this backend with `set_text` (OS: process-wide; fake: that fake's), as it was given.
    /// Never what another program put there, and never `get_text`'s staged input.
    pub fn last_set(&self) -> Option<String> {
        match &self.0 {
            Backend::Os => LAST_SET.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            Backend::Fake(f) => f.last_set(),
        }
    }

    /// Whether this is a [`Fake`].
    pub fn is_fake(&self) -> bool {
        matches!(self.0, Backend::Fake(_))
    }
}

/// Drop the OS clipboard handle.
///
/// Call once when the app exits, after workers that copy have stopped. On X11 the app itself holds what it copied;
/// arboard hands it to a clipboard manager only when the last handle drops (egui's own handle too). Without a clipboard
/// manager, a copy does not outlive the app on X11. A later `set_text` makes a new handle.
pub fn release() {
    os::release();
}

#[cfg(feature = "os")]
mod os {
    use super::{note_copy, Error, LAST_SET};
    use std::sync::Mutex;

    /// Lock order: OS, then LAST_SET, then the record's. Nothing takes them in another order; no callbacks run under them.
    static OS: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

    fn map(e: arboard::Error) -> Error {
        match e {
            arboard::Error::ClipboardOccupied => Error::Occupied,
            arboard::Error::ClipboardNotSupported => Error::NotSupported(e.to_string()),
            arboard::Error::ConversionFailure => Error::Conversion,
            e => Error::Other(e.to_string()),
        }
    }

    pub fn set(text: &str) -> Result<(), Error> {
        let mut guard = OS.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(arboard::Clipboard::new().map_err(map)?);
        }
        let cb = guard.as_mut().expect("just made");
        match cb.set_text(super::line_endings(text, cfg!(windows)).into_owned()) {
            Ok(()) => {
                *LAST_SET.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.to_owned());
                note_copy(text);
                Ok(())
            }
            Err(e) => {
                let err = map(e);
                if matches!(err, Error::Other(_) | Error::NotSupported(_)) {
                    *guard = None;
                }
                Err(err)
            }
        }
    }

    pub fn get() -> Result<Option<String>, Error> {
        // Its own short-lived handle: a slow read never holds up a write, and never drops the writer's handle.
        let mut cb = arboard::Clipboard::new().map_err(map)?;
        match cb.get_text() {
            Ok(t) => Ok(Some(t)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(map(e)),
        }
    }

    pub fn release() {
        *OS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[cfg(not(feature = "os"))]
mod os {
    use super::Error;

    fn off() -> Error {
        Error::NotSupported("built without the `os` feature of ito-clipboard".into())
    }

    pub fn set(_: &str) -> Result<(), Error> {
        Err(off())
    }

    pub fn get() -> Result<Option<String>, Error> {
        Err(off())
    }

    pub fn release() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::sync::{Mutex, MutexGuard};

    /// Tests that touch the process-wide record take turns.
    static RECORD: Mutex<()> = Mutex::new(());

    fn turn() -> MutexGuard<'static, ()> {
        let g = RECORD.lock().unwrap_or_else(|e| e.into_inner());
        record::reset();
        g
    }

    #[test]
    fn line_endings_for_windows() {
        assert_eq!(line_endings("a\nb\n", true), "a\r\nb\r\n");
        assert_eq!(line_endings("a\r\nb", true), "a\r\nb");
        assert_eq!(line_endings("a\r\nb\nc", true), "a\r\nb\r\nc");
        assert!(matches!(line_endings("abc", true), Cow::Borrowed(_)));
    }

    #[test]
    fn line_endings_elsewhere() {
        assert_eq!(line_endings("a\r\nb\r\n", false), "a\nb\n");
        assert!(matches!(line_endings("a\nb", false), Cow::Borrowed(_)));
        assert_eq!(line_endings("a\rb", false), "a\rb");
        assert!(matches!(line_endings("a\rb", false), Cow::Borrowed(_)));
    }

    #[test]
    fn test_build_picks_this_threads_fake() {
        let c = __pick(true);
        assert!(c.is_fake());
        c.set_text("here").unwrap();
        let other = std::thread::spawn(|| __pick(true).last_set()).join().unwrap();
        assert_eq!(other, None);
        assert_eq!(__pick(true).last_set().as_deref(), Some("here"));
    }

    #[test]
    fn entered_fake_wins_in_any_build_and_nests() {
        let outer = Fake::new();
        let inner = Fake::new();
        {
            let _o = outer.enter();
            assert!(__pick(false).is_fake());
            __pick(false).set_text("o").unwrap();
            {
                let _i = inner.enter();
                __pick(false).set_text("i").unwrap();
            }
            __pick(false).set_text("o2").unwrap();
        }
        assert_eq!(outer.writes(), ["o", "o2"]);
        assert_eq!(inner.writes(), ["i"]);
        assert!(!__pick(false).is_fake());
    }

    #[test]
    fn a_handle_follows_the_work_to_a_worker() {
        let fake = Fake::new();
        let _in = fake.enter();
        let handle = __pick(false);
        std::thread::spawn(move || handle.set_text("from worker").unwrap()).join().unwrap();
        assert_eq!(fake.last_set().as_deref(), Some("from worker"));
    }

    #[test]
    fn put_stages_input_without_a_write() {
        let fake = Fake::new();
        let c = Clipboard(Backend::Fake(fake.clone()));
        assert_eq!(c.get_text(), Ok(None));
        fake.put(Some("theirs"));
        assert_eq!(c.get_text(), Ok(Some("theirs".into())));
        assert_eq!(c.last_set(), None);
        assert!(fake.writes().is_empty());
        c.set_text("mine").unwrap();
        assert_eq!(c.get_text(), Ok(Some("mine".into())));
        fake.put(None);
        assert_eq!(c.get_text(), Ok(None));
        assert_eq!(c.last_set().as_deref(), Some("mine"));
        fake.put(Some(""));
        assert_eq!(c.get_text(), Ok(None));
        c.set_text("").unwrap();
        assert_eq!(c.get_text(), Ok(None));
        assert_eq!(c.last_set().as_deref(), Some(""));
    }

    #[test]
    fn fail_next_write_fails_once() {
        let fake = Fake::new();
        let c = Clipboard(Backend::Fake(fake.clone()));
        fake.fail_next_write(Error::Occupied);
        assert_eq!(c.set_text("a"), Err(Error::Occupied));
        assert!(fake.writes().is_empty());
        assert_eq!(c.set_text("b"), Ok(()));
        assert_eq!(fake.writes(), ["b"]);
    }

    #[test]
    fn only_a_noting_fake_reaches_the_record() {
        let _g = turn();
        record::start();
        let quiet = Fake::new();
        Clipboard(Backend::Fake(quiet)).set_text("quiet").unwrap();
        assert_eq!(record::snapshot().count, 0);
        let loud = Fake::new().noting();
        let c = Clipboard(Backend::Fake(loud));
        c.set_text("one").unwrap();
        let first = record::snapshot();
        assert_eq!((first.count, first.text.as_deref()), (1, Some("one")));
        c.set_text("two").unwrap();
        let second = record::snapshot();
        assert_eq!(second.count, 2);
        assert!(second.stamp > first.stamp);
    }

    #[test]
    fn record_is_off_until_started_and_stamps_increase() {
        let _g = turn();
        record::note_when(false, "off");
        assert_eq!(record::snapshot().count, 0);
        let a = record::next_stamp();
        record::note_when(true, "x");
        let s = record::snapshot().stamp;
        let b = record::next_stamp();
        assert!(a < s && s < b);
    }

    #[test]
    fn types_are_usable_across_threads() {
        fn send_sync<T: Send + Sync>() {}
        fn is_error<T: std::error::Error>() {}
        send_sync::<Clipboard>();
        send_sync::<Fake>();
        is_error::<Error>();
        assert_eq!(Error::Occupied.to_string(), "clipboard: another program is using it");
    }
}
