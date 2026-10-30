//! The question before something is done: a process is ended, a service
//! stopped, a file deleted. A dimmed screen and a titled box with, top to
//! bottom, `key : value` rows (what it is), lines of body text (wrapped, never
//! cut in the middle), notes in the warning colour (how it will be done), the
//! choices and a hint of the keys that answer.
//!
//! A choice is a button, or, when there are three or more, a [`Card`]: a verb
//! for a title, a line saying what it does, the result in numbers, whether it
//! can be undone, and the key that picks it. Green marks the one recommended,
//! red the one that cannot be taken back; the two never share a card.
//!
//! The app owns the words and the colours ([`Style`]) and the keys: it reads
//! its own keymap for Enter/Esc/`y`/`n` and calls [`show`] only to draw and to
//! hear a click. A button is drawn but never clipped away while still clickable
//! (a row that runs past the edge wraps instead).

use egui::text::{LayoutJob, TextFormat};
use egui::{vec2, Align2, Color32, Context, CursorIcon, FontId, Id, Order, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use std::sync::Arc;

/// The colours of the box.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub panel: Color32,
    pub border: Color32,
    /// The outline that marks the question as one to think about (red for a
    /// kill); `border` is used when `None`.
    pub accent: Option<Color32>,
    pub title: Color32,
    pub text: Color32,
    /// Row keys and the hint.
    pub dim: Color32,
    /// The notes.
    pub warn: Color32,
    pub button_bg: Color32,
    pub button_text: Color32,
}

/// Whether a [`Card`]'s choice can be taken back, and how.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Undo {
    /// Nothing is said.
    #[default]
    Unsaid,
    /// It can be: the text says how ("reflog").
    Yes(String),
    /// It cannot: the text says what is lost. The card is drawn red.
    No(String),
}

/// One choice drawn as a card.
#[derive(Clone, Debug, Default)]
pub struct Card {
    /// A verb for what happens ("Reset the commits").
    pub title: String,
    /// The name it goes by elsewhere, small beside the title ("Soft"); may be empty.
    pub en: String,
    /// One line on what it does; may be empty.
    pub description: String,
    /// What it does in numbers ("4 commits leave main"); may be empty.
    pub result: String,
    pub undo: Undo,
    /// The key that picks it, as a key cap ("1"); may be empty.
    pub key: String,
    /// Draw it red even when [`Undo`] does not say so.
    pub danger: bool,
}

/// What to ask. Everything is borrowed from the app's own state for the frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Question<'a> {
    pub title: &'a str,
    /// `(key, value)` rows, the keys lined up.
    pub rows: &'a [(String, String)],
    /// Lines of text under the rows, wrapped to the box.
    pub body: &'a [String],
    /// A line diff under the body (from `ito_diff::diff`): `+`/`-` marks and
    /// colour, the line numbers in a gutter, and a scroll when it is long.
    pub diff: &'a [ito_diff::Item],
    /// Lines in the warning colour under the body.
    pub notes: &'a [String],
    /// The cards, in order, above the buttons. The choices are numbered cards
    /// first, then buttons: every index below (`focus`, `primary`, `recommended`)
    /// and the one that comes back counts them together.
    pub cards: &'a [Card],
    /// The buttons, in order; the index of the one clicked comes back. With
    /// cards they are the quiet ones (cancel), drawn with a border and no fill.
    pub buttons: &'a [String],
    /// The key caps of the buttons, one each; a missing or empty one shows none.
    pub keys: &'a [String],
    /// The keys that answer, shown dim under the buttons; empty for none.
    pub hint: &'a str,
    /// The button the keyboard is on (the app moves it with Tab / Shift+Tab and
    /// answers Enter itself); drawn with an outline in `Style::accent`/`text`.
    pub focus: Option<usize>,
    /// The button that carries on with what is asked (drawn solid green with
    /// bold white text, as the primary button of a GitHub form); the others
    /// keep `Style::button_bg`. `None` draws them all alike.
    pub primary: Option<usize>,
    /// Draw the primary button red: what it carries on with cannot be taken back.
    pub danger: bool,
    /// The card to point out in green, with `recommended_label` beside its title.
    pub recommended: Option<usize>,
    /// The word on the recommended card ("Recommended"); empty for the green alone.
    pub recommended_label: &'a str,
}

/// The fill of the [`Question::primary`] button.
pub const PRIMARY_BG: Color32 = Color32::from_rgb(0x2e, 0xa0, 0x43);
/// Its text.
pub const PRIMARY_TEXT: Color32 = Color32::WHITE;

/// The fill of a red primary button ([`Question::danger`]).
pub const DANGER_BG: Color32 = Color32::from_rgb(0xd1, 0x24, 0x2f);
/// Red as text and outline on the dark panel.
pub const DANGER_TEXT: Color32 = Color32::from_rgb(0xf2, 0x55, 0x5e);
/// Green as text and outline on the dark panel.
pub const OK_TEXT: Color32 = Color32::from_rgb(0x56, 0xd3, 0x64);

/// How round the buttons' corners are, in points.
const BUTTON_RADIUS: f32 = 6.0;

/// The widest the box gets, in points.
pub const MAX_WIDTH: f32 = 600.0;

/// The box's width on a screen `screen` wide: the whole width less a margin, up
/// to [`MAX_WIDTH`], and never below what a button can sit in.
pub fn width(screen: f32) -> f32 {
    (screen - 40.0).clamp(160.0, MAX_WIDTH)
}

/// Draw the question over everything and return the index of the button
/// clicked this frame, if any. The keys are the app's to read.
pub fn show(ctx: &Context, q: &Question, style: &Style) -> Option<usize> {
    let screen = ctx.content_rect();
    let mut clicked = None;
    // Under the box and over the app, taking the mouse so nothing behind is hit.
    egui::Area::new(Id::new("ito-confirm-dim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(140));
    });
    let w = width(screen.width());
    egui::Area::new(Id::new("ito-confirm")).order(Order::Tooltip).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        let frame = egui::Frame::new().fill(style.panel).stroke(Stroke::new(1.0, style.border)).corner_radius(6.0).inner_margin(14.0);
        let resp = frame.show(ui, |ui| {
            ui.set_width(w - 28.0);
            ui.label(RichText::new(q.title).strong().color(style.title));
            ui.add_space(6.0);
            // Everything between the title and the buttons scrolls when the
            // box would not fit the window; the title, buttons and hint stay.
            let reserve = 26.0 + if q.buttons.is_empty() { 0.0 } else { 52.0 } + if q.hint.is_empty() { 0.0 } else { 40.0 };
            let room = (screen.height() - 40.0 - 28.0 - reserve).max(60.0);
            egui::ScrollArea::vertical().max_height(room).auto_shrink([false, true]).show(ui, |ui| {
                let key_w = q.rows.iter().map(|(k, _)| text_width(ui, k)).fold(0.0, f32::max);
                for (k, v) in q.rows {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui(vec2(key_w + 12.0, 0.0), |ui| {
                            ui.set_min_width(key_w + 12.0);
                            ui.label(RichText::new(k).color(style.dim));
                        });
                        ui.add(egui::Label::new(RichText::new(v).color(style.text)).wrap());
                    });
                }
                if !q.body.is_empty() {
                    ui.add_space(6.0);
                    for line in q.body {
                        ui.add(egui::Label::new(RichText::new(line).color(style.text)).wrap());
                    }
                }
                if !q.diff.is_empty() {
                    ui.add_space(6.0);
                    diff_view(ui, q.diff, style);
                }
                if !q.notes.is_empty() {
                    ui.add_space(6.0);
                    for line in q.notes {
                        ui.add(egui::Label::new(RichText::new(line).color(style.warn)).wrap());
                    }
                }
                if !q.cards.is_empty() {
                    ui.add_space(8.0);
                    for (i, c) in q.cards.iter().enumerate() {
                        if card(ui, c, q.focus == Some(i), q.recommended == Some(i), q.recommended_label, style) {
                            clicked = Some(i);
                        }
                        ui.add_space(6.0);
                    }
                }
            });
            if !q.buttons.is_empty() {
                ui.add_space(if q.cards.is_empty() { 10.0 } else { 2.0 });
                // At the box's right edge, the last button rightmost; a row too
                // long for the box wraps to the next line, not past the edge.
                // Sized to its own height: a bare `with_layout` would take all the
                // room the area had last frame and grow the box every frame.
                let layout = egui::Layout::right_to_left(egui::Align::Center).with_main_wrap(true);
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 0.0), layout, |ui| {
                    let n = q.cards.len();
                    for (j, label) in q.buttons.iter().enumerate().rev() {
                        let i = n + j;
                        let key = q.keys.get(j).map_or("", |k| k.as_str());
                        let primary = q.primary == Some(i);
                        let (fg, bg) = match (primary, q.danger) {
                            (true, true) => (PRIMARY_TEXT, DANGER_BG),
                            (true, false) => (PRIMARY_TEXT, PRIMARY_BG),
                            _ => (style.button_text, if n > 0 { Color32::TRANSPARENT } else { style.button_bg }),
                        };
                        let mut b = egui::Button::new(with_key(label, key, fg, primary)).fill(bg).corner_radius(BUTTON_RADIUS);
                        if q.focus == Some(i) {
                            b = b.stroke(Stroke::new(2.0, style.accent.unwrap_or(style.text)));
                        } else if n > 0 && !primary {
                            b = b.stroke(Stroke::new(1.0, style.border));
                        }
                        if ui.add(b).clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            }
            if !q.hint.is_empty() {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(q.hint).color(style.dim)).wrap());
            }
        });
        let outline = resp.response.rect;
        ui.painter().rect_stroke(
            outline,
            6.0,
            Stroke::new(1.0, style.accent.map_or(style.border, |c| c.gamma_multiply(0.6))),
            StrokeKind::Outside,
        );
    });
    clicked
}

/// A button's text with its key cap after it, in one line of text.
fn with_key(label: &str, key: &str, fg: Color32, primary: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let fmt = |font_id: FontId, color: Color32| TextFormat { font_id, color, ..Default::default() };
    job.append(label, 0.0, fmt(FontId::proportional(14.0), fg));
    if !key.is_empty() {
        let dim = if primary { fg.gamma_multiply(0.8) } else { fg.gamma_multiply(0.6) };
        job.append(key, 10.0, fmt(FontId::monospace(11.0), dim));
    }
    job
}

/// A chip's colours: text, fill, outline.
type Chip = (Color32, Color32, Color32);

/// Draw one card across the box and say whether it was clicked.
fn card(ui: &mut Ui, c: &Card, focused: bool, recommended: bool, badge: &str, style: &Style) -> bool {
    const PAD: f32 = 10.0;
    let red = c.danger || matches!(c.undo, Undo::No(_));
    // Red wins over green: a card that cannot be undone is never "recommended".
    let green = recommended && !red;
    let hue = if red {
        DANGER_TEXT
    } else if green {
        OK_TEXT
    } else {
        style.title
    };
    let w = ui.available_width();
    let small = FontId::proportional(12.0);
    let mono = FontId::monospace(11.0);
    let key_w = if c.key.is_empty() { 0.0 } else { text_width(ui, &c.key).max(8.0) + 14.0 };
    let inner = (w - 2.0 * PAD - if key_w > 0.0 { key_w + 10.0 } else { 0.0 }).max(40.0);
    let fmt = |font_id: FontId, color: Color32| TextFormat { font_id, color, ..Default::default() };

    let mut job = LayoutJob::default();
    job.wrap.max_width = inner;
    job.append(&c.title, 0.0, fmt(FontId::proportional(14.0), hue));
    if !c.en.is_empty() {
        job.append(&c.en, 8.0, fmt(mono.clone(), style.dim));
    }
    if green && !badge.is_empty() {
        let mut f = fmt(FontId::proportional(11.0), OK_TEXT);
        f.background = OK_TEXT.gamma_multiply(0.18);
        job.append(badge, 8.0, f);
    }
    let title = ui.painter().layout_job(job);
    let desc = (!c.description.is_empty()).then(|| ui.painter().layout(c.description.clone(), small.clone(), style.dim, inner));

    // Chips flow left to right and wrap at `inner`.
    let mut chips: Vec<(Arc<egui::Galley>, Chip)> = Vec::new();
    if !c.result.is_empty() {
        chips.push((
            ui.painter().layout(c.result.clone(), small.clone(), style.text, inner - 16.0),
            (style.text, style.text.gamma_multiply(0.08), style.text.gamma_multiply(0.25)),
        ));
    }
    match &c.undo {
        Undo::Unsaid => {}
        Undo::Yes(t) => chips.push((
            ui.painter().layout(t.clone(), small.clone(), style.dim, inner - 16.0),
            (style.dim, Color32::TRANSPARENT, style.dim.gamma_multiply(0.5)),
        )),
        Undo::No(t) => chips.push((
            ui.painter().layout(t.clone(), small.clone(), DANGER_TEXT, inner - 16.0),
            (DANGER_TEXT, DANGER_BG.gamma_multiply(0.25), DANGER_TEXT.gamma_multiply(0.6)),
        )),
    }
    let mut at = Vec::new(); // each chip's offset from the chip area's top-left
    let (mut x, mut y, mut row_h) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (g, _) in &chips {
        let (cw, ch) = (g.size().x + 16.0, g.size().y + 4.0);
        if x > 0.0 && x + cw > inner {
            x = 0.0;
            y += row_h + 4.0;
            row_h = 0.0;
        }
        at.push(vec2(x, y));
        x += cw + 6.0;
        row_h = row_h.max(ch);
    }
    let chips_h = if chips.is_empty() { 0.0 } else { y + row_h };

    let mut h = PAD + title.size().y + PAD;
    if let Some(d) = &desc {
        h += 2.0 + d.size().y;
    }
    if !chips.is_empty() {
        h += 5.0 + chips_h;
    }
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h.max(34.0)), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let p = ui.painter();
    let (mut fill, mut line) = if red {
        (DANGER_BG.gamma_multiply(0.22), DANGER_TEXT.gamma_multiply(0.55))
    } else if green {
        (OK_TEXT.gamma_multiply(0.14), OK_TEXT.gamma_multiply(0.5))
    } else {
        (style.text.gamma_multiply(0.07), style.border)
    };
    if resp.hovered() {
        fill = fill.blend(style.text.gamma_multiply(0.07));
        line = hue.gamma_multiply(0.8);
    }
    p.rect(rect, BUTTON_RADIUS + 2.0, fill, Stroke::new(1.0, line), StrokeKind::Inside);
    if focused {
        p.rect_stroke(rect.expand(1.0), BUTTON_RADIUS + 2.0, Stroke::new(2.0, style.accent.unwrap_or(style.text)), StrokeKind::Outside);
    }
    let left = rect.min.x + PAD;
    let mut top = rect.min.y + PAD;
    let title_h = title.size().y;
    p.galley(egui::pos2(left, top), title, hue);
    top += title_h;
    if let Some(d) = desc {
        top += 2.0;
        let dh = d.size().y;
        p.galley(egui::pos2(left, top), d, style.dim);
        top += dh;
    }
    if !chips.is_empty() {
        top += 5.0;
        for ((g, (fg, bg, edge)), off) in chips.into_iter().zip(at) {
            let r = Rect::from_min_size(egui::pos2(left, top) + off, g.size() + vec2(16.0, 4.0));
            p.rect(r, r.height() / 2.0, bg, Stroke::new(1.0, edge), StrokeKind::Inside);
            p.galley(r.min + vec2(8.0, 2.0), g, fg);
        }
    }
    if key_w > 0.0 {
        let kg = p.layout_no_wrap(c.key.clone(), mono, hue);
        let r = Rect::from_center_size(egui::pos2(rect.max.x - PAD - key_w / 2.0, rect.center().y), vec2(key_w, kg.size().y + 4.0));
        p.rect(r, 4.0, style.text.gamma_multiply(0.06), Stroke::new(1.0, hue.gamma_multiply(0.5)), StrokeKind::Inside);
        p.galley(r.center() - kg.size() / 2.0, kg, hue);
    }
    resp.clicked()
}

/// The tallest the diff gets before it scrolls, in points.
const DIFF_MAX_HEIGHT: f32 = 240.0;

/// The diff as monospace rows: a gutter of old and new line numbers, a `+`/`-`
/// mark (so the kind is not told by colour alone), then the text.
fn diff_view(ui: &mut Ui, items: &[ito_diff::Item], style: &Style) {
    use ito_diff::{Item, Kind};
    let digits =
        items.iter().filter_map(|i| if let Item::Line(l) = i { l.old_no.max(l.new_no) } else { None }).max().unwrap_or(0).to_string().len();
    let font = FontId::monospace(12.0);
    let fmt = |color: Color32| TextFormat { font_id: font.clone(), color, ..Default::default() };
    egui::ScrollArea::vertical().max_height(DIFF_MAX_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for item in items {
            let mut job = LayoutJob::default();
            job.wrap.max_width = ui.available_width();
            match item {
                Item::Folded(n) => job.append(&format!("{:w$}  … {n} lines", "", w = digits * 2 + 1), 0.0, fmt(style.dim)),
                Item::Line(l) => {
                    let no = |n: Option<usize>| n.map_or_else(|| " ".repeat(digits), |n| format!("{n:>digits$}"));
                    job.append(&format!("{} {} ", no(l.old_no), no(l.new_no)), 0.0, fmt(style.dim));
                    let color = match l.kind {
                        Kind::Same => style.text,
                        Kind::Added => OK_TEXT,
                        Kind::Removed => DANGER_TEXT,
                    };
                    job.append(&format!("{} {}", l.kind.mark(), l.text), 0.0, fmt(color));
                }
            }
            ui.add(egui::Label::new(job).wrap());
        }
    });
}

fn text_width(ui: &egui::Ui, s: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().layout_no_wrap(s.to_owned(), font, Color32::WHITE).size().x
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Rect;

    fn style() -> Style {
        let c = Color32::GRAY;
        Style { panel: c, border: c, accent: Some(Color32::RED), title: c, text: c, dim: c, warn: c, button_bg: c, button_text: c }
    }

    fn run(q: &Question, w: f32) -> Option<usize> {
        let ctx = Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(w, 600.0))), ..Default::default() };
        let mut out = None;
        // Twice: the first frame only measures.
        for _ in 0..2 {
            let mut o = ctx.run_ui(input.clone(), |ui| out = show(ui.ctx(), q, &style()));
            o.textures_delta.clear();
        }
        out
    }

    /// The box's rect after `frames` frames in a window `h` tall.
    fn box_rect(q: &Question, h: f32, frames: usize) -> (Rect, Rect) {
        let ctx = Context::default();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(800.0, h));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        for _ in 0..frames {
            let mut o = ctx.run_ui(input.clone(), |ui| {
                show(ui.ctx(), q, &style());
            });
            o.textures_delta.clear();
        }
        (ctx.memory(|m| m.area_rect(Id::new("ito-confirm"))).expect("the box was drawn"), screen)
    }

    #[test]
    fn a_tall_box_stays_inside_the_window_and_is_steady() {
        let body: Vec<String> = (0..80).map(|i| format!("line {i} of a long body")).collect();
        let buttons = vec!["Yes".to_owned(), "No".to_owned()];
        let q = Question { title: "Discard?", body: &body, buttons: &buttons, hint: "Enter / Esc", ..Default::default() };
        let (a, screen) = box_rect(&q, 400.0, 3);
        let (b, _) = box_rect(&q, 400.0, 30);
        assert!(screen.contains_rect(a), "{a:?} not inside {screen:?}");
        assert_eq!(a.height(), b.height(), "the box grew from frame to frame");
    }

    #[test]
    fn a_short_box_after_a_tall_one_is_short_again() {
        let body: Vec<String> = (0..80).map(|i| format!("line {i}")).collect();
        let buttons = vec!["Yes".to_owned()];
        let tall = Question { title: "T", body: &body, buttons: &buttons, ..Default::default() };
        let short = Question { title: "T", buttons: &buttons, ..Default::default() };
        let ctx = Context::default();
        let input =
            egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(800.0, 400.0))), ..Default::default() };
        for q in [&tall, &tall, &short, &short, &short] {
            let mut o = ctx.run_ui(input.clone(), |ui| {
                show(ui.ctx(), q, &style());
            });
            o.textures_delta.clear();
        }
        let r = ctx.memory(|m| m.area_rect(Id::new("ito-confirm"))).unwrap();
        assert!(r.height() < 120.0, "still tall: {}", r.height());
    }

    #[test]
    fn it_draws_every_part_without_a_click() {
        let rows = vec![("Name".to_owned(), "sshd".to_owned()), ("PID".to_owned(), "12".to_owned())];
        let body = vec!["a long line ".repeat(40)];
        let notes = vec!["Sends SIGTERM".to_owned()];
        let buttons = vec!["Yes".to_owned(), "No".to_owned()];
        let q = Question {
            title: "End process?",
            rows: &rows,
            body: &body,
            notes: &notes,
            buttons: &buttons,
            hint: "Enter / Esc",
            focus: None,
            primary: Some(0),
            ..Default::default()
        };
        assert_eq!(run(&q, 800.0), None);
        // A narrow window still draws.
        assert_eq!(run(&q, 120.0), None);
        assert_eq!(run(&Question::default(), 800.0), None);
        // A focused button draws too, and an out-of-range focus is harmless.
        assert_eq!(run(&Question { focus: Some(1), ..q }, 800.0), None);
        assert_eq!(run(&Question { focus: Some(9), ..q }, 800.0), None);
        // So does a primary one, in range or not, narrow or wide.
        assert_eq!(run(&Question { primary: Some(1), focus: Some(1), ..q }, 120.0), None);
        assert_eq!(run(&Question { primary: Some(9), ..q }, 800.0), None);
    }

    #[test]
    fn a_long_diff_draws_in_a_narrow_box_too() {
        let old: String = (1..=300)
            .map(|i| {
                format!(
                    "line {i}
"
                )
            })
            .collect();
        let new = old.replace(
            "line 150
",
            "a changed line that is long enough to wrap in a narrow box
",
        );
        let items = ito_diff::diff(&old, &new, 3);
        let buttons = vec!["Run".to_owned()];
        let q = Question { title: "Changed", diff: &items, buttons: &buttons, ..Default::default() };
        assert_eq!(run(&q, 800.0), None);
        assert_eq!(run(&q, 120.0), None);
    }

    #[test]
    fn the_box_fits_the_screen() {
        assert_eq!(width(2000.0), MAX_WIDTH);
        assert_eq!(width(300.0), 260.0);
        assert_eq!(width(50.0), 160.0);
    }
    /// The number of button-sized rounded rects in the frame (the fill fades in, so it is not compared).
    fn filled(q: &Question, w: f32) -> usize {
        let ctx = Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(w, 600.0))), ..Default::default() };
        let st = style();
        let mut n = 0;
        for _ in 0..2 {
            let mut o = ctx.run_ui(input.clone(), |ui| {
                show(ui.ctx(), q, &st);
            });
            o.textures_delta.clear();
            n = o.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill != Color32::TRANSPARENT && r.rect.height() < 70.0 && r.corner_radius.nw == 6)).count();
        }
        n
    }

    #[test]
    fn every_button_is_drawn_as_one_however_many() {
        for count in 1..=6 {
            let buttons: Vec<String> = (0..count).map(|i| format!("A fairly long choice {i}  {i}")).collect();
            let q = Question { buttons: &buttons, ..Default::default() };
            for w in [800.0, 400.0] {
                assert_eq!(filled(&q, w), count, "{count} buttons in {w}");
            }
        }
    }

    fn cards() -> Vec<Card> {
        let c = |t: &str, undo: Undo, danger: bool| Card {
            title: t.into(),
            en: "Soft".into(),
            description: "A line that says what it does and is long enough to wrap in a narrow box ".repeat(2),
            result: "4 commits leave main".into(),
            undo,
            key: "1".into(),
            danger,
        };
        vec![
            c("Keep the changes", Undo::Yes("reflog".into()), false),
            c("Keep the files", Undo::Unsaid, false),
            c("Throw them away", Undo::No("3 unsaved files are lost".into()), false),
            c("Marked by hand", Undo::Unsaid, true),
        ]
    }

    /// The rects of the cards (rounded 8), top to bottom, after the frames have settled.
    fn card_rects(ctx: &Context, q: &Question, w: f32, events: Vec<egui::Event>) -> (Vec<Rect>, Option<usize>) {
        let input =
            egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(w, 700.0))), events, ..Default::default() };
        let mut out = None;
        let mut o = ctx.run_ui(input, |ui| out = show(ui.ctx(), q, &style()));
        o.textures_delta.clear();
        let mut r: Vec<Rect> = o
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.corner_radius.nw == 8 => Some(r.rect),
                _ => None,
            })
            .collect();
        r.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
        (r, out)
    }

    #[test]
    fn cards_draw_with_every_part_at_any_width() {
        let cs = cards();
        let buttons = vec!["Cancel".to_owned()];
        let keys = vec!["Esc".to_owned()];
        for w in [800.0, 400.0, 160.0] {
            let q = Question {
                title: "Reset?",
                cards: &cs,
                buttons: &buttons,
                keys: &keys,
                focus: Some(1),
                recommended: Some(1),
                recommended_label: "Recommended",
                ..Default::default()
            };
            let ctx = Context::default();
            let mut n = 0;
            for _ in 0..3 {
                n = card_rects(&ctx, &q, w, vec![]).0.len();
            }
            // The focused card has an outline of its own.
            assert_eq!(n, cs.len() + 1, "{w}");
        }
        // Cards alone, no buttons, a card with nothing but a title.
        let bare = [Card { title: "x".into(), ..Default::default() }];
        assert_eq!(run(&Question { cards: &bare, ..Default::default() }, 300.0), None);
    }

    #[test]
    fn a_click_on_a_card_answers_with_its_index_and_buttons_come_after() {
        let cs = cards();
        let buttons = vec!["Cancel".to_owned()];
        let q = Question { cards: &cs, buttons: &buttons, ..Default::default() };
        let ctx = Context::default();
        let mut rects = vec![];
        for _ in 0..3 {
            rects = card_rects(&ctx, &q, 800.0, vec![]).0;
        }
        assert_eq!(rects.len(), 4);
        let at = rects[2].center();
        let click =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut got = None;
        for ev in [vec![egui::Event::PointerMoved(at)], vec![click(true)], vec![click(false)]] {
            got = got.or(card_rects(&ctx, &q, 800.0, ev).1);
        }
        assert_eq!(got, Some(2));
    }
}
