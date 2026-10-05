//! Keys into the bytes a shell expects: VT escape sequences, and win32-input-mode
//! records for ConPTY.




#[allow(unused_imports)]
use crate::{grid::*, log::*, osc::*, shell::*, terminal::*};

/// A key as a single win32-input-mode record, **pressed only**:
/// `CSI Vk ; Sc ; Uc ; 1 ; Cs ; 1 _`.
///
/// This is how `Esc` reaches a tcell program (lazygit, gh-dash) intact, and
/// the "pressed only" is the whole fix. A program in virtual-terminal input
/// mode gets the press as the character `0x1b` and waits 50ms to be sure it is
/// a lone Escape. If a release record follows -- which it did, both when
/// ConPTY made one up from a plain `ESC` and when this sent one -- tcell turns
/// it into `ESC [ 27 ; 1 ; 27 ; 0 ; 0 ; 1 _` in the same stream. A second ESC
/// inside the wait makes the first an Alt prefix, the release is then ignored,
/// and the key is gone. Windows Terminal delivers no release to such a
/// program, so the wait runs out and Escape arrives; this matches it.
///
/// ConPTY takes the record without having asked for win32-input-mode, which
/// `scripts/keyprobe.ps1` showed on the real machine: what this sent came out
/// as a key record carrying the virtual key.
pub fn win32_key(vk: u16, scan: u16, ch: u16, mods: Mods) -> Vec<u8> {
    record(vk, scan, ch, mods, false)
}

pub(crate) fn record(vk: u16, scan: u16, ch: u16, mods: Mods, enhanced: bool) -> Vec<u8> {
    // dwControlKeyState: the left-hand bits, since a synthesised key has no
    // side and the left is what every keyboard has. ENHANCED_KEY (0x100) is
    // what the arrows and the block above them carry on a real keyboard.
    let state = (if mods.alt { 0x02 } else { 0 })
        | (if mods.ctrl { 0x08 } else { 0 })
        | (if mods.shift { 0x10 } else { 0 })
        | (if enhanced { 0x100 } else { 0 });
    format!("\x1b[{vk};{scan};{ch};1;{state};1_").into_bytes()
}

/// A special key as a win32-input-mode record, for when the other end has
/// asked for them (Q27).
///
/// Every sequence a special key sends as VT starts with ESC, and a tcell
/// program cannot tell a lone `<Esc>` from the start of the next one if it
/// arrives inside its 50ms wait: `<Esc>` followed within 31ms by a forwarded
/// `<S-End>` (`\e[1;2F`) lost the Escape for good (#99). A record is one key
/// whatever follows it, which is how Windows Terminal sends them all.
pub fn special_record(key: Special, mods: Mods) -> Vec<u8> {
    use Special::*;
    let (vk, scan, ch, enhanced, mods) = match key {
        Enter => (0x0d, 0x1c, 0x0d, false, mods),
        Backspace => (0x08, 0x0e, if mods.ctrl { 0x7f } else { 0x08 }, false, mods),
        Tab => (0x09, 0x0f, 0x09, false, mods),
        BackTab => (0x09, 0x0f, 0x09, false, Mods { shift: true, ..mods }),
        Escape => (0x1b, 0x01, 0x1b, false, mods),
        Up => (0x26, 0x48, 0, true, mods),
        Down => (0x28, 0x50, 0, true, mods),
        Left => (0x25, 0x4b, 0, true, mods),
        Right => (0x27, 0x4d, 0, true, mods),
        Home => (0x24, 0x47, 0, true, mods),
        End => (0x23, 0x4f, 0, true, mods),
        PageUp => (0x21, 0x49, 0, true, mods),
        PageDown => (0x22, 0x51, 0, true, mods),
        Insert => (0x2d, 0x52, 0, true, mods),
        Delete => (0x2e, 0x53, 0, true, mods),
        F(n @ 1..=10) => (0x70 + u16::from(n) - 1, 0x3b + u16::from(n) - 1, 0, false, mods),
        F(11) => (0x7a, 0x57, 0, false, mods),
        F(12) => (0x7b, 0x58, 0, false, mods),
        F(n) => (0x70 + u16::from(n.min(24)) - 1, 0, 0, false, mods),
    };
    record(vk, scan, ch, mods, enhanced)
}

/// A letter or digit held with Ctrl or Alt as a win32-input-mode record.
/// `None` for anything else, which then goes as the bytes it always did.
///
/// `ch` is what the key would type: the control code under Ctrl (`Ctrl+C` is
/// 3), the character itself under Alt alone. The scan codes are a US
/// keyboard's, which is what ConPTY expects of a synthesised key.
pub fn char_record(c: char, mods: Mods) -> Option<Vec<u8>> {
    const LETTERS: [u16; 26] = [
        0x1e, 0x30, 0x2e, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1f, 0x14, 0x16, 0x2f, 0x11, 0x2d, 0x15, 0x2c,
    ];
    if !(mods.ctrl || mods.alt) {
        return None;
    }
    let (vk, scan) = match c.to_ascii_uppercase() {
        u @ 'A'..='Z' => (u as u16, LETTERS[(u as u8 - b'A') as usize]),
        '0' => (0x30, 0x0b),
        d @ '1'..='9' => (d as u16, 0x02 + (d as u16 - '1' as u16)),
        _ => return None,
    };
    let typed = match (mods.ctrl, c.is_ascii_alphabetic()) {
        (true, true) => (c.to_ascii_lowercase() as u16) - u16::from(b'a') + 1,
        (true, false) => 0,
        (false, _) if mods.shift => c.to_ascii_uppercase() as u16,
        (false, _) => c as u16,
    };
    Some(record(vk, scan, typed, mods, false))
}

/// The keys a terminal wants that are not text.
///
/// egui hands printable characters over as text events, which go through
/// untouched; these are the ones that have to become escape sequences.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Special {
    Enter,
    Backspace,
    Tab,
    BackTab,
    Escape,
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
}

/// What is held down while a key is pressed. `ctrl` doubles for `Cmd` on
/// macOS the way the rest of this app treats it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    /// The number xterm uses for a modifier combination, or `None` for none of
    /// them — which is the difference between `\x1b[1;5A` and `\x1b[A`.
    fn code(self) -> Option<u8> {
        let n = 1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl);
        (n > 1).then_some(n)
    }
}

/// The bytes a special key sends.
///
/// `app_cursor` is the terminal's application-cursor mode, which swaps the
/// arrows between CSI and SS3 — vim and readline both rely on it, so it is
/// read from the terminal rather than assumed.
pub fn encode(key: Special, mods: Mods, app_cursor: bool) -> Vec<u8> {
    use Special::*;
    // A modifier turns the short forms into the parameterised ones, so the
    // two cases are kept apart rather than patched together.
    if let Some(m) = mods.code() {
        let out = match key {
            Up => Some(format!("\x1b[1;{m}A")),
            Down => Some(format!("\x1b[1;{m}B")),
            Right => Some(format!("\x1b[1;{m}C")),
            Left => Some(format!("\x1b[1;{m}D")),
            Home => Some(format!("\x1b[1;{m}H")),
            End => Some(format!("\x1b[1;{m}F")),
            Insert => Some(format!("\x1b[2;{m}~")),
            Delete => Some(format!("\x1b[3;{m}~")),
            PageUp => Some(format!("\x1b[5;{m}~")),
            PageDown => Some(format!("\x1b[6;{m}~")),
            F(n @ 1..=4) => Some(format!("\x1b[1;{m}{}", (b'P' + n - 1) as char)),
            F(n) => f_key_number(n).map(|num| format!("\x1b[{num};{m}~")),
            _ => None,
        };
        if let Some(s) = out {
            return s.into_bytes();
        }
    }

    let plain: &[u8] = match key {
        Enter => b"\r",
        // A terminal's Backspace is DEL, and Ctrl+Backspace is the BS that
        // shells read as "delete the word".
        Backspace if mods.ctrl => b"\x08",
        Backspace => b"\x7f",
        Tab => b"\t",
        BackTab => b"\x1b[Z",
        Escape => b"\x1b",
        Up if app_cursor => b"\x1bOA",
        Down if app_cursor => b"\x1bOB",
        Right if app_cursor => b"\x1bOC",
        Left if app_cursor => b"\x1bOD",
        Up => b"\x1b[A",
        Down => b"\x1b[B",
        Right => b"\x1b[C",
        Left => b"\x1b[D",
        Home => b"\x1b[H",
        End => b"\x1b[F",
        PageUp => b"\x1b[5~",
        PageDown => b"\x1b[6~",
        Insert => b"\x1b[2~",
        Delete => b"\x1b[3~",
        F(1) => b"\x1bOP",
        F(2) => b"\x1bOQ",
        F(3) => b"\x1bOR",
        F(4) => b"\x1bOS",
        F(n) => {
            return match f_key_number(n) {
                Some(num) => format!("\x1b[{num}~").into_bytes(),
                None => Vec::new(),
            };
        }
    };
    let mut out = Vec::with_capacity(plain.len() + 1);
    // Alt is the escape prefix, which is how a terminal has always spelled it.
    if mods.alt && !matches!(key, Escape) {
        out.push(0x1b);
    }
    out.extend_from_slice(plain);
    out
}

/// F5 and up are numbered, with gaps where the numbering skipped.
pub(crate) fn f_key_number(n: u8) -> Option<u8> {
    Some(match n {
        5 => 15,
        6..=10 => 17 + (n - 6),
        11 => 23,
        12 => 24,
        _ => return None,
    })
}

/// A character typed with Ctrl held, as the control code it stands for.
/// `Ctrl+C` is 0x03, `Ctrl+[` is Escape, and so on down the C0 range.
pub fn control_code(c: char, alt: bool) -> Option<Vec<u8>> {
    let byte = match c {
        ' ' | '@' => 0x00,
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' | '?' => 0x1f,
        _ => return None,
    };
    let mut out = Vec::with_capacity(2);
    if alt {
        out.push(0x1b);
    }
    out.push(byte);
    Some(out)
}
