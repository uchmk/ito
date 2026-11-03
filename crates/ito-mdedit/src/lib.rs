//! Structure commands for a Markdown text, as pure functions: the text and the caret's byte go in, a [`Change`] (the
//! range to replace, what to put there, where the caret goes) comes out, or `None` when the command does not apply
//! there. An editor applies the change as one step of its undo history; nothing here knows a buffer, a window or a
//! key.
//!
//! - blocks and sections: [`move_block`], [`move_section`], [`promote_section`], [`demote_section`]
//! - lists: [`move_item`], [`indent_item`], [`outdent_item`], [`toggle_task`], [`renumber`], [`shift_lines`] (Tab and
//!   Shift+Tab over a selection: items, other lines, fences)
//! - tables: [`format_table`], [`next_cell`]
//! - table of contents: [`insert_toc`] (insert, or rebuild the one the document has)
//! - structural selection: [`expand_selection`], [`shrink_selection`]
//! - markup on a selection (a [`SelChange`]: the change and the selection after it): [`inline`] (bold, italic, code,
//!   strike, link; a toggle), [`line_type`] (heading, quote, fence, bullet, number, task), [`set_tasks`]
//! - heading deletes that keep the structure: [`delete_heading`]
//! - navigation (a byte, no change): [`next`]
//!
//! All positions are byte offsets on `char` boundaries of the given text.

use std::ops::Range;

mod blocks;
mod format;
mod heading;
mod list;
mod nav;
mod select;
mod table;
mod toc;

pub use blocks::{demote_section, move_block, move_section, promote_section, Dir};
pub use format::{check_tasks, inline, line_type, set_tasks, toggle_tasks, uncheck_tasks, Inline, Line, SelChange};
pub use heading::{delete_heading, HeadingDelete};
pub use list::{indent_item, move_item, outdent_item, renumber, shift_lines, toggle_task, Unit};
pub use nav::{next, Target};
pub use select::{expand_selection, shrink_selection};
pub use table::{format_table, next_cell, Step};
pub use toc::insert_toc;

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

/// Several non-overlapping edits (sorted, in old-text bytes) as one [`Change`] spanning them all. The caret keeps its
/// place in the text around the edits; inside an edited range it goes to the start of that edit.
pub(crate) fn change_from_edits(text: &str, edits: &[(Range<usize>, String)], caret: usize) -> Option<Change> {
    let (first, last) = (edits.first()?, edits.last()?);
    let span = first.0.start..last.0.end;
    let mut new = String::new();
    let mut at = span.start;
    let (mut delta, mut cursor) = (0isize, None);
    for (r, s) in edits {
        new.push_str(&text[at..r.start]);
        new.push_str(s);
        at = r.end;
        if cursor.is_none() {
            if caret >= r.end {
                delta += s.len() as isize - (r.end - r.start) as isize;
            } else {
                cursor = Some(if caret >= r.start { (r.start as isize + delta) as usize } else { (caret as isize + delta) as usize });
            }
        }
    }
    let cursor = cursor.unwrap_or_else(|| (caret as isize + delta) as usize);
    Some(Change { range: span, text: new, cursor })
}

/// The end of the line break at `pos` (CRLF or LF), or `pos` when there is none.
pub(crate) fn after_break(text: &str, pos: usize) -> usize {
    let rest = &text[pos..];
    if rest.starts_with("\r\n") {
        pos + 2
    } else if rest.starts_with('\n') {
        pos + 1
    } else {
        pos
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
