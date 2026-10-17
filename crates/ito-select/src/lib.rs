//! Text picked up with the mouse and copied, in a pane that is painted
//! rather than made of widgets (a preview, a detail column). egui can select
//! the text of a `Label`, but not a galley handed to a `Painter`; this keeps
//! the texts a frame painted, in the order they were painted, and turns a drag
//! over them into a selection between two places in that order: a text's
//! number and a character in it. Copying takes what lies between, a line for
//! each text.
//!
//! It behaves as tsumugi's terminal does: the selection is [`fill`]ed behind
//! the letters (over what the pane painted under them), a drag, a double click
//! (a word, or the whole web address under it) or a triple click (a line)
//! hands the app what it picked up to copy, and a click lets it go.
//!
//! One frame of use:
//!
//! 1. [`Select::begin`] with what the pane shows (another item drops the
//!    selection);
//! 2. [`Select::note`] each galley just before painting it, top to bottom,
//!    with the painter it is painted with: its selection goes there;
//! 3. [`Select::drive`] with the pane's `Response` (made with
//!    `Sense::click_and_drag()`), then [`Select::paint`] once the texts are
//!    painted;
//! 4. copy [`Driven::copy`] (copy on select), and answer egui's `Copy` event
//!    (Ctrl+C or Cmd+C) with [`Select::copy`], or take [`Select::text`].

use std::sync::Arc;

use egui::epaint::text::cursor::CCursor;
use egui::layers::ShapeIdx;
use egui::{Color32, Context, Event, Galley, Painter, PointerButton, Pos2, Rect, Response, Shape};

/// The selection's colour: tsumugi's. The accent mixed into the ground the
/// text sits on (a pane's or a panel's), a quarter of the way on a dark theme
/// and less on a light one. Opaque, so it goes behind the letters
/// ([`Select::note`] keeps the place); the text keeps its own colour on it.
pub fn fill(ground: Color32, accent: Color32, light: bool) -> Color32 {
    let t = if light { 0.18 } else { 0.25 };
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(ground.r(), accent.r()), m(ground.g(), accent.g()), m(ground.b(), accent.b()))
}

/// What a frame's mouse did ([`Select::drive`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Driven {
    /// A selection was just made (a drag let go, a double or a triple click):
    /// its text, for the app to copy (tsumugi's copy on select) and to offer
    /// as the primary selection where there is one.
    pub copy: Option<String>,
    /// The web address that Ctrl (Cmd on a Mac) and a click landed on: the
    /// pane opens it.
    pub open: Option<String>,
}

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
    /// The place kept just before the text in its painter's list: its
    /// selection goes there, over what lies under the text and under the text.
    slot: (Painter, ShapeIdx),
    /// The pass it was painted in: a place in another pass's list is not its.
    pass: u64,
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

    /// A text about to be painted with `p`, its top left at `pos`. Note them in
    /// reading order, each just before painting it (after what goes under it,
    /// such as a code block's band): its selection is painted at this point.
    pub fn note(&mut self, p: &Painter, pos: Pos2, galley: &Arc<Galley>) {
        self.push(p, pos, galley, false);
    }

    /// A text painted on the same line as the one noted before it (the spans
    /// of a line, each placed by itself): a copy joins them, with a space where
    /// they do not touch, instead of breaking the line between them.
    pub fn note_inline(&mut self, p: &Painter, pos: Pos2, galley: &Arc<Galley>) {
        self.push(p, pos, galley, true);
    }

    fn push(&mut self, p: &Painter, pos: Pos2, galley: &Arc<Galley>, inline: bool) {
        let slot = (p.clone(), p.add(Shape::Noop));
        let pass = p.ctx().cumulative_pass_nr();
        self.runs.push(Run { pos, galley: galley.clone(), inline, slot, pass });
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

    /// The character under `at` (not the boundary nearest to it): its text's
    /// number and its index in it. None over an empty text.
    fn char_at(&self, at: Pos2, view: Rect) -> Option<(usize, usize)> {
        let p = self.place(at, view)?;
        let r = &self.last[p.run];
        let len = r.galley.text().chars().count();
        if len == 0 {
            return None;
        }
        let x = at.x - r.pos.x;
        let mut c = p.ch.min(len);
        if c > 0 && x < r.galley.pos_from_cursor(CCursor::new(c)).min.x {
            c -= 1;
        }
        Some((p.run, c.min(len - 1)))
    }

    /// The selection of a drag from `from` to `to`: the characters under both
    /// ends are in it whole, wherever in them the pointer is (a full-width
    /// character is one unit, not two halves), as in a terminal.
    fn dragged(&self, from: Pos2, to: Pos2, view: Rect) -> Option<(At, At)> {
        let span = |at: Pos2| {
            let p = self.place(at, view)?;
            Some(match self.char_at(at, view) {
                Some((run, c)) => (At { run, ch: c }, At { run, ch: c + 1 }),
                None => (p, p),
            })
        };
        let (f, t) = (span(from)?, span(to)?);
        Some(if t.0 >= f.0 { (f.0, t.1) } else { (f.1, t.0) })
    }

    /// The word under `at`, as two places: a run of letters and digits, of
    /// blanks, or of other marks.
    pub fn word_at(&self, at: Pos2, view: Rect) -> Option<(At, At)> {
        let (run, c) = self.char_at(at, view)?;
        let chars: Vec<char> = self.last[run].galley.text().chars().collect();
        let class = |ch: char| {
            if ch.is_alphanumeric() || ch == '_' {
                0
            } else if ch.is_whitespace() {
                1
            } else {
                2
            }
        };
        let k = class(chars[c]);
        let mut lo = c;
        while lo > 0 && class(chars[lo - 1]) == k {
            lo -= 1;
        }
        let mut hi = c + 1;
        while hi < chars.len() && class(chars[hi]) == k {
            hi += 1;
        }
        Some((At { run, ch: lo }, At { run, ch: hi }))
    }

    /// The line under `at`: its text, and the texts that carry on the same line
    /// ([`Select::note_inline`]).
    pub fn line_at(&self, at: Pos2, view: Rect) -> Option<(At, At)> {
        let run = self.place(at, view)?.run;
        let mut first = run;
        while first > 0 && self.last[first].inline {
            first -= 1;
        }
        let mut last = run;
        while self.last.get(last + 1).is_some_and(|r| r.inline) {
            last += 1;
        }
        let ch = self.last[last].galley.text().chars().count();
        Some((At { run: first, ch: 0 }, At { run: last, ch }))
    }

    /// The web address under `at` (`http://`, `https://`, `ftp://`, or `www.`
    /// made into `https://`), without the marks a sentence puts after it.
    pub fn url_at(&self, at: Pos2, view: Rect) -> Option<String> {
        self.url_span(at, view).map(|(_, _, url)| url)
    }

    /// The web address under `at`, and the two places around it as written.
    fn url_span(&self, at: Pos2, view: Rect) -> Option<(At, At, String)> {
        let (run, c) = self.char_at(at, view)?;
        let chars: Vec<char> = self.last[run].galley.text().chars().collect();
        let stop = |ch: char| ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '`' | '|');
        if stop(chars[c]) {
            return None;
        }
        let mut lo = c;
        while lo > 0 && !stop(chars[lo - 1]) {
            lo -= 1;
        }
        let mut hi = c + 1;
        while hi < chars.len() && !stop(chars[hi]) {
            hi += 1;
        }
        // A bracket or quote that opens before the address is not part of it.
        while lo < hi && matches!(chars[lo], '(' | '[' | '{' | '\'' | '\u{300c}') {
            lo += 1;
        }
        let count = |ch: char, lo: usize, hi: usize| chars[lo..hi].iter().filter(|&&x| x == ch).count();
        while hi > lo
            && matches!(chars[hi - 1], '.' | ',' | ';' | ':' | '!' | '?' | '\'' | ')' | ']' | '}' | '\u{3002}' | '\u{3001}' | '\u{300d}')
        {
            // Keep a closing bracket that the address itself opened.
            if chars[hi - 1] == ')' && count('(', lo, hi) >= count(')', lo, hi) {
                break;
            }
            hi -= 1;
        }
        if !(lo..hi).contains(&c) {
            return None;
        }
        let tok: String = chars[lo..hi].iter().collect();
        let lower = tok.to_ascii_lowercase();
        let url = if ["http://", "https://", "ftp://"].iter().any(|p| lower.starts_with(p) && tok.len() > p.len()) {
            tok
        } else if lower.starts_with("www.") && tok.len() > 4 {
            format!("https://{tok}")
        } else {
            return None;
        };
        Some((At { run, ch: lo }, At { run, ch: hi }, url))
    }

    /// Follow the mouse on the pane as tsumugi's terminal does: a press and
    /// drag picks text up from where it was pressed, a click drops the
    /// selection, a double click takes the word (the whole web address over
    /// one) and a triple click the line. `view` is the part of the pane that
    /// shows texts.
    ///
    /// Says what to copy when a selection was just made, and the web address
    /// that Ctrl (Cmd on a Mac) and a click landed on, for the pane to open.
    /// [`Select::url_at`] says whether the pointer is over one, for the cursor.
    pub fn drive(&mut self, resp: &Response, view: Rect) -> Driven {
        let mut out = Driven::default();
        if resp.drag_started_by(PointerButton::Primary) {
            let from = resp.ctx.input(|i| i.pointer.press_origin()).and_then(|p| self.place(p, view));
            self.sel = from.map(|a| (a, a));
        }
        if resp.dragged_by(PointerButton::Primary) || resp.drag_stopped_by(PointerButton::Primary) {
            let origin = resp.ctx.input(|i| i.pointer.press_origin());
            if let (Some(_), Some(o), Some(p)) = (self.sel, origin, resp.interact_pointer_pos()) {
                if let Some(sel) = self.dragged(o, p, view) {
                    self.sel = Some(sel);
                }
            }
        }
        if resp.drag_stopped_by(PointerButton::Primary) {
            out.copy = self.text();
        }
        let at = resp.interact_pointer_pos();
        if resp.clicked() {
            self.sel = None;
            if resp.ctx.input(|i| i.modifiers.command) {
                out.open = at.and_then(|p| self.url_at(p, view));
                return out;
            }
        }
        if let Some(p) = at {
            let picked = if resp.triple_clicked() {
                Some(self.line_at(p, view))
            } else if resp.double_clicked() {
                Some(self.url_span(p, view).map(|(a, b, _)| (a, b)).or_else(|| self.word_at(p, view)))
            } else {
                None
            };
            if let Some(sel) = picked {
                self.sel = sel;
                out.copy = self.text();
            }
        }
        out
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

    /// Paint the selection behind the texts noted this frame, each in the
    /// place its [`Select::note`] kept, with `fill` (see [`fill`]); drops it
    /// when the texts are no longer there (the pane changed under it).
    pub fn paint(&mut self, fill: Color32) {
        let Some((a, b)) = self.span() else { return };
        if b.run >= self.runs.len() {
            self.sel = None;
            return;
        }
        for run in a.run..=b.run {
            let r = &self.runs[run];
            let mut rects = Vec::new();
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
                        let rect = Rect::from_x_y_ranges(x0..=x1, y).translate(r.pos.to_vec2());
                        rects.push(Shape::rect_filled(rect, 0.0, fill));
                    }
                }
                at += row.char_count_including_newline().0;
            }
            let (p, idx) = &r.slot;
            if p.ctx().cumulative_pass_nr() == r.pass {
                p.set(*idx, Shape::Vec(rects));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2, FontId, Modifiers, RawInput};

    /// Note the lines one under another and paint them, as a pane does.
    fn lines(s: &mut Select, p: &Painter, lines: &[&str]) {
        for (i, l) in lines.iter().enumerate() {
            let g = p.layout_no_wrap(l.to_string(), FontId::monospace(14.0), Color32::WHITE);
            let at = pos2(10.0, 10.0 + i as f32 * 20.0);
            s.note(p, at, &g);
            p.galley(at, g, Color32::WHITE);
        }
    }

    /// Lay the lines out in a headless frame.
    fn frame(s: &mut Select, owner: &str, texts: &[&str]) {
        painted(s, owner, texts, None);
    }

    /// A headless frame of the lines, painting the selection with `fill`:
    /// the shapes it painted.
    fn painted(s: &mut Select, owner: &str, texts: &[&str], fill: Option<Color32>) -> Vec<Shape> {
        let ctx = Context::default();
        let out = ctx.run_ui(Default::default(), |ui| {
            s.begin(owner);
            lines(s, ui.painter(), texts);
            if let Some(fill) = fill {
                s.paint(fill);
            }
        });
        let shapes = out.shapes.iter().map(|c| c.shape.clone()).collect();
        out.drop_without_applying_deltas();
        shapes
    }

    fn view() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0))
    }

    #[test]
    fn the_fill_is_tsumugis_accent_mixed_into_the_ground() {
        let (ground, accent) = (Color32::from_rgb(0, 0, 0), Color32::from_rgb(200, 100, 40));
        assert_eq!(fill(ground, accent, false), Color32::from_rgb(50, 25, 10));
        assert_eq!(fill(ground, accent, true), Color32::from_rgb(36, 18, 7));
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

    /// The point over character `ch` of text `run`, after two frames.
    fn over(s: &Select, run: usize, ch: usize) -> Pos2 {
        let r = &s.last[run];
        let x = r.galley.pos_from_cursor(CCursor::new(ch)).min.x;
        let w = r.galley.pos_from_cursor(CCursor::new(ch + 1)).min.x - x;
        r.pos + vec2(x + w / 2.0, 6.0)
    }

    /// The point just after the boundary before character `ch` of text `run`.
    fn before(s: &Select, run: usize, ch: usize) -> Pos2 {
        let r = &s.last[run];
        r.pos + vec2(r.galley.pos_from_cursor(CCursor::new(ch)).min.x + 1.0, 6.0)
    }

    fn two_frames(lines: &[&str]) -> Select {
        let mut s = Select::default();
        frame(&mut s, "a", lines);
        frame(&mut s, "a", lines);
        s
    }

    #[test]
    fn a_double_click_takes_the_word_and_a_triple_click_the_line() {
        let s = two_frames(&["see foo_bar, now", "next"]);
        let (a, b) = s.word_at(over(&s, 0, 6), view()).unwrap();
        assert_eq!((a, b), (At { run: 0, ch: 4 }, At { run: 0, ch: 11 }));
        let (a, b) = s.line_at(over(&s, 0, 6), view()).unwrap();
        assert_eq!((a, b), (At { run: 0, ch: 0 }, At { run: 0, ch: 16 }));
    }

    #[test]
    fn a_url_loses_the_marks_after_it() {
        let s = two_frames(&["go (https://a.example/x_(y)). ok", "www.example.org, and", "plain words"]);
        assert_eq!(s.url_at(over(&s, 0, 10), view()).as_deref(), Some("https://a.example/x_(y)"));
        assert_eq!(s.url_span(over(&s, 0, 10), view()).map(|(a, b, _)| (a.ch, b.ch)), Some((4, 27)));
        assert_eq!(s.url_at(over(&s, 1, 3), view()).as_deref(), Some("https://www.example.org"));
        assert_eq!(s.url_at(over(&s, 2, 2), view()), None);
        assert_eq!(s.url_at(over(&s, 0, 1), view()), None);
        // The bracket before it and the marks after it are not the address.
        assert_eq!(s.url_at(over(&s, 0, 3), view()), None);
        assert_eq!(s.url_at(over(&s, 0, 28), view()), None);
    }

    #[test]
    fn inline_texts_join_into_their_line() {
        let ctx = Context::default();
        let mut s = Select::default();
        for _ in 0..2 {
            let out = ctx.run_ui(Default::default(), |ui| {
                let p = ui.painter().clone();
                s.begin("a");
                let g = |t: &str| p.layout_no_wrap(t.to_string(), FontId::monospace(14.0), Color32::WHITE);
                s.note(&p, pos2(10.0, 10.0), &g("ab"));
                s.note_inline(&p, pos2(18.0, 10.0), &g("cd"));
                s.note_inline(&p, pos2(100.0, 10.0), &g("ef"));
                s.note(&p, pos2(10.0, 30.0), &g("gh"));
            });
            out.drop_without_applying_deltas();
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
    fn the_selection_goes_behind_its_text_and_is_dropped_when_the_texts_go() {
        let mut s = Select::default();
        frame(&mut s, "a", &["alpha", "beta"]);
        s.set(Some((At { run: 0, ch: 1 }, At { run: 1, ch: 2 })));
        let shapes = painted(&mut s, "a", &["alpha", "beta"], Some(Color32::RED));
        let fill = |sh: &Shape| matches!(sh, Shape::Vec(v) if !v.is_empty());
        let text = |sh: &Shape| matches!(sh, Shape::Text(_));
        // Each line's fill comes just before its text: over what lies under
        // the text, under the letters.
        let order: Vec<bool> = shapes.iter().filter(|sh| fill(sh) || text(sh)).map(fill).collect();
        assert_eq!(order, [true, false, true, false]);
        painted(&mut s, "a", &["alpha"], Some(Color32::RED));
        assert_eq!(s.get(), None);
    }

    /// A pane of lines, driven by the mouse a pass at a time.
    struct Pane {
        ctx: Context,
        s: Select,
        lines: Vec<&'static str>,
        t: f64,
    }

    impl Pane {
        fn new(lines: &[&'static str]) -> Self {
            let mut pane = Pane { ctx: Context::default(), s: Select::default(), lines: lines.to_vec(), t: 0.0 };
            pane.step(Modifiers::NONE, vec![]);
            pane.step(Modifiers::NONE, vec![]);
            pane
        }

        fn step(&mut self, modifiers: Modifiers, events: Vec<Event>) -> Driven {
            self.t += 0.05;
            let mut events = events;
            events.insert(0, Event::ModifiersChanged(modifiers));
            let input = RawInput { time: Some(self.t), events, ..Default::default() };
            let (s, texts) = (&mut self.s, &self.lines);
            let mut driven = Driven::default();
            let out = self.ctx.run_ui(input, |ui| {
                s.begin("a");
                lines(s, ui.painter(), texts);
                let resp = ui.interact(view(), ui.id().with("pane"), egui::Sense::click_and_drag());
                driven = s.drive(&resp, view());
                s.paint(Color32::RED);
            });
            out.drop_without_applying_deltas();
            driven
        }

        fn button(&mut self, at: Pos2, down: bool, modifiers: Modifiers) -> Driven {
            let e = Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: down, modifiers };
            self.step(modifiers, vec![Event::PointerMoved(at), e])
        }

        fn click(&mut self, at: Pos2, modifiers: Modifiers) -> Driven {
            assert_eq!(self.button(at, true, modifiers), Driven::default());
            self.button(at, false, modifiers)
        }
    }

    #[test]
    fn letting_a_drag_go_hands_over_what_it_picked_up() {
        let mut pane = Pane::new(&["alpha", "beta"]);
        let (from, to) = (before(&pane.s, 0, 1), before(&pane.s, 1, 2));
        pane.step(Modifiers::NONE, vec![Event::PointerMoved(from)]);
        assert_eq!(pane.button(from, true, Modifiers::NONE), Driven::default());
        let moving = pane.step(Modifiers::NONE, vec![Event::PointerMoved(to)]);
        assert_eq!(moving.copy, None, "nothing is copied while the drag goes on");
        assert_eq!(pane.s.text().as_deref(), Some("lpha\nbet"));
        assert_eq!(pane.button(to, false, Modifiers::NONE).copy.as_deref(), Some("lpha\nbet"));
        // A click lets it go.
        pane.t += 1.0;
        let later = pane.click(over(&pane.s, 0, 2), Modifiers::NONE);
        assert_eq!(later, Driven::default());
        assert_eq!(pane.s.get(), None);
    }

    #[test]
    fn a_double_click_on_an_address_takes_all_of_it_and_a_triple_the_line() {
        let mut pane = Pane::new(&["go https://a.example/x_y ok", "next"]);
        let at = over(&pane.s, 0, 9);
        assert_eq!(pane.click(at, Modifiers::NONE).copy, None);
        assert_eq!(pane.click(at, Modifiers::NONE).copy.as_deref(), Some("https://a.example/x_y"));
        assert_eq!(pane.click(at, Modifiers::NONE).copy.as_deref(), Some("go https://a.example/x_y ok"));
        // Ctrl+click opens it.
        pane.t += 1.0;
        let opened = pane.click(at, Modifiers::COMMAND);
        assert_eq!(opened.open.as_deref(), Some("https://a.example/x_y"));
        assert_eq!(opened.copy, None);
        // Off an address, a double click takes the word.
        pane.t += 1.0;
        let word = over(&pane.s, 1, 1);
        pane.click(word, Modifiers::NONE);
        assert_eq!(pane.click(word, Modifiers::NONE).copy.as_deref(), Some("next"));
    }

    #[test]
    fn a_drag_takes_whole_characters_wherever_in_them_it_starts_and_ends() {
        let mut s = two_frames(&["abあいうcd"]);
        let near_right = |s: &Select, ch: usize| {
            let r = &s.last[0];
            let x = r.galley.pos_from_cursor(CCursor::new(ch)).min.x;
            let w = r.galley.pos_from_cursor(CCursor::new(ch + 1)).min.x - x;
            r.pos + vec2(x + w * 0.9, 6.0)
        };
        let (p2, p4, q3) = (near_right(&s, 2), before(&s, 0, 4), near_right(&s, 3));
        // Pressed on the right edge of one full-width letter, released on the left edge of another.
        let sel = s.dragged(p2, p4, view()).unwrap();
        s.set(Some(sel));
        assert_eq!(s.text().as_deref(), Some("あいう"));
        let sel = s.dragged(p4, p2, view()).unwrap();
        s.set(Some(sel));
        assert_eq!(s.text().as_deref(), Some("あいう"));
        let sel = s.dragged(q3, q3, view()).unwrap();
        s.set(Some(sel));
        assert_eq!(s.text().as_deref(), Some("い"));
    }
}
