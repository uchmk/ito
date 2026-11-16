//! What the app receives when a key is pressed, and the way back: which
//! presses give a key.
//!
//! This models the real conversion rather than reading legends: a printable
//! key with no Ctrl, Alt or Cmd reaches the app as text, and one with a chord
//! goes through egui's key and `ito_keys::from_egui`. On a JIS keyboard that
//! is not the same as the legend (Ctrl+`@` arrives as `<C-[>`).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ito_keys::Key;

use crate::layout::{caps, modifier, types, Board, Phys};

/// The modifiers held with a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The Mac command key, when the app tells it apart from Ctrl; it has no
    /// meaning on a PC.
    pub sup: bool,
}

impl Mods {
    /// Where the app does not tell Cmd from Ctrl (or on a PC), Cmd is Ctrl:
    /// `sup` folds into `ctrl`, as the picture lights them.
    pub fn folded(self, b: Board, apart: bool) -> Mods {
        if apart && b.is_mac() {
            self
        } else {
            Mods { ctrl: self.ctrl || self.sup, sup: false, ..self }
        }
    }

    /// No modifier at all.
    pub fn none(self) -> bool {
        self == Mods::default()
    }
}

/// The egui key a named key position gives, and Space.
fn named_key(p: Phys) -> Option<egui::Key> {
    use egui::Key as K;
    Some(match p {
        Phys::Tab => K::Tab,
        Phys::Enter => K::Enter,
        Phys::Escape => K::Escape,
        Phys::Backspace => K::Backspace,
        Phys::Delete => K::Delete,
        Phys::Insert => K::Insert,
        Phys::Home => K::Home,
        Phys::End => K::End,
        Phys::PageUp => K::PageUp,
        Phys::PageDown => K::PageDown,
        Phys::ArrowUp => K::ArrowUp,
        Phys::ArrowLeft => K::ArrowLeft,
        Phys::ArrowDown => K::ArrowDown,
        Phys::ArrowRight => K::ArrowRight,
        Phys::F1 => K::F1,
        Phys::F2 => K::F2,
        Phys::F3 => K::F3,
        Phys::F4 => K::F4,
        Phys::F5 => K::F5,
        Phys::F6 => K::F6,
        Phys::F7 => K::F7,
        Phys::F8 => K::F8,
        Phys::F9 => K::F9,
        Phys::F10 => K::F10,
        Phys::F11 => K::F11,
        Phys::F12 => K::F12,
        Phys::Space => K::Space,
        _ => return None,
    })
}

/// The egui key a position gives by itself, when the character it types is
/// not one egui knows by name. This copies egui-winit's `key_from_key_code`
/// (egui-winit 0.36, src/lib.rs) for the keys drawn: winit's physical key is
/// taken at its US position, and it has no IntlYen or IntlRo.
fn us_position(p: Phys) -> Option<egui::Key> {
    use egui::Key as K;
    Some(match p {
        Phys::Backquote => K::Backtick,
        Phys::Minus => K::Minus,
        Phys::Equal => K::Equals,
        Phys::BracketLeft => K::OpenBracket,
        Phys::BracketRight => K::CloseBracket,
        Phys::Backslash => K::Backslash,
        Phys::Semicolon => K::Semicolon,
        Phys::Quote => K::Quote,
        Phys::Comma => K::Comma,
        Phys::Period => K::Period,
        Phys::Slash => K::Slash,
        Phys::Digit0 => K::Num0,
        Phys::Digit1 => K::Num1,
        Phys::Digit2 => K::Num2,
        Phys::Digit3 => K::Num3,
        Phys::Digit4 => K::Num4,
        Phys::Digit5 => K::Num5,
        Phys::Digit6 => K::Num6,
        Phys::Digit7 => K::Num7,
        Phys::Digit8 => K::Num8,
        Phys::Digit9 => K::Num9,
        Phys::KeyA => K::A,
        Phys::KeyB => K::B,
        Phys::KeyC => K::C,
        Phys::KeyD => K::D,
        Phys::KeyE => K::E,
        Phys::KeyF => K::F,
        Phys::KeyG => K::G,
        Phys::KeyH => K::H,
        Phys::KeyI => K::I,
        Phys::KeyJ => K::J,
        Phys::KeyK => K::K,
        Phys::KeyL => K::L,
        Phys::KeyM => K::M,
        Phys::KeyN => K::N,
        Phys::KeyO => K::O,
        Phys::KeyP => K::P,
        Phys::KeyQ => K::Q,
        Phys::KeyR => K::R,
        Phys::KeyS => K::S,
        Phys::KeyT => K::T,
        Phys::KeyU => K::U,
        Phys::KeyV => K::V,
        Phys::KeyW => K::W,
        Phys::KeyX => K::X,
        Phys::KeyY => K::Y,
        Phys::KeyZ => K::Z,
        _ => return None,
    })
}

/// What the app receives when `p` is pressed on `b` with `m` held: `None` for
/// a key that gives nothing (a modifier, an inert key, a position the layout
/// types nothing on). `apart`: the app tells Cmd from Ctrl (`KeyTable::sup`),
/// which only a Mac has; the key then keeps what was held, as the page's
/// capture does.
pub fn produce(b: Board, p: Phys, m: Mods, apart: bool) -> Option<Key> {
    let apart = apart && b.is_mac();
    let cap = caps(b).iter().find(|c| c.phys == p)?;
    if cap.inert || modifier(b, p).is_some() {
        return None;
    }
    // Cmd means nothing on a PC.
    let sup = m.sup && b.is_mac();
    let ctrl = m.ctrl;
    let mods = egui::Modifiers { ctrl, alt: m.alt, shift: m.shift, mac_cmd: sup, command: if b.is_mac() { sup } else { ctrl } };
    if let Some(k) = named_key(p) {
        if let Some(key) = ito_keys::from_egui(k, &mods) {
            return Some(held(key, m, apart));
        }
        if p != Phys::Space {
            return None;
        }
    }
    let c = types(b, p, m.shift)?;
    if !(ctrl || m.alt || sup) {
        // A character with no chord arrives as text.
        return Some(Key::char(c));
    }
    let k = egui::Key::from_name(&c.to_string()).or_else(|| us_position(p))?;
    ito_keys::from_egui(k, &mods).map(|key| held(key, m, apart))
}

/// Where Cmd is its own key, `ctrl` is Control and `sup` is Cmd.
fn held(mut k: Key, m: Mods, apart: bool) -> Key {
    if apart {
        k.ctrl = m.ctrl;
        k.sup = m.sup;
    }
    k
}

/// Every `(position, modifiers)` on `b` that gives `k`, in the order of
/// [`caps`]. Shift is only added where the key needs it. Empty: `k` is not on
/// this keyboard (an F13, Insert on a Mac, a character the layout lacks).
///
/// `apart`: the app tells Cmd from Ctrl, which only a Mac has: `sup` is then
/// Cmd and `ctrl` is Control. Otherwise a key event never carries Cmd as its
/// own (it comes as `ctrl`), and on a Mac a `ctrl` key is pressed with Cmd or
/// with Control.
pub fn place(b: Board, k: &Key, apart: bool) -> Vec<(Phys, Mods)> {
    type Memo = Mutex<HashMap<(Board, Key, bool), Vec<(Phys, Mods)>>>;
    static PLACED: OnceLock<Memo> = OnceLock::new();
    let key = (b, *k, apart && b.is_mac());
    let memo = PLACED.get_or_init(Default::default);
    if let Some(found) = memo.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return found;
    }
    let found = place_uncached(b, k, apart);
    if let Ok(mut m) = memo.lock() {
        m.insert(key, found.clone());
    }
    found
}

fn place_uncached(b: Board, k: &Key, apart: bool) -> Vec<(Phys, Mods)> {
    let chords: Vec<(bool, bool)> = if apart && b.is_mac() {
        vec![(k.ctrl, k.sup)]
    } else if k.sup {
        return Vec::new();
    } else {
        match (k.ctrl, b.is_mac()) {
            (false, _) => vec![(false, false)],
            (true, false) => vec![(true, false)],
            (true, true) => vec![(false, true), (true, false)],
        }
    };
    let mut out = Vec::new();
    for cap in caps(b) {
        for &(ctrl, sup) in &chords {
            for shift in [false, true] {
                let m = Mods { ctrl, alt: k.alt, shift, sup };
                if produce(b, cap.phys, m, apart).as_ref() == Some(k) {
                    out.push((cap.phys, m));
                    // Shift held as well gives the same key: not listed again.
                    break;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> Key {
        Key::parse(s).unwrap()
    }

    fn at(b: Board, k: &str) -> Vec<(Phys, Mods)> {
        place(b, &key(k), false)
    }

    fn m(ctrl: bool, shift: bool) -> Mods {
        Mods { ctrl, shift, ..Mods::default() }
    }

    #[test]
    fn a_jis_ctrl_at_sign_arrives_as_the_open_bracket() {
        assert_eq!(produce(Board::PcJis, Phys::BracketLeft, m(true, false), false), Some(key("<C-[>")));
        assert_eq!(produce(Board::PcJis, Phys::BracketRight, m(true, false), false), Some(key("<C-[>")));
        assert_eq!(at(Board::PcJis, "<C-[>"), [(Phys::BracketLeft, m(true, false)), (Phys::BracketRight, m(true, false))]);
    }

    #[test]
    fn the_jis_equals_is_ctrl_shift_minus_or_the_caret_key() {
        assert_eq!(at(Board::PcJis, "<C-=>"), [(Phys::Minus, m(true, true)), (Phys::Equal, m(true, false))]);
        assert_eq!(at(Board::PcJis, "<C-+>"), [(Phys::Semicolon, m(true, true))]);
        assert_eq!(at(Board::PcUs, "<C-=>"), [(Phys::Equal, m(true, false))]);
    }

    #[test]
    fn the_jis_digits_with_shift_keep_the_us_position() {
        assert_eq!(produce(Board::PcJis, Phys::Digit2, m(true, true), false), Some(key("<C-S-2>")));
        // Ctrl+Shift+ろ types `_`, which egui has no name for, and no US position.
        assert_eq!(produce(Board::PcJis, Phys::IntlRo, m(true, true), false), None);
    }

    #[test]
    fn the_backslash_is_on_the_yen_key_and_on_ro_of_a_jis_keyboard() {
        assert_eq!(at(Board::PcJis, "\\"), [(Phys::IntlYen, Mods::default()), (Phys::IntlRo, Mods::default())]);
        assert_eq!(at(Board::PcUs, "\\"), [(Phys::Backslash, Mods::default())]);
    }

    #[test]
    fn a_character_is_text_and_shift_makes_it_upper_case() {
        assert_eq!(at(Board::PcUs, "?"), [(Phys::Slash, m(false, true))]);
        assert_eq!(at(Board::PcUs, "G"), [(Phys::KeyG, m(false, true))]);
        assert_eq!(at(Board::PcUs, "g"), [(Phys::KeyG, Mods::default())]);
        assert_eq!(produce(Board::PcUs, Phys::KeyG, m(false, true), false), Some(key("G")));
        assert_eq!(at(Board::PcUs, " "), [(Phys::Space, Mods::default())]);
    }

    #[test]
    fn shift_tab_is_the_back_tab() {
        assert_eq!(produce(Board::PcUs, Phys::Tab, m(false, true), false), Some(key("<BackTab>")));
        assert_eq!(at(Board::PcUs, "<BackTab>"), [(Phys::Tab, m(false, true))]);
        assert_eq!(at(Board::PcUs, "<Tab>"), [(Phys::Tab, Mods::default())]);
    }

    #[test]
    fn a_mac_ctrl_key_is_pressed_with_cmd_or_control() {
        let p = at(Board::MacUs, "<C-q>");
        assert!(p.contains(&(Phys::KeyQ, Mods { sup: true, ..Mods::default() })), "{p:?}");
        assert!(p.contains(&(Phys::KeyQ, m(true, false))), "{p:?}");
        // The same on a PC is Ctrl alone.
        assert_eq!(at(Board::PcUs, "<C-q>"), [(Phys::KeyQ, m(true, false))]);
    }

    #[test]
    fn where_cmd_is_apart_a_mac_keeps_cmd_and_control_as_they_are() {
        let cmd = Mods { sup: true, ..Mods::default() };
        assert_eq!(place(Board::MacUs, &key("<D-,>"), true), [(Phys::Comma, cmd)]);
        assert_eq!(place(Board::MacUs, &key("<C-a>"), true), [(Phys::KeyA, m(true, false))]);
        assert_eq!(produce(Board::MacUs, Phys::Comma, cmd, true), Some(key("<D-,>")));
        assert_eq!(produce(Board::MacUs, Phys::Home, cmd, true), Some(key("<D-Home>")));
        // Not apart: either modifier gives `<C-,>`, and `<D-,>` is nowhere.
        let p = place(Board::MacUs, &key("<C-,>"), false);
        assert!(p.contains(&(Phys::Comma, cmd)) && p.contains(&(Phys::Comma, m(true, false))), "{p:?}");
        assert_eq!(place(Board::MacUs, &key("<D-,>"), false), []);
        // A PC has no Cmd, apart or not.
        assert_eq!(place(Board::PcUs, &key("<D-,>"), true), []);
        assert_eq!(place(Board::PcUs, &key("<C-,>"), true), [(Phys::Comma, m(true, false))]);
    }

    #[test]
    fn the_remembered_places_are_the_computed_ones_in_the_same_order() {
        for b in Board::ALL {
            for apart in [false, true] {
                for s in ["a", "G", "<C-q>", "<D-,>", "<C-=>", "\\", "<BackTab>", "<F13>", "<A-x>"] {
                    let k = key(s);
                    let first = place(b, &k, apart);
                    assert_eq!(first, place_uncached(b, &k, apart), "{b:?} {s} {apart}");
                    assert_eq!(first, place(b, &k, apart), "{b:?} {s} {apart} again");
                }
            }
        }
    }

    #[test]
    fn cmd_folds_into_ctrl_unless_a_mac_keeps_them_apart() {
        let cmd = Mods { sup: true, ..Mods::default() };
        assert_eq!(cmd.folded(Board::MacUs, false), m(true, false));
        assert_eq!(cmd.folded(Board::PcUs, true), m(true, false));
        assert_eq!(cmd.folded(Board::MacUs, true), cmd);
    }

    #[test]
    fn a_key_the_board_lacks_is_nowhere() {
        assert_eq!(at(Board::PcUs, "<F13>"), []);
        assert_eq!(at(Board::MacUs, "<Insert>"), []);
        assert_eq!(at(Board::MacJis, "<Insert>"), []);
        assert!(!at(Board::PcUs, "<Insert>").is_empty());
        // `<D-x>` is never delivered as its own key.
        assert_eq!(at(Board::MacUs, "<D-x>"), []);
        assert_eq!(at(Board::PcUs, "\u{a5}"), []);
    }

    #[test]
    fn modifiers_and_inert_keys_give_nothing() {
        for b in Board::ALL {
            for cap in caps(b) {
                if cap.inert || modifier(b, cap.phys).is_some() {
                    assert_eq!(produce(b, cap.phys, Mods::default(), false), None, "{b:?} {:?}", cap.phys);
                }
            }
        }
    }

    #[test]
    fn every_character_a_board_types_is_placed_on_it() {
        for b in Board::ALL {
            for c in ' '..='~' {
                let typed = caps(b).iter().any(|cap| [false, true].iter().any(|&s| types(b, cap.phys, s) == Some(c) && !cap.inert));
                if typed {
                    assert!(!place(b, &Key::char(c), false).is_empty(), "{b:?} {c:?}");
                }
            }
        }
    }

    #[test]
    fn the_yen_key_types_a_backslash_on_a_pc_and_the_yen_sign_on_a_mac() {
        assert_eq!(types(Board::PcJis, Phys::IntlYen, false), Some('\\'));
        assert_eq!(types(Board::PcJis, Phys::IntlYen, true), Some('|'));
        assert_eq!(types(Board::MacJis, Phys::IntlYen, false), Some('\u{a5}'));
        assert_eq!(types(Board::PcJis, Phys::Backquote, false), None);
        assert_eq!(types(Board::PcUs, Phys::Backquote, true), Some('~'));
        assert_eq!(types(Board::PcJis, Phys::Digit2, true), Some('"'));
        assert_eq!(types(Board::PcUs, Phys::Digit2, true), Some('@'));
        assert_eq!(types(Board::PcJis, Phys::Digit0, true), None);
    }
}
