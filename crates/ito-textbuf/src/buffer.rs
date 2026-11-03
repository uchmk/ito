use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::{pos, Edit};

/// The line endings a text was read with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    /// No line break at all (a new one is up to the app).
    None,
    Lf,
    Crlf,
    /// Both. Edits keep what is there.
    Mixed,
}

/// The text as read, a version and what was around it. Edits work on the text as read: a CRLF text stays CRLF in
/// memory, and [`Buffer::saved`] gives back exactly what was read plus the edits.
#[derive(Clone, Debug)]
pub struct Buffer {
    text: String,
    version: u64,
    bom: bool,
    eol: Eol,
    /// The byte where each line starts (always begins with 0; one more entry per newline). Kept up to date by
    /// [`Buffer::replace`], the only way the text changes, so line lookups are a binary search.
    starts: Vec<usize>,
}

impl Buffer {
    /// Take `read` as read from a file. A leading BOM is remembered and taken out of the text.
    pub fn new(read: &str) -> Buffer {
        let (bom, text) = match read.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest),
            None => (false, read),
        };
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count() - crlf;
        let eol = match (crlf, lf) {
            (0, 0) => Eol::None,
            (0, _) => Eol::Lf,
            (_, 0) => Eol::Crlf,
            _ => Eol::Mixed,
        };
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Buffer { text: text.to_owned(), version: 0, bom, eol, starts }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn len(&self) -> usize {
        self.text.len()
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    /// Goes up by one on each edit (and each undo / redo), never back: a worker tags its result with it and drops it
    /// when the number has moved on.
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn bom(&self) -> bool {
        self.bom
    }
    /// The line endings it was read with (not updated by edits).
    pub fn eol(&self) -> Eol {
        self.eol
    }
    /// The line break to type for a new line: the one the text uses (`\n` when it has none or mixes).
    pub fn newline(&self) -> &'static str {
        if self.eol == Eol::Crlf {
            "\r\n"
        } else {
            "\n"
        }
    }

    /// How many lines the text has: one more than its `\n`s. O(1).
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }
    /// The line (from 0) holding `byte` (pulled back to a `char` boundary). O(log lines); same answer as [`pos::line_of`].
    pub fn line_of(&self, byte: usize) -> usize {
        let b = pos::floor(&self.text, byte);
        self.starts.partition_point(|&x| x <= b) - 1
    }
    /// The bytes of line `line` without its break (`\r\n` or `\n`); `None` past the last line. Same as [`pos::line_range`].
    pub fn line_range(&self, line: usize) -> Option<Range<usize>> {
        let start = *self.starts.get(line)?;
        let end = self.starts.get(line + 1).map_or(self.text.len(), |&n| n - 1);
        let end = if end > start && self.text.as_bytes()[end - 1] == b'\r' { end - 1 } else { end };
        Some(start..end)
    }
    /// `(line, column)` of `byte`, the column in graphemes. The line is a search; the column scans that one line.
    /// Same as [`pos::line_col`].
    pub fn line_col(&self, byte: usize) -> (usize, usize) {
        let b = pos::floor(&self.text, byte);
        let line = self.line_of(b);
        (line, self.text[self.starts[line]..b].graphemes(true).count())
    }
    /// The byte at `(line, column)`; a column past the end of the line gives its end, a line past the end gives the
    /// end of the text. Same as [`pos::byte_at`].
    pub fn byte_at(&self, line: usize, col: usize) -> usize {
        let Some(r) = self.line_range(line) else { return self.text.len() };
        let mut at = r.start;
        for _ in 0..col {
            match pos::next_grapheme(&self.text[..r.end], at) {
                Some(n) => at = n,
                None => break,
            }
        }
        at
    }

    /// What to write to the file: the BOM it had, then the text.
    pub fn saved(&self) -> String {
        let mut out = String::with_capacity(self.text.len() + 3);
        if self.bom {
            out.push('\u{feff}');
        }
        out.push_str(&self.text);
        out
    }

    /// Replace `range` with `text` and return the edit that undoes it. `Err` when the range is outside the text,
    /// backwards or inside a character; the text is then untouched.
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Result<Edit, crate::Error> {
        let ok = range.start <= range.end
            && range.end <= self.text.len()
            && self.text.is_char_boundary(range.start)
            && self.text.is_char_boundary(range.end);
        if !ok {
            return Err(crate::Error::Range);
        }
        let old = self.text[range.clone()].to_owned();
        self.text.replace_range(range.clone(), text);
        // Line starts inside (start, end] are gone, the new breaks come in, the later ones move by the length change.
        let i = self.starts.partition_point(|&x| x <= range.start);
        let j = self.starts.partition_point(|&x| x <= range.end);
        let delta = text.len() as isize - (range.end - range.start) as isize;
        for x in &mut self.starts[j..] {
            *x = (*x as isize + delta) as usize;
        }
        let added: Vec<usize> = text.match_indices('\n').map(|(p, _)| range.start + p + 1).collect();
        self.starts.splice(i..j, added);
        self.version += 1;
        Ok(Edit { range: range.start..range.start + text.len(), text: old })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small deterministic generator (xorshift), no dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as usize
        }
    }

    fn same_as_pos(b: &Buffer) {
        let t = b.text();
        assert_eq!(b.line_count(), pos::line_count(t), "{t:?}");
        for line in 0..=b.line_count() {
            assert_eq!(b.line_range(line), pos::line_range(t, line), "{t:?} line {line}");
            for col in [0, 1, 2, 5, 99] {
                assert_eq!(b.byte_at(line, col), pos::byte_at(t, line, col), "{t:?} at {line}:{col}");
            }
        }
        for byte in 0..=t.len() + 1 {
            assert_eq!(b.line_of(byte), pos::line_of(t, byte), "{t:?} byte {byte}");
            assert_eq!(b.line_col(byte), pos::line_col(t, byte), "{t:?} byte {byte}");
        }
    }

    #[test]
    fn line_index_matches_the_scanning_functions_after_random_edits() {
        let pieces = ["a", "bc", "\n", "\r\n", "\r", "日本", "👨\u{200d}👩", "😀", "e\u{301}", "", "x\ny\n", "\n\n"];
        let mut rng = Rng(0x9e3779b97f4a7c15);
        for start in ["", "no newline", "a\r\nb\n日本\n", "\n", "\r\n\r\n"] {
            let mut b = Buffer::new(start);
            same_as_pos(&b);
            for _ in 0..300 {
                let len = b.len();
                let mut a = rng.next() % (len + 1);
                let mut z = rng.next() % (len + 1);
                if a > z {
                    std::mem::swap(&mut a, &mut z);
                }
                let (a, z) = (pos::floor(b.text(), a), pos::floor(b.text(), z));
                let ins = pieces[rng.next() % pieces.len()];
                let inv = b.replace(a..z, ins).unwrap();
                same_as_pos(&b);
                if rng.next() % 3 == 0 {
                    b.replace(inv.range, &inv.text).unwrap();
                    same_as_pos(&b);
                }
            }
        }
    }

    #[test]
    fn a_failed_replace_leaves_the_index_alone() {
        let mut b = Buffer::new("a\nあ\n");
        assert!(b.replace(3..4, "x").is_err());
        same_as_pos(&b);
    }
}
