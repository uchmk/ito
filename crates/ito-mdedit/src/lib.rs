//! Structure commands for a Markdown text, as pure functions: the text and the caret's byte go in, a [`Change`] (the
//! range to replace, what to put there, where the caret goes) comes out, or `None` when the command does not apply
//! there. An editor applies the change as one step of its undo history; nothing here knows a buffer, a window or a
//! key.
//!
//! - blocks and sections: [`move_block`], [`move_section`], [`promote_section`], [`demote_section`]
//! - lists: [`indent_item`], [`outdent_item`], [`toggle_task`], [`renumber`]
//! - tables: [`format_table`], [`next_cell`]
//!
//! All positions are byte offsets on `char` boundaries of the given text.

use std::ops::Range;

mod blocks;
mod list;
mod table;

pub use blocks::{demote_section, move_block, move_section, promote_section, Dir};
pub use list::{indent_item, outdent_item, renumber, toggle_task};
pub use table::{format_table, next_cell, Step};

/// One edit: replace `range` of the old text with `text`, and put the caret at `cursor` of the new text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub range: Range<usize>,
    pub text: String,
    pub cursor: usize,
}

impl Change {
    /// The text with the change applied.
    pub fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + self.text.len());
        out.push_str(&text[..self.range.start]);
        out.push_str(&self.text);
        out.push_str(&text[self.range.end..]);
        out
    }
}

pub(crate) fn trim_end(text: &str, r: Range<usize>) -> Range<usize> {
    r.start..r.start + text[r.clone()].trim_end_matches(['\n', '\r']).len()
}

/// The byte ranges of the lines of `text`, without their line breaks (the `\r` of CRLF is left out too).
pub(crate) fn lines(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            let end = if i > start && text.as_bytes()[i - 1] == b'\r' { i - 1 } else { i };
            out.push(start..end);
            start = i + 1;
        }
    }
    if start < text.len() {
        out.push(start..text.len());
    }
    out
}

/// The index of the line holding `byte` (a byte at the end of the text is on the last line).
pub(crate) fn line_of(lines: &[Range<usize>], byte: usize) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    Some(lines.partition_point(|l| l.start <= byte).saturating_sub(1))
}
