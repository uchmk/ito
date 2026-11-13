//! A `name | value` rows editor: HTTP headers, a job's environment variables, query parameters, any `key = value` table on a
//! settings page. [`Pairs`] holds the rows being edited (the app keeps one across frames) and [`PairsEditor`] draws them.

use std::borrow::Cow;

use egui::{Id, RichText, Sense, Ui, Vec2};
use ito_a11y::Role;

use crate::{mark_button, z, IconButton, InputField, Mark};

/// One row being edited.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PairRow {
    key: u32,
    name: String,
    value: String,
}

/// The rows of a name | value table being edited. The app keeps one across frames (open it from what is saved, drop or
/// rebuild it on cancel). Each row has a key fixed for its life: removing another row does not renumber it, and a key is
/// never given out twice, so its test IDs keep naming the same row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pairs {
    rows: Vec<PairRow>,
    next: u32,
}

impl Pairs {
    /// Rows from saved pairs, in the order given, keys `0..len`.
    pub fn new<N: Into<String>, V: Into<String>>(pairs: impl IntoIterator<Item = (N, V)>) -> Self {
        let mut p = Pairs::default();
        for (n, v) in pairs {
            p.push(n, v);
        }
        p
    }

    /// Adds an empty row at the end; returns its key.
    pub fn add(&mut self) -> u32 {
        self.push(String::new(), String::new())
    }

    /// Adds a filled row at the end; returns its key.
    pub fn push(&mut self, name: impl Into<String>, value: impl Into<String>) -> u32 {
        let key = self.next;
        self.next += 1;
        self.rows.push(PairRow { key, name: name.into(), value: value.into() });
        key
    }

    /// Removes the row with `key`; whether there was one.
    pub fn remove(&mut self, key: u32) -> bool {
        let before = self.rows.len();
        self.rows.retain(|r| r.key != key);
        self.rows.len() != before
    }

    /// How many rows there are.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether there is no row.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// `(key, name, value)` as typed, in order.
    pub fn rows(&self) -> impl Iterator<Item = (u32, &str, &str)> {
        self.rows.iter().map(|r| (r.key, r.name.as_str(), r.value.as_str()))
    }

    /// The pairs to save, in order: names trimmed, values as typed. A row with an empty name is left out (an empty row is
    /// blank; one with a value but no name is a fault the editor shows, and it is left out here too).
    pub fn pairs(&self) -> Vec<(String, String)> {
        self.rows.iter().filter(|r| !r.name.trim().is_empty()).map(|r| (r.name.trim().to_owned(), r.value.clone())).collect()
    }
}

/// The words of the editor. [`PairWords::en`], [`PairWords::ja`] and [`PairWords::of`] give ito's words; an app changes the
/// column names with struct update (`PairWords { name: "Header", ..PairWords::of(lang) }`).
#[derive(Clone, Copy, Debug)]
pub struct PairWords<'a> {
    /// The head of the name column; also the spoken name of a name box (followed by the row's place).
    pub name: &'a str,
    /// The head of the value column; also the spoken name of a value box.
    pub value: &'a str,
    /// The label and spoken name of the Add button.
    pub add: &'a str,
    /// The tip and spoken name of a row's remove button (followed by the row's name, or its place).
    pub remove: &'a str,
    /// Shown while there is no row.
    pub empty: &'a str,
    /// A row with a value and no name.
    pub missing: &'a str,
    /// A name used by an earlier row (with [`PairsEditor::unique`]).
    pub twice: &'a str,
}

impl PairWords<'static> {
    /// ito's English words.
    pub fn en() -> Self {
        PairWords {
            name: "Name",
            value: "Value",
            add: "Add",
            remove: "Remove",
            empty: "None yet",
            missing: "Enter a name",
            twice: "This name is used above",
        }
    }

    /// ito's Japanese words.
    pub fn ja() -> Self {
        PairWords {
            name: "名前",
            value: "値",
            add: "追加",
            remove: "削除",
            empty: "まだありません",
            missing: "名前を入れてください",
            twice: "同じ名前が上にあります",
        }
    }

    /// The words of the language code (`"ja"`, else English).
    pub fn of(code: &str) -> Self {
        if code.starts_with("ja") {
            Self::ja()
        } else {
            Self::en()
        }
    }
}

impl Default for PairWords<'static> {
    fn default() -> Self {
        Self::en()
    }
}

/// What a frame of the editor found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PairsResponse {
    /// A row was typed in, added or removed this frame.
    pub changed: bool,
    /// No row has a fault (a value without a name, the app's check, a repeated name). The app keeps its save button off while
    /// false. It comes from this frame's final rows, so a button drawn after the editor sees the truth.
    pub valid: bool,
    /// The key of the row added this frame.
    pub added: Option<u32>,
    /// A box or button of the editor has the keyboard focus.
    pub focused: bool,
}

/// The focus the editor still owes a widget, kept in egui's memory under the editor's id: set by Add and Remove, applied
/// when that widget is drawn, then cleared. Nothing else ever asks for focus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pending {
    /// The name box of this row.
    name: Option<u32>,
    /// The remove button of this row.
    remove: Option<u32>,
    /// The add button.
    add: bool,
}

impl Pending {
    fn is_empty(&self) -> bool {
        *self == Pending::default()
    }
}

type Check<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// The editor. Builder like [`InputField`].
///
/// `id` tells this editor from another on the same screen (two editors need different ids) and starts its test IDs:
/// `{id}.row.{key}.name`, `{id}.row.{key}.value`, `{id}.row.{key}.remove`, `{id}.add` (each only when
/// [`ito_a11y::is_test_id`] accepts it).
pub struct PairsEditor<'a> {
    id: &'a str,
    words: PairWords<'a>,
    hint_name: &'a str,
    hint_value: &'a str,
    mono_name: bool,
    mono_value: bool,
    check_name: Option<&'a Check<'a>>,
    check_value: Option<&'a Check<'a>>,
    unique: Option<bool>,
    name_width: Option<f32>,
}

impl<'a> PairsEditor<'a> {
    /// An editor with the given id (see the type's docs) and words; no hints, proportional font, no checks, names may repeat.
    pub fn new(id: &'a str, words: PairWords<'a>) -> Self {
        PairsEditor {
            id,
            words,
            hint_name: "",
            hint_value: "",
            mono_name: false,
            mono_value: false,
            check_name: None,
            check_value: None,
            unique: None,
            name_width: None,
        }
    }

    /// Grey text of an empty name box and of an empty value box (`Authorization` and `secret:api`).
    pub fn hints(mut self, name: &'a str, value: &'a str) -> Self {
        self.hint_name = name;
        self.hint_value = value;
        self
    }

    /// Names in the monospace font.
    pub fn mono_names(mut self) -> Self {
        self.mono_name = true;
        self
    }

    /// Values in the monospace font.
    pub fn mono_values(mut self) -> Self {
        self.mono_value = true;
        self
    }

    /// The app's check of one trimmed, non-empty name: `None` when fine, else the reason shown under the row.
    pub fn check(mut self, f: &'a dyn Fn(&str) -> Option<String>) -> Self {
        self.check_name = Some(f);
        self
    }

    /// The app's check of one non-empty value (a header value holding a line break from a paste): `None` when fine, else the
    /// reason shown under the row, when the name has no fault of its own.
    pub fn check_value(mut self, f: &'a dyn Fn(&str) -> Option<String>) -> Self {
        self.check_value = Some(f);
        self
    }

    /// A name may appear once; `fold_case` compares ASCII letters without case (Windows variables). Without this a name can
    /// repeat (HTTP allows repeated headers); the app decides.
    pub fn unique(mut self, fold_case: bool) -> Self {
        self.unique = Some(fold_case);
        self
    }

    /// Room for the name column (default: 38% of the width, at least 120 points).
    pub fn name_width(mut self, w: f32) -> Self {
        self.name_width = Some(w);
        self
    }

    /// The fault of a name (trimmed) in a row, given the rows above it and the row removed this frame.
    fn name_fault(&self, name: &str, has_value: bool, before: &[PairRow], gone: Option<u32>) -> Option<Cow<'a, str>> {
        if name.is_empty() {
            return has_value.then_some(Cow::Borrowed(self.words.missing));
        }
        if let Some(reason) = self.check_name.and_then(|f| f(name)) {
            return Some(Cow::Owned(reason));
        }
        if let Some(fold) = self.unique {
            let same = |o: &str| if fold { o.eq_ignore_ascii_case(name) } else { o == name };
            if before.iter().filter(|r| Some(r.key) != gone).any(|r| same(r.name.trim())) {
                return Some(Cow::Borrowed(self.words.twice));
            }
        }
        None
    }

    fn value_fault(&self, value: &str) -> Option<String> {
        if value.is_empty() {
            return None;
        }
        self.check_value.and_then(|f| f(value))
    }

    /// Draws the rows and the Add button, edits `pairs` in place, and reports what this frame found.
    pub fn show(self, ui: &mut Ui, pairs: &mut Pairs) -> PairsResponse {
        let ctx = ui.ctx().clone();
        let c = ito_theme::colors();
        let state_id = Id::new(("ito-pairs", self.id));
        let mut pend: Pending = ctx.data(|d| d.get_temp(state_id)).unwrap_or_default();
        let gap = ito_theme::scale::space::SM;
        let total = ui.available_width();
        let side_pts = ito_theme::scale::height::CONTROL;
        let side = z(ui, side_pts);
        let min_value = z(ui, 120.0);
        let name_w = self.name_width.unwrap_or_else(|| (total * 0.38).max(z(ui, 120.0))).min(total);
        let value_w = total - name_w - side - 2.0 * gap;
        // Below this the value goes under the name rather than squeezing both.
        let stacked = value_w < min_value;
        let mut out = PairsResponse { valid: true, ..Default::default() };
        // Who had the keys when the frame began: a press on a button may move them before the button is drawn.
        let focused_at_start = ctx.memory(|m| m.focused());

        if pairs.is_empty() {
            ui.label(RichText::new(self.words.empty).size(z(ui, 12.0)).color(c.dim));
            ui.add_space(ito_theme::scale::space::SM);
        } else if !stacked {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (w, text) in [(name_w, self.words.name), (value_w, self.words.value)] {
                    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, z(ui, 16.0)), Sense::hover());
                    ui.painter().text(
                        rect.left_center() + Vec2::new(z(ui, 9.0), 0.0),
                        egui::Align2::LEFT_CENTER,
                        text,
                        egui::FontId::proportional(z(ui, 11.5)),
                        c.dim,
                    );
                    ito_a11y::say(&r, Role::Label, text);
                }
            });
            ui.add_space(ito_theme::scale::space::XS);
        }

        let mut gone: Option<u32> = None;
        let mut gone_focus = false;
        let mut pick_after_gone: Option<Pending> = None;
        for i in 0..pairs.rows.len() {
            let (before, rest) = pairs.rows.split_at_mut(i);
            let before_key = before.last().map(|r| r.key);
            let after_key = rest.get(1).map(|r| r.key);
            let row = &mut rest[0];
            let gone_now = gone;
            let key = row.key;
            let n = i + 1;
            let had_value = !row.value.is_empty();
            let name_id = format!("{}.row.{key}.name", self.id);
            let value_id = format!("{}.row.{key}.value", self.id);
            let remove_id = format!("{}.row.{key}.remove", self.id);
            let name_said = format!("{} {n}", self.words.name);
            let value_said = format!("{} {n}", self.words.value);
            let hint_name = self.hint_name;
            let hint_value = self.hint_value;
            let (mono_name, mono_value) = (self.mono_name, self.mono_value);
            let name_late = |t: &str| self.name_fault(t.trim(), had_value, before, gone_now).is_some();
            let value_late = |t: &str| self.value_fault(t).is_some();

            let mut name_resp = None;
            let mut value_resp = None;
            let mut remove_resp = None;
            let draw_name = |ui: &mut Ui, w: f32, row: &mut PairRow| {
                let mut f = InputField::new(&name_id).name(&name_said).hint(hint_name).width(w);
                if mono_name {
                    f = f.mono();
                }
                f.show_with(ui, &mut row.name, &name_late)
            };
            let draw_value = |ui: &mut Ui, w: f32, row: &mut PairRow| {
                let mut f = InputField::new(&value_id).name(&value_said).hint(hint_value).width(w);
                if mono_value {
                    f = f.mono();
                }
                f.show_with(ui, &mut row.value, &value_late)
            };
            // The remove button's spoken name: the row's name when it has one, else its place.
            let remove_said = |row: &PairRow| {
                let n_trim = row.name.trim();
                if n_trim.is_empty() {
                    format!("{} {n}", self.words.remove)
                } else {
                    format!("{} {n_trim}", self.words.remove)
                }
            };
            ui.scope_builder(egui::UiBuilder::new().id(Id::new((self.id, key))), |ui| {
                if stacked {
                    // The two boxes of one pair sit closer than one pair to the next (see the space after the row).
                    ui.spacing_mut().item_spacing.y *= 0.5;
                    let mut slot = egui::Rect::NOTHING;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        name_resp = Some(draw_name(ui, (total - side - gap).max(min_value), row));
                        slot = ui.allocate_exact_size(Vec2::splat(side), Sense::hover()).0;
                    });
                    value_resp = Some(draw_value(ui, total, row));
                    let said = remove_said(row);
                    remove_resp = Some(
                        ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| mark_button(ui, Mark::Close, &said, side_pts)).inner,
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        name_resp = Some(draw_name(ui, name_w, row));
                        value_resp = Some(draw_value(ui, value_w, row));
                        let said = remove_said(row);
                        remove_resp = Some(mark_button(ui, Mark::Close, &said, side_pts));
                    });
                }
            });
            let (name_resp, value_resp, remove_resp) = (name_resp.expect("drawn"), value_resp.expect("drawn"), remove_resp.expect("drawn"));
            for (r, id) in [(&name_resp, &name_id), (&value_resp, &value_id), (&remove_resp, &remove_id)] {
                if ito_a11y::is_test_id(id) {
                    ito_a11y::test_id(r, id);
                }
            }
            if pend.name == Some(key) {
                name_resp.request_focus();
                pend.name = None;
            }
            if pend.remove == Some(key) {
                remove_resp.request_focus();
                pend.remove = None;
            }
            out.changed |= name_resp.changed() || value_resp.changed();
            out.focused |= name_resp.has_focus() || value_resp.has_focus() || remove_resp.has_focus();
            // The name box was drawn with the value as it was before this frame's typing: draw it again to settle its border.
            if row.name.trim().is_empty() && had_value != !row.value.is_empty() {
                ctx.request_repaint();
            }
            if remove_resp.clicked() {
                gone = Some(key);
                gone_focus = [&name_resp, &value_resp, &remove_resp].iter().any(|r| r.has_focus() || focused_at_start == Some(r.id));
                // Where the keys go: the next row's remove button, else the previous row's, else Add.
                pick_after_gone = Some(match after_key.or(before_key) {
                    Some(k) => Pending { remove: Some(k), ..Default::default() },
                    None => Pending { add: true, ..Default::default() },
                });
                continue;
            }
            let fault = self
                .name_fault(row.name.trim(), !row.value.is_empty(), before, gone)
                .map(Cow::into_owned)
                .or_else(|| self.value_fault(&row.value));
            if let Some(text) = fault {
                out.valid = false;
                ui.horizontal(|ui| {
                    ui.add_space(z(ui, 9.0));
                    ui.label(RichText::new(text).size(z(ui, 11.5)).color(c.err));
                });
            }
            // Stacked pairs are two boxes tall: a larger step between pairs than inside one keeps each pair together.
            ui.add_space(if stacked { ito_theme::scale::space::MD } else { ito_theme::scale::space::XS });
        }
        if let Some(key) = gone {
            pairs.remove(key);
            out.changed = true;
            if gone_focus {
                if let Some(p) = pick_after_gone {
                    pend.remove = p.remove;
                    pend.add = p.add;
                }
            }
        }

        ui.add_space(ito_theme::scale::space::XS);
        // An explicit id: the rows above come and go, and an automatic id follows the widgets before it.
        let add = ui
            .scope_builder(egui::UiBuilder::new().id(Id::new((self.id, "add"))), |ui| IconButton::new(Mark::Plus, self.words.add).show(ui))
            .inner;
        if ito_a11y::is_test_id(self.id) {
            let id = format!("{}.add", self.id);
            if ito_a11y::is_test_id(&id) {
                ito_a11y::test_id(&add, &id);
            }
        }
        if pend.add {
            add.request_focus();
            pend.add = false;
        }
        out.focused |= add.has_focus();
        if add.clicked() {
            let key = pairs.add();
            pend.name = Some(key);
            out.added = Some(key);
            out.changed = true;
        }

        // A pending focus whose row is gone is dropped.
        if pend.name.is_some_and(|k| !pairs.rows.iter().any(|r| r.key == k)) {
            pend.name = None;
        }
        if pend.remove.is_some_and(|k| !pairs.rows.iter().any(|r| r.key == k)) {
            pend.remove = None;
        }
        if pend.is_empty() {
            ctx.data_mut(|d| d.remove_temp::<Pending>(state_id));
        } else {
            ctx.data_mut(|d| d.insert_temp(state_id, pend));
            ctx.request_repaint();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, Rect};

    fn screen(w: f32) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(w, 800.0))), ..Default::default() }
    }

    /// Frames of one or two editors on a screen.
    struct Rig {
        ctx: Context,
        width: f32,
        words: PairWords<'static>,
    }

    impl Rig {
        fn new() -> Self {
            let ctx = Context::default();
            ctx.enable_accesskit();
            Rig { ctx, width: 700.0, words: PairWords::en() }
        }

        /// One frame; `f` draws (and returns what the test wants out of it).
        fn frame<R>(&self, events: Vec<Event>, mut f: impl FnMut(&mut Ui) -> R) -> (R, egui::FullOutput) {
            let mut input = screen(self.width);
            input.events = events;
            let mut r = None;
            let mut out = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| r = Some(f(ui)));
            });
            out.textures_delta.clear();
            (r.expect("drawn"), out)
        }

        fn step<'e>(&self, events: Vec<Event>, pairs: &mut Pairs, build: impl Fn(PairsEditor<'e>) -> PairsEditor<'e>) -> PairsResponse {
            let words = self.words;
            self.frame(events, |ui| {
                let r = build(PairsEditor::new("hd", words)).show(ui, pairs);
                // Somewhere else to have the keys.
                InputField::new("elsewhere").show(ui, &mut String::new());
                r
            })
            .0
        }

        fn ids(&self) -> Vec<ito_a11y::TestIdEntry> {
            ito_a11y::take_test_ids(&self.ctx, egui::ViewportId::ROOT)
        }

        fn focus(&self, name_id: &str) {
            self.ctx.memory_mut(|m| m.request_focus(Id::new(("ito-input", name_id))));
        }

        fn has_focus(&self, name_id: &str) -> bool {
            self.ctx.memory(|m| m.has_focus(Id::new(("ito-input", name_id))))
        }

        fn typed(&self, text: &str) -> Vec<Event> {
            vec![Event::Text(text.to_owned())]
        }
    }

    fn plain(e: PairsEditor<'_>) -> PairsEditor<'_> {
        e
    }

    /// A click on the widget with test ID `name` as four frames (move, press, release, settle).
    fn click(rig: &Rig, name: &str, mut draw: impl FnMut(&Rig, Vec<Event>)) {
        draw(rig, vec![]);
        let at = rig.ids().iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} not registered")).rect.center();
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        draw(rig, vec![Event::PointerMoved(at)]);
        draw(rig, vec![button(true)]);
        draw(rig, vec![button(false)]);
        draw(rig, vec![]);
    }

    #[test]
    fn keys_are_stable_and_never_reused() {
        let mut p = Pairs::new([("a", "1"), ("b", "2"), ("c", "3")]);
        assert_eq!(p.rows().map(|r| r.0).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(p.remove(1));
        assert!(!p.remove(1));
        assert_eq!(p.rows().map(|r| r.0).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(p.add(), 3);
        assert!(p.remove(3));
        assert_eq!(p.push("d", "4"), 4, "a removed key is not given out again");
        assert_eq!(p.len(), 3);
        assert!(!p.is_empty());
    }

    #[test]
    fn pairs_trims_names_drops_blank_rows_and_keeps_order() {
        let mut p = Pairs::new([(" B ", " 2 "), ("", ""), ("  ", ""), ("", "orphan"), ("A", "")]);
        p.add();
        assert_eq!(p.pairs(), [("B".to_owned(), " 2 ".to_owned()), ("A".to_owned(), String::new())]);
    }

    #[test]
    fn a_value_without_a_name_is_a_fault_and_a_new_empty_row_is_not() {
        let rig = Rig::new();
        let mut p = Pairs::default();
        assert!(rig.step(vec![], &mut p, plain).valid, "empty editor");
        p.add();
        assert!(rig.step(vec![], &mut p, plain).valid, "a new empty row");
        p.push("", "x");
        assert!(!rig.step(vec![], &mut p, plain).valid);
        let mut p = Pairs::new([("A", "1")]);
        assert!(rig.step(vec![], &mut p, plain).valid);
    }

    #[test]
    fn the_apps_checks_make_a_fault() {
        let rig = Rig::new();
        let bad = |n: &str| n.contains(' ').then(|| "no spaces".to_owned());
        let no_break = |v: &str| v.contains('\n').then(|| "one line".to_owned());
        let mut p = Pairs::new([("a b", "1")]);
        assert!(!rig.step(vec![], &mut p, |e| e.check(&bad)).valid);
        let mut p = Pairs::new([("ab", "1\n2")]);
        assert!(rig.step(vec![], &mut p, |e| e.check(&bad)).valid);
        assert!(!rig.step(vec![], &mut p, |e| e.check_value(&no_break)).valid);
        let mut p = Pairs::new([("ab", "")]);
        assert!(rig.step(vec![], &mut p, |e| e.check_value(&no_break)).valid);
    }

    #[test]
    fn unique_names_fault_the_later_row_only_exactly_or_folded() {
        let rig = Rig::new();
        let mut p = Pairs::new([("Accept", "1"), ("accept", "2")]);
        assert!(rig.step(vec![], &mut p, plain).valid, "repeats are allowed unless asked");
        assert!(rig.step(vec![], &mut p, |e| e.unique(false)).valid, "exact: the case differs");
        assert!(!rig.step(vec![], &mut p, |e| e.unique(true)).valid);
        // The fault is under the later row only.
        let (_, out) = rig.frame(vec![], |ui| PairsEditor::new("hd", PairWords::en()).unique(true).show(ui, &mut p));
        let update = out.platform_output.accesskit_update.expect("accesskit");
        let invalid: Vec<String> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.invalid() == Some(egui::accesskit::Invalid::True))
            .map(|(_, n)| n.label().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(invalid, ["Name 2"], "{invalid:?}");
        let mut p = Pairs::new([("A", "1"), (" A ", "2")]);
        assert!(!rig.step(vec![], &mut p, |e| e.unique(false)).valid, "names are compared trimmed");
    }

    #[test]
    fn test_ids_and_spoken_names_are_registered_also_after_a_remove() {
        let rig = Rig::new();
        let mut p = Pairs::new([("Authorization", "x"), ("", ""), ("Accept", "y")]);
        let (_, out) = rig.frame(vec![], |ui| PairsEditor::new("hd", PairWords::en()).show(ui, &mut p));
        let names: Vec<String> = rig.ids().into_iter().map(|e| e.name).collect();
        for want in ["hd.row.0.name", "hd.row.0.value", "hd.row.0.remove", "hd.row.2.remove", "hd.add"] {
            assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
        }
        let labels: Vec<String> =
            out.platform_output.accesskit_update.expect("ak").nodes.iter().filter_map(|(_, n)| n.label().map(str::to_owned)).collect();
        for want in ["Name 1", "Value 1", "Remove Authorization", "Name 2", "Remove 2", "Remove Accept", "Value 3", "Add"] {
            assert!(labels.iter().any(|l| l == want), "{want} in {labels:?}");
        }
        p.remove(1);
        rig.frame(vec![], |ui| PairsEditor::new("hd", PairWords::en()).show(ui, &mut p));
        let names: Vec<String> = rig.ids().into_iter().map(|e| e.name).collect();
        assert!(names.iter().any(|n| n == "hd.row.2.name"), "the kept row keeps its key: {names:?}");
        assert!(!names.iter().any(|n| n.starts_with("hd.row.1.")), "{names:?}");
    }

    #[test]
    fn a_test_id_that_is_not_one_is_not_attached() {
        let rig = Rig::new();
        let mut p = Pairs::new([("a", "b")]);
        rig.frame(vec![], |ui| PairsEditor::new("My Headers", PairWords::en()).show(ui, &mut p));
        assert!(rig.ids().is_empty());
    }

    #[test]
    fn add_puts_the_keys_in_the_new_rows_name_box_next_frame() {
        let rig = Rig::new();
        let mut p = Pairs::new([("a", "1")]);
        let mut added = None;
        click(&rig, "hd.add", |rig, ev| {
            let r = rig.step(ev, &mut p, plain);
            added = added.or(r.added);
        });
        assert_eq!(added, Some(1));
        assert_eq!(p.len(), 2);
        assert!(rig.has_focus("hd.row.1.name"));
        rig.step(vec![], &mut p, plain);
        assert!(rig.has_focus("hd.row.1.name"), "the focus stays");
        // Typing goes into the new row.
        rig.step(rig.typed("Zed"), &mut p, plain);
        assert_eq!(p.rows().nth(1).map(|r| r.1), Some("Zed"));
    }

    #[test]
    fn the_focus_is_not_taken_when_nothing_was_added_or_removed() {
        let rig = Rig::new();
        let mut p = Pairs::new([("a", "1")]);
        rig.step(vec![], &mut p, plain);
        rig.focus("elsewhere");
        rig.step(vec![], &mut p, plain);
        rig.step(vec![], &mut p, plain);
        assert!(rig.has_focus("elsewhere"));
        assert!(!rig.has_focus("hd.row.0.name"));
    }

    #[test]
    fn two_editors_on_one_screen_do_not_share_state() {
        let rig = Rig::new();
        let (mut a, mut b) = (Pairs::new([("a", "1")]), Pairs::new([("a", "1")]));
        let draw = |events: Vec<Event>, a: &mut Pairs, b: &mut Pairs| {
            rig.frame(events, |ui| {
                PairsEditor::new("one", PairWords::en()).show(ui, a);
                PairsEditor::new("two", PairWords::en()).show(ui, b);
            });
        };
        draw(vec![], &mut a, &mut b);
        rig.focus("one.row.0.name");
        draw(vec![], &mut a, &mut b);
        draw(vec![Event::Text("x".into())], &mut a, &mut b);
        assert_eq!(a.rows().next().map(|r| r.1), Some("ax"));
        assert_eq!(b.rows().next().map(|r| r.1), Some("a"));
        assert!(rig.has_focus("one.row.0.name") && !rig.has_focus("two.row.0.name"));
        // Add in one: the focus goes to its new row and not to the other editor's.
        let names = rig.ids();
        let at = names.iter().find(|e| e.name == "two.add").expect("two.add").rect.center();
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        for ev in [vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![], vec![]] {
            draw(ev, &mut a, &mut b);
        }
        assert_eq!((a.len(), b.len()), (1, 2));
        assert!(rig.has_focus("two.row.1.name") && !rig.has_focus("one.row.0.name"));
    }

    /// Removes row `key` by a click on its button, with the keys on `focus_on` first (if any).
    fn remove_with_focus(keys: &[(&str, &str)], key: u32, focus_on: Option<&str>) -> (Rig, Pairs) {
        let rig = Rig::new();
        let mut p = Pairs::new(keys.iter().copied());
        rig.step(vec![], &mut p, plain);
        let at = rig.ids().iter().find(|e| e.name == format!("hd.row.{key}.remove")).expect("remove").rect.center();
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        rig.step(vec![Event::PointerMoved(at)], &mut p, plain);
        if let Some(f) = focus_on {
            rig.focus(f);
        }
        rig.step(vec![button(true)], &mut p, plain);
        rig.step(vec![button(false)], &mut p, plain);
        rig.step(vec![], &mut p, plain);
        rig.step(vec![], &mut p, plain);
        (rig, p)
    }

    fn remove_button_focused(rig: &Rig, key: u32) -> bool {
        // The remove buttons are not text boxes: ask the registered rect's widget.
        let e = rig.ids().into_iter().find(|e| e.name == format!("hd.row.{key}.remove"));
        e.is_some_and(|e| rig.ctx.memory(|m| m.has_focus(e.egui_id)))
    }

    #[test]
    fn removing_a_row_moves_the_keys_to_the_next_remove_button_else_the_previous_else_add() {
        let rows = [("a", "1"), ("b", "2"), ("c", "3")];
        // First row, with the keys on it: the next row's button.
        let (rig, p) = remove_with_focus(&rows, 0, Some("hd.row.0.value"));
        assert_eq!(p.rows().map(|r| r.0).collect::<Vec<_>>(), [1, 2]);
        rig.step(vec![], &mut p.clone(), plain);
        assert!(remove_button_focused(&rig, 1));
        // A middle row: the next one.
        let (rig, p) = remove_with_focus(&rows, 1, Some("hd.row.1.name"));
        assert_eq!(p.rows().map(|r| r.0).collect::<Vec<_>>(), [0, 2]);
        assert!(remove_button_focused(&rig, 2));
        // The last row: the previous one.
        let (rig, p) = remove_with_focus(&rows, 2, Some("hd.row.2.value"));
        assert_eq!(p.rows().map(|r| r.0).collect::<Vec<_>>(), [0, 1]);
        assert!(remove_button_focused(&rig, 1));
        // The only row: the add button.
        let (rig, p) = remove_with_focus(&[("a", "1")], 0, Some("hd.row.0.name"));
        assert!(p.is_empty());
        let add = rig.ids().into_iter().find(|e| e.name == "hd.add").expect("add");
        assert!(rig.ctx.memory(|m| m.has_focus(add.egui_id)));
    }

    #[test]
    fn removing_a_row_does_not_take_the_keys_from_elsewhere() {
        let rig = Rig::new();
        let mut p = Pairs::new([("a", "1"), ("b", "2")]);
        rig.step(vec![], &mut p, plain);
        rig.focus("elsewhere");
        rig.step(vec![], &mut p, plain);
        assert!(rig.has_focus("elsewhere"));
        // A screen reader's click on the remove button does not move the keys by itself.
        let target = rig.ids().into_iter().find(|e| e.name == "hd.row.0.remove").expect("remove").egui_id;
        let click = Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            action: egui::accesskit::Action::Click,
            target_tree: egui::accesskit::TreeId::ROOT,
            target_node: egui::accesskit::NodeId(target.value()),
            data: None,
        });
        rig.step(vec![click], &mut p, plain);
        rig.step(vec![], &mut p, plain);
        rig.step(vec![], &mut p, plain);
        assert_eq!(p.len(), 1, "the row is gone");
        assert!(rig.has_focus("elsewhere"), "the keys stay where they were");
        assert!(!remove_button_focused(&rig, 1));
        assert!(!rig.has_focus("hd.row.1.name") && !rig.has_focus("hd.row.1.value"));
    }

    #[test]
    fn validity_is_this_frames_after_typing_and_after_a_remove() {
        let rig = Rig::new();
        let mut p = Pairs::new([("A", "1"), ("B", "2")]);
        let unique = |e: PairsEditor<'static>| e.unique(true);

        assert!(rig.step(vec![], &mut p, unique).valid);
        rig.focus("hd.row.1.name");
        rig.step(vec![], &mut p, unique);
        // Select the name and type over it: "a" repeats "A" when folded.
        rig.step(
            vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }],
            &mut p,
            unique,
        );
        let r = rig.step(rig.typed("a"), &mut p, unique);
        assert_eq!(p.rows().nth(1).map(|r| r.1), Some("a"));
        assert!(!r.valid, "the repeat is seen in the frame it is typed");
        assert!(r.changed);
        // Removing the earlier row in one click makes the later one valid in that very frame.
        let at = rig.ids().iter().find(|e| e.name == "hd.row.0.remove").expect("remove").rect.center();
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        rig.step(vec![Event::PointerMoved(at)], &mut p, unique);
        rig.step(vec![button(true)], &mut p, unique);
        let r = rig.step(vec![button(false)], &mut p, unique);
        assert_eq!(p.len(), 1);
        assert!(r.valid, "the removed row takes no part");
    }

    #[test]
    fn a_nameless_value_blocks_in_the_frame_it_is_typed() {
        let rig = Rig::new();
        let mut p = Pairs::new([("", "")]);
        rig.step(vec![], &mut p, plain);
        rig.focus("hd.row.0.value");
        rig.step(vec![], &mut p, plain);
        let r = rig.step(rig.typed("v"), &mut p, plain);
        assert!(!r.valid);
        rig.focus("hd.row.0.name");
        rig.step(vec![], &mut p, plain);
        assert!(rig.step(rig.typed("N"), &mut p, plain).valid);
    }

    #[test]
    fn the_editor_fits_a_narrow_and_a_wide_room() {
        for (w, over) in [(200.0_f32, 0.0_f32), (260.0, 0.0), (420.0, 0.0), (1000.0, 0.0)] {
            let mut rig = Rig::new();
            rig.width = w;
            let mut p =
                Pairs::new([("Authorization", "secret:api"), ("A very long header name indeed", "a very long value ".repeat(8).as_str())]);
            let bad = |n: &str| n.starts_with("A ").then(|| "bad".to_owned());
            let (right, _) = rig.frame(vec![], |ui| {
                let room = ui.max_rect().right();
                let r = PairsEditor::new("hd", PairWords::ja()).check(&bad).show(ui, &mut p);
                assert!(!r.valid);
                ui.min_rect().right() - room
            });
            assert!(right <= over + 0.5, "width {w}: the editor is {right} wider than its room");
            let names = rig.ids();
            let x_max = names.iter().map(|e| e.rect.right()).fold(0.0_f32, f32::max);
            assert!(x_max <= w + 0.5, "width {w}: a widget ends at {x_max}");
        }
    }

    #[test]
    fn the_empty_editor_says_so_and_still_adds() {
        let rig = Rig::new();
        let mut p = Pairs::default();
        let (_, out) = rig.frame(vec![], |ui| PairsEditor::new("hd", PairWords::ja()).show(ui, &mut p));
        let labels: Vec<String> = out
            .platform_output
            .accesskit_update
            .expect("ak")
            .nodes
            .iter()
            .filter_map(|(_, n)| n.label().or(n.value()).map(str::to_owned))
            .collect();
        assert!(labels.iter().any(|l| l == "まだありません") && labels.iter().any(|l| l == "追加"), "{labels:?}");
        // Add works from the empty editor: one row, and the keys in its name box.
        click(&rig, "hd.add", |rig, ev| {
            rig.step(ev, &mut p, plain);
        });
        assert_eq!(p.len(), 1);
        assert!(rig.has_focus("hd.row.0.name"));
    }
}
