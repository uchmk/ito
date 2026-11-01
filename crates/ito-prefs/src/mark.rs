//! Drawn marks (a magnifier, an arrow onto a tray, a play triangle ...) and the quiet outlined buttons built on them.
//!
//! The marks are strokes, not font glyphs, so they follow every theme and high contrast; the colours are the theme's
//! (`raised` / `border` / `hover` / `border_strong` / `fg` / `dim`, the focus ring `run`).

use crate::{focus_ring, take_keys, z};
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use ito_a11y::Role as Said;
use std::f32::consts::PI;

/// A mark. Each is drawn in a 14 x 14 box, scaled to the rect it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A magnifying glass.
    Search,
    /// An arrow down onto a tray.
    Download,
    /// An arrow up from a tray.
    Upload,
    /// A triangle pointing on: start again.
    Play,
    /// Two bars: stop for now.
    Pause,
    /// A chevron pointing left (open a pane / back).
    ChevronLeft,
    /// A chevron pointing right (fold a pane / on).
    ChevronRight,
    Plus,
    /// A cross.
    Close,
    /// A circling arrow.
    Refresh,
    /// Two sheets, one over the other.
    Copy,
}

impl Mark {
    /// Every mark, for a gallery or a test.
    pub const ALL: [Mark; 11] = [
        Mark::Search,
        Mark::Download,
        Mark::Upload,
        Mark::Play,
        Mark::Pause,
        Mark::ChevronLeft,
        Mark::ChevronRight,
        Mark::Plus,
        Mark::Close,
        Mark::Refresh,
        Mark::Copy,
    ];
}

/// The box a mark is designed in.
const BOX: f32 = 14.0;

/// Draw `mark` centred in `rect` (scaled by the smaller side over 14) with 1.5 strokes in `colour`.
pub fn mark(p: &Painter, rect: Rect, mark: Mark, colour: Color32) {
    let k = rect.width().min(rect.height()) / BOX;
    let m = rect.center();
    let at = |x: f32, y: f32| m + Vec2::new(x, y) * k;
    let line = Stroke::new(1.5 * k, colour);
    let path = |pts: Vec<Pos2>| Shape::line(pts, line);
    match mark {
        Mark::Search => {
            let o = at(-1.5, -1.5);
            p.circle_stroke(o, 4.5 * k, line);
            p.line_segment([o + Vec2::splat(3.3 * k), o + Vec2::splat(7.0 * k)], line);
        }
        Mark::Download | Mark::Upload => {
            let d = if mark == Mark::Download { 1.0 } else { -1.0 };
            // The tray is under the arrow for a download, over it for an upload.
            let (tip, tail) = (at(0.0, 2.5 * d), at(0.0, -6.0 * d));
            p.line_segment([tail, tip], line);
            p.add(path(vec![at(-3.5, -d), tip, at(3.5, -d)]));
            p.add(path(vec![at(-6.0, 3.0 * d), at(-6.0, 6.0 * d), at(6.0, 6.0 * d), at(6.0, 3.0 * d)]));
        }
        Mark::Play => {
            p.add(Shape::convex_polygon(vec![at(-3.5, -5.5), at(5.0, 0.0), at(-3.5, 5.5)], Color32::TRANSPARENT, line));
        }
        Mark::Pause => {
            for x in [-2.5, 2.5] {
                p.line_segment([at(x, -5.0), at(x, 5.0)], Stroke::new(2.0 * k, colour));
            }
        }
        Mark::ChevronLeft | Mark::ChevronRight => {
            let d = if mark == Mark::ChevronRight { 1.0 } else { -1.0 };
            p.add(path(vec![at(-2.5 * d, -4.5), at(2.5 * d, 0.0), at(-2.5 * d, 4.5)]));
        }
        Mark::Plus => {
            p.line_segment([at(-5.0, 0.0), at(5.0, 0.0)], line);
            p.line_segment([at(0.0, -5.0), at(0.0, 5.0)], line);
        }
        Mark::Close => {
            p.line_segment([at(-4.5, -4.5), at(4.5, 4.5)], line);
            p.line_segment([at(4.5, -4.5), at(-4.5, 4.5)], line);
        }
        Mark::Refresh => {
            // Most of a circle, open at the upper right, with a head on the clockwise end.
            let (r, from, to) = (5.0, -0.4 * PI, 1.2 * PI);
            let arc: Vec<Pos2> = (0..=18).map(|i| from + (to - from) * i as f32 / 18.0).map(|a| at(r * a.cos(), r * a.sin())).collect();
            p.add(path(arc));
            let (tip, t, n) = (at(r * to.cos(), r * to.sin()), Vec2::new(-to.sin(), to.cos()), Vec2::new(to.cos(), to.sin()));
            p.add(path(vec![tip - t * 3.0 * k + n * 2.5 * k, tip, tip - t * 3.0 * k - n * 2.5 * k]));
        }
        Mark::Copy => {
            // The sheet behind shows only its top and left; the one in front is whole.
            p.add(path(vec![at(-2.0, 2.0), at(-6.0, 2.0), at(-6.0, -6.0), at(2.0, -6.0), at(2.0, -2.0)]));
            p.rect_stroke(Rect::from_min_max(at(-2.0, -2.0), at(6.0, 6.0)), 1.5 * k, line, StrokeKind::Middle);
        }
    }
}

/// A quiet outlined button with `mark` before `words`: the `raised` fill and a thin `border`, `hover` and `border_strong` under
/// the pointer or with the keys, the mark in `dim` (`fg` when hot), the words in the body size of `ui`. `said` is its spoken
/// name, and its tip when it differs from the words.
pub fn icon_button(ui: &mut Ui, m: Mark, words: &str, said: &str) -> Response {
    let c = ito_theme::colors();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(words.to_owned(), font, c.fg);
    let (glyph, pad, gap) = (z(ui, 14.0), z(ui, 10.0), z(ui, 6.0));
    let size = Vec2::new(pad + glyph + gap + galley.size().x + pad, ui.spacing().interact_size.y.max(z(ui, 26.0)));
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
    let r = if said != words { r.on_hover_text(said) } else { r };
    ito_a11y::say(&r, Said::Button, said);
    take_keys(ui, &r);
    if ui.is_rect_visible(rect) {
        let hot = r.hovered() || r.has_focus();
        let (fill, edge) = if hot { (c.hover(), c.border_strong()) } else { (c.raised(), c.border) };
        let p = ui.painter();
        p.rect(rect, 6.0, fill, Stroke::new(1.0, edge), StrokeKind::Inside);
        let box_ = Rect::from_center_size(Pos2::new(rect.left() + pad + glyph / 2.0, rect.center().y), Vec2::splat(glyph));
        mark(p, box_, m, if hot { c.fg } else { c.dim });
        p.galley(Pos2::new(rect.left() + pad + glyph + gap, rect.center().y - galley.size().y / 2.0), galley, c.fg);
    }
    focus_ring(ui, &r);
    r
}

/// The same button with the mark alone, a square `side` points on a side (before the letters' zoom): a fold mark, a close
/// cross. `said` is its tip and what a reader says.
pub fn mark_button(ui: &mut Ui, m: Mark, said: &str, side: f32) -> Response {
    let c = ito_theme::colors();
    let side = z(ui, side);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let r = r.on_hover_text(said).on_hover_cursor(egui::CursorIcon::PointingHand);
    ito_a11y::say(&r, Said::Button, said);
    take_keys(ui, &r);
    if ui.is_rect_visible(rect) {
        let hot = r.hovered() || r.has_focus();
        let (fill, edge, ink) = if hot { (c.hover(), c.border_strong(), c.fg) } else { (c.raised(), c.border, c.dim) };
        let p = ui.painter();
        p.rect(rect, 6.0, fill, Stroke::new(1.0, edge), StrokeKind::Inside);
        mark(p, rect.shrink(side * 0.25), m, ink);
    }
    focus_ring(ui, &r);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(f: impl FnMut(&mut Ui)) -> egui::FullOutput {
        let mut f = f;
        let mut out = egui::Context::default().run_ui(egui::RawInput::default(), |ui| f(ui));
        out.textures_delta.clear();
        out
    }

    #[test]
    fn every_mark_paints_something() {
        for m in Mark::ALL {
            let out = run(|ui| {
                mark(ui.painter(), Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(14.0)), m, Color32::WHITE);
            });
            assert!(!out.shapes.is_empty(), "{m:?} drew nothing");
        }
    }

    #[test]
    fn the_buttons_draw_and_the_words_widen_the_button() {
        let (mut short, mut long, mut square) = (0.0, 0.0, 0.0);
        run(|ui| {
            short = icon_button(ui, Mark::Search, "Go", "Go").rect.width();
            long = icon_button(ui, Mark::Download, "Export everything", "Export").rect.width();
            square = mark_button(ui, Mark::ChevronRight, "Fold", 22.0).rect.width();
        });
        assert!(long > short, "{long} {short}");
        assert!((square - 22.0).abs() < 0.5, "{square}");
    }
}
