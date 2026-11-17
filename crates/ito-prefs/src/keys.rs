//! The Keys page every app has (`docs/common-spec.md`): each operation of
//! the app's keymap on a row of its own, its keys beside it, to find, change,
//! add, take away and put back. Nothing is written until a change is set:
//! the page hands back [`Edit`]s, which the app writes into its own
//! keymap.toml with [`ito_keymap::edit`] (off the UI thread) and then reads
//! the keymap again.

use egui::{Color32, FontId, RichText};
use ito_keymap::{clashes, rows, Binding, Clash, Edit, Row, State, BUILT_IN};
use ito_keys::{human_seq_for, render_seq, Code, Key, Named};

use crate::{select, switch, z, Look, Tone};

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
    /// The keyboard picture (`keys_page_kb`): the layout's label.
    pub kb_keyboard: &'static str,
    /// `{0}` is the guessed layout (`kb_us` or `kb_jis`).
    pub kb_auto: &'static str,
    /// The layout "auto" while the machine is still being asked.
    pub kb_auto_wait: &'static str,
    pub kb_us: &'static str,
    pub kb_jis: &'static str,
    pub kb_show: &'static str,
    pub kb_hide: &'static str,
    /// `{0}` is the layer shown, `{1}` the label of the layer filter.
    pub kb_showing: &'static str,
    pub kb_common: &'static str,
    /// `{0}` is the layer.
    pub kb_layer_only: &'static str,
    /// `{0}` is how many keys follow.
    pub kb_follows: &'static str,
    /// The legend line under the picture: no number, as the mark is `▸N`.
    pub kb_follows_legend: &'static str,
    /// Hover on a modifier key: `{0}` is how many keys, `{1}` the modifier's name.
    pub kb_mod_count: &'static str,
    /// Hover on a modifier key that is held by a click.
    pub kb_unlock: &'static str,
    pub kb_changed: &'static str,
    pub kb_unreachable: &'static str,
    /// `{0}` is how many keys.
    pub kb_unplaced: &'static str,
    pub kb_back: &'static str,
    pub kb_detail: &'static str,
    /// Above the operations offered for a free key: `{0}` the keys, `{1}` the layer.
    pub kb_assign_head: &'static str,
    /// The button on an operation offered for a free key: `{0}` the keys.
    pub kb_assign: &'static str,
    /// Hover on that button: `{0}` the keys.
    pub kb_assign_tip: &'static str,
    /// Hover on a free key of the picture: `{0}` the keys.
    pub kb_free: &'static str,
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
    kb_keyboard: "Keyboard",
    kb_auto: "Auto ({0})",
    kb_auto_wait: "Auto",
    kb_us: "US",
    kb_jis: "JIS",
    kb_show: "Show keyboard",
    kb_hide: "Hide keyboard",
    kb_showing: "Showing {0} — pick another in {1}",
    kb_common: "All apps",
    kb_layer_only: "{0} only",
    kb_follows: "{0} more — click to show",
    kb_follows_legend: "more keys follow — click to show",
    kb_mod_count: "{0} keys with {1} — click to lock it",
    kb_unlock: "click to unlock",
    kb_changed: "Changed",
    kb_unreachable: "Never runs",
    kb_unplaced: "Not on this keyboard ({0})",
    kb_back: "Back",
    kb_detail: "On this key",
    kb_assign_head: "{0} is free in {1}. Pick the operation it runs:",
    kb_assign: "Give it {0}",
    kb_assign_tip: "Adds {0} to this operation; its other keys stay",
    kb_free: "{0} is free — click to give it an operation",
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
    kb_keyboard: "キーボード",
    kb_auto: "自動（{0}）",
    kb_auto_wait: "自動",
    kb_us: "US",
    kb_jis: "JIS",
    kb_show: "キーボードを表示",
    kb_hide: "キーボードを隠す",
    kb_showing: "表示中: {0}（「{1}」で変える）",
    kb_common: "全アプリ共通",
    kb_layer_only: "{0}だけ",
    kb_follows: "続き {0} 個（クリックで表示）",
    kb_follows_legend: "続きのキーあり（クリックで表示）",
    kb_mod_count: "{1} との組み合わせ {0} 個（クリックで固定）",
    kb_unlock: "クリックで解除",
    kb_changed: "変えたキー",
    kb_unreachable: "届かない",
    kb_unplaced: "このキーボードに無いキー（{0}）",
    kb_back: "戻る",
    kb_detail: "このキー",
    kb_assign_head: "「{0}」は{1}で空いています。割り当てる操作を選んでください。",
    kb_assign: "{0} にする",
    kb_assign_tip: "この操作に {0} を足します（今のキーはそのまま）",
    kb_free: "{0} は空いています（クリックで操作を選ぶ）",
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
    vec![w.by_key, w.changed_only, w.layer, w.add, w.remove, w.reset, w.special, w.kb_keyboard, w.kb_show, w.kb_hide]
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
    /// The keyboard picture of [`keys_page_kb`] is folded away.
    pub kb_hidden: bool,
    /// Modifiers the picture holds down for the person: clicked on its
    /// modifier keys, or taken from the last key pressed while finding by key.
    pub kb_latch: ito_keyboard::Mods,
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
pub(crate) fn fill(s: &str, args: &[&str]) -> String {
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
    ui.add(
        egui::Button::new(text)
            .fill(if removed { Color32::TRANSPARENT } else { c.side })
            .stroke(stroke)
            .corner_radius(ito_theme::scale::radius::CONTROL),
    )
}

/// A small button of one sign, with a tooltip, named by it.
fn sign(ui: &mut egui::Ui, l: Look, s: &str, tip: &str, enabled: bool) -> bool {
    let b = egui::Button::new(RichText::new(s).size(z(ui, 12.5)).color(l.c.dim)).fill(Color32::TRANSPARENT).frame(false);
    let r = ui.add_enabled(enabled, b);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    let r = if enabled { r.on_hover_text(tip) } else { r.on_disabled_hover_text(tip) };
    r.clicked()
}

/// What [`keys_page_kb`] hands back.
#[derive(Debug, Default)]
pub struct KeysOut {
    /// The changes to the keymap, as [`keys_page`] gives them.
    pub edits: Vec<Edit>,
    /// A change to `common.toml` (the keyboard layout picked): the app
    /// writes it with its common.toml writer off the UI thread, as it does
    /// for other common changes.
    pub common: Option<ito_common::CommonChange>,
}

/// The Keys page: the search and filters, then the table. What is set hands
/// back the edits to write; `None` the rest of the time.
pub fn keys_page<A>(ui: &mut egui::Ui, l: Look, w: &KeyWords, t: &KeyTable<A>, st: &mut KeysEdit) -> Option<Vec<Edit>> {
    page(ui, l, w, t, st, None).0
}

/// [`keys_page`] with a picture of the keyboard between the filters and the
/// table: each key shows what it does in the layer chosen, and a click
/// narrows the table to the keys that start so. `layout` is `common.toml`'s
/// `keyboard` ([`ito_common::Common::keyboard_or_auto`]); picking another in
/// the picture is handed back in [`KeysOut::common`].
pub fn keys_page_kb<A>(
    ui: &mut egui::Ui,
    l: Look,
    w: &KeyWords,
    t: &KeyTable<A>,
    st: &mut KeysEdit,
    layout: ito_common::Keyboard,
) -> KeysOut {
    let (edits, common) = page(ui, l, w, t, st, Some(layout));
    KeysOut { edits: edits.unwrap_or_default(), common }
}

fn page<A>(
    ui: &mut egui::Ui,
    l: Look,
    w: &KeyWords,
    t: &KeyTable<A>,
    st: &mut KeysEdit,
    kb: Option<ito_common::Keyboard>,
) -> (Option<Vec<Edit>>, Option<ito_common::CommonChange>) {
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
                    Press::Key(k) => {
                        // The keyboard picture shows the layer this key is in.
                        let own = ito_keyboard::Mods { ctrl: k.ctrl, alt: k.alt, shift: k.shift, sup: k.sup };
                        st.kb_latch = match kb {
                            // Where the key is, as the picture has it (`G` is Shift+g).
                            Some(setting) => {
                                let (board, _) = crate::keyboard::board_for(ui.ctx(), setting);
                                ito_keyboard::place(board, &k, t.sup).first().map_or(own, |(_, m)| *m).folded(board, t.sup)
                            }
                            None => own,
                        };
                        st.by_key.push(k);
                    }
                    Press::Back => {
                        st.by_key.pop();
                    }
                    Press::Esc => {
                        st.listening = false;
                        st.by_key.clear();
                        st.kb_latch = Default::default();
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
        ui.add_space(ito_theme::scale::space::SM);
        if st.listening {
            let words = if st.by_key.is_empty() { w.listening.to_owned() } else { format!("{} …", human_seq_for(&st.by_key, t.sup)) };
            chip(ui, l, &words, true, false);
            if crate::button(ui, l, w.clear) {
                st.listening = false;
                st.by_key.clear();
                st.kb_latch = Default::default();
            }
        } else {
            if crate::button(ui, l, w.by_key) {
                st.listening = true;
                st.by_key.clear();
                st.kb_latch = Default::default();
                st.capture = None;
            }
            if !st.by_key.is_empty() {
                chip(ui, l, &human_seq_for(&st.by_key, t.sup), false, false);
                if crate::button(ui, l, w.clear) {
                    st.by_key.clear();
                    st.kb_latch = Default::default();
                }
            }
        }
    });
    ui.add_space(ito_theme::scale::space::SM);
    ui.horizontal(|ui| {
        if !one_layer {
            ui.label(RichText::new(w.layer).size(z(ui, 12.5)).color(c.dim));
            let names: Vec<String> = std::iter::once(w.all.to_owned()).chain(layers.iter().map(|n| (t.layer_name)(n))).collect();
            let options: Vec<(usize, &str)> = names.iter().enumerate().map(|(k, n)| (k, n.as_str())).collect();
            if let Some(k) = select(ui, "keys-layer", layer_at, &options) {
                st.layer = if k == 0 { String::new() } else { layers[k - 1].to_owned() };
            }
            ui.add_space(ito_theme::scale::space::LG);
        }
        if switch(ui, l, st.changed_only) {
            st.changed_only = !st.changed_only;
        }
        ui.label(RichText::new(w.changed_only).size(z(ui, 12.5)).color(c.fg));
    });
    ui.add_space(ito_theme::scale::space::MD);
    let mut common_out = None;
    if let Some(setting) = kb {
        common_out = crate::keyboard::picture(ui, l, w, t, st, &all, &layers, setting);
        ui.add_space(ito_theme::scale::space::MD);
    }

    // The rows shown, with the words each is found by.
    let assign = assign_for(t, st, &layers, kb.is_some());
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
                || if let Some(a) = &assign {
                    r.layer == a.layer && (q.is_empty() || hay.contains(&q))
                } else {
                    (layer_at == 0 || r.layer == st.layer)
                        && (!st.changed_only || r.changed())
                        && (q.is_empty() || hay.contains(&q))
                        && (st.by_key.is_empty() || r.keys.iter().any(|k| k.on.starts_with(&st.by_key)))
                };
            keep.then_some((r, desc, layer, hay))
        })
        .collect();

    egui::Frame::NONE
        .fill(c.panel)
        .stroke(egui::Stroke::new(1.0, c.border))
        .corner_radius(ito_theme::scale::radius::BOX)
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
            ui.add_space(ito_theme::scale::space::XS);
            if let Some(a) = &assign {
                crate::sep(ui, l);
                ui.add_space(ito_theme::scale::space::MD);
                let human = human_seq_for(&a.keys, t.sup);
                let head = fill(w.kb_assign_head, &[&human, &(t.layer_name)(&a.layer)]);
                let r = ui
                    .scope(|ui| {
                        ui.label(RichText::new(&head).size(z(ui, 12.5)).color(c.dim));
                        for note in key_notes(w, t, &a.layer, &a.keys) {
                            ui.label(RichText::new(note).size(z(ui, 12.0)).color(c.dim));
                        }
                    })
                    .response;
                ito_a11y::say(&r, ito_a11y::Role::Label, &head);
                ito_a11y::test_id(&r, "settings.keys.assign");
                ui.add_space(ito_theme::scale::space::SM);
            }
            if shown.is_empty() {
                crate::sep(ui, l);
                ui.add_space(ito_theme::scale::space::MD);
                ui.label(RichText::new(w.nothing).size(z(ui, 12.5)).color(c.dim));
                ui.add_space(ito_theme::scale::space::SM);
            }
            for (n, (row, desc, layer, hay)) in shown.iter().enumerate() {
                crate::sep(ui, l);
                let lit = !l.q.is_empty() && hay.contains(l.q);
                if lit {
                    crate::mark_hit(ui);
                }
                let behind = ui.painter().add(egui::Shape::Noop);
                let drawn = ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.allocate_ui_with_layout(egui::vec2(op_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(op_w);
                        ui.add_space(ito_theme::scale::space::SM);
                        ui.label(RichText::new(desc.as_str()).size(z(ui, 13.0)).color(c.strong()));
                        if !row.desc.is_empty() {
                            ui.add(
                                egui::Label::new(RichText::new(&row.run).font(FontId::monospace(z(ui, 11.0))).color(c.faint())).truncate(),
                            );
                        }
                        ui.add_space(ito_theme::scale::space::SM);
                    });
                    ui.allocate_ui_with_layout(egui::vec2(keys_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(keys_w);
                        ui.add_space(ito_theme::scale::space::SM);
                        keys_cell(ui, l, w, t, row, layer, st, &mut out, assign.as_ref().map(|a| (a, n)));
                        ui.add_space(ito_theme::scale::space::SM);
                    });
                    if !one_layer {
                        ui.allocate_ui_with_layout(egui::vec2(layer_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(layer_w);
                            ui.add_space(ito_theme::scale::space::MD);
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
    ((!out.is_empty()).then_some(out), common_out)
}

/// The keys of a row: a cap for each, ＋ to add one, ↺ when it was changed;
/// the defaults under it when it was, and while keys are pressed for it,
/// what they are and what they take.
#[allow(clippy::too_many_arguments)]
fn keys_cell<A>(
    ui: &mut egui::Ui,
    l: Look,
    w: &KeyWords,
    t: &KeyTable<A>,
    row: &Row,
    layer: &str,
    st: &mut KeysEdit,
    out: &mut Vec<Edit>,
    assign: Option<(&Assign, usize)>,
) {
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
        // The keys picked on the picture are free: this operation may take them.
        if let (Some((a, n)), None) = (assign, &this) {
            let human = human_seq_for(&a.keys, t.sup);
            let r = crate::button_with(ui, l, &fill(w.kb_assign, &[&human]), Tone::Plain).on_hover_text(fill(w.kb_assign_tip, &[&human]));
            ito_a11y::test_id(&r, &format!("settings.keys.assign.{n}"));
            if r.clicked() {
                let cap = Capture { layer: row.layer.clone(), run: row.run.clone(), keys: a.keys.clone(), replace: None };
                out.extend(set(&cap, Some(row)));
                st.capture = None;
                st.listening = false;
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
            said.extend(key_notes(w, t, &cap.layer, &cap.keys));
            for s in said {
                ui.label(RichText::new(s).size(z(ui, 12.0)).color(c.wait));
            }
        }
    }
}

/// What else is worth knowing about giving `layer` these keys: the key every
/// app keeps for itself, and what the app says (`KeyTable::extra`).
fn key_notes<A>(w: &KeyWords, t: &KeyTable<A>, layer: &str, keys: &[Key]) -> Vec<String> {
    let mut said = Vec::new();
    if let Some(k) = ito_keymap::common_for(keys, t.sup) {
        let first = human_seq_for(&keys[..1], t.sup);
        said.push(fill(w.clash_common, &[&first, w.common_what[k]]));
    }
    said.extend((t.extra)(layer, keys));
    said
}

/// The bindings of a layer (none for a name the keymap does not have).
pub(crate) fn layer_bindings<'a, A>(t: &'a KeyTable<A>, layer: &str) -> &'a [Binding<A>] {
    t.now.iter().find(|(n, _)| n == layer).map_or(&[][..], |(_, b)| b.as_slice())
}

/// Nothing is on these keys in a layer: no binding on them, none that starts
/// with them and none they start with.
pub(crate) fn keys_are_free<A>(t: &KeyTable<A>, layer: &str, keys: &[Key]) -> bool {
    clashes(layer_bindings(t, layer), keys, "", t.noop).is_empty()
}

/// The keys the table offers to give an operation, and the layer.
struct Assign {
    layer: String,
    keys: Vec<Key>,
}

/// The keys to give an operation, when the page has a picture and the keys
/// picked are free in the layer it shows.
fn assign_for<A>(t: &KeyTable<A>, st: &KeysEdit, layers: &[&str], kb: bool) -> Option<Assign> {
    if !kb || st.by_key.is_empty() || layers.is_empty() {
        return None;
    }
    let (_, layer) = crate::keyboard::shown_layer(layers, &st.layer);
    keys_are_free(t, &layer, &st.by_key).then(|| Assign { layer, keys: st.by_key.clone() })
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
                w.kb_follows,
                w.kb_follows_legend,
                w.kb_mod_count,
                w.kb_unlock,
                w.kb_assign_head,
                w.kb_assign,
                w.kb_assign_tip,
                w.kb_free,
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

    // ---- the keyboard picture ----

    type Ids = Vec<ito_a11y::TestIdEntry>;

    /// One frame of `keys_page_kb` at this width with AccessKit on: the test IDs drawn, and what it hands back.
    fn kb_frame(
        ctx: &egui::Context,
        files: &[(&str, &str)],
        st: &mut KeysEdit,
        events: Vec<egui::Event>,
        width: f32,
        layout: ito_common::Keyboard,
    ) -> (Ids, KeysOut) {
        let (ids, got, _) = kb_run(ctx, files, st, events, width, layout);
        (ids, got)
    }

    /// [`kb_frame`] and the text of each galley painted, as shown (an elided one ends in `…`), with its place.
    fn kb_run(
        ctx: &egui::Context,
        files: &[(&str, &str)],
        st: &mut KeysEdit,
        events: Vec<egui::Event>,
        width: f32,
        layout: ito_common::Keyboard,
    ) -> (Ids, KeysOut, Vec<(String, egui::Rect)>) {
        kb_run_with(ctx, files, st, events, width, layout, &[])
    }

    /// [`kb_run`] with operations that have no key.
    fn kb_run_with(
        ctx: &egui::Context,
        files: &[(&str, &str)],
        st: &mut KeysEdit,
        events: Vec<egui::Event>,
        width: f32,
        layout: ito_common::Keyboard,
        unbound: &[(&str, &str, &str)],
    ) -> (Ids, KeysOut, Vec<(String, egui::Rect)>) {
        ctx.enable_accesskit();
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
            sup: false,
        };
        let nav = Nav { pages: &[("Keys", "")], index: &[], words: &EN, file: "config.toml" };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 1600.0))),
            events,
            ..Default::default()
        };
        let mut screen = Screen::default();
        let mut got = None;
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                // As an app shows it: the Keys page is a wide one.
                let extras = crate::Extras { wide: &[0], ..Default::default() };
                crate::show_with(ui, ito_theme::colors(), &mut screen, &nav, Keys::default(), extras, |ui, _, l| {
                    got = Some(keys_page_kb(ui, l, &KEYS_EN, &t, st, layout));
                });
            });
        });
        out.textures_delta.clear();
        fn walk(s: &egui::Shape, v: &mut Vec<(String, egui::Rect)>) {
            match s {
                egui::Shape::Text(t) => {
                    let shown: String = t.galley.rows.iter().map(|r| r.text()).collect();
                    v.push((shown, t.galley.rect.translate(t.pos.to_vec2())));
                }
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, v)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        (ito_a11y::take_test_ids(ctx, egui::ViewportId::ROOT), got.unwrap(), texts)
    }

    const US: ito_common::Keyboard = ito_common::Keyboard::Us;

    fn id_rect(ids: &[ito_a11y::TestIdEntry], name: &str) -> Option<egui::Rect> {
        ids.iter().find(|e| e.name == name).map(|e| e.rect)
    }

    fn button(p: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    /// A real click on the thing with this test ID, over four frames; the IDs of a frame after it.
    fn kb_click(ctx: &egui::Context, files: &[(&str, &str)], st: &mut KeysEdit, name: &str, width: f32) -> Ids {
        let (ids, _) = kb_frame(ctx, files, st, Vec::new(), width, US);
        let p = id_rect(&ids, name).unwrap_or_else(|| panic!("{name} is drawn")).center();
        kb_frame(ctx, files, st, vec![egui::Event::PointerMoved(p)], width, US);
        kb_frame(ctx, files, st, vec![button(p, true)], width, US);
        kb_frame(ctx, files, st, vec![button(p, false)], width, US);
        kb_frame(ctx, files, st, Vec::new(), width, US).0
    }

    // ---- giving a free key an operation ----

    const UNBOUND: [(&str, &str, &str); 2] = [("list", "rename", "Rename"), ("help", "scroll", "Scroll")];

    fn assign_frame(
        ctx: &egui::Context,
        files: &[(&str, &str)],
        st: &mut KeysEdit,
        events: Vec<egui::Event>,
    ) -> (Ids, KeysOut, Vec<(String, egui::Rect)>) {
        kb_run_with(ctx, files, st, events, 1400.0, US, &UNBOUND)
    }

    /// The edits a real click on the thing with this test ID hands back.
    fn assign_click(ctx: &egui::Context, files: &[(&str, &str)], st: &mut KeysEdit, name: &str) -> Vec<Edit> {
        let (ids, _, _) = assign_frame(ctx, files, st, Vec::new());
        let p = id_rect(&ids, name).unwrap_or_else(|| panic!("{name} is drawn")).center();
        assign_frame(ctx, files, st, vec![egui::Event::PointerMoved(p)]);
        assign_frame(ctx, files, st, vec![button(p, true)]);
        assign_frame(ctx, files, st, vec![button(p, false)]).1.edits
    }

    fn assign_ids(ids: &[ito_a11y::TestIdEntry]) -> Vec<String> {
        let mut v: Vec<String> = ids.iter().filter(|e| e.name.starts_with("settings.keys.assign")).map(|e| e.name.clone()).collect();
        v.sort();
        v
    }

    fn add(layer: &str, on: &str, run: &str, desc: &str) -> Edit {
        Edit::Add { layer: layer.into(), on: keys(on), run: run.into(), desc: desc.into() }
    }

    #[test]
    fn a_free_key_offers_the_operations_of_its_layer() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { by_key: keys("0"), ..Default::default() };
        let (ids, _, texts) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert!(id_rect(&ids, "settings.keys.assign").is_some());
        let head = fill(KEYS_EN.kb_assign_head, &["0", "List"]);
        assert!(has(&texts, &head), "{head}");
        assert!(!has(&texts, KEYS_EN.nothing));
        // Copy, Paste, Go to the top, Visual mode, and Rename (no key) of the list layer; not the help layer's.
        let want: Vec<String> = ["", ".0", ".1", ".2", ".3", ".4"].iter().map(|n| format!("settings.keys.assign{n}")).collect();
        assert_eq!(assign_ids(&ids), want);

        let edits = assign_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.assign.4");
        assert_eq!(edits, vec![add("list", "0", "rename", "Rename")]);
        assert_eq!(st.by_key, keys("0"), "the keys stay picked");
        let edits = assign_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.assign.1");
        assert_eq!(edits, vec![add("list", "0", "paste", "Paste")]);

        // The folded picture does not matter.
        st.kb_hidden = true;
        let (ids, _, _) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert!(id_rect(&ids, "settings.keys.assign").is_some());
        assert!(id_rect(&ids, "settings.keys.kb.key.keyg").is_none());
    }

    #[test]
    fn once_the_key_is_bound_the_table_is_the_plain_filter() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { by_key: keys("0"), ..Default::default() };
        let own = "[[list.keymap]]\non = \"0\"\nrun = \"rename\"\ndesc = \"Rename\"\n";
        let (ids, _, texts) = assign_frame(&ctx, &[(OWN, own)], &mut st, Vec::new());
        assert!(assign_ids(&ids).is_empty(), "{:?}", assign_ids(&ids));
        assert!(!has(&texts, "is free in"));
        assert!(!has(&texts, KEYS_EN.nothing), "the row with the key is listed");
    }

    #[test]
    fn the_search_narrows_the_operations_offered() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { by_key: keys("0"), query: "paste".into(), ..Default::default() };
        let (ids, _, _) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert_eq!(assign_ids(&ids), ["settings.keys.assign", "settings.keys.assign.0"]);
        let edits = assign_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.assign.0");
        assert_eq!(edits, vec![add("list", "0", "paste", "Paste")]);
        st.query = "no such operation".into();
        let (ids, _, texts) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert_eq!(assign_ids(&ids), ["settings.keys.assign"]);
        assert!(has(&texts, KEYS_EN.nothing), "the head, and nothing under it");
    }

    #[test]
    fn the_layer_is_the_one_the_picture_shows() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { by_key: keys("0"), layer: "help".into(), ..Default::default() };
        let (ids, _, texts) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert!(has(&texts, &fill(KEYS_EN.kb_assign_head, &["0", "Help"])));
        assert_eq!(assign_ids(&ids), ["settings.keys.assign", "settings.keys.assign.0", "settings.keys.assign.1"]);
        let edits = assign_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.assign.1");
        assert_eq!(edits, vec![add("help", "0", "scroll", "Scroll")]);
        // A layer the keymap does not have is the first one.
        st.layer = "gone".into();
        let (_, _, texts) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert!(has(&texts, &fill(KEYS_EN.kb_assign_head, &["0", "List"])));
    }

    #[test]
    fn a_modifier_and_a_follow_up_are_keys_too() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { by_key: keys("<C-0>"), ..Default::default() };
        let edits = assign_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.assign.4");
        assert_eq!(edits, vec![add("list", "<C-0>", "rename", "Rename")]);
        let mut st = KeysEdit { by_key: keys("g x"), ..Default::default() };
        let (_, _, texts) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert!(has(&texts, &fill(KEYS_EN.kb_assign_head, &["g x", "List"])));
        let edits = assign_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.assign.4");
        assert_eq!(edits, vec![add("list", "g x", "rename", "Rename")]);
    }

    #[test]
    fn keys_that_are_taken_offer_nothing() {
        let ctx = egui::Context::default();
        // `g` starts `g g`; `y` is on a binding; `g g g` starts with one.
        for taken in ["g", "y", "g g g"] {
            let mut st = KeysEdit { by_key: keys(taken), ..Default::default() };
            let (ids, _, _) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
            assert!(assign_ids(&ids).is_empty(), "{taken}: {:?}", assign_ids(&ids));
        }
    }

    #[test]
    fn the_page_without_a_picture_offers_nothing() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { by_key: keys("0"), ..Default::default() };
        let (texts, _) = page(&ctx, "", &mut st, Vec::new());
        assert!(!has(&texts, "is free in"));
        assert!(at(&texts, KEYS_EN.nothing).is_some());
    }

    #[test]
    fn a_key_every_app_keeps_says_so_under_the_head() {
        let ctx = egui::Context::default();
        // F1 is the help in every app and the test keymap does not bind it.
        let mut st = KeysEdit { by_key: keys("<F1>"), ..Default::default() };
        let (ids, _, texts) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert!(id_rect(&ids, "settings.keys.assign").is_some());
        let k = ito_keymap::common_for(&st.by_key, false).expect("F1 is a common key");
        let note = fill(KEYS_EN.clash_common, &["F1", KEYS_EN.common_what[k]]);
        assert!(has(&texts, &note), "{note}");
        // A key that is not common has no such line.
        let mut st = KeysEdit { by_key: keys("0"), ..Default::default() };
        let (_, _, texts) = assign_frame(&ctx, &[(OWN, "")], &mut st, Vec::new());
        assert!(!has(&texts, "keeps for"));
    }

    #[test]
    fn the_plain_page_draws_no_keyboard_and_the_other_does() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let (ids, _) = kb_frame(&ctx, &[(OWN, "")], &mut st, Vec::new(), 1400.0, US);
        assert!(id_rect(&ids, "settings.keys.kb").is_some());
        assert!(id_rect(&ids, "settings.keys.kb.key.keyg").is_some());

        let ctx2 = egui::Context::default();
        ctx2.enable_accesskit();
        let mut st2 = KeysEdit::default();
        let ids = {
            let (now, built_in) = keymap(&[(OWN, "")]);
            let layer_name = |n: &str| n.to_owned();
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
                unbound: &[],
                sup: false,
            };
            let nav = Nav { pages: &[("Keys", "")], index: &[], words: &EN, file: "config.toml" };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1600.0))),
                ..Default::default()
            };
            let mut screen = Screen::default();
            let mut out = ctx2.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    show(ui, ito_theme::colors(), &mut screen, &nav, Keys::default(), |ui, _, l| {
                        keys_page(ui, l, &KEYS_EN, &t, &mut st2);
                    });
                });
            });
            out.textures_delta.clear();
            ito_a11y::take_test_ids(&ctx2, egui::ViewportId::ROOT)
        };
        assert!(ids.iter().all(|e| !e.name.starts_with("settings.keys.kb")));
    }

    #[test]
    fn a_click_on_a_key_follows_it_and_the_chip_goes_back() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let ids = kb_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.kb.key.keyg", 1400.0);
        assert_eq!(st.by_key, keys("g"));
        assert!(id_rect(&ids, "settings.keys.kb.prefix").is_some());
        let ids = kb_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.kb.prefix", 1400.0);
        assert!(st.by_key.is_empty());
        assert!(id_rect(&ids, "settings.keys.kb.prefix").is_none());
    }

    #[test]
    fn a_latched_shift_shows_the_shifted_bindings() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let own = "[[list.keymap]]\non = \"G\"\nrun = \"bottom\"\ndesc = \"Go to the bottom\"\n";
        kb_click(&ctx, &[(OWN, own)], &mut st, "settings.keys.kb.mod.shift", 1400.0);
        assert!(st.kb_latch.shift);
        let ids = kb_click(&ctx, &[(OWN, own)], &mut st, "settings.keys.kb.key.keyg", 1400.0);
        assert_eq!(st.by_key, keys("G"));
        assert!(id_rect(&ids, "settings.keys.kb.detail.0").is_some(), "the G binding is on the key");
    }

    #[test]
    fn listening_latches_the_modifiers_of_the_key_pressed() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { listening: true, ..Default::default() };
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        let mut ev = vec![egui::Event::ModifiersChanged(ctrl)];
        ev.extend(key(egui::Key::Comma, ctrl));
        kb_frame(&ctx, &[(OWN, "")], &mut st, ev, 1400.0, US);
        assert_eq!(st.by_key, keys("<C-,>"));
        assert!(st.kb_latch.ctrl && !st.kb_latch.shift);
    }

    #[test]
    fn a_capital_pressed_while_listening_latches_shift() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { listening: true, ..Default::default() };
        kb_frame(&ctx, &[(OWN, "")], &mut st, vec![egui::Event::Text("G".into())], 1400.0, US);
        assert_eq!(st.by_key, keys("G"));
        assert!(st.kb_latch.shift);
    }

    #[test]
    fn a_key_with_nothing_on_it_can_be_picked_too() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        kb_click(&ctx, &[(OWN, "")], &mut st, "settings.keys.kb.key.keyq", 1400.0);
        assert_eq!(st.by_key, keys("q"));
    }

    /// A keymap that binds the keys whose legends were cut to "…".
    const KB_OWN: &str = "[[list.keymap]]\non = \"<Esc>\"\nrun = \"escape\"\ndesc = \"Leave\"\n\n\
        [[list.keymap]]\non = \"<PageUp>\"\nrun = \"page_up\"\ndesc = \"Page up\"\n\n\
        [[list.keymap]]\non = \"<PageDown>\"\nrun = \"page_down\"\ndesc = \"Page down\"\n\n\
        [[list.keymap]]\non = \"<Home>\"\nrun = \"top\"\ndesc = \"Go to the top\"\n\n\
        [[list.keymap]]\non = [\"g\", \"g\"]\nrun = \"top\"\ndesc = \"Go to the top\"\n\n\
        [[list.keymap]]\non = [\"g\", \"h\"]\nrun = \"home\"\ndesc = \"Go home\"\n";

    /// The screen width that gives the board keys of this size, found by probing how wide the page leaves it.
    fn width_for_unit(u: f32) -> f32 {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let (ids, _) = kb_frame(&ctx, &[(OWN, KB_OWN)], &mut st, Vec::new(), 700.0, US);
        let u0 = id_rect(&ids, "settings.keys.kb.key.keyg").unwrap().width();
        700.0 - 15.0 * u0 + 15.0 * u
    }

    /// What is painted on one key: the texts that sit inside its rect.
    fn on_key(ids: &[ito_a11y::TestIdEntry], texts: &[(String, egui::Rect)], code: &str) -> Vec<String> {
        let r = id_rect(ids, &format!("settings.keys.kb.key.{code}")).unwrap_or_else(|| panic!("{code} is drawn"));
        texts.iter().filter(|(_, t)| r.contains(t.center())).map(|(t, _)| t.clone()).collect()
    }

    #[test]
    fn the_narrow_keys_keep_their_legends_whole() {
        // u = 22 (the smallest; the board scrolls when the page is narrower), a page narrower than the board there, and u about 29.
        for (u, page) in [(22.0, width_for_unit(22.0)), (22.0, width_for_unit(22.0) - 120.0), (29.0, width_for_unit(29.0))] {
            let ctx = egui::Context::default();
            let mut st = KeysEdit::default();
            let (ids, _, texts) = kb_run(&ctx, &[(OWN, KB_OWN)], &mut st, Vec::new(), page, US);
            let esc = id_rect(&ids, "settings.keys.kb.key.escape").unwrap();
            assert!(esc.width() >= 21.0 && esc.width() < 40.0, "narrow keys at {page}: {esc:?}");
            for e in ids.iter().filter(|e| e.name.starts_with("settings.keys.kb.key.")) {
                for t in on_key(&ids, &texts, e.name.rsplit('.').next().unwrap()) {
                    assert_ne!(t, "\u{2026}", "{} shows a lone ellipsis at page {page}", e.name);
                }
            }
            // At u = 22 a four-letter legend does not fit even at the smallest size, so it may be cut there
            // (never to a lone "…", checked above); from u about 29 it is whole.
            if u > 25.0 {
                for (code, legend) in [("escape", "Esc"), ("pageup", "PgUp"), ("pagedown", "PgDn"), ("home", "Home")] {
                    let on = on_key(&ids, &texts, code);
                    assert!(on.iter().any(|t| t == legend), "{code} shows {legend}: {on:?}");
                    let e = ids.iter().find(|e| e.name == format!("settings.keys.kb.key.{code}")).unwrap();
                    assert_eq!(e.elided, Some(false), "{code} is not elided");
                }
            }
        }
    }

    #[test]
    fn the_modifier_count_does_not_sit_on_the_legend() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        // One binding per modifier, so each modifier key draws a count beside its legend.
        let own = format!(
            "{KB_OWN}\n[[list.keymap]]\non = \"G\"\nrun = \"bottom\"\ndesc = \"Bottom\"\n\n\
             [[list.keymap]]\non = \"<C-p>\"\nrun = \"palette\"\ndesc = \"Palette\"\n\n\
             [[list.keymap]]\non = \"<A-x>\"\nrun = \"cut\"\ndesc = \"Cut\"\n"
        );
        let (ids, _, texts) = kb_run(&ctx, &[(OWN, own.as_str())], &mut st, Vec::new(), width_for_unit(29.0), US);
        for name in ["shift", "ctrl", "alt"] {
            let key = id_rect(&ids, &format!("settings.keys.kb.mod.{name}")).unwrap();
            let inside: Vec<&(String, egui::Rect)> = texts.iter().filter(|(_, t)| key.contains(t.center())).collect();
            assert!(inside.len() >= 2, "{name} draws its legend and its count: {inside:?}");
            for (i, a) in inside.iter().enumerate() {
                for b in &inside[i + 1..] {
                    let both = a.1.intersect(b.1);
                    assert!(both.width() <= 0.5 || both.height() <= 0.5, "{name}: {a:?} over {b:?}");
                }
            }
        }
    }

    #[test]
    fn pgup_and_pgdn_are_told_apart_in_the_middle_form() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let (ids, _, texts) = kb_run(&ctx, &[(OWN, KB_OWN)], &mut st, Vec::new(), width_for_unit(44.0), US);
        let key = id_rect(&ids, "settings.keys.kb.key.pageup").unwrap();
        assert!(key.width() > 40.0 && key.width() < 48.0, "{key:?}");
        let (up, down) = (on_key(&ids, &texts, "pageup"), on_key(&ids, &texts, "pagedown"));
        assert!(up.iter().any(|t| t == "PgUp"), "{up:?}");
        assert!(down.iter().any(|t| t == "PgDn"), "{down:?}");
    }

    #[test]
    fn the_legend_line_does_not_repeat_the_number() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let (_, _, texts) = kb_run(&ctx, &[(OWN, KB_OWN)], &mut st, Vec::new(), 900.0, US);
        let want = format!("\u{25b8} {}", KEYS_EN.kb_follows_legend);
        assert!(texts.iter().any(|(t, _)| *t == want), "{:?}", texts.iter().map(|(t, _)| t).collect::<Vec<_>>());
        assert!(texts.iter().all(|(t, _)| !t.contains("\u{25b8}N")));
        assert!(!KEYS_EN.kb_follows_legend.contains("{0}") && !KEYS_JA.kb_follows_legend.contains("{0}"));
    }

    #[test]
    fn the_board_changes_its_shape_with_the_width() {
        let rects = |width: f32| {
            let ctx = egui::Context::default();
            let mut st = KeysEdit::default();
            let (ids, _) = kb_frame(&ctx, &[(OWN, "")], &mut st, Vec::new(), width, US);
            ids
        };
        // Wide: the nav keys sit at the right of the main block, on its rows.
        let ids = rects(1600.0);
        let (home, space, back) = (
            id_rect(&ids, "settings.keys.kb.key.home").unwrap(),
            id_rect(&ids, "settings.keys.kb.key.space").unwrap(),
            id_rect(&ids, "settings.keys.kb.key.backspace").unwrap(),
        );
        assert!(home.top() < space.top() && home.left() > back.right() && home.right() <= 1600.0, "{home:?} {space:?} {back:?}");
        // Middle: the nav keys are a strip under the main block.
        let ids = rects(900.0);
        let (home, space) = (id_rect(&ids, "settings.keys.kb.key.home").unwrap(), id_rect(&ids, "settings.keys.kb.key.space").unwrap());
        assert!(home.top() > space.bottom() && home.right() <= 900.0, "{home:?} {space:?}");
        // Narrow: the keys are small and the board scrolls.
        let ids = rects(200.0);
        assert!(id_rect(&ids, "settings.keys.kb.key.keyg").is_some());
    }

    #[test]
    fn clearing_the_search_lets_go_of_the_latch() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { listening: true, ..Default::default() };
        kb_frame(&ctx, &[(OWN, "")], &mut st, vec![egui::Event::Text("G".into())], 1400.0, US);
        assert!(st.kb_latch.shift);
        let (ids, _) = kb_frame(&ctx, &[(OWN, "")], &mut st, Vec::new(), 1400.0, US);
        let _ = ids;
        let texts = {
            // The Clear button is a plain text button on the page: find it by its words.
            let (now, built_in) = keymap(&[(OWN, "")]);
            let layer_name = |n: &str| n.to_owned();
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
                unbound: &[],
                sup: false,
            };
            let nav = Nav { pages: &[("Keys", "")], index: &[], words: &EN, file: "config.toml" };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1600.0))),
                ..Default::default()
            };
            let mut screen = Screen::default();
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    show(ui, ito_theme::colors(), &mut screen, &nav, Keys::default(), |ui, _, l| {
                        keys_page_kb(ui, l, &KEYS_EN, &t, &mut st, US);
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
            texts
        };
        let p = at(&texts, KEYS_EN.clear).expect("the Clear button").center();
        for ev in [vec![egui::Event::PointerMoved(p)], vec![button(p, true)], vec![button(p, false)]] {
            kb_frame(&ctx, &[(OWN, "")], &mut st, ev, 1400.0, US);
        }
        assert!(st.by_key.is_empty() && !st.listening);
        assert_eq!(st.kb_latch, ito_keyboard::Mods::default());
    }

    #[test]
    fn a_long_description_is_elided_on_its_key() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let own =
            "[[list.keymap]]\non = \"q\"\nrun = \"quit\"\ndesc = \"とても長い日本語の説明がここに入ります。キーの中には収まりません。\"\n";
        let (ids, _) = kb_frame(&ctx, &[(OWN, own)], &mut st, Vec::new(), 1400.0, US);
        let e = ids.iter().find(|e| e.name == "settings.keys.kb.key.keyq").expect("keyq");
        assert_eq!(e.elided, Some(true));
    }

    #[test]
    fn the_layout_is_picked_in_the_header_and_jis_has_its_own_keys() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit::default();
        let files = [(OWN, "")];
        let (ids, _) = kb_frame(&ctx, &files, &mut st, Vec::new(), 1400.0, ito_common::Keyboard::Jis);
        assert!(id_rect(&ids, "settings.keys.kb.key.intlyen").is_some());
        let (ids, _) = kb_frame(&ctx, &files, &mut st, Vec::new(), 1400.0, US);
        assert!(id_rect(&ids, "settings.keys.kb.key.intlyen").is_none());

        let p = id_rect(&ids, "settings.select.keys-keyboard").expect("select").center();
        kb_frame(&ctx, &files, &mut st, vec![egui::Event::PointerMoved(p)], 1400.0, US);
        kb_frame(&ctx, &files, &mut st, vec![button(p, true)], 1400.0, US);
        let (ids, _) = kb_frame(&ctx, &files, &mut st, vec![button(p, false)], 1400.0, US);
        let (ids2, _) = kb_frame(&ctx, &files, &mut st, Vec::new(), 1400.0, US);
        let opt = id_rect(&ids2, "settings.select.keys-keyboard.2")
            .or_else(|| id_rect(&ids, "settings.select.keys-keyboard.2"))
            .expect("option 2")
            .center();
        kb_frame(&ctx, &files, &mut st, vec![egui::Event::PointerMoved(opt)], 1400.0, US);
        kb_frame(&ctx, &files, &mut st, vec![button(opt, true)], 1400.0, US);
        let mut got = None;
        for ev in [vec![button(opt, false)], Vec::new()] {
            let (_, o) = kb_frame(&ctx, &files, &mut st, ev, 1400.0, US);
            if o.common.is_some() {
                got = o.common;
            }
        }
        assert_eq!(got, Some(ito_common::keyboard_change(ito_common::Keyboard::Jis)));
    }

    #[test]
    fn a_folded_keyboard_leaves_only_the_header() {
        let ctx = egui::Context::default();
        let mut st = KeysEdit { kb_hidden: true, ..Default::default() };
        let (ids, _) = kb_frame(&ctx, &[(OWN, "")], &mut st, Vec::new(), 1400.0, US);
        assert!(id_rect(&ids, "settings.keys.kb.toggle").is_some());
        assert!(id_rect(&ids, "settings.keys.kb").is_none());
        assert!(id_rect(&ids, "settings.keys.kb.key.keyg").is_none());
    }
}
