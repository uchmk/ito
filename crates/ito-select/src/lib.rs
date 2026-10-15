//! Text picked up with the mouse and copied, in a pane that is painted
//! rather than made of widgets (a preview, a detail column). egui can select
//! the text of a `Label`, but not a galley handed to a `Painter`; this keeps
//! the texts a frame painted, in the order they were painted, and turns a drag
//! over them into a selection between two places in that order: a text's
//! number and a character in it. Copying takes what lies between, a line for
//! each text.
//!
//! One frame of use:
//!
//! 1. [`Select::begin`] with what the pane shows (another item drops the
//!    selection);
//! 2. [`Select::note`] each galley as it is painted, top to bottom;
//! 3. [`Select::drive`] with the pane's `Response` (made with
//!    `Sense::click_and_drag()`), then [`Select::paint`] over the texts;
//! 4. [`Select::copy`] to answer egui's `Copy` event (Ctrl+C or Cmd+C), or
//!    [`Select::text`] to take the selection yourself.

use std::sync::Arc;

use egui::epaint::text::cursor::CCursor;
use egui::{Color32, Context, Event, Galley, Painter, PointerButton, Pos2, Rect, Response};

/// A place in the painted texts: which one, and the character in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct At {
    pub run: usize,
    pub ch: usize,
}

struct Run {
    pos: Pos2,
    galley: Arc<Galley>,
    /// Painted on the same line as the text before it (a span of a line).
    inline: bool,
}

impl Run {
    fn rect(&self) -> Rect {
        Rect::from_min_size(self.pos, self.galley.size())
    }
}

#[derive(Default)]
pub struct Select {
    /// The texts of the frame being painted, and of the one before it (the
    /// ones the user saw, and the ones a drag lands on).
    runs: Vec<Run>,
    last: Vec<Run>,
    /// Where the drag began and where it is.
    sel: Option<(At, At)>,
    /// What the selection belongs to: another owner drops it.
    owner: String,
}

impl Select {
    /// A new frame of the pane begins; `owner` is what it shows.
    pub fn begin(&mut self, owner: &str) {
        self.last = std::mem::take(&mut self.runs);
        if self.owner != owner {
            self.owner = owner.to_string();
            self.sel = None;
        }
    }

    /// A text painted with its top left at `pos`. Note them in reading order.
    pub fn note(&mut self, pos: Pos2, galley: &Arc<Galley>) {
        self.runs.push(Run { pos, galley: galley.clone(), inline: false });
    }

    /// A text painted on the same line as the one noted before it (the spans
    /// of a line, each placed by itself): a copy joins them, with a space where
    /// they do not touch, instead of breaking the line between them.
    pub fn note_inline(&mut self, pos: Pos2, galley: &Arc<Galley>) {
        self.runs.push(Run { pos, galley: galley.clone(), inline: true });
    }

    /// The place at `at` among the texts of the last frame that show in
    /// `view`: inside one, or the nearest (so a drag past the end of a line or
    /// below the last still lands).
    pub fn place(&self, at: Pos2, view: Rect) -> Option<At> {
        let near = |r: &Run| {
            let rect = r.rect();
            let dx = (rect.left() - at.x).max(at.x - rect.right()).max(0.0);
            let dy = (rect.top() - at.y).max(at.y - rect.bottom()).max(0.0);
            dx * dx + dy * dy * 4.0
        };
        let (run, r) =
            self.last.iter().enumerate().filter(|(_, r)| r.rect().intersects(view)).min_by(|a, b| near(a.1).total_cmp(&near(b.1)))?;
        let c = r.galley.cursor_from_pos((at - r.pos).to_owned());
        Some(At { run, ch: c.index.0 })
    }

    /// Follow the mouse on the pane: a press and drag picks text up, a click
    /// drops the selection. `view` is the part of the pane that shows texts.
    pub fn drive(&mut self, resp: &Response, view: Rect) {
        if resp.drag_started_by(PointerButton::Primary) {
            let from = resp.ctx.input(|i| i.pointer.press_origin()).and_then(|p| self.place(p, view));
            self.sel = from.map(|a| (a, a));
        }
        if resp.dragged_by(PointerButton::Primary) {
            if let (Some((a, _)), Some(p)) = (self.sel, resp.interact_pointer_pos()) {
                if let Some(b) = self.place(p, view) {
                    self.sel = Some((a, b));
                }
            }
        }
        if resp.clicked() {
            self.sel = None;
        }
    }

    /// Select between two places (for a caller with its own gestures).
    pub fn set(&mut self, sel: Option<(At, At)>) {
        self.sel = sel;
    }

    pub fn get(&self) -> Option<(At, At)> {
        self.sel
    }

    pub fn clear(&mut self) {
        self.sel = None;
    }

    /// The two ends in text order; none when they are one place.
    fn span(&self) -> Option<(At, At)> {
        self.sel.filter(|(a, b)| a != b).map(|(a, b)| (a.min(b), a.max(b)))
    }

    /// What is selected, a line for each text it takes in (of the last frame:
    /// the one the user saw).
    pub fn text(&self) -> Option<String> {
        let (a, b) = self.span()?;
        let mut out = String::new();
        for run in a.run..=b.run {
            let r = self.last.get(run)?;
            let g = &r.galley;
            let chars: Vec<char> = g.text().chars().collect();
            let from = if run == a.run { a.ch.min(chars.len()) } else { 0 };
            let to = if run == b.run { b.ch.min(chars.len()) } else { chars.len() };
            if run > a.run {
                let prev = &self.last[run - 1];
                out.push_str(if !r.inline {
                    "\n"
                } else if r.pos.x - prev.rect().right() > 1.0 {
                    " "
                } else {
                    ""
                });
            }
            out.extend(&chars[from..to.max(from)]);
        }
        Some(out).filter(|t| !t.trim_matches('\n').is_empty())
    }

    /// Answer egui's `Copy` event (Ctrl+C, Cmd+C): put the selection on the
    /// clipboard. False when there is no event or no selection, so the app's
    /// own copy can take the key.
    pub fn copy(&self, ctx: &Context) -> bool {
        if !ctx.input(|i| i.events.iter().any(|e| matches!(e, Event::Copy))) {
            return false;
        }
        match self.text() {
            Some(t) => {
                ctx.copy_text(t);
                true
            }
            None => false,
        }
    }

    /// Paint the selection over the texts noted this frame; drops it when the
    /// texts are no longer there (the pane changed under it).
    pub fn paint(&mut self, p: &Painter, fill: Color32) {
        let Some((a, b)) = self.span() else { return };
        if b.run >= self.runs.len() {
            self.sel = None;
            return;
        }
        for run in a.run..=b.run {
            let r = &self.runs[run];
            let total: usize = r.galley.rows.iter().map(|row| row.char_count_including_newline().0).sum();
            let from = if run == a.run { a.ch } else { 0 };
            let to = if run == b.run { b.ch } else { total };
            let mut at = 0;
            for row in &r.galley.rows {
                let n = row.char_count_excluding_newline().0;
                let (lo, hi) = (from.max(at), to.min(at + n));
                // A row the selection runs through to its end is filled a little past it.
                // (Not where the next text carries on the same line.)
                let ends_line = !self.runs.get(run + 1).is_some_and(|next| next.inline);
                let through = ends_line && (run != b.run || to > at + n);
                if lo < hi || (through && from <= at + n && to > at + n) {
                    let lo = lo.min(at + n);
                    let x0 = r.galley.pos_from_cursor(CCursor::new(lo)).min.x;
                    let mut x1 = r.galley.pos_from_cursor(CCursor::new(hi.max(lo))).min.x;
                    if through && to > at + n {
                        x1 = r.galley.pos_from_cursor(CCursor::new(at + n)).min.x + 4.0;
                    }
                    if x1 > x0 {
                        let y = row.rect().y_range();
                        p.rect_filled(Rect::from_x_y_ranges(x0..=x1, y).translate(r.pos.to_vec2()), 0.0, fill);
                    }
                }
                at += row.char_count_including_newline().0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2, FontId};

    /// Lay the lines out in a headless frame and note them one under another.
    fn frame(s: &mut Select, owner: &str, lines: &[&str]) {
        let ctx = Context::default();
        pass(&ctx, |ui| {
            let p = ui.painter().clone();
            s.begin(owner);
            for (i, l) in lines.iter().enumerate() {
                let g = p.layout_no_wrap(l.to_string(), FontId::monospace(14.0), Color32::WHITE);
                s.note(pos2(10.0, 10.0 + i as f32 * 20.0), &g);
            }
        });
    }

    /// One headless pass; how many shapes it painted.
    fn pass(ctx: &Context, f: impl FnMut(&mut egui::Ui)) -> usize {
        let out = ctx.run_ui(Default::default(), f);
        let n = out.shapes.len();
        out.drop_without_applying_deltas();
        n
    }

    fn view() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0))
    }

    #[test]
    fn a_drag_copies_from_the_first_place_to_the_second_a_line_each() {
        let mut s = Select::default();
        frame(&mut s, "a", &["alpha", "beta", "gamma"]);
        frame(&mut s, "a", &["alpha", "beta", "gamma"]);
        s.set(Some((At { run: 0, ch: 2 }, At { run: 2, ch: 3 })));
        assert_eq!(s.text().as_deref(), Some("pha\nbeta\ngam"));
        // Backwards is the same selection.
        s.set(Some((At { run: 2, ch: 3 }, At { run: 0, ch: 2 })));
        assert_eq!(s.text().as_deref(), Some("pha\nbeta\ngam"));
    }

    #[test]
    fn inline_texts_join_into_their_line() {
        let ctx = Context::default();
        let mut s = Select::default();
        for _ in 0..2 {
            pass(&ctx, |ui| {
                let p = ui.painter().clone();
                s.begin("a");
                let g = |t: &str| p.layout_no_wrap(t.to_string(), FontId::monospace(14.0), Color32::WHITE);
                s.note(pos2(10.0, 10.0), &g("ab"));
                s.note_inline(pos2(18.0, 10.0), &g("cd"));
                s.note_inline(pos2(100.0, 10.0), &g("ef"));
                s.note(pos2(10.0, 30.0), &g("gh"));
            });
        }
        s.set(Some((At { run: 0, ch: 0 }, At { run: 3, ch: 2 })));
        assert_eq!(s.text().as_deref(), Some("abcd ef\ngh"));
    }

    #[test]
    fn one_place_selects_nothing() {
        let mut s = Select::default();
        frame(&mut s, "a", &["alpha"]);
        frame(&mut s, "a", &["alpha"]);
        s.set(Some((At { run: 0, ch: 2 }, At { run: 0, ch: 2 })));
        assert_eq!(s.text(), None);
    }

    #[test]
    fn another_owner_drops_the_selection() {
        let mut s = Select::default();
        frame(&mut s, "a", &["alpha"]);
        s.set(Some((At { run: 0, ch: 0 }, At { run: 0, ch: 3 })));
        frame(&mut s, "a", &["alpha"]);
        assert!(s.get().is_some());
        frame(&mut s, "b", &["alpha"]);
        assert_eq!(s.get(), None);
    }

    #[test]
    fn a_place_is_inside_a_text_or_the_nearest_one() {
        let mut s = Select::default();
        frame(&mut s, "a", &["alpha", "beta"]);
        frame(&mut s, "a", &["alpha", "beta"]);
        // Below the last line, far to the right: the end of the last text.
        assert_eq!(s.place(pos2(300.0, 300.0), view()), Some(At { run: 1, ch: 4 }));
        // Left of the first: its start.
        assert_eq!(s.place(pos2(0.0, 12.0), view()), Some(At { run: 0, ch: 0 }));
        // Texts outside the view do not count.
        let up = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 25.0));
        assert_eq!(s.place(pos2(300.0, 300.0), up).map(|a| a.run), Some(0));
    }

    #[test]
    fn the_selection_is_painted_and_dropped_when_the_texts_go() {
        let mut s = Select::default();
        frame(&mut s, "a", &["alpha", "beta"]);
        s.set(Some((At { run: 0, ch: 1 }, At { run: 1, ch: 2 })));
        let ctx = Context::default();
        assert!(pass(&ctx, |ui| s.paint(ui.painter(), Color32::RED)) > 0);
        frame(&mut s, "a", &["alpha"]);
        pass(&ctx, |ui| s.paint(ui.painter(), Color32::RED));
        assert_eq!(s.get(), None);
    }
}
