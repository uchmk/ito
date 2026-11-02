use std::ops::Range;

use crate::Edit;

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
        Buffer { text: text.to_owned(), version: 0, bom, eol }
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
        self.version += 1;
        Ok(Edit { range: range.start..range.start + text.len(), text: old })
    }
}
