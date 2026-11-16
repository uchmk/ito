//! The boards: which key positions each keyboard has, where they sit and
//! what is printed on them.

use std::sync::OnceLock;

pub use crate::phys::Phys;

/// Which key set the hardware has: the US one or the Japanese (JIS) one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Us,
    Jis,
}

/// A keyboard the picture can draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Board {
    PcUs,
    PcJis,
    MacUs,
    MacJis,
}

impl Board {
    /// Every board.
    pub const ALL: [Board; 4] = [Board::PcUs, Board::PcJis, Board::MacUs, Board::MacJis];

    /// The board of a key set on a PC (`mac` false) or a Mac.
    pub fn of(family: Family, mac: bool) -> Self {
        match (family, mac) {
            (Family::Us, false) => Board::PcUs,
            (Family::Jis, false) => Board::PcJis,
            (Family::Us, true) => Board::MacUs,
            (Family::Jis, true) => Board::MacJis,
        }
    }

    /// The board of `family` on the OS this runs on.
    pub fn here(family: Family) -> Self {
        Self::of(family, cfg!(target_os = "macos"))
    }

    pub fn family(self) -> Family {
        match self {
            Board::PcUs | Board::MacUs => Family::Us,
            Board::PcJis | Board::MacJis => Family::Jis,
        }
    }

    pub fn is_mac(self) -> bool {
        matches!(self, Board::MacUs | Board::MacJis)
    }
}

/// The part of the keyboard a key belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Block {
    /// Esc and the F row.
    Fn,
    Main,
    /// Insert, Home, PageUp, Delete, End, PageDown.
    Nav,
    Arrows,
}

/// One key as drawn, in key units `u` (a 1u key is a square letter key).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cap {
    pub phys: Phys,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Only the JIS Enter has one: its top row also covers this much to the
    /// left of `x`, which makes it the usual inverted L.
    pub notch: f32,
    pub block: Block,
    /// Drawn faint, and never carries a binding: ito has no such key.
    pub inert: bool,
}

impl Cap {
    /// `(x, y, w, h)` of the part left of the main rectangle, for the JIS Enter.
    pub fn notch_rect(&self) -> Option<(f32, f32, f32, f32)> {
        (self.notch > 0.0).then_some((self.x - self.notch, self.y, self.notch, 1.0))
    }
}

/// The modifier a cap is: pressing it changes which layer of the keyboard
/// the other keys are on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
    /// The Mac command key.
    Cmd,
}

/// The modifier `p` is on `b`, if it is one.
pub fn modifier(b: Board, p: Phys) -> Option<Modifier> {
    match p {
        Phys::ShiftLeft | Phys::ShiftRight => Some(Modifier::Shift),
        Phys::ControlLeft | Phys::ControlRight => Some(Modifier::Ctrl),
        Phys::AltLeft | Phys::AltRight => Some(Modifier::Alt),
        Phys::MetaLeft | Phys::MetaRight if b.is_mac() => Some(Modifier::Cmd),
        _ => None,
    }
}

/// Keys that are drawn faint and never carry a binding: ito has no Menu key,
/// and the others are not keys a keymap can bind.
fn inert(b: Board, p: Phys) -> bool {
    match p {
        Phys::MetaLeft | Phys::MetaRight => !b.is_mac(),
        Phys::Fn | Phys::ContextMenu | Phys::CapsLock => true,
        Phys::Backquote => b == Board::PcJis,
        Phys::NonConvert | Phys::Convert | Phys::KanaMode | Phys::Lang1 | Phys::Lang2 => true,
        _ => false,
    }
}

struct Builder {
    b: Board,
    caps: Vec<Cap>,
}

impl Builder {
    fn key(&mut self, phys: Phys, x: f32, y: f32, w: f32, block: Block) -> f32 {
        self.caps.push(Cap { phys, x, y, w, h: 1.0, notch: 0.0, block, inert: inert(self.b, phys) });
        x + w
    }

    /// Keys one after the other from `x`, each as wide as given; the x after the last.
    fn run(&mut self, y: f32, mut x: f32, block: Block, keys: &[(Phys, f32)]) -> f32 {
        for &(p, w) in keys {
            x = self.key(p, x, y, w, block);
        }
        x
    }

    /// Keys of one unit each in the main block.
    fn ones(&mut self, y: f32, x: f32, keys: &[Phys]) -> f32 {
        let mut x = x;
        for &p in keys {
            x = self.key(p, x, y, 1.0, Block::Main);
        }
        x
    }
}

const DIGITS: [Phys; 10] = [
    Phys::Digit1,
    Phys::Digit2,
    Phys::Digit3,
    Phys::Digit4,
    Phys::Digit5,
    Phys::Digit6,
    Phys::Digit7,
    Phys::Digit8,
    Phys::Digit9,
    Phys::Digit0,
];

const TOP_LETTERS: [Phys; 12] = [
    Phys::KeyQ,
    Phys::KeyW,
    Phys::KeyE,
    Phys::KeyR,
    Phys::KeyT,
    Phys::KeyY,
    Phys::KeyU,
    Phys::KeyI,
    Phys::KeyO,
    Phys::KeyP,
    Phys::BracketLeft,
    Phys::BracketRight,
];

const HOME_LETTERS: [Phys; 11] = [
    Phys::KeyA,
    Phys::KeyS,
    Phys::KeyD,
    Phys::KeyF,
    Phys::KeyG,
    Phys::KeyH,
    Phys::KeyJ,
    Phys::KeyK,
    Phys::KeyL,
    Phys::Semicolon,
    Phys::Quote,
];

const BOTTOM_LETTERS: [Phys; 10] =
    [Phys::KeyZ, Phys::KeyX, Phys::KeyC, Phys::KeyV, Phys::KeyB, Phys::KeyN, Phys::KeyM, Phys::Comma, Phys::Period, Phys::Slash];

/// Where the main block's rows start.
const MAIN_Y: f32 = 1.25;

fn build(b: Board) -> Vec<Cap> {
    let mut c = Builder { b, caps: Vec::new() };
    let jis = b.family() == Family::Jis;
    let mac = b.is_mac();

    // The F row: Esc, then three groups of four.
    c.key(Phys::Escape, 0.0, 0.0, 1.0, Block::Fn);
    let groups = [
        (2.0, [Phys::F1, Phys::F2, Phys::F3, Phys::F4]),
        (6.5, [Phys::F5, Phys::F6, Phys::F7, Phys::F8]),
        (11.0, [Phys::F9, Phys::F10, Phys::F11, Phys::F12]),
    ];
    for (x, keys) in groups {
        let mut x = x;
        for p in keys {
            x = c.key(p, x, 0.0, 1.0, Block::Fn);
        }
    }

    // The number row.
    let y = MAIN_Y;
    let mut x = 0.0;
    if !(jis && mac) {
        x = c.ones(y, x, &[Phys::Backquote]);
    }
    x = c.ones(y, x, &DIGITS);
    x = c.ones(y, x, &[Phys::Minus, Phys::Equal]);
    if jis {
        x = c.ones(y, x, &[Phys::IntlYen]);
    }
    c.key(Phys::Backspace, x, y, 15.0 - x, Block::Main);

    // The Tab row, and the JIS Enter that spans two rows.
    let y = MAIN_Y + 1.0;
    let x = c.key(Phys::Tab, 0.0, y, 1.5, Block::Main);
    let x = c.ones(y, x, &TOP_LETTERS);
    if jis {
        c.caps.push(Cap { phys: Phys::Enter, x: 13.75, y, w: 1.25, h: 2.0, notch: 0.25, block: Block::Main, inert: false });
    } else {
        c.key(Phys::Backslash, x, y, 15.0 - x, Block::Main);
    }

    // The home row.
    let y = MAIN_Y + 2.0;
    let x = c.key(Phys::CapsLock, 0.0, y, 1.75, Block::Main);
    let x = c.ones(y, x, &HOME_LETTERS);
    if jis {
        c.ones(y, x, &[Phys::Backslash]);
    } else {
        c.key(Phys::Enter, x, y, 15.0 - x, Block::Main);
    }

    // The Shift row.
    let y = MAIN_Y + 3.0;
    let x = c.key(Phys::ShiftLeft, 0.0, y, 2.25, Block::Main);
    let mut x = c.ones(y, x, &BOTTOM_LETTERS);
    if jis {
        x = c.ones(y, x, &[Phys::IntlRo]);
    }
    c.key(Phys::ShiftRight, x, y, 15.0 - x, Block::Main);

    // The bottom row. The Mac ones are a simplification of Apple's widths.
    let y = MAIN_Y + 4.0;
    let m = Block::Main;
    let bottom: &[(Phys, f32)] = match b {
        Board::PcUs => &[
            (Phys::ControlLeft, 1.25),
            (Phys::MetaLeft, 1.25),
            (Phys::AltLeft, 1.25),
            (Phys::Space, 6.25),
            (Phys::AltRight, 1.25),
            (Phys::MetaRight, 1.25),
            (Phys::ContextMenu, 1.25),
            (Phys::ControlRight, 1.25),
        ],
        Board::PcJis => &[
            (Phys::ControlLeft, 1.25),
            (Phys::MetaLeft, 1.25),
            (Phys::AltLeft, 1.25),
            (Phys::NonConvert, 1.25),
            (Phys::Space, 3.75),
            (Phys::Convert, 1.25),
            (Phys::KanaMode, 1.25),
            (Phys::AltRight, 1.25),
            (Phys::ContextMenu, 1.25),
            (Phys::ControlRight, 1.25),
        ],
        Board::MacUs => &[
            (Phys::ControlLeft, 1.25),
            (Phys::AltLeft, 1.25),
            (Phys::MetaLeft, 1.5),
            (Phys::Space, 7.0),
            (Phys::MetaRight, 1.5),
            (Phys::AltRight, 1.25),
        ],
        Board::MacJis => &[
            (Phys::ControlLeft, 1.25),
            (Phys::AltLeft, 1.25),
            (Phys::MetaLeft, 1.25),
            (Phys::Lang2, 1.25),
            (Phys::Space, 5.0),
            (Phys::Lang1, 1.25),
            (Phys::MetaRight, 1.25),
            (Phys::AltRight, 1.25),
        ],
    };
    c.run(y, 0.0, m, bottom);

    // The block of six, the arrows under it. On a Mac, Fn is where Insert is.
    let n = Block::Nav;
    let first = if mac { Phys::Fn } else { Phys::Insert };
    c.run(MAIN_Y, 15.5, n, &[(first, 1.0), (Phys::Home, 1.0), (Phys::PageUp, 1.0)]);
    c.run(MAIN_Y + 1.0, 15.5, n, &[(Phys::Delete, 1.0), (Phys::End, 1.0), (Phys::PageDown, 1.0)]);
    c.key(Phys::ArrowUp, 16.5, MAIN_Y + 3.0, 1.0, Block::Arrows);
    c.run(MAIN_Y + 4.0, 15.5, Block::Arrows, &[(Phys::ArrowLeft, 1.0), (Phys::ArrowDown, 1.0), (Phys::ArrowRight, 1.0)]);
    c.caps
}

static CAPS: [OnceLock<Vec<Cap>>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];

/// The keys of `b`, in reading order (the F row first, then the number row
/// to the bottom row, the nav block and the arrows after their rows).
pub fn caps(b: Board) -> &'static [Cap] {
    let i = match b {
        Board::PcUs => 0,
        Board::PcJis => 1,
        Board::MacUs => 2,
        Board::MacJis => 3,
    };
    CAPS[i].get_or_init(|| build(b))
}

/// The ASCII glyphs from the space on, to hand out `'static` strs of one character.
const ASCII: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

fn glyph(c: char) -> &'static str {
    match c {
        ' '..='~' => {
            let i = c as usize - ' ' as usize;
            &ASCII[i..i + 1]
        }
        '\u{a5}' => "\u{a5}",
        _ => "",
    }
}

/// The lower-case letter of a letter key.
fn letter(p: Phys) -> Option<char> {
    Some(match p {
        Phys::KeyA => 'a',
        Phys::KeyB => 'b',
        Phys::KeyC => 'c',
        Phys::KeyD => 'd',
        Phys::KeyE => 'e',
        Phys::KeyF => 'f',
        Phys::KeyG => 'g',
        Phys::KeyH => 'h',
        Phys::KeyI => 'i',
        Phys::KeyJ => 'j',
        Phys::KeyK => 'k',
        Phys::KeyL => 'l',
        Phys::KeyM => 'm',
        Phys::KeyN => 'n',
        Phys::KeyO => 'o',
        Phys::KeyP => 'p',
        Phys::KeyQ => 'q',
        Phys::KeyR => 'r',
        Phys::KeyS => 's',
        Phys::KeyT => 't',
        Phys::KeyU => 'u',
        Phys::KeyV => 'v',
        Phys::KeyW => 'w',
        Phys::KeyX => 'x',
        Phys::KeyY => 'y',
        Phys::KeyZ => 'z',
        _ => return None,
    })
}

/// The number of a digit key.
fn digit(p: Phys) -> Option<u32> {
    Some(match p {
        Phys::Digit0 => 0,
        Phys::Digit1 => 1,
        Phys::Digit2 => 2,
        Phys::Digit3 => 3,
        Phys::Digit4 => 4,
        Phys::Digit5 => 5,
        Phys::Digit6 => 6,
        Phys::Digit7 => 7,
        Phys::Digit8 => 8,
        Phys::Digit9 => 9,
        _ => return None,
    })
}

/// What a symbol key types: plain, and with Shift (`None`: nothing).
/// Per the W3C key tables of the two layouts.
fn symbol(f: Family, p: Phys) -> Option<(char, Option<char>)> {
    let us = f == Family::Us;
    Some(match p {
        Phys::Minus => ('-', Some(if us { '_' } else { '=' })),
        Phys::Equal if us => ('=', Some('+')),
        Phys::Equal => ('^', Some('~')),
        Phys::BracketLeft if us => ('[', Some('{')),
        Phys::BracketLeft => ('@', Some('`')),
        Phys::BracketRight if us => (']', Some('}')),
        Phys::BracketRight => ('[', Some('{')),
        Phys::Backslash if us => ('\\', Some('|')),
        Phys::Backslash => (']', Some('}')),
        Phys::Semicolon => (';', Some(if us { ':' } else { '+' })),
        Phys::Quote if us => ('\'', Some('"')),
        Phys::Quote => (':', Some('*')),
        Phys::Backquote if us => ('`', Some('~')),
        Phys::Comma => (',', Some('<')),
        Phys::Period => ('.', Some('>')),
        Phys::Slash => ('/', Some('?')),
        _ => return None,
    })
}

/// The shifted symbol of a digit key.
fn digit_shifted(f: Family, n: u32) -> Option<char> {
    let us = "!@#$%^&*()";
    let jis = "!\"#$%&'() ";
    let i = if n == 0 { 9 } else { n as usize - 1 };
    let c = if f == Family::Us { us } else { jis }.chars().nth(i)?;
    (c != ' ').then_some(c)
}

/// What is printed on a key, for drawing only: the plain legend and the
/// shifted one. See [`types`] for what the key types, which is not always
/// the same.
pub fn legend(b: Board, p: Phys) -> (&'static str, Option<&'static str>) {
    let mac = b.is_mac();
    let f = b.family();
    if let Some(c) = letter(p) {
        return (glyph(c.to_ascii_uppercase()), None);
    }
    if let Some(n) = digit(p) {
        return (glyph(char::from_digit(n, 10).unwrap_or(' ')), digit_shifted(f, n).map(glyph));
    }
    let named = match p {
        Phys::F1 => "F1",
        Phys::F2 => "F2",
        Phys::F3 => "F3",
        Phys::F4 => "F4",
        Phys::F5 => "F5",
        Phys::F6 => "F6",
        Phys::F7 => "F7",
        Phys::F8 => "F8",
        Phys::F9 => "F9",
        Phys::F10 => "F10",
        Phys::F11 => "F11",
        Phys::F12 => "F12",
        Phys::Escape if mac => "esc",
        Phys::Escape => "Esc",
        Phys::Tab if mac => "tab",
        Phys::Tab => "Tab",
        Phys::Enter if mac => "return",
        Phys::Enter => "Enter",
        Phys::Backspace if mac => "delete",
        Phys::Backspace => "Backspace",
        Phys::CapsLock if mac => "caps",
        Phys::CapsLock => "Caps",
        Phys::ShiftLeft | Phys::ShiftRight if mac => "shift \u{21e7}",
        Phys::ShiftLeft | Phys::ShiftRight => "Shift",
        Phys::ControlLeft | Phys::ControlRight if mac => "control \u{2303}",
        Phys::ControlLeft | Phys::ControlRight => "Ctrl",
        Phys::AltLeft | Phys::AltRight if mac => "option \u{2325}",
        Phys::AltLeft | Phys::AltRight => "Alt",
        Phys::MetaLeft | Phys::MetaRight if mac => "command \u{2318}",
        Phys::MetaLeft | Phys::MetaRight => "Win",
        Phys::ContextMenu => "Menu",
        Phys::Fn => "fn",
        Phys::Insert => "Ins",
        Phys::Delete => "Del",
        Phys::Home => "Home",
        Phys::End => "End",
        Phys::PageUp => "PgUp",
        Phys::PageDown => "PgDn",
        Phys::ArrowUp => "\u{2191}",
        Phys::ArrowLeft => "\u{2190}",
        Phys::ArrowDown => "\u{2193}",
        Phys::ArrowRight => "\u{2192}",
        Phys::NonConvert => "\u{7121}\u{5909}\u{63db}",
        Phys::Convert => "\u{5909}\u{63db}",
        Phys::KanaMode | Phys::Lang1 => "\u{304b}\u{306a}",
        Phys::Lang2 => "\u{82f1}\u{6570}",
        Phys::Space => "",
        Phys::IntlYen if f == Family::Jis => return ("\u{a5}", Some("|")),
        Phys::IntlRo if f == Family::Jis => return ("\\", Some("_")),
        Phys::Backquote if f == Family::Jis => "\u{534a}/\u{5168}",
        _ => {
            return match symbol(f, p) {
                Some((plain, shifted)) => (glyph(plain), shifted.map(glyph)),
                None => ("", None),
            }
        }
    };
    (named, None)
}

/// The character the OS layout types when `p` is pressed (with Shift when
/// `shift`), which is not always what the legend shows. `None` for keys that
/// type nothing: the named keys, the modifiers and the inert ones.
pub fn types(b: Board, p: Phys, shift: bool) -> Option<char> {
    let f = b.family();
    if let Some(c) = letter(p) {
        return Some(if shift { c.to_ascii_uppercase() } else { c });
    }
    if let Some(n) = digit(p) {
        return if shift { digit_shifted(f, n) } else { char::from_digit(n, 10) };
    }
    if p == Phys::Space {
        return Some(' ');
    }
    // The JIS key with the yen sign types a backslash on a PC; a Mac types the
    // yen sign itself.
    match (b, p) {
        (Board::PcJis, Phys::IntlYen) => return Some(if shift { '|' } else { '\\' }),
        // to check on a Mac
        (Board::MacJis, Phys::IntlYen) => return Some(if shift { '|' } else { '\u{a5}' }),
        // to check on a Mac
        (Board::PcJis | Board::MacJis, Phys::IntlRo) => return Some(if shift { '_' } else { '\\' }),
        _ => {}
    }
    let (plain, shifted) = symbol(f, p)?;
    if shift {
        shifted
    } else {
        Some(plain)
    }
}
