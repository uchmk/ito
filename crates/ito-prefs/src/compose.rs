//! Parts of a compose window: a split button, recipient chips and a card that floats in a corner of the window.

use crate::mark::tone_look;
use crate::{focus_ring, mark, tag_hue, take_keys, z, Look, Mark, Tone};
use egui::{
    Align, Align2, Color32, Context, CornerRadius, FontId, Layout, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2,
};
use ito_a11y::Role as Said;
use ito_theme::mix;

/// What a click on a [`SplitButton`] did.
#[derive(Debug)]
pub struct SplitResponse {
    /// The left part, the main action.
    pub main: Response,
    /// The right part, a chevron that opens a menu. Put the popup under `menu.rect`.
    pub menu: Response,
}

/// A button in two parts: the main action (a mark, words, optionally a key chip) and a chevron that opens a menu. Only the outer
/// corners are round, and a thin line sits between the parts.
#[derive(Clone, Copy, Debug)]
pub struct SplitButton<'a> {
    mark: Mark,
    words: &'a str,
    menu_said: &'a str,
    tone: Tone,
    key: Option<&'a str>,
    mark_last: bool,
}

impl<'a> SplitButton<'a> {
    /// `words` are the main action's; `menu_said` is the chevron's tip and spoken name ("More send options").
    pub fn new(mark: Mark, words: &'a str, menu_said: &'a str) -> Self {
        SplitButton { mark, words, menu_said, tone: Tone::Plain, key: None, mark_last: false }
    }

    /// `Tone::Primary` for the one main action.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// The shortcut of the main action, as the app's keymap spells it.
    pub fn key(mut self, key: &'a str) -> Self {
        self.key = Some(key);
        self
    }

    /// The mark after the words rather than before.
    pub fn mark_last(mut self) -> Self {
        self.mark_last = true;
        self
    }

    pub fn show(self, ui: &mut Ui) -> SplitResponse {
        let c = ito_theme::colors();
        let font = egui::TextStyle::Body.resolve(ui.style());
        let galley = ui.painter().layout_no_wrap(self.words.to_owned(), font, Color32::PLACEHOLDER);
        let chip = self.key.map(|k| ui.painter().layout_no_wrap(k.to_owned(), FontId::proportional(z(ui, 10.5)), Color32::PLACEHOLDER));
        let (glyph, pad, gap) = (z(ui, 14.0), z(ui, 10.0), z(ui, 6.0));
        let chip_w = chip.as_ref().map_or(0.0, |g| (g.size().x + z(ui, 10.0)).max(z(ui, 18.0)));
        let mut main_w = pad + glyph + gap + galley.size().x + pad;
        if chip.is_some() {
            main_w += gap + chip_w;
        }
        let h = ui.spacing().interact_size.y.max(z(ui, 26.0));
        let menu_w = z(ui, 26.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(main_w + menu_w, h), Sense::hover());
        let main_rect = Rect::from_min_size(rect.min, Vec2::new(main_w, h));
        let menu_rect = Rect::from_min_max(Pos2::new(main_rect.right(), rect.top()), rect.max);
        let id = ui.id().with(("split", self.words));
        let main = ui.interact(main_rect, id.with("main"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let menu = ui
            .interact(menu_rect, id.with("menu"), Sense::click())
            .on_hover_text(self.menu_said)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        ito_a11y::say(&main, Said::Button, self.words);
        ito_a11y::say(&menu, Said::Button, self.menu_said);
        take_keys(ui, &main);
        if ui.is_rect_visible(rect) {
            let r = 6;
            let left = CornerRadius { nw: r, sw: r, ne: 0, se: 0 };
            let right = CornerRadius { nw: 0, sw: 0, ne: r, se: r };
            let (fill, edge, mark_ink, text_ink) = tone_look(&c, self.tone, main.hovered() || main.has_focus());
            let (mfill, medge, mmark, _) = tone_look(&c, self.tone, menu.hovered() || menu.has_focus());
            let p = ui.painter();
            p.rect(main_rect, left, fill, Stroke::new(1.0, edge), StrokeKind::Inside);
            p.rect(menu_rect, right, mfill, Stroke::new(1.0, medge), StrokeKind::Inside);
            let divider = if self.tone == Tone::Plain { c.border_strong() } else { mix(mfill, text_ink, 0.35) };
            p.line_segment(
                [main_rect.right_top() + Vec2::new(0.0, 1.0), main_rect.right_bottom() - Vec2::new(0.0, 1.0)],
                Stroke::new(1.0, divider),
            );
            let cy = rect.center().y;
            let glyph_at = |x: f32| Rect::from_center_size(Pos2::new(x + glyph / 2.0, cy), Vec2::splat(glyph));
            let mut x = rect.left() + pad;
            let text_w = galley.size().x;
            let text_pos = |x: f32| Pos2::new(x, cy - galley.size().y / 2.0);
            if self.mark_last {
                p.galley(text_pos(x), galley.clone(), text_ink);
                mark(p, glyph_at(x + text_w + gap), self.mark, mark_ink);
                x += text_w + gap + glyph;
            } else {
                mark(p, glyph_at(x), self.mark, mark_ink);
                p.galley(text_pos(x + glyph + gap), galley.clone(), text_ink);
                x += glyph + gap + text_w;
            }
            if let Some(g) = chip {
                let cr = Rect::from_min_size(Pos2::new(x + gap, cy - z(ui, 8.0)), Vec2::new(chip_w, z(ui, 16.0)));
                p.rect_stroke(cr, 4.0, Stroke::new(1.0, mix(fill, text_ink, 0.35)), StrokeKind::Inside);
                p.galley(cr.center() - g.size() / 2.0, g, mix(fill, text_ink, 0.8));
            }
            mark(p, Rect::from_center_size(menu_rect.center(), Vec2::splat(glyph)), Mark::ChevronDown, mmark);
        }
        focus_ring(ui, &main);
        focus_ring(ui, &menu);
        SplitResponse { main, menu }
    }
}

/// What [`chips`] did this frame.
#[derive(Debug)]
pub struct ChipsResponse {
    /// The list changed (a chip was added or removed).
    pub changed: bool,
    /// The chip taken away this frame, if any (by its ✕ or by Backspace in the empty field).
    pub removed: Option<String>,
    /// The field after the chips, for placing a list of suggestions under `input.rect` and for its focus.
    pub input: Response,
}

/// The name part of "Name <addr>", else the whole text; and the address.
fn split_address(s: &str) -> (&str, &str) {
    match (s.rfind('<'), s.rfind('>')) {
        (Some(a), Some(b)) if a < b => {
            let name = s[..a].trim().trim_matches('"');
            let addr = s[a + 1..b].trim();
            (if name.is_empty() { addr } else { name }, addr)
        }
        _ => (s.trim(), s.trim()),
    }
}

/// Move whole entries out of `input` into `list` (`,` `;` or a newline ends one), and with `all` the rest too. True if any moved.
fn commit(list: &mut Vec<String>, input: &mut String, all: bool) -> bool {
    let mut moved = false;
    loop {
        let cut = input.find([',', ';', '\n']);
        let piece = match cut {
            Some(i) => input[..i].to_owned(),
            None if all => input.clone(),
            None => break,
        };
        let piece = piece.trim().to_owned();
        match cut {
            Some(i) => input.replace_range(..=i, ""),
            None => input.clear(),
        }
        if !piece.is_empty() && !list.contains(&piece) {
            list.push(piece);
            moved = true;
        }
        if cut.is_none() {
            break;
        }
    }
    moved
}

/// Recipient chips: each entry of `list` as a round tag (an initial avatar, the text, a ✕), then a field with `hint` for the next
/// one. Enter, `,` or `;` (or leaving the field with text in it) turns the text into a chip, Backspace in the empty field takes
/// the last chip away. The chips wrap when the row is full. The app keeps `list` and the half-typed `input`.
pub fn chips(ui: &mut Ui, l: Look, list: &mut Vec<String>, input: &mut String, hint: &str) -> ChipsResponse {
    chips_with(ui, l, list, input, hint, "Remove")
}

/// [`chips`] with the words of each ✕ ("Remove", "削除"), for its tip and spoken name.
pub fn chips_with(ui: &mut Ui, l: Look, list: &mut Vec<String>, input: &mut String, hint: &str, remove_words: &str) -> ChipsResponse {
    let c = l.c;
    let mut removed = None;
    let mut changed = false;
    let was_empty = input.is_empty();
    let mut field = None;
    // The field's id is taken here, from the place of the row, so adding or taking away a chip (which moves the auto ids inside
    // the row) does not change it and the field keeps the focus.
    let field_id = ui.next_auto_id().with("ito-chips-input");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(z(ui, 6.0), z(ui, 4.0));
        let mut gone = None;
        for (i, text) in list.iter().enumerate() {
            let (name, addr) = split_address(text);
            let (h, pad, gap, av, x_side) = (z(ui, 24.0), z(ui, 3.0), z(ui, 6.0), z(ui, 18.0), z(ui, 14.0));
            let font = FontId::proportional(z(ui, 12.5));
            let label = if name == addr { addr.to_owned() } else { format!("{name} <{addr}>") };
            let galley = ui.painter().layout_no_wrap(label.clone(), font, c.fg);
            let w = (pad + av + gap + galley.size().x + gap + x_side + pad + z(ui, 2.0)).min(ui.available_width().max(z(ui, 80.0)));
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
            let x_rect =
                Rect::from_center_size(Pos2::new(rect.right() - pad - x_side / 2.0 - z(ui, 2.0), rect.center().y), Vec2::splat(x_side));
            let x = ui
                .interact(x_rect, ui.id().with(("chip-x", i, text.as_str())), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let x = x.on_hover_text(format!("{remove_words}: {label}"));
            ito_a11y::say(&x, Said::Button, &format!("{remove_words}: {label}"));
            if x.clicked() {
                gone = Some(i);
            }
            if ui.is_rect_visible(rect) {
                let p = ui.painter();
                p.rect(rect, h / 2.0, c.raised(), Stroke::new(1.0, c.border), StrokeKind::Inside);
                let hue = tag_hue(&c, addr);
                let centre = Pos2::new(rect.left() + pad + av / 2.0, rect.center().y);
                p.circle_filled(centre, av / 2.0, mix(c.side, hue, 0.4));
                let initial = name.chars().next().map(|ch| ch.to_uppercase().collect::<String>()).unwrap_or_else(|| "?".to_owned());
                p.text(centre, Align2::CENTER_CENTER, initial, FontId::proportional(av * 0.5), c.strong());
                let tx = rect.left() + pad + av + gap;
                let room = (x_rect.left() - gap - tx).max(0.0);
                let font = FontId::proportional(z(ui, 12.5));
                let width_of = |s: &str| p.layout_no_wrap(s.to_owned(), font.clone(), c.fg).size().x;
                let shown = crate::tag::fit(&label, room, width_of);
                p.text(Pos2::new(tx, rect.center().y), Align2::LEFT_CENTER, shown, font, c.fg);
                let ink = if x.hovered() { c.err } else { c.dim };
                mark(p, x_rect.shrink(x_side * 0.2), Mark::Close, ink);
            }
        }
        if let Some(i) = gone {
            removed = Some(list.remove(i));
            changed = true;
        }
        let room = ui.available_width().max(z(ui, 140.0));
        let r = ui.add(
            egui::TextEdit::singleline(input)
                .hint_text(hint)
                .id(field_id)
                .frame(egui::Frame::NONE)
                .desired_width(room)
                .margin(Vec2::new(2.0, 2.0)),
        );
        ito_a11y::say(&r, Said::Label, hint);
        field = Some(r);
    });
    let input_r = field.expect("the field is always added");
    if commit(list, input, false) {
        changed = true;
    }
    // `lost_focus` is true on the frame Enter lets go of the field, which also holds while the OS window is not focused.
    let enter = (input_r.has_focus() || input_r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if input_r.lost_focus() || enter {
        if commit(list, input, true) {
            changed = true;
        }
        if enter {
            input_r.request_focus();
        }
    }
    if was_empty && input.is_empty() && input_r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Backspace)) {
        if let Some(last) = list.pop() {
            removed = Some(last);
            changed = true;
        }
    }
    ChipsResponse { changed, removed, input: input_r }
}

/// The words of a [`FloatCard`]'s buttons, for their tips and spoken names.
#[derive(Clone, Copy, Debug)]
pub struct FloatWords<'a> {
    pub minimize: &'a str,
    /// The minimize button while the card is already shrunk.
    pub unminimize: &'a str,
    pub expand: &'a str,
    pub restore: &'a str,
    pub close: &'a str,
}

impl Default for FloatWords<'_> {
    fn default() -> Self {
        FloatWords { minimize: "Minimize", unminimize: "Restore", expand: "Full screen", restore: "Exit full screen", close: "Close" }
    }
}

/// What a [`FloatCard`] did this frame.
#[derive(Debug)]
pub struct FloatResponse<R> {
    /// The ─ button (or the title of a shrunk card) was clicked: flip the app's `minimized`.
    pub minimize: bool,
    /// The ⤢ button was clicked: flip the app's `expanded`.
    pub expand: bool,
    /// The ✕ button was clicked.
    pub close: bool,
    /// What `add` returned; `None` while the card is shrunk to its title.
    pub inner: Option<R>,
    /// The whole card, to know what it covers.
    pub rect: Rect,
}

/// A card with rounded corners and a shadow, floating in a corner of the window (a compose window, a small tool). Its head is a
/// title, an optional small badge and the buttons ─ ⤢ ✕ (✕ turns red under the pointer). Whether it is shrunk to the head or
/// fills the window is the app's: pass `minimized` and `expanded` and flip them on the matching [`FloatResponse`] flag.
#[derive(Clone, Copy, Debug)]
pub struct FloatCard<'a> {
    id: egui::Id,
    title: &'a str,
    badge: Option<&'a str>,
    corner: Align2,
    size: Vec2,
    avoid_top: f32,
    avoid_bottom: f32,
    minimized: bool,
    expanded: bool,
    words: FloatWords<'a>,
}

impl<'a> FloatCard<'a> {
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug, title: &'a str) -> Self {
        FloatCard {
            id: egui::Id::new(id),
            title,
            badge: None,
            corner: Align2::RIGHT_BOTTOM,
            size: Vec2::new(580.0, 540.0),
            avoid_top: 0.0,
            avoid_bottom: 0.0,
            minimized: false,
            expanded: false,
            words: FloatWords::default(),
        }
    }

    /// A small tag after the title ("Editing a draft").
    pub fn badge(mut self, badge: &'a str) -> Self {
        self.badge = Some(badge);
        self
    }

    /// The corner it sits in; the default is the bottom right.
    pub fn corner(mut self, corner: Align2) -> Self {
        self.corner = corner;
        self
    }

    /// The card's size in points before the letters' zoom; it never grows past the window.
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.size = Vec2::new(w, h);
        self
    }

    /// A height at the top of the window to keep clear (the title bar), in points at zoom 1: it follows the letters' zoom, so
    /// pass `ito_titlebar::HEIGHT` as it is. The card, expanded or not, stays below it.
    pub fn avoid_top(mut self, h: f32) -> Self {
        self.avoid_top = h;
        self
    }

    /// A height at the bottom of the window to keep clear (a status bar), in points at zoom 1: it follows the letters' zoom,
    /// so pass `ito_statusbar::HEIGHT` as it is.
    pub fn avoid_bottom(mut self, h: f32) -> Self {
        self.avoid_bottom = h;
        self
    }

    pub fn minimized(mut self, on: bool) -> Self {
        self.minimized = on;
        self
    }

    pub fn expanded(mut self, on: bool) -> Self {
        self.expanded = on;
        self
    }

    pub fn words(mut self, words: FloatWords<'a>) -> Self {
        self.words = words;
        self
    }

    /// The card's rectangle in `screen` (the window less the status bar).
    fn place(&self, zoom: f32, screen: Rect) -> Rect {
        let margin = 16.0 * zoom;
        let head = 36.0 * zoom;
        let room = Rect::from_min_max(
            screen.min + Vec2::new(margin, margin + self.avoid_top * zoom),
            screen.max - Vec2::new(margin, margin + self.avoid_bottom * zoom),
        );
        let want = if self.expanded {
            room.size()
        } else {
            let h = if self.minimized { head } else { self.size.y * zoom };
            Vec2::new(self.size.x * zoom, h)
        };
        // The frame's outline is 2 points more than what it holds, so the head fits in `head + 2`.
        let size = Vec2::new(want.x.min(room.width()).max(0.0), want.y.min(room.height()).max((head + 2.0).min(room.height())));
        self.corner.align_size_within_rect(size, room)
    }

    pub fn show<R>(self, ctx: &Context, l: Look, add: impl FnOnce(&mut Ui) -> R) -> FloatResponse<R> {
        let c = l.c;
        let zoom = ito_theme::zoomed(ctx, 1.0);
        let card = self.place(zoom, ctx.content_rect());
        let head = 36.0 * zoom;
        let (mut minimize, mut expand, mut close) = (false, false, false);
        let mut inner = None;
        let area = egui::Area::new(self.id).order(egui::Order::Foreground).fixed_pos(card.min).constrain(false).show(ctx, |ui| {
            let frame = egui::Frame::new()
                .fill(c.side)
                .stroke(Stroke::new(1.0, c.border_strong()))
                .corner_radius(ito_theme::scale::radius::BOX)
                .shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 2, color: Color32::from_black_alpha(90) });
            frame.show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let (w, h) = (card.width() - 2.0, (card.height() - 2.0).max(head));
                ui.set_min_size(Vec2::new(w, h));
                ui.set_max_size(Vec2::new(w, h));
                let head_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(w, head));
                ui.allocate_rect(head_rect, Sense::hover());
                // A shrunk card opens again from anywhere on its head; the buttons are on top of this.
                let whole = ui.interact(head_rect, self.id.with("head"), Sense::click());
                if self.minimized && whole.clicked() {
                    minimize = true;
                }
                let mut hd = ui.new_child(
                    UiBuilder::new().max_rect(head_rect.shrink2(Vec2::new(12.0 * zoom, 0.0))).layout(Layout::right_to_left(Align::Center)),
                );
                hd.spacing_mut().item_spacing = Vec2::new(2.0 * zoom, 0.0);
                let side = 26.0 * zoom;
                close = head_button(&mut hd, Mark::Close, self.words.close, side, true).clicked();
                let (em, ew) = if self.expanded { (Mark::Restore, self.words.restore) } else { (Mark::Expand, self.words.expand) };
                expand = head_button(&mut hd, em, ew, side, false).clicked();
                let mw = if self.minimized { self.words.unminimize } else { self.words.minimize };
                if head_button(&mut hd, Mark::Minimize, mw, side, false).clicked() {
                    minimize = true;
                }
                hd.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0 * zoom;
                    ui.add(egui::Label::new(egui::RichText::new(self.title).color(c.strong()).strong()).truncate());
                    if let Some(b) = self.badge {
                        badge(ui, c.dim, c.border, b);
                    }
                });
                if !self.minimized {
                    let line = Rect::from_min_size(head_rect.left_bottom(), Vec2::new(w, 1.0));
                    ui.painter().rect_filled(line, 0.0, c.border);
                    let body = Rect::from_min_max(
                        Pos2::new(head_rect.left(), head_rect.bottom() + 1.0),
                        Pos2::new(head_rect.right(), head_rect.top() + h),
                    );
                    ui.allocate_rect(body, Sense::hover());
                    let mut b = ui.new_child(UiBuilder::new().max_rect(body.shrink2(Vec2::new(12.0 * zoom, 8.0 * zoom))));
                    b.set_clip_rect(body.intersect(ui.clip_rect()));
                    inner = Some(add(&mut b));
                }
            });
        });
        FloatResponse { minimize, expand, close, inner, rect: area.response.rect }
    }
}

/// A small tag: `ink` words in a thin `edge` outline.
fn badge(ui: &mut Ui, ink: Color32, edge: Color32, text: &str) {
    let font = FontId::proportional(z(ui, 10.5));
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let size = galley.size() + Vec2::new(z(ui, 12.0), z(ui, 4.0));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_stroke(rect, rect.height() / 2.0, Stroke::new(1.0, edge), StrokeKind::Inside);
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    }
}

/// A head button: no outline until the pointer is on it. The `danger` one turns red.
fn head_button(ui: &mut Ui, m: Mark, said: &str, side: f32, danger: bool) -> Response {
    let c = ito_theme::colors();
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let r = r.on_hover_text(said).on_hover_cursor(egui::CursorIcon::PointingHand);
    ito_a11y::say(&r, Said::Button, said);
    take_keys(ui, &r);
    if ui.is_rect_visible(rect) {
        let hot = r.hovered() || r.has_focus();
        let (fill, ink) = match (hot, danger) {
            (true, true) => (mix(c.side, c.err, 0.2), c.err),
            (true, false) => (c.hover(), c.fg),
            _ => (Color32::TRANSPARENT, c.dim),
        };
        ui.painter().rect_filled(rect, 6.0, fill);
        mark(ui.painter(), rect.shrink(side * 0.27), m, ink);
    }
    focus_ring(ui, &r);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> Look<'static> {
        Look { c: ito_theme::colors(), q: "" }
    }

    fn screen() -> egui::RawInput {
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))), ..Default::default() }
    }

    #[test]
    fn a_split_button_is_wider_than_its_main_part_and_has_two_responses() {
        let ctx = Context::default();
        let (mut plain, mut keyed, mut main_w, mut menu_w) = (0.0, 0.0, 0.0, 0.0);
        let mut out = ctx.run_ui(screen(), |ui| {
            let a = SplitButton::new(Mark::Send, "Send", "More").tone(Tone::Primary).mark_last().show(ui);
            let b = SplitButton::new(Mark::Send, "Send", "More").key("Ctrl+Enter").show(ui);
            (plain, keyed) = (a.main.rect.width() + a.menu.rect.width(), b.main.rect.width() + b.menu.rect.width());
            (main_w, menu_w) = (a.main.rect.width(), a.menu.rect.width());
            assert!((a.main.rect.right() - a.menu.rect.left()).abs() < 0.01);
        });
        out.textures_delta.clear();
        assert!(keyed > plain && main_w > menu_w, "{plain} {keyed} {main_w} {menu_w}");
    }

    #[test]
    fn typed_entries_become_chips() {
        let (mut list, mut input) = (vec![], "a@x.jp, 田中 一郎 <t@x.jp>;b@x.jp".to_owned());
        assert!(commit(&mut list, &mut input, false));
        assert_eq!(list, ["a@x.jp", "田中 一郎 <t@x.jp>"]);
        assert_eq!(input, "b@x.jp");
        assert!(commit(&mut list, &mut input, true));
        assert_eq!(list.len(), 3);
        assert!(input.is_empty());
        input = "a@x.jp".to_owned();
        assert!(!commit(&mut list, &mut input, true), "no twice");
        assert_eq!(split_address("田中 一郎 <t@x.jp>"), ("田中 一郎", "t@x.jp"));
        assert_eq!(split_address("<t@x.jp>"), ("t@x.jp", "t@x.jp"));
        assert_eq!(split_address("t@x.jp"), ("t@x.jp", "t@x.jp"));
    }

    #[test]
    fn chips_draw_and_a_cross_takes_one_away() {
        let ctx = Context::default();
        let mut list = vec!["a@x.jp".to_owned(), "田中 一郎 <t@x.jp>".to_owned()];
        let mut input = String::new();
        let mut rects = vec![];
        let mut out = ctx.run_ui(screen(), |ui| {
            let r = chips(ui, look(), &mut list, &mut input, "To");
            assert!(!r.changed);
            rects.push(r.input.rect);
        });
        out.textures_delta.clear();
        assert!(rects[0].width() > 100.0);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn the_field_keeps_its_id_when_a_chip_comes_and_goes() {
        let ctx = Context::default();
        let mut input = String::new();
        let mut ids = vec![];
        for n in [0usize, 1, 3, 0] {
            let mut list: Vec<String> = (0..n).map(|k| format!("a{k}@x.jp")).collect();
            let mut out = ctx.run_ui(screen(), |ui| {
                ids.push(chips(ui, look(), &mut list, &mut input, "To").input.id);
            });
            out.textures_delta.clear();
        }
        assert!(ids.windows(2).all(|w| w[0] == w[1]), "{ids:?}");
    }

    #[test]
    fn a_float_card_stays_below_the_top_and_follows_the_zoom() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0));
        let card = FloatCard::new("c", "t").avoid_top(40.0).avoid_bottom(24.0).expanded(true);
        for zoom in [1.0, 1.5] {
            let r = card.place(zoom, screen);
            assert!(r.top() >= (16.0 + 40.0) * zoom - 0.5 && r.bottom() <= screen.bottom() - (16.0 + 24.0) * zoom + 0.5, "{r:?}");
        }
    }

    #[test]
    fn a_float_card_sits_in_its_corner_clear_of_the_bottom() {
        let ctx = Context::default();
        let mut got = vec![];
        for (min, exp) in [(false, false), (true, false), (false, true)] {
            let mut out = ctx.run_ui(screen(), |ui| {
                let r = FloatCard::new("c", "New message")
                    .badge("Draft")
                    .size(580.0, 540.0)
                    .avoid_bottom(24.0)
                    .minimized(min)
                    .expanded(exp)
                    .show(ui.ctx(), look(), |ui| ui.label("body"));
                assert_eq!(r.inner.is_some(), !min);
                got.push(r.rect);
            });
            out.textures_delta.clear();
        }
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0));
        assert!(got[0].right() <= screen.right() - 15.0 && got[0].bottom() <= screen.bottom() - 24.0 - 15.0, "{:?}", got[0]);
        assert!(got[1].height() < 60.0 && (got[1].bottom() - got[0].bottom()).abs() < 2.0, "{:?} {:?}", got[1], got[0]);
        assert!(got[2].width() > 1100.0 && got[2].bottom() <= screen.bottom() - 24.0, "{:?}", got[2]);
    }
}
