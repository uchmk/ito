//! Positions in a text. Structure commands work in bytes, the caret moves by graphemes, a screen shows lines and
//! columns; this is the one place that converts. A line break is `\n` or `\r\n`; a column counts graphemes.
//! Each function scans from the start (or from a line), so call it for the caret, not for every character.

use std::ops::Range;

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// `byte` pulled back to the nearest `char` boundary at or below it (and into the text).
pub fn floor(text: &str, byte: usize) -> usize {
    let mut b = byte.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

/// The `char` index of `byte`.
pub fn char_of_byte(text: &str, byte: usize) -> usize {
    text[..floor(text, byte)].chars().count()
}

/// The byte offset of the `n`-th `char` (the end of the text past the last).
pub fn byte_of_char(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
}

/// The grapheme boundary after `byte`, or `None` at the end.
pub fn next_grapheme(text: &str, byte: usize) -> Option<usize> {
    let b = floor(text, byte);
    GraphemeCursor::new(b, text.len(), true).next_boundary(text, 0).ok().flatten()
}

/// The grapheme boundary before `byte`, or `None` at the start.
pub fn prev_grapheme(text: &str, byte: usize) -> Option<usize> {
    let b = floor(text, byte);
    GraphemeCursor::new(b, text.len(), true).prev_boundary(text, 0).ok().flatten()
}

/// How many lines the text has: one more than its `\n`, so `"a\n"` has two (the second empty).
pub fn line_count(text: &str) -> usize {
    text.bytes().filter(|&b| b == b'\n').count() + 1
}

/// The line (from 0) holding `byte`.
pub fn line_of(text: &str, byte: usize) -> usize {
    text.as_bytes()[..floor(text, byte)].iter().filter(|&&b| b == b'\n').count()
}

/// The bytes of line `line` without its break; `None` past the last line.
pub fn line_range(text: &str, line: usize) -> Option<Range<usize>> {
    let mut start = 0;
    for _ in 0..line {
        start += text[start..].find('\n')? + 1;
    }
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let end = if end > start && text.as_bytes()[end - 1] == b'\r' { end - 1 } else { end };
    Some(start..end)
}

/// `(line, column)` of `byte`, both from 0, the column in graphemes.
pub fn line_col(text: &str, byte: usize) -> (usize, usize) {
    let b = floor(text, byte);
    let start = text[..b].rfind('\n').map_or(0, |i| i + 1);
    (line_of(text, b), text[start..b].graphemes(true).count())
}

/// The byte at `(line, column)`; a column past the end of the line gives the end of the line, a line past the end
/// gives the end of the text.
pub fn byte_at(text: &str, line: usize, col: usize) -> usize {
    let Some(r) = line_range(text, line) else { return text.len() };
    let mut at = r.start;
    for _ in 0..col {
        match next_grapheme(&text[..r.end], at) {
            Some(n) => at = n,
            None => break,
        }
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_chars_and_graphemes() {
        let t = "aあ👨‍👩‍👧b";
        assert_eq!(char_of_byte(t, 1), 1);
        assert_eq!(byte_of_char(t, 2), 4);
        assert_eq!(char_of_byte(t, 5), 2, "inside a character falls back");
        let fam = 4;
        let after = next_grapheme(t, fam).unwrap();
        assert_eq!(&t[after..], "b", "the family is one grapheme");
        assert_eq!(prev_grapheme(t, after), Some(fam));
        assert_eq!(prev_grapheme(t, 0), None);
        assert_eq!(next_grapheme(t, t.len()), None);
    }

    #[test]
    fn lines_and_columns_with_crlf() {
        let t = "ab\r\n日本\nc";
        assert_eq!(line_count(t), 3);
        assert_eq!(line_range(t, 0), Some(0..2));
        assert_eq!(line_range(t, 1), Some(4..10));
        assert_eq!(line_range(t, 2), Some(11..12));
        assert_eq!(line_range(t, 3), None);
        assert_eq!(line_col(t, 7), (1, 1));
        assert_eq!(byte_at(t, 1, 1), 7);
        assert_eq!(byte_at(t, 1, 99), 10);
        assert_eq!(byte_at(t, 9, 0), t.len());
        assert_eq!(line_count("a\n"), 2);
        assert_eq!(line_range("a\n", 1), Some(2..2));
    }
}
