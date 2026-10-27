//! The window's bottom status bar, the same in every app that draws one.
//!
//! [`show`] fills the rect it is given (the app's bottom panel): the side
//! colour with a line on top, the app's items from the left, the clock at the
//! right end as docs/common-spec.md has it (monospace, the strongest text
//! colour, the whole date on hover, drawn again only at the minute), and,
//! for a few seconds after [`scale_changed`], the scale's words just left of
//! the clock. The items are the app's: [`item`], [`link`], [`divider`] and
//! [`dot`] draw them in the bar's look. Colours are `ito_theme::colors()`;
//! no language or key is known here.
//!
//! The app hides the bar while its settings screen is open.

use std::time::Duration;

use egui::{Align, Color32, Context, FontId, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2};
use ito_common::Clock;

/// The bar's usual height.
pub const HEIGHT: f32 = 26.0;

/// How long the scale's words stay.
const SCALE_SHOWN: Duration = Duration::from_secs(2);

/// What the bar needs from the app.
#[derive(Clone, Copy, Debug)]
pub struct Spec<'a> {
    /// common.toml's clock (`Common::clock_or`); `show = false` hides it.
    pub clock: &'a Clock,
    /// The language the clock's words are in (`ja`, `en`).
    pub lang: &'a str,
    /// 0.0 to 1.0, for an app that fades its chrome while the person types.
    /// Everything is drawn at this opacity; the bar still takes its room.
    pub opacity: f32,
}

/// The bar, filling `ui`'s rect: `left` runs from the left edge, `right`
/// from just left of the clock (and of the scale's words) towards the left,
/// in a right-to-left layout.
pub fn show(ui: &mut Ui, spec: &Spec, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    let c = ito_theme::colors();
    let fade = |col: Color32| col.gamma_multiply(spec.opacity.clamp(0.0, 1.0));
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, fade(c.side));
    ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, fade(c.border)));
    ui.set_opacity(spec.opacity.clamp(0.0, 1.0));
    let inner = Rect::from_min_max(rect.min + Vec2::new(12.0, 1.0), rect.max - Vec2::new(12.0, 0.0));
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
    left(&mut bar);
    let mut end = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Center)));
    if spec.clock.show {
        let now = chrono::Local::now().naive_local();
        let r = end.label(RichText::new(spec.clock.text(&now, spec.lang)).font(FontId::monospace(12.0)).color(c.strong()));
        r.on_hover_text(Clock::full(&now, spec.lang));
        ui.ctx().request_repaint_after(Clock::next_minute(&now));
    }
    if let Some(words) = scale_words(ui.ctx(), spec.lang) {
        end.add_space(16.0);
        end.label(RichText::new(words).font(FontId::monospace(11.5)).color(c.wait));
    }
    end.add_space(16.0);
    right(&mut end);
}

/// Plain words in the bar.
pub fn item(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(RichText::new(text).size(12.0).color(color));
    ui.add_space(10.0);
}

/// A word that is a link: true when it was clicked.
pub fn link(ui: &mut Ui, text: &str) -> bool {
    let c = ito_theme::colors();
    let r = ui.add(egui::Label::new(RichText::new(text).size(12.0).color(c.blue).underline()).sense(Sense::click()));
    ui.add_space(10.0);
    r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// A thin line between two items.
pub fn divider(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 14.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, ito_theme::colors().border);
    ui.add_space(10.0);
}

/// A small coloured dot (a state), before the item it belongs to.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 3.5, color);
    ui.add_space(4.0);
}

/// The scale was set to `scale`: the bar says so for a few seconds.
pub fn scale_changed(ctx: &Context, scale: f32) {
    let at = ctx.input(|i| i.time);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("ito-statusbar-scale"), (scale, at)));
    ctx.request_repaint_after(SCALE_SHOWN);
}

fn scale_words(ctx: &Context, lang: &str) -> Option<String> {
    let id = egui::Id::new("ito-statusbar-scale");
    let (scale, at) = ctx.data(|d| d.get_temp::<(f32, f64)>(id))?;
    let left = SCALE_SHOWN.as_secs_f64() - (ctx.input(|i| i.time) - at);
    if left <= 0.0 {
        ctx.data_mut(|d| d.remove::<(f32, f64)>(id));
        return None;
    }
    ctx.request_repaint_after(Duration::from_secs_f64(left));
    Some(ito_common::scale_text(scale, lang))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ctx: &Context, time: f64, f: impl FnOnce(&mut Ui)) {
        let screen = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(600.0, 200.0));
        let input = egui::RawInput { time: Some(time), screen_rect: Some(screen), ..Default::default() };
        let mut f = Some(f);
        let mut out = ctx.run_ui(input, |ui| {
            if let Some(f) = f.take() {
                f(ui);
            }
        });
        out.textures_delta.clear();
    }

    #[test]
    fn the_scale_shows_for_two_seconds() {
        let ctx = Context::default();
        run(&ctx, 10.0, |ui| scale_changed(ui.ctx(), 1.2));
        run(&ctx, 11.0, |ui| assert_eq!(scale_words(ui.ctx(), "en").as_deref(), Some("Scale 120%")));
        run(&ctx, 12.5, |ui| assert_eq!(scale_words(ui.ctx(), "en"), None));
    }

    #[test]
    fn the_bar_draws_with_and_without_the_clock() {
        let ctx = Context::default();
        for show_clock in [true, false] {
            let clock = Clock { show: show_clock, ..Clock::default() };
            let spec = Spec { clock: &clock, lang: "ja", opacity: 0.5 };
            let mut seen = 0;
            run(&ctx, 0.0, |ui| {
                show(ui, &spec, |ui| item(ui, "words", Color32::WHITE), |ui| seen += usize::from(!link(ui, "x")));
            });
            assert_eq!(seen, 1);
        }
    }
}
