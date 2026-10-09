//! Keys in kitty's keyboard protocol (`CSI … u`), for a program that asked.
//!
//! A program pushes the enhancements it wants (`CSI > flags u`) and pops
//! them as it leaves; `alacritty_terminal` keeps that stack and answers the
//! query, and this turns a key into what the flags in force call for. With
//! none in force a key goes as it always did, so a shell sees no change.
//! Only the levels asked for are used: Shift+Enter is `CSI 13;2u` once a
//! program asked to have keys told apart, and plain `\r` otherwise.
//!
//! The protocol: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>.

use crate::{Mods, Special};

/// Tell apart the keys that legacy encoding runs together (Esc, Ctrl+I and
/// Tab, Shift+Enter and Enter).
pub const DISAMBIGUATE: u8 = 1;
/// Report repeats and releases as well as presses.
pub const EVENT_TYPES: u8 = 2;
/// Add the shifted key to a key sent with Shift.
pub const ALTERNATE_KEYS: u8 = 4;
/// Every key as an escape code, text keys and the modifiers themselves too.
pub const ALL_KEYS: u8 = 8;
/// Add the text a key types to its escape code (with [`ALL_KEYS`]).
pub const ASSOCIATED_TEXT: u8 = 16;

/// What happened to a key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyEvent {
    Press,
    Repeat,
    Release,
}

/// A key as the protocol names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyKey {
    Special(Special),
    /// A key that types text, by the character it types unshifted (`a`, `1`,
    /// `/`).
    Char(char),
    /// A modifier key itself, by its number in the protocol (left Shift is
    /// 57441).
    Modifier(u32),
}

/// The protocol's numbers for the modifier keys: left Shift, Ctrl, Alt,
/// Super, then the same on the right.
pub const MODIFIER_KEYS: [u32; 8] = [57441, 57442, 57443, 57444, 57447, 57448, 57449, 57450];

/// The bytes for `key` under `flags`. `None` when the key goes as it would
/// with no flags (legacy), and an empty `Vec` when it goes as nothing at all
/// (a release nobody asked to hear of). `text` is what the press typed, if
/// anything.
pub fn encode(key: KittyKey, mods: Mods, event: KeyEvent, text: Option<&str>, flags: u8) -> Option<Vec<u8>> {
    let all = flags & ALL_KEYS != 0;
    let event = match event {
        KeyEvent::Release if flags & EVENT_TYPES == 0 => return Some(Vec::new()),
        KeyEvent::Repeat if flags & EVENT_TYPES == 0 => KeyEvent::Press,
        e => e,
    };
    if flags & (DISAMBIGUATE | ALL_KEYS) == 0 {
        // Event types alone change nothing about how a key is spelled.
        return if event == KeyEvent::Release { Some(Vec::new()) } else { None };
    }
    // The legacy form stands for a press with nothing to tell apart.
    let legacy = || if event == KeyEvent::Release { Some(Vec::new()) } else { None };
    let (code, last, shifted) = match key {
        KittyKey::Modifier(_) if !all => return legacy(),
        KittyKey::Modifier(code) => (code, b'u', None),
        KittyKey::Char(c) => {
            // A key that types text goes as its text unless Ctrl or Alt is
            // held (or every key is asked for).
            if !all && !mods.ctrl && !mods.alt {
                return legacy();
            }
            let shifted = (flags & ALTERNATE_KEYS != 0 && mods.shift).then(|| shifted_char(c, text)).flatten();
            (c as u32, b'u', shifted)
        }
        KittyKey::Special(s) => {
            use Special::*;
            let mods = if s == BackTab { Mods { shift: true, ..mods } } else { mods };
            let plain = Mod(mods).code() == 1;
            match s {
                // These three stay as they were unless modified, so that a
                // shell left in the mode by a crashed program can still run
                // `reset` (the protocol's own rule).
                Enter | Tab | BackTab | Backspace if !all && plain => return legacy(),
                Escape | Enter | Tab | BackTab | Backspace => {
                    let code = match s {
                        Escape => 27,
                        Enter => 13,
                        Tab | BackTab => 9,
                        _ => 127,
                    };
                    return Some(csi(code, b'u', None, mods, event, None));
                }
                _ if !all && plain && event != KeyEvent::Release => return None,
                Up => (1, b'A', None),
                Down => (1, b'B', None),
                Right => (1, b'C', None),
                Left => (1, b'D', None),
                Home => (1, b'H', None),
                End => (1, b'F', None),
                F(1) => (1, b'P', None),
                F(2) => (1, b'Q', None),
                // `CSI R` is a cursor position report, so F3 is numbered.
                F(3) => (13, b'~', None),
                F(4) => (1, b'S', None),
                Insert => (2, b'~', None),
                Delete => (3, b'~', None),
                PageUp => (5, b'~', None),
                PageDown => (6, b'~', None),
                F(n) => (u32::from(crate::keys::f_key_number(n)?), b'~', None),
            }
        }
    };
    let text = match (all && flags & ASSOCIATED_TEXT != 0 && event != KeyEvent::Release, text) {
        (true, Some(t)) if !t.is_empty() && !t.chars().any(char::is_control) => Some(t),
        _ => None,
    };
    Some(csi(code, last, shifted, mods, event, text))
}

/// `CSI code[:shifted] ; mods[:event] [; text] last`, leaving out what is
/// the default.
fn csi(code: u32, last: u8, shifted: Option<u32>, mods: Mods, event: KeyEvent, text: Option<&str>) -> Vec<u8> {
    let m = Mod(mods).code();
    let ev = match event {
        KeyEvent::Press => "",
        KeyEvent::Repeat => ":2",
        KeyEvent::Release => ":3",
    };
    let mut out = String::from("\x1b[");
    // A letter-final key with nothing to say is `CSI A`, not `CSI 1 A`.
    let bare = m == 1 && ev.is_empty() && text.is_none();
    if !(bare && last != b'u' && last != b'~' && code == 1) {
        out += &code.to_string();
    }
    if let Some(s) = shifted {
        out += &format!(":{s}");
    }
    if !bare {
        out += &format!(";{m}{ev}");
    }
    if let Some(t) = text {
        let codes: Vec<String> = t.chars().map(|c| (c as u32).to_string()).collect();
        out += &format!(";{}", codes.join(":"));
    }
    out.push(last as char);
    out.into_bytes()
}

/// What `c` types with Shift: the text the press brought when there is one
/// character of it, or the capital of a letter.
fn shifted_char(c: char, text: Option<&str>) -> Option<u32> {
    let typed = text.and_then(|t| {
        let mut it = t.chars();
        match (it.next(), it.next()) {
            (Some(s), None) => Some(s),
            _ => None,
        }
    });
    let s = typed.unwrap_or_else(|| c.to_ascii_uppercase());
    (s != c).then_some(s as u32)
}

struct Mod(Mods);

impl Mod {
    /// 1 plus the bits: Shift 1, Alt 2, Ctrl 4.
    fn code(&self) -> u8 {
        1 + u8::from(self.0.shift) + 2 * u8::from(self.0.alt) + 4 * u8::from(self.0.ctrl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use KeyEvent::*;

    const NONE: Mods = Mods { ctrl: false, alt: false, shift: false };
    const SHIFT: Mods = Mods { ctrl: false, alt: false, shift: true };
    const CTRL: Mods = Mods { ctrl: true, alt: false, shift: false };

    fn s(key: KittyKey, mods: Mods, event: KeyEvent, text: Option<&str>, flags: u8) -> Option<String> {
        encode(key, mods, event, text, flags).map(|b| String::from_utf8(b).unwrap())
    }

    /// With nothing asked for, every key goes the old way.
    #[test]
    fn no_flags_no_change() {
        assert_eq!(s(KittyKey::Special(Special::Enter), SHIFT, Press, None, 0), None);
        assert_eq!(s(KittyKey::Char('c'), CTRL, Press, None, 0), None);
    }

    /// Told apart: Shift+Enter, Ctrl+Enter, Esc and Ctrl+I; plain Enter,
    /// Tab and Backspace, and text, stay as they were.
    #[test]
    fn disambiguate_tells_apart_what_legacy_runs_together() {
        let f = DISAMBIGUATE;
        assert_eq!(s(KittyKey::Special(Special::Enter), SHIFT, Press, None, f).as_deref(), Some("\x1b[13;2u"));
        assert_eq!(s(KittyKey::Special(Special::Enter), CTRL, Press, None, f).as_deref(), Some("\x1b[13;5u"));
        assert_eq!(s(KittyKey::Special(Special::Escape), NONE, Press, None, f).as_deref(), Some("\x1b[27u"));
        assert_eq!(s(KittyKey::Char('i'), CTRL, Press, None, f).as_deref(), Some("\x1b[105;5u"));
        assert_eq!(s(KittyKey::Special(Special::BackTab), NONE, Press, None, f).as_deref(), Some("\x1b[9;2u"));
        assert_eq!(s(KittyKey::Special(Special::Enter), NONE, Press, None, f), None);
        assert_eq!(s(KittyKey::Special(Special::Backspace), NONE, Press, None, f), None);
        assert_eq!(s(KittyKey::Char('a'), SHIFT, Press, Some("A"), f), None, "text stays text");
        assert_eq!(s(KittyKey::Special(Special::Up), NONE, Press, None, f), None, "app cursor still decides");
        assert_eq!(s(KittyKey::Special(Special::Up), CTRL, Press, None, f).as_deref(), Some("\x1b[1;5A"));
        assert_eq!(s(KittyKey::Special(Special::F(3)), SHIFT, Press, None, f).as_deref(), Some("\x1b[13;2~"));
        assert_eq!(s(KittyKey::Modifier(57441), SHIFT, Press, None, f), None);
    }

    /// Releases go only when asked for, and nothing is sent for one that
    /// was not.
    #[test]
    fn releases_and_repeats_only_when_asked() {
        let esc = KittyKey::Special(Special::Escape);
        assert_eq!(s(esc, NONE, Release, None, DISAMBIGUATE).as_deref(), Some(""));
        assert_eq!(s(esc, NONE, Repeat, None, DISAMBIGUATE).as_deref(), Some("\x1b[27u"));
        let f = DISAMBIGUATE | EVENT_TYPES;
        assert_eq!(s(esc, NONE, Release, None, f).as_deref(), Some("\x1b[27;1:3u"));
        assert_eq!(s(esc, NONE, Repeat, None, f).as_deref(), Some("\x1b[27;1:2u"));
        assert_eq!(s(KittyKey::Special(Special::Up), NONE, Release, None, f).as_deref(), Some("\x1b[1;1:3A"));
        assert_eq!(s(KittyKey::Special(Special::Enter), NONE, Release, None, f).as_deref(), Some(""), "Enter's release needs all keys");
    }

    /// Every key as a code, with the shifted key and the text when asked.
    #[test]
    fn all_keys_with_alternates_and_text() {
        let f = DISAMBIGUATE | ALL_KEYS;
        assert_eq!(s(KittyKey::Char('a'), NONE, Press, Some("a"), f).as_deref(), Some("\x1b[97u"));
        assert_eq!(s(KittyKey::Special(Special::Enter), NONE, Press, None, f).as_deref(), Some("\x1b[13u"));
        assert_eq!(s(KittyKey::Special(Special::Up), NONE, Press, None, f).as_deref(), Some("\x1b[A"));
        assert_eq!(s(KittyKey::Modifier(57441), SHIFT, Press, None, f).as_deref(), Some("\x1b[57441;2u"));
        let f = f | ALTERNATE_KEYS | ASSOCIATED_TEXT;
        assert_eq!(s(KittyKey::Char('a'), SHIFT, Press, Some("A"), f).as_deref(), Some("\x1b[97:65;2;65u"));
        assert_eq!(s(KittyKey::Char('a'), NONE, Press, Some("a"), f).as_deref(), Some("\x1b[97;1;97u"));
        assert_eq!(s(KittyKey::Char('1'), SHIFT, Press, Some("!"), f).as_deref(), Some("\x1b[49:33;2;33u"));
        assert_eq!(s(KittyKey::Char('c'), CTRL, Press, None, f).as_deref(), Some("\x1b[99;5u"));
    }
}
