//! An emoji picker: a button's popup with a row of categories over a grid of emoji. The app draws the button (for example
//! `mark_button(.., Mark::Emoji, ..)`) and passes its response; the picker opens on a click of it, goes above it (it is meant
//! for a card's foot), and shuts on a pick, on Esc and on a press outside.

use egui::{Align2, Color32, Context, FontId, Id, Order, Response, Sense, Stroke, Ui, Vec2};
use ito_a11y::Role;
use ito_theme::mix;

use crate::{focus_ring, z, Look};

/// Each category's name (its spoken name) in the order of [`CATEGORIES`].
#[derive(Clone, Copy, Debug)]
pub struct EmojiWords<'a> {
    pub categories: [&'a str; 5],
}

impl EmojiWords<'static> {
    pub fn en() -> Self {
        EmojiWords { categories: ["Smileys", "Gestures", "Hearts", "Things", "Symbols"] }
    }

    pub fn ja() -> Self {
        EmojiWords { categories: ["顔", "手", "ハート", "もの", "記号"] }
    }

    /// The words of the language code (`"ja"`, else English).
    pub fn for_lang(code: &str) -> Self {
        if code.starts_with("ja") {
            Self::ja()
        } else {
            Self::en()
        }
    }
}

impl Default for EmojiWords<'static> {
    fn default() -> Self {
        Self::en()
    }
}

/// The emoji of each category: the tab shows the first one.
pub const CATEGORIES: [&[&str]; 5] = [
    &[
        "😀", "😃", "😄", "😁", "😆", "😅", "😂", "🤣", "😊", "😇", "🙂", "🙃", "😉", "😌", "😍", "🥰", "😘", "😋", "😛", "😜", "🤪", "🤔",
        "🤨", "😐", "😑", "😶", "🙄", "😏", "😒", "😞", "😔", "😟", "😕", "🙁", "😣", "😖", "😫", "😩", "🥺", "😢", "😭", "😤", "😠", "😡",
        "🤯", "😳", "😱", "😨", "😰", "😥", "😓", "🤗", "🫡", "😴", "🥱", "😷", "🤒", "🤕", "🤢", "🥳", "😎", "🤓",
    ],
    &[
        "👍", "👎", "👌", "✌️", "🤞", "🤟", "🤘", "🤙", "👈", "👉", "👆", "👇", "☝️", "✋", "🤚", "🖐️", "🖖", "👋", "🤝", "🙏", "👏", "🙌",
        "👐", "🤲", "💪", "✍️", "🤳", "👀", "👂", "🧠",
    ],
    &[
        "❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "🤍", "🤎", "💔", "❣️", "💕", "💞", "💓", "💗", "💖", "💘", "💝", "💯", "💢", "💥", "💫",
        "💦", "✨",
    ],
    &[
        "📧", "📅", "📎", "📌", "📝", "📄", "📁", "📂", "📊", "📈", "📉", "🔍", "🔒", "🔓", "🔑", "🔔", "🔕", "💡", "🔧", "🔨", "⚙️", "💻",
        "📱", "☎️", "🎁", "🎉", "🎂", "☕", "🍺", "🍕", "🏠", "🚗", "✈️", "⏰", "⌛",
    ],
    &[
        "✅", "❌", "⭕", "❓", "❗", "⚠️", "🚫", "➕", "➖", "➡️", "⬅️", "⬆️", "⬇️", "🔄", "▶️", "⏸️", "⏹️", "⭐", "🌟", "🔥", "🌈", "☀️",
        "🌙", "☔",
    ],
];

#[derive(Clone, Copy, Default)]
struct Pop {
    open: bool,
    tab: usize,
    /// The last frame the picker was called in: a gap means its card went away, so it is shut.
    seen: u64,
}

/// Whether the picker `id` is open: ask before the app's own Esc handling, which would otherwise close the card the
/// picker is in (the picker takes its Esc only when it is drawn).
pub fn emoji_picker_open(ctx: &Context, id: &str) -> bool {
    ctx.data(|d| d.get_temp::<Pop>(Id::new(("ito-emoji", id)))).is_some_and(|p| p.open)
}

/// The emoji picker for `anchor`, the app's button. `id` tells this picker from another. The emoji picked this frame, if any.
pub fn emoji_picker(ui: &mut Ui, l: Look, id: &str, anchor: &Response, words: &EmojiWords) -> Option<&'static str> {
    let ctx = ui.ctx().clone();
    let key = Id::new(("ito-emoji", id));
    let mut st: Pop = ctx.data_mut(|d| d.get_temp(key)).unwrap_or_default();
    let frame = ctx.cumulative_frame_nr();
    if st.open && frame > st.seen + 1 {
        st.open = false;
    }
    st.seen = frame;
    if anchor.clicked() {
        st.open = !st.open;
    }
    let mut picked = None;
    if st.open {
        picked = popup(&ctx, l, key, anchor, words, &mut st);
    }
    ctx.data_mut(|d| d.insert_temp(key, st));
    picked
}

fn popup(ctx: &Context, l: Look, id: Id, anchor: &Response, words: &EmojiWords, st: &mut Pop) -> Option<&'static str> {
    let c = l.c;
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        st.open = false;
        return None;
    }
    let zoom = ito_theme::zoomed(ctx, 1.0);
    let (cell, pad, cols) = (30.0 * zoom, 8.0 * zoom, 8usize);
    let mut picked = None;
    let area = egui::Area::new(id.with("grid"))
        .order(Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(anchor.rect.left_top() - Vec2::new(0.0, 4.0))
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(c.side)
                .stroke(Stroke::new(1.0, c.border_strong()))
                .corner_radius(ito_theme::scale::radius::BOX)
                .inner_margin(pad)
                .shadow(egui::Shadow { offset: [0, 6], blur: 20, spread: 1, color: Color32::from_black_alpha(80) })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    ui.set_width(cell * cols as f32);
                    ui.horizontal(|ui| {
                        for (k, list) in CATEGORIES.iter().enumerate() {
                            if cell_button(ui, id.with(("tab", k)), list[0], words.categories[k], cell, c, k == st.tab).clicked() {
                                st.tab = k;
                            }
                        }
                    });
                    ui.add_space(pad / 2.0);
                    let list = CATEGORIES[st.tab.min(CATEGORIES.len() - 1)];
                    for (r, row) in list.chunks(cols).enumerate() {
                        ui.horizontal(|ui| {
                            for (k, e) in row.iter().enumerate() {
                                if cell_button(ui, id.with(("e", r, k)), e, e, cell, c, false).clicked() {
                                    picked = Some(*e);
                                }
                            }
                        });
                    }
                });
        });
    if picked.is_some() {
        st.open = false;
    }
    let outside = ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !anchor.rect.contains(p))
    });
    if outside {
        st.open = false;
    }
    picked
}

/// One square holding `glyph`, named `said`; `on` for the category shown.
fn cell_button(ui: &mut Ui, id: Id, glyph: &str, said: &str, size: f32, c: ito_theme::Colors, on: bool) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let r = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    ito_a11y::say(&r, Role::Button, said);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if on {
            p.rect_filled(rect.shrink(1.0), 6.0, mix(c.side, c.run, 0.25));
        } else if r.hovered() {
            p.rect_filled(rect.shrink(1.0), 6.0, c.hover());
        }
        p.text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(z(ui, 17.0)), c.fg);
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

    fn click(at: egui::Pos2) -> Vec<egui::Event> {
        let b =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        vec![egui::Event::PointerMoved(at), b(true), b(false)]
    }

    /// The anchor's click opens it; a click on the first emoji picks it and shuts it.
    #[test]
    fn a_click_opens_it_and_a_pick_shuts_it() {
        let ctx = Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let button = egui::Rect::from_min_size(egui::pos2(300.0, 400.0), Vec2::splat(26.0));
        let frame = |events: Vec<egui::Event>| {
            let mut got = None;
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                let r = ui.interact(button, Id::new("emoji-btn"), Sense::click());
                got = emoji_picker(ui, look(), "t", &r, &EmojiWords::en());
            });
            out.textures_delta.clear();
            got
        };
        let open = || emoji_picker_open(&ctx, "t");
        frame(vec![]);
        for e in click(button.center()) {
            frame(vec![e]);
        }
        assert!(open());
        frame(vec![]);
        let grid = ctx.memory(|m| m.area_rect(Id::new(("ito-emoji", "t")).with("grid"))).expect("the grid is laid out");
        // The first emoji sits under the tab row: pad 8, one cell of tabs (30), half a pad (4).
        let at = grid.left_top() + Vec2::new(8.0 + 15.0, 8.0 + 30.0 + 4.0 + 15.0);
        let mut got = None;
        for e in click(at) {
            got = got.or(frame(vec![e]));
        }
        assert_eq!(got, Some("😀"));
        assert!(!open());
    }

    #[test]
    fn escape_shuts_it() {
        let ctx = Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0));
        let button = egui::Rect::from_min_size(egui::pos2(300.0, 400.0), Vec2::splat(26.0));
        let frame = |events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                let r = ui.interact(button, Id::new("emoji-btn"), Sense::click());
                emoji_picker(ui, look(), "t", &r, &EmojiWords::en());
            });
            out.textures_delta.clear();
        };
        let open = || emoji_picker_open(&ctx, "t");
        frame(vec![]);
        for e in click(button.center()) {
            frame(vec![e]);
        }
        assert!(open());
        let esc =
            egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
        frame(vec![esc]);
        assert!(!open());
    }

    #[test]
    fn every_category_has_a_name_and_emoji() {
        let w = EmojiWords::ja();
        assert_eq!(w.categories.len(), CATEGORIES.len());
        assert!(CATEGORIES.iter().all(|c| !c.is_empty()));
        assert_eq!(EmojiWords::for_lang("ja-JP").categories[0], "顔");
    }
}
