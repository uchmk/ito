//! An input method for text the app paints itself (a terminal's cells, a page
//! of flowing lines), where no `TextEdit` does it.
//!
//! An input method types only where the window says text is being edited, and
//! puts its candidate window at the rectangle it is given. It then sends the
//! composition so far (`Preedit`, only when it changes) and, at the end, what it
//! settled on (`Commit`). [`Ime::frame`] does the three things an app would
//! otherwise write itself:
//!
//! - tells the window where the input point is, for the candidate window
//!   ([`Orientation::Vertical`] puts it beside a column of vertical text);
//! - keeps the composition between frames, since an event comes only when it
//!   changes, and keeps a commit shown for a moment, until the app has drawn it
//!   itself, so the text does not vanish and come back;
//! - hands over the commits.
//!
//! While a composition is under way the input method owns Enter, Backspace and
//! the arrows: [`Ime::swallow_keys`] takes them out of the frame's input, so the
//! app does not also move its caret or send a newline.
//!
//! Call [`Ime::frame`] only while the app owns the keys: an input method works
//! only while the window says text is edited, and asking for focus every frame
//! breaks a composition.

use egui::{Context, Event, Id, ImeEvent, Pos2, Rect, Vec2};

/// How long a commit stays drawn where the composition was, for the app to echo it.
pub const HOLD: f64 = 0.25;

/// Which way the text runs at the input point. It decides where the candidate
/// window opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Lines run left to right: the candidates open below the input point.
    #[default]
    Horizontal,
    /// Lines run top to bottom: the candidates open beside the column, to its
    /// right (the next column of vertical text is to its left, so this is the
    /// side that stays clear).
    Vertical,
}

/// What [`Ime::frame`] saw this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    /// What the input method settled on, in order. Send or insert each.
    pub commits: Vec<String>,
    /// The composition to draw at the input point, underlined (see [`paint`]).
    /// Empty when there is none. Right after a commit it is the committed text,
    /// until [`HOLD`] has passed or `key` changed.
    pub preedit: String,
    /// A composition is under way (the keys are the input method's).
    pub composing: bool,
}

/// One input point's input method. Cheap to make every frame; the state lives
/// in the context under `id`.
#[derive(Clone, Copy, Debug)]
pub struct Ime {
    id: Id,
    purpose: egui::IMEPurpose,
    orientation: Orientation,
}

#[derive(Clone, Debug, Default)]
struct State {
    /// The composition, or the commit being held.
    text: String,
    /// Set by a commit: the input point's key and the time. `None` while composing.
    held: Option<(u64, f64)>,
}

impl Ime {
    /// The input method of the input point `id` names.
    pub fn new(id: Id) -> Ime {
        Ime { id: id.with("ito-ime"), purpose: egui::IMEPurpose::Normal, orientation: Orientation::Horizontal }
    }

    /// What is typed (`Terminal` and `Password` make some input methods go quiet).
    pub fn purpose(mut self, purpose: egui::IMEPurpose) -> Ime {
        self.purpose = purpose;
        self
    }

    /// Which way the text runs.
    pub fn orientation(mut self, orientation: Orientation) -> Ime {
        self.orientation = orientation;
        self
    }

    /// Whether a composition is under way, as of the last frame.
    pub fn composing(&self, ctx: &Context) -> bool {
        let s = ctx.data(|d| d.get_temp::<State>(self.id)).unwrap_or_default();
        !s.text.is_empty() && s.held.is_none()
    }

    /// Take Enter, Backspace, Delete, the arrows, Tab and Escape out of this
    /// frame's input while a composition is under way. Call before reading keys.
    pub fn swallow_keys(&self, ctx: &Context) {
        if self.composing(ctx) {
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { key, .. } if owned(*key))));
        }
    }

    /// Tell the window the input point is `at` (a caret-sized rectangle), read
    /// this frame's input method events, and return what they came to. `key`
    /// names the input point's position (a cell, an offset): a held commit is
    /// dropped when it changes, because the echo has moved the point.
    pub fn frame(&self, ui: &egui::Ui, at: Rect, key: u64) -> Frame {
        let rect = place(at, self.orientation);
        ui.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: self.purpose,
                // egui-winit puts the candidate window by `rect`, so it is the
                // input point, not the whole text.
                rect,
                cursor_rect: rect,
                should_interrupt_composition: false,
            });
        });
        let now = ui.input(|i| i.time);
        let events: Vec<ImeEvent> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Ime(e) => Some(e.clone()),
                    _ => None,
                })
                .collect()
        });
        let mut state = ui.ctx().data(|d| d.get_temp::<State>(self.id)).unwrap_or_default();
        let commits = apply(&mut state, &events, key, now);
        let (preedit, wait) = shown(&mut state, key, now);
        if let Some(wait) = wait {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        let composing = !preedit.is_empty() && state.held.is_none();
        ui.ctx().data_mut(|d| d.insert_temp(self.id, state));
        Frame { commits, preedit, composing }
    }
}

/// Keys the input method owns while it composes.
fn owned(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(key, Enter | Backspace | Delete | Tab | Escape | ArrowLeft | ArrowRight | ArrowUp | ArrowDown | Home | End | PageUp | PageDown)
}

/// Where the candidate window is asked to open. egui-winit opens it at the
/// rectangle's bottom left, so for a column of vertical text the rectangle is
/// moved to put that corner at the column's top right.
pub fn place(at: Rect, orientation: Orientation) -> Rect {
    match orientation {
        Orientation::Horizontal => at,
        Orientation::Vertical => Rect::from_min_size(Pos2::new(at.right(), at.top() - at.height()), at.size()),
    }
}

/// Fold this frame's events into the state; the commits, in order.
fn apply(state: &mut State, events: &[ImeEvent], key: u64, now: f64) -> Vec<String> {
    let mut commits = Vec::new();
    for e in events {
        match e {
            ImeEvent::Preedit { text, .. } => *state = State { text: text.clone(), held: None },
            ImeEvent::Commit(text) => {
                if !text.is_empty() {
                    commits.push(text.clone());
                }
                *state = State { text: text.clone(), held: Some((key, now)) };
            }
            _ => {}
        }
    }
    commits
}

/// The text to draw now, and how long until it should be looked at again.
fn shown(state: &mut State, key: u64, now: f64) -> (String, Option<f64>) {
    match state.held {
        Some((was, since)) if was == key && now - since < HOLD => (state.text.clone(), Some(HOLD - (now - since))),
        Some(_) => {
            *state = State::default();
            (String::new(), None)
        }
        None => (state.text.clone(), None),
    }
}

/// Draw `text` at `origin` as a composition: over a `bg` patch, underlined in
/// `fg`. Returns the rectangle it covers. For horizontal text; vertical text is
/// laid out by the app, which can underline (or draw a line beside) its own.
pub fn paint(painter: &egui::Painter, origin: Pos2, text: &str, font: egui::FontId, fg: egui::Color32, bg: egui::Color32) -> Rect {
    let galley = painter.layout_no_wrap(text.to_owned(), font, fg);
    let area = Rect::from_min_size(origin, galley.size());
    painter.rect_filled(area, egui::CornerRadius::ZERO, bg);
    painter.galley(area.min, galley, fg);
    painter.line_segment([area.left_bottom() - Vec2::new(0.0, 1.0), area.right_bottom() - Vec2::new(0.0, 1.0)], egui::Stroke::new(1.0, fg));
    area
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preedit(t: &str) -> ImeEvent {
        ImeEvent::Preedit { text: t.into(), active_range_chars: None }
    }

    #[test]
    fn a_composition_is_kept_until_the_next_event() {
        let mut s = State::default();
        assert!(apply(&mut s, &[preedit("に")], 7, 1.0).is_empty());
        assert_eq!(shown(&mut s, 7, 1.1), ("に".into(), None));
        // No event this frame: it is still there.
        assert!(apply(&mut s, &[], 7, 1.2).is_empty());
        assert_eq!(shown(&mut s, 7, 1.2).0, "に");
        apply(&mut s, &[preedit("")], 7, 1.3);
        assert_eq!(shown(&mut s, 7, 1.3).0, "");
    }

    #[test]
    fn a_commit_is_handed_over_and_held_until_the_point_moves_or_time_passes() {
        let mut s = State::default();
        let commits = apply(&mut s, &[preedit("にほん"), ImeEvent::Commit("日本".into())], 3, 2.0);
        assert_eq!(commits, ["日本"]);
        let (text, wait) = shown(&mut s, 3, 2.1);
        assert_eq!(text, "日本");
        assert!((wait.unwrap() - 0.15).abs() < 1e-9);
        // The echo moved the point.
        assert_eq!(shown(&mut s, 4, 2.1).0, "");
        // Or the moment passed.
        apply(&mut s, &[ImeEvent::Commit("x".into())], 3, 5.0);
        assert_eq!(shown(&mut s, 3, 5.0 + HOLD).0, "");
        // An empty commit is not handed over.
        assert!(apply(&mut s, &[ImeEvent::Commit(String::new())], 3, 6.0).is_empty());
    }

    #[test]
    fn vertical_candidates_open_beside_the_column() {
        let at = Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(20.0, 20.0));
        assert_eq!(place(at, Orientation::Horizontal), at);
        // The window opens at the bottom left of the result: the column's top right.
        assert_eq!(place(at, Orientation::Vertical).left_bottom(), Pos2::new(120.0, 50.0));
    }

    /// Through a real egui frame: the output carries the rectangle, events are
    /// read, and Enter is taken while composing.
    #[test]
    fn a_frame_reports_the_point_and_swallows_enter_while_composing() {
        let ctx = Context::default();
        let ime = Ime::new(Id::new("line")).orientation(Orientation::Vertical);
        let at = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(16.0));
        let run = |events: Vec<Event>| {
            let mut got = Frame::default();
            let mut enters = 0;
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                ime.swallow_keys(ui.ctx());
                enters = ui.input(|i| i.events.iter().filter(|e| matches!(e, Event::Key { key: egui::Key::Enter, .. })).count());
                got = ime.frame(ui, at, 0);
            });
            out.textures_delta.clear();
            (got, enters, out.platform_output.ime)
        };
        let enter =
            || Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let (f, _, out) = run(vec![Event::Ime(preedit("にほん"))]);
        assert_eq!((f.preedit.as_str(), f.composing), ("にほん", true));
        assert_eq!(out.unwrap().rect, place(at, Orientation::Vertical));
        // The next frame is composing from the start: Enter does not get through.
        let (f, enters, _) = run(vec![enter()]);
        assert_eq!((enters, f.composing), (0, true));
        let (f, _, _) = run(vec![Event::Ime(ImeEvent::Commit("日本".into()))]);
        assert_eq!((f.commits, f.composing), (vec!["日本".to_string()], false));
        // Composition over: Enter is the app's again.
        let (_, enters, _) = run(vec![enter()]);
        assert_eq!(enters, 1);
    }
}
