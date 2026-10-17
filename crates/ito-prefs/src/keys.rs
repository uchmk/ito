//! The Keys page every app has (`docs/common-spec.md`): each operation of
//! the app's keymap on a row of its own, its keys beside it, to find, change,
//! add, take away and put back. Nothing is written until a change is set:
//! the page hands back [`Edit`]s, which the app writes into its own
//! keymap.toml with [`ito_keymap::edit`] (off the UI thread) and then reads
//! the keymap again.

use egui::{Color32, FontId, RichText};
use ito_keymap::{clashes, common, rows, Binding, Clash, Edit, Row, State, BUILT_IN};
use ito_keys::{human_seq_for, render_seq, Code, Key, Named};

use crate::{select, switch, z, Look};

/// The words of the Keys page, in one language.
pub struct KeyWords {
    pub search: &'static str,
    /// The button that finds rows by pressing their keys.
    pub by_key: &'static str,
    pub listening: &'static str,
    pub clear: &'static str,
    /// The layer filter's label, and its head in the table.
    pub layer: &'static str,
    pub all: &'static str,
    pub changed_only: &'static str,
    pub operation: &'static str,
    pub keys: &'static str,
    /// Under a changed row: `{0}` is its default keys.
    pub default_is: &'static str,
    pub none: &'static str,
    pub press: &'static str,
    pub hint: &'static str,
    pub special: &'static str,
    pub set: &'static str,
    pub cancel: &'static str,
    pub add: &'static str,
    pub remove: &'static str,
    pub put_back: &'static str,
    pub reset: &'static str,
    pub change: &'static str,
    pub built_in: &'static str,
    /// `{0}` is the app's keymap.toml.
    pub mine: &'static str,
    /// `{0}` is the file the key is written in.
    pub elsewhere: &'static str,
    /// `{0}` is what runs instead.
    pub shadowed: &'static str,
    /// A key with a line that does nothing in front of it.
    pub taken_away: &'static str,
    /// `{0}` the keys, `{1}` the layer, `{2}` what they run now.
    pub clash_same: &'static str,
    /// `{0}` the other keys, `{1}` what they run.
    pub clash_prefix: &'static str,
    /// `{0}` the keys, `{1}` one of [`KeyWords::common_what`].
    pub clash_common: &'static str,
    /// After the first few clashes of the same kind: `{0}` how many more.
    pub more: &'static str,
    /// What each of [`ito_keymap::COMMON`] is for, in its order.
    pub common_what: [&'static str; 9],
    pub nothing: &'static str,
}

pub const KEYS_EN: KeyWords = KeyWords {
    search: "Search operations, commands or keys",
    by_key: "⌨ Find by key",
    listening: "Press keys… (Enter keeps them, Esc stops)",
    clear: "Clear",
    layer: "Where",
    all: "All",
    changed_only: "Changed only",
    operation: "Operation",
    keys: "Keys",
    default_is: "Default: {0}",
    none: "none",
    press: "Press the new keys…",
    hint: "Keys pressed in turn make a sequence · Enter sets · Backspace takes one back · Esc stops",
    special: "Special keys ▾",
    set: "Set",
    cancel: "Cancel",
    add: "Add a key",
    remove: "Remove",
    put_back: "Removed. Click to put it back",
    reset: "Back to the defaults",
    change: "Click to change",
    built_in: "A default key",
    mine: "Changed in {0}",
    elsewhere: "Written in {0}: change it there",
    shadowed: "Not reached: “{0}” runs first on these keys",
    taken_away: "Taken away: a line that does nothing comes first on these keys",
    clash_same: "{0} is on “{2}” in {1}: setting it here takes it from that",
    clash_prefix: "{0} (“{1}”) starts the same way as these keys: setting them takes it from that",
    clash_common: "{0} is the key every app keeps for {1}: it may never reach here",
    more: "…and {0} more like these",
    common_what: [
        "the settings",
        "the help",
        "the command palette",
        "quitting",
        "zooming in",
        "zooming in",
        "zooming in",
        "zooming out",
        "the zoom back to 100%",
    ],
    nothing: "No key matches",
};

pub const KEYS_JA: KeyWords = KeyWords {
    search: "操作・コマンド・キーで検索",
    by_key: "⌨ キーで探す",
    listening: "キーを押して…（Enter で決める · Esc でやめる）",
    clear: "解除",
    layer: "場面",
    all: "すべて",
    changed_only: "変えたものだけ",
    operation: "操作",
    keys: "キー",
    default_is: "既定: {0}",
    none: "なし",
    press: "新しいキーを押して…",
    hint: "続けて押せます · Enter で決める · Backspace で 1 つ戻す · Esc でやめる",
    special: "特別なキー ▾",
    set: "決める",
    cancel: "やめる",
    add: "キーを足す",
    remove: "外す",
    put_back: "外したキー。押すと戻します",
    reset: "既定に戻す",
    change: "押して変える",
    built_in: "既定のキー",
    mine: "{0} で変えたキー",
    elsewhere: "{0} に書かれています。そちらで変えてください",
    shadowed: "届きません。このキーでは先に『{0}』が動きます",
    taken_away: "外されています。このキーには先に何もしない行があります",
    clash_same: "{0} は『{1}』で『{2}』に使っています。決めるとそちらが効かなくなります",
    clash_prefix: "{0}（『{1}』）とは片方がもう片方の頭なので、決めるとそちらが効かなくなります",
    clash_common: "{0} はどのアプリでも{1}に使うキーです。この場面に届かないことがあります",
    more: "ほか {0} 件も同じです",
    common_what: ["設定", "ヘルプ", "コマンドパレット", "終了", "拡大", "拡大", "拡大", "縮小", "倍率を 100% に戻す"],
    nothing: "当てはまるキーはありません",
};

impl KeyWords {
    /// The words for a language code (`ja`, `en`); English for the rest.
    pub fn of(language: &str) -> &'static KeyWords {
        match language {
            "ja" => &KEYS_JA,
            _ => &KEYS_EN,
        }
    }
}

/// The page's words, for the settings' search.
pub fn keys_index(w: &KeyWords) -> Vec<&'static str> {
    vec![w.by_key, w.changed_only, w.layer, w.add, w.remove, w.reset, w.special]
}

/// How many keys a sequence being set starts (or is the start of) are named
/// before the rest are only counted.
const PREFIXES_SHOWN: usize = 3;

/// The keymap the page shows, and the app's ways of naming its parts.
pub struct KeyTable<'a, A> {
    /// The keymap in force, layer by layer ([`ito_keymap::Keymap::layers`]).
    pub now: &'a [(String, Vec<Binding<A>>)],
    /// The built-in defaults alone (the keymap loaded with no files).
    pub built_in: &'a [(String, Vec<Binding<A>>)],
    /// The app's command that does nothing.
    pub noop: &'a str,
    /// The path of the keymap.toml the page's changes go into, as the
    /// keymap names it ([`Binding::from`]). A key from any other file is
    /// shown, but changed there.
    pub own: &'a str,
    /// A layer's name in the person's words.
    pub layer_name: &'a dyn Fn(&str) -> String,
    /// A binding's `desc` in the person's words (the `run` when empty).
    pub tr: &'a dyn Fn(&str) -> String,
    /// More to say about giving a layer some keys (keys the app or what runs
    /// in it keeps for itself), in the person's words.
    pub extra: &'a dyn Fn(&str, &[Key]) -> Vec<String>,
    /// Operations with no key by default, as `(layer, run, desc)`: shown
    /// after the layer's other rows, so a key can be given to them.
    pub unbound: &'a [(&'a str, &'a str, &'a str)],
    /// Cmd on a Mac is its own modifier, written `D`, rather than `C` like
    /// Ctrl (for an app that tells the two apart).
    pub sup: bool,
}

/// The page's own state.
#[derive(Clone, Debug, Default)]
pub struct KeysEdit {
    /// The page's search.
    pub query: String,
    /// The keys the rows are found by (the rows with a key that starts so).
    pub by_key: Vec<Key>,
    /// Keys pressed go into `by_key`.
    pub listening: bool,
    /// The layer filter: a layer's name, or empty for all.
    pub layer: String,
    pub changed_only: bool,
    /// The keys being pressed for an operation.
    pub capture: Option<Capture>,
}

/// Keys being pressed for an operation, not yet set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    pub layer: String,
    pub run: String,
    pub keys: Vec<Key>,
    /// The keys they take the place of; `None` when they are added.
    pub replace: Option<Vec<Key>>,
}

impl KeysEdit {
    /// The page takes every key: hand [`crate::show`] `Keys::default()`
    /// rather than reading the screen's keys, so Esc and Ctrl+Tab are the
    /// page's.
    pub fn busy(&self) -> bool {
        self.capture.is_some() || self.listening
    }
}

/// A key pressed while the page takes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    Key(Key),
    /// Enter, Esc and Backspace with no modifier: set, stop, one back.
    Enter,
    Esc,
    Back,
}

/// The keys the menu puts in: the ones pressed alone set or stop, and the
/// clipboard's, which egui hands over only when there is text to paste (with
/// Cmd on a Mac, `D` where Ctrl and Cmd are apart).
fn special(sup: bool) -> [Key; 7] {
    let named = |n| Key::plain(Code::Named(n));
    let clip = |c| {
        let mut k = Key::ctrl(c);
        if sup && cfg!(target_os = "macos") {
            (k.ctrl, k.sup) = (false, true);
        }
        k
    };
    [named(Named::Enter), named(Named::Esc), named(Named::Backspace), named(Named::Tab), clip('c'), clip('x'), clip('v')]
}

/// The key every app keeps that `keys` starts with: where Ctrl and Cmd are
/// apart on macOS (`sup`), the common keys are Cmd's (`<D-,>`), not Ctrl's.
fn common_of(keys: &[Key], sup: bool) -> Option<usize> {
    if !(sup && cfg!(target_os = "macos")) {
        return common(keys);
    }
    let mut first = *keys.first()?;
    if first.ctrl {
        return None;
    }
    (first.ctrl, first.sup) = (first.sup, false);
    common(&[first])
}

/// The keys pressed this frame, taken out of egui's input so nothing else
/// sees them.
fn presses(ui: &egui::Ui, sup: bool) -> Vec<Press> {
    ui.input_mut(|i| {
        let clip: Vec<_> = (0..i.events.len()).map(|n| ito_keys::from_clipboard_for(&i.events, n, &i.modifiers, sup)).collect();
        let mut clip = clip.into_iter();
        let mut out = Vec::new();
        // The text a key press brings with it when the key is taken already
        // (a character with Alt), or when the key is held down.
        let mut skip_text = false;
        i.events.retain(|e| match (e, clip.next().flatten()) {
            // egui hands these over as the clipboard's, with no key: Ctrl+X,
            // C and V, and on Windows Shift+Delete, Ctrl+Insert (as Ctrl+C
            // here: the two are one to egui) and Shift+Insert.
            (_, Some(k)) => {
                skip_text = false;
                out.push(Press::Key(k));
                false
            }
            (e, None) => match e {
                egui::Event::Key { key, pressed: true, repeat, modifiers, .. } => {
                    // A key held down is pressed once.
                    skip_text = *repeat;
                    if !*repeat {
                        let bare = modifiers.is_none();
                        match key {
                            egui::Key::Enter if bare => out.push(Press::Enter),
                            egui::Key::Escape if bare => out.push(Press::Esc),
                            egui::Key::Backspace if bare => out.push(Press::Back),
                            _ => {
                                if let Some(mut k) = ito_keys::from_egui(*key, modifiers) {
                                    if sup {
                                        k.ctrl = modifiers.ctrl;
                                        k.sup = modifiers.mac_cmd;
                                    }
                                    skip_text = matches!(k.code, Code::Char(_)) && k.alt && !k.ctrl && !k.sup;
                                    out.push(Press::Key(k));
                                }
                            }
                        }
                    }
                    false
                }
                egui::Event::Key { .. } | egui::Event::Ime(_) => false,
                egui::Event::Text(t) => {
                    if !std::mem::take(&mut skip_text) {
                        out.extend(t.chars().filter(|c| !c.is_control()).map(|c| Press::Key(Key::char(c))));
                    }
                    false
                }
                _ => true,
            },
        });
        out
    })
}

/// `s` with `{0}`, `{1}` … filled in.
fn fill(s: &str, args: &[&str]) -> String {
    args.iter().enumerate().fold(s.to_owned(), |s, (k, a)| s.replace(&format!("{{{k}}}"), a))
}

/// The edits that set `cap`, against the row it is for; none when they
/// change nothing.
fn set(cap: &Capture, row: Option<&Row>) -> Vec<Edit> {
    if cap.keys.is_empty() || cap.replace.as_ref() == Some(&cap.keys) {
        return Vec::new();
    }
    let has = row.is_some_and(|r| r.keys.iter().any(|k| k.on == cap.keys && !k.removed && k.state == State::Live));
    let mut out = Vec::new();
    if let Some(old) = &cap.replace {
        out.push(Edit::Remove { layer: cap.layer.clone(), on: old.clone(), run: cap.run.clone() });
    }
    if !has {
        let desc = row.map_or_else(String::new, |r| r.desc.clone());
        out.push(Edit::Add { layer: cap.layer.clone(), on: cap.keys.clone(), run: cap.run.clone(), desc });
    }
    out
}

/// A key's cap: lit while it takes keys, struck through when taken away.
fn chip(ui: &mut egui::Ui, l: Look, words: &str, lit: bool, removed: bool) -> egui::Response {
    let c = l.c;
    let mut text = RichText::new(words).font(FontId::monospace(z(ui, 12.0))).color(if removed { c.faint() } else { c.strong() });
    if removed {
        text = text.strikethrough();
    }
    let stroke = if lit { egui::Stroke::new(1.5, c.run) } else { egui::Stroke::new(1.0, c.border_strong()) };
    ui.add(egui::Button::new(text).fill(if removed { Color32::TRANSPARENT } else { c.side }).stroke(stroke).corner_radius(6.0))
}

/// A small button of one sign, with a tooltip, named by it.
fn sign(ui: &mut egui::Ui, l: Look, s: &str, tip: &str, enabled: bool) -> bool {
    let b = egui::Button::new(RichText::new(s).size(z(ui, 12.5)).color(l.c.dim)).fill(Color32::TRANSPARENT).frame(false);
    let r = ui.add_enabled(enabled, b);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    let r = if enabled { r.on_hover_text(tip) } else { r.on_disabled_hover_text(tip) };
    r.clicked()
}

/// The Keys page: the search and filters, then the table. What is set hands
/// back the edits to write; `None` the rest of the time.
pub fn keys_page<A>(ui: &mut egui::Ui, l: Look, w: &KeyWords, t: &KeyTable<A>, st: &mut KeysEdit) -> Option<Vec<Edit>> {
    let c = l.c;
    let mut all = rows(t.now, t.built_in, t.noop);
    for &(layer, run, desc) in t.unbound {
        if all.iter().any(|r| r.layer == layer && r.run == run) {
            continue;
        }
        let at = all.iter().rposition(|r| r.layer == layer).map_or(all.len(), |k| k + 1);
        let row = Row { layer: layer.to_owned(), run: run.to_owned(), desc: desc.to_owned(), keys: Vec::new(), defaults: Vec::new() };
        all.insert(at, row);
    }
    let mut out = Vec::new();

    // Keys pressed for a row no longer there (the keymap was read again)
    // are for nothing; for a key no longer on it, they are added instead.
    if let Some(cap) = &st.capture {
        let row = all.iter().find(|r| r.layer == cap.layer && r.run == cap.run);
        match row.map(|r| cap.replace.as_ref().is_none_or(|old| r.keys.iter().any(|k| &k.on == old && !k.removed))) {
            None => st.capture = None,
            Some(false) => st.capture.as_mut().expect("checked").replace = None,
            Some(true) => {}
        }
    }

    // The keys first, before anything drawn here can see them.
    if st.busy() {
        ui.memory_mut(|m| {
            if let Some(f) = m.focused() {
                m.surrender_focus(f);
            }
        });
        for p in presses(ui, t.sup) {
            if let Some(cap) = &mut st.capture {
                match p {
                    Press::Key(k) => cap.keys.push(k),
                    Press::Back => {
                        cap.keys.pop();
                    }
                    Press::Esc => st.capture = None,
                    Press::Enter => {
                        let row = all.iter().find(|r| r.layer == cap.layer && r.run == cap.run);
                        out.extend(set(cap, row));
                        st.capture = None;
                    }
                }
            } else if st.listening {
                match p {
                    Press::Key(k) => st.by_key.push(k),
                    Press::Back => {
                        st.by_key.pop();
                    }
                    Press::Esc => {
                        st.listening = false;
                        st.by_key.clear();
                    }
                    Press::Enter => st.listening = false,
                }
            }
        }
    }

    let mut layers: Vec<&str> = Vec::new();
    for r in &all {
        if !layers.contains(&r.layer.as_str()) {
            layers.push(&r.layer);
        }
    }
    let one_layer = layers.len() <= 1;
    // The filter's place in the menu: 0 for all.
    let layer_at = layers.iter().position(|n| *n == st.layer).map_or(0, |k| k + 1);

    // The search and the filters. Wrapped: the words while keys are taken
    // are long, and a narrow page puts them on a line of their own.
    ui.horizontal_wrapped(|ui| {
        let search = egui::TextEdit::singleline(&mut st.query)
            .id(egui::Id::new("prefs-keys-search"))
            .hint_text(w.search)
            .desired_width(300.0)
            .margin(egui::vec2(10.0, 6.0))
            .font(FontId::proportional(z(ui, 12.5)));
        let r = ui.add(search);
        crate::focus_ring(ui, &r);
        ui.add_space(8.0);
        if st.listening {
            let words = if st.by_key.is_empty() { w.listening.to_owned() } else { format!("{} …", human_seq_for(&st.by_key, t.sup)) };
            chip(ui, l, &words, true, false);
            if crate::button(ui, l, w.clear) {
                st.listening = false;
                st.by_key.clear();
            }
        } else {
            if crate::button(ui, l, w.by_key) {
                st.listening = true;
                st.by_key.clear();
                st.capture = None;
            }
            if !st.by_key.is_empty() {
                chip(ui, l, &human_seq_for(&st.by_key, t.sup), false, false);
                if crate::button(ui, l, w.clear) {
                    st.by_key.clear();
                }
            }
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if !one_layer {
            ui.label(RichText::new(w.layer).size(z(ui, 12.5)).color(c.dim));
            let names: Vec<String> = std::iter::once(w.all.to_owned()).chain(layers.iter().map(|n| (t.layer_name)(n))).collect();
            let options: Vec<(usize, &str)> = names.iter().enumerate().map(|(k, n)| (k, n.as_str())).collect();
            if let Some(k) = select(ui, "keys-layer", layer_at, &options) {
                st.layer = if k == 0 { String::new() } else { layers[k - 1].to_owned() };
            }
            ui.add_space(16.0);
        }
        if switch(ui, l, st.changed_only) {
            st.changed_only = !st.changed_only;
        }
        ui.label(RichText::new(w.changed_only).size(z(ui, 12.5)).color(c.fg));
    });
    ui.add_space(12.0);

    // The rows shown, with the words each is found by.
    let q = st.query.trim().to_lowercase();
    let shown: Vec<(&Row, String, String, String)> = all
        .iter()
        .filter_map(|r| {
            let desc = if r.desc.is_empty() { r.run.clone() } else { (t.tr)(&r.desc) };
            let layer = (t.layer_name)(&r.layer);
            let keys: Vec<String> = r.keys.iter().flat_map(|k| [human_seq_for(&k.on, t.sup), render_seq(&k.on)]).collect();
            let hay = format!("{desc}\n{}\n{}\n{layer}\n{}", r.desc, r.run, keys.join("\n")).to_lowercase();
            let capturing = st.capture.as_ref().is_some_and(|cap| cap.layer == r.layer && cap.run == r.run);
            let keep = capturing
                || ((layer_at == 0 || r.layer == st.layer)
                    && (!st.changed_only || r.changed())
                    && (q.is_empty() || hay.contains(&q))
                    && (st.by_key.is_empty() || r.keys.iter().any(|k| k.on.starts_with(&st.by_key))));
            keep.then_some((r, desc, layer, hay))
        })
        .collect();

    egui::Frame::NONE
        .fill(c.panel)
        .stroke(egui::Stroke::new(1.0, c.border))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(18, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let width = ui.available_width();
            let gap = 12.0;
            // Narrow panes take from the operation and the layer first.
            let layer_w = if one_layer { 0.0 } else { 110.0_f32.min(width * 0.2) };
            let op_w = (width * 0.34).max(160.0_f32.min(width * 0.4));
            let keys_w = (width - op_w - layer_w - if one_layer { gap } else { 2.0 * gap }).max(60.0);
            let head = |ui: &mut egui::Ui, words: &str, w: f32| {
                ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(w);
                    ui.label(RichText::new(words).size(z(ui, 11.0)).strong().color(c.dim));
                });
            };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                head(ui, w.operation, op_w);
                head(ui, w.keys, keys_w);
                if !one_layer {
                    head(ui, w.layer, layer_w);
                }
            });
            ui.add_space(4.0);
            if shown.is_empty() {
                crate::sep(ui, l);
                ui.add_space(10.0);
                ui.label(RichText::new(w.nothing).size(z(ui, 12.5)).color(c.dim));
                ui.add_space(8.0);
            }
            for (row, desc, layer, hay) in &shown {
                crate::sep(ui, l);
                let lit = !l.q.is_empty() && hay.contains(l.q);
                let behind = ui.painter().add(egui::Shape::Noop);
                let drawn = ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.allocate_ui_with_layout(egui::vec2(op_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(op_w);
                        ui.add_space(8.0);
                        ui.label(RichText::new(desc.as_str()).size(z(ui, 13.0)).color(c.strong()));
                        if !row.desc.is_empty() {
                            ui.add(
                                egui::Label::new(RichText::new(&row.run).font(FontId::monospace(z(ui, 11.0))).color(c.faint())).truncate(),
                            );
                        }
                        ui.add_space(6.0);
                    });
                    ui.allocate_ui_with_layout(egui::vec2(keys_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(keys_w);
                        ui.add_space(6.0);
                        keys_cell(ui, l, w, t, row, layer, st, &mut out);
                        ui.add_space(6.0);
                    });
                    if !one_layer {
                        ui.allocate_ui_with_layout(egui::vec2(layer_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(layer_w);
                            ui.add_space(9.0);
                            ui.label(RichText::new(layer.as_str()).size(z(ui, 12.5)).color(c.dim));
                        });
                    }
                });
                if lit {
                    let rect = drawn.response.rect.expand2(egui::vec2(10.0, 0.0));
                    ui.painter().set(behind, egui::Shape::rect_filled(rect, 6.0, c.wait.gamma_multiply(0.14)));
                }
            }
        });
    (!out.is_empty()).then_some(out)
}

/// The keys of a row: a cap for each, ＋ to add one, ↺ when it was changed;
/// the defaults under it when it was, and while keys are pressed for it,
/// what they are and what they take.
#[allow(clippy::too_many_arguments)]
fn keys_cell<A>(ui: &mut egui::Ui, l: Look, w: &KeyWords, t: &KeyTable<A>, row: &Row, layer: &str, st: &mut KeysEdit, out: &mut Vec<Edit>) {
    let c = l.c;
    let own = |from: &str| from == t.own;
    let editable = |from: &str| from == BUILT_IN || own(from);
    let this = st.capture.as_ref().filter(|cap| cap.layer == row.layer && cap.run == row.run).cloned();
    let tr_desc = |desc: &str, run: &str| if desc.is_empty() { run.to_owned() } else { (t.tr)(desc) };
    let capture_chip = |ui: &mut egui::Ui, cap: &Capture| {
        let words = if cap.keys.is_empty() { w.press.to_owned() } else { format!("{} …", human_seq_for(&cap.keys, t.sup)) };
        chip(ui, l, &words, true, false);
    };
    let start = |st: &mut KeysEdit, replace: Option<Vec<Key>>| {
        st.capture = Some(Capture { layer: row.layer.clone(), run: row.run.clone(), keys: Vec::new(), replace });
        st.listening = false;
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for cell in &row.keys {
            if this.as_ref().is_some_and(|cap| cap.replace.as_ref() == Some(&cell.on)) {
                capture_chip(ui, this.as_ref().expect("checked"));
                continue;
            }
            let from = &*cell.from;
            // What a press of it reaches first, when not this.
            let first = match &cell.state {
                State::Shadowed { run, desc, from } => Some((run, desc, &**from)),
                State::Live => None,
            };
            // A default taken away comes back by taking out what the page's
            // file put in front of it; another file's is changed there.
            let back = cell.removed && first.is_some_and(|(_, _, f)| own(f));
            let mut words = human_seq_for(&cell.on, t.sup);
            if from != BUILT_IN {
                words.push_str(" •");
            }
            if first.is_some() && !cell.removed {
                words.push_str(" ⚠");
            }
            let mut tip = vec![render_seq(&cell.on)];
            tip.push(if from == BUILT_IN {
                w.built_in.to_owned()
            } else if own(from) {
                fill(w.mine, &[from])
            } else {
                fill(w.elsewhere, &[from])
            });
            match first {
                Some((_, _, f)) if cell.removed && !back => tip.push(fill(w.elsewhere, &[f])),
                _ if cell.removed => tip.push(w.put_back.to_owned()),
                Some((run, _, _)) if run == t.noop => tip.push(w.taken_away.to_owned()),
                Some((run, desc, _)) => tip.push(fill(w.shadowed, &[&tr_desc(desc, run)])),
                None => {}
            }
            if editable(from) && !cell.removed {
                tip.push(w.change.to_owned());
            }
            let r = chip(ui, l, &words, false, cell.removed).on_hover_text(tip.join("\n"));
            if r.clicked() {
                if back {
                    out.push(Edit::Add { layer: row.layer.clone(), on: cell.on.clone(), run: row.run.clone(), desc: row.desc.clone() });
                    st.capture = None;
                } else if editable(from) && !cell.removed {
                    start(st, Some(cell.on.clone()));
                }
            }
            if !cell.removed && editable(from) && sign(ui, l, "×", w.remove, true) {
                out.push(Edit::Remove { layer: row.layer.clone(), on: cell.on.clone(), run: row.run.clone() });
                st.capture = None;
            }
        }
        match &this {
            Some(cap) => {
                if cap.replace.is_none() {
                    capture_chip(ui, cap);
                }
                ui.menu_button(RichText::new(w.special).size(z(ui, 12.0)).color(c.fg), |ui| {
                    for k in special(t.sup) {
                        if ui.button(k.human_for(t.sup)).clicked() {
                            if let Some(cap) = &mut st.capture {
                                cap.keys.push(k);
                            }
                            ui.close();
                        }
                    }
                });
                if crate::button(ui, l, w.set) {
                    out.extend(set(cap, Some(row)));
                    st.capture = None;
                }
                if crate::button(ui, l, w.cancel) {
                    st.capture = None;
                }
            }
            None => {
                if sign(ui, l, "＋", w.add, true) {
                    start(st, None);
                }
            }
        }
        if row.changed() {
            // The file's lines about the row are the page's to take out;
            // other files' are not.
            let mine = row.keys.iter().any(|k| own(&k.from) || matches!(&k.state, State::Shadowed { from, .. } if own(from)));
            let elsewhere = row.keys.iter().map(|k| &*k.from).find(|f| *f != BUILT_IN && !own(f)).unwrap_or(t.own);
            let tip = if mine { w.reset.to_owned() } else { fill(w.elsewhere, &[elsewhere]) };
            if sign(ui, l, "↺", &tip, mine) {
                out.push(Edit::Reset { layer: row.layer.clone(), run: row.run.clone() });
                st.capture = None;
            }
        }
    });
    if row.changed() {
        let d: Vec<String> = row.defaults.iter().map(|d| human_seq_for(d, t.sup)).collect();
        let d = if d.is_empty() { w.none.to_owned() } else { d.join(", ") };
        ui.label(RichText::new(fill(w.default_is, &[&d])).size(z(ui, 11.5)).color(c.faint()));
    }
    if let Some(cap) = &this {
        ui.label(RichText::new(w.hint).size(z(ui, 11.5)).color(c.dim));
        if !cap.keys.is_empty() {
            let bindings: &[Binding<A>] = t.now.iter().find(|(n, _)| *n == cap.layer).map_or(&[], |(_, b)| b);
            let keys = human_seq_for(&cap.keys, t.sup);
            let mut said: Vec<String> = Vec::new();
            let mut prefixes = 0;
            for cl in clashes(bindings, &cap.keys, &cap.run, t.noop) {
                match cl {
                    Clash::Same { run, desc } => said.push(fill(w.clash_same, &[&keys, layer, &tr_desc(&desc, &run)])),
                    // `g` alone is the head of a dozen keys: name a few.
                    Clash::Prefix { run, desc, on } => {
                        prefixes += 1;
                        if prefixes <= PREFIXES_SHOWN {
                            said.push(fill(w.clash_prefix, &[&human_seq_for(&on, t.sup), &tr_desc(&desc, &run)]));
                        }
                    }
                }
            }
            if prefixes > PREFIXES_SHOWN {
                said.push(fill(w.more, &[&(prefixes - PREFIXES_SHOWN).to_string()]));
            }
            if let Some(k) = common_of(&cap.keys, t.sup) {
                let first = human_seq_for(&cap.keys[..1], t.sup);
                said.push(fill(w.clash_common, &[&first, w.common_what[k]]));
            }
            said.extend((t.extra)(&cap.layer, &cap.keys));
            for s in said {
                ui.label(RichText::new(s).size(z(ui, 12.0)).color(c.wait));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{show, Keys, Nav, State as Screen, EN};
    use ito_keymap::{load, Layer, Spec};

    const DEFAULTS: &str = r#"
[[list.keymap]]
on = "y"
run = "copy"
desc = "Copy"

[[list.keymap]]
on = "p"
run = "paste"
desc = "Paste"

[[list.keymap]]
on = "g g"
run = "top"
desc = "Go to the top"

[[list.keymap]]
on = "<C-v>"
run = "visual"
desc = "Visual mode"

[[help.keymap]]
on = "<Esc>"
run = "close"
desc = "Close the help"
"#;

    const SPEC: Spec = Spec { layers: &[Layer::new("list"), Layer::new("help")], ignored: &[], defaults: DEFAULTS };
    const OWN: &str = "C:/u/keymap.toml";

    type Layers = Vec<(String, Vec<Binding<String>>)>;

    type Drawn = (Vec<(String, egui::Rect)>, Option<Vec<Edit>>);

    fn keymap(files: &[(&str, &str)]) -> (Layers, Layers) {
        let mut cmd = |s: &str, _: &mut Vec<String>| Some(s.to_owned());
        let now = load(&SPEC, files, &mut cmd).layers;
        let built_in = load(&SPEC, &[], &mut cmd).layers;
        (now, built_in)
    }

    /// One frame of the page with the page's file: the texts drawn and
    /// where, and the edits.
    fn page(ctx: &egui::Context, file: &str, st: &mut KeysEdit, events: Vec<egui::Event>) -> Drawn {
        frame(ctx, &[(OWN, file)], st, events, egui::Modifiers::NONE)
    }

    /// One frame of the page with these files, and these modifiers held.
    fn frame(
        ctx: &egui::Context,
        files: &[(&str, &str)],
        st: &mut KeysEdit,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> Drawn {
        frame_with(ctx, files, st, events, modifiers, &[], false)
    }

    /// [`frame`] with operations that have no key, and Cmd told from Ctrl.
    fn frame_with(
        ctx: &egui::Context,
        files: &[(&str, &str)],
        st: &mut KeysEdit,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
        unbound: &[(&str, &str, &str)],
        sup: bool,
    ) -> Drawn {
        let (now, built_in) = keymap(files);
        let layer_name = |n: &str| if n == "list" { "List".to_owned() } else { "Help".to_owned() };
        let tr = |d: &str| d.to_owned();
        let extra = |_: &str, _: &[Key]| Vec::new();
        let t = KeyTable {
            now: &now,
            built_in: &built_in,
            noop: "noop",
            own: OWN,
            layer_name: &layer_name,
            tr: &tr,
            extra: &extra,
            unbound,
            sup,
        };
        let nav = Nav { pages: &[("Keys", "")], index: &[], words: &EN, file: "config.toml" };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1600.0))),
            events: [vec![egui::Event::ModifiersChanged(modifiers)], events].concat(),
            ..Default::default()
        };
        let mut screen = Screen::default();
        let mut edits = None;
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(ui, ito_theme::colors(), &mut screen, &nav, Keys::default(), |ui, _, l| {
                    edits = keys_page(ui, l, &KEYS_EN, &t, st);
                });
            });
        });
        out.textures_delta.clear();
        fn walk(s: &egui::Shape, v: &mut Vec<(String, egui::Rect)>) {
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
        (texts, edits)
    }

    fn at(texts: &[(String, egui::Rect)], words: &str) -> Option<egui::Rect> {
        texts.iter().find(|(t, _)| t == words).map(|(_, r)| *r)
    }

    fn has(texts: &[(String, egui::Rect)], words: &str) -> bool {
        texts.iter().any(|(t, _)| t.contains(words))
    }

    fn click(p: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(p), button(true), button(false)]
    }

    /// A key pressed and let go: egui takes one held down as repeated.
    fn key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        vec![event(true), event(false)]
    }

    fn keys(s: &str) -> Vec<Key> {
        s.split(' ').map(|t| Key::parse(t).unwrap()).collect()
    }

    fn adding(run: &str, layer: &str) -> KeysEdit {
        KeysEdit { capture: Some(Capture { layer: layer.into(), run: run.into(), keys: Vec::new(), replace: None }), ..Default::default() }
    }

    #[test]
    fn every_operation_is_a_row_and_the_search_finds_them() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        for words in ["Copy", "Paste", "Go to the top", "Close the help", "g g", "Ctrl+V", "Esc", "List", "Help", "Operation"] {
            assert!(at(&texts, words).is_some(), "{words} is on the page");
        }
        st.query = "past".into();
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        assert!(at(&texts, "Paste").is_some());
        assert!(at(&texts, "Copy").is_none(), "the search leaves the rest out");
        st.query = "nothing like it".into();
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        assert!(at(&texts, KEYS_EN.nothing).is_some());
    }

    #[test]
    fn rows_are_found_by_pressing_their_keys() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { listening: true, ..Default::default() };
        page(&ctx, "", &mut st, vec![egui::Event::Text("g".into())]);
        assert_eq!(st.by_key, keys("g"));
        let (texts, _) = page(&ctx, "", &mut st, key(egui::Key::Enter, egui::Modifiers::NONE));
        assert!(!st.listening);
        assert!(at(&texts, "Go to the top").is_some());
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        assert!(at(&texts, "Copy").is_none(), "only the rows with a key that starts with g");
    }

    #[test]
    fn an_operation_with_no_key_is_a_row_to_give_one() {
        let ctx = egui::Context::default();
        let unbound = [("list", "rename", "Rename"), ("list", "copy", "Copy again"), ("help", "scroll", "Scroll")];
        let mut st = KeysEdit::default();
        let (texts, _) = frame_with(&ctx, &[(OWN, "")], &mut st, Vec::new(), egui::Modifiers::NONE, &unbound, false);
        let rename = at(&texts, "Rename").expect("a row with no key");
        assert!(at(&texts, "Copy again").is_none(), "an operation with keys is its own row");
        assert!(rename.top() > at(&texts, "Visual mode").unwrap().top(), "after its layer's rows");
        assert!(rename.top() < at(&texts, "Close the help").unwrap().top(), "before the next layer's");
        assert!(at(&texts, "Scroll").unwrap().top() > at(&texts, "Close the help").unwrap().top());

        let mut st = adding("rename", "list");
        let events = [vec![egui::Event::Text("r".into())], key(egui::Key::Enter, egui::Modifiers::NONE)].concat();
        let (_, edits) = frame_with(&ctx, &[(OWN, "")], &mut st, events, egui::Modifiers::NONE, &unbound, false);
        let add = Edit::Add { layer: "list".into(), on: keys("r"), run: "rename".into(), desc: "Rename".into() };
        assert_eq!(edits, Some(vec![add]));
    }

    #[test]
    fn cmd_is_its_own_key_when_the_app_tells_it_from_ctrl() {
        let ctx = egui::Context::default();
        let cmd = egui::Modifiers { mac_cmd: true, command: true, ..Default::default() };
        let mut st = adding("paste", "list");
        frame_with(&ctx, &[(OWN, "")], &mut st, key(egui::Key::K, cmd), cmd, &[], true);
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<D-k>"));
        let mut st = adding("paste", "list");
        frame_with(&ctx, &[(OWN, "")], &mut st, key(egui::Key::K, cmd), cmd, &[], false);
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<C-k>"), "Cmd is Ctrl to the rest");
        let ctrl = egui::Modifiers::CTRL;
        let mut st = adding("paste", "list");
        frame_with(&ctx, &[(OWN, "")], &mut st, key(egui::Key::K, ctrl), ctrl, &[], true);
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<C-k>"));
    }

    #[test]
    fn keys_are_written_only_when_set() {
        let ctx = egui::Context::default();
        let mut st = adding("paste", "list");
        let shift_ctrl = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
        let (texts, edits) = page(&ctx, "", &mut st, key(egui::Key::Enter, shift_ctrl));
        assert!(edits.is_none(), "pressed is not set");
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<C-S-Enter>"));
        assert!(has(&texts, KEYS_EN.hint));
        let (_, edits) = page(&ctx, "", &mut st, key(egui::Key::Enter, egui::Modifiers::NONE));
        let add = Edit::Add { layer: "list".into(), on: keys("<C-S-Enter>"), run: "paste".into(), desc: "Paste".into() };
        assert_eq!(edits, Some(vec![add]));
        assert!(st.capture.is_none());
    }

    #[test]
    fn a_sequence_is_pressed_in_turn_and_backspace_takes_one_back() {
        let ctx = egui::Context::default();
        let mut st = adding("copy", "list");
        page(&ctx, "", &mut st, vec![egui::Event::Text("c".into()), egui::Event::Text("x".into())]);
        page(&ctx, "", &mut st, [key(egui::Key::Backspace, egui::Modifiers::NONE), vec![egui::Event::Text("c".into())]].concat());
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("c c"));
        let (_, edits) = page(&ctx, "", &mut st, key(egui::Key::Escape, egui::Modifiers::NONE));
        assert!(edits.is_none() && st.capture.is_none(), "Esc stops with nothing written");
    }

    #[test]
    fn a_clash_is_said_before_it_is_set() {
        let ctx = egui::Context::default();
        let mut st = adding("paste", "list");
        let (texts, _) = frame(&ctx, &[(OWN, "")], &mut st, vec![egui::Event::Paste(String::new())], egui::Modifiers::CTRL);
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<C-v>"), "Ctrl+V comes as the clipboard's");
        assert!(has(&texts, "Ctrl+V is on “Visual mode” in List"));
        let mut st = adding("paste", "list");
        let (texts, _) = page(&ctx, "", &mut st, vec![egui::Event::Text("g".into())]);
        assert!(has(&texts, "g g (“Go to the top”) starts the same way"));
        let mut st = adding("paste", "list");
        let (texts, _) = page(&ctx, "", &mut st, key(egui::Key::F1, egui::Modifiers::NONE));
        assert!(has(&texts, "F1 is the key every app keeps for the help"));
    }

    #[test]
    fn a_few_keys_it_starts_are_named_and_the_rest_counted() {
        let ctx = egui::Context::default();
        let file: String = ["a", "b", "c", "d"]
            .iter()
            .map(|k| format!("[[list.prepend_keymap]]\non = \"g {k}\"\nrun = \"go {k}\"\ndesc = \"Go {k}\"\n"))
            .collect();
        let mut st = adding("paste", "list");
        let (texts, _) = page(&ctx, &file, &mut st, vec![egui::Event::Text("g".into())]);
        let named = texts.iter().filter(|(t, _)| t.contains("starts the same way")).count();
        assert_eq!(named, PREFIXES_SHOWN);
        assert!(has(&texts, "…and 2 more like these"), "five keys start with g");
    }

    #[test]
    fn a_special_key_comes_from_the_menu() {
        let ctx = egui::Context::default();
        let mut st = adding("copy", "list");
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        page(&ctx, "", &mut st, click(at(&texts, KEYS_EN.special).unwrap().center()));
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        page(&ctx, "", &mut st, click(at(&texts, "Tab").unwrap().center()));
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<Tab>"));
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        let (_, edits) = page(&ctx, "", &mut st, click(at(&texts, KEYS_EN.set).unwrap().center()));
        let add = Edit::Add { layer: "list".into(), on: keys("<Tab>"), run: "copy".into(), desc: "Copy".into() };
        assert_eq!(edits, Some(vec![add]));
    }

    #[test]
    fn a_changed_row_shows_its_defaults_and_goes_back() {
        let ctx = egui::Context::default();
        let file = "[[list.prepend_keymap]]\non = \"<C-p>\"\nrun = \"paste\"\n\n[[list.prepend_keymap]]\non = \"p\"\nrun = \"noop\"\n";
        let mut st = KeysEdit::default();
        let (texts, _) = page(&ctx, file, &mut st, Vec::new());
        assert!(at(&texts, "Ctrl+P •").is_some(), "a key of the file is marked");
        assert!(at(&texts, "Default: p").is_some());
        assert!(at(&texts, "p").is_some(), "the default taken away is still shown");
        // ↺ is the only one: the rest are unchanged.
        let (_, edits) = page(&ctx, file, &mut st, click(at(&texts, "↺").unwrap().center()));
        assert_eq!(edits, Some(vec![Edit::Reset { layer: "list".into(), run: "paste".into() }]));
        // The struck-through default puts it back.
        let (_, edits) = page(&ctx, file, &mut st, click(at(&texts, "p").unwrap().center()));
        let add = Edit::Add { layer: "list".into(), on: keys("p"), run: "paste".into(), desc: "Paste".into() };
        assert_eq!(edits, Some(vec![add]));
        st.changed_only = true;
        let (texts, _) = page(&ctx, file, &mut st, Vec::new());
        assert!(at(&texts, "Copy").is_none() && at(&texts, "Paste").is_some());
    }

    #[test]
    fn a_key_is_changed_and_taken_away() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        page(&ctx, "", &mut st, click(at(&texts, "y").unwrap().center()));
        assert_eq!(st.capture.as_ref().unwrap().replace, Some(keys("y")));
        let (_, edits) =
            page(&ctx, "", &mut st, [vec![egui::Event::Text("Y".into())], key(egui::Key::Enter, egui::Modifiers::NONE)].concat());
        let remove = Edit::Remove { layer: "list".into(), on: keys("y"), run: "copy".into() };
        let add = Edit::Add { layer: "list".into(), on: keys("Y"), run: "copy".into(), desc: "Copy".into() };
        assert_eq!(edits, Some(vec![remove.clone(), add]));
        // × beside y.
        let y = at(&texts, "y").unwrap();
        let x = texts
            .iter()
            .filter(|(t, r)| t == "×" && r.left() > y.right() && (r.center().y - y.center().y).abs() < 8.0)
            .map(|(_, r)| *r)
            .next();
        let (_, edits) = page(&ctx, "", &mut st, click(x.unwrap().center()));
        assert_eq!(edits, Some(vec![remove]));
    }

    #[test]
    fn keys_from_another_file_are_not_the_page_s_to_change() {
        let ctx = egui::Context::default();
        let yazi = ("C:/yazi/keymap.toml", "[[list.prepend_keymap]]\non = \"Y\"\nrun = \"copy\"\n");
        let (now, built_in) = keymap(&[yazi]);
        let rows = rows(&now, &built_in, "noop");
        assert!(rows.iter().find(|r| r.run == "copy").unwrap().changed());
        // Clicking it starts nothing.
        let mut st = KeysEdit::default();
        let (texts, _) = frame(&ctx, &[yazi], &mut st, Vec::new(), egui::Modifiers::NONE);
        frame(&ctx, &[yazi], &mut st, click(at(&texts, "Y •").unwrap().center()), egui::Modifiers::NONE);
        assert!(st.capture.is_none());
    }

    #[test]
    fn a_default_another_file_took_away_is_put_back_there() {
        let ctx = egui::Context::default();
        let yazi = ("C:/yazi/keymap.toml", "[[list.prepend_keymap]]\non = \"p\"\nrun = \"noop\"\n");
        let mut st = KeysEdit::default();
        let (texts, _) = frame(&ctx, &[yazi, (OWN, "")], &mut st, Vec::new(), egui::Modifiers::NONE);
        let (_, edits) = frame(&ctx, &[yazi, (OWN, "")], &mut st, click(at(&texts, "p").unwrap().center()), egui::Modifiers::NONE);
        assert!(edits.is_none() && st.capture.is_none(), "the page's file has nothing to take out");
    }

    #[test]
    fn a_key_held_down_is_pressed_once() {
        let ctx = egui::Context::default();
        let mut st = adding("copy", "list");
        let down =
            || egui::Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let a = || egui::Event::Text("a".into());
        page(&ctx, "", &mut st, vec![down(), a(), down(), a(), down(), a()]);
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("a"));
    }

    #[test]
    fn modifiers_and_text_in_one_frame_are_each_one_key() {
        let ctx = egui::Context::default();
        let mut st = adding("copy", "list");
        let ev = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        // Ctrl+K brings no text; the x after it is a key of its own.
        page(&ctx, "", &mut st, vec![ev(egui::Key::K, egui::Modifiers::CTRL), egui::Event::Text("x".into())]);
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<C-k> x"));
        // Alt+A may bring its character too: it is one key.
        let mut st = adding("copy", "list");
        page(&ctx, "", &mut st, vec![ev(egui::Key::A, egui::Modifiers::ALT), egui::Event::Text("a".into())]);
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<A-a>"));
        // Tab and Shift+Tab are keys too; IME text is not.
        let mut st = adding("copy", "list");
        let ime = egui::Event::Ime(egui::ImeEvent::Commit("あ".into()));
        page(
            &ctx,
            "",
            &mut st,
            [key(egui::Key::Tab, egui::Modifiers::NONE), key(egui::Key::Tab, egui::Modifiers::SHIFT), vec![ime]].concat(),
        );
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<Tab> <BackTab>"));
    }

    #[test]
    fn the_clipboard_s_keys_are_told_apart() {
        let ctx = egui::Context::default();
        let shift = egui::Modifiers::SHIFT;
        let ctrl = egui::Modifiers::CTRL;
        for (event, mods, on) in [
            (egui::Event::Paste("t".into()), shift, "<S-Insert>"),
            (egui::Event::Cut, shift, "<S-Delete>"),
            (egui::Event::Copy, ctrl, "<C-c>"),
            (egui::Event::Cut, ctrl, "<C-x>"),
            (egui::Event::Paste("t".into()), ctrl | shift, "<C-S-v>"),
        ] {
            let mut st = adding("copy", "list");
            frame(&ctx, &[(OWN, "")], &mut st, vec![event], mods);
            assert_eq!(st.capture.as_ref().unwrap().keys, keys(on), "{on}");
        }
        // Let go of before the frame (the real machine: Ctrl+Shift+V came out
        // as Shift+Insert): the release of the key still has the modifiers.
        let mut st = adding("copy", "list");
        let up = egui::Event::Key { key: egui::Key::V, physical_key: None, pressed: false, repeat: false, modifiers: ctrl | shift };
        frame(&ctx, &[(OWN, "")], &mut st, vec![egui::Event::Paste("t".into()), up], egui::Modifiers::NONE);
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("<C-S-v>"));
    }

    #[test]
    fn keys_pressed_never_reach_the_search() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        let search = at(&texts, KEYS_EN.search).unwrap().center();
        page(&ctx, "", &mut st, click(search));
        page(&ctx, "", &mut st, vec![egui::Event::Text("p".into())]);
        assert_eq!(st.query, "p", "the search has the focus");
        st.capture = adding("copy", "list").capture;
        page(&ctx, "", &mut st, vec![egui::Event::Text("q".into())]);
        page(&ctx, "", &mut st, vec![egui::Event::Text("r".into())]);
        assert_eq!(st.query, "p");
        assert_eq!(st.capture.as_ref().unwrap().keys, keys("q r"));
    }

    #[test]
    fn the_row_keys_are_pressed_for_stays_in_sight() {
        let ctx = egui::Context::default();
        let mut st = adding("paste", "list");
        st.query = "copy".into();
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        assert!(at(&texts, "Paste").is_some() && at(&texts, "Copy").is_some());
        assert!(at(&texts, "Go to the top").is_none());
    }

    #[test]
    fn keys_pressed_for_what_is_gone_are_dropped() {
        let ctx = egui::Context::default();
        // The row is no longer there.
        let mut st = adding("gone", "list");
        page(&ctx, "", &mut st, Vec::new());
        assert!(st.capture.is_none() && !st.busy());
        // The key it was to replace is no longer on it: the keys are added.
        let mut st = adding("copy", "list");
        st.capture.as_mut().unwrap().replace = Some(keys("z"));
        let (_, edits) =
            page(&ctx, "", &mut st, [vec![egui::Event::Text("w".into())], key(egui::Key::Enter, egui::Modifiers::NONE)].concat());
        let add = Edit::Add { layer: "list".into(), on: keys("w"), run: "copy".into(), desc: "Copy".into() };
        assert_eq!(edits, Some(vec![add]));
        // × while keys are pressed for the row stops them.
        let mut st = adding("copy", "list");
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        let y = at(&texts, "y").unwrap();
        let x = texts.iter().find(|(t, r)| t == "×" && (r.center().y - y.center().y).abs() < 8.0).unwrap().1;
        let (_, edits) = page(&ctx, "", &mut st, click(x.center()));
        assert!(edits.is_some() && st.capture.is_none());
    }

    #[test]
    fn finding_by_key_takes_one_back_and_stops() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { listening: true, ..Default::default() };
        page(&ctx, "", &mut st, [vec![egui::Event::Text("g".into())], key(egui::Key::Backspace, egui::Modifiers::NONE)].concat());
        assert!(st.by_key.is_empty() && st.listening);
        page(&ctx, "", &mut st, [vec![egui::Event::Text("g".into())], key(egui::Key::Escape, egui::Modifiers::NONE)].concat());
        assert!(st.by_key.is_empty() && !st.listening);
        // The mouse stops it too.
        let mut st = KeysEdit { listening: true, ..Default::default() };
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        page(&ctx, "", &mut st, click(at(&texts, KEYS_EN.clear).unwrap().center()));
        assert!(!st.listening);
    }

    #[test]
    fn the_layer_filter_is_kept_by_name() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { layer: "help".into(), ..Default::default() };
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        assert!(at(&texts, "Close the help").is_some() && at(&texts, "Copy").is_none());
        st.layer = "gone".into();
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        assert!(at(&texts, "Copy").is_some(), "a layer no longer there is all of them");
    }

    #[test]
    fn every_word_has_both_languages() {
        fn all(w: &KeyWords) -> Vec<&'static str> {
            let mut v = vec![
                w.search,
                w.by_key,
                w.listening,
                w.clear,
                w.layer,
                w.all,
                w.changed_only,
                w.operation,
                w.keys,
                w.default_is,
                w.none,
                w.press,
                w.hint,
                w.special,
                w.set,
                w.cancel,
                w.add,
                w.remove,
                w.put_back,
                w.reset,
                w.change,
                w.built_in,
                w.mine,
                w.elsewhere,
                w.shadowed,
                w.taken_away,
                w.clash_same,
                w.clash_prefix,
                w.clash_common,
                w.more,
                w.nothing,
            ];
            v.extend(w.common_what);
            v
        }
        for (en, ja) in all(&KEYS_EN).into_iter().zip(all(&KEYS_JA)) {
            assert!(!en.is_empty() && !ja.is_empty());
            for k in 0..3 {
                let n = format!("{{{k}}}");
                assert_eq!(en.contains(&n), ja.contains(&n), "{en} / {ja}");
            }
        }
        assert_eq!(KeyWords::of("ja").set, "決める");
        assert_eq!(KeyWords::of("fr").set, "Set");
        assert!(keys_index(&KEYS_JA).contains(&"変えたものだけ"));
        assert_eq!(fill("{0} and {1}", &["a", "b"]), "a and b");
    }
}
