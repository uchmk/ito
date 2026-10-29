//! Right-to-left text (Hebrew, Arabic, ...) drawn in the order it is read:
//! each row on its own, left to right as a whole (the Unicode bidirectional
//! algorithm's implicit levels, with the paragraph at level 0, as VTE does
//! by default). Only the drawing moves; the grid, the cursor, a selection
//! and what is copied keep the order the program wrote (tsumugi's #28).

use unicode_bidi::{BidiClass, BidiInfo, Level};

use crate::CellView;

/// A row drawn in another order than its cells: by place on the screen,
/// the cell drawn there, and whether it sits in a right-to-left run.
#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    /// `cells[visual] == logical`.
    pub cells: Vec<usize>,
    /// By place on the screen: read right to left there.
    pub rtl: Vec<bool>,
}

impl Order {
    /// Where the cell `logical` is drawn.
    pub fn visual(&self, logical: usize) -> usize {
        self.cells.iter().position(|&l| l == logical).unwrap_or(logical)
    }

    /// The cell drawn at `visual` (past the row's end, itself).
    pub fn logical(&self, visual: usize) -> usize {
        self.cells.get(visual).copied().unwrap_or(visual)
    }

    /// Where the cells `from..to` are drawn, as runs side by side on the
    /// screen, left to right.
    pub fn spans(&self, from: usize, to: usize) -> Vec<std::ops::Range<usize>> {
        let mut at: Vec<usize> = (from..to).map(|l| self.visual(l)).collect();
        at.sort_unstable();
        let mut out: Vec<std::ops::Range<usize>> = Vec::new();
        for v in at {
            match out.last_mut() {
                Some(r) if r.end == v => r.end = v + 1,
                _ => out.push(v..v + 1),
            }
        }
        out
    }
}

/// Whether a character starts right-to-left text: a strong right-to-left
/// letter or an embedding, override or isolate that asks for one.
fn rtl(c: char) -> bool {
    // Below Hebrew there is none; most rows end here.
    (c as u32) >= 0x0590
        && matches!(unicode_bidi::bidi_class(c), BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI)
}

/// The order to draw `row` in, or `None` when it is drawn as it stands (no
/// right-to-left text in it).
pub fn order(row: &[CellView]) -> Option<Order> {
    if !row.iter().any(|c| rtl(c.c)) {
        return None;
    }
    // One character a cell, so a character's place is its cell.
    let text: String = row.iter().map(|c| if c.c == '\0' { ' ' } else { c.c }).collect();
    let info = BidiInfo::new(&text, Some(Level::ltr()));
    let para = info.paragraphs.first()?;
    let levels = info.reordered_levels_per_char(para, para.range.clone());
    let mut cells = BidiInfo::reorder_visual(&levels);
    // A wide character turned around would come after the half that
    // follows it, and spill into the next cell: put it back in front.
    let wide = alacritty_terminal::term::cell::Flags::WIDE_CHAR;
    for v in 1..cells.len() {
        if row[cells[v]].flags.contains(wide) && cells[v] + 1 == cells[v - 1] {
            cells.swap(v - 1, v);
        }
    }
    let rtl = cells.iter().map(|&l| levels[l].is_rtl()).collect();
    Some(Order { cells, rtl })
}

/// The character drawn for `c` in a right-to-left run: a bracket faces the
/// other way (the algorithm's mirroring, for the pairs a terminal shows).
pub fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(c: char) -> CellView {
        use alacritty_terminal::vte::ansi::{Color, NamedColor};
        let (fg, bg) = (Color::Named(NamedColor::Foreground), Color::Named(NamedColor::Background));
        CellView { c, fg, bg, flags: alacritty_terminal::term::cell::Flags::empty(), ul: None, selected: false, found: false }
    }

    fn row(text: &str) -> Vec<CellView> {
        text.chars().map(cell).collect()
    }

    #[test]
    fn a_row_without_right_to_left_text_stays() {
        assert_eq!(order(&row("echo hello (world)")), None);
        assert_eq!(order(&row("日本語のテキスト")), None);
    }

    #[test]
    fn hebrew_is_read_right_to_left_in_place() {
        // "ab אבג cd": the three letters turn around, the rest stays.
        let r = row("ab אבג cd");
        let o = order(&r).unwrap();
        let drawn: String = o.cells.iter().map(|&l| r[l].c).collect();
        assert_eq!(drawn, "ab גבא cd");
        assert_eq!(o.rtl, [false, false, false, true, true, true, false, false, false]);
        assert_eq!(o.visual(3), 5);
        assert_eq!(o.logical(5), 3);
        assert_eq!(o.logical(40), 40, "past the end, itself");
        // A selection over "ab א" is drawn in two pieces.
        assert_eq!(o.spans(0, 4), [0..3, 5..6]);
    }

    #[test]
    fn numbers_keep_their_order_inside_right_to_left_text() {
        let r = row("שלום 123 עולם");
        let o = order(&r).unwrap();
        let drawn: String = o.cells.iter().map(|&l| r[l].c).collect();
        assert_eq!(drawn, "םלוע 123 םולש");
    }

    #[test]
    fn brackets_face_the_other_way() {
        assert_eq!(mirror('('), ')');
        assert_eq!(mirror('»'), '«');
        assert_eq!(mirror('a'), 'a');
    }

    #[test]
    fn a_wide_character_stays_in_front_of_its_half() {
        use alacritty_terminal::term::cell::Flags;
        // Hebrew, a wide emoji (neutral, so it joins the run) and its half,
        // then Hebrew again.
        let mut r = row("א😀 ב");
        r[1].flags = Flags::WIDE_CHAR;
        r[2].flags = Flags::WIDE_CHAR_SPACER;
        let o = order(&r).unwrap();
        let wide = o.cells.iter().position(|&l| l == 1).unwrap();
        assert_eq!(o.cells[wide + 1], 2, "{:?}", o.cells);
    }
}
