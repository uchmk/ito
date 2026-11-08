//! The copy record: what `--keys` (`<Clipboard:>`, `<ClipboardHas:>`) reads for copies an app makes by its own path.
//!
//! Process-wide, and a no-op until [`start`] (which `ito_keyscript::observe` calls), so a shipped app keeps nothing.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// Copies the app gave to [`crate::note_copy`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Noted {
    pub text: Option<String>,
    pub count: u64,
    pub stamp: u64,
}

/// Set by [`start`]; until then nothing is kept.
static NOTING: AtomicBool = AtomicBool::new(false);
static NOTED: Mutex<Noted> = Mutex::new(Noted { text: None, count: 0, stamp: 0 });
/// Every recorded copy, from the `--keys` tap or from [`note_when`], takes the next number, so the two kinds can be ordered.
static COPY_STAMP: AtomicU64 = AtomicU64::new(0);

/// Start recording.
pub fn start() {
    NOTING.store(true, Ordering::SeqCst);
}

pub(crate) fn started() -> bool {
    NOTING.load(Ordering::SeqCst)
}

/// The next stamp. The `--keys` tap stamps egui copies from here, and [`note_when`] stamps noted copies from the same source.
pub fn next_stamp() -> u64 {
    COPY_STAMP.fetch_add(1, Ordering::SeqCst) + 1
}

/// A copy of the record as of now.
pub fn snapshot() -> Noted {
    NOTED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// [`crate::note_copy`] with the switch passed in, so a test does not race other tests that call [`start`].
pub fn note_when(noting: bool, text: &str) {
    if !noting {
        return;
    }
    let mut noted = NOTED.lock().unwrap_or_else(|e| e.into_inner());
    noted.text = Some(text.to_owned());
    noted.count += 1;
    noted.stamp = next_stamp();
}

/// Forget the record (not the switch). For tests that take turns.
pub fn reset() {
    *NOTED.lock().unwrap_or_else(|e| e.into_inner()) = Noted::default();
}
