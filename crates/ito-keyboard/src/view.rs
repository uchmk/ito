//! What each key of a keyboard shows for one layer of a keymap: pure, so the
//! picture is only a drawing of this.
//!
//! The layer's ordered bindings decide what a press reaches ([`live`] walks
//! every prefix in order); the rows of the settings table only add what the
//! bindings do not say (where a binding was written, which dead ones sit under
//! a key).

use ito_keymap::{common_for, live, Binding, Row, State, BUILT_IN};
use ito_keys::Key;

use crate::layout::{caps, Board, Phys};
use crate::place::{place, Mods};

/// Whose key a binding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A key every app keeps (`ito_keymap::COMMON`).
    Common,
    /// A key of this layer of this app.
    Layer,
}

/// What one position shows with one set of modifiers held.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyView {
    pub phys: Phys,
    pub mods: Mods,
    /// The binding's full keys; empty when the key only leads on.
    pub on: Vec<Key>,
    pub run: String,
    pub desc: String,
    pub kind: Kind,
    /// Live bindings that continue past this key.
    pub follows: usize,
    /// The binding was written by a user's file.
    pub changed: bool,
    /// Other bindings on this very position.
    pub also: Vec<Also>,
}

/// Another binding on a position that already has one.
#[derive(Clone, Debug, PartialEq)]
pub struct Also {
    pub on: Vec<Key>,
    pub run: String,
    pub desc: String,
    /// An earlier binding runs first, so this one never does.
    pub unreachable: bool,
}

/// All the positions of a layer that show something.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// In the order of [`caps`], then of the modifiers.
    pub keys: Vec<KeyView>,
    /// Live bindings whose next key is nowhere on this keyboard.
    pub unplaced: Vec<(Vec<Key>, String)>,
    /// How many [`KeyView`]s each combination of modifiers has.
    pub per_mods: Vec<(Mods, usize)>,
    /// How many different keys can come next after the prefix, placed or not.
    pub next: usize,
}

impl View {
    /// The view of a position with these modifiers.
    pub fn at(&self, phys: Phys, mods: Mods) -> Option<&KeyView> {
        self.keys.iter().find(|k| k.phys == phys && k.mods == mods)
    }
}

fn mods_order(m: Mods) -> (bool, bool, bool, bool) {
    (m.shift, m.ctrl, m.alt, m.sup)
}

fn entry(keys: &mut Vec<KeyView>, phys: Phys, mods: Mods) -> &mut KeyView {
    if let Some(i) = keys.iter().position(|k| k.phys == phys && k.mods == mods) {
        return &mut keys[i];
    }
    keys.push(KeyView {
        phys,
        mods,
        on: Vec::new(),
        run: String::new(),
        desc: String::new(),
        kind: Kind::Layer,
        follows: 0,
        changed: false,
        also: Vec::new(),
    });
    keys.last_mut().expect("just pushed")
}

/// What each position shows for `layer` after `prefix` has been pressed.
/// `bindings` are the layer's, in order; `noop` the app's command that does
/// nothing; `rows` the settings table's rows (any layers); `sup` that the app
/// tells Cmd from Ctrl on a Mac.
pub fn view<A>(bindings: &[Binding<A>], noop: &str, rows: &[Row], layer: &str, prefix: &[Key], board: Board, sup: bool) -> View {
    let mut keys: Vec<KeyView> = Vec::new();
    let mut unplaced: Vec<(Vec<Key>, String)> = Vec::new();
    let mut next: Vec<Key> = Vec::new();
    for b in live(bindings, noop) {
        if b.on.len() <= prefix.len() || !b.on.starts_with(prefix) {
            continue;
        }
        let n = b.on[prefix.len()];
        if !next.contains(&n) {
            next.push(n);
        }
        let at = place(board, &n, sup);
        if at.is_empty() {
            let text = if b.desc.is_empty() { b.raw.clone() } else { b.desc.clone() };
            if !unplaced.iter().any(|(on, _)| *on == b.on) {
                unplaced.push((b.on.clone(), text));
            }
            continue;
        }
        let last = b.on.len() == prefix.len() + 1;
        for (phys, mods) in at {
            let kv = entry(&mut keys, phys, mods);
            if !last {
                kv.follows += 1;
            } else if kv.on.is_empty() {
                let cell = rows.iter().filter(|r| r.layer == layer && r.run == b.raw).flat_map(|r| &r.keys).find(|c| c.on == b.on);
                kv.on = b.on.clone();
                kv.run = b.raw.clone();
                kv.desc = b.desc.clone();
                kv.kind = if common_for(&b.on, sup).is_some() { Kind::Common } else { Kind::Layer };
                kv.changed = cell.map_or(&*b.from != BUILT_IN, |c| &*c.from != BUILT_IN);
            } else {
                kv.also.push(Also { on: b.on.clone(), run: b.raw.clone(), desc: b.desc.clone(), unreachable: false });
            }
        }
    }
    // The bindings an earlier one hides: they are not live, so the rows say.
    for row in rows.iter().filter(|r| r.layer == layer) {
        for cell in row.keys.iter().filter(|c| !c.removed && c.on.len() == prefix.len() + 1 && c.on.starts_with(prefix)) {
            if !matches!(cell.state, State::Shadowed { .. }) {
                continue;
            }
            for (phys, mods) in place(board, &cell.on[prefix.len()], sup) {
                entry(&mut keys, phys, mods).also.push(Also {
                    on: cell.on.clone(),
                    run: row.run.clone(),
                    desc: row.desc.clone(),
                    unreachable: true,
                });
            }
        }
    }
    let order: Vec<Phys> = caps(board).iter().map(|c| c.phys).collect();
    keys.sort_by_key(|k| (order.iter().position(|p| *p == k.phys), mods_order(k.mods)));
    let mut per_mods: Vec<(Mods, usize)> = Vec::new();
    for k in &keys {
        match per_mods.iter_mut().find(|(m, _)| *m == k.mods) {
            Some((_, n)) => *n += 1,
            None => per_mods.push((k.mods, 1)),
        }
    }
    per_mods.sort_by_key(|(m, _)| mods_order(*m));
    View { keys, unplaced, per_mods, next: next.len() }
}

/// Where `by_key` (the keys searched by) lands in the layer: how many of them
/// are the prefix, and the key after it, if one is selected.
///
/// When some live binding is longer than `by_key` and starts with all of it,
/// what follows is shown and nothing is selected. Otherwise the prefix is the
/// longest one that still leads on, and the key after it is selected. A
/// sequence an earlier exact binding cuts off never counts: `live` has
/// dropped it.
pub fn split<A>(bindings: &[Binding<A>], noop: &str, by_key: &[Key]) -> (usize, Option<Key>) {
    if by_key.is_empty() {
        return (0, None);
    }
    let live: Vec<&Binding<A>> = live(bindings, noop).collect();
    if live.iter().any(|b| b.on.len() > by_key.len() && b.on.starts_with(by_key)) {
        return (by_key.len(), None);
    }
    let n = (0..by_key.len()).rev().find(|&n| n == 0 || live.iter().any(|b| b.on.len() > n && b.on.starts_with(&by_key[..n]))).unwrap_or(0);
    (n, Some(by_key[n]))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn seq(s: &str) -> Vec<Key> {
        s.split_whitespace().map(|t| Key::parse(t).unwrap()).collect()
    }

    fn bind(on: &str, run: &str, desc: &str, from: &str) -> Binding<String> {
        Binding { on: seq(on), run: vec![run.to_owned()], desc: desc.to_owned(), raw: run.to_owned(), from: Arc::from(from) }
    }

    fn built(on: &str, run: &str, desc: &str) -> Binding<String> {
        bind(on, run, desc, BUILT_IN)
    }

    fn rows_of(layer: &[Binding<String>], defaults: &[Binding<String>]) -> Vec<Row> {
        ito_keymap::rows(&[("main".to_owned(), layer.to_vec())], &[("main".to_owned(), defaults.to_vec())], "noop")
    }

    fn keymap() -> Vec<Binding<String>> {
        let mut v = vec![built("j", "down", "Down"), built("G", "bottom", "Go to the bottom"), built("?", "help", "Help")];
        v.push(built("<C-,>", "settings", "Settings"));
        v.push(built("<BackTab>", "prev", "Previous tab"));
        for (i, c) in "abcdefghij".chars().enumerate() {
            v.push(built(&format!("g {c}"), &format!("goto{i}"), &format!("Go {c}")));
        }
        v.push(built("<F13>", "far", "Far"));
        v
    }

    fn view_of(layer: &[Binding<String>], prefix: &str, board: Board) -> View {
        view(layer, "noop", &rows_of(layer, layer), "main", &seq(prefix), board, false)
    }

    fn plain() -> Mods {
        Mods::default()
    }

    fn shift() -> Mods {
        Mods { shift: true, ..Mods::default() }
    }

    fn ctrl() -> Mods {
        Mods { ctrl: true, ..Mods::default() }
    }

    #[test]
    fn a_cmd_key_of_an_app_that_tells_it_apart_is_on_its_key() {
        let km = vec![built("<D-,>", "settings", "Settings")];
        let rows = rows_of(&km, &km);
        let v = view(&km, "noop", &rows, "main", &[], Board::MacUs, true);
        assert_eq!(v.at(Phys::Comma, Mods { sup: true, ..Mods::default() }).map(|k| k.run.as_str()), Some("settings"));
        assert!(v.unplaced.is_empty());
        // An app that does not tell them apart never receives it as its own key.
        let v = view(&km, "noop", &rows, "main", &[], Board::MacUs, false);
        assert_eq!(v.unplaced.len(), 1);
    }

    #[test]
    fn the_next_keys_are_counted_once_however_many_places_they_have() {
        let km = vec![built("g <C-x>", "a", "A"), built("g h", "b", "B")];
        let v = view(&km, "noop", &rows_of(&km, &km), "main", &seq("g"), Board::MacUs, false);
        assert_eq!(v.next, 2);
        assert!(v.keys.len() > 2, "{}", v.keys.len());
        // On a Mac without apart, the Control key entry is there for a ctrl press.
        let q = vec![built("<C-q>", "quit", "Quit")];
        let v = view(&q, "noop", &rows_of(&q, &q), "main", &[], Board::MacUs, false);
        let cmd = Mods { sup: true, ..Mods::default() };
        assert_eq!(v.at(Phys::KeyQ, cmd.folded(Board::MacUs, false)).map(|k| k.run.as_str()), Some("quit"));
    }

    #[test]
    fn a_leading_key_counts_what_follows_it() {
        let km = keymap();
        let v = view_of(&km, "", Board::PcUs);
        let g = v.at(Phys::KeyG, plain()).expect("g");
        assert_eq!((g.follows, g.on.is_empty()), (10, true));
        assert_eq!(v.at(Phys::KeyJ, plain()).map(|k| k.run.as_str()), Some("down"));
        assert_eq!(split(&km, "noop", &seq("g")), (1, None));
        assert_eq!(split(&km, "noop", &seq("j")), (0, Some(Key::char('j'))));
        assert_eq!(split(&km, "noop", &seq("g h")), (1, Some(Key::char('h'))));
        assert_eq!(split(&km, "noop", &[]), (0, None));
        // After g, the ten keys it leads to are shown.
        let after = view_of(&km, "g", Board::PcUs);
        assert_eq!(after.at(Phys::KeyH, plain()).map(|k| k.run.as_str()), Some("goto7"));
        assert_eq!(after.at(Phys::KeyJ, plain()).map(|k| k.desc.as_str()), Some("Go j"));
        assert!(after.at(Phys::KeyK, plain()).is_none());
    }

    #[test]
    fn a_capital_is_on_the_letter_with_shift_and_a_symbol_where_the_layout_puts_it() {
        let km = keymap();
        let v = view_of(&km, "", Board::PcUs);
        assert_eq!(v.at(Phys::KeyG, shift()).map(|k| k.run.as_str()), Some("bottom"));
        assert_eq!(v.at(Phys::Slash, shift()).map(|k| k.run.as_str()), Some("help"));
        let jis = view_of(&km, "", Board::PcJis);
        assert_eq!(jis.at(Phys::Slash, shift()).map(|k| k.run.as_str()), Some("help"));
        assert_eq!(v.at(Phys::Tab, shift()).map(|k| k.run.as_str()), Some("prev"));
    }

    #[test]
    fn a_common_key_is_marked_and_a_users_own_key_is_changed() {
        let mut km = keymap();
        km.insert(0, bind("x", "mine", "Mine", "/cfg/keymap.toml"));
        let v = view_of(&km, "", Board::PcUs);
        let comma = v.at(Phys::Comma, ctrl()).expect("<C-,>");
        assert_eq!((comma.kind, comma.changed), (Kind::Common, false));
        let x = v.at(Phys::KeyX, plain()).expect("x");
        assert_eq!((x.kind, x.changed), (Kind::Layer, true));
        assert_eq!(v.at(Phys::KeyJ, plain()).map(|k| k.changed), Some(false));
    }

    #[test]
    fn a_binding_a_key_hides_is_listed_as_never_running() {
        let mut km = keymap();
        km.push(built("<S-Tab>", "other", "Other"));
        let v = view_of(&km, "", Board::PcUs);
        let tab = v.at(Phys::Tab, shift()).expect("shift tab");
        assert_eq!(tab.run, "prev");
        assert_eq!(tab.also.len(), 1);
        assert_eq!((tab.also[0].run.as_str(), tab.also[0].unreachable), ("other", true));
    }

    #[test]
    fn a_key_the_board_lacks_is_unplaced_not_dropped() {
        let km = keymap();
        let v = view_of(&km, "", Board::PcUs);
        assert_eq!(v.unplaced.len(), 1);
        assert_eq!(v.unplaced[0].0, seq("<F13>"));
        assert!(v.keys.iter().all(|k| k.run != "far"));
    }

    #[test]
    fn an_earlier_exact_key_cuts_off_a_longer_sequence() {
        let km = vec![built("g", "top", "Top"), built("g h x", "deep", "Deep")];
        let v = view_of(&km, "", Board::PcUs);
        let g = v.at(Phys::KeyG, plain()).expect("g");
        assert_eq!((g.follows, g.run.as_str()), (0, "top"));
        assert_eq!(split(&km, "noop", &seq("g h")), (0, Some(Key::char('g'))));
    }

    #[test]
    fn a_noop_continuation_is_not_counted() {
        let km = vec![built("g a", "noop", ""), built("g b", "goto", "Go"), built("g c", "noop", "")];
        let v = view_of(&km, "", Board::PcUs);
        assert_eq!(v.at(Phys::KeyG, plain()).map(|k| k.follows), Some(1));
        let after = view_of(&km, "g", Board::PcUs);
        assert_eq!(after.keys.len(), 1);
    }

    #[test]
    fn on_a_jis_keyboard_a_ctrl_bracket_shows_on_two_keys() {
        let km = vec![built("<C-[>", "esc", "Escape")];
        let v = view_of(&km, "", Board::PcJis);
        assert_eq!(v.keys.iter().map(|k| k.phys).collect::<Vec<_>>(), [Phys::BracketLeft, Phys::BracketRight]);
        assert_eq!(v.per_mods, [(ctrl(), 2)]);
    }

    #[test]
    fn a_binding_a_default_removed_does_not_show() {
        let defaults = vec![built("q", "quit", "Quit"), built("j", "down", "Down")];
        let mut now = vec![bind("q", "noop", "", "/cfg/keymap.toml")];
        now.extend(defaults.clone());
        let rows = rows_of(&now, &defaults);
        let v = view(&now, "noop", &rows, "main", &[], Board::PcUs, false);
        assert!(v.at(Phys::KeyQ, plain()).is_none());
        assert!(v.at(Phys::KeyJ, plain()).is_some());
    }
}
