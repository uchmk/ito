//! The minimap of a text pane: a narrow strip down the right where each line is a 2-pixel band, the part on screen
//! is framed, a click or drag jumps there, and hovering shows the few lines around the pointer.
//!
//! The sums (rows, scale, the hover card's place and window) are plain functions and need no window. The strip is
//! drawn behind the feature `egui`. Text is given as `(text, color)` pieces, not an app's own span type, so a
//! highlighter's output (`ito-syntax`) or a Markdown source both fit.

use std::ops::Range;

use unicode_width::UnicodeWidthChar;

#[cfg(feature = "egui")]
mod draw;
#[cfg(feature = "egui")]
pub use draw::{show, Look, Shown, Style};

/// Height of one band, in points. Two is enough to read the shape of a file and coarse enough that a thousand lines
/// fit in a pane.
pub const BAND_H: f32 = 2.0;
/// Width of the strip, in cells. Narrower than this the bands are a smudge.
pub const COLS: u16 = 7;
/// A pane narrower than this (in cells) needs its width for the text more than for a map.
pub const MIN_COLS: u16 = 56;
/// How much of the pane the hover card may take.
pub const HOVER_CARD_SHARE: f32 = 0.6;
/// Lines the card shows either side of the one pointed at.
pub const HOVER_CONTEXT: usize = 3;
/// How much of a line the card lays out.
pub const HOVER_MAX_CHARS: usize = 200;

/// One line as the strip needs it: where its text starts, how far it runs, and what color it mostly is.
///
/// Six bytes a line, so even a large file costs tens of kilobytes. The color stays `None` where the highlighter had
/// nothing to say, so the theme (which can change under a pane already on screen) decides the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row {
    /// Leading white space, in cells.
    pub indent: u16,
    /// Cells of text after the indent. Zero for a blank line.
    pub len: u16,
    pub color: Option<[u8; 3]>,
}

/// Columns a string takes on the cell grid, with full-width characters counting two.
pub fn cells(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// One line's row from its `(text, color)` pieces. The widest piece speaks for the color, so code with a comment
/// after it still reads as code. The indent is counted across pieces: a highlighter may hand back the leading white
/// space on its own.
pub fn row<'a>(pieces: impl IntoIterator<Item = (&'a str, Option<[u8; 3]>)>) -> Row {
    let mut row = Row::default();
    let mut started = false;
    let mut widest = 0usize;
    for (text, color) in pieces {
        let trimmed = text.trim_start();
        if !started {
            let lead = text.len() - trimmed.len();
            row.indent = row.indent.saturating_add(cells(&text[..lead]) as u16);
            if trimmed.is_empty() {
                continue;
            }
            started = true;
        }
        let w = cells(trimmed.trim_end());
        row.len = row.len.saturating_add(cells(trimmed) as u16);
        if w > widest {
            widest = w;
            row.color = color;
        }
    }
    row
}

/// The rows of `lines`, each a list of `(text, color)` pieces. Build them on a worker, beside the lines: walking
/// every piece of a ten-thousand-line file is not something to do again on each frame.
pub fn rows<'a, L, P>(lines: L) -> Vec<Row>
where
    L: IntoIterator<Item = P>,
    P: IntoIterator<Item = (&'a str, Option<[u8; 3]>)>,
{
    lines.into_iter().map(row).collect()
}

/// One word of a line on the cell grid: where it starts (counting the indent), how many cells it covers, and its color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub start: u16,
    pub len: u16,
    pub color: Option<[u8; 3]>,
}

/// The words of a line as coloured runs, up to `max_cols` cells. A run ends at white space or where the next piece
/// begins, so the gaps between words stay blank. Unlike [`row`] this keeps every piece's color.
pub fn runs<'a>(pieces: impl IntoIterator<Item = (&'a str, Option<[u8; 3]>)>, max_cols: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut col = 0usize;
    for (text, color) in pieces {
        let mut from: Option<usize> = None;
        for ch in text.chars() {
            if col >= max_cols {
                break;
            }
            let w = ch.width().unwrap_or(0);
            if ch.is_whitespace() {
                if let Some(a) = from.take() {
                    out.push(Run { start: a as u16, len: (col - a) as u16, color });
                }
            } else if from.is_none() {
                from = Some(col);
            }
            col += w;
        }
        if let Some(a) = from {
            let end = col.min(max_cols);
            if end > a {
                out.push(Run { start: a as u16, len: (end - a) as u16, color });
            }
        }
        if col >= max_cols {
            break;
        }
    }
    out
}

/// How much ink a character leaves, 0 to 1: letters and digits are dense, punctuation light, white space none.
pub fn ink(c: char) -> f32 {
    if c.is_whitespace() || c.is_control() {
        0.0
    } else if c.is_alphanumeric() {
        0.85
    } else {
        0.45
    }
}

/// A rectangle in points, `y` growing down. The sums below use this so they need no window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Area {
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self { left, top, right, bottom }
    }
    pub fn width(&self) -> f32 {
        self.right - self.left
    }
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// Whether a pane `width` points wide, with cells `cell` points wide, has room for the strip (and `rows` is worth a
/// map: one line is not).
pub fn fits(rows: usize, width: f32, cell: f32) -> bool {
    rows >= 2 && ((width - 20.0) / cell).max(0.0) as u16 >= MIN_COLS
}

/// The strip's width in points.
pub fn strip_width(cell: f32) -> f32 {
    COLS as f32 * cell
}

/// Where a line sits down the strip, as a fraction of its height.
///
/// The bands and the box marking the viewport both go through here: with a scale each, they disagreed and the picture
/// and the marker pointed at different parts of the same file.
pub fn at_line(line: usize, lines: usize) -> f32 {
    line.min(lines) as f32 / lines.max(1) as f32
}

/// The line the strip points at for a pointer at `y`, in a strip `top` down and `height` tall. The inverse of
/// [`at_line`], clamped at both ends (the bottom pixel is the last line, not one past it).
pub fn line_at_y(y: f32, top: f32, height: f32, lines: usize) -> usize {
    if lines == 0 {
        return 0;
    }
    let t = ((y - top) / height.max(1.0)).clamp(0.0, 1.0);
    ((t * lines as f32) as usize).min(lines - 1)
}

/// Height of a line in the strip when the file is short enough not to need squeezing, in points.
pub const LINE_H: f32 = 3.0;

/// How tall the map is in a strip `height` points tall: a short file keeps its natural size at the top (as in VS
/// Code) instead of being stretched down the whole strip. A long one fills the strip.
pub fn map_height(lines: usize, height: f32) -> f32 {
    height.min(lines as f32 * LINE_H).max(0.0)
}

/// How many lines one band holds when the strip is `height` points tall.
pub fn per_band(lines: usize, height: f32) -> usize {
    let bands = ((height / BAND_H).floor() as usize).max(1);
    lines.div_ceil(bands).max(1)
}

/// The top line a click at `line` should scroll to: the line pointed at lands in the middle of the pane.
pub fn scroll_to(line: usize, on_screen: usize) -> usize {
    line.saturating_sub(on_screen / 2)
}

/// A band from `top` to `bottom`, in points, on whole physical pixels and at least one pixel high. Rounding both ends
/// the same way makes each band end exactly where the next begins.
pub fn band_pixels(top: f32, bottom: f32, ppp: f32) -> (f32, f32) {
    let t = (top * ppp).round();
    let b = (bottom * ppp).round().max(t + 1.0);
    (t / ppp, b / ppp)
}

/// Where the hover card goes: against the strip, beside the pointer, inside the pane. `at` is how far down the card
/// the pointer should be (the middle of the row for the line pointed at; the middle of the card except near either
/// end of the file, where the card shows the lines there are).
pub fn hover_card(pointer_y: f32, pane: Area, strip: Area, w: f32, h: f32, at: f32) -> Area {
    let w = w.min(pane.width() * HOVER_CARD_SHARE);
    let right = strip.left - 4.0;
    let top = (pointer_y - at).clamp(pane.top, (pane.bottom - h).max(pane.top));
    Area::new(right - w, top, right, top + h)
}

/// The lines the card shows for line `k` of `len`, when `fit` rows fit in the pane and there is room for `above`
/// rows over the pointer's and `below` under it: `k` and up to [`HOVER_CONTEXT`] either side. Near either end of the
/// file the window slides rather than shrinks, so the card is the same height wherever it is. Near either end of
/// the pane it slides too, so the pointer stays beside the line marked.
pub fn hover_window(k: usize, len: usize, fit: usize, above: usize, below: usize) -> Range<usize> {
    let n = (2 * HOVER_CONTEXT + 1).min(len).min(fit.max(1));
    let row = (n / 2).min(above).max((n - 1).saturating_sub(below));
    let start = k.saturating_sub(row).min(len - n);
    start..start + n
}

/// How many white-space characters a line starts with; `None` for a blank line, which says nothing about how deep
/// the lines around it are.
pub fn indent_chars<'a>(pieces: impl IntoIterator<Item = &'a str>) -> Option<usize> {
    pieces.into_iter().flat_map(|s| s.chars()).position(|c| !c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(top: f32, h: f32) -> Area {
        Area::new(200.0, top, 220.0, top + h)
    }

    #[test]
    fn runs_keep_the_gaps_between_words_and_each_pieces_color() {
        let red = Some([255, 0, 0]);
        let r = runs([("  let", None), (" x", red), (" = 1;", None)], 80);
        assert_eq!(
            r,
            vec![
                Run { start: 2, len: 3, color: None },
                Run { start: 6, len: 1, color: red },
                Run { start: 8, len: 1, color: None },
                Run { start: 10, len: 2, color: None },
            ]
        );
    }

    #[test]
    fn runs_stop_at_the_strip_width_and_count_wide_letters_double() {
        let r = runs([("abcdef", None)], 4);
        assert_eq!(r, vec![Run { start: 0, len: 4, color: None }]);
        assert_eq!(runs([("日本", None)], 80), vec![Run { start: 0, len: 4, color: None }]);
        assert!(ink('a') > ink('.') && ink(' ') == 0.0);
    }

    #[test]
    fn a_short_file_keeps_its_natural_height() {
        assert_eq!(map_height(33, 800.0), 99.0);
        assert_eq!(map_height(5000, 800.0), 800.0);
        assert_eq!(map_height(0, 800.0), 0.0);
    }

    #[test]
    fn a_row_records_the_indent_and_the_width() {
        let r = rows([vec![("    let x = 1;", None)], vec![("", None)], vec![("fn main() {", None)]]);
        assert_eq!(r[0], Row { indent: 4, len: 10, color: None });
        assert_eq!(r[1], Row::default(), "a blank line leaves a gap");
        assert_eq!(r[2].indent, 0);
    }

    #[test]
    fn the_widest_piece_gives_the_row_its_color() {
        const CODE: [u8; 3] = [0x80, 0xc0, 0xff];
        const NOTE: [u8; 3] = [0x60, 0x60, 0x60];
        assert_eq!(row([("let answer = compute();", Some(CODE)), (" // why", Some(NOTE))]).color, Some(CODE));
        assert_eq!(row([("x;", Some(CODE)), (" // a long explanation", Some(NOTE))]).color, Some(NOTE));
    }

    #[test]
    fn an_indent_split_across_pieces_still_counts() {
        let r = row([("  ", None), ("  ", None), ("x", None)]);
        assert_eq!((r.indent, r.len), (4, 1));
    }

    #[test]
    fn full_width_letters_take_two_cells() {
        assert_eq!(row([("　設定", None)]), Row { indent: 2, len: 4, color: None });
    }

    #[test]
    fn the_strip_is_for_a_wide_pane_with_more_than_one_line() {
        assert!(fits(500, 620.0, 8.0));
        assert!(!fits(1, 620.0, 8.0), "one line is not a map");
        assert!(!fits(500, 400.0, 8.0), "too narrow to spare the columns");
        assert_eq!(strip_width(8.0), 56.0);
    }

    #[test]
    fn the_pointer_never_indexes_past_the_end() {
        for lines in [1usize, 2, 7, 99, 100, 101, 4001] {
            for h in [1.0f32, 13.0, 200.0, 999.5] {
                let r = strip(50.0, h);
                assert_eq!(line_at_y(r.bottom, r.top, r.height(), lines), lines - 1, "{lines} lines, {h} tall");
                assert_eq!(line_at_y(r.bottom + 500.0, r.top, r.height(), lines), lines - 1);
                assert_eq!(line_at_y(r.top, r.top, r.height(), lines), 0);
                assert_eq!(line_at_y(r.top - 500.0, r.top, r.height(), lines), 0);
            }
        }
        assert_eq!(line_at_y(50.0, 0.0, 100.0, 0), 0);
    }

    #[test]
    fn the_scale_round_trips() {
        let r = strip(10.0, 400.0);
        for n in [1usize, 2, 50, 400, 1000, 4001] {
            for l in [0, 1, n / 3, n / 2, n - 1] {
                let y = r.top + at_line(l, n) * r.height();
                let back = line_at_y(y, r.top, r.height(), n);
                assert!(back.abs_diff(l) <= 1, "{n} lines: line {l} drawn at {y} reads back as {back}");
            }
        }
    }

    #[test]
    fn the_hover_card_stays_in_the_pane() {
        let pane = Area::new(0.0, 100.0, 200.0, 500.0);
        let strip = Area::new(200.0, 100.0, 220.0, 500.0);
        for y in [-100.0f32, 100.0, 101.0, 300.0, 499.0, 500.0, 900.0] {
            let card = hover_card(y, pane, strip, 80.0, 18.0, 9.0);
            assert!(card.right <= strip.left, "y={y}: {card:?} runs into the strip");
            assert!(card.top >= pane.top - 0.01, "y={y}: {card:?} above the pane");
            assert!(card.bottom <= pane.bottom + 0.01, "y={y}: {card:?} below the pane");
        }
        let wide = hover_card(300.0, pane, strip, 10_000.0, 18.0, 9.0);
        assert!(wide.width() <= pane.width() * 0.6 + 0.01, "{wide:?}");
        assert!(wide.left >= pane.left - 0.01, "{wide:?}");
        let tall = hover_card(499.0, pane, strip, 80.0, 130.0, 65.0);
        assert!(tall.bottom <= pane.bottom + 0.01 && tall.top >= pane.top - 0.01, "{tall:?}");
        let off = hover_card(300.0, pane, strip, 80.0, 130.0, 20.0);
        assert!((off.top + 20.0 - 300.0).abs() < 0.01, "{off:?}");
    }

    #[test]
    fn the_hover_window_slides_at_the_ends() {
        let w = |k, len, fit| hover_window(k, len, fit, 50, 50);
        assert_eq!(w(200, 400, 50), 197..204, "three either side");
        assert_eq!(w(0, 400, 50), 0..7);
        assert_eq!(w(2, 400, 50), 0..7);
        assert_eq!(w(399, 400, 50), 393..400);
        assert_eq!(w(1, 3, 50), 0..3, "a short file is all of it");
        assert_eq!(w(0, 1, 50), 0..1);
        assert_eq!(w(200, 400, 3), 199..202, "a short pane takes what fits");
        assert_eq!(w(200, 400, 0), 200..201, "and at least the line itself");
        assert_eq!(hover_window(200, 400, 50, 0, 50), 200..207);
        assert_eq!(hover_window(200, 400, 50, 1, 50), 199..206);
        assert_eq!(hover_window(200, 400, 50, 50, 0), 194..201);
        assert_eq!(hover_window(200, 400, 50, 50, 2), 196..203);
        assert_eq!(hover_window(1, 400, 50, 50, 0), 0..7);
        for len in 1..20 {
            for fit in 0..10 {
                for room in 0..9 {
                    for k in 0..len {
                        for (above, below) in [(room, 50), (50, room), (room, room)] {
                            let w = hover_window(k, len, fit, above, below);
                            assert!(w.contains(&k) && w.end <= len && w.len() <= fit.max(1), "{k} of {len}, {fit} fit: {w:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_bands_fill_the_strip() {
        for lines in [1usize, 2, 7, 99, 100, 101, 202, 1000, 4001, 12345] {
            for bands in [1usize, 17, 100, 377] {
                let per = lines.div_ceil(bands).max(1);
                let chunks = lines.div_ceil(per);
                let bottom = at_line((chunks - 1) * per, lines) + at_line(per, lines);
                assert!(bottom >= 1.0 - f32::EPSILON, "lines {lines} bands {bands}: strip ends at {bottom}");
            }
        }
        assert_eq!(per_band(200, 200.0), 2);
        assert_eq!(per_band(5, 200.0), 1);
    }

    #[test]
    fn the_scale_spans_nought_to_one() {
        assert_eq!(at_line(0, 500), 0.0);
        assert_eq!(at_line(500, 500), 1.0);
        assert_eq!(at_line(9999, 500), 1.0);
        assert!(at_line(0, 0).is_finite() && at_line(5, 0).is_finite());
    }

    #[test]
    fn a_click_lands_with_the_line_in_the_middle() {
        assert_eq!(scroll_to(100, 40), 80);
        assert_eq!(scroll_to(3, 40), 0);
    }

    #[test]
    fn bands_meet_on_whole_pixels() {
        let (t, b) = band_pixels(10.1, 12.2, 1.5);
        assert!(((t * 1.5).fract()).abs() < 1e-4 && ((b * 1.5).fract()).abs() < 1e-4);
        let (t, b) = band_pixels(5.0, 5.0, 1.0);
        assert!(b - t >= 1.0, "at least a pixel high");
    }

    #[test]
    fn indent_is_none_for_a_blank_line() {
        assert_eq!(indent_chars(["  ", "x"]), Some(2));
        assert_eq!(indent_chars(["  ", " "]), None);
    }
}
