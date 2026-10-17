//! The dividers (and column grips) in egui (the `egui` feature): only a gap until the pointer
//! comes near, then a line to grab; dragged, the split follows; double-
//! clicked, it halves. The layout is the caller's: what changes comes back
//! as [`Moved`], so tsumugi sends it to its server and filer keeps it.

use crate::{Dir, Node, Rect};

/// How the dividers look and how far from one it can be taken.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// The gap between two panes.
    pub gap: f32,
    /// How wide a strip takes the pointer, the gap in the middle.
    pub grab: f32,
    /// The line drawn in the gap while it is under the pointer.
    pub line: egui::Color32,
}

/// What the pointer did to a divider this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Moved<T> {
    /// Being dragged: the layout as it would be dropped here. Draw with it;
    /// keep it for [`Moved::Released`].
    Dragging(Node<T>),
    /// The drag ended: the last [`Moved::Dragging`] is the one to keep.
    Released,
    /// Double-clicked: this split halved, to keep at once.
    Halved(Node<T>),
}

pub fn to_egui(r: Rect) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(r.x, r.y), egui::vec2(r.w, r.h))
}

pub fn from_egui(r: egui::Rect) -> Rect {
    Rect::new(r.min.x, r.min.y, r.width(), r.height())
}

/// The dividers of `layout` laid over `area`, `id` telling this layout's
/// apart from another's.
pub fn dividers<T: Clone + PartialEq>(ui: &egui::Ui, id: egui::Id, layout: &Node<T>, area: Rect, look: Look) -> Option<Moved<T>> {
    let mut out = None;
    for d in layout.dividers(area, look.gap) {
        let gap = to_egui(d.gap);
        let more = (look.grab - look.gap) / 2.0;
        let grab = match d.dir {
            Dir::Right => gap.expand2(egui::vec2(more, 0.0)),
            Dir::Down => gap.expand2(egui::vec2(0.0, more)),
        };
        let resp = ui.interact(grab, id.with(("divider", &d.path)), egui::Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(match d.dir {
                Dir::Right => egui::CursorIcon::ResizeHorizontal,
                Dir::Down => egui::CursorIcon::ResizeVertical,
            });
            let thin = match d.dir {
                Dir::Right => egui::vec2(look.gap / 2.0 - 1.0, 0.0),
                Dir::Down => egui::vec2(0.0, look.gap / 2.0 - 1.0),
            };
            ui.painter().rect_filled(gap.shrink2(thin), 1.0, look.line);
        }
        if resp.double_clicked() {
            let mut l = layout.clone();
            l.set_ratio(&d.path, 0.5);
            out = Some(Moved::Halved(l));
        } else if let (true, Some(p)) = (resp.dragged(), resp.interact_pointer_pos()) {
            // On the same base the split is laid out on (its room less the
            // gap), so the divider stays under the pointer.
            let ratio = match d.dir {
                Dir::Right => (p.x - d.area.x - look.gap / 2.0) / (d.area.w - look.gap).max(1.0),
                Dir::Down => (p.y - d.area.y - look.gap / 2.0) / (d.area.h - look.gap).max(1.0),
            };
            let mut l = layout.clone();
            l.set_ratio(&d.path, ratio);
            out = Some(Moved::Dragging(l));
        } else if resp.drag_stopped() {
            out = Some(Moved::Released);
        }
    }
    out
}

/// What a column grip did this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grip {
    /// The edge, as its place in the `edges` given to [`grips`].
    pub index: usize,
    /// How far the pointer moved along x this frame; give it to
    /// [`columns::resize`](crate::columns::resize) or `trade`.
    pub delta: f32,
    /// The drag ended: save the widths now.
    pub released: bool,
    /// Double-clicked: put the column back to its default width.
    pub reset: bool,
}

/// Grips on a header's column edges: a strip `grab` wide at each x in `edges`,
/// over `y`, that takes the pointer and shows the resize cursor. One edge is
/// answered at a time (the one being dragged, else the first that has
/// something to say); `id` tells this header's from another's.
pub fn grips(ui: &egui::Ui, id: egui::Id, edges: &[f32], y: std::ops::Range<f32>, grab: f32) -> Option<Grip> {
    let mut out = None;
    for (index, &x) in edges.iter().enumerate() {
        let strip = egui::Rect::from_min_max(egui::pos2(x - grab / 2.0, y.start), egui::pos2(x + grab / 2.0, y.end));
        let resp = ui.interact(strip, id.with(("grip", index)), egui::Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let grip = if resp.double_clicked() {
            Grip { index, delta: 0.0, released: false, reset: true }
        } else if resp.dragged() {
            Grip { index, delta: resp.drag_delta().x, released: false, reset: false }
        } else if resp.drag_stopped() {
            Grip { index, delta: 0.0, released: true, reset: false }
        } else {
            continue;
        };
        if resp.dragged() || out.is_none() {
            out = Some(grip);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drag across the divider of two panes side by side moves it; a
    /// release says so; nothing is said with the pointer elsewhere.
    #[test]
    fn a_divider_follows_the_pointer() {
        let ctx = egui::Context::default();
        let layout = Node::Split { dir: Dir::Right, ratio: 0.5, first: Box::new(Node::Leaf(1)), second: Box::new(Node::Leaf(2)) };
        let area = Rect::new(0.0, 0.0, 400.0, 200.0);
        let look = Look { gap: 8.0, grab: 12.0, line: egui::Color32::WHITE };
        let mut said = Vec::new();
        let mut held_at_release = None;
        let mut frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                if let Some(m) = dividers(ui, egui::Id::new("t"), &layout, area, look) {
                    if m == Moved::Released {
                        held_at_release = Some(ui.input(|i| i.pointer.primary_down()));
                    }
                    said.push(m);
                }
            });
            out.textures_delta.clear();
        };
        let at = |x: f32| egui::pos2(x, 100.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(at(10.0))]);
        frame(vec![egui::Event::PointerMoved(at(200.0))]);
        frame(vec![button(at(200.0), true)]);
        for x in [210.0, 260.0, 300.0] {
            frame(vec![egui::Event::PointerMoved(at(x))]);
        }
        frame(vec![button(at(300.0), false)]);
        frame(vec![]);
        let last = said.iter().rev().find_map(|m| match m {
            Moved::Dragging(Node::Split { ratio, .. }) => Some(*ratio),
            _ => None,
        });
        assert!(last.is_some_and(|r| (r - 0.75).abs() < 0.02), "{said:?}");
        assert!(said.contains(&Moved::Released), "{said:?}");
        // The caller must not drop the dragged layout for a button that is
        // up: on the frame of the release it already is (tsumugi, 2.3).
        assert_eq!(held_at_release, Some(false));
    }

    /// A drag on a column edge says how far it moved, the release says so,
    /// and a double-click asks for the default.
    #[test]
    fn a_grip_reports_the_drag() {
        let ctx = egui::Context::default();
        let mut said = Vec::new();
        let mut frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                said.extend(grips(ui, egui::Id::new("g"), &[100.0, 200.0], 0.0..30.0, 8.0));
            });
            out.textures_delta.clear();
        };
        let at = |x: f32| egui::pos2(x, 10.0);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(at(50.0))]);
        frame(vec![egui::Event::PointerMoved(at(200.0))]);
        frame(vec![button(at(200.0), true)]);
        for x in [210.0, 230.0] {
            frame(vec![egui::Event::PointerMoved(at(x))]);
        }
        frame(vec![button(at(230.0), false)]);
        frame(vec![]);
        assert!(said.iter().all(|g| g.index == 1), "{said:?}");
        let moved: f32 = said.iter().map(|g| g.delta).sum();
        assert!((moved - 30.0).abs() < 0.01, "{said:?}");
        assert!(said.iter().any(|g| g.released), "{said:?}");
    }
}
