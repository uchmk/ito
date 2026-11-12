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
//! hear a click. So egui's own keyboard focus is nobody's while a question is
//! up: Tab cannot hand it to a widget behind the dim (whose Enter would press
//! it along with the answer), and the choices here never take it. A button is
//! drawn but never clipped away while still clickable (a row that runs past the
//! edge wraps instead).

use egui::text::{LayoutJob, TextFormat};
use egui::{vec2, Align2, Color32, Context, CursorIcon, FontId, Id, Order, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use ito_prefs::Mark;
use ito_theme::scale::space::{MD, SM};
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
    /// The marks of the buttons, one each, drawn before the words in the text colour (as `ito_prefs::IconButton`);
    /// a missing or `None` one shows none.
    pub marks: &'a [Option<Mark>],
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

/// The room a button's mark takes before its words: a 14 box and a gap.
const MARK_ROOM: f32 = 20.0;

/// How round the buttons' corners are, in points.
const BUTTON_RADIUS: f32 = ito_theme::scale::radius::CONTROL as f32;

/// The widest the box gets, in points.
pub const MAX_WIDTH: f32 = 600.0;

/// The box's width on a screen `screen` wide: the whole width less a margin, up
/// to [`MAX_WIDTH`], and never below what a button can sit in.
pub fn width(screen: f32) -> f32 {
    (screen - 40.0).clamp(160.0, MAX_WIDTH)
}

/// Draw the question over everything and return the index of the button
/// clicked this frame, if any. The keys are the app's to read.
///
/// A widget behind the question, in the app's panels and windows
/// (`Order::Background` and `Order::Middle`), that has egui's keyboard focus
/// when this returns loses it (and the frame is drawn again without its focus
/// ring), and a Tab this frame moves the focus no further, so it does not
/// reach a widget behind and last to the next Enter. A widget in an overlay
/// the app draws over its panels and windows (`Order::Foreground` and up),
/// such as ito-palette's field opened over the question, keeps it and takes
/// what is typed: one drawn before this call, or drawn in the last frame
/// (one first drawn after this call, that already has the focus, is not
/// known yet and loses it). So does one in an overlay that
/// was open before the question and is now under its dim (an egui popup or
/// menu, ito-notices' list): close those when the question opens, or take
/// their focus off. Focus the app gives back after this
/// call, in the frame the answer comes, is kept; given before it, to a widget
/// behind, it is dropped. The question's choices never take the
/// focus (an Enter would answer twice): they are pressed by the mouse or an
/// AccessKit click, and offer no AccessKit focus.
///
/// Left to egui, for widgets drawn before this call: a Tab and an Enter in the
/// one frame can still press one. And in the first frame of a question, if one
/// of them still has the focus from before, a Tab or a Shift+Tab can pass it on
/// to another widget, which may hold it at the start of the next frame (and be
/// pressed by an Enter there) until this call drops it, when it is behind.
/// A Tab from a widget that keeps the focus, in an overlay, can likewise pass
/// it on to a widget behind drawn after this call: it holds it for the rest of
/// that frame (a Tab and an Enter in the one frame can press it) and loses it
/// here in the next.
pub fn show(ctx: &Context, q: &Question, style: &Style) -> Option<usize> {
    let screen = ctx.content_rect();
    let mut clicked = None;
    // Under the box and over the app, taking the mouse so nothing behind is hit.
    // Not focusable: Tab does not stop on it.
    egui::Area::new(Id::new("ito-confirm-dim")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::CLICK | Sense::DRAG);
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(140));
    });
    let w = width(screen.width());
    egui::Area::new(Id::new("ito-confirm")).order(Order::Tooltip).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        let frame =
            egui::Frame::new().fill(style.panel).stroke(Stroke::new(1.0, style.border)).corner_radius(BUTTON_RADIUS).inner_margin(14.0);
        let resp = frame.show(ui, |ui| {
            ui.set_width(w - 28.0);
            ui.label(RichText::new(q.title).strong().color(style.title));
            ui.add_space(SM);
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
                    ui.add_space(SM);
                    for line in q.body {
                        ui.add(egui::Label::new(RichText::new(line).color(style.text)).wrap());
                    }
                }
                if !q.diff.is_empty() {
                    ui.add_space(SM);
                    diff_view(ui, q.diff, style);
                }
                if !q.notes.is_empty() {
                    ui.add_space(SM);
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
                        ui.add_space(SM);
                    }
                }
            });
            if !q.buttons.is_empty() {
                ui.add_space(if q.cards.is_empty() { MD } else { 2.0 });
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
                        let mark = q.marks.get(j).copied().flatten();
                        let mut job = with_key(label, key, fg, primary);
                        if mark.is_some() {
                            // Room for the mark: the button's own padding, then a 14 box and a gap.
                            if let Some(first) = job.sections.first_mut() {
                                first.leading_space = MARK_ROOM;
                            }
                        }
                        // `CLICK` alone, not focusable: the app's `focus` is the
                        // keyboard's, and a second, egui one would answer Enter too.
                        let mut b = egui::Button::new(job).fill(bg).corner_radius(BUTTON_RADIUS).sense(Sense::CLICK);
                        if q.focus == Some(i) {
                            b = b.stroke(Stroke::new(2.0, style.accent.unwrap_or(style.text)));
                        } else if n > 0 && !primary {
                            b = b.stroke(Stroke::new(1.0, style.border));
                        }
                        let r = ui.add(b);
                        if let Some(m) = mark {
                            let at = egui::pos2(r.rect.left() + ui.spacing().button_padding.x + 7.0, r.rect.center().y);
                            ito_prefs::mark(ui.painter(), egui::Rect::from_center_size(at, vec2(14.0, 14.0)), m, fg);
                        }
                        if r.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            }
            if !q.hint.is_empty() {
                ui.add_space(SM);
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
    // Nothing behind the question keeps the focus: a Tab this frame may have
    // given it to a widget there, drawn before this call. Behind is the app's
    // panels and windows (and the question's own layers); a widget in an
    // overlay over those, `Foreground` and up, such as ito-palette's field
    // drawn over the question, keeps it. One drawn in neither this frame nor
    // the last is not over the question: it loses it. The layer's `Order`
    // decides, not where the layer sorts against the dim: a palette opened in
    // the question's first frame, drawn before this call, sorts under the dim
    // in that frame, and a click on the dim moves the dim to the top.
    // And the Tab stops here, so a widget drawn after this call does not take
    // it by itself (one that had the focus before this call, behind or in an
    // overlay, may still pass it on: see below and `show`'s doc).
    let own = |l: egui::LayerId| l.id == Id::new("ito-confirm") || l.id == Id::new("ito-confirm-dim");
    let behind = ctx
        .memory(|m| m.focused())
        .filter(|&id| ctx.read_response(id).is_none_or(|r| r.layer_id.order < Order::Foreground || own(r.layer_id)));
    ctx.memory_mut(|m| {
        m.move_focus(egui::FocusDirection::None);
        if let Some(id) = behind {
            m.surrender_focus(id);
        }
    });
    let dropped = behind.is_some();
    // That widget was drawn with its focus ring: draw it again without, now.
    // On a Tab too: a widget that had the focus before this call can pass it
    // on to the next one drawn, after this call, which loses it here in the
    // next frame. `raw`: the app may have consumed the Tab, egui saw it.
    let tab = ctx.input(|i| i.raw.events.iter().any(|e| matches!(e, egui::Event::Key { key: egui::Key::Tab, pressed: true, .. })));
    if dropped || tab {
        ctx.request_repaint();
    }
    clicked
}

/// A button's text with its key cap after it, in one line of text.
fn with_key(label: &str, key: &str, fg: Color32, primary: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let fmt = |font_id: FontId, color: Color32| TextFormat { font_id, color, ..Default::default() };
    job.append(label, 0.0, fmt(FontId::proportional(ito_theme::scale::Text::ChromeBody.size()), fg));
    if !key.is_empty() {
        let dim = if primary { fg.gamma_multiply(0.8) } else { fg.gamma_multiply(0.6) };
        job.append(key, 10.0, fmt(FontId::monospace(ito_theme::scale::Text::ChromeSmall.size()), dim));
    }
    job
}

/// A chip's colours: text, fill, outline.
type Chip = (Color32, Color32, Color32);

/// Draw one card across the box and say whether it was clicked.
fn card(ui: &mut Ui, c: &Card, focused: bool, recommended: bool, badge: &str, style: &Style) -> bool {
    const PAD: f32 = MD;
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
    let small = FontId::proportional(ito_theme::scale::Text::ChromeButton.size());
    let mono = FontId::monospace(ito_theme::scale::Text::ChromeSmall.size());
    let key_w = if c.key.is_empty() { 0.0 } else { text_width(ui, &c.key).max(8.0) + 14.0 };
    let inner = (w - 2.0 * PAD - if key_w > 0.0 { key_w + 10.0 } else { 0.0 }).max(40.0);
    let fmt = |font_id: FontId, color: Color32| TextFormat { font_id, color, ..Default::default() };

    let mut job = LayoutJob::default();
    job.wrap.max_width = inner;
    job.append(&c.title, 0.0, fmt(FontId::proportional(ito_theme::scale::Text::ChromeBody.size()), hue));
    if !c.en.is_empty() {
        job.append(&c.en, 8.0, fmt(mono.clone(), style.dim));
    }
    if green && !badge.is_empty() {
        let mut f = fmt(FontId::proportional(ito_theme::scale::Text::ChromeSmall.size()), OK_TEXT);
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
    // Not focusable, as the buttons: the app's `focus` is the keyboard's.
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h.max(34.0)), Sense::CLICK);
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
    let font = FontId::monospace(ito_theme::scale::Text::DialogKey.size());
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

    /// Marks draw before the words; a short list, a `None` and a click all work.
    #[test]
    fn buttons_take_marks() {
        let buttons = vec!["Trash".to_owned(), "Cancel".to_owned()];
        let marks = [Some(Mark::Trash), None];
        let q = Question { title: "T", buttons: &buttons, marks: &marks, primary: Some(0), danger: true, ..Default::default() };
        assert_eq!(run(&q, 800.0), None);
        let short = [Some(Mark::Close)];
        assert_eq!(run(&Question { marks: &short, ..q }, 120.0), None);
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

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    /// Where an app's own button sits in the frame, relative to the question.
    #[derive(Clone, Copy, Debug)]
    enum Behind {
        None,
        Before,
        After,
    }

    /// One frame with a button of the app's behind the question: whether that
    /// button was pressed, the answer, and who holds egui's focus afterwards.
    fn frame(ctx: &Context, q: &Question, behind: Behind, events: Vec<egui::Event>) -> (bool, Option<usize>, Option<Id>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let (mut pressed, mut out) = (false, None);
        let mut o = ctx.run_ui(input, |ui| {
            if matches!(behind, Behind::Before) {
                pressed = ui.button("Connect to kei").clicked();
            }
            out = show(ui.ctx(), q, &style());
            if matches!(behind, Behind::After) {
                pressed = ui.button("Connect to kei").clicked();
            }
        });
        o.textures_delta.clear();
        (pressed, out, ctx.memory(|m| m.focused()))
    }

    /// koyomi: Tab moved egui's focus to a status-bar button behind the
    /// question, and the next Enter pressed it along with the answer. Here
    /// from a question already up, with nothing focused from before it.
    #[test]
    fn tab_leaves_no_focus_behind_the_question_or_on_its_choices() {
        let buttons = vec!["Keep editing".to_owned(), "Discard".to_owned()];
        let cs = cards();
        let with_buttons = Question { title: "Discard the changes?", buttons: &buttons, focus: Some(1), ..Default::default() };
        let with_cards = Question { title: "Reset?", cards: &cs, buttons: &buttons, focus: Some(0), ..Default::default() };
        let (none, shift) = (egui::Modifiers::NONE, egui::Modifiers::SHIFT);
        for q in [&with_buttons, &with_cards] {
            for behind in [Behind::None, Behind::Before, Behind::After] {
                let ctx = Context::default();
                for _ in 0..2 {
                    frame(&ctx, q, behind, vec![]);
                }
                let mut steps = vec![];
                for m in [none, none, shift, none, none, none, shift] {
                    steps.push(vec![key(egui::Key::Tab, m)]);
                }
                for arrow in [egui::Key::ArrowDown, egui::Key::ArrowRight, egui::Key::ArrowUp] {
                    steps.push(vec![key(egui::Key::Tab, none)]);
                    steps.push(vec![key(arrow, none)]);
                }
                for events in steps {
                    let (pressed, out, focused) = frame(&ctx, q, behind, events.clone());
                    assert_eq!((pressed, out, focused), (false, None, None), "{behind:?} {events:?}");
                    // The frame `show` asked for, with the focus ring gone.
                    let (pressed, out, focused) = frame(&ctx, q, behind, vec![]);
                    assert_eq!((pressed, out, focused), (false, None, None), "repaint after {behind:?} {events:?}");
                    let (pressed, out, focused) = frame(&ctx, q, behind, vec![key(egui::Key::Enter, none)]);
                    assert_eq!((pressed, out, focused), (false, None, None), "Enter after {behind:?} {events:?}");
                }
            }
            // A Tab and an Enter in the one frame: no choice of the question
            // takes the focus, nor a widget drawn after it. (One drawn before
            // it still can: see `show`.)
            for behind in [Behind::None, Behind::After] {
                let ctx = Context::default();
                for _ in 0..2 {
                    frame(&ctx, q, behind, vec![]);
                }
                let tab_enter = vec![key(egui::Key::Tab, none), key(egui::Key::Enter, none)];
                assert_eq!(frame(&ctx, q, behind, tab_enter), (false, None, None), "Tab+Enter, {behind:?}");
            }
        }
    }

    /// The focus is dropped while the question is drawn, not after it: focus
    /// the app gives back with the answer stays, and Tab works again at once.
    #[test]
    fn focus_given_back_with_the_answer_is_kept() {
        let buttons = vec!["Keep editing".to_owned()];
        let q = Question { title: "Discard the changes?", buttons: &buttons, ..Default::default() };
        let ctx = Context::default();
        let input =
            egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(800.0, 600.0))), ..Default::default() };
        let mut id = None;
        for asking in [true, true, true, false, false] {
            let mut o = ctx.run_ui(input.clone(), |ui| {
                let r = ui.button("Editor");
                id = Some(r.id);
                if asking {
                    show(ui.ctx(), &q, &style());
                    // As in the frame the answer comes (the last one with the
                    // question): the app hands the focus back after `show`.
                    ui.ctx().memory_mut(|m| m.request_focus(r.id));
                }
            });
            o.textures_delta.clear();
        }
        assert_eq!(ctx.memory(|m| m.focused()), id);
        // And with the question gone, Tab moves egui's focus as before.
        ctx.memory_mut(|m| m.surrender_focus(id.unwrap()));
        let tab = egui::RawInput { events: vec![key(egui::Key::Tab, egui::Modifiers::NONE)], ..input };
        let mut o = ctx.run_ui(tab, |ui| {
            let _ = ui.button("Editor");
        });
        o.textures_delta.clear();
        assert_eq!(ctx.memory(|m| m.focused()), id);
    }

    /// One frame of koyomi's: a button in the app's panel, then the palette
    /// over a question, both drawn before `show`. Whether the button was
    /// pressed, the palette's answer, and who holds egui's focus afterwards.
    fn palette_frame(
        ctx: &Context,
        q: &Question,
        view: &mut ito_palette::View,
        events: Vec<egui::Event>,
    ) -> (bool, Option<ito_palette::Answer<u8>>, Option<Id>) {
        let c = Color32::GRAY;
        let look = ito_palette::Style { panel: c, border: c, text: c, dim: c, selected_bg: c, selected_text: c, hit: c, accent: c };
        let words = ito_palette::Words { hint: "", none: "", header: "", footer: "" };
        let entries = [ito_palette::Entry::new("One", "", 1), ito_palette::Entry::new("Gamma", "", 2)];
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let (mut pressed, mut answer) = (false, None);
        let mut o = ctx.run_ui(input, |ui| {
            pressed = ui.button("Connect to kei").clicked();
            answer = ito_palette::show(ui.ctx(), view, &entries, &look, &words);
            show(ui.ctx(), q, &style());
        });
        o.textures_delta.clear();
        (pressed, answer, ctx.memory(|m| m.focused()))
    }

    /// koyomi: with the question dropping all focus, ito-palette's field over
    /// it (drawn before `show` in the frame) lost the focus every frame, and
    /// nothing typed reached it. It keeps it now. A Tab stays in the field
    /// (it locks the focus, so egui does not move it on) and does not reach
    /// the panel's button behind.
    #[test]
    fn a_palette_over_the_question_keeps_its_field() {
        let buttons = vec!["Keep editing".to_owned(), "Open".to_owned()];
        let q = Question { title: "Open in the browser?", buttons: &buttons, focus: Some(1), ..Default::default() };
        let field = Some(Id::new("ito-palette-query"));
        let ctx = Context::default();
        let mut view = ito_palette::View::new();
        for _ in 0..4 {
            palette_frame(&ctx, &q, &mut view, vec![]);
        }
        assert_eq!(palette_frame(&ctx, &q, &mut view, vec![egui::Event::Paste("ga".into())]), (false, None, field));
        assert_eq!(palette_frame(&ctx, &q, &mut view, vec![egui::Event::Text("mm".into())]), (false, None, field));
        assert_eq!(view.query, "gamm");
        for m in [egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
            assert_eq!(palette_frame(&ctx, &q, &mut view, vec![key(egui::Key::Tab, m)]), (false, None, field), "{m:?}");
            assert_eq!(palette_frame(&ctx, &q, &mut view, vec![]), (false, None, field), "repaint after {m:?}");
        }
        // The palette answers without drawing its field, and egui lets the
        // focus of a widget not drawn go: nothing has it, the button neither.
        let enter = palette_frame(&ctx, &q, &mut view, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
        assert_eq!(enter, (false, Some(ito_palette::Answer::Pick(2)), None));
    }

    /// Behind the question is the app's panels and windows; an overlay over
    /// them (`Foreground` and up) keeps the focus the app gives it before `show`.
    #[test]
    fn only_the_focus_behind_the_question_is_dropped() {
        let buttons = vec!["Keep editing".to_owned()];
        let q = Question { title: "Discard the changes?", buttons: &buttons, ..Default::default() };
        let input =
            egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(800.0, 600.0))), ..Default::default() };
        for (order, kept) in [(Order::Background, false), (Order::Middle, false), (Order::Foreground, true), (Order::Tooltip, true)] {
            let ctx = Context::default();
            let mut id = None;
            for _ in 0..3 {
                let mut o = ctx.run_ui(input.clone(), |ui| {
                    egui::Area::new(Id::new("the app's")).order(order).show(ui.ctx(), |ui| {
                        let r = ui.button("Connect to kei");
                        r.request_focus();
                        id = Some(r.id);
                    });
                    show(ui.ctx(), &q, &style());
                });
                o.textures_delta.clear();
            }
            assert_eq!(ctx.memory(|m| m.focused()) == id, kept, "{order:?}");
        }
    }

    /// 0.292.2's Tab case, from an overlay: a Tab in a widget the question keeps (one that
    /// does not lock the focus) moves it on to the next widget, and when that
    /// one is behind the question, drawn before `show`, it loses it in the
    /// same frame, so the next Enter presses nothing.
    #[test]
    fn a_tab_from_an_overlay_does_not_leave_the_focus_behind() {
        let buttons = vec!["Keep editing".to_owned()];
        let q = Question { title: "Discard the changes?", buttons: &buttons, ..Default::default() };
        let ctx = Context::default();
        let frame = |events: Vec<egui::Event>, focus: bool| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let (mut pressed, mut over) = (false, None);
            let mut o = ctx.run_ui(input, |ui| {
                egui::Area::new(Id::new("the app's overlay")).order(Order::Foreground).show(ui.ctx(), |ui| {
                    let r = ui.button("Search");
                    if focus {
                        r.request_focus();
                    }
                    over = Some(r.id);
                });
                pressed = ui.button("Connect to kei").clicked();
                show(ui.ctx(), &q, &style());
            });
            o.textures_delta.clear();
            (pressed, over, ctx.memory(|m| m.focused()))
        };
        let mut over = None;
        for _ in 0..3 {
            let (pressed, id, focused) = frame(vec![], true);
            assert!(!pressed);
            assert_eq!(focused, id, "the overlay keeps the focus it is given");
            over = id;
        }
        assert!(over.is_some());
        let none = egui::Modifiers::NONE;
        let (pressed, _, focused) = frame(vec![key(egui::Key::Tab, none)], false);
        assert_eq!((pressed, focused), (false, None), "Tab");
        let (pressed, _, focused) = frame(vec![], false);
        assert_eq!((pressed, focused), (false, None), "repaint after Tab");
        let (pressed, _, focused) = frame(vec![key(egui::Key::Enter, none)], false);
        assert_eq!((pressed, focused), (false, None), "Enter after Tab");
    }
}
