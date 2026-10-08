//! Reading the grid: cells, the cursor, modes, selection and search.


use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::search::RegexSearch;
use alacritty_terminal::term::Term;


#[allow(unused_imports)]
use crate::{keys::*, log::*, osc::*, shell::*, terminal::*};

/// A cell of the visible grid as a point in the whole buffer, which is where
/// the scrollback lives above line zero.
pub fn point_at<T: EventListener>(term: &Term<T>, (col, line): (usize, usize)) -> Point {
    let offset = term.grid().display_offset() as i32;
    Point::new(Line(line as i32 - offset), Column(col))
}

/// Begin a drag selection at a cell, or carry one on to it.
///
/// `right_half` is which half of the cell the pointer is in, and it is not
/// cosmetic: alacritty settles a range by ordering the two anchors and then
/// dropping the first cell if that anchor says `Right` and the last if it says
/// `Left`. Fixing the sides -- `Left` to start, `Right` to extend -- is
/// therefore right only while the drag runs left to right. Drag the other way
/// and the ordering swaps them, so a cell goes at each end, which reads as the
/// first character of the line quietly refusing to be copied.
///
/// Taking the side from the pointer is what every terminal does, and it comes
/// out right both ways round: the outer half of whichever cell the drag began
/// on faces away from the selection, and so does the one it ended on.
pub fn select_at<T: EventListener>(
    term: &mut Term<T>,
    cell: (usize, usize),
    right_half: bool,
    start: bool,
) {
    let point = point_at(term, cell);
    let side = if right_half { Side::Right } else { Side::Left };
    match start {
        true => term.selection = Some(Selection::new(SelectionType::Simple, point, side)),
        false => {
            if let Some(sel) = term.selection.as_mut() {
                sel.update(point, side);
            }
        }
    }
}

/// The whole buffer as text -- scrollback, then screen -- a line per line
/// as the program wrote it: a row the terminal wrapped is joined to the
/// next, a wide character's spacer left out, the blanks at a line's end and
/// below the last line cut.
pub fn all_text<T: EventListener>(term: &Term<T>) -> String {
    use alacritty_terminal::term::cell::Flags;
    let grid = term.grid();
    let top = -(grid.history_size() as i32);
    let bottom = grid.screen_lines() as i32 - 1;
    let cols = grid.columns();
    let mut out = String::new();
    let mut line = String::new();
    for l in top..=bottom {
        let row = &grid[Line(l)];
        for c in 0..cols {
            let cell = &row[Column(c)];
            if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                line.push(cell.c);
            }
        }
        if !row[Column(cols - 1)].flags.contains(Flags::WRAPLINE) {
            out.push_str(line.trim_end());
            out.push('\n');
            line.clear();
        }
    }
    out.push_str(line.trim_end());
    let kept = out.trim_end().len();
    out.truncate(kept);
    out.push('\n');
    out
}

/// The lines of the whole buffer -- scrollback and screen -- holding
/// `needle`, case aside: newest first, at most `max`. Each is its line (the
/// scrollback above zero, as [`point_at`] has it), the cell the match starts
/// at, and the line's text.
pub fn find_lines<T: EventListener>(term: &Term<T>, needle: &str, max: usize) -> Vec<(i32, usize, String)> {
    let want = needle.to_lowercase();
    let mut out = Vec::new();
    if want.is_empty() {
        return out;
    }
    let grid = term.grid();
    let top = -(grid.history_size() as i32);
    let bottom = grid.screen_lines() as i32 - 1;
    let cols = grid.columns();
    for l in (top..=bottom).rev() {
        let row = &grid[Line(l)];
        // A wide character's spacer is a cell too, so a char is a cell.
        let text: String = (0..cols).map(|c| row[Column(c)].c).collect();
        // Lowered a cell at a time, each lowered character remembering its
        // cell: a character that lowers to two (`İ`) would otherwise put
        // every later column one off (the source review, 2026-10-07).
        let mut lower = String::new();
        let mut cell_of = Vec::new();
        for (cell, c) in text.chars().enumerate() {
            for l in c.to_lowercase() {
                lower.push(l);
                cell_of.push(cell);
            }
        }
        if let Some(byte) = lower.find(&want) {
            let col = cell_of.get(lower[..byte].chars().count()).copied().unwrap_or(0);
            out.push((l, col, text.trim_end().to_owned()));
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

/// Put `line` on screen and select `len` cells of it from `col`: where a
/// search across sessions found something.
pub fn reveal<T: EventListener>(term: &mut Term<T>, line: i32, col: usize, len: usize) {
    let grid = term.grid();
    let line = line.clamp(-(grid.history_size() as i32), grid.screen_lines() as i32 - 1);
    let want = (-line).max(0);
    let now = grid.display_offset() as i32;
    term.scroll_display(Scroll::Delta(want - now));
    let last = (col + len.max(1) - 1).min(term.grid().columns() - 1);
    let mut sel = Selection::new(SelectionType::Simple, Point::new(Line(line), Column(col)), Side::Left);
    sel.update(Point::new(Line(line), Column(last)), Side::Right);
    term.selection = Some(sel);
}

/// Where a search with nothing to carry on from begins.
///
/// Backwards starts at the bottom right of what is on screen, so the visible
/// rows are searched before the history above them. Starting at the top left
/// instead -- which is what the view's own first line is -- steps away from
/// every row the reader can see *and* every row above it, and `<C-S-f>` said
/// there was no match for text that was plainly on the screen.
pub fn search_origin<T: EventListener>(term: &Term<T>, back: bool) -> Point {
    let grid = term.grid();
    let offset = grid.display_offset() as i32;
    match back {
        true => Point::new(
            Line(grid.screen_lines() as i32 - 1 - offset),
            Column(grid.columns().saturating_sub(1)),
        ),
        false => Point::new(Line(-offset), Column(0)),
    }
}

/// Where the cursor is, in cells, for the renderer to draw over.
/// The row is where the cursor is *on screen*, which is not where it is in the
/// buffer once the view has been scrolled back: the caller draws nothing when
/// the row falls past the last line, so the cursor leaves with the rows it
/// belongs to instead of hanging on the scrollback at its old height.
pub fn cursor_cell<T: EventListener>(term: &Term<T>) -> (usize, usize) {
    let grid = term.grid();
    let p = grid.cursor.point;
    (p.column.0, (p.line.0.max(0) as usize).saturating_add(grid.display_offset()))
}

/// Whether the arrows should be sent as SS3 rather than CSI.
pub fn app_cursor<T: EventListener>(term: &Term<T>) -> bool {
    use alacritty_terminal::term::TermMode;
    term.mode().contains(TermMode::APP_CURSOR)
}

/// Whether the program has asked to be told about the mouse, and in which
/// encoding. nvim asks at startup (`\e[?1002h\e[?1006h`), as do htop, tmux and
/// most other full-screen programs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MouseReport {
    #[default]
    Off,
    /// `\e[<b;x;yM` (mode 1006): any column, and what everything current asks for.
    Sgr,
    /// `\e[M` and three bytes, the original X10 form. Columns past 223 cannot
    /// be said in it, so they are said as 223.
    Plain,
}

pub fn mouse_report<T: EventListener>(term: &Term<T>) -> MouseReport {
    use alacritty_terminal::term::TermMode;
    let mode = term.mode();
    match (mode.intersects(TermMode::MOUSE_MODE), mode.contains(TermMode::SGR_MOUSE)) {
        (false, _) => MouseReport::Off,
        (true, true) => MouseReport::Sgr,
        (true, false) => MouseReport::Plain,
    }
}

/// One notch of the wheel as the program asked to hear it: buttons 64 (up)
/// and 65 (down) at the cell under the pointer, counted from zero here and
/// from one on the wire. A wheel has no release, so there is no `m` form.
pub fn wheel_report(up: bool, col: usize, line: usize, how: MouseReport) -> Vec<u8> {
    let button = if up { 64 } else { 65 };
    match how {
        MouseReport::Off => Vec::new(),
        MouseReport::Sgr => format!("\x1b[<{button};{};{}M", col + 1, line + 1).into_bytes(),
        MouseReport::Plain => {
            let at = |n: usize| (32 + 1 + n.min(222)) as u8;
            vec![0x1b, b'[', b'M', 32 + button, at(col), at(line)]
        }
    }
}

/// Whether a full-screen program is drawing: `nvim`, `less`, `htop` and the
/// rest switch to the alternate screen on the way in and back out on the way
/// out.
///
/// The alternate grid is built with a scrollback of zero lines — see
/// `Grid::new(num_lines, num_cols, 0)` in alacritty — so while this is true
/// there is nothing above the viewport to scroll to. That makes it the right
/// question to ask before the pane keeps a scrolling key or a wheel turn for
/// itself: the answer decides between moving a scrollback that exists and
/// handing the gesture to the program that owns the screen.
pub fn alt_screen<T: EventListener>(term: &Term<T>) -> bool {
    use alacritty_terminal::term::TermMode;
    term.mode().contains(TermMode::ALT_SCREEN)
}

/// `Alt` held with an ordinary character, in the meta-prefix form every shell
/// and readline expects: `ESC` then the character.
///
/// The same convention [`control_code`] already uses for `Alt` with a control
/// chord. Without this, `Alt`+letter reached the pane as a key with no bytes
/// behind it and was dropped on the floor, so `Alt-b` and `Alt-f` never moved
/// readline's cursor.
pub fn meta_char(c: char) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + c.len_utf8());
    out.push(0x1b);
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    out
}

/// The visible grid as rows of cells, for a renderer that cannot hold the
/// lock while it paints.
pub fn snapshot<T: EventListener>(term: &Term<T>) -> Vec<Vec<CellView>> {
    let grid = term.grid();
    let lines = grid.screen_lines();
    let cols = grid.columns();
    // Indexing by `Line` is relative to the active area, where line zero is the
    // top of the screen and the scrollback is above it at negative lines --
    // `display_offset` is not applied for you. Copying `0..lines` therefore
    // returned the same rows however far back the view had been scrolled: the
    // keys moved the offset, the badge read it back and said "16 lines back",
    // and the screen did not move. The same subtraction is in `point`, which
    // has always had to do this to turn a click into a buffer position.
    let offset = grid.display_offset() as i32;
    let sel = term.selection.as_ref().and_then(|s| s.to_range(term));
    let mut out = Vec::with_capacity(lines);
    for l in 0..lines {
        let line = Line(l as i32 - offset);
        let mut row = Vec::with_capacity(cols);
        for c in 0..cols {
            let cell = &grid[line][Column(c)];
            let at = Point::new(line, Column(c));
            row.push(CellView {
                c: cell.c,
                fg: cell.fg,
                bg: cell.bg,
                flags: cell.flags,
                selected: sel.is_some_and(|r| r.contains(at)),
            });
        }
        out.push(row);
    }
    out
}

/// One cell, copied out from under the lock.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellView {
    pub c: char,
    pub fg: alacritty_terminal::vte::ansi::Color,
    pub bg: alacritty_terminal::vte::ansi::Color,
    pub flags: alacritty_terminal::term::cell::Flags,
    /// Inside the drag, or inside what a search just found. Both set the same
    /// selection, so both are drawn the same way, and until v0.20.4 neither
    /// was drawn at all -- a drag copied text with no sign of what it took,
    /// and a search that worked was indistinguishable from one that did not.
    pub selected: bool,
}

/// Plain text as a pattern for [`Terminal::search`](crate::Terminal::search):
/// every character the regex syntax gives a meaning to escaped. Lower case
/// matches either case, as alacritty's search has it (smart case).
pub fn plain_pattern(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// One step of a scrollback search: the next match of `needle` from the last
/// one, `found`, in the direction asked, selected and scrolled onto the screen.
/// A free function over the grid so that the walk can be tested without a PTY.
pub(crate) fn search_in(term: &mut Term<Proxy>, found: &mut Option<(Point, Point)>, needle: &str, back: bool) -> Option<bool> {
    let Ok(mut re) = RegexSearch::new(needle) else { return None };
    // From just past the last match *in the direction of this search*, so a
    // repeat walks the matches rather than finding the same one. The match's
    // two ends are kept rather than one stepped-past point: that point was
    // stepped the way the *last* search went, so the first press after turning
    // round started beside the current match and found it again -- `<C-S-b>`
    // after `<C-S-n>` did nothing once (#93, 1.9g).
    let origin = match *found {
        Some((start, _)) if back => start.sub(&*term, Boundary::Grid, 1),
        Some((_, end)) => end.add(&*term, Boundary::Grid, 1),
        None => search_origin(term, back),
    };
    let dir = if back { Direction::Left } else { Direction::Right };
    let Some(m) = term.search_next(&mut re, origin, dir, Side::Left, None) else {
        // Wrap: a search that runs off the end starts again.
        *found = None;
        return None;
    };
    let hit = *m.start();
    // alacritty wraps by itself, so the only sign is a match on the wrong side
    // of the last one: it went from 433 to "442 lines back" without a word (#98).
    let wrapped = found.is_some_and(|(last, _)| if back { hit > last } else { hit < last });
    // Put the line holding the match on screen.
    let want = (-hit.line.0).max(0);
    let now = term.grid().display_offset() as i32;
    term.scroll_display(Scroll::Delta(want - now));
    term.selection = Some(Selection::new(SelectionType::Simple, hit, Side::Left));
    if let Some(sel) = term.selection.as_mut() {
        sel.update(*m.end(), Side::Right);
    }
    *found = Some((hit, *m.end()));
    Some(wrapped)
}

/// The buffer's lines where a prompt starts, oldest first: those whose
/// cells carry the link [`PromptLinks`](crate::osc) puts after OSC 133 `A`.
pub fn prompt_lines<T: EventListener>(term: &Term<T>) -> Vec<i32> {
    let grid = term.grid();
    let top = -(grid.history_size() as i32);
    (top..grid.screen_lines() as i32)
        .filter(|&line| {
            let row = &grid[Line(line)];
            (0..grid.columns()).any(|col| row[Column(col)].hyperlink().is_some_and(|h| h.uri() == crate::osc::PROMPT_LINK))
        })
        .collect()
}

/// Scroll to the prompt before the view's top line (`back`) or after it,
/// putting it at the top; past the last one, down to the bottom. False when
/// there was nowhere to go.
pub fn jump_prompt<T: EventListener>(term: &mut Term<T>, back: bool) -> bool {
    let lines = prompt_lines(term);
    let grid = term.grid();
    let now = grid.display_offset() as i32;
    let top = -now;
    let target = match back {
        true => lines.iter().rev().find(|&&l| l < top).copied(),
        false => lines.iter().find(|&&l| l > top).copied(),
    };
    // A line on the screen as it is at the bottom cannot go to the top.
    let want = match target {
        Some(line) => (-line).max(0),
        None if back => return false,
        None => 0,
    };
    if want == now {
        return false;
    }
    term.scroll_display(Scroll::Delta(want - now));
    true
}
