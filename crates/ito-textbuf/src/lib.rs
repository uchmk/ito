//! A document an editor holds and edits itself (not `egui::TextEdit`): the text, one selection, and undo / redo where
//! one user action is one undo step. No UI, no clock, no file: testable on its own.
//!
//! - [`Buffer`]: the text as read, a version that goes up on each edit, and the line endings and BOM it came with
//!   (so saving writes them back). One contiguous `String`, so [`Buffer::text`] is a free `&str` for `ito-mdedit`;
//!   inserting in the middle of an 8 MiB text is one memmove.
//! - [`pos`]: byte <-> char <-> grapheme <-> line / column, the only place that converts.
//! - [`Selection`]: a caret and its anchor.
//! - [`Doc`]: buffer + selection + history. [`Transaction`] is a list of [`Edit`]s with the selection before and
//!   after and optional [`Effect`]s (a pasted image's file) that undo and redo with the text.

use std::fmt;
use std::ops::Range;

mod buffer;
mod doc;
pub mod pos;

pub use buffer::{Buffer, Eol};
pub use doc::{Doc, Effect, Transaction};

/// Replace `range` of the text with `text`. Byte offsets on `char` boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
}

impl From<&ito_mdedit::Change> for Edit {
    fn from(c: &ito_mdedit::Change) -> Edit {
        Edit { range: c.range.clone(), text: c.text.clone() }
    }
}

/// A caret (`head`) and the other end of the selection (`anchor`); equal when nothing is selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Selection {
        Selection { anchor: at, head: at }
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
    /// The selected bytes, low to high.
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// An edit's range is outside the text, backwards, or not on `char` boundaries. Nothing was changed.
    Range,
    /// An [`Effect`] failed. The document is as it was before the call.
    Effect(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Range => f.write_str("edit range is outside the text"),
            Error::Effect(e) => write!(f, "side effect failed: {e}"),
        }
    }
}

impl std::error::Error for Error {}
