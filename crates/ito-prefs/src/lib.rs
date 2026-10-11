//! uchmk's settings screen, as every app draws it (`docs/common-spec.md` in ito):
//! a nav down the left -- the search, the pages, a button for the file --
//! and the page on the right, its title and lead over cards of rows, each
//! row its label and note on the left and its control on the right.
//!
//! An app hands [`show`] its pages and draws each page's cards in the
//! closure, with [`section`], [`row`] and the controls here. The pages every
//! app shares -- the language, the clock, the theme -- are here too
//! ([`language_row`], [`clock_card`], [`theme_page`]); they hand back
//! [`CommonChange`]s, which go into common.toml
//! ([`ito_common::edit_common`]), so one change reaches every app.

use std::collections::HashMap;

use egui::{Color32, FontId, RichText};
use ito_common::{quote, scale_change, Clock, CommonChange, DATE_FORMATS};
use ito_theme::{Colors, Theme};

/// The words the screen and the shared pages say, in one language.
pub struct Words {
    pub search: &'static str,
    pub nothing: &'static str,
    /// Before the file's name on the button at the foot of the nav.
    pub open: &'static str,
    pub language: &'static str,
    pub auto: &'static str,
    pub scale: &'static str,
    pub clock: &'static str,
    pub show_time: &'static str,
    pub show_time_note: &'static str,
    pub time_format: &'static str,
    pub hour24: &'static str,
    pub hour12: &'static str,
    pub show_date: &'static str,
    pub show_date_note: &'static str,
    pub date_format: &'static str,
    pub weekday: &'static str,
    pub mode: &'static str,
    pub follow_os: &'static str,
    pub light: &'static str,
    pub dark: &'static str,
    pub themes: &'static str,
    pub when_light: &'static str,
    pub when_dark: &'static str,
    pub preview: &'static str,
}

pub const EN: Words = Words {
    search: "🔍  Search settings",
    nothing: "Nothing matches",
    open: "Open",
    language: "Language",
    auto: "Auto (system)",
    scale: "Scale",
    clock: "CLOCK",
    show_time: "Show the time in the status bar",
    show_time_note: "Stays visible when the window is maximized or full screen",
    time_format: "Time format",
    hour24: "14:32 (24-hour)",
    hour12: "2:32 PM (12-hour)",
    show_date: "Show the date",
    show_date_note: "Next to the time",
    date_format: "Date format",
    weekday: "Show the weekday",
    mode: "Mode",
    follow_os: "Follow OS",
    light: "Light",
    dark: "Dark",
    themes: "THEME",
    when_light: "when light",
    when_dark: "when dark",
    preview: "PREVIEW",
};

pub const JA: Words = Words {
    search: "🔍  設定を検索",
    nothing: "当てはまる設定はありません",
    open: "開く:",
    language: "言語",
    auto: "自動（OS に従う）",
    scale: "倍率",
    clock: "時計",
    show_time: "ステータスバーに時刻を出す",
    show_time_note: "窓を最大化・全画面にしても出たまま",
    time_format: "時刻の形",
    hour24: "14:32（24 時間）",
    hour12: "2:32 PM（12 時間）",
    show_date: "日付を出す",
    show_date_note: "時刻の隣に",
    date_format: "日付の形",
    weekday: "曜日を出す",
    mode: "モード",
    follow_os: "OS に従う",
    light: "ライト",
    dark: "ダーク",
    themes: "テーマ",
    when_light: "ライトのとき",
    when_dark: "ダークのとき",
    preview: "見本",
};

impl Words {
    /// The words for a language code (`ja`, `en`); English for the rest.
    pub fn of(language: &str) -> &'static Words {
        match language {
            "ja" => &JA,
            _ => &EN,
        }
    }
}

/// The screen's own state: the page, the search, and whether a control had
/// the keys at the end of the last frame.
#[derive(Default)]
pub struct State {
    /// The page, as an index into [`Nav::pages`].
    pub page: usize,
    /// What the search field above the pages holds.
    pub query: String,
    /// A control had the keys at the end of the last frame: an Esc then
    /// leaves it (egui lets go of it before this frame is drawn), and only
    /// the next Esc leaves the screen.
    pub held: bool,
}

impl State {
    /// A control has the keys, or had them a moment ago: Esc is its.
    pub fn busy(&self, ui: &egui::Ui) -> bool {
        self.held || ui.memory(|m| m.focused().is_some())
    }
}

/// The screen's keys this frame, read by the app (from its keymap, or with
/// [`Keys::standard`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub close: bool,
    pub next: bool,
    pub back: bool,
    pub find: bool,
}

impl Keys {
    /// The keys as tsumugi has them, taken from egui's input: Esc closes
    /// unless `busy`; Ctrl+Tab and Ctrl+PageDown go to the next page, with
    /// Shift or PageUp to the one before, as tabs do elsewhere; Ctrl+F (Cmd+F
    /// on macOS) goes to the search.
    pub fn standard(ui: &egui::Ui, busy: bool) -> Self {
        let close = !busy && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        ui.input_mut(|i| {
            let ctrl = egui::Modifiers::CTRL;
            let back = i.consume_key(ctrl | egui::Modifiers::SHIFT, egui::Key::Tab) || i.consume_key(ctrl, egui::Key::PageUp);
            let next = i.consume_key(ctrl, egui::Key::Tab) || i.consume_key(ctrl, egui::Key::PageDown);
            Self { close, next, back, find: i.consume_key(egui::Modifiers::COMMAND, egui::Key::F) }
        })
    }
}

/// The pages: each one's title and lead, the rows on each (the page's
/// index, the row's words) for the search, the words and the file.
pub struct Nav<'a> {
    pub pages: &'a [(&'a str, &'a str)],
    pub index: &'a [(usize, &'a str)],
    pub words: &'a Words,
    /// The settings file's name, on the button at the foot.
    pub file: &'a str,
}

/// What the frame was asked for, besides what the page hands back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Out {
    pub close: bool,
    pub open_file: bool,
}

/// The colours and the search's words, for every row.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub c: Colors,
    /// The search, lower case; empty for none.
    pub q: &'a str,
}

/// The screen across all of `ui`: the nav and the page, which `body` draws
/// (it gets the page's index and the [`Look`]).
pub fn show(ui: &mut egui::Ui, c: Colors, state: &mut State, nav: &Nav, keys: Keys, body: impl FnOnce(&mut egui::Ui, usize, Look)) -> Out {
    let mut out = Out { close: keys.close, open_file: false };
    let n = nav.pages.len().max(1);
    state.page = state.page.min(n - 1);
    if keys.next || keys.back {
        let k = state.page;
        state.page = if keys.next { (k + 1) % n } else { (k + n - 1) % n };
        state.query.clear();
        // The keys to the page's first control with the next Tab.
        ui.memory_mut(|m| m.surrender_focus(m.focused().unwrap_or(egui::Id::NULL)));
    }
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, c.bg);
    // The nav down the left: the search, the pages, and the file.
    let side = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + 240.0, rect.bottom()));
    ui.painter().rect_filled(side, 0.0, c.side);
    ui.painter().line_segment([side.right_top(), side.right_bottom()], egui::Stroke::new(1.0, c.border));
    let mut nav_ui = ui.new_child(egui::UiBuilder::new().max_rect(side.shrink2(egui::vec2(10.0, 16.0))));
    let search = egui::TextEdit::singleline(&mut state.query)
        .id(egui::Id::new("prefs-search"))
        .hint_text(nav.words.search)
        .desired_width(f32::INFINITY)
        .margin(egui::vec2(10.0, 8.0))
        .font(FontId::proportional(12.5));
    let searched = nav_ui.add(search);
    focus_ring(&nav_ui, &searched);
    if keys.find {
        searched.request_focus();
    }
    nav_ui.add_space(10.0);
    let q = state.query.trim().to_lowercase();
    let hits = |p: usize| -> usize {
        if q.is_empty() {
            return 1;
        }
        nav.index.iter().filter(|(at, words)| *at == p && words.to_lowercase().contains(&q)).count()
            + usize::from(nav.pages[p].0.to_lowercase().contains(&q))
    };
    let shown: Vec<usize> = (0..nav.pages.len()).filter(|p| hits(*p) > 0).collect();
    // The search moves to the first page that has it.
    if !q.is_empty() && !shown.contains(&state.page) {
        if let Some(first) = shown.first() {
            state.page = *first;
        }
    }
    if searched.lost_focus() && nav_ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Some(first) = shown.first() {
            state.page = *first;
            // The keys go on to the first row the search lights.
            if !q.is_empty() {
                ui.data_mut(|d| d.insert_temp(egui::Id::new(JUMP), true));
            }
        }
    }
    if shown.is_empty() {
        nav_ui.label(RichText::new(nav.words.nothing).size(12.5).color(c.dim));
    }
    for p in shown {
        let on = state.page == p;
        // Mouse only: the keys go page to page with Ctrl+Tab, and Tab goes
        // from the search straight into the page.
        let (r, resp) = nav_ui.allocate_exact_size(egui::vec2(nav_ui.available_width(), 32.0), egui::Sense::CLICK);
        if on {
            nav_ui.painter().rect_filled(r, 6.0, c.chosen());
        } else if resp.hovered() {
            nav_ui.painter().rect_filled(r, 6.0, c.hover());
        }
        let color = if on { c.strong() } else { c.dim };
        nav_ui.painter().text(
            egui::pos2(r.left() + 10.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            nav.pages[p].0,
            FontId::proportional(13.0),
            color,
        );
        if !q.is_empty() {
            nav_ui.painter().text(
                egui::pos2(r.right() - 10.0, r.center().y),
                egui::Align2::RIGHT_CENTER,
                hits(p).to_string(),
                FontId::proportional(11.0),
                c.faint(),
            );
        }
        if resp.clicked() {
            state.page = p;
        }
    }

    // The page.
    let l = Look { c, q: &q };
    let area = egui::Rect::from_min_max(egui::pos2(side.right() + 1.0, rect.top()), rect.max);
    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
    let page = state.page;
    let (title, lead) = nav.pages.get(page).copied().unwrap_or(("", ""));
    egui::ScrollArea::vertical().id_salt(("prefs-page", title)).auto_shrink([false, false]).show(&mut body_ui, |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin { left: 48, right: 48, top: 28, bottom: 40 }).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(820.0));
            ui.label(RichText::new(title).size(24.0).strong().color(c.strong()));
            ui.add_space(2.0);
            ui.label(RichText::new(lead).size(13.5).color(c.dim));
            ui.add_space(16.0);
            body(ui, page, l);
        });
    });
    // No row lit took the keys: they stay where the search left them.
    ui.data_mut(|d| d.remove::<bool>(egui::Id::new(JUMP)));
    // The file last, so Tab comes to it after the page.
    let foot = egui::Rect::from_min_max(
        egui::pos2(side.left() + 10.0, side.bottom() - 48.0),
        egui::pos2(side.right() - 10.0, side.bottom() - 16.0),
    );
    let words = format!("{} {}", nav.words.open, nav.file);
    let file = egui::Button::new(RichText::new(words).size(12.5).color(c.dim))
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, c.border_strong()))
        .corner_radius(6.0);
    let file = nav_ui.put(foot, file);
    focus_ring(&nav_ui, &file);
    out.open_file = file.clicked();
    state.held = ui.memory(|m| m.focused().is_some());
    out
}

/// Enter in the search: the first row it lights takes the keys.
const JUMP: &str = "prefs-jump";
/// The row taking the keys: its control takes them.
const JUMP_HERE: &str = "prefs-jump-here";

/// The control `r` takes the keys, when the row it is in is to have them.
fn take_keys(ui: &egui::Ui, r: &egui::Response) {
    if ui.data_mut(|d| d.remove_temp::<bool>(egui::Id::new(JUMP_HERE))).is_some() {
        r.request_focus();
    }
}

/// A card of rows under a small heading.
pub fn section(ui: &mut egui::Ui, l: Look, head: &str, rows: impl FnOnce(&mut egui::Ui)) {
    let c = l.c;
    let hit = !l.q.is_empty() && head.to_lowercase().contains(l.q);
    ui.label(RichText::new(head).size(11.0).strong().color(if hit { c.wait } else { c.dim }));
    ui.add_space(6.0);
    egui::Frame::NONE
        .fill(c.panel)
        .stroke(egui::Stroke::new(1.0, c.border))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(18, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            rows(ui);
        });
    ui.add_space(22.0);
}

/// The line between two rows of a card, across the whole card.
pub fn sep(ui: &mut egui::Ui, l: Look) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(r.x_range().expand(18.0), r.center().y, egui::Stroke::new(1.0, l.c.border));
}

/// A row: its label and note on the left, its control on the right. Lit
/// when the search's words are in it.
pub fn row<R>(ui: &mut egui::Ui, l: Look, label: &str, note: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let c = l.c;
    let hit = !l.q.is_empty() && (label.to_lowercase().contains(l.q) || note.to_lowercase().contains(l.q));
    let behind = ui.painter().add(egui::Shape::Noop);
    let mut r = None;
    let shown = ui.horizontal(|ui| {
        ui.set_min_height(48.0);
        // The control first, on the right; the words wrap in what is left,
        // never under it.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let jump = hit && ui.data_mut(|d| d.remove_temp::<bool>(egui::Id::new(JUMP))).is_some();
            if jump {
                ui.data_mut(|d| d.insert_temp(egui::Id::new(JUMP_HERE), true));
            }
            r = Some(control(ui));
            if jump {
                ui.data_mut(|d| d.remove::<bool>(egui::Id::new(JUMP_HERE)));
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width());
                    // A label alone sits in the middle of the row, as the
                    // control does.
                    ui.add_space(if note.is_empty() { 15.0 } else { 8.0 });
                    ui.label(RichText::new(label).size(13.0).color(c.strong()));
                    if !note.is_empty() {
                        ui.label(RichText::new(note).size(12.0).color(c.dim));
                    }
                    ui.add_space(6.0);
                });
            });
        });
    });
    if hit {
        let lit = egui::Shape::rect_filled(shown.response.rect.expand2(egui::vec2(10.0, 0.0)), 6.0, c.wait.gamma_multiply(0.14));
        ui.painter().set(behind, lit);
    }
    r.expect("the control ran")
}

/// The design's switch: a pill with a knob, cyan when on.
pub fn switch(ui: &mut egui::Ui, l: Look, on: bool) -> bool {
    let c = l.c;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(40.0, 22.0), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 11.0, if on { c.run } else { c.border_strong() });
    let x = if on { rect.right() - 11.0 } else { rect.left() + 11.0 };
    p.circle_filled(egui::pos2(x, rect.center().y), 8.0, if on { c.on_accent() } else { c.dim });
    take_keys(ui, &resp);
    focus_ring(ui, &resp);
    resp.clicked()
}

/// A ring round the control that has the keys, so Tab shows where it is.
pub fn focus_ring(ui: &egui::Ui, r: &egui::Response) {
    if r.has_focus() {
        ui.painter().rect_stroke(r.rect.expand(3.0), 8.0, egui::Stroke::new(1.5, ito_theme::colors().run), egui::StrokeKind::Outside);
    }
}

/// A list to pick one of: the value picked, when it changed.
pub fn select<T: PartialEq + Copy>(ui: &mut egui::Ui, id: &str, now: T, options: &[(T, &str)]) -> Option<T> {
    let mut pick = now;
    let shown = options.iter().find(|(v, _)| *v == now).map_or("", |(_, t)| *t).to_owned();
    let combo = egui::ComboBox::from_id_salt(("prefs", id)).selected_text(shown).width(190.0).show_ui(ui, |ui| {
        for (v, t) in options {
            ui.selectable_value(&mut pick, *v, *t);
        }
    });
    take_keys(ui, &combo.response);
    focus_ring(ui, &combo.response);
    (pick != now).then_some(pick)
}

/// The design's plain button: an outline, words in the text colour.
pub fn button(ui: &mut egui::Ui, l: Look, words: &str) -> bool {
    let b = egui::Button::new(RichText::new(words).size(12.5).color(l.c.fg))
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, l.c.border_strong()))
        .corner_radius(7.0)
        .min_size(egui::vec2(0.0, 30.0));
    let r = ui.add(b);
    take_keys(ui, &r);
    focus_ring(ui, &r);
    r.clicked()
}

pub fn status(ui: &mut egui::Ui, words: &str, color: Color32) {
    ui.label(RichText::new(words).size(12.5).color(color));
}

/// A text field's words while it is typed in, and just after.
pub struct Draft {
    text: String,
    /// Sent to the file, and what the file said then: shown until the file
    /// says something else, or for a moment.
    sent: Option<(String, std::time::Instant)>,
}

/// The fields' drafts, by the field's id.
pub type Drafts = HashMap<String, Draft>;

/// A one-line field that writes when Enter is pressed or it is left (Esc
/// leaves it as it was): the new words, when they differ. The words are all
/// picked when it takes the keys, so what is typed replaces them.
pub fn field(ui: &mut egui::Ui, drafts: &mut Drafts, id: &str, now: &str, hint: &str, width: f32) -> Option<String> {
    let eid = egui::Id::new(("prefs-field", id));
    let focused = ui.memory(|m| m.has_focus(eid));
    if !focused {
        let done = drafts.get(id).is_some_and(|d| match &d.sent {
            Some((was, at)) => now != was || at.elapsed() > std::time::Duration::from_secs(2),
            None => true,
        });
        if done {
            drafts.remove(id);
        }
    }
    let mut text = drafts.get(id).map_or_else(|| now.to_owned(), |d| d.text.clone());
    let edit = egui::TextEdit::singleline(&mut text)
        .id(eid)
        .hint_text(hint)
        .desired_width(width)
        .font(FontId::monospace(12.5))
        .margin(egui::vec2(10.0, 6.0));
    let shown = edit.show(ui);
    let resp = &shown.response.response;
    take_keys(ui, resp);
    if resp.gained_focus() {
        let mut st = shown.state.clone();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::select_all(&shown.galley)));
        st.store(ui.ctx(), eid);
    }
    focus_ring(ui, resp);
    if resp.has_focus() || resp.changed() {
        drafts.insert(id.to_owned(), Draft { text: text.clone(), sent: None });
    }
    if resp.lost_focus() {
        // Enter is reported on two frames in a row; what was sent is not sent
        // again until the field has had the keys (which clears `sent`).
        if drafts.get(id).is_some_and(|d| d.sent.is_some()) {
            return None;
        }
        drafts.remove(id);
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || text == now {
            return None;
        }
        drafts.insert(id.to_owned(), Draft { text: text.clone(), sent: Some((now.to_owned(), std::time::Instant::now())) });
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
        return Some(text);
    }
    None
}

/// A key's cap, as the design draws one.
pub fn keycap(ui: &mut egui::Ui, l: Look, words: &str, lit: bool) -> egui::Response {
    let c = l.c;
    let stroke = if lit { c.run } else { c.border_strong() };
    let b = egui::Button::new(RichText::new(words).font(FontId::monospace(12.0)).color(c.strong()))
        .fill(c.side)
        .stroke(egui::Stroke::new(1.0, stroke))
        .corner_radius(6.0);
    ui.add(b)
}

fn common(table: Option<&'static str>, key: &'static str, value: String) -> CommonChange {
    CommonChange { table, key, value }
}

/// The language row, common.toml's `language` (`now`; `auto` when unset).
/// The note is the app's: what its own words do.
pub fn language_row(ui: &mut egui::Ui, l: Look, w: &Words, now: &str, note: &str) -> Option<CommonChange> {
    let options: Vec<(&str, &str)> =
        ito_i18n::LANGUAGES.iter().map(|(code, name)| (*code, if *code == "auto" { w.auto } else { *name })).collect();
    let code = row(ui, l, w.language, note, |ui| select(ui, "language", now, &options))?;
    Some(common(None, "language", quote(code)))
}

/// The scales the scale row offers, in percent: 50% to 200% by tens, then
/// a few bigger ones.
const SCALES: [i32; 19] = [50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200, 250, 300, 400];

/// The scale row, common.toml's `scale` (`now`; 1.0 when unset), the size
/// the whole window is drawn at. The note is the app's: its keys.
pub fn scale_row(ui: &mut egui::Ui, l: Look, w: &Words, now: f32, note: &str) -> Option<CommonChange> {
    let now = (now * 100.0).round() as i32;
    let mut percents = SCALES.to_vec();
    if !percents.contains(&now) {
        percents.push(now);
        percents.sort_unstable();
    }
    let names: Vec<String> = percents.iter().map(|p| format!("{p}%")).collect();
    let options: Vec<(i32, &str)> = percents.iter().zip(&names).map(|(p, n)| (*p, n.as_str())).collect();
    let picked = row(ui, l, w.scale, note, |ui| select(ui, "scale", now, &options))?;
    Some(scale_change(picked as f32 / 100.0))
}

/// The CLOCK card: common.toml's `[clock]`.
pub fn clock_card(ui: &mut egui::Ui, l: Look, w: &Words, k: &Clock) -> Vec<CommonChange> {
    let mut out = Vec::new();
    section(ui, l, w.clock, |ui| {
        let set = |key: &'static str, v: bool| common(Some("clock"), key, v.to_string());
        if row(ui, l, w.show_time, w.show_time_note, |ui| switch(ui, l, k.show)) {
            out.push(set("show", !k.show));
        }
        sep(ui, l);
        if let Some(h) = row(ui, l, w.time_format, "", |ui| select(ui, "clock-hours", k.hour24, &[(true, w.hour24), (false, w.hour12)])) {
            out.push(set("hour24", h));
        }
        sep(ui, l);
        if row(ui, l, w.show_date, w.show_date_note, |ui| switch(ui, l, k.date)) {
            out.push(set("date", !k.date));
        }
        sep(ui, l);
        let formats: Vec<(&str, &str)> = DATE_FORMATS.iter().map(|f| (*f, *f)).collect();
        if let Some(f) = row(ui, l, w.date_format, "", |ui| select(ui, "clock-date", k.date_format.as_str(), &formats)) {
            out.push(common(Some("clock"), "date_format", quote(f)));
        }
        sep(ui, l);
        if row(ui, l, w.weekday, "", |ui| switch(ui, l, k.weekday)) {
            out.push(set("weekday", !k.weekday));
        }
    });
    out
}

/// The theme the choice in force names, given a theme's name and whether it
/// is light: `system` keeps one for each kind, `dark` and `light` are the
/// two built in.
fn picked(choice: (&str, &str, &str), name: &str, light: bool) -> bool {
    let (theme, dark, bright) = choice;
    match theme {
        "system" => name == if light { bright } else { dark },
        "dark" => name == "tsumugi Dark",
        "light" => name == "tsumugi Light",
        other => name.eq_ignore_ascii_case(other),
    }
}

/// The name of the theme in force: in Follow OS, the one picked for the kind
/// shown (`light`). As the theme list spells it, when it has the theme.
fn in_force<'a>(themes: &'a [Theme], choice: (&'a str, &'a str, &'a str), light: bool) -> &'a str {
    let (theme, dark, bright) = choice;
    let name = match theme {
        "system" if light => bright,
        "system" => dark,
        "dark" => "tsumugi Dark",
        "light" => "tsumugi Light",
        other => other,
    };
    themes.iter().find(|t| t.name.eq_ignore_ascii_case(name)).map_or(name, |t| t.name.as_str())
}

/// The theme page: the mode (Follow OS, Light, Dark) under the heading, the
/// themes of that mode on the left, and on the right the preview the app
/// draws in the theme in force (`l.c`). `choice` is common.toml's `theme`,
/// `dark_theme` and `light_theme`; `note` goes under the list (where the
/// app's own themes are read from).
pub fn theme_page(
    ui: &mut egui::Ui,
    l: Look,
    w: &Words,
    themes: &[Theme],
    choice: (&str, &str, &str),
    note: &str,
    preview: impl FnOnce(&mut egui::Ui),
) -> Vec<CommonChange> {
    let c = &l.c;
    let mut out = Vec::new();
    let (theme, dark, light) = choice;
    let mode = match theme {
        "system" => "system",
        "light" => "light",
        "dark" => "dark",
        name => {
            if themes.iter().any(|t| t.name.eq_ignore_ascii_case(name) && t.colors.light) {
                "light"
            } else {
                "dark"
            }
        }
    };
    // The mode as one segmented control at the left, under the heading, in
    // a row of its own height.
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 36.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.label(RichText::new(w.mode).size(12.0).color(c.dim));
        ui.add_space(8.0);
        egui::Frame::NONE.stroke(egui::Stroke::new(1.0, c.border_strong())).corner_radius(8.0).inner_margin(egui::Margin::same(3)).show(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (k, label) in [("system", w.follow_os), ("light", w.light), ("dark", w.dark)] {
                        let on = mode == k;
                        let b = egui::Button::new(RichText::new(label).size(12.5).color(if on { c.strong() } else { c.dim }))
                            .fill(if on { c.chosen() } else { Color32::TRANSPARENT })
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(6.0)
                            .min_size(egui::vec2(84.0, 26.0));
                        let r = ui.add(b);
                        focus_ring(ui, &r);
                        if r.clicked() && !on {
                            // A mode keeps the theme picked for it.
                            let value = match k {
                                "system" => "system",
                                "light" => light,
                                _ => dark,
                            };
                            out.push(common(None, "theme", quote(value)));
                        }
                    }
                });
            },
        );
    });
    ui.add_space(10.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            // Light and Dark list their own kind; Follow OS lists the kind the
            // OS is showing now (the other kind's theme is set when the OS
            // turns to it, and could not be seen here).
            let list: Vec<&Theme> =
                themes.iter().filter(|t| t.colors.light == if mode == "system" { l.c.light } else { mode == "light" }).collect();
            ui.label(RichText::new(format!("{} · {}", w.themes, list.len())).size(11.0).strong().color(c.dim));
            egui::Frame::NONE
                .fill(c.panel)
                .stroke(egui::Stroke::new(1.0, c.border))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    for t in list {
                        let on = picked(choice, &t.name, t.colors.light);
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
                        let p = ui.painter();
                        if on {
                            p.rect_filled(r, 7.0, c.chosen());
                            p.rect_stroke(r, 7.0, egui::Stroke::new(1.0, c.run), egui::StrokeKind::Inside);
                        } else if resp.hovered() {
                            p.rect_filled(r, 7.0, c.hover());
                        }
                        // A small chip of four: its ground and three of its states.
                        let chip = egui::Rect::from_min_size(egui::pos2(r.left() + 10.0, r.center().y - 9.0), egui::vec2(40.0, 18.0));
                        for (k, sw) in [t.colors.bg, t.colors.wait, t.colors.run, t.colors.err].iter().enumerate() {
                            let x = chip.left() + k as f32 * 10.0;
                            let corner = match k {
                                0 => egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 },
                                3 => egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 },
                                _ => egui::CornerRadius::ZERO,
                            };
                            p.rect_filled(egui::Rect::from_min_size(egui::pos2(x, chip.top()), egui::vec2(10.0, 18.0)), corner, *sw);
                        }
                        p.rect_stroke(chip, 4.0, egui::Stroke::new(1.0, c.border_strong()), egui::StrokeKind::Outside);
                        p.text(
                            egui::pos2(r.left() + 62.0, r.center().y),
                            egui::Align2::LEFT_CENTER,
                            &t.name,
                            FontId::proportional(13.0),
                            if on { c.strong() } else { c.fg },
                        );
                        let tagged = on && mode == "system";
                        let kind = match (tagged, t.colors.light) {
                            (true, true) => w.when_light,
                            (true, false) => w.when_dark,
                            (false, true) => w.light,
                            (false, false) => w.dark,
                        };
                        p.text(
                            egui::pos2(r.right() - 10.0, r.center().y),
                            egui::Align2::RIGHT_CENTER,
                            kind.to_lowercase(),
                            FontId::proportional(11.0),
                            if tagged { c.run } else { c.faint() },
                        );
                        if resp.clicked() {
                            // In Follow OS, a theme becomes the one for its kind.
                            let key = match mode {
                                "system" if t.colors.light => "light_theme",
                                "system" => "dark_theme",
                                _ => "theme",
                            };
                            out.push(common(None, key, quote(&t.name)));
                        }
                    }
                });
            ui.label(RichText::new(note).size(12.0).color(c.dim));
        });
        ui.add_space(20.0);
        ui.vertical(|ui| {
            // By the name picked, not the colours: a theme.toml that repaints
            // the window leaves no theme with these colours (kura #321).
            let name = in_force(themes, choice, l.c.light);
            ui.label(RichText::new(format!("{} · {name}", w.preview)).size(11.0).strong().color(c.fg));
            preview(ui);
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_named_is_the_one_picked() {
        let choice = ("system", "Nord", "tsumugi Light");
        assert!(picked(choice, "Nord", false));
        assert!(picked(choice, "tsumugi Light", true));
        assert!(!picked(choice, "Nord", true), "Nord is the dark one");
        assert!(picked(("dark", "Nord", "x"), "tsumugi Dark", false), "dark is the built-in one");
        assert!(picked(("nord", "", ""), "Nord", false), "a name, whatever its case");
    }

    #[test]
    fn every_language_has_its_words() {
        assert_eq!(Words::of("ja").language, "言語");
        assert_eq!(Words::of("en").language, "Language");
        assert_eq!(Words::of("fr").language, "Language", "English for the rest");
        assert_eq!(Words::of("ja").scale, "倍率");
    }

    type Texts = Vec<(String, egui::Rect)>;

    /// One frame of the frame and the shared pages, drawn without a window.
    fn frame(ctx: &egui::Context, state: &mut State, events: Vec<egui::Event>, keys: Keys) -> (Vec<CommonChange>, Texts) {
        let themes = ito_theme::builtin();
        let nav =
            Nav { pages: &[("General", "One"), ("Theme", "Two")], index: &[(0, "Language"), (1, "Mode")], words: &EN, file: "config.toml" };
        let mut changes = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 1600.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(ui, ito_theme::colors(), state, &nav, keys, |ui, page, l| {
                    if page == 1 {
                        changes = theme_page(ui, l, &EN, &themes, ("dark", "tsumugi Dark", "tsumugi Light"), "", |_| {});
                    } else if let Some(c) = language_row(ui, l, &EN, "auto", "") {
                        changes.push(c);
                    }
                });
            });
        });
        out.textures_delta.clear();
        fn walk(s: &egui::Shape, v: &mut Texts) {
            match s {
                egui::Shape::Text(t) => v.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, v)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        (changes, texts)
    }

    #[test]
    fn the_preview_names_the_theme_picked() {
        let themes = ito_theme::builtin();
        let choice = ("system", "Nord", "tsumugi Light");
        assert_eq!(in_force(&themes, choice, false), "Nord");
        assert_eq!(in_force(&themes, choice, true), "tsumugi Light");
        assert_eq!(in_force(&themes, ("light", "", ""), false), "tsumugi Light");
        assert_eq!(in_force(&themes, ("nord", "", ""), false), "Nord", "as the list spells it");
        assert_eq!(in_force(&themes, ("Mine", "", ""), false), "Mine", "an app's own, not built in");
    }

    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    fn tap(k: egui::Key) -> Vec<egui::Event> {
        vec![key(k, true), key(k, false)]
    }

    /// One frame of a page of a field and a switch: what the field wrote.
    fn form(ctx: &egui::Context, state: &mut State, drafts: &mut Drafts, events: Vec<egui::Event>) -> Option<String> {
        let nav = Nav { pages: &[("Form", "")], index: &[(0, "Width"), (0, "Wrap")], words: &EN, file: "config.toml" };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut wrote = None;
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(ui, ito_theme::colors(), state, &nav, Keys::default(), |ui, _, l| {
                    section(ui, l, "LIST", |ui| {
                        wrote = row(ui, l, "Width", "", |ui| field(ui, drafts, "width", "14", "", 80.0));
                        sep(ui, l);
                        row(ui, l, "Wrap", "", |ui| switch(ui, l, false));
                    });
                });
            });
        });
        out.textures_delta.clear();
        wrote
    }

    fn focused(ctx: &egui::Context) -> Option<egui::Id> {
        ctx.memory(|m| m.focused())
    }

    /// Tab comes to the field with its words picked, so what is typed
    /// replaces them (kura #324, #340).
    #[test]
    fn a_field_tabbed_to_is_typed_over() {
        let ctx = egui::Context::default();
        let (mut state, mut drafts) = (State::default(), Drafts::new());
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Tab));
        assert_eq!(focused(&ctx), Some(egui::Id::new("prefs-search")), "the search first");
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Tab));
        assert_eq!(focused(&ctx), Some(egui::Id::new(("prefs-field", "width"))));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, vec![egui::Event::Text("abc".into())]);
        assert_eq!(form(&ctx, &mut state, &mut drafts, tap(egui::Key::Enter)), Some("abc".into()));
    }

    /// Enter in the search hands the keys to the first row it lights.
    #[test]
    fn enter_in_the_search_goes_to_the_row_found() {
        let ctx = egui::Context::default();
        let (mut state, mut drafts) = (State::default(), Drafts::new());
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Tab));
        form(&ctx, &mut state, &mut drafts, vec![egui::Event::Text("wrap".into())]);
        assert_eq!(state.query, "wrap");
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Enter));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        let at = focused(&ctx).expect("a control has the keys");
        assert_ne!(at, egui::Id::new("prefs-search"));
        assert_ne!(at, egui::Id::new(("prefs-field", "width")), "the row the search lit, not the first");
        // The width row next, as the search lights it.
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Escape));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("prefs-search")));
        state.query = "wid".into();
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Enter));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        assert_eq!(focused(&ctx), Some(egui::Id::new(("prefs-field", "width"))));
    }

    /// A click on a theme writes it into common.toml; the keys walk the
    /// pages round; the file's button is at the foot.
    #[test]
    fn a_theme_clicked_goes_to_common_toml_and_the_keys_walk_the_pages() {
        let ctx = egui::Context::default();
        let mut state = State { page: 1, ..State::default() };
        let (_, texts) = frame(&ctx, &mut state, Vec::new(), Keys::default());
        let themes = ito_theme::builtin();
        let second = themes.iter().find(|t| !t.colors.light && t.name != "tsumugi Dark").expect("another dark theme");
        let at = texts.iter().find(|(t, _)| *t == second.name).map(|(_, r)| r.center()).expect("the theme is listed");
        let click = |down| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, &mut state, vec![egui::Event::PointerMoved(at)], Keys::default());
        let mut changes = frame(&ctx, &mut state, vec![click(true)], Keys::default()).0;
        changes.extend(frame(&ctx, &mut state, vec![click(false)], Keys::default()).0);
        assert_eq!(changes, vec![CommonChange { table: None, key: "theme", value: quote(&second.name) }]);
        assert!(texts.iter().any(|(t, _)| t == "Open config.toml"), "the file at the foot");
        frame(&ctx, &mut state, Vec::new(), Keys { next: true, ..Keys::default() });
        assert_eq!(state.page, 0, "round from the last");
        frame(&ctx, &mut state, Vec::new(), Keys { back: true, ..Keys::default() });
        assert_eq!(state.page, 1);
        frame(&ctx, &mut state, Vec::new(), Keys { find: true, ..Keys::default() });
        let (_, texts) = frame(&ctx, &mut state, Vec::new(), Keys::default());
        assert!(texts.iter().any(|(t, _)| t == "Mode"), "the theme page's own rows");
    }
}
