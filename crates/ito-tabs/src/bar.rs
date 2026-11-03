//! The tab bar: a strip under the toolbar, one tab per open file, in the
//! style of a document-tab editor. The front tab is joined to the page below
//! with an accent line on top; the others are quieter. A tab that has unsaved
//! changes shows a mark where the cross is, until the pointer is on it.
//!
//! The bar closes nothing and moves nothing: it returns the [`Op`] the person
//! asked for. Every tab has a name for the screen reader (its label), so a
//! `--keys` script can `<Click:label>` it.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use ito_a11y::Role;

use crate::Op;

/// The bar's height in points at the letters' zoom 1.0.
pub const HEIGHT: f32 = 30.0;
const MIN_W: f32 = 96.0;
const MAX_W: f32 = 220.0;
const ARROW_W: f32 = 22.0;
const CROSS_W: f32 = 22.0;
const ACCENT: f32 = 2.0;

/// One tab as the app describes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabView<'a> {
    /// The name on the tab (see [`crate::labels`]).
    pub label: &'a str,
    /// Shown when the pointer rests on the tab: the full path.
    pub tooltip: &'a str,
    /// The file has unsaved changes.
    pub unsaved: bool,
}

/// How wide each tab is: all of `avail` shared out, within the minimum and
/// maximum width. When `n` tabs do not fit at the minimum the strip scrolls.
pub(crate) fn tab_width(avail: f32, n: usize, min: f32, max: f32) -> f32 {
    if n == 0 {
        return max;
    }
    (avail / n as f32).clamp(min, max)
}

/// The scroll offset that shows tab `i` (of width `w`) in a strip `view`
/// wide, moving `offset` as little as it can.
pub(crate) fn reveal(offset: f32, i: usize, w: f32, view: f32) -> f32 {
    let (l, r) = (i as f32 * w, (i as f32 + 1.0) * w);
    if l < offset {
        l
    } else if r > offset + view {
        r - view
    } else {
        offset
    }
}

/// Where a tab dragged to pointer x lands (an index).
pub(crate) fn drop_index(x: f32, left: f32, offset: f32, w: f32, n: usize) -> usize {
    (((x - left + offset) / w).floor().max(0.0) as usize).min(n.saturating_sub(1))
}

#[derive(Clone, Copy, Default)]
struct State {
    offset: f32,
    /// The front tab the offset was last fitted to.
    seen: Option<usize>,
    dragging: Option<usize>,
}

/// Draw the bar across `ui`'s width and return what the person asked for:
/// `Goto(i)` for a click, `CloseAt(i)` for the cross or a middle click,
/// `Move(from, to)` for a drag. `front` is the index of the front tab.
pub fn bar(ui: &mut Ui, tabs: &[TabView<'_>], front: usize) -> Option<Op> {
    let ctx = ui.ctx().clone();
    let c = ito_theme::colors();
    let zoom = ito_theme::text_zoom(&ctx);
    let h = HEIGHT * zoom;
    let (strip, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, c.side);
    painter.hline(strip.x_range(), strip.bottom() - 0.5, Stroke::new(1.0, c.border));
    let n = tabs.len();
    if n == 0 {
        return None;
    }

    let key = ui.id().with("ito-tabs");
    let mut st = ctx.data(|d| d.get_temp::<State>(key)).unwrap_or_default();
    let mut ask = None;

    let min = MIN_W * zoom;
    let fits = tab_width(strip.width(), n, min, MAX_W * zoom) * n as f32 <= strip.width() + 0.5 && strip.width() / n as f32 >= min;
    let arrow = if fits { 0.0 } else { ARROW_W * zoom };
    let view = Rect::from_min_max(Pos2::new(strip.left() + arrow, strip.top()), Pos2::new(strip.right() - arrow, strip.bottom()));
    let w = tab_width(view.width(), n, min, MAX_W * zoom);
    let max_off = (w * n as f32 - view.width()).max(0.0);
    if fits {
        st.offset = 0.0;
    } else {
        if st.seen != Some(front) {
            st.offset = reveal(st.offset, front.min(n - 1), w, view.width());
        }
        if ui.rect_contains_pointer(view) {
            let d = ctx.input(|i| i.smooth_scroll_delta);
            st.offset -= if d.x != 0.0 { d.x } else { d.y };
        }
        st.offset = st.offset.clamp(0.0, max_off);
    }
    st.seen = Some(front);

    let font = FontId::proportional(ito_theme::zoomed(&ctx, 13.0));
    let tab_painter = painter.with_clip_rect(view);
    for (i, t) in tabs.iter().enumerate() {
        let left = view.left() + i as f32 * w - st.offset;
        let rect = Rect::from_min_size(Pos2::new(left, strip.top()), Vec2::new(w, h));
        if rect.right() < view.left() || rect.left() > view.right() {
            continue;
        }
        let vis = rect.intersect(view);
        let id = key.with(i);
        let resp = ui.interact(vis, id, Sense::click_and_drag());
        let cross = Rect::from_min_size(Pos2::new(rect.right() - CROSS_W * zoom, rect.top()), Vec2::new(CROSS_W * zoom, h)).intersect(view);
        let cross_resp = ui.interact(cross, id.with("x"), Sense::click());
        let active = i == front;
        let hot = resp.hovered() || cross_resp.hovered();

        let bg = if active {
            c.bg
        } else if hot {
            c.panel
        } else {
            c.side
        };
        tab_painter.rect_filled(rect.shrink2(Vec2::new(0.0, 0.0)).with_max_y(rect.bottom() - if active { 0.0 } else { 1.0 }), 0.0, bg);
        if active {
            tab_painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(w, ACCENT * zoom.max(1.0))), 0.0, c.blue);
        }
        tab_painter.vline(rect.right() - 0.5, rect.y_range().shrink(6.0), Stroke::new(1.0, c.border));

        // The mark or the cross, right-aligned.
        let mid = Pos2::new(rect.right() - CROSS_W * zoom * 0.5, rect.center().y + ACCENT);
        let show_dot = t.unsaved && !hot;
        let (glyph, col) = if show_dot {
            ("\u{25CF}", c.wait)
        } else if cross_resp.hovered() {
            ("\u{00D7}", c.fg)
        } else if active || hot {
            ("\u{00D7}", c.dim)
        } else {
            ("", c.dim)
        };
        if cross_resp.hovered() {
            tab_painter.circle_filled(mid, 8.0 * zoom, c.border);
        }
        if !glyph.is_empty() {
            tab_painter.text(mid, Align2::CENTER_CENTER, glyph, font.clone(), col);
        }

        // The label, cut with an ellipsis where it does not fit.
        let pad = 10.0 * zoom;
        let room = (w - pad - CROSS_W * zoom).max(8.0);
        let text_col = if active { c.fg } else { c.dim };
        let mut job = egui::text::LayoutJob::simple_singleline(t.label.to_owned(), font.clone(), text_col);
        job.wrap = egui::text::TextWrapping { max_width: room, max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
        let galley = ctx.fonts_mut(|f| f.layout_job(job));
        let pos = Pos2::new(rect.left() + pad, rect.center().y + ACCENT * 0.5 - galley.size().y * 0.5);
        tab_painter.galley(pos, galley, text_col);

        ito_a11y::say(&resp, Role::Row, t.label);
        ito_a11y::say(&cross_resp, Role::Button, "Close tab");

        if cross_resp.clicked() || resp.middle_clicked() {
            ask = Some(Op::CloseAt(i));
        } else if resp.clicked() && !cross_resp.hovered() {
            ask = Some(Op::Goto(i));
        }
        if resp.drag_started() {
            st.dragging = Some(i);
        }
        if resp.drag_stopped() {
            if let (Some(from), Some(p)) = (st.dragging.take(), ctx.pointer_latest_pos()) {
                let to = drop_index(p.x, view.left(), st.offset, w, n);
                if to != from {
                    ask = Some(Op::Move(from, to));
                }
            }
        }
        if !t.tooltip.is_empty() && !resp.dragged() {
            resp.on_hover_text(t.tooltip);
        }
    }

    if arrow > 0.0 {
        let step = w;
        for (right, glyph, name) in [(false, "\u{2039}", "Scroll tabs left"), (true, "\u{203A}", "Scroll tabs right")] {
            let r = if right {
                Rect::from_min_max(Pos2::new(view.right(), strip.top()), strip.max)
            } else {
                Rect::from_min_max(strip.min, Pos2::new(view.left(), strip.bottom()))
            };
            let resp = ui.interact(r, key.with(("arrow", right)), Sense::click());
            let can = if right { st.offset < max_off } else { st.offset > 0.0 };
            if resp.hovered() && can {
                painter.rect_filled(r, 0.0, c.panel);
            }
            let col: Color32 = if can { c.fg } else { c.border };
            painter.text(r.center(), Align2::CENTER_CENTER, glyph, font.clone(), col);
            ito_a11y::say(&resp, Role::Button, name);
            if resp.clicked() {
                st.offset = (st.offset + if right { step } else { -step }).clamp(0.0, max_off);
            }
        }
    }

    ctx.data_mut(|d| d.insert_temp(key, st));
    ask
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput};

    const W: f32 = 600.0;

    #[test]
    fn tabs_share_the_strip_within_their_limits() {
        assert_eq!(tab_width(600.0, 3, 96.0, 220.0), 200.0);
        assert_eq!(tab_width(600.0, 2, 96.0, 220.0), 220.0);
        assert_eq!(tab_width(600.0, 12, 96.0, 220.0), 96.0);
        assert_eq!(tab_width(0.0, 0, 96.0, 220.0), 220.0);
    }

    #[test]
    fn reveal_moves_the_strip_as_little_as_it_can() {
        assert_eq!(reveal(0.0, 1, 100.0, 300.0), 0.0);
        assert_eq!(reveal(0.0, 5, 100.0, 300.0), 300.0);
        assert_eq!(reveal(500.0, 2, 100.0, 300.0), 200.0);
    }

    #[test]
    fn a_drop_lands_on_the_tab_under_the_pointer() {
        assert_eq!(drop_index(10.0, 0.0, 0.0, 100.0, 4), 0);
        assert_eq!(drop_index(250.0, 0.0, 0.0, 100.0, 4), 2);
        assert_eq!(drop_index(950.0, 0.0, 0.0, 100.0, 4), 3);
        assert_eq!(drop_index(50.0, 0.0, 200.0, 100.0, 4), 2);
        assert_eq!(drop_index(-30.0, 0.0, 0.0, 100.0, 4), 0);
    }

    /// Runs frames of the bar; `events` for each frame after a first one that lays it out.
    struct Harness {
        ctx: egui::Context,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            Self { ctx: egui::Context::default(), time: 0.0 }
        }

        fn frame(&mut self, views: &[TabView<'_>], front: usize, events: Vec<Event>) -> Option<Op> {
            self.time += 0.1;
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(W, HEIGHT));
            let input = RawInput { screen_rect: Some(screen), time: Some(self.time), events, ..Default::default() };
            let mut out = None;
            let mut full = self.ctx.run_ui(input, |ui| out = bar(ui, views, front));
            full.textures_delta.clear();
            out
        }

        fn click(&mut self, views: &[TabView<'_>], front: usize, at: Pos2, button: PointerButton) -> Option<Op> {
            self.frame(views, front, vec![]);
            self.frame(views, front, vec![Event::PointerMoved(at)]);
            let mods = egui::Modifiers::NONE;
            let down = Event::PointerButton { pos: at, button, pressed: true, modifiers: mods };
            let up = Event::PointerButton { pos: at, button, pressed: false, modifiers: mods };
            let a = self.frame(views, front, vec![down]);
            let b = self.frame(views, front, vec![up]);
            a.or(b)
        }
    }

    fn views() -> Vec<TabView<'static>> {
        vec![
            TabView { label: "one.md", tooltip: "C:/d/one.md", unsaved: false },
            TabView { label: "two.md", tooltip: "C:/d/two.md", unsaved: true },
            TabView { label: "three.md", tooltip: "C:/d/three.md", unsaved: false },
        ]
    }

    // Three tabs in 600 points are 200 wide each.
    #[test]
    fn a_click_on_a_tab_asks_to_go_to_it() {
        let mut h = Harness::new();
        let r = h.click(&views(), 0, Pos2::new(250.0, 15.0), PointerButton::Primary);
        assert_eq!(r, Some(Op::Goto(1)));
    }

    #[test]
    fn the_cross_asks_to_close_that_tab() {
        let mut h = Harness::new();
        let r = h.click(&views(), 0, Pos2::new(390.0, 15.0), PointerButton::Primary);
        assert_eq!(r, Some(Op::CloseAt(1)));
    }

    #[test]
    fn a_middle_click_asks_to_close() {
        let mut h = Harness::new();
        let r = h.click(&views(), 0, Pos2::new(500.0, 15.0), PointerButton::Middle);
        assert_eq!(r, Some(Op::CloseAt(2)));
    }

    #[test]
    fn nothing_is_asked_without_a_click() {
        let mut h = Harness::new();
        assert_eq!(h.frame(&views(), 0, vec![]), None);
        assert_eq!(h.frame(&[], 0, vec![]), None);
    }

    #[test]
    fn every_tab_has_a_name_for_the_screen_reader() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(W, HEIGHT));
        let v = views();
        let mut out = ctx.run_ui(RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            bar(ui, &v, 0);
        });
        out.textures_delta.clear();
        let update = out.platform_output.accesskit_update.expect("accesskit update");
        let names: Vec<String> = update.nodes.iter().filter_map(|(_, n)| n.label().map(str::to_owned)).collect();
        for t in &v {
            assert!(names.iter().any(|n| n == t.label), "{} not in {names:?}", t.label);
        }
    }

    #[test]
    fn many_tabs_scroll_instead_of_shrinking_below_the_minimum() {
        let labels: Vec<String> = (0..12).map(|i| format!("f{i}.md")).collect();
        let v: Vec<TabView> = labels.iter().map(|l| TabView { label: l, tooltip: "", unsaved: false }).collect();
        let mut h = Harness::new();
        h.frame(&v, 0, vec![]);
        // Bringing the last tab forward scrolls it into view: its tab can then be clicked at the right end.
        h.frame(&v, 11, vec![]);
        let r = h.click(&v, 11, Pos2::new(W - ARROW_W - 40.0, 15.0), PointerButton::Primary);
        assert_eq!(r, Some(Op::Goto(11)));
    }
}
