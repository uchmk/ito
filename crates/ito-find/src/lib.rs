//! The Find / Replace bar of a text editor, so every uchmk editor looks and
//! behaves alike. The bar owns the queries, the toggles and the result label;
//! the app owns the text. The bar never edits: it reports what the person asked
//! ([`Output`]) and the app builds the edit with [`FindBar::subst`] (one
//! `Subst::edits` list = one undo).
//!
//! The search itself runs in the app, off the UI thread. A change of query
//! bumps [`FindBar::version`]; the app tags its job with it and hands the
//! result back with [`FindBar::set_result`], which drops one that is stale.
//!
//! Keys by command name, for the keymap and `--keys`: [`cmd`].

use std::ops::Range;

use egui::{Key, Modifiers, RichText};
use ito_match::{Query, Subst};
use ito_prefs::{button_with, focus_ring, Look, Tone};

/// Command names for the keymap (`find.open` is Ctrl+F, `replace.open` Ctrl+H, ...).
pub mod cmd {
    pub const OPEN: &str = "find.open";
    pub const NEXT: &str = "find.next";
    pub const PREV: &str = "find.prev";
    pub const REPLACE_OPEN: &str = "replace.open";
    pub const REPLACE_ONE: &str = "replace.one";
    pub const REPLACE_ALL: &str = "replace.all";
}

/// What the result label says.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Status {
    /// Nothing asked yet (an empty query).
    #[default]
    Idle,
    /// A search is running.
    Searching,
    /// `current` is the 1-based index of the match at the caret, if any.
    Found { current: Option<usize>, total: usize },
    /// The query is not valid: the engine's one line.
    Error(String),
}

/// What the person did this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// The query or a toggle changed: start a new search with [`FindBar::version`].
    pub query_changed: bool,
    pub next: bool,
    pub prev: bool,
    pub replace_one: bool,
    pub replace_all: bool,
    /// Esc or the close button: hide the bar and give focus back to the editor.
    pub closed: bool,
}

/// Every word on the bar, from the app's `lang/*.toml`.
#[derive(Clone, Copy, Debug)]
pub struct Words<'a> {
    pub find_hint: &'a str,
    pub replace_hint: &'a str,
    pub regex: &'a str,
    pub match_case: &'a str,
    pub whole_word: &'a str,
    pub in_selection: &'a str,
    pub next: &'a str,
    pub prev: &'a str,
    pub replace: &'a str,
    pub replace_all: &'a str,
    pub close: &'a str,
    pub toggle_replace: &'a str,
    pub no_results: &'a str,
    pub searching: &'a str,
}

#[derive(Clone, Debug, Default)]
pub struct FindBar {
    pub open: bool,
    /// The Replace row is shown (Ctrl+H, or the toggle).
    pub replace_open: bool,
    pub find: String,
    pub replace: String,
    pub regex: bool,
    /// Off is smart case, as `Matcher::new` has it; on is exact.
    pub match_case: bool,
    pub whole_word: bool,
    pub in_selection: bool,
    pub status: Status,
    version: u64,
    focus_find: bool,
}

impl FindBar {
    /// Ctrl+F: show the bar with focus in the Find field. `seed` (the selected word) replaces the query when given.
    pub fn open_find(&mut self, seed: Option<&str>) {
        self.open = true;
        self.focus_find = true;
        if let Some(s) = seed.filter(|s| !s.is_empty() && !s.contains('\n')) {
            self.find = s.to_owned();
            self.changed();
        }
    }

    /// Ctrl+H: as `open_find`, with the Replace row.
    pub fn open_replace(&mut self, seed: Option<&str>) {
        self.open_find(seed);
        self.replace_open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Run the keymap command `name` ([`cmd`]); the ones that step or replace come back as an [`Output`].
    pub fn command(&mut self, name: &str, seed: Option<&str>) -> Output {
        let mut out = Output::default();
        match name {
            cmd::OPEN => self.open_find(seed),
            cmd::REPLACE_OPEN => self.open_replace(seed),
            cmd::NEXT => out.next = true,
            cmd::PREV => out.prev = true,
            cmd::REPLACE_ONE => out.replace_one = true,
            cmd::REPLACE_ALL => out.replace_all = true,
            _ => {}
        }
        out
    }

    /// The number of the latest query. Tag a search job with it.
    pub fn version(&self) -> u64 {
        self.version
    }

    fn changed(&mut self) {
        self.version += 1;
        self.status = if self.find.is_empty() { Status::Idle } else { Status::Searching };
    }

    /// The search options as `ito-match` reads them.
    pub fn query(&self) -> Query {
        Query { regex: self.regex, whole_word: self.whole_word, ignore_case: self.match_case.then_some(false), all: true }
    }

    /// The substitution for Replace / Replace all (`Err` is the engine's one line).
    pub fn subst(&self) -> Result<Subst, String> {
        Subst::from_query(&self.find, &self.replace, self.query())
    }

    /// The matches in `text` (or in `within` for "in selection"), non-empty, ascending: the ones to highlight and step through.
    pub fn ranges(&self, text: &str, within: Option<Range<usize>>) -> Result<Vec<Range<usize>>, String> {
        if self.find.is_empty() {
            return Ok(Vec::new());
        }
        let s = Subst::from_query(&self.find, "", self.query())?;
        let within = if self.in_selection { within } else { None };
        let edits = s.edits_in(text, within.unwrap_or(0..text.len()));
        Ok(edits.into_iter().map(|(r, _)| r).filter(|r| r.start < r.end).collect())
    }

    /// The result of the search tagged `version`. A stale one (the query moved on) is dropped; `true` when taken.
    pub fn set_result(&mut self, version: u64, result: Result<Status, String>) -> bool {
        if version != self.version {
            return false;
        }
        self.status = match result {
            Ok(s) => s,
            Err(e) => Status::Error(e),
        };
        true
    }

    /// The label for the count: `3 / 12`, the "no results" words, the searching words or the error.
    pub fn label(&self, w: &Words) -> String {
        match &self.status {
            Status::Idle => String::new(),
            Status::Searching => w.searching.to_owned(),
            Status::Found { total: 0, .. } => w.no_results.to_owned(),
            Status::Found { current, total } => format!("{} / {}", current.map_or("?".to_owned(), |c| c.to_string()), total),
            Status::Error(e) => e.clone(),
        }
    }

    /// Draw the bar (nothing when closed) and say what the person did.
    pub fn show(&mut self, ui: &mut egui::Ui, look: Look, w: &Words) -> Output {
        let mut out = Output::default();
        if !self.open {
            return out;
        }
        let c = look.c;
        // While a composition is going on, Enter / Esc belong to the IME.
        let composing = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Ime(_))));
        egui::Frame::new()
            .fill(c.raised())
            .stroke(egui::Stroke::new(1.0, c.border_strong()))
            .corner_radius(ito_theme::scale::radius::BOX)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let arrow = if self.replace_open { "▾" } else { "▸" };
                    let t = ui.add(egui::Button::new(arrow).frame(false)).on_hover_text(w.toggle_replace);
                    ito_a11y::say(&t, ito_a11y::Role::Button, w.toggle_replace);
                    if t.clicked() {
                        self.replace_open = !self.replace_open;
                    }
                    let before = (self.find.clone(), self.regex, self.match_case, self.whole_word, self.in_selection);
                    let f = ui.add(egui::TextEdit::singleline(&mut self.find).hint_text(w.find_hint).desired_width(240.0));
                    ito_a11y::say(&f, ito_a11y::Role::Label, w.find_hint);
                    if std::mem::take(&mut self.focus_find) {
                        f.request_focus();
                    }
                    if f.lost_focus() && !composing && ui.input(|i| i.key_pressed(Key::Enter)) {
                        if ui.input(|i| i.modifiers.matches_exact(Modifiers::SHIFT)) {
                            out.prev = true;
                        } else {
                            out.next = true;
                        }
                        f.request_focus();
                    }
                    toggle(ui, &c, &mut self.match_case, "Aa", w.match_case);
                    toggle(ui, &c, &mut self.whole_word, "ab", w.whole_word);
                    toggle(ui, &c, &mut self.regex, ".*", w.regex);
                    toggle(ui, &c, &mut self.in_selection, "≡", w.in_selection);
                    if before != (self.find.clone(), self.regex, self.match_case, self.whole_word, self.in_selection) {
                        self.changed();
                        out.query_changed = true;
                    }
                    let label = self.label(w);
                    if !label.is_empty() {
                        let bad = matches!(self.status, Status::Error(_)) || label == w.no_results;
                        let mark = if bad { "✖ " } else { "" };
                        let col = if bad { c.err } else { c.dim };
                        let r = ui.label(RichText::new(format!("{mark}{label}")).color(col).size(ito_theme::zoomed(ui.ctx(), 12.0)));
                        ito_a11y::say(&r, ito_a11y::Role::Label, &label);
                    }
                    for (words, glyph, flag) in [(w.prev, "↑", &mut out.prev), (w.next, "↓", &mut out.next)] {
                        let r = ui.add(egui::Button::new(glyph).min_size(egui::vec2(24.0, 24.0))).on_hover_text(words);
                        ito_a11y::say(&r, ito_a11y::Role::Button, words);
                        focus_ring(ui, &r);
                        *flag |= r.clicked();
                    }
                    let x = ui.add(egui::Button::new("✕").frame(false)).on_hover_text(w.close);
                    ito_a11y::say(&x, ito_a11y::Role::Button, w.close);
                    out.closed |= x.clicked();
                });
                if self.replace_open {
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        let r = ui.add(egui::TextEdit::singleline(&mut self.replace).hint_text(w.replace_hint).desired_width(240.0));
                        ito_a11y::say(&r, ito_a11y::Role::Label, w.replace_hint);
                        if r.lost_focus() && !composing && ui.input(|i| i.key_pressed(Key::Enter)) {
                            out.replace_one = true;
                            r.request_focus();
                        }
                        out.replace_one |= button_with(ui, look, w.replace, Tone::Plain).clicked();
                        out.replace_all |= button_with(ui, look, w.replace_all, Tone::Primary).clicked();
                    });
                }
            });
        if !composing && ui.input(|i| i.key_pressed(Key::Escape)) && ui.memory(|m| m.focused().is_some()) {
            out.closed = true;
        }
        if out.closed {
            self.open = false;
        }
        out
    }
}

/// A small on/off button. On is told by a fill, a frame and an underline, not by colour alone; the reader hears on / off.
fn toggle(ui: &mut egui::Ui, c: &ito_theme::Colors, on: &mut bool, glyph: &str, words: &str) {
    let text = if *on { RichText::new(glyph).strong().underline().color(c.on_accent()) } else { RichText::new(glyph).color(c.fg) };
    let mut b = egui::Button::new(text).min_size(egui::vec2(26.0, 24.0));
    if *on {
        b = b.fill(c.accent()).stroke(egui::Stroke::new(1.5, c.strong()));
    }
    let r = ui.add(b).on_hover_text(words);
    ito_a11y::say(&r, ito_a11y::Role::Button, &format!("{words}: {}", if *on { "on" } else { "off" }));
    focus_ring(ui, &r);
    if r.clicked() {
        *on = !*on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Words = Words {
        find_hint: "Find",
        replace_hint: "Replace",
        regex: "Regex",
        match_case: "Match case",
        whole_word: "Whole word",
        in_selection: "In selection",
        next: "Next",
        prev: "Previous",
        replace: "Replace",
        replace_all: "Replace all",
        close: "Close",
        toggle_replace: "Show replace",
        no_results: "No results",
        searching: "searching…",
    };

    #[test]
    fn stale_results_are_dropped() {
        let mut b = FindBar::default();
        b.open_find(Some("cat"));
        let v = b.version();
        b.find.push('s');
        b.changed();
        assert!(!b.set_result(v, Ok(Status::Found { current: Some(1), total: 3 })));
        assert_eq!(b.status, Status::Searching);
        assert!(b.set_result(b.version(), Ok(Status::Found { current: Some(2), total: 12 })));
        assert_eq!(b.label(&W), "2 / 12");
        assert!(b.set_result(b.version(), Err("bad".into())));
        assert_eq!(b.label(&W), "bad");
        b.status = Status::Found { current: None, total: 0 };
        assert_eq!(b.label(&W), "No results");
    }

    #[test]
    fn ranges_follow_the_toggles() {
        let mut b = FindBar { find: "cat".into(), ..Default::default() };
        let t = "cat Cat concat";
        assert_eq!(b.ranges(t, None).unwrap().len(), 3);
        b.match_case = true;
        assert_eq!(b.ranges(t, None).unwrap().len(), 2);
        b.whole_word = true;
        assert_eq!(b.ranges(t, None).unwrap(), vec![0..3]);
        b.in_selection = true;
        assert!(b.ranges(t, Some(4..7)).unwrap().is_empty());
        b.find = "(".into();
        b.regex = true;
        assert!(b.ranges(t, None).is_err());
    }

    #[test]
    fn replace_goes_through_subst() {
        let b = FindBar { find: "a(b)".into(), replace: "$1!".into(), regex: true, ..Default::default() };
        assert_eq!(b.subst().unwrap().edits("xab ab")[1].1, "b!");
        let lit = FindBar { find: "a.b".into(), replace: "$".into(), ..Default::default() };
        assert_eq!(lit.subst().unwrap().apply("a.b axb"), "$ axb");
    }

    #[test]
    fn commands_and_seed() {
        let mut b = FindBar::default();
        assert!(!b.command(cmd::NEXT, None).query_changed);
        b.command(cmd::REPLACE_OPEN, Some("word"));
        assert!(b.open && b.replace_open && b.find == "word" && b.version() == 1);
        b.open_find(Some("two\nlines"));
        assert_eq!(b.find, "word", "a multi-line selection is not a query");
        assert!(b.command(cmd::REPLACE_ALL, None).replace_all);
        assert!(b.command(cmd::REPLACE_ONE, None).replace_one);
    }

    #[test]
    fn it_draws_headless() {
        let ctx = egui::Context::default();
        let mut b = FindBar::default();
        b.open_replace(Some("x"));
        let mut out = Output::default();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            out = b.show(ui, Look { c: ito_theme::colors(), q: "" }, &W);
        })
        .drop_without_applying_deltas();
        assert!(!out.closed && b.open);
    }
}
