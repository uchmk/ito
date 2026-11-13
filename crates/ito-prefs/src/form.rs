//! Parts of an edit form (a drawer or a pane of fields): a label that says whether the field is required, a framed
//! one-line field, a code box with a line-number gutter, and a section that is not a card.

use egui::{Align, FontFamily, FontId, Id, Layout, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};
use ito_a11y::Role;
use ito_theme::mix;

use crate::{mark, z, Look, Mark};

/// Whether a field must be filled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// A ` *` in the error colour.
    Required,
    /// A small `(optional)`.
    Optional,
    /// Nothing.
    Plain,
}

/// The words [`form_label`] adds, in the app's language.
#[derive(Clone, Copy, Debug)]
pub struct FormWords<'a> {
    /// Spoken for [`Need::Required`] (the star is not read).
    pub required: &'a str,
    /// Shown and spoken for [`Need::Optional`], without the brackets.
    pub optional: &'a str,
}

impl FormWords<'static> {
    pub fn en() -> Self {
        FormWords { required: "required", optional: "optional" }
    }

    pub fn ja() -> Self {
        FormWords { required: "必須", optional: "任意" }
    }

    /// The words of the language code (`"ja"`, else English).
    pub fn of(code: &str) -> Self {
        if code.starts_with("ja") {
            Self::ja()
        } else {
            Self::en()
        }
    }
}

impl Default for FormWords<'static> {
    fn default() -> Self {
        Self::en()
    }
}

/// A field's label: small, semibold, dim; ` *` after it when required, `(optional)` when optional, then the `note`
/// (a hint such as "one per line"). A screen reader hears "label, required" or "label, optional".
pub fn form_label(ui: &mut Ui, l: Look, label: &str, need: Need, note: &str, words: &FormWords) -> Response {
    let c = l.c;
    let r = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new(label).size(z(ui, 12.0)).strong().color(c.dim));
            match need {
                Need::Required => {
                    ui.label(RichText::new("*").size(z(ui, 12.0)).strong().color(c.err));
                }
                Need::Optional => {
                    ui.label(RichText::new(format!("({})", words.optional)).size(z(ui, 11.0)).color(c.dim));
                }
                Need::Plain => {}
            }
            if !note.is_empty() {
                ui.label(RichText::new(note).size(z(ui, 11.0)).color(mix(c.bg, c.dim, 0.8)));
            }
        })
        .response;
    let said = match need {
        Need::Required => format!("{label}, {}", words.required),
        Need::Optional => format!("{label}, {}", words.optional),
        Need::Plain => label.to_owned(),
    };
    ito_a11y::say(&r, Role::Label, &said);
    r
}

/// The frame of a field: `c.bg`, a 1px border (the error colour for an error, the accent while it has the keys, with a
/// soft glow round it), radius 6. `body` draws inside and returns what it made, whether it has the keys and whether the
/// field is in error (decided after the content, from what was just typed, so the border is right in the same frame).
fn framed<R>(ui: &mut Ui, width: Option<f32>, body: impl FnOnce(&mut Ui) -> (R, bool, bool)) -> (R, bool, bool, Response) {
    let c = ito_theme::colors();
    let w = width.unwrap_or_else(|| ui.available_width());
    ui.scope(|ui| {
        ui.set_max_width(w);
        let mut frame = egui::Frame::new()
            .fill(c.bg)
            .stroke(Stroke::new(1.0, c.border))
            .corner_radius(ito_theme::scale::radius::CONTROL)
            .inner_margin(egui::Margin::symmetric(8, 5))
            .begin(ui);
        frame.content_ui.set_min_width((w - 18.0).max(0.0));
        let (out, focused, error) = body(&mut frame.content_ui);
        frame.frame.stroke = Stroke::new(
            1.0,
            if error {
                c.err
            } else if focused {
                c.run
            } else {
                c.border
            },
        );
        let resp = frame.end(ui);
        if focused || error {
            let tint = if error { c.err } else { c.run };
            ui.painter().rect_stroke(resp.rect.expand(1.5), 7.5, Stroke::new(2.0, mix(c.bg, tint, 0.3)), StrokeKind::Outside);
        }
        (out, focused, error, resp)
    })
    .inner
}

/// A framed one-line field: an optional mark at the left, an optional unit (`秒`) at the right, full width or `.width(w)`.
pub struct InputField<'a> {
    id: &'a str,
    name: &'a str,
    mark: Option<Mark>,
    unit: &'a str,
    hint: &'a str,
    mono: bool,
    width: Option<f32>,
    error: bool,
    rows: usize,
}

impl<'a> InputField<'a> {
    /// `id` tells this field from another (and is the name a screen reader hears until [`name`](Self::name) is set).
    pub fn new(id: &'a str) -> Self {
        InputField { id, name: id, mark: None, unit: "", hint: "", mono: false, width: None, error: false, rows: 1 }
    }

    /// The name a screen reader hears (and a `--keys` script clicks by): the visible label of the field.
    pub fn name(mut self, name: &'a str) -> Self {
        self.name = name;
        self
    }

    pub fn mark(mut self, m: Mark) -> Self {
        self.mark = Some(m);
        self
    }

    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }

    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    /// In the monospace font (paths, commands).
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// Draw the border in the error colour.
    pub fn error(mut self, on: bool) -> Self {
        self.error = on;
        self
    }

    /// More than one row makes it a multiline prose field in the same frame (no gutter; see [`CodeBox`] for code).
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    /// The field; its response is the text edit's (`changed()`, `lost_focus()`).
    pub fn show(self, ui: &mut Ui, text: &mut String) -> Response {
        self.show_with(ui, text, &|_| false)
    }

    /// [`show`](Self::show), with `late`: a check of the text as it is after this frame's typing. When it says yes the
    /// field is in error (as with [`error`](Self::error)) in this same frame.
    pub(crate) fn show_with(self, ui: &mut Ui, text: &mut String, late: &dyn Fn(&str) -> bool) -> Response {
        let c = ito_theme::colors();
        let id = Id::new(("ito-input", self.id));
        let size = z(ui, 13.0);
        let family = if self.mono { FontFamily::Monospace } else { FontFamily::Proportional };
        let unit_w = if self.unit.is_empty() {
            0.0
        } else {
            ui.painter().layout_no_wrap(self.unit.to_owned(), FontId::proportional(z(ui, 12.0)), c.dim).size().x + 8.0
        };
        let (edit, _, error, _) = framed(ui, self.width, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let mut out = None;
            ui.horizontal(|ui| {
                if let Some(m) = self.mark {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(z(ui, 14.0)), Sense::hover());
                    mark(ui.painter(), r, m, c.dim);
                }
                let room = (ui.available_width() - unit_w).max(20.0);
                let edit =
                    if self.rows > 1 { egui::TextEdit::multiline(text).desired_rows(self.rows) } else { egui::TextEdit::singleline(text) };
                let r = ui.add(
                    edit.id(id)
                        .frame(egui::Frame::NONE)
                        .hint_text(RichText::new(self.hint).color(mix(c.bg, c.dim, 0.6)))
                        .font(FontId::new(size, family))
                        .text_color(c.fg)
                        .desired_width(room),
                );
                if !self.unit.is_empty() {
                    ui.label(RichText::new(self.unit).size(z(ui, 12.0)).color(c.dim));
                }
                out = Some(r);
            });
            let r = out.expect("the edit is drawn");
            let focused = r.has_focus();
            let error = self.error || late(text);
            (r, focused, error)
        });
        let name = self.name;
        let enabled = edit.enabled();
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, enabled, name));
        if error {
            ui.ctx().accesskit_node_builder(edit.id, |n| n.set_invalid(egui::accesskit::Invalid::True));
        }
        edit
    }
}

/// A multiline monospace editor with a line-number gutter, and an optional title bar (the title at the left, the aside
/// at the right). The numbers are those of the lines of text, so a wrapped line keeps one number.
pub struct CodeBox<'a> {
    id: &'a str,
    name: &'a str,
    title: &'a str,
    aside: &'a str,
    rows: usize,
    error: bool,
    hint: &'a str,
}

impl<'a> CodeBox<'a> {
    pub fn new(id: &'a str) -> Self {
        CodeBox { id, name: id, title: "", aside: "", rows: 4, error: false, hint: "" }
    }

    /// The name a screen reader hears (and a `--keys` script clicks by).
    pub fn name(mut self, name: &'a str) -> Self {
        self.name = name;
        self
    }

    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    pub fn aside(mut self, aside: &'a str) -> Self {
        self.aside = aside;
        self
    }

    /// The rows shown before it scrolls.
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    pub fn error(mut self, on: bool) -> Self {
        self.error = on;
        self
    }

    /// An example shown, faint, while the box is empty (for instance in the language of the picked shell). May span lines.
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    pub fn show(self, ui: &mut Ui, text: &mut String) -> Response {
        let c = ito_theme::colors();
        let id = Id::new(("ito-codebox", self.id));
        let font = FontId::monospace(z(ui, 12.5));
        let line_h = ui.fonts_mut(|f| f.row_height(&font));
        let digits = text.lines().count().max(1).to_string().len().max(2);
        let char_w = ui.painter().layout_no_wrap("0".into(), font.clone(), c.dim).size().x;
        let gutter = digits as f32 * char_w + 16.0;
        let (edit, _, _, _) = framed(ui, None, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            if !self.title.is_empty() || !self.aside.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(self.title).size(z(ui, 11.0)).strong().color(c.dim));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(self.aside).size(z(ui, 11.0)).color(mix(c.bg, c.dim, 0.8)));
                    });
                });
                ui.add_space(ito_theme::scale::space::XS);
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, c.border));
                ui.add_space(ito_theme::scale::space::XS);
            }
            let mut out = None;
            egui::ScrollArea::vertical()
                .id_salt(id.with("scroll"))
                .max_height(line_h * self.rows as f32 + 4.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        let (gr, _) = ui.allocate_exact_size(Vec2::new(gutter, 0.0), Sense::hover());
                        let o = egui::TextEdit::multiline(text)
                            .id(id)
                            .frame(egui::Frame::NONE)
                            .hint_text(RichText::new(self.hint).font(font.clone()).color(mix(c.bg, c.dim, 0.6)))
                            .font(font.clone())
                            .text_color(c.fg)
                            .code_editor()
                            .lock_focus(true)
                            .desired_rows(self.rows)
                            // A hair under the room: the editor rounds its width up to the layout grid, and a form that is a
                            // hair too wide makes an auto-sized parent grow every frame.
                            .desired_width((ui.available_width() - 1.0).max(40.0))
                            .show(ui);
                        // A number at the first row of each line of text.
                        let mut n = 0;
                        let mut starts = true;
                        for row in &o.galley.rows {
                            if starts {
                                n += 1;
                                let at = egui::pos2(gr.max.x - 8.0, o.galley_pos.y + row.pos.y);
                                ui.painter().text(at, egui::Align2::RIGHT_TOP, n.to_string(), font.clone(), mix(c.bg, c.dim, 0.6));
                            }
                            starts = row.ends_with_newline;
                        }
                        out = Some(o.response.response);
                    });
                });
            let r = out.expect("the editor is drawn");
            let focused = r.has_focus();
            (r, focused, self.error)
        });
        let name = self.name;
        let enabled = edit.enabled();
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, enabled, name));
        if self.error {
            ui.ctx().accesskit_node_builder(edit.id, |n| n.set_invalid(egui::accesskit::Invalid::True));
        }
        edit
    }
}

/// A section of a form, not a card: the title (13, strong, a heading) at the left of a row, the aside (small, dim, upper
/// case) at its right, a 1px line under the row, then the body.
pub fn fieldset<R>(ui: &mut Ui, l: Look, title: &str, aside: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    let c = l.c;
    ui.horizontal(|ui| {
        let r = ui.label(RichText::new(title).size(z(ui, 13.0)).strong().color(c.strong()));
        ito_a11y::say(&r, Role::Heading, title);
        if !aside.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(aside.to_uppercase()).size(z(ui, 10.0)).color(c.dim));
            });
        }
    });
    ui.add_space(ito_theme::scale::space::SM);
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, c.border));
    ui.add_space(ito_theme::scale::space::MD);
    let out = body(ui);
    ui.add_space(ito_theme::scale::space::LG);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> Look<'static> {
        Look { c: ito_theme::colors(), q: "" }
    }

    fn run(f: impl FnMut(&mut Ui)) {
        let ctx = egui::Context::default();
        let mut f = f;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(600.0, 600.0))),
                ..Default::default()
            },
            |ui| f(ui),
        );
        out.textures_delta.clear();
    }

    #[test]
    fn the_form_parts_draw() {
        let (mut a, mut b) = (String::from("30"), String::from("echo 1\necho 2\n"));
        run(|ui| {
            fieldset(ui, look(), "Run", "run", |ui| {
                for need in [Need::Required, Need::Optional, Need::Plain] {
                    form_label(ui, look(), "Timeout", need, "per line", &FormWords::ja());
                }
                InputField::new("t").name("Timeout").mark(Mark::Lock).unit("s").hint("30").mono().width(200.0).error(true).show(ui, &mut a);
                InputField::new("n").rows(3).show(ui, &mut b.clone());
                let r = CodeBox::new("c").title("run").aside("per line").rows(3).show(ui, &mut b);
                assert!(r.rect.height() > 0.0);
            });
        });
    }

    /// A form part never grows past the room it was given, even at a fractional scale: an auto-sized parent would grow every frame.
    #[test]
    fn the_parts_stay_inside_their_room() {
        for ppp in [1.0, 1.26, 1.5, 1.75] {
            for width in [363.125_f32, 300.3, 411.7] {
                let ctx = egui::Context::default();
                let mut over = Vec::new();
                let mut text = String::from(
                    "a
b",
                );
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0))),
                        viewports: std::iter::once((
                            egui::ViewportId::ROOT,
                            egui::ViewportInfo { native_pixels_per_point: Some(ppp), ..Default::default() },
                        ))
                        .collect(),
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_max_width(width);
                        let room = ui.max_rect().width();
                        CodeBox::new("c").aside("x").rows(4).show(ui, &mut text);
                        over.push(ui.min_rect().width() - room);
                        InputField::new("i").rows(3).show(ui, &mut text.clone());
                        over.push(ui.min_rect().width() - room);
                    },
                );
                out.textures_delta.clear();
                assert!(over.iter().all(|o| *o <= 1e-3), "ppp {ppp} width {width}: {over:?}");
            }
        }
    }

    #[test]
    fn the_label_is_spoken_with_its_need() {
        assert_eq!(FormWords::of("ja-JP").optional, "任意");
        assert_eq!(FormWords::of("en").required, "required");
    }
}
