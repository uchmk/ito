//! A stand-in for the clipboard, so a `--keys` test can paste a picture without a hand.
//!
//! [`clipboard_image`](crate::clipboard_image) asks here first:
//!
//! 1. a picture set with [`set`] (in the process; an app's `--keys` dialect can call it);
//! 2. the PNG file named by the environment variable `ITO_CLIPBOARD_IMAGE`
//!    (so a test lane can start the app with it set and send `<C-v>`). A path that
//!    cannot be read as a PNG is an [`Error`], not an empty clipboard, so a typo
//!    in the test fails loudly.
//!
//! With neither, the real clipboard is read.

use crate::{decode_png, Error, Rgba};
use std::sync::Mutex;

/// The name of the environment variable.
pub const ENV: &str = "ITO_CLIPBOARD_IMAGE";

static SLOT: Mutex<Option<Slot>> = Mutex::new(None);

#[derive(Clone)]
enum Slot {
    Image(Rgba),
    Empty,
}

/// Puts `image` on the fake clipboard; `None` makes the fake clipboard empty
/// (a paste finds no picture). Use [`clear`] to go back to the real one.
pub fn set(image: Option<Rgba>) {
    *SLOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(match image {
        Some(i) => Slot::Image(i),
        None => Slot::Empty,
    });
}

/// Puts a PNG file on the fake clipboard.
pub fn set_png_file(path: &std::path::Path) -> Result<(), Error> {
    set(Some(read_png(path)?));
    Ok(())
}

/// Goes back to the real clipboard (and the environment variable).
pub fn clear() {
    *SLOT.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn read_png(path: &std::path::Path) -> Result<Rgba, Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::Clipboard(format!("{}: {e}", path.display())))?;
    decode_png(&bytes)
}

/// `Some` when the fake clipboard answers.
pub(crate) fn get() -> Option<Result<Option<Rgba>, Error>> {
    match SLOT.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        Some(Slot::Image(i)) => return Some(Ok(Some(i))),
        Some(Slot::Empty) => return Some(Ok(None)),
        None => {}
    }
    let path = std::env::var_os(ENV).filter(|p| !p.is_empty())?;
    Some(read_png(std::path::Path::new(&path)).map(Some))
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test: the slot is process-wide.
    #[test]
    fn slot_answers_before_the_os() {
        let img = Rgba::new(1, 1, vec![1, 2, 3, 4]).unwrap();
        set(Some(img.clone()));
        assert_eq!(crate::clipboard_image().unwrap(), Some(img));
        set(None);
        assert_eq!(crate::clipboard_image().unwrap(), None);
        clear();
        assert!(set_png_file(std::path::Path::new("no/such/file.png")).is_err());
    }
}
