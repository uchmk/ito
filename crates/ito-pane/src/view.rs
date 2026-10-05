//! Drawing the pane with egui (the `egui` feature).
//!
//! One glyph per cell, painted the way filer paints its file list: only what
//! is on screen is touched, and the font is the app's own so the pane looks
//! like part of the program. The grid is copied out from under the lock first
//! -- the PTY reader thread wants it back, and laying out text takes longer
//! than copying a screenful of cells.
//!
//! The app hands in its colors as a [`Palette`] and gets back what only it can
//! do as a [`Shown`]: take the keys, put text on the clipboard, read the
//! clipboard for a paste. Everything else -- the cells, the cursor, selecting,
//! the wheel -- happens here.

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Stroke, Ui, Vec2};

use crate::{Mods, MouseReport, Pane, Size, Special};

/// The colors the pane is drawn in. The sixteen named ANSI colors other than
/// these are the pane's own.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// The pane's background, and what a program's default background is.
    pub bg: Color32,
    /// Text, and a program's default foreground.
    pub fg: Color32,
    /// Bright black, and the outline of the cursor when the pane is not focused.
    pub fg_dim: Color32,
    /// Behind selected cells (a drag, or what a search found).
    pub selection: Color32,
    /// The focused cursor, and the "lines back" note.
    pub cursor: Color32,
    /// Text drawn on top of `cursor`.
    pub on_cursor: Color32,
}

/// filer's default theme, so a pane with no palette of its own still reads.
impl Default for Palette {
    fn default() -> Self {
        Self {
            bg: Color32::from_rgb(0x1b, 0x1e, 0x24),
            fg: Color32::from_rgb(0xc8, 0xcd, 0xd8),
            fg_dim: Color32::from_rgb(0x79, 0x80, 0x90),
            selection: Color32::from_rgb(0x2f, 0x4a, 0x6b),
            cursor: Color32::from_rgb(0x6f, 0xd0, 0xd0),
            on_cursor: Color32::from_rgb(0x16, 0x18, 0x1d),
        }
    }
}

/// What the view keeps between frames.
#[derive(Clone, Copy, Debug, Default)]
pub struct ViewState {
    /// The wheel's part-rows not yet scrolled; see [`wheel_whole`].
    pub scroll_rows: f32,
}

/// How the pane is to be drawn this frame.
#[derive(Clone, Copy, Debug)]
pub struct ViewOptions {
    /// The pane has the keys: the cursor is filled rather than outlined.
    pub focused: bool,
    /// The wheel is the pane's when the pointer is over it. An app turns this
    /// off while a panel over the pane owns the wheel.
    pub wheel: bool,
}

/// What happened in the pane that the app has to act on.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    /// The pane was clicked, dragged in or right-clicked: give it the keys.
    pub focus: bool,
    /// A selection was finished: put this on the clipboard.
    pub copy: Option<String>,
    /// A right-click: read the clipboard and [`Pane::paste`] it.
    pub paste: bool,
}

/// How many cells fit, given the space and the font.
pub fn fit(rect: Rect, cell_w: f32, row_h: f32) -> Size {
    Size::new((rect.width() / cell_w).floor() as usize, (rect.height() / row_h).floor() as usize)
}

/// How far the view travels for a pixel of wheel movement. One notch of a
/// Windows wheel reaches egui as about 50 points, so at this rate a notch is
/// close to the three lines every other terminal moves.
const LINES_PER_PIXEL: f32 = 1.0;

/// The wheel this frame in notches, read from the raw events rather than
/// egui's smoothed delta. A program that asked for the mouse expects one
/// report per notch, as every terminal sends; the smoothed delta spreads a
/// notch over several frames, and three notches came out as five reports
/// (#107). A trackpad, which reports points, counts a notch per 50 of them --
/// what one notch of a Windows wheel reaches egui as.
fn notches(events: &[egui::Event]) -> f32 {
    events
        .iter()
        .map(|e| match e {
            egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => delta.y,
            egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Page, delta, .. } => delta.y * 3.0,
            egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, .. } => delta.y / 50.0,
            _ => 0.0,
        })
        .sum()
}

/// Draw `term` (or, with none, the empty pane) in `rect`, in font `f` with
/// rows `row_h` apart, and handle the pointer over it: selecting, the wheel,
/// right-click to paste. The grid is resized to fit.
#[allow(clippy::too_many_arguments)]
pub fn show<P: Pane + ?Sized>(
    ui: &mut Ui,
    term: Option<&mut P>,
    state: &mut ViewState,
    rect: Rect,
    f: &FontId,
    row_h: f32,
    pal: &Palette,
    opts: ViewOptions,
) -> Shown {
    let mut shown = Shown::default();
    let mut focused = opts.focused;
    let painter = ui.painter_at(rect);
    // A monospace font gives every cell the same width, so one measurement
    // does for the whole grid.
    let cell_w = painter.layout_no_wrap("M".into(), f.clone(), pal.fg).size().x.max(1.0);
    painter.rect_filled(rect, CornerRadius::ZERO, pal.bg);

    let inner = rect.shrink2(Vec2::new(6.0, 4.0));
    let size = fit(inner, cell_w, row_h);

    let id = ui.id().with("term-pane");
    let resp = ui.interact(rect, id, egui::Sense::click_and_drag());
    let pointer = resp.interact_pointer_pos();
    let over = ui.rect_contains_pointer(rect);
    // A panel over the pane owns the wheel; the app says so.
    let (wheel, notched) = match over && opts.wheel {
        true => ui.ctx().input(|i| (i.smooth_scroll_delta.y, notches(&i.events))),
        false => (0.0, 0.0),
    };
    // Select to copy, right-click to paste -- the pair a terminal has always
    // had. Reading the clipboard is the app's (it reports a clipboard that will
    // not open); what it reads goes to `Terminal::paste`, which decides whether
    // the bracketed-paste markers go on. Where they do, a multi-line clipboard
    // waits in the line editor instead of running itself.
    shown.paste = resp.secondary_clicked();
    let press = ui.ctx().input(|i| i.pointer.press_origin());
    let hover = ui.ctx().input(|i| i.pointer.hover_pos());
    if resp.clicked() || resp.drag_started() || resp.secondary_clicked() {
        focused = true;
        shown.focus = true;
    }

    let Some(term) = term else { return shown };
    term.resize(size, (cell_w.round() as u16, row_h.round() as u16));
    // Out from under the lock before any laying out happens.
    let crate::Screen { rows, cursor, app_cursor, alt_screen, mouse } = term.screen();

    for (y, row) in rows.iter().enumerate() {
        let top = inner.top() + y as f32 * row_h;
        if top > inner.bottom() {
            break;
        }
        // Backgrounds first, run by run: one rectangle for a stretch of the
        // same color beats one per cell.
        let mut run: Option<(usize, Color32)> = None;
        for (x, cell) in row.iter().enumerate() {
            // A selected cell takes the same background the list gives the row
            // under the cursor: already proven to carry text in this palette,
            // and already the colour that means "this one" everywhere else.
            let bg = match cell.selected {
                true => pal.selection,
                false => color(cell.bg, pal, true),
            };
            match run {
                Some((_, c)) if c == bg => {}
                Some((start, c)) => {
                    fill(&painter, inner, start, x, top, cell_w, row_h, c, pal);
                    run = Some((x, bg));
                }
                None => run = Some((x, bg)),
            }
        }
        if let Some((start, c)) = run {
            fill(&painter, inner, start, row.len(), top, cell_w, row_h, c, pal);
        }

        for (x, cell) in row.iter().enumerate() {
            if cell.c == ' ' || cell.c == '\0' {
                continue;
            }
            let mut fg = color(cell.fg, pal, false);
            if cell.flags.contains(Flags::DIM) {
                fg = fg.linear_multiply(0.6);
            }
            if cell.flags.contains(Flags::INVERSE) {
                fg = color(cell.bg, pal, true);
            }
            painter.text(
                egui::pos2(inner.left() + x as f32 * cell_w, top),
                Align2::LEFT_TOP,
                cell.c,
                f.clone(),
                fg,
            );
        }
    }

    // The cursor: filled while the pane has the keys, outlined when it does
    // not, which is the same language the split panes use.
    let (cx, cy) = cursor;
    if cy < size.lines && cx < size.cols {
        let at = Rect::from_min_size(
            egui::pos2(inner.left() + cx as f32 * cell_w, inner.top() + cy as f32 * row_h),
            Vec2::new(cell_w, row_h),
        );
        if focused {
            painter.rect_filled(at, CornerRadius::ZERO, pal.cursor);
            if let Some(cell) = rows.get(cy).and_then(|r| r.get(cx)) {
                if cell.c != ' ' {
                    painter.text(at.left_top(), Align2::LEFT_TOP, cell.c, f.clone(), pal.on_cursor);
                }
            }
        } else {
            painter.rect_stroke(
                at,
                CornerRadius::ZERO,
                Stroke::new(1.0, pal.fg_dim),
                egui::StrokeKind::Inside,
            );
        }
    }

    // Somewhere above the bottom, which is easy to forget being in.
    let back = term.scrolled_back();
    if back > 0 {
        let note = format!(" {back} lines back — <S-End> to return ");
        let g = painter.layout_no_wrap(note, f.clone(), pal.on_cursor);
        let at = egui::pos2(inner.right() - g.size().x - 6.0, inner.top() + 2.0);
        painter.rect_filled(
            Rect::from_min_size(at, g.size()),
            CornerRadius::same(3),
            pal.cursor,
        );
        painter.galley(at, g, pal.on_cursor);
    }

    // Which cell the pointer is over, and which half of it. The half decides
    // whether that cell ends up inside the selection when the drag turns out
    // to run the other way; see `crate::select_at`.
    let cell_at = |p: egui::Pos2| {
        let x = (p.x - inner.left()) / cell_w;
        let col = x.floor().max(0.0);
        let line = ((p.y - inner.top()) / row_h).floor().max(0.0) as usize;
        let cell = (col as usize).min(size.cols.saturating_sub(1));
        (cell, line.min(size.lines.saturating_sub(1)), x - col >= 0.5)
    };
    if let Some(p) = pointer {
        if resp.double_clicked() {
            let (col, line, _) = cell_at(p);
            term.select_word((col, line));
        } else if resp.drag_started() {
            // Where the button went down, not where the pointer is now: egui
            // calls a drag started only once it has moved past a threshold,
            // and a few points of that is most of a cell this narrow. Reading
            // the live position anchored the selection one cell along, so a
            // drag begun on the first character dropped it -- which is why it
            // had to be begun slightly to its left to keep it.
            let from = press.unwrap_or(p);
            let (col, line, right) = cell_at(from);
            term.select((col, line), right, true);
        } else if resp.dragged() {
            let (col, line, right) = cell_at(p);
            term.select((col, line), right, false);
        } else if resp.clicked() {
            // A plain click puts the caret nowhere; it just clears what was
            // selected, the way a terminal does.
            term.clear_selection();
        }
    }
    // Letting go of a selection copies it, which is what a terminal means by
    // selecting: there is no other step.
    if resp.drag_stopped() || resp.double_clicked() {
        shown.copy = term.selection();
    }
    // The wheel walks the scrollback rather than the file list under it --
    // except where a program is the thing being scrolled. One that asked for
    // the mouse (nvim, htop, tmux) is told about the wheel as the mouse, at
    // the cell under the pointer, and scrolls its view without moving its
    // cursor (Q28). One that did not but owns the alternate screen, where
    // there is no scrollback, gets arrows: what every terminal does, and the
    // only way a pager like `less` can be scrolled with the wheel at all.
    let rows = match mouse {
        MouseReport::Off => wheel * LINES_PER_PIXEL / row_h,
        _ => notched,
    };
    let whole = wheel_whole(&mut state.scroll_rows, rows);
    if whole != 0 {
        if mouse != MouseReport::Off {
            let (col, line, _) = cell_at(hover.unwrap_or(inner.center()));
            let one = crate::wheel_report(whole > 0, col, line, mouse);
            term.send(one.repeat(whole.unsigned_abs() as usize));
        } else if alt_screen {
            let key = if whole > 0 { Special::Up } else { Special::Down };
            let bytes = match term.win32_input() {
                true => crate::special_record(key, Mods::default()),
                false => crate::encode(key, Mods::default(), app_cursor),
            };
            let mut out = Vec::with_capacity(bytes.len() * whole.unsigned_abs() as usize);
            for _ in 0..whole.unsigned_abs() {
                out.extend_from_slice(&bytes);
            }
            term.send(out);
        } else {
            term.scroll(Scroll::Delta(whole as i32));
        }
    }
    shown
}

/// Turn wheel movement, measured in rows, into whole rows — keeping the part
/// that is not yet one.
///
/// A frame's share of a notch is usually a fraction of a row, and truncating
/// each frame on its own threw that away every time: only a frame that cleared
/// a whole row on its own moved anything, which is a wheel you have to spin
/// hard for one or two lines. Every surface that scrolls by rows needs this,
/// and each keeps its own remainder — one shared between them would jump when
/// the pointer crossed from one to another mid-turn.
///
/// Turning the other way throws the remainder away. What was owed was owed in
/// the old direction; carried into the new one it ate part of the first notch,
/// so one notch up moved a row and one notch down moved none (40.8, #100).
pub fn wheel_whole(acc: &mut f32, rows: f32) -> i64 {
    if rows * *acc < 0.0 {
        *acc = 0.0;
    }
    *acc += rows;
    let whole = acc.trunc();
    *acc -= whole;
    whole as i64
}

#[allow(clippy::too_many_arguments)]
fn fill(
    painter: &egui::Painter,
    inner: Rect,
    from: usize,
    to: usize,
    top: f32,
    cell_w: f32,
    row_h: f32,
    c: Color32,
    pal: &Palette,
) {
    if c == pal.bg || to <= from {
        return;
    }
    painter.rect_filled(
        Rect::from_min_size(
            egui::pos2(inner.left() + from as f32 * cell_w, top),
            Vec2::new((to - from) as f32 * cell_w, row_h),
        ),
        CornerRadius::ZERO,
        c,
    );
}

/// An ANSI color as something to paint with.
///
/// The sixteen named colors come from the palette so the pane matches the rest
/// of the window; the 256-color cube and the true-color values are what the
/// program asked for and are used as given.
fn color(c: AnsiColor, pal: &Palette, is_bg: bool) -> Color32 {
    match c {
        AnsiColor::Named(n) => named(n, pal, is_bg),
        AnsiColor::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(i) => indexed(i, pal, is_bg),
    }
}

fn named(n: NamedColor, pal: &Palette, is_bg: bool) -> Color32 {
    use NamedColor::*;
    match n {
        Background => pal.bg,
        Foreground => pal.fg,
        Cursor => pal.cursor,
        Black => Color32::from_rgb(0x1b, 0x1e, 0x24),
        Red => Color32::from_rgb(0xf0, 0x71, 0x78),
        Green => Color32::from_rgb(0x8e, 0xd0, 0x8e),
        Yellow => Color32::from_rgb(0xe8, 0xc8, 0x7a),
        Blue => Color32::from_rgb(0x7a, 0xb8, 0xf5),
        Magenta => Color32::from_rgb(0xc9, 0x9c, 0xf0),
        Cyan => Color32::from_rgb(0x6f, 0xd0, 0xd0),
        White => pal.fg,
        BrightBlack => pal.fg_dim,
        BrightRed => Color32::from_rgb(0xff, 0x96, 0x9c),
        BrightGreen => Color32::from_rgb(0xb0, 0xe8, 0xb0),
        BrightYellow => Color32::from_rgb(0xff, 0xe0, 0x9c),
        BrightBlue => Color32::from_rgb(0x9c, 0xd0, 0xff),
        BrightMagenta => Color32::from_rgb(0xe0, 0xbc, 0xff),
        BrightCyan => Color32::from_rgb(0x9c, 0xe8, 0xe8),
        BrightWhite => Color32::WHITE,
        _ => {
            if is_bg {
                pal.bg
            } else {
                pal.fg
            }
        }
    }
}

/// The xterm 256-color palette: sixteen named, a 6×6×6 cube, then a grey ramp.
fn indexed(i: u8, pal: &Palette, is_bg: bool) -> Color32 {
    match i {
        0..=15 => {
            let n = [
                NamedColor::Black,
                NamedColor::Red,
                NamedColor::Green,
                NamedColor::Yellow,
                NamedColor::Blue,
                NamedColor::Magenta,
                NamedColor::Cyan,
                NamedColor::White,
                NamedColor::BrightBlack,
                NamedColor::BrightRed,
                NamedColor::BrightGreen,
                NamedColor::BrightYellow,
                NamedColor::BrightBlue,
                NamedColor::BrightMagenta,
                NamedColor::BrightCyan,
                NamedColor::BrightWhite,
            ][i as usize];
            named(n, pal, is_bg)
        }
        16..=231 => {
            let i = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(step(i / 36), step((i % 36) / 6), step(i % 6))
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            Color32::from_gray(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_color_cube_lands_where_xterm_puts_it() {
        let pal = Palette::default();
        // The corners of the 6x6x6 cube.
        assert_eq!(indexed(16, &pal, false), Color32::from_rgb(0, 0, 0));
        assert_eq!(indexed(231, &pal, false), Color32::from_rgb(255, 255, 255));
        // Pure red is the first step of the red axis, not 0xff.
        assert_eq!(indexed(196, &pal, false), Color32::from_rgb(255, 0, 0));
        // The grey ramp at both ends.
        assert_eq!(indexed(232, &pal, false), Color32::from_gray(8));
        assert_eq!(indexed(255, &pal, false), Color32::from_gray(238));
        // The first sixteen come from the theme, so the pane matches.
        assert_eq!(indexed(7, &pal, false), pal.fg);
    }

    #[test]
    fn the_grid_is_measured_in_whole_cells() {
        let r = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(101.0, 55.0));
        let size = fit(r, 10.0, 20.0);
        assert_eq!((size.cols, size.lines), (10, 2), "a part-cell is not a cell");
        // A pane too small to hold anything still has to answer something a
        // grid can divide by.
        let tiny = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(1.0, 1.0));
        let size = fit(tiny, 10.0, 20.0);
        assert_eq!((size.cols, size.lines), (1, 1));
    }
}

#[cfg(test)]
mod wheel_notches {
    use super::notches;
    use egui::{Event, Modifiers, MouseWheelUnit, TouchPhase, Vec2};

    fn wheel(unit: MouseWheelUnit, y: f32) -> Event {
        Event::MouseWheel { unit, delta: Vec2::new(0.0, y), phase: TouchPhase::Move, modifiers: Modifiers::NONE }
    }

    /// #107: one notch, one report -- counted from the raw events,
    /// not from the smoothed delta that spread three notches into five.
    #[test]
    fn a_notch_is_one() {
        assert_eq!(notches(&[wheel(MouseWheelUnit::Line, 1.0)]), 1.0);
        assert_eq!(notches(&[wheel(MouseWheelUnit::Line, -1.0), wheel(MouseWheelUnit::Line, -1.0)]), -2.0);
        assert_eq!(notches(&[wheel(MouseWheelUnit::Point, 50.0)]), 1.0);
        assert_eq!(notches(&[Event::WindowFocused(true)]), 0.0);
    }
}

/// The wheel's arithmetic, from filer's `ui/mod.rs`.
#[cfg(test)]
mod wheel_rows {
    use super::wheel_whole;

    /// Turning the wheel has to move the view by what was turned.
    ///
    /// Each frame gets a fraction of a notch, and truncating each one on its
    /// own discards it: twenty frames of "not quite a row" used to move
    /// nothing at all. That is the whole bug — a wheel that had to be spun
    /// hard for one or two lines — and it was in three places, so the
    /// arithmetic lives in one.
    #[test]
    fn a_notch_spread_over_frames_still_arrives() {
        let mut acc = 0.0;
        // Three rows' worth, delivered a fifth of a row at a time.
        let moved: i64 = (0..15).map(|_| wheel_whole(&mut acc, 0.2)).sum();
        assert_eq!(moved, 3, "every fifth frame completes a row");

        // Truncating each frame instead is what used to happen.
        let dropped: i64 = (0..15).map(|_| 0.2_f32.trunc() as i64).sum();
        assert_eq!(dropped, 0, "which is why nothing moved");
    }

    /// The remainder is kept, not rounded away, and works both ways.
    #[test]
    fn it_keeps_the_part_that_is_not_yet_a_row() {
        let mut acc = 0.0;
        assert_eq!(wheel_whole(&mut acc, 0.6), 0, "not a row yet");
        assert_eq!(wheel_whole(&mut acc, 0.6), 1, "now it is");
        assert!((acc - 0.2).abs() < 1e-5, "and 0.2 of a row is still owed: {acc}");

        // The other way round works the same.
        let mut acc = 0.0;
        assert_eq!(wheel_whole(&mut acc, -0.6), 0);
        assert_eq!(wheel_whole(&mut acc, -0.6), -1);

        // A whole row at a time is unaffected — the common case must not drift.
        let mut acc = 0.0;
        for _ in 0..10 {
            assert_eq!(wheel_whole(&mut acc, 1.0), 1);
        }
        assert!(acc.abs() < 1e-5, "no drift after ten rows: {acc}");
    }

    /// 40.8: a notch is about 1.6 rows, arriving over several frames. Up one
    /// notch and down one must move the same number of rows each way; carrying
    /// the 0.6 left from the way up into the way down made the second move
    /// nothing at all.
    #[test]
    fn a_change_of_direction_starts_from_nothing() {
        let notch = |acc: &mut f32, dir: f32| -> i64 { (0..8).map(|_| wheel_whole(acc, dir * 0.2)).sum() };
        let mut acc = 0.0;
        let up = notch(&mut acc, 1.0);
        let down = notch(&mut acc, -1.0);
        assert_eq!(up, 1);
        assert_eq!(down, -1, "the same one row back down, not none");
        assert_eq!(notch(&mut acc, 1.0), 1, "and up again");
    }
}
