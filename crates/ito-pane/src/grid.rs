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

/// Whether the program asked to hear when the pane gains and loses the keys
/// (DECSET 1004): `\e[I` and `\e[O`. vim and nvim ask, to reread files changed
/// meanwhile and to drop the cursor's shape; so do tmux and the TUIs that
/// dim themselves when not in use.
pub fn focus_report<T: EventListener>(term: &Term<T>) -> bool {
    use alacritty_terminal::term::TermMode;
    term.mode().contains(TermMode::FOCUS_IN_OUT)
}

/// The kitty keyboard enhancements in force (`kitty::DISAMBIGUATE` and the
/// rest), 0 while the program asked for none.
pub fn kitty_flags<T: EventListener>(term: &Term<T>) -> u8 {
    use alacritty_terminal::term::TermMode as M;
    let mode = term.mode();
    [M::DISAMBIGUATE_ESC_CODES, M::REPORT_EVENT_TYPES, M::REPORT_ALTERNATE_KEYS, M::REPORT_ALL_KEYS_AS_ESC, M::REPORT_ASSOCIATED_TEXT]
        .iter()
        .enumerate()
        .filter(|(_, m)| mode.contains(**m))
        .fold(0, |acc, (i, _)| acc | 1 << i)
}

/// The bytes that tell the program the pane gained (`true`) or lost the keys.
pub fn focus_bytes(gained: bool) -> Vec<u8> {
    if gained { b"\x1b[I".to_vec() } else { b"\x1b[O".to_vec() }
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
                ul: cell.underline_color(),
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
    /// The colour of the cell's underline (SGR 58), when a program gave it
    /// one; without, the line takes the text's colour.
    pub ul: Option<alacritty_terminal::vte::ansi::Color>,
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
    (top..grid.screen_lines() as i32).filter(|&line| matches!(mark_on(term, line), Some(Mark::Prompt(_)))).collect()
}

/// What tsumugi marked a row as (see [`PromptLinks`](crate::osc)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    /// A prompt starts here, with the exit code of the command before it.
    Prompt(Option<i32>),
    /// A command's output starts here.
    Output,
}

fn mark_on<T: EventListener>(term: &Term<T>, line: i32) -> Option<Mark> {
    let grid = term.grid();
    let row = &grid[Line(line)];
    (0..grid.columns()).find_map(|col| {
        let uri = row[Column(col)].hyperlink()?.uri().to_owned();
        if uri.starts_with(crate::osc::PROMPT_LINK) {
            Some(Mark::Prompt(crate::osc::exit_of(&uri)))
        } else {
            (uri == crate::osc::OUTPUT_LINK).then_some(Mark::Output)
        }
    })
}

/// A command and its output as the view shows it (a block, as Warp and
/// iTerm2 have them): from its prompt's row to the row before the next
/// prompt, on `lines` of the view (0 at its top), and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Block {
    pub lines: std::ops::Range<usize>,
    pub exit: i32,
}

/// How far above the view a block's prompt is looked for: a block longer
/// than this has no bar on the rows past it.
const BLOCK_BACK: i32 = 2000;

/// The commands on the screen as shown whose exit code the shell gave
/// (OSC 133 `D;<code>`, read when the next prompt comes): a command still
/// running, or one the shell said nothing of, is none of them.
pub fn blocks<T: EventListener>(term: &Term<T>) -> Vec<Block> {
    let grid = term.grid();
    let offset = grid.display_offset() as i32;
    let (top, bottom) = (-offset, grid.screen_lines() as i32 - offset);
    let oldest = -(grid.history_size() as i32);
    // The prompt the view's first block starts at, above it, if near.
    let above = (oldest.max(top - BLOCK_BACK)..top).rev().find(|&l| matches!(mark_on(term, l), Some(Mark::Prompt(_))));
    let mut starts: Vec<i32> = above.into_iter().collect();
    let mut exits: Vec<Option<i32>> = Vec::new();
    for line in top..grid.screen_lines() as i32 {
        if let Some(Mark::Prompt(exit)) = mark_on(term, line) {
            if !starts.is_empty() {
                exits.push(exit);
            }
            starts.push(line);
            if line >= bottom {
                break;
            }
        }
    }
    let view = |l: i32| (l.clamp(top, bottom) - top) as usize;
    starts
        .windows(2)
        .zip(exits)
        .filter_map(|(w, exit)| Some(Block { lines: view(w[0])..view(w[1]), exit: exit? }))
        .filter(|b| !b.lines.is_empty())
        .collect()
}

/// The output of the last command that has any: what a command still
/// running has written so far (from its OSC 133 `C`), else what is between
/// the last two prompts that have something between them -- from the `C`
/// there, or the row after the prompt where the shell marks none (pwsh).
/// Trailing blank lines are left out; `None` when there is nothing.
pub fn last_output<T: EventListener>(term: &Term<T>) -> Option<String> {
    let grid = term.grid();
    let oldest = -(grid.history_size() as i32);
    let last = grid.screen_lines() as i32 - 1;
    let text = |from: i32, to: i32| {
        if from > to {
            return None;
        }
        let all = term.bounds_to_string(Point::new(Line(from), Column(0)), Point::new(Line(to), grid.last_column()));
        let all = all.trim_end_matches(['\n', '\r', ' ']).to_owned();
        (!all.is_empty()).then_some(all)
    };
    // Bottom up: the rows since the prompt below, and where an output began.
    let (mut below, mut output) = (last + 1, None);
    for line in (oldest..=last).rev() {
        match mark_on(term, line) {
            Some(Mark::Output) => output = Some(line),
            Some(Mark::Prompt(_)) => {
                let running = below > last;
                let from = output.unwrap_or(line + 1);
                // A prompt with nothing run after it yet is not a command.
                if !(running && output.is_none()) {
                    if let Some(t) = text(from, below - 1) {
                        return Some(t);
                    }
                }
                (below, output) = (line, None);
            }
            None => {}
        }
    }
    None
}

/// A link a program put on the screen (OSC 8, as `ls --hyperlink` and
/// `gcc` do): on `line` of the view (0 at its top), over `cells`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Hyperlink {
    pub line: usize,
    pub cells: std::ops::Range<usize>,
    pub uri: String,
}

/// The OSC 8 links on the screen as it is shown, row by row; the marks
/// tsumugi puts on prompts are none of them. A link broken by other cells
/// is one for each piece.
pub fn hyperlinks<T: EventListener>(term: &Term<T>) -> Vec<Hyperlink> {
    let grid = term.grid();
    let offset = grid.display_offset() as i32;
    let mut out: Vec<Hyperlink> = Vec::new();
    for line in 0..grid.screen_lines() {
        let row = &grid[Line(line as i32 - offset)];
        for col in 0..grid.columns() {
            let Some(link) = row[Column(col)].hyperlink() else { continue };
            if crate::osc::is_mark(link.uri()) {
                continue;
            }
            match out.last_mut() {
                Some(last) if last.line == line && last.cells.end == col && last.uri == link.uri() => last.cells.end = col + 1,
                _ => out.push(Hyperlink { line, cells: col..col + 1, uri: link.uri().to_string() }),
            }
        }
    }
    out
}

/// The pictures' top left cells ([`IMAGE_LINK`](crate::IMAGE_LINK)) on
/// the view and up to `above` lines over it: each picture's key, its line
/// (0 the view's top, less above it) and its column.
pub fn placements<T: EventListener>(term: &Term<T>, above: usize) -> Vec<(u64, i32, usize)> {
    let grid = term.grid();
    let offset = grid.display_offset() as i32;
    let oldest = -(grid.history_size() as i32);
    let mut out = Vec::new();
    for line in (-(above as i32)).max(oldest + offset)..grid.screen_lines() as i32 {
        let row = &grid[Line(line - offset)];
        for col in 0..grid.columns() {
            let Some(link) = row[Column(col)].hyperlink() else { continue };
            if let Some(key) = link.uri().strip_prefix(crate::image::IMAGE_LINK).and_then(|k| k.parse().ok()) {
                out.push((key, line, col));
            }
        }
    }
    out
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
