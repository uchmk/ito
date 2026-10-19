//! The list the notification bell opens, the same in every app.
//!
//! The app keeps its notices (a `Vec<Notice<T>>`, oldest first as they are
//! pushed) and calls [`show`] every frame the list is open, with the bell's
//! rect from `ito_titlebar::Out::bell_rect`. The newest is on top, unread
//! ones bright, each with its age and a dot in its kind's colour; a click or
//! Enter opens the row's [`Notice::go`] (the app's own jump target: a
//! session, a commit), and the app answers the [`Action`] it gets back.
//! This crate knows no app and no language: every text is in [`Words`].
//! Colours are `ito_theme::colors()`.
//!
//! [`unread`] and [`any_error`] give the badge of `ito_titlebar::Spec`.

use std::time::{Duration, SystemTime};

use egui::{vec2, Align2, Color32, FontId, Id, Rect, RichText, Sense, Stroke};

/// The list's width and a row's height.
const WIDTH: f32 = 360.0;
const ROW_H: f32 = 46.0;
/// The most the rows take before they scroll.
const MAX_ROWS_H: f32 = 420.0;

/// What a notice is about; it is the colour of its dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Finished well (`done`).
    Ok,
    /// For your information (`run`).
    Info,
    /// Needs a look, but nothing failed (`magenta`).
    Warn,
    /// Failed (`err`).
    Err,
    /// Waits for a person (`wait`).
    Wait,
}

impl Kind {
    /// The kind's colour in the theme.
    pub fn color(self) -> Color32 {
        let c = ito_theme::colors();
        match self {
            Kind::Ok => c.done,
            Kind::Info => c.run,
            Kind::Warn => c.magenta,
            Kind::Err => c.err,
            Kind::Wait => c.wait,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One notice. `T` is the app's own place to go to when it is opened.
#[derive(Clone, Debug)]
pub struct Notice<T> {
    pub id: u64,
    pub at: SystemTime,
    pub kind: Kind,
    pub title: String,
    pub note: Option<String>,
    /// Where a click goes; none, and the row only marks itself read.
    pub go: Option<T>,
    pub read: bool,
}

/// Every text the list shows, in the app's language.
#[derive(Clone, Copy, Debug)]
pub struct Words<'a> {
    /// The heading, with the unread count already in it ("Notices (3 unread)").
    pub title: &'a str,
    pub mark_read: &'a str,
    pub empty: &'a str,
    /// The age around `{}`: `"{} ago"` gives "3m ago".
    pub ago: &'a str,
    /// A word after each title, in [`Kind`]'s order: Ok, Info, Warn, Err, Wait.
    pub kinds: [&'a str; 5],
}

/// What the app is to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action<T> {
    /// A row was opened: go to `T` (when it has one) and mark that id read.
    Open(Option<T>, u64),
    /// "Mark all read".
    ReadAll,
    /// Esc, or a click outside the list.
    Close,
}

/// How many are unread: the badge's number.
pub fn unread<T>(notices: &[Notice<T>]) -> usize {
    notices.iter().filter(|n| !n.read).count()
}

/// Whether an unread one is an error: the badge's red.
pub fn any_error<T>(notices: &[Notice<T>]) -> bool {
    notices.iter().any(|n| !n.read && n.kind == Kind::Err)
}

/// The age as short text: `42s`, `3m`, `5h`, `2d`.
pub fn age(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

/// The list, under `anchor` (the bell) and ending at its right edge. Draw it
/// while it is open; the answer is what the app does this frame.
pub fn show<T: Clone>(ctx: &egui::Context, anchor: Rect, notices: &[Notice<T>], words: &Words<'_>) -> Option<Action<T>> {
    let c = ito_theme::colors();
    let mut action = None;
    let now = SystemTime::now();
    // The row the keys are on (0 is the newest), kept between frames; the
    // arrows move it, Enter opens it. None until a key is pressed.
    let sel_id = Id::new("ito-notices-row");
    let mut sel = ctx.data(|d| d.get_temp::<Option<usize>>(sel_id)).flatten();
    let mut moved = false;
    if !notices.is_empty() {
        let last = notices.len() - 1;
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                sel = Some(sel.map_or(0, |s| (s + 1).min(last)));
                moved = true;
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                sel = Some(sel.map_or(0, |s| s.saturating_sub(1)));
                moved = true;
            }
        });
        if let Some(s) = sel.filter(|_| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))) {
            if let Some(n) = notices.iter().rev().nth(s) {
                action = Some(Action::Open(n.go.clone(), n.id));
            }
        }
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        action = Some(Action::Close);
    }
    ctx.data_mut(|d| d.insert_temp(sel_id, sel));
    let area = egui::Area::new(Id::new("ito-notices"))
        .order(egui::Order::Foreground)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(anchor.right_bottom() + vec2(0.0, 4.0));
    let mut first = None;
    let resp = area.show(ctx, |ui| {
        egui::Frame::NONE.fill(c.panel).stroke(Stroke::new(1.0, c.border_strong())).corner_radius(10.0).inner_margin(10.0).show(ui, |ui| {
            ui.set_width(WIDTH);
            let unread = unread(notices);
            ui.horizontal(|ui| {
                ui.label(RichText::new(words.title).strong().color(c.fg));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mark = ui.add_enabled(unread > 0, egui::Button::new(words.mark_read).small());
                    if unread > 0 {
                        first = Some(mark.id);
                    }
                    if mark.clicked() {
                        action = Some(Action::ReadAll);
                    }
                });
            });
            ui.separator();
            if notices.is_empty() {
                ui.label(RichText::new(words.empty).color(c.dim));
                return;
            }
            // The rows' own height, not what an Area leaves of its last
            // frame's size: a list that gained an entry stayed at 1.5 rows.
            let rows = notices.len() as f32 * (ROW_H + ui.spacing().item_spacing.y);
            egui::ScrollArea::vertical()
                .max_height(rows.min(MAX_ROWS_H))
                .min_scrolled_height(rows.min(MAX_ROWS_H))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (row, n) in notices.iter().rev().enumerate() {
                        let text_color = if n.read { c.dim } else { c.fg };
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                        first = first.or(Some(resp.id));
                        let p = ui.painter_at(rect);
                        let keyed = sel == Some(row);
                        if keyed && moved {
                            resp.scroll_to_me(None);
                        }
                        if resp.hovered() || keyed {
                            p.rect_filled(rect, 6.0, c.selection().gamma_multiply(0.6));
                        }
                        let color = n.kind.color();
                        let dot = rect.left_top() + vec2(10.0, 14.0);
                        if n.read {
                            p.circle_stroke(dot, 4.0, Stroke::new(1.2, color));
                        } else {
                            p.circle_filled(dot, 4.0, color);
                        }
                        // The age first, then the word, and the title cut short
                        // in what is left: a long one ran over both.
                        let line = |text: String, size: f32, color: Color32, width: f32| {
                            let mut job = egui::text::LayoutJob::simple_singleline(text, FontId::proportional(size), color);
                            job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(10.0));
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                        let since = now.duration_since(n.at).unwrap_or_default();
                        let ago = line(words.ago.replace("{}", &age(since)), 11.0, c.dim, rect.width());
                        let room = rect.width() - 22.0 - 6.0 - ago.size().x - 10.0;
                        let word = line(format!(" · {}", words.kinds[n.kind.index()]), 13.0, text_color, room / 2.0);
                        let title = line(n.title.clone(), 13.0, text_color, room - word.size().x);
                        let head = rect.left_top() + vec2(22.0, 6.0);
                        let after = head + vec2(title.size().x, 0.0);
                        p.galley(egui::pos2(rect.right() - 6.0 - ago.size().x, rect.top() + 6.0), ago, c.dim);
                        p.galley(head, title, text_color);
                        p.galley(after, word, text_color);
                        if let Some(note) = n.note.as_ref().filter(|s| !s.is_empty()) {
                            let note = line(note.clone(), 11.5, c.dim, rect.width() - 22.0 - 6.0);
                            p.galley(rect.left_top() + vec2(22.0, 25.0), note, c.dim);
                        }
                        if resp.clicked() {
                            action = Some(Action::Open(n.go.clone(), n.id));
                        }
                    }
                });
        });
    });
    keep_tab_inside(ctx, resp.response.layer_id, first);
    if resp.response.clicked_elsewhere() && action.is_none() {
        action = Some(Action::Close);
    }
    action
}

/// Keeps the keys inside the list: when they are on nothing, or on a control
/// behind it, they go to `first`.
fn keep_tab_inside(ctx: &egui::Context, layer: egui::LayerId, first: Option<Id>) {
    let focused = ctx.memory(|m| m.focused());
    let inside = focused.and_then(|id| ctx.read_response(id)).is_some_and(|r| r.layer_id == layer);
    if let (false, Some(id)) = (inside, first) {
        ctx.memory_mut(|m| m.request_focus(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> Words<'static> {
        Words {
            title: "Notices",
            mark_read: "Mark all read",
            empty: "None",
            ago: "{} ago",
            kinds: ["done", "info", "warn", "failed", "waiting"],
        }
    }

    fn notice(id: u64, kind: Kind, read: bool, go: Option<&'static str>) -> Notice<&'static str> {
        Notice { id, at: SystemTime::now(), kind, title: format!("n{id}"), note: Some("note".into()), go, read }
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    /// The list over frames, each with its own events; what the last answered.
    fn run(notices: &[Notice<&'static str>], frames: Vec<Vec<egui::Event>>) -> Option<Action<&'static str>> {
        let ctx = egui::Context::default();
        let mut out = None;
        for events in frames {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut full = ctx.run_ui(input, |ui| {
                let anchor = Rect::from_min_size(egui::pos2(700.0, 8.0), vec2(28.0, 24.0));
                out = show(ui.ctx(), anchor, notices, &words());
            });
            full.textures_delta.clear();
        }
        out
    }

    #[test]
    fn the_badge_counts_unread_and_goes_red_for_an_unread_error() {
        let list = [notice(1, Kind::Err, true, None), notice(2, Kind::Ok, false, None)];
        assert_eq!(unread(&list), 1);
        assert!(!any_error(&list));
        let list = [notice(1, Kind::Err, false, None), notice(2, Kind::Ok, false, None)];
        assert!(any_error(&list));
    }

    #[test]
    fn the_age_is_short() {
        assert_eq!(age(Duration::from_secs(5)), "5s");
        assert_eq!(age(Duration::from_secs(180)), "3m");
        assert_eq!(age(Duration::from_secs(7300)), "2h");
        assert_eq!(age(Duration::from_secs(200_000)), "2d");
    }

    #[test]
    fn the_arrows_and_enter_open_the_newest_first() {
        let list = [notice(1, Kind::Ok, false, Some("old")), notice(2, Kind::Wait, false, Some("new"))];
        assert_eq!(run(&list, vec![vec![key(egui::Key::ArrowDown), key(egui::Key::Enter)]]), Some(Action::Open(Some("new"), 2)));
        assert_eq!(
            run(&list, vec![vec![key(egui::Key::ArrowDown)], vec![key(egui::Key::ArrowDown)], vec![key(egui::Key::Enter)]]),
            Some(Action::Open(Some("old"), 1))
        );
    }

    #[test]
    fn a_notice_without_a_place_opens_to_nothing() {
        let list = [notice(7, Kind::Info, false, None)];
        assert_eq!(run(&list, vec![vec![key(egui::Key::ArrowDown), key(egui::Key::Enter)]]), Some(Action::Open(None, 7)));
    }

    #[test]
    fn escape_closes_and_enter_before_a_key_does_nothing() {
        let list = [notice(1, Kind::Ok, false, Some("x"))];
        assert_eq!(run(&list, vec![vec![key(egui::Key::Escape)]]), Some(Action::Close));
        assert_eq!(run(&list, vec![vec![key(egui::Key::Enter)]]), None);
        assert_eq!(run(&[], vec![vec![key(egui::Key::Enter)]]), None);
    }
}
