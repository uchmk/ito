//! The window's own title bar, the same in every app that draws one.
//!
//! [`show`] fills the rect it is given (the app's top panel): the empty
//! parts move the window and a double click maximizes it, Windows-style
//! minimize / maximize / close sit at the right, the command-palette box
//! stays in the middle of the window with the notification bell just
//! right of it, and two slots are the app's own: the left (logo, name,
//! tabs) and the right (tags, chips, an avatar). What the box says, the
//! key it shows, the bell's count and every hover text are passed in: this
//! crate knows no app, no key map and no language. Colours are
//! `ito_theme::colors()`: the box is `panel` (`hover` under the pointer)
//! with a `border`, the badge `wait` (`err` while a notice is an error).
//!
//! The app sends what [`Out::window`] asks with [`apply`], and calls
//! [`resize_edges`] last in the frame when the window has no frame.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// The bar's usual height.
pub const HEIGHT: f32 = 40.0;

/// A caption button's width, as Windows 11 has them.
pub const CAPTION_W: f32 = 46.0;

/// The palette box: its widest, its height, its corner.
const BOX_MAX_W: f32 = 420.0;
const BOX_H: f32 = 26.0;
const BOX_RADIUS: f32 = 6.0;
/// The bell's size, and what the bell and its gap take beside the box.
const BELL_W: f32 = 28.0;
const BELL_H: f32 = 24.0;
const GAP: f32 = 6.0;
const BELL_SPAN: f32 = 34.0;
/// Narrower than this the box is the magnifier alone.
const COMPACT_BELOW: f32 = 160.0;
/// The room kept from the app's slots, and from the window's edge.
const MARGIN: f32 = 12.0;

/// What a click on the bar asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowOp {
    Drag,
    ToggleMax,
    Minimize,
    Close,
}

/// Every text the bar shows or hovers, in the app's language.
#[derive(Clone, Copy, Debug)]
pub struct Words<'a> {
    /// In the box ("Command palette"), and as its hover.
    pub search: &'a str,
    /// The key the box shows at its right (`Ctrl+Shift+P`, as the app's key
    /// map spells it).
    pub search_key: &'a str,
    /// The bell's hover ("Notices (3 unread)").
    pub bell: &'a str,
    pub minimize: &'a str,
    pub maximize: &'a str,
    pub restore: &'a str,
    pub close: &'a str,
}

/// How the bar is to be drawn this frame.
#[derive(Clone, Copy, Debug)]
pub struct Spec<'a> {
    /// The window has no system title bar: the bar drags it and has the
    /// caption buttons (off the Mac, whose traffic lights stay).
    pub own: bool,
    /// Room kept on the left (the Mac's traffic lights over the bar).
    pub left: f32,
    pub maximized: bool,
    /// Unread notices, and whether one of them is an error (the badge is
    /// red then, else the theme's `wait`).
    pub unread: usize,
    pub any_error: bool,
    /// False: no box and no bell, and the right slot gets everything the
    /// left leaves (a settings screen that puts its own title there).
    pub centre: bool,
    /// Width the right slot wants kept (0.0: none), as the slot's own width
    /// (the margins come on top). The box shrinks first,
    /// down to the magnifier, so the slot has this much on the right of the
    /// bell; if that is still too little, the slot folds itself.
    pub right_min: f32,
    pub words: Words<'a>,
}

/// What happened on the bar.
#[derive(Clone, Copy, Debug, Default)]
pub struct Out {
    pub window: Option<WindowOp>,
    /// The box was clicked: open the palette.
    pub search: bool,
    /// The bell was clicked.
    pub bell: bool,
    /// Where the bell is: a list under it ends at its right edge.
    pub bell_rect: Option<Rect>,
}

/// The bar in `ui.max_rect()`. `left` draws in the rect that is the bar
/// less the caption buttons and the Mac's room, and returns where it ended
/// (x); `right` gets what is left on the right of the bell (or of `left`,
/// when there is no centre), with the margin kept, and may draw in it.
pub fn show(ui: &mut Ui, spec: &Spec<'_>, left: impl FnOnce(&mut Ui, Rect) -> f32, right: impl FnOnce(&mut Ui, Rect)) -> Out {
    let c = ito_theme::colors();
    let whole = ui.max_rect();
    let p = ui.painter().clone();
    p.line_segment([whole.left_bottom(), whole.right_bottom()], Stroke::new(1.0, c.border));
    let mut out = Out::default();
    // As the title bar: the bar's empty parts move the window. Taken first,
    // so what is drawn on it comes first and wins.
    if spec.own {
        let bg = ui.interact(whole, ui.id().with("titlebar-drag"), Sense::CLICK | Sense::DRAG);
        if bg.double_clicked() {
            out.window = Some(WindowOp::ToggleMax);
        } else if bg.drag_started() {
            out.window = Some(WindowOp::Drag);
        }
    }
    let buttons = if spec.own && !cfg!(target_os = "macos") { 3.0 * CAPTION_W } else { 0.0 };
    if buttons > 0.0 {
        let at = Rect::from_min_max(Pos2::new(whole.right() - buttons, whole.top()), whole.right_bottom());
        if let Some(op) = caption_buttons(ui, at, spec) {
            out.window = Some(op);
        }
    }
    let mut rect = whole;
    rect.min.x += spec.left;
    rect.max.x -= buttons;
    let left_end = left(ui, rect);
    if !spec.centre {
        let free = Rect::from_min_max(Pos2::new(left_end + MARGIN, rect.top()), Pos2::new(rect.right() - MARGIN, rect.bottom()));
        right(ui, free);
        return out;
    }
    // The box and the bell beside it, in the middle of the window whatever
    // the app puts at the right: as wide as there is room for on both
    // sides of the middle, then only the magnifier.
    let middle = whole.center().x;
    let keep = if spec.right_min > 0.0 { spec.right_min + MARGIN } else { 0.0 };
    let half = (middle - left_end - MARGIN).min(rect.right() - MARGIN - keep - middle);
    let box_w = BOX_MAX_W.min(2.0 * half - BELL_SPAN);
    let compact = box_w < COMPACT_BELOW;
    let w = if compact { 30.0 } else { box_w };
    let group_left = (middle - (w + BELL_SPAN) / 2.0).max(left_end + MARGIN);
    let r = Rect::from_min_size(Pos2::new(group_left, rect.center().y - BOX_H / 2.0), Vec2::new(w, BOX_H));
    let bell_rect = Rect::from_min_size(Pos2::new(r.right() + GAP, rect.center().y - BELL_H / 2.0), Vec2::new(BELL_W, BELL_H));
    let free = Rect::from_min_max(Pos2::new(r.right() + BELL_SPAN + MARGIN, rect.top()), Pos2::new(rect.right() - MARGIN, rect.bottom()));
    right(ui, free);
    out.search = search_box(ui, r, compact, &spec.words);
    out.bell = bell(ui, bell_rect, spec).clicked();
    out.bell_rect = Some(bell_rect);
    out
}

/// What a [`WindowOp`] is for the window: send it to the viewport.
/// `maximized` is the window's state now.
pub fn apply(ctx: &egui::Context, op: WindowOp, maximized: bool) {
    use egui::ViewportCommand as V;
    ctx.send_viewport_cmd(match op {
        WindowOp::Drag => V::StartDrag,
        WindowOp::ToggleMax => V::Maximized(!maximized),
        WindowOp::Minimize => V::Minimized(true),
        WindowOp::Close => V::Close,
    });
}

/// The viewport for an app that draws its own bar: no system frame, but on
/// the Mac the traffic lights stay, over the bar.
pub fn frameless(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if cfg!(target_os = "macos") {
        viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false)
    } else {
        viewport.with_decorations(false)
    }
}

fn search_box(ui: &mut Ui, r: Rect, compact: bool, words: &Words<'_>) -> bool {
    let c = ito_theme::colors();
    let p = ui.painter().clone();
    let resp = ui.interact(r, ui.id().with("titlebar-search"), Sense::CLICK).on_hover_text(words.search);
    p.rect_filled(r, BOX_RADIUS, if resp.hovered() { c.hover() } else { c.panel });
    p.rect_stroke(r, BOX_RADIUS, Stroke::new(1.0, c.border), egui::StrokeKind::Inside);
    let glass = Pos2::new(r.left() + 15.0, r.center().y - 1.0);
    let stroke = Stroke::new(1.3, c.dim);
    p.circle_stroke(glass, 4.0, stroke);
    p.line_segment([glass + Vec2::new(3.0, 3.0), glass + Vec2::new(6.0, 6.0)], stroke);
    if !compact {
        let key = p.layout_no_wrap(words.search_key.to_owned(), FontId::monospace(11.0), c.faint());
        let key_x = r.right() - 10.0 - key.size().x;
        let mut job = egui::text::LayoutJob::simple_singleline(words.search.to_owned(), FontId::proportional(12.0), c.dim);
        job.wrap = egui::text::TextWrapping::truncate_at_width((key_x - r.left() - 40.0).max(0.0));
        let text = ui.fonts_mut(|f| f.layout_job(job));
        if key_x - r.left() > 140.0 {
            p.galley(Pos2::new(r.left() + 28.0, r.center().y - 7.0), text, c.dim);
        }
        p.galley(Pos2::new(key_x, r.center().y - key.size().y / 2.0), key, c.faint());
    }
    resp.clicked()
}

/// The bell in `rect`, with the unread count on it.
fn bell(ui: &mut Ui, rect: Rect, spec: &Spec<'_>) -> egui::Response {
    let c = ito_theme::colors();
    let resp = ui.interact(rect, ui.id().with("titlebar-bell"), Sense::CLICK).on_hover_text(spec.words.bell);
    let p = ui.painter_at(rect.expand(6.0));
    if resp.hovered() {
        p.rect_filled(rect, 6.0, c.selection().gamma_multiply(0.5));
    }
    // A bell, drawn: the cup, its lip and the clapper.
    let at = rect.center() + Vec2::new(0.0, -1.0);
    let stroke = Stroke::new(1.4, if spec.unread == 0 { c.dim } else { c.fg });
    let cup = [(-5.5, 4.0), (-4.5, -1.0), (-3.0, -4.5), (0.0, -5.5), (3.0, -4.5), (4.5, -1.0), (5.5, 4.0)];
    p.add(egui::Shape::line(cup.iter().map(|&(x, y)| at + Vec2::new(x, y)).collect(), stroke));
    p.line_segment([at + Vec2::new(-6.5, 4.0), at + Vec2::new(6.5, 4.0)], stroke);
    p.circle_filled(at + Vec2::new(0.0, 6.2), 1.4, stroke.color);
    if spec.unread > 0 {
        let color = if spec.any_error { c.err } else { c.wait };
        let at = rect.right_top() + Vec2::new(-5.0, 5.0);
        p.circle_filled(at, 7.0, color);
        p.text(at, Align2::CENTER_CENTER, badge(spec.unread), FontId::proportional(10.0), c.on_accent());
    }
    resp
}

/// The count on the badge: 1 to 9, then `9+`.
pub fn badge(unread: usize) -> String {
    if unread > 9 {
        "9+".to_owned()
    } else {
        unread.to_string()
    }
}

/// Minimize, maximize (restore when maximized) and close, in `rect`, drawn
/// as Windows draws them: thin lines, close red under the pointer.
fn caption_buttons(ui: &mut Ui, rect: Rect, spec: &Spec<'_>) -> Option<WindowOp> {
    let c = ito_theme::colors();
    let mut out = None;
    let w = &spec.words;
    let ops = [
        (WindowOp::Minimize, w.minimize),
        (WindowOp::ToggleMax, if spec.maximized { w.restore } else { w.maximize }),
        (WindowOp::Close, w.close),
    ];
    for (k, (op, words)) in ops.into_iter().enumerate() {
        let r = Rect::from_min_size(rect.min + Vec2::new(k as f32 * CAPTION_W, 0.0), Vec2::new(CAPTION_W, rect.height()));
        let resp = ui.interact(r, ui.id().with(("titlebar-caption", k)), Sense::CLICK).on_hover_text(words);
        let p = ui.painter();
        let hot = resp.hovered();
        let close = op == WindowOp::Close;
        if hot {
            p.rect_filled(r, 0.0, if close { Color32::from_rgb(0xc4, 0x2b, 0x1c) } else { c.hover() });
        }
        let stroke = Stroke::new(1.0, if hot && close { Color32::WHITE } else { c.fg });
        let m = r.center();
        match op {
            WindowOp::Minimize => {
                p.line_segment([m + Vec2::new(-5.0, 0.0), m + Vec2::new(5.0, 0.0)], stroke);
            }
            WindowOp::ToggleMax if spec.maximized => {
                // Two windows, one behind the other.
                p.rect_stroke(Rect::from_center_size(m + Vec2::new(-1.0, 1.0), Vec2::splat(8.0)), 1.0, stroke, egui::StrokeKind::Inside);
                p.line_segment([m + Vec2::new(-3.0, -5.0), m + Vec2::new(5.0, -5.0)], stroke);
                p.line_segment([m + Vec2::new(5.0, -5.0), m + Vec2::new(5.0, 3.0)], stroke);
            }
            WindowOp::ToggleMax => {
                p.rect_stroke(Rect::from_center_size(m, Vec2::splat(10.0)), 1.0, stroke, egui::StrokeKind::Inside);
            }
            WindowOp::Close | WindowOp::Drag => {
                p.line_segment([m + Vec2::new(-5.0, -5.0), m + Vec2::new(5.0, 5.0)], stroke);
                p.line_segment([m + Vec2::new(-5.0, 5.0), m + Vec2::new(5.0, -5.0)], stroke);
            }
        }
        if resp.clicked() {
            out = Some(op);
        }
    }
    out
}

/// A window without the system's frame is resized by its edges: a 6px
/// strip along each, and the corners, set the pointer and start the
/// system's resizing on a press (on the press, not once a drag is seen: by
/// then the pointer has left the window, outward). Called last in a frame,
/// so its pointer is the one shown. Not while maximized.
pub fn resize_edges(ctx: &egui::Context) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let r = ctx.content_rect();
    let (pos, pressed) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_pressed()));
    let Some(p) = pos else { return };
    let Some(dir) = edge_at(r, p) else { return };
    ctx.set_cursor_icon(match dir {
        D::North | D::South => C::ResizeVertical,
        D::East | D::West => C::ResizeHorizontal,
        D::NorthWest | D::SouthEast => C::ResizeNwSe,
        D::NorthEast | D::SouthWest => C::ResizeNeSw,
    });
    if pressed {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
    }
}

/// Which edge or corner of `r` the point `p` is on, if any. The top edge is
/// thinner: the bar under it is the title bar to drag by.
pub fn edge_at(r: Rect, p: Pos2) -> Option<egui::ResizeDirection> {
    use egui::ResizeDirection as D;
    const EDGE: f32 = 6.0;
    const CORNER: f32 = 12.0;
    if !r.contains(p) {
        return None;
    }
    let (left, right) = (p.x - r.left() < EDGE, r.right() - p.x < EDGE);
    let (top, bottom) = (p.y - r.top() < EDGE / 2.0, r.bottom() - p.y < EDGE);
    let (near_left, near_right) = (p.x - r.left() < CORNER, r.right() - p.x < CORNER);
    let (near_top, near_bottom) = (p.y - r.top() < CORNER, r.bottom() - p.y < CORNER);
    let dir = match () {
        _ if (top && near_left) || (left && near_top) => D::NorthWest,
        _ if (top && near_right) || (right && near_top) => D::NorthEast,
        _ if (bottom && near_left) || (left && near_bottom) => D::SouthWest,
        _ if (bottom && near_right) || (right && near_bottom) => D::SouthEast,
        _ if top => D::North,
        _ if bottom => D::South,
        _ if left => D::West,
        _ if right => D::East,
        _ => return None,
    };
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> Words<'static> {
        Words {
            search: "Command palette",
            search_key: "Ctrl+Shift+P",
            bell: "Notices",
            minimize: "m",
            maximize: "M",
            restore: "r",
            close: "c",
        }
    }

    fn spec(unread: usize, centre: bool) -> Spec<'static> {
        Spec { own: true, left: 0.0, maximized: false, unread, any_error: false, centre, right_min: 0.0, words: words() }
    }

    /// One frame of a bar `width` wide; the rects the slots were given.
    fn run(width: f32, spec: &Spec<'_>, left_end: f32) -> (Out, Option<Rect>, Option<Rect>) {
        let ctx = egui::Context::default();
        let (mut out, mut l, mut r) = (Out::default(), None, None);
        let mut full = ctx.run_ui(
            egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, HEIGHT))), ..Default::default() },
            |ui| {
                out = show(
                    ui,
                    spec,
                    |_, rect| {
                        l = Some(rect);
                        left_end
                    },
                    |_, rect| r = Some(rect),
                );
            },
        );
        full.textures_delta.clear();
        (out, l, r)
    }

    #[test]
    fn the_bell_is_right_of_the_box_which_is_in_the_middle_of_the_window() {
        let (out, _, right) = run(1600.0, &spec(2, true), 120.0);
        let bell = out.bell_rect.expect("a bell");
        assert_eq!(bell.size(), Vec2::new(BELL_W, BELL_H));
        // The box and the bell's span are centred on the window.
        let box_left = bell.left() - GAP - BOX_MAX_W;
        let span_right = bell.left() - GAP + BELL_SPAN;
        assert!(((box_left + span_right) / 2.0 - 800.0).abs() < 1.0, "box_left {box_left}");
        let right = right.expect("the right slot");
        assert!(right.left() > bell.right() && right.right() < 1600.0 - 3.0 * CAPTION_W);
    }

    #[test]
    fn the_box_shrinks_first_to_leave_the_right_slot_its_minimum() {
        let right_of = |min: f32, width: f32| {
            let mut s = spec(0, true);
            s.right_min = min;
            let (out, _, right) = run(width, &s, 120.0);
            (out.bell_rect.unwrap(), right.unwrap())
        };
        // Wide bar, nothing asked: the box is at its widest.
        let (bell0, slot0) = right_of(0.0, 1700.0);
        assert!(slot0.width() > 0.0);
        // The same bar with 500 asked for: the slot has at least that, and the box got narrower.
        let (bell, slot) = right_of(500.0, 1700.0);
        assert!(slot.width() >= 500.0 - 0.5, "slot {}", slot.width());
        assert!(bell.left() < bell0.left(), "the box did not shrink");
        // Too little room for the minimum: the box is the magnifier, and the slot gets what is left.
        let (bell, slot) = right_of(500.0, 720.0);
        assert!(bell.left() - GAP - 30.0 >= 120.0 + MARGIN - 0.5);
        assert!(slot.right() <= 720.0 - 3.0 * CAPTION_W - MARGIN + 0.5);
        let _ = bell;
    }

    #[test]
    fn a_narrow_bar_keeps_the_magnifier_and_the_bell_clear_of_the_left() {
        let (out, _, _) = run(420.0, &spec(0, true), 150.0);
        let bell = out.bell_rect.unwrap();
        assert!(bell.left() - GAP - 30.0 >= 150.0 + MARGIN - 0.5, "bell at {}", bell.left());
    }

    #[test]
    fn without_a_centre_the_right_slot_has_all_that_the_left_leaves() {
        let (out, _, right) = run(1000.0, &spec(0, false), 200.0);
        assert!(out.bell_rect.is_none());
        let right = right.unwrap();
        assert_eq!(right.left(), 200.0 + MARGIN);
        assert_eq!(right.right(), 1000.0 - 3.0 * CAPTION_W - MARGIN);
    }

    #[test]
    fn the_left_slot_is_the_bar_less_the_caption_buttons_and_the_rooms() {
        let mut s = spec(0, true);
        s.left = 76.0;
        let (_, left, _) = run(900.0, &s, 100.0);
        let left = left.unwrap();
        assert_eq!(left.left(), 76.0);
        assert_eq!(left.right(), 900.0 - 3.0 * CAPTION_W);
        s.own = false;
        let (_, left, _) = run(900.0, &s, 100.0);
        assert_eq!(left.unwrap().right(), 900.0);
    }

    #[test]
    fn the_badge_counts_to_nine_and_then_says_so() {
        assert_eq!(badge(1), "1");
        assert_eq!(badge(9), "9");
        assert_eq!(badge(10), "9+");
    }

    #[test]
    fn the_edges_resize_and_the_top_one_is_thin() {
        use egui::ResizeDirection as D;
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        assert_eq!(edge_at(r, Pos2::new(400.0, 300.0)), None);
        assert_eq!(edge_at(r, Pos2::new(2.0, 300.0)), Some(D::West));
        assert_eq!(edge_at(r, Pos2::new(799.0, 599.0)), Some(D::SouthEast));
        assert_eq!(edge_at(r, Pos2::new(400.0, 2.0)), Some(D::North));
        assert_eq!(edge_at(r, Pos2::new(400.0, 4.0)), None);
        assert_eq!(edge_at(r, Pos2::new(900.0, 300.0)), None);
    }
}
