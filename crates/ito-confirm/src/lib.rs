//! The question before something is done: a process is ended, a service
//! stopped, a file deleted. A dimmed screen and a titled box with, top to
//! bottom, `key : value` rows (what it is), lines of body text (wrapped, never
//! cut in the middle), notes in the warning colour (how it will be done), the
//! buttons and a hint of the keys that answer.
//!
//! The app owns the words and the colours ([`Style`]) and the keys: it reads
//! its own keymap for Enter/Esc/`y`/`n` and calls [`show`] only to draw and to
//! hear a click. A button is drawn but never clipped away while still clickable
//! (a row that runs past the edge wraps instead).

use egui::{vec2, Align2, Color32, Context, Id, Order, RichText, Sense, Stroke, StrokeKind};

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

/// What to ask. Everything is borrowed from the app's own state for the frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Question<'a> {
    pub title: &'a str,
    /// `(key, value)` rows, the keys lined up.
    pub rows: &'a [(String, String)],
    /// Lines of text under the rows, wrapped to the box.
    pub body: &'a [String],
    /// Lines in the warning colour under the body.
    pub notes: &'a [String],
    /// The buttons, in order; the index of the one clicked comes back.
    pub buttons: &'a [String],
    /// The keys that answer, shown dim under the buttons; empty for none.
    pub hint: &'a str,
    /// The button the keyboard is on (the app moves it with Tab / Shift+Tab and
    /// answers Enter itself); drawn with an outline in `Style::accent`/`text`.
    pub focus: Option<usize>,
    /// The button that carries on with what is asked (drawn solid green with
    /// bold white text, as the primary button of a GitHub form); the others
    /// keep `Style::button_bg`. `None` draws them all alike.
    pub primary: Option<usize>,
}

/// The fill of the [`Question::primary`] button.
pub const PRIMARY_BG: Color32 = Color32::from_rgb(0x2e, 0xa0, 0x43);
/// Its text.
pub const PRIMARY_TEXT: Color32 = Color32::WHITE;

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
            if !q.notes.is_empty() {
                ui.add_space(6.0);
                for line in q.notes {
                    ui.add(egui::Label::new(RichText::new(line).color(style.warn)).wrap());
                }
            }
            if !q.buttons.is_empty() {
                ui.add_space(10.0);
                // At the box's right edge, the last button rightmost; a row too
                // long for the box wraps to the next line, not past the edge.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center).with_main_wrap(true), |ui| {
                    for (i, label) in q.buttons.iter().enumerate().rev() {
                        let mut b = if q.primary == Some(i) {
                            egui::Button::new(RichText::new(label).strong().color(PRIMARY_TEXT)).fill(PRIMARY_BG)
                        } else {
                            egui::Button::new(RichText::new(label).color(style.button_text)).fill(style.button_bg)
                        };
                        b = b.corner_radius(BUTTON_RADIUS);
                        if q.focus == Some(i) {
                            b = b.stroke(Stroke::new(2.0, style.accent.unwrap_or(style.text)));
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
}
