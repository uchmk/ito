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
    /// The sixteen named colours (black, red, … bright white) a theme gives;
    /// `None` keeps filer's.
    pub ansi: Option<[Color32; 16]>,
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
            ansi: None,
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
    /// Letting go of a selection puts it on the clipboard ([`Shown::copy`]);
    /// off, it stays selected until a key copies it.
    pub copy_on_select: bool,
}

/// What happened in the pane that the app has to act on.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    /// The pane was clicked, dragged in or right-clicked: give it the keys.
    pub focus: bool,
    /// A selection was finished: put this on the clipboard.
    pub copy: Option<String>,
    /// A selection was finished, copied or not: what Linux calls the
    /// primary selection, for a middle-click to paste.
    pub selected: bool,
    /// A right-click: read the clipboard and [`Pane::paste`] it.
    pub paste: bool,
    /// A middle-click: paste the primary selection (Linux), or the
    /// clipboard where there is none.
    pub middle: bool,
    /// A click with Ctrl (Cmd on a Mac) on a web address or a path: open it.
    pub open: Option<crate::Link>,
    /// Where the grid's first cell is, and a cell's size: for drawing over
    /// a cell (a copy mode's cursor).
    pub grid: Option<(egui::Pos2, Vec2)>,
}

/// How many cells fit, given the space and the font.
pub fn fit(rect: Rect, cell_w: f32, row_h: f32) -> Size {
    // Both guarded: a row of no height would make `inf` rows (the source
    // review, 2026-10-07).
    Size::new((rect.width() / cell_w.max(1.0)).floor() as usize, (rect.height() / row_h.max(1.0)).floor() as usize)
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

/// The faces a pane's text is drawn in: the regular one, and those for bold
/// and italic text when the app has them (else the regular one stands in).
/// All of the same width per cell.
#[derive(Clone, Debug)]
pub struct Faces {
    pub regular: FontId,
    pub bold: Option<FontId>,
    pub italic: Option<FontId>,
    pub bold_italic: Option<FontId>,
    /// The regular face's file, to draw its ligatures from; none, none.
    pub shaper: Option<std::sync::Arc<crate::liga::Shaper>>,
    /// How the cursor of the pane with the keys is drawn.
    pub cursor: CursorStyle,
}

/// The shape of the focused pane's cursor, and whether it blinks. A pane
/// without the keys always shows an outlined block, so it reads as asleep.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorStyle {
    pub shape: CursorShape,
    pub blink: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
}

impl CursorStyle {
    /// Read from a word such as `block`, `bar-blink` or `underline`; an
    /// unknown shape is a block.
    pub fn from_word(word: &str) -> Self {
        let (shape, blink) = match word.strip_suffix("-blink") {
            Some(s) => (s, true),
            None => (word, false),
        };
        let shape = match shape {
            "bar" => CursorShape::Bar,
            "underline" => CursorShape::Underline,
            _ => CursorShape::Block,
        };
        Self { shape, blink }
    }
}

/// How long the cursor stays on, then off, while it blinks.
const BLINK: f64 = 0.53;

impl Faces {
    pub fn plain(regular: FontId) -> Self {
        Self { regular, bold: None, italic: None, bold_italic: None, shaper: None, cursor: CursorStyle::default() }
    }

    /// The face for a cell's flags.
    fn pick(&self, flags: Flags) -> &FontId {
        let (b, i) = (flags.contains(Flags::BOLD), flags.contains(Flags::ITALIC));
        let face = match (b, i) {
            (true, true) => self.bold_italic.as_ref().or(self.bold.as_ref()).or(self.italic.as_ref()),
            (true, false) => self.bold.as_ref(),
            (false, true) => self.italic.as_ref(),
            (false, false) => None,
        };
        face.unwrap_or(&self.regular)
    }
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
    show_faces(ui, term, state, rect, &Faces::plain(f.clone()), row_h, pal, opts)
}

/// The pictures' textures, by key, and when each was last drawn; one not
/// drawn for a minute is let go.
type Textures = std::collections::HashMap<u64, (egui::TextureHandle, f64)>;

/// A picture's texture, made the first time it is drawn.
fn texture<P: Pane + ?Sized>(ctx: &egui::Context, term: &P, key: u64) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("tsumugi-pane-pictures");
    let now = ctx.input(|i| i.time);
    let mut cache: Textures = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let found = match cache.get_mut(&key) {
        Some((t, used)) => {
            *used = now;
            Some(t.clone())
        }
        // Pixels that do not add up (from across the wire) are not drawn.
        None => term.picture(key).filter(|p| p.rgba.len() == p.width as usize * p.height as usize * 4).map(|p| {
            let image = egui::ColorImage::from_rgba_unmultiplied([p.width as usize, p.height as usize], &p.rgba);
            let t = ctx.load_texture(format!("tsumugi-picture-{key}"), image, egui::TextureOptions::LINEAR);
            cache.insert(key, (t.clone(), now));
            t
        }),
    };
    cache.retain(|_, (_, used)| now - *used < 60.0);
    ctx.data_mut(|d| d.insert_temp(id, cache));
    found
}

/// [`show`] with bold and italic faces; rows taller than the font's own put
/// the text in their middle.
#[allow(clippy::too_many_arguments)]
pub fn show_faces<P: Pane + ?Sized>(
    ui: &mut Ui,
    term: Option<&mut P>,
    state: &mut ViewState,
    rect: Rect,
    faces: &Faces,
    row_h: f32,
    pal: &Palette,
    opts: ViewOptions,
) -> Shown {
    let f = &faces.regular;
    let mut shown = Shown::default();
    let mut focused = opts.focused;
    let painter = ui.painter_at(rect);
    // A monospace font gives every cell the same width, so one measurement
    // does for the whole grid.
    let cell_w = painter.layout_no_wrap("M".into(), f.clone(), pal.fg).size().x.max(1.0);
    painter.rect_filled(rect, CornerRadius::ZERO, pal.bg);

    let inner = rect.shrink2(Vec2::new(6.0, 4.0));
    let size = fit(inner, cell_w, row_h);
    shown.grid = Some((inner.min, Vec2::new(cell_w, row_h)));

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
    shown.middle = resp.middle_clicked();
    let press = ui.ctx().input(|i| i.pointer.press_origin());
    let hover = ui.ctx().input(|i| i.pointer.hover_pos());
    if resp.clicked() || resp.drag_started() || resp.secondary_clicked() || resp.middle_clicked() {
        focused = true;
        shown.focus = true;
    }

    let Some(term) = term else { return shown };
    term.resize(size, (cell_w.round() as u16, row_h.round() as u16));
    // Out from under the lock before any laying out happens.
    let crate::Screen { rows, cursor, app_cursor, alt_screen, mouse, .. } = term.screen();
    let hyperlinks = term.hyperlinks();
    let blocks = if alt_screen { Vec::new() } else { term.blocks() };
    let pictures = term.pictures();

    // Rows taller than the font (a line height over 1): the text in the middle.
    let font_h = ui.fonts_mut(|x| x.row_height(f));
    let lift = ((row_h - font_h) / 2.0).max(0.0).floor();
    // Where egui puts the baseline in a row, for the ligatures drawn by hand.
    let baseline = faces.shaper.as_ref().map(|_| painter.layout_no_wrap("M".into(), f.clone(), pal.fg).rows.first().and_then(|r| r.glyphs.first()).map_or(0.0, |g| g.pos.y));
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

        // The row's ligatures: by cell, the glyph drawn there instead, and
        // the cells a merged glyph covers, not drawn at all.
        let mut liga: Vec<Option<crate::liga::Sub>> = Vec::new();
        let mut hidden: Vec<bool> = Vec::new();
        if let Some(shaper) = &faces.shaper {
            liga = vec![None; row.len()];
            hidden = vec![false; row.len()];
            let plain = |c: &crate::CellView| c.c != ' ' && c.c != '\0' && !c.flags.intersects(Flags::BOLD | Flags::ITALIC | Flags::WIDE_CHAR | Flags::WIDE_CHAR_SPACER);
            let mut x = 0;
            while x < row.len() {
                if !plain(&row[x]) {
                    x += 1;
                    continue;
                }
                // A run: plain cells side by side in one colour.
                let start = x;
                while x < row.len() && plain(&row[x]) && row[x].fg == row[start].fg {
                    x += 1;
                }
                let text: String = row[start..x].iter().map(|c| c.c).collect();
                if crate::liga::worth_shaping(&text) {
                    for s in shaper.subs(&text).iter() {
                        let at = start + s.at;
                        if at < row.len() {
                            liga[at] = Some(*s);
                            for h in hidden.iter_mut().take((at + s.covers).min(row.len())).skip(at + 1) {
                                *h = true;
                            }
                        }
                    }
                }
            }
        }
        for (x, cell) in row.iter().enumerate() {
            // Concealed text (SGR 8) is there to be copied, not seen.
            if cell.flags.contains(Flags::HIDDEN) {
                continue;
            }
            let fg = ink(cell, pal);
            // The lines go under spaces too: an underlined gap is still one.
            if cell.flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) {
                let ul = cell.ul.map_or(fg, |c| color(c, pal, false));
                let left = inner.left() + x as f32 * cell_w;
                for shape in decorations(cell.flags, left, cell_w, top + lift, font_h, fg, ul) {
                    painter.add(shape);
                }
            }
            if cell.c == ' ' || cell.c == '\0' || hidden.get(x).copied().unwrap_or(false) {
                continue;
            }
            if let (Some(Some(sub)), Some(shaper), Some(base)) = (liga.get(x), &faces.shaper, baseline) {
                let pen = egui::pos2(inner.left() + x as f32 * cell_w, top + lift + base);
                shaper.draw(ui.ctx(), &painter, sub, pen, cell_w, fg);
                continue;
            }
            painter.text(
                egui::pos2(inner.left() + x as f32 * cell_w, top + lift),
                Align2::LEFT_TOP,
                cell.c,
                faces.pick(cell.flags).clone(),
                fg,
            );
        }
    }

    // The pictures, over the text as kitty draws them, each over the cells
    // from its top left one.
    if !pictures.is_empty() {
        let clip = painter.with_clip_rect(inner);
        for p in &pictures {
            let Some(texture) = texture(ui.ctx(), &*term, p.key) else { continue };
            let at = Rect::from_min_size(
                egui::pos2(inner.left() + p.col as f32 * cell_w, inner.top() + p.line as f32 * row_h),
                Vec2::new(p.cols as f32 * cell_w, p.rows as f32 * row_h),
            );
            // The picture keeps its shape inside its cells, from their top left.
            let [w, h] = texture.size().map(|v| v.max(1) as f32);
            let scale = (at.width() / w).min(at.height() / h);
            let at = Rect::from_min_size(at.min, Vec2::new(w * scale, h * scale));
            clip.image(texture.id(), at, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
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
        let style = faces.cursor;
        let lit = !style.blink || {
            let now = ui.input(|i| i.time);
            let left = BLINK - now % BLINK;
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left));
            ((now / BLINK) as u64).is_multiple_of(2)
        };
        if focused && !lit {
        } else if focused && style.shape == CursorShape::Block {
            painter.rect_filled(at, CornerRadius::ZERO, pal.cursor);
            if let Some(cell) = rows.get(cy).and_then(|r| r.get(cx)) {
                if cell.c != ' ' {
                    painter.text(at.left_top(), Align2::LEFT_TOP, cell.c, f.clone(), pal.on_cursor);
                }
            }
        } else if focused {
            let thick = (cell_w / 6.0).max(2.0).round();
            let mark = match style.shape {
                CursorShape::Bar => Rect::from_min_size(at.left_top(), Vec2::new(thick, at.height())),
                _ => Rect::from_min_max(egui::pos2(at.left(), at.bottom() - thick), at.right_bottom()),
            };
            painter.rect_filled(mark, CornerRadius::ZERO, pal.cursor);
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
    // Ctrl (Cmd on a Mac) over a web address or a path underlines it and
    // shows a hand; a click opens it, as in VS Code's and Windows Terminal's
    // panes.
    // A link a program put there (OSC 8) is always marked, with a dotted
    // line, as in Windows Terminal: its text need not look like one.
    let x = |c: usize| inner.left() + c as f32 * cell_w;
    let under = |line: usize| inner.top() + (line + 1) as f32 * row_h - 1.5;
    // Each command that ended, a bar in the margin from its prompt to the
    // next: green for success, red for a failure (Warp's and iTerm2's marks).
    for b in &blocks {
        let tint = match b.exit {
            0 => named(NamedColor::Green, pal, false),
            _ => named(NamedColor::Red, pal, false),
        };
        let (y0, y1) = (inner.top() + b.lines.start as f32 * row_h + 1.0, (inner.top() + b.lines.end as f32 * row_h - 1.0).min(inner.bottom()));
        if y1 > y0 {
            painter.rect_filled(Rect::from_min_max(egui::pos2(rect.left() + 2.0, y0), egui::pos2(rect.left() + 4.0, y1)), CornerRadius::same(1), tint.gamma_multiply(0.8));
        }
    }
    for h in &hyperlinks {
        let mut at = x(h.cells.start);
        while at < x(h.cells.end) {
            painter.hline(at..=(at + 2.0).min(x(h.cells.end)), under(h.line), Stroke::new(1.0, pal.fg_dim));
            at += 4.0;
        }
    }
    let linking = over && ui.ctx().input(|i| i.modifiers.command);
    if let Some(p) = hover.filter(|_| linking) {
        let (col, line, _) = cell_at(p);
        let program = hyperlinks.iter().find(|h| h.line == line && h.cells.contains(&col)).map(|h| (h.cells.clone(), crate::Link::Url(h.uri.clone())));
        if let Some((cells, link)) = program.or_else(|| rows.get(line).and_then(|r| crate::link_at(r, col))) {
            painter.hline(x(cells.start)..=x(cells.end), under(line), Stroke::new(1.0, pal.cursor));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                shown.open = Some(link);
            }
        }
    }
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
            // Alt (Option on a Mac) makes it a block: the rectangle between
            // the corners, a column of a table rather than whole lines.
            match ui.ctx().input(|i| i.modifiers.alt) {
                true => term.select_block((col, line), right),
                false => term.select((col, line), right, true),
            }
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
    // selecting: there is no other step -- unless the app turned that off.
    if resp.drag_stopped() || resp.double_clicked() {
        shown.selected = true;
        if opts.copy_on_select {
            shown.copy = term.selection();
        }
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

/// The colour a cell's text is drawn in: its own, dimmed (SGR 2), or its
/// background's when reversed (SGR 7).
fn ink(cell: &crate::CellView, pal: &Palette) -> Color32 {
    if cell.flags.contains(Flags::INVERSE) {
        // Solid even when the pane's colour is see-through.
        return color(cell.bg, pal, true).to_opaque();
    }
    let fg = color(cell.fg, pal, false);
    match cell.flags.contains(Flags::DIM) {
        true => fg.linear_multiply(0.6),
        false => fg,
    }
}

/// The lines a cell's flags put across it, in a cell `w` wide from `left`,
/// whose text is `font_h` high from `top`: the five underlines (SGR 4, 4:2 to
/// 4:5) in `ul` and the strike (SGR 9) in `fg`. The curl, dots and dashes
/// are placed by where they are on the row, not in the cell, so a run of
/// cells draws one unbroken line instead of a pattern restarting at each.
fn decorations(flags: Flags, left: f32, w: f32, top: f32, font_h: f32, fg: Color32, ul: Color32) -> Vec<egui::Shape> {
    let thick = (font_h / 14.0).round().max(1.0);
    let y = top + font_h - thick;
    let right = left + w;
    let line = |y: f32, c: Color32| egui::Shape::hline(left..=right, y, Stroke::new(thick, c));
    let mut out = Vec::new();
    if flags.contains(Flags::UNDERLINE) {
        out.push(line(y, ul));
    }
    if flags.contains(Flags::DOUBLE_UNDERLINE) {
        out.push(line(y, ul));
        out.push(line(y - 2.0 * thick, ul));
    }
    if flags.contains(Flags::UNDERCURL) {
        // One wave per cell width, as kitty and WezTerm draw it.
        let amp = thick.max(1.5);
        let steps = 8;
        let points = (0..=steps)
            .map(|i| {
                let px = left + w * i as f32 / steps as f32;
                egui::pos2(px, y - amp + amp * (std::f32::consts::TAU * px / w.max(1.0)).sin())
            })
            .collect();
        out.push(egui::Shape::line(points, Stroke::new(thick, ul)));
    }
    if flags.contains(Flags::DOTTED_UNDERLINE) {
        // Squares two points across at least: anything smaller is all
        // anti-aliasing and does not show.
        let size = thick.max(2.0);
        let step = 2.0 * size;
        let mut at = (left / step).ceil() * step;
        while at < right {
            let dot = Rect::from_min_max(egui::pos2(at, y - size / 2.0), egui::pos2((at + size).min(right), y + size / 2.0));
            out.push(egui::Shape::rect_filled(dot, CornerRadius::ZERO, ul));
            at += step;
        }
    }
    if flags.contains(Flags::DASHED_UNDERLINE) {
        let (dash, step) = (3.0 * thick, 5.0 * thick);
        let mut at = (left / step).floor() * step;
        while at < right {
            let (a, b) = (at.max(left), (at + dash).min(right));
            if b > a {
                out.push(egui::Shape::hline(a..=b, y, Stroke::new(thick, ul)));
            }
            at += step;
        }
    }
    if flags.contains(Flags::STRIKEOUT) {
        out.push(line((top + font_h * 0.55).round(), fg));
    }
    out
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
    if let Some(ansi) = &pal.ansi {
        let at = [Black, Red, Green, Yellow, Blue, Magenta, Cyan, White, BrightBlack, BrightRed, BrightGreen, BrightYellow, BrightBlue, BrightMagenta, BrightCyan, BrightWhite]
            .iter()
            .position(|x| *x == n);
        if let Some(k) = at {
            return ansi[k];
        }
    }
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
    fn a_cursor_word_names_its_shape_and_blink() {
        assert_eq!(CursorStyle::from_word("block-blink"), CursorStyle { shape: CursorShape::Block, blink: true });
        assert_eq!(CursorStyle::from_word("bar"), CursorStyle { shape: CursorShape::Bar, blink: false });
        assert_eq!(CursorStyle::from_word("underline-blink"), CursorStyle { shape: CursorShape::Underline, blink: true });
        assert_eq!(CursorStyle::from_word("odd"), CursorStyle::default());
    }

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

    /// A theme's own sixteen colours take the named ones' place.
    #[test]
    fn a_theme_gives_the_named_colours() {
        let mut ansi = [Color32::BLACK; 16];
        ansi[1] = Color32::from_rgb(1, 2, 3);
        ansi[15] = Color32::from_rgb(4, 5, 6);
        let pal = Palette { ansi: Some(ansi), ..Palette::default() };
        assert_eq!(named(NamedColor::Red, &pal, false), Color32::from_rgb(1, 2, 3));
        assert_eq!(indexed(15, &pal, false), Color32::from_rgb(4, 5, 6));
        assert_eq!(named(NamedColor::Foreground, &pal, false), pal.fg, "the default colours stay the palette's");
    }

    /// The strokes of a cell's lines: where each sits and in what colour.
    fn strokes(flags: Flags, left: f32) -> Vec<(f32, f32, f32, Color32)> {
        let (fg, ul) = (Color32::RED, Color32::BLUE);
        decorations(flags, left, 10.0, 100.0, 28.0, fg, ul)
            .into_iter()
            .map(|s| match s {
                egui::Shape::LineSegment { points: [a, b], stroke } => (a.x, b.x, a.y, stroke.color),
                egui::Shape::Path(p) => (p.points[0].x, p.points[p.points.len() - 1].x, p.points[0].y, match p.stroke.color { egui::epaint::ColorMode::Solid(c) => c, _ => panic!("a curl in one colour") }),
                egui::Shape::Rect(r) => (r.rect.left(), r.rect.right(), r.rect.center().y, r.fill),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    /// Each style draws what it says, under the text, in the underline's
    /// colour; the strike goes through the middle in the text's.
    #[test]
    fn each_line_style_draws_its_own_strokes() {
        assert!(strokes(Flags::empty(), 0.0).is_empty());
        // 28 high: two points thick, so the line sits at 100 + 28 - 2.
        assert_eq!(strokes(Flags::UNDERLINE, 0.0), vec![(0.0, 10.0, 126.0, Color32::BLUE)]);
        let double = strokes(Flags::DOUBLE_UNDERLINE, 0.0);
        assert_eq!(double.iter().map(|s| s.2).collect::<Vec<_>>(), [126.0, 122.0], "two lines, apart");
        let curl = strokes(Flags::UNDERCURL, 0.0);
        assert_eq!(curl.len(), 1, "one wave across the cell");
        assert_eq!((curl[0].0, curl[0].1, curl[0].3), (0.0, 10.0, Color32::BLUE));
        let strike = strokes(Flags::STRIKEOUT, 0.0);
        assert_eq!(strike, vec![(0.0, 10.0, (100.0_f32 + 28.0 * 0.55).round(), Color32::RED)]);
        assert!(strike[0].2 < 126.0 && strike[0].2 > 100.0, "through the text, not under it");
        let both = strokes(Flags::UNDERLINE | Flags::STRIKEOUT, 0.0);
        assert_eq!(both.len(), 2, "flags add up");
    }

    /// The dots and the dashes stay inside their cell, and fall on the row's
    /// grid rather than restarting at each cell, so a run reads as one line.
    #[test]
    fn dots_and_dashes_keep_to_the_row() {
        for flags in [Flags::DOTTED_UNDERLINE, Flags::DASHED_UNDERLINE] {
            for left in [0.0, 10.0, 30.0] {
                let s = strokes(flags, left);
                assert!(!s.is_empty(), "{flags:?} at {left} draws something");
                assert!(s.iter().all(|(a, b, _, _)| *a >= left && *b <= left + 10.0 && b > a), "{flags:?} at {left}: {s:?}");
            }
        }
        // Dots are every four points (2 thick, 2 apart) wherever the cell is.
        let starts: Vec<f32> = [0.0, 10.0].iter().flat_map(|l| strokes(Flags::DOTTED_UNDERLINE, *l)).map(|s| s.0).collect();
        assert!(starts.iter().all(|x| x % 4.0 == 0.0), "{starts:?}");
    }

    /// The curl's phase comes from where it is on the row: one cell's end is
    /// the next one's start.
    #[test]
    fn the_curl_runs_on_from_cell_to_cell() {
        let ends = |left: f32| match &decorations(Flags::UNDERCURL, left, 10.0, 0.0, 28.0, Color32::RED, Color32::BLUE)[0] {
            egui::Shape::Path(p) => (p.points[0], p.points[p.points.len() - 1]),
            other => panic!("{other:?}"),
        };
        let (_, end) = ends(0.0);
        let (start, _) = ends(10.0);
        assert!((end.y - start.y).abs() < 0.01 && (end.x - start.x).abs() < 0.01, "{end:?} then {start:?}");
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
