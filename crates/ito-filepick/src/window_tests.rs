//! Window tests: headless egui frames with fake input.

use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use ito_a11y::TestIdEntry;

use super::*;
use crate::window::Look;

/// The announcement log is process-wide, so the tests that count what was said take turns.
static ANNOUNCING: std::sync::Mutex<()> = std::sync::Mutex::new(());

type Extra = Box<dyn FnMut(&Context)>;

struct H {
    ctx: Context,
    p: Picker,
    words: Words,
    clock: ito_common::Clock,
    time: f64,
    wants: Vec<Want>,
    done: Option<Done>,
    ids: Vec<TestIdEntry>,
    extra: Option<Extra>,
    mods: Modifiers,
    sent_mods: Modifiers,
    screen: egui::Vec2,
    /// When the last frame asked to be drawn again.
    repaint: std::time::Duration,
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn plain(k: Key) -> Event {
    key(k, Modifiers::NONE)
}

impl H {
    fn new(p: Picker) -> H {
        Self::warm(p, 3)
    }

    fn warm(p: Picker, frames: usize) -> H {
        Self::warm_on(p, frames, egui::vec2(1000.0, 700.0), 1.0)
    }

    /// A screen of this size and this text zoom.
    fn sized(p: Picker, w: f32, h: f32, zoom: f32) -> H {
        Self::warm_on(p, 3, egui::vec2(w, h), zoom)
    }

    fn warm_on(p: Picker, frames: usize, screen: egui::Vec2, zoom: f32) -> H {
        let ctx = Context::default();
        ito_theme::set_text_zoom(&ctx, zoom);
        ctx.enable_accesskit();
        let mut h = H {
            ctx,
            p,
            words: Words::en(),
            clock: ito_common::Clock::default(),
            time: 1.0,
            wants: Vec::new(),
            done: None,
            ids: Vec::new(),
            extra: None,
            mods: Modifiers::NONE,
            sent_mods: Modifiers::NONE,
            screen,
            repaint: std::time::Duration::MAX,
        };
        // The first frames only learn the modal layer.
        for _ in 0..frames {
            h.frame(vec![]);
        }
        h
    }

    fn frame(&mut self, mut events: Vec<Event>) {
        // Only a change is an event; an event in every frame would itself ask for a repaint.
        if self.mods != self.sent_mods {
            events.insert(0, Event::ModifiersChanged(self.mods));
            self.sent_mods = self.mods;
        }
        self.time += 0.05;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.screen)),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let mut shown = Shown::default();
        let (words, clock) = (self.words.clone(), self.clock.clone());
        let p = &mut self.p;
        let extra = &mut self.extra;
        let mut out = self.ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            let look = Look { words: &words, clock: &clock, lang: "en" };
            shown = p.show(&ctx, &look);
            if let Some(f) = extra {
                f(&ctx);
            }
        });
        out.textures_delta.clear();
        self.repaint = out.viewport_output.get(&egui::ViewportId::ROOT).map_or(std::time::Duration::MAX, |v| v.repaint_delay);
        if let Some(w) = shown.want {
            self.wants.push(w);
        }
        if shown.done.is_some() {
            self.done = shown.done;
        }
        self.ids = ito_a11y::take_test_ids(&self.ctx, egui::ViewportId::ROOT);
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    fn rect(&self, name: &str) -> Rect {
        let full = format!("t.{name}");
        self.ids.iter().find(|e| e.name == full).unwrap_or_else(|| panic!("no test id {full}: {:?}", self.names())).rect
    }

    /// A test ID as the app that owns the widget wrote it (ito-prefs names the filter select itself).
    fn rect_raw(&self, full: &str) -> Rect {
        self.ids.iter().find(|e| e.name == full).unwrap_or_else(|| panic!("no test id {full}: {:?}", self.names())).rect
    }

    fn has(&self, name: &str) -> bool {
        let full = format!("t.{name}");
        self.ids.iter().any(|e| e.name == full)
    }

    fn names(&self) -> Vec<&str> {
        self.ids.iter().map(|e| e.name.as_str()).collect()
    }

    fn row_count(&self) -> usize {
        self.ids.iter().filter(|e| e.name.starts_with("t.row.")).count()
    }

    /// Answer the newest want.
    fn answer(&mut self, entries: Vec<Entry>) {
        self.answer_with(Ok(Listing::new(entries)));
    }

    fn answer_with(&mut self, l: Result<Listing, String>) {
        let Some(Want::List { ticket, .. }) = self.wants.pop() else { panic!("no want") };
        assert!(self.p.set_listing(ticket, l));
        self.frames(3);
    }

    fn press(&mut self, pos: Pos2, mods: Modifiers) {
        self.mods = mods;
        self.frame(vec![Event::PointerMoved(pos)]);
        self.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: mods }]);
        self.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: mods }]);
        self.frames(2);
        self.mods = Modifiers::NONE;
    }

    fn click(&mut self, name: &str) {
        let r = self.rect(name);
        self.press(r.center(), Modifiers::NONE);
    }

    fn click_with(&mut self, name: &str, mods: Modifiers) {
        let r = self.rect(name);
        self.press(r.center(), mods);
    }

    fn key(&mut self, k: Key) {
        self.frame(vec![plain(k)]);
        self.frames(2);
    }

    fn text(&mut self, t: &str) {
        self.frame(vec![Event::Text(t.to_owned())]);
        self.frames(2);
    }

    fn last_want(&self) -> Option<&str> {
        match self.wants.last() {
            Some(Want::List { path, .. }) => Some(path),
            _ => None,
        }
    }
}

fn posix(mode: Mode) -> Picker {
    Picker::new("t", mode, Style::Posix).start("/home/u")
}

fn sample() -> Vec<Entry> {
    vec![
        Entry::dir("sub").modified(1_700_000_000),
        Entry::file("a.txt").size(10).modified(1_700_000_000),
        Entry::file("b.txt").size(2048),
        Entry::file("c.md").size(3),
    ]
}

fn ready(mode: Mode) -> H {
    let mut h = H::new(posix(mode));
    assert_eq!(h.last_want(), Some("/home/u"));
    h.answer(sample());
    h
}

fn focus_list(h: &mut H) {
    h.click("row.0");
}

#[test]
fn the_first_show_wants_the_start_folder() {
    let h = H::new(posix(Mode::OpenFile));
    assert_eq!(h.wants.len(), 1);
    assert_eq!(h.last_want(), Some("/home/u"));
}

#[test]
fn a_listing_paints_row_ids() {
    let h = ready(Mode::OpenFile);
    for n in 0..4 {
        assert!(h.has(&format!("row.{n}")), "{:?}", h.names());
    }
    assert!(!h.has("row.4"));
    for id in ["back", "forward", "up", "refresh", "name", "ok", "cancel", "close", "list", "sort.name", "path", "hidden"] {
        assert!(h.has(id), "missing {id}: {:?}", h.names());
    }
}

#[test]
fn down_down_enter_on_a_folder_wants_it() {
    let mut h = ready(Mode::OpenFile);
    h.key(Key::ArrowDown); // from the name box to the list
    h.key(Key::ArrowDown);
    h.key(Key::Enter);
    assert_eq!(h.last_want(), Some("/home/u/sub"), "{:?}", h.names());
    assert!(h.done.is_none());
}

#[test]
fn enter_on_a_file_picks_it() {
    let mut h = ready(Mode::OpenFile);
    focus_list(&mut h);
    h.key(Key::ArrowDown);
    h.key(Key::Enter);
    assert_eq!(h.done, Some(Done::Picked(vec!["/home/u/a.txt".to_owned()])));
}

#[test]
fn escape_cancels() {
    let mut h = ready(Mode::OpenFile);
    h.key(Key::Escape);
    assert_eq!(h.done, Some(Done::Cancelled));
}

#[test]
fn escape_in_the_path_edit_only_puts_the_crumbs_back() {
    let mut h = ready(Mode::OpenFile);
    h.click("edit");
    assert!(h.p.ui.editing);
    h.key(Key::Escape);
    assert!(!h.p.ui.editing);
    assert!(h.done.is_none());
    h.key(Key::Escape);
    assert_eq!(h.done, Some(Done::Cancelled));
}

#[test]
fn backspace_goes_up_in_the_list_but_edits_the_name() {
    let mut h = ready(Mode::OpenFile);
    h.text("abc");
    assert_eq!(h.p.name(), "abc");
    h.key(Key::Backspace);
    assert_eq!(h.p.name(), "ab");
    assert_eq!(h.wants.len(), 0);
    focus_list(&mut h);
    h.key(Key::Backspace);
    assert_eq!(h.last_want(), Some("/home"));
}

#[test]
fn a_backdrop_click_does_not_cancel() {
    let mut h = ready(Mode::OpenFile);
    h.press(Pos2::new(3.0, 3.0), Modifiers::NONE);
    assert!(h.done.is_none());
}

#[test]
fn a_double_click_opens_a_row() {
    let mut h = ready(Mode::OpenFile);
    let pos = h.rect("row.1").center();
    h.frame(vec![Event::PointerMoved(pos)]);
    for _ in 0..2 {
        h.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        h.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    }
    h.frames(2);
    assert_eq!(h.done, Some(Done::Picked(vec!["/home/u/a.txt".to_owned()])));
}

#[test]
fn ctrl_click_two_files_then_open() {
    let mut h = ready(Mode::OpenFiles);
    h.click_with("row.1", Modifiers::NONE);
    h.click_with("row.3", Modifiers::COMMAND);
    assert_eq!(h.p.selected_count(), 2);
    h.click("ok");
    assert_eq!(h.done, Some(Done::Picked(vec!["/home/u/a.txt".to_owned(), "/home/u/c.md".to_owned()])));
}

#[test]
fn a_typed_relative_path_wants_the_parent_then_picks_on_the_answer() {
    let mut h = ready(Mode::OpenFile);
    h.text("../x/y.txt");
    h.key(Key::Enter);
    assert_eq!(h.last_want(), Some("/home/x"), "{:?}", h.wants);
    h.answer(vec![Entry::file("y.txt")]);
    assert_eq!(h.done, Some(Done::Picked(vec!["/home/x/y.txt".to_owned()])));
}

#[test]
fn a_header_click_flips_the_sort() {
    let mut h = ready(Mode::OpenFile);
    assert_eq!(h.p.sort_state(), (Sort::Name, false));
    h.click("sort.name");
    assert_eq!(h.p.sort_state(), (Sort::Name, true));
    h.click("sort.size");
    assert_eq!(h.p.sort_state().0, Sort::Size);
}

#[test]
fn loading_shows_only_after_150_ms() {
    let mut h = H::warm(posix(Mode::OpenFile), 1);
    assert!(!h.p.ui.loading_visible);
    h.frame(vec![]);
    assert!(!h.p.ui.loading_visible);
    for _ in 0..6 {
        h.frame(vec![]);
    }
    assert!(h.p.ui.loading_visible);
    h.answer(sample());
    assert!(!h.p.ui.loading_visible);
}

#[test]
fn retry_asks_again() {
    let mut h = H::new(posix(Mode::OpenFile));
    h.answer_with(Err("boom".to_owned()));
    assert!(h.has("retry"), "{:?}", h.names());
    h.click("retry");
    assert_eq!(h.last_want(), Some("/home/u"));
}

#[test]
fn a_huge_listing_names_only_the_rows_in_sight_and_end_reaches_the_last() {
    let mut h = H::new(posix(Mode::OpenFile));
    h.answer((0..100_000).map(|i| Entry::file(&format!("file{i:06}.txt"))).collect());
    assert!(h.row_count() > 5 && h.row_count() < 60, "{}", h.row_count());
    focus_list(&mut h);
    h.key(Key::End);
    h.frames(3);
    assert!(h.has("row.99999"), "{:?}", h.names().into_iter().filter(|n| n.contains("row")).take(5).collect::<Vec<_>>());
    assert!(h.row_count() < 60);
}

#[test]
fn a_foreground_overlay_with_a_focused_text_edit_keeps_escape_and_enter() {
    let mut h = ready(Mode::OpenFile);
    let mut text = String::new();
    let edit = egui::Id::new("overlay-edit");
    h.extra = Some(Box::new(move |ctx| {
        egui::Area::new(egui::Id::new("overlay")).order(egui::Order::Foreground).fixed_pos(Pos2::new(10.0, 10.0)).show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(edit));
        });
    }));
    h.frames(2);
    h.ctx.memory_mut(|m| m.request_focus(edit));
    h.frames(2);
    h.key(Key::Escape);
    assert!(h.done.is_none());
    h.key(Key::Enter);
    assert!(h.done.is_none());
}

#[test]
fn an_open_popup_keeps_escape() {
    let mut h = ready(Mode::OpenFile);
    egui::Popup::open_id(&h.ctx, egui::Id::new("some-popup"));
    h.frames(1);
    assert!(egui::Popup::is_any_open(&h.ctx));
    h.frame(vec![plain(Key::Escape)]);
    assert!(h.done.is_none());
}

#[test]
fn enter_with_nothing_focused_is_the_default_button() {
    let mut h = ready(Mode::OpenFile);
    h.p.set_name("a.txt");
    h.ctx.memory_mut(|m| m.stop_text_input());
    h.frames(2);
    h.key(Key::Enter);
    assert_eq!(h.done, Some(Done::Picked(vec!["/home/u/a.txt".to_owned()])));
}

// ---- Fixes for the review (#228) ----

fn shift(k: Key) -> Event {
    key(k, Modifiers::SHIFT)
}

fn with_filters(mode: Mode) -> Picker {
    Picker::new("t", mode, Style::Posix).filters(vec![Filter::new("Text", &["txt"])]).start("/home/u")
}

#[test]
fn shift_home_end_page_keys_extend_the_selection_in_open_files() {
    let mut h = ready(Mode::OpenFiles);
    h.click("row.2");
    assert_eq!(h.p.selected_count(), 1);
    h.frame(vec![shift(Key::End)]);
    h.frames(2);
    assert_eq!(h.p.selected_count(), 2, "b.txt and c.md");
    h.frame(vec![shift(Key::Home)]);
    h.frames(2);
    assert_eq!(h.p.selected_count(), 2, "a.txt and b.txt (the folder is not pickable)");
    h.frame(vec![shift(Key::PageDown)]);
    h.frames(2);
    assert_eq!(h.p.selected_count(), 2);
    h.frame(vec![shift(Key::PageUp)]);
    h.frames(2);
    assert_eq!(h.p.selected_count(), 2);
}

#[test]
fn shift_end_in_a_single_mode_moves_the_cursor_like_the_plain_key() {
    let mut h = ready(Mode::OpenFile);
    h.click("row.1");
    h.frame(vec![shift(Key::End)]);
    h.frames(2);
    assert_eq!(h.p.selected_count(), 1);
    assert_eq!(h.p.name(), "c.md");
}

#[test]
fn the_deep_path_keeps_the_root_and_the_last_crumb_and_one_ellipsis() {
    let deep = "/aaaaaaaaaa/bbbbbbbbbb/cccccccccc/dddddddddd/eeeeeeeeee/ffffffffff/gggggggggg";
    let mut h = H::sized(Picker::new("t", Mode::OpenFile, Style::Posix).start(deep), 420.0, 600.0, 1.0);
    h.answer(sample());
    assert!(h.has("path.0"), "{:?}", h.names());
    assert!(h.has("path.7"), "{:?}", h.names());
    assert!(!h.has("path.1") && !h.has("path.3"), "the middle gives way: {:?}", h.names());
    // The ellipsis follows the root (one separator between); it has no index ID. Clicking it starts the text edit.
    let root = h.rect("path.0");
    let ell = Pos2::new(root.max.x + 14.0 + 8.0, root.center().y);
    assert!(!h.p.ui.editing);
    h.press(ell, Modifiers::NONE);
    assert!(h.p.ui.editing, "the click on the ellipsis starts the edit");
    assert_eq!(h.p.target(), deep, "it does not navigate");
}

#[test]
fn a_short_path_shows_every_crumb() {
    let h = ready(Mode::OpenFile);
    for n in 0..3 {
        assert!(h.has(&format!("path.{n}")), "{:?}", h.names());
    }
}

#[test]
fn the_places_column_is_180_wide() {
    let mut h = H::new(
        Picker::new("t", Mode::OpenFile, Style::Posix).places(vec![Place::new("Home", "/home/u", PlaceKind::Home)]).start("/home/u/x"),
    );
    h.answer(sample());
    let (place, list) = (h.rect("place.0"), h.rect("list"));
    // The column starts 4 inside its left edge and the list 8 after its right edge.
    assert!((list.min.x - place.min.x - (180.0 + 8.0 - 4.0)).abs() < 0.5, "{place:?} {list:?}");
}

#[test]
fn a_note_is_announced_once_not_every_frame() {
    let _one = ANNOUNCING.lock().unwrap_or_else(|e| e.into_inner());
    ito_a11y::record_announcements(true);
    let mut h = ready(Mode::OpenFile);
    h.p.refresh();
    h.frames(2);
    let before = ito_a11y::announced_count();
    h.answer_with(Ok(Listing::new(sample()).truncated(true)));
    h.frames(12);
    let note = Words::en().first_n(h.p.entry_count());
    let heard = ito_a11y::announced_after(before).iter().filter(|m| **m == note).count();
    assert_eq!(heard, 1, "{:?}", ito_a11y::announced_after(before));
}

#[test]
fn escape_finishes_the_dialog_and_draws_nothing_that_frame() {
    let mut h = ready(Mode::OpenFile);
    assert!(!h.ids.is_empty());
    h.frame(vec![plain(Key::Escape)]);
    assert_eq!(h.done, Some(Done::Cancelled));
    assert!(h.ids.is_empty(), "nothing drawn after Done: {:?}", h.names());
}

#[cfg(target_os = "macos")]
#[test]
fn cmd_r_refreshes_on_macos() {
    let mut h = ready(Mode::OpenFile);
    h.wants.clear();
    h.frame(vec![key(Key::R, Modifiers::MAC_CMD)]);
    assert!(h.last_want().is_some());
}

fn inside(h: &H, name: &str) -> bool {
    let screen = Rect::from_min_size(Pos2::ZERO, h.screen);
    let r = h.rect(name);
    screen.contains_rect(r)
}

#[test]
fn a_small_screen_gets_a_dialog_that_fits_it() {
    let mut h = H::sized(posix(Mode::OpenFile), 250.0, 250.0, 1.0);
    h.answer(sample());
    for name in ["close", "ok", "cancel", "name"] {
        assert!(inside(&h, name), "{name}: {:?} not in the screen", h.rect(name));
    }
    // A screen under 32 px must not panic either.
    let mut tiny = H::sized(posix(Mode::OpenFile), 20.0, 20.0, 1.0);
    tiny.answer(sample());
    tiny.frames(2);
}

#[test]
fn the_layout_follows_the_text_zoom() {
    let mut one = H::sized(posix(Mode::OpenFile), 1600.0, 1100.0, 1.0);
    one.answer(sample());
    let mut big = H::sized(posix(Mode::OpenFile), 1600.0, 1100.0, 1.5);
    big.answer(sample());
    for name in ["name", "ok", "row.0", "close", "path", "hidden"] {
        let (a, b) = (one.rect(name).height(), big.rect(name).height());
        assert!((b - a * 1.5).abs() <= 1.0, "{name}: {a} at 1.0 and {b} at 1.5");
    }
    let (a, b) = (one.rect("list").min.y - one.rect("path").min.y, big.rect("list").min.y - big.rect("path").min.y);
    assert!(b > a, "the list starts lower at 1.5: {a} {b}");
}

#[test]
fn the_footer_buttons_fill_their_slots() {
    let h = ready(Mode::OpenFile);
    let (ok, cancel, list) = (h.rect("ok"), h.rect("cancel"), h.rect("list"));
    assert!((cancel.max.x - list.max.x).abs() < 0.5, "cancel ends at the footer's right edge: {cancel:?} {list:?}");
    assert!((cancel.min.x - ok.max.x - 8.0).abs() < 0.5, "8 apart: {ok:?} {cancel:?}");
    assert!(ok.width() >= 76.0 - 0.5 && cancel.width() >= 76.0 - 0.5, "{ok:?} {cancel:?}");
    assert!(ok.min.x < cancel.min.x, "Open is left of Cancel");
}

#[test]
fn the_filter_ends_at_the_footer_edge_beside_the_name_box() {
    let mut h = H::new(with_filters(Mode::OpenFile));
    h.answer(sample());
    let (filter, list, name) = (h.rect_raw("settings.select.t_filter"), h.rect("list"), h.rect("name"));
    assert!((filter.max.x - list.max.x).abs() < 0.5, "{filter:?} {list:?}");
    assert!((filter.center().y - name.center().y).abs() <= 1.0, "centred on the name box: {filter:?} {name:?}");
    assert!(name.max.x < filter.min.x, "side by side");
}

#[test]
fn a_narrow_window_keeps_the_name_box_wider_than_the_filter() {
    let mut h = H::sized(with_filters(Mode::OpenFile), 420.0, 600.0, 1.0);
    h.answer(sample());
    let (filter, name) = (h.rect_raw("settings.select.t_filter"), h.rect("name"));
    assert!(name.width() > filter.width(), "{name:?} {filter:?}");
    assert!(inside(&h, "ok") && inside(&h, "cancel"));
    assert!(h.rect("hidden").min.y > filter.max.y, "the footer rows do not overlap");
}

#[test]
fn the_folder_mode_has_no_filter() {
    let mut h = H::new(with_filters(Mode::OpenFolder));
    h.answer(sample());
    assert!(!h.ids.iter().any(|e| e.name == "settings.select.t_filter"), "{:?}", h.names());
}

#[test]
fn nothing_asks_for_a_repaint_while_the_list_is_showing() {
    let mut h = ready(Mode::OpenFile);
    h.click("row.1");
    h.frames(40);
    assert!(h.repaint > std::time::Duration::from_secs(1), "{:?}", h.repaint);
}

#[test]
fn loading_repaints_for_the_spinner_only_while_it_shows() {
    let mut h = H::new(posix(Mode::OpenFile));
    // Past 150 ms the text and the spinner show.
    h.frames(6);
    assert!(h.p.ui.loading_visible);
    assert!(h.repaint < std::time::Duration::from_secs(1), "{:?}", h.repaint);
}

fn windows_picker() -> Picker {
    Picker::new("t", Mode::OpenFile, Style::Windows).start("C:\\Users")
}

#[test]
fn a_bad_typed_path_keeps_the_edit_and_the_draft_with_the_error_under_the_bar() {
    let _one = ANNOUNCING.lock().unwrap_or_else(|e| e.into_inner());
    ito_a11y::record_announcements(true);
    let mut h = H::new(windows_picker());
    h.answer(sample());
    h.click("edit");
    assert!(h.p.ui.editing);
    let before = ito_a11y::announced_count();
    h.text("C:foo");
    h.key(Key::Enter);
    h.frames(4);
    assert!(h.p.ui.editing, "the edit stays open");
    assert_eq!(h.p.ui.path_text, "C:foo");
    assert!(h.p.path_error().is_some());
    let (bar, err, list) = (h.rect("path"), h.rect("path.error"), h.rect("list"));
    assert!(err.min.y >= bar.max.y - 0.5 && err.max.y <= list.min.y + 0.5, "{bar:?} {err:?} {list:?}");
    let said = Words::en().path_error(h.p.path_error().unwrap());
    assert_eq!(ito_a11y::announced_after(before).iter().filter(|m| **m == said).count(), 1, "{:?}", ito_a11y::announced_after(before));
    // The caret is at the end of the draft and the focus is in the field.
    let id = h.p.ui.path_id.unwrap();
    assert!(h.ctx.memory(|m| m.has_focus(id)));
    let st = egui::TextEdit::load_state(&h.ctx, id).unwrap();
    assert_eq!(st.cursor.char_range().map(|r| r.primary.index.0), Some(5));
    // The footer note no longer carries it.
    h.text("x");
    assert!(h.p.path_error().is_none(), "an edit clears it");
    assert!(!h.has("path.error"));
}

#[test]
fn escape_clears_a_path_error_and_leaves_the_edit_without_closing_the_dialog() {
    let _one = ANNOUNCING.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new(windows_picker());
    h.answer(sample());
    h.click("edit");
    h.text("C:foo");
    h.key(Key::Enter);
    h.frames(3);
    assert!(h.p.path_error().is_some());
    h.key(Key::Escape);
    assert!(!h.p.ui.editing);
    assert!(h.p.path_error().is_none());
    assert!(h.done.is_none());
}

#[test]
fn a_click_away_from_a_bad_typed_path_drops_the_draft_and_its_error() {
    let _one = ANNOUNCING.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new(windows_picker());
    h.answer(sample());
    h.click("edit");
    h.text("C:foo");
    h.key(Key::Enter);
    h.frames(3);
    assert!(h.p.path_error().is_some());
    h.click("list");
    h.frames(2);
    assert!(!h.p.ui.editing);
    assert!(h.p.path_error().is_none());
    assert!(!h.has("path.error"));
    assert!(h.done.is_none());
}

#[test]
fn a_good_typed_path_after_a_bad_one_goes_there() {
    let _one = ANNOUNCING.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = H::new(windows_picker());
    h.answer(sample());
    h.click("edit");
    h.text("C:foo");
    h.key(Key::Enter);
    h.frames(2);
    assert!(h.p.ui.editing);
    // Select all and type a real path.
    h.frame(vec![key(Key::A, Modifiers::COMMAND)]);
    h.text("C:\\Temp");
    h.key(Key::Enter);
    assert!(!h.p.ui.editing);
    assert_eq!(h.last_want(), Some("C:\\Temp"));
}

#[test]
fn a_ctrl_click_whose_modifier_rides_only_on_the_event_still_toggles() {
    let mut h = ready(Mode::OpenFiles);
    h.click_with("row.1", Modifiers::NONE);
    // Press and release in separate frames; the frame itself reports no modifier (a script does this).
    let pos = h.rect("row.3").center();
    h.frame(vec![Event::PointerMoved(pos)]);
    h.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::COMMAND }]);
    h.frame(vec![Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::COMMAND }]);
    h.frames(2);
    assert_eq!(h.p.selected_count(), 2);
}

#[test]
fn a_ctrl_click_with_press_and_release_in_one_frame_toggles() {
    let mut h = ready(Mode::OpenFiles);
    h.click_with("row.1", Modifiers::NONE);
    let pos = h.rect("row.3").center();
    h.frame(vec![Event::PointerMoved(pos)]);
    h.frame(vec![
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::COMMAND },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::COMMAND },
    ]);
    h.frames(2);
    assert_eq!(h.p.selected_count(), 2);
}

#[test]
fn the_filter_popup_takes_down_enter_and_escape() {
    let mut h = H::new(
        Picker::new("t", Mode::OpenFile, Style::Posix)
            .filters(vec![Filter::new("Text", &["txt"]), Filter::new("Markdown", &["md"])])
            .filter_index(0)
            .start("/home/u"),
    );
    h.answer(sample());
    // Tab to the filter (Space opens it; Alt+Down does not on a select), then move with the arrow and pick with Enter.
    let mut opened = false;
    for _ in 0..30 {
        h.key(Key::Tab);
        h.key(Key::Space);
        if egui::Popup::is_any_open(&h.ctx) {
            opened = true;
            break;
        }
    }
    assert!(opened, "Tab never reached a select that opens");
    assert!(h.done.is_none());
    h.key(Key::ArrowDown);
    h.key(Key::Enter);
    assert!(!egui::Popup::is_any_open(&h.ctx), "Enter closed the popup");
    assert_eq!(h.p.filter_position(), 1, "the filter changed");
    assert!(h.done.is_none());
    // Escape closes only the popup.
    h.key(Key::Space);
    assert!(egui::Popup::is_any_open(&h.ctx), "the popup opens again from the select's focus");
    h.key(Key::Escape);
    assert!(!egui::Popup::is_any_open(&h.ctx));
    assert!(h.done.is_none(), "the dialog stays");
}
