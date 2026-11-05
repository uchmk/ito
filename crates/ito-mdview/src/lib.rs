//! Markdown laid out for reading, the way a viewer shows it: wrapped paragraphs, list markers, quote bars, aligned
//! tables, highlighted code. Everything sits on a monospace cell grid (CJK = 2 columns), so widths are counted, not
//! measured. The result is a [`Doc`] of [`DocLine`]s that knows no app type and no window.
//!
//! Every line carries the bytes of the original text it came from, so an editor can scroll its preview with the text
//! ([`Doc::line_at_byte`]) and a click on the preview can put the caret in the text ([`Doc::byte_at_line`]).
//!
//! Math, diagrams and images get a line of their own with a placeholder ([`LineKind::Placeholder`]), so a later
//! picture has its place reserved.

use std::ops::Range;

mod lay;
#[cfg(feature = "egui")]
mod view;

#[cfg(feature = "egui")]
pub use view::{show, Options, Output, State};

pub use lay::{lay_out, render, table_lines, Row, MAX_LINES};

/// A run of text sharing one style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub color: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    /// Inline code, painted on a tinted background.
    pub code: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineKind {
    #[default]
    Text,
    /// Last line of a heading, which H1 and H2 underline.
    Heading(u8),
    Code,
    Rule,
    /// A place kept for something drawn later: a formula, a diagram.
    Placeholder,
}

#[derive(Clone, Debug, Default)]
pub struct DocLine {
    pub spans: Vec<Span>,
    pub kind: LineKind,
    /// Columns taken by quote bars and list markers; code bands and rules start here.
    pub indent: u16,
    /// 0-based line of the original text this came from.
    pub src: usize,
    /// The bytes of the original text this line shows. A paragraph cut into several lines gives each the pieces it
    /// holds; a blank line between blocks is empty (it sits at the end of the block above).
    pub bytes: Range<usize>,
    /// Carries on the line above: the same paragraph (or code line) cut to the pane's width, so a search sees the
    /// two as one.
    pub wrap: bool,
}

/// Markdown laid out. Lines are already wrapped to `body_cols`, and each span starts at the column the widths before
/// it add up to, so the UI can place spans on a cell grid even where glyph widths disagree.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub lines: Vec<DocLine>,
    /// Level and first rendered line of every heading (down to level 6), in order.
    pub headings: Vec<(u8, usize)>,
    pub body_cols: u16,
}

impl Doc {
    /// The original line a rendered line came from.
    pub fn src_for_line(&self, line: usize) -> usize {
        self.lines.get(line).or(self.lines.last()).map_or(0, |l| l.src)
    }

    /// The first rendered line of the block holding an original line (or of the nearest one above it, for blank lines).
    pub fn line_for_src(&self, src: usize) -> usize {
        let Some(last) = self.lines.iter().rposition(|l| l.src <= src) else { return 0 };
        let at = self.lines[last].src;
        self.lines[..last].iter().rposition(|l| l.src != at).map_or(0, |i| i + 1)
    }

    /// The rendered line that shows the original `byte`: the last line starting at or before it, skipping the blank
    /// lines between blocks (so the gap after a paragraph maps to the paragraph).
    pub fn line_at_byte(&self, byte: usize) -> usize {
        self.lines.iter().rposition(|l| !l.bytes.is_empty() && l.bytes.start <= byte).unwrap_or(0)
    }

    /// Where rendered `line` starts in the original text (a line past the end gives the end of the last one).
    pub fn byte_at_line(&self, line: usize) -> usize {
        match self.lines.get(line) {
            Some(l) => l.bytes.start,
            None => self.lines.last().map_or(0, |l| l.bytes.end),
        }
    }

    /// The original byte under cell `col` of rendered `line` (a click on the preview). The text a line shows is
    /// shorter than the bytes it came from where markup was dropped (`**`, link targets), so past the markup the
    /// answer is the line's start plus the bytes of the text before `col`, clamped to the line's bytes: close, not
    /// exact. The caller snaps it to a character boundary of the original. Quote bars and list markers (`indent`)
    /// give the start of the line.
    pub fn byte_at(&self, line: usize, col: usize) -> usize {
        let Some(l) = self.lines.get(line) else { return self.byte_at_line(line) };
        let (mut at, mut seen) = (0, 0);
        'spans: for s in &l.spans {
            for c in s.text.chars() {
                let w = cells(c.encode_utf8(&mut [0; 4]));
                if at >= col {
                    break 'spans;
                }
                if at >= usize::from(l.indent) {
                    seen += c.len_utf8();
                }
                at += w;
            }
        }
        l.bytes.start + seen.min(l.bytes.len())
    }

    /// Where `m` matches in the rendered lines `from..to`: for each, the byte ranges of its spans' text laid end to
    /// end. A paragraph cut into several lines to fit the pane is searched as the one run of text it is (`a.*b` finds
    /// an `a` and a `b` on different lines). The quote bars and list markers in front of a line never match.
    pub fn marks(&self, m: &ito_match::Matcher, from: usize, to: usize) -> Vec<Vec<Range<usize>>> {
        marks(self, m, from, to)
    }
}

/// Colours of the page. `None` is the pane's own text colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Palette {
    pub heading: Option<[u8; 3]>,
    pub code: Option<[u8; 3]>,
    pub link: Option<[u8; 3]>,
    pub quote: Option<[u8; 3]>,
    pub marker: Option<[u8; 3]>,
    pub dim: Option<[u8; 3]>,
    /// Note, Tip, Important, Warning, Caution.
    pub alerts: [Option<[u8; 3]>; 5],
    /// Code blocks: keyword, string, comment, number, constant, type, function.
    pub tokens: [Option<[u8; 3]>; 7],
}

impl Palette {
    /// The page in the colours of the current theme (`ito_theme::colors()`).
    #[cfg(feature = "theme")]
    pub fn from_theme() -> Self {
        let c = ito_theme::colors();
        let rgb = |c: [u8; 4]| Some([c[0], c[1], c[2]]);
        Self {
            heading: rgb(c.blue.to_array()),
            code: rgb(c.done.to_array()),
            link: rgb(c.blue.to_array()),
            quote: rgb(c.dim.to_array()),
            marker: rgb(c.wait.to_array()),
            dim: rgb(c.dim.to_array()),
            alerts: [
                rgb(c.blue.to_array()),
                rgb(c.done.to_array()),
                rgb(c.magenta.to_array()),
                rgb(c.wait.to_array()),
                rgb(c.err.to_array()),
            ],
            tokens: [
                rgb(c.magenta.to_array()),
                rgb(c.done.to_array()),
                rgb(c.dim.to_array()),
                rgb(c.wait.to_array()),
                rgb(c.wait.to_array()),
                rgb(c.run.to_array()),
                rgb(c.blue.to_array()),
            ],
        }
    }

    pub(crate) fn token(&self, tok: ito_syntax::Tok) -> Option<[u8; 3]> {
        use ito_syntax::Tok;
        self.tokens[match tok {
            Tok::Keyword => 0,
            Tok::Str => 1,
            Tok::Comment => 2,
            Tok::Number => 3,
            Tok::Const => 4,
            Tok::Type => 5,
            Tok::Func => 6,
        }]
    }
}

/// Columns a string takes on the cell grid, with CJK characters counting two.
pub fn cells(s: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

fn marks(doc: &Doc, m: &ito_match::Matcher, from: usize, to: usize) -> Vec<Vec<Range<usize>>> {
    let to = to.min(doc.lines.len());
    let mut out = vec![Vec::new(); to.saturating_sub(from)];
    let mut at = from.min(to);
    while at > 0 && doc.lines.get(at).is_some_and(|l| l.wrap) {
        at -= 1;
    }
    while at < to {
        let mut end = at + 1;
        while doc.lines.get(end).is_some_and(|l| l.wrap) {
            end += 1;
        }
        // Each line's content: where it starts in the line's own text, how long it is, and where it sits in the
        // joined text.
        let mut joined = String::new();
        let mut parts = Vec::new();
        for l in &doc.lines[at..end] {
            let (mut cells_in, mut skip) = (0, 0);
            for s in &l.spans {
                if cells_in >= usize::from(l.indent) {
                    break;
                }
                cells_in += cells(&s.text);
                skip += s.text.len();
            }
            let whole: String = l.spans.iter().map(|s| s.text.as_str()).collect();
            let body = &whole[skip.min(whole.len())..];
            if !parts.is_empty() && l.kind != LineKind::Code {
                let wide = |c: Option<char>| c.is_some_and(|c| cells(c.encode_utf8(&mut [0; 4])) >= 2);
                if !(wide(joined.chars().next_back()) && wide(body.chars().next())) {
                    joined.push(' ');
                }
            }
            parts.push((joined.len(), skip, body.len()));
            joined.push_str(body);
        }
        for r in m.ranges(&joined) {
            for (k, &(off, skip, len)) in parts.iter().enumerate() {
                let (s, e) = (r.start.max(off), r.end.min(off + len));
                let i = at + k;
                if s < e && (from..to).contains(&i) {
                    out[i - from].push(skip + s - off..skip + e - off);
                }
            }
        }
        at = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_at_follows_columns_past_the_markers() {
        let d = render(
            "- ab日c
",
            40,
            &Palette::default(),
        )
        .0;
        let l = &d.lines[0];
        assert!(l.indent >= 2);
        let at = |col| d.byte_at(0, col);
        assert_eq!(at(0), l.bytes.start);
        assert_eq!(at(usize::from(l.indent)), l.bytes.start);
        assert_eq!(at(usize::from(l.indent) + 2), l.bytes.start + 2);
        assert!(at(500) <= l.bytes.end);
        assert_eq!(d.byte_at(99, 0), d.byte_at_line(99));
    }
}
