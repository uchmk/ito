//! yazi's key notation: `a`, `Z`, `<C-a>`, `<Enter>`, `<F5>`, `<A-S-Up>` …,
//! shared by uchmk's apps that read a yazi-style `keymap.toml` (yagura;
//! kura's `config/keys.rs` is where it started).
//!
//! `C-` is Ctrl on Windows and Linux and Cmd on macOS (where Ctrl itself
//! still counts as `C` too), so one default keymap serves every OS: `<C-S-e>`
//! is Ctrl+Shift+E on the one and Cmd+Shift+E on the other. With the `egui`
//! feature, [`from_egui`] turns egui's key events into the same [`Key`] the
//! notation parses to.
//!
//! What a key *does* (the keymap's layers, the commands) stays in each app:
//! those differ from app to app, the spelling of a key does not.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Named {
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    F(u8),
}

impl Named {
    fn label(self) -> String {
        match self {
            Self::Enter => "Enter".into(),
            Self::Esc => "Esc".into(),
            Self::Tab => "Tab".into(),
            Self::BackTab => "BackTab".into(),
            Self::Backspace => "Backspace".into(),
            Self::Delete => "Delete".into(),
            Self::Insert => "Insert".into(),
            Self::Home => "Home".into(),
            Self::End => "End".into(),
            Self::PageUp => "PageUp".into(),
            Self::PageDown => "PageDown".into(),
            Self::Up => "Up".into(),
            Self::Down => "Down".into(),
            Self::Left => "Left".into(),
            Self::Right => "Right".into(),
            Self::F(n) => format!("F{n}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Code {
    Char(char),
    Named(Named),
}

/// One keypress. `shift` is only meaningful for [`Code::Named`] keys and for
/// chords that carry another modifier — a bare `A` is simply `Char('A')`, the
/// same way yazi spells it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub code: Code,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub sup: bool,
}

impl Key {
    pub fn plain(code: Code) -> Self {
        Self { code, ctrl: false, alt: false, shift: false, sup: false }
    }

    pub fn char(c: char) -> Self {
        Self::plain(Code::Char(c))
    }

    /// A ctrl chord, for the keys egui hands over as clipboard events.
    pub fn ctrl(c: char) -> Self {
        Self { code: Code::Char(c), ctrl: true, alt: false, shift: false, sup: false }
    }

    pub fn is_bare_char(&self) -> bool {
        matches!(self.code, Code::Char(_)) && !self.ctrl && !self.alt && !self.sup
    }

    /// Parse a single yazi key token. Returns `None` for unrecognized notation.
    pub fn parse(s: &str) -> Option<Self> {
        let mut chars = s.chars();
        let first = chars.next()?;
        if first != '<' {
            // A bare token must be exactly one character.
            if chars.next().is_some() {
                return None;
            }
            return Some(Self::char(first));
        }
        if !s.ends_with('>') || s.len() < 3 {
            return None;
        }
        let inner = &s[1..s.len() - 1];
        let mut key = Self { code: Code::Char(' '), ctrl: false, alt: false, shift: false, sup: false };

        // Split on '-', but the final segment is the key itself and may be "-".
        let parts: Vec<&str> = split_mods(inner);
        let (mods, last) = parts.split_at(parts.len() - 1);
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                // `p` and `primary` were ito 0.5's name for what `C` is now.
                "c" | "ctrl" | "p" | "primary" => key.ctrl = true,
                "a" | "alt" | "m" | "meta" => key.alt = true,
                "s" | "shift" => key.shift = true,
                "d" | "cmd" | "super" | "win" => key.sup = true,
                _ => return None,
            }
        }
        let name = last[0];
        key.code = match name.to_ascii_lowercase().as_str() {
            "enter" | "cr" | "return" => Code::Named(Named::Enter),
            "esc" | "escape" => Code::Named(Named::Esc),
            "tab" => Code::Named(Named::Tab),
            "backtab" => Code::Named(Named::BackTab),
            "backspace" | "bs" => Code::Named(Named::Backspace),
            "delete" | "del" => Code::Named(Named::Delete),
            "insert" | "ins" => Code::Named(Named::Insert),
            "home" => Code::Named(Named::Home),
            "end" => Code::Named(Named::End),
            "pageup" | "pgup" => Code::Named(Named::PageUp),
            "pagedown" | "pgdn" => Code::Named(Named::PageDown),
            "up" => Code::Named(Named::Up),
            "down" => Code::Named(Named::Down),
            "left" => Code::Named(Named::Left),
            "right" => Code::Named(Named::Right),
            "space" => Code::Char(' '),
            "lt" => Code::Char('<'),
            "gt" => Code::Char('>'),
            "minus" => Code::Char('-'),
            other => {
                if let Some(n) = other.strip_prefix('f').and_then(|d| d.parse::<u8>().ok()) {
                    Code::Named(Named::F(n))
                } else {
                    let mut it = name.chars();
                    let c = it.next()?;
                    if it.next().is_some() {
                        return None;
                    }
                    // `<C-A>` means ctrl+shift+a; normalize to lowercase + shift.
                    if c.is_ascii_uppercase() && (key.ctrl || key.alt || key.sup) {
                        key.shift = true;
                        Code::Char(c.to_ascii_lowercase())
                    } else {
                        Code::Char(c)
                    }
                }
            }
        };
        // Shift+Tab arrives as BackTab with shift held, whichever way the
        // config spells it: `<BackTab>`, `<S-Tab>` or `<S-BackTab>`.
        let backtab = Code::Named(Named::BackTab);
        if key.code == backtab || (key.code == Code::Named(Named::Tab) && key.shift) {
            key.code = backtab;
            key.shift = true;
        }
        Some(key)
    }
}

fn split_mods(inner: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = inner.as_bytes();
    let mut start = 0;
    for i in 0..bytes.len() {
        if bytes[i] == b'-' && i + 1 < bytes.len() && i > start {
            out.push(&inner[start..i]);
            start = i + 1;
        }
    }
    out.push(&inner[start..]);
    out
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named = match self.code {
            Code::Char(' ') => Some("Space".to_string()),
            Code::Char('<') => Some("lt".to_string()),
            Code::Named(n) => Some(n.label()),
            Code::Char(_) => None,
        };
        let has_mods = self.ctrl || self.alt || self.sup || (self.shift && named.is_some());
        match (named, has_mods) {
            (None, false) => {
                let Code::Char(c) = self.code else { unreachable!() };
                write!(f, "{c}")
            }
            (label, _) => {
                write!(f, "<")?;
                if self.ctrl {
                    write!(f, "C-")?;
                }
                if self.alt {
                    write!(f, "A-")?;
                }
                // BackTab is Shift+Tab already.
                if self.shift && self.code != Code::Named(Named::BackTab) {
                    write!(f, "S-")?;
                }
                if self.sup {
                    write!(f, "D-")?;
                }
                match label {
                    Some(l) => write!(f, "{l}>"),
                    None => {
                        let Code::Char(c) = self.code else { unreachable!() };
                        write!(f, "{c}>")
                    }
                }
            }
        }
    }
}

/// Render a chord sequence the way the help shows it.
pub fn render_seq(keys: &[Key]) -> String {
    keys.iter().map(|k| k.to_string()).collect()
}

impl Key {
    /// The key the way the OS names it, for what a person reads on screen
    /// (help, palette, settings, status bar) rather than the keymap's
    /// notation: `<C-S-p>` is `Ctrl+Shift+P` on Windows and Linux and `⇧⌘P`
    /// on macOS, where `C` is Cmd.
    pub fn human(&self) -> String {
        let name = match self.code {
            Code::Char(' ') => "Space".to_string(),
            Code::Char(c) if self.ctrl || self.alt || self.sup => c.to_uppercase().to_string(),
            Code::Char(c) => c.to_string(),
            Code::Named(Named::BackTab) => "Tab".to_string(),
            Code::Named(n) => n.label(),
        };
        let shift = self.shift || self.code == Code::Named(Named::BackTab);
        if cfg!(target_os = "macos") {
            let mut out = String::new();
            for (on, sign) in [(self.alt, '⌥'), (shift, '⇧'), (self.ctrl || self.sup, '⌘')] {
                if on {
                    out.push(sign);
                }
            }
            out + &name
        } else {
            let mods = [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (shift, "Shift"), (self.sup, "Win")];
            let mut parts: Vec<&str> = mods.iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
            parts.push(&name);
            parts.join("+")
        }
    }
}

/// A chord sequence the way the OS names its keys ([`Key::human`]), one
/// space between the keys (`g h`).
pub fn human_seq(keys: &[Key]) -> String {
    keys.iter().map(Key::human).collect::<Vec<_>>().join(" ")
}

/// A chord sequence the way a reader of a help or a status line sees it: on
/// macOS, where `C` is Cmd, `<C-q>` is `⌘Q` and `<C-A-x>` is `⌥⌘X`
/// (`⌥⇧⌘` in that order, then the key); a bare named key stays `<Enter>`;
/// elsewhere it is the keymap's own notation ([`render_seq`]). Matching and
/// the keymap keep using [`render_seq`]; this is only for text a reader sees.
/// kura's spelling; [`human_seq`] is the one that says `Ctrl+Q`.
pub fn display_seq(keys: &[Key]) -> String {
    display_seq_for(keys, cfg!(target_os = "macos"))
}

/// [`display_seq`] for `mac` named, so either spelling can be tested anywhere.
pub fn display_seq_for(keys: &[Key], mac: bool) -> String {
    if !mac {
        return render_seq(keys);
    }
    keys.iter().map(mac_key).collect()
}

/// One key the macOS way: `⌥⇧⌘` in that order, then the key.
fn mac_key(k: &Key) -> String {
    let label = match k.code {
        Code::Char(' ') => "Space".to_string(),
        Code::Char(c) => c.to_uppercase().to_string(),
        Code::Named(n) => n.label(),
    };
    let named = !matches!(k.code, Code::Char(c) if c != ' ');
    let shift = k.shift && k.code != Code::Named(Named::BackTab);
    if !(k.ctrl || k.alt || k.sup || shift) {
        return if named { format!("<{label}>") } else { k.to_string() };
    }
    let mut out = String::new();
    if k.alt {
        out.push('⌥');
    }
    if shift {
        out.push('⇧');
    }
    if k.ctrl || k.sup {
        out.push('⌘');
    }
    out + &label
}

/// Translate an egui key event into our representation.
///
/// Returns `None` for events that arrive as text instead (bare printable
/// characters), so a keypress is never handled twice. On macOS both Cmd and
/// Ctrl come out as `ctrl`, so `<C-q>` is Cmd+Q there.
#[cfg(feature = "egui")]
pub fn from_egui(key: egui::Key, mods: &egui::Modifiers) -> Option<Key> {
    use egui::Key as K;
    // Cmd is `C` on a Mac (since 0.6.0): one keymap line serves every OS.
    // `<D-…>` still parses, as yazi's, but no key event carries it.
    let ctrl = mods.ctrl || mods.mac_cmd;
    let alt = mods.alt;
    let sup = false;
    let named = |n: Named| Some(Key { code: Code::Named(n), ctrl, alt, shift: mods.shift, sup });
    match key {
        K::Enter => named(Named::Enter),
        K::Escape => named(Named::Esc),
        K::Tab => {
            if mods.shift {
                Some(Key { code: Code::Named(Named::BackTab), ctrl, alt, shift: true, sup })
            } else {
                named(Named::Tab)
            }
        }
        K::Backspace => named(Named::Backspace),
        K::Delete => named(Named::Delete),
        K::Insert => named(Named::Insert),
        K::Home => named(Named::Home),
        K::End => named(Named::End),
        K::PageUp => named(Named::PageUp),
        K::PageDown => named(Named::PageDown),
        K::ArrowUp => named(Named::Up),
        K::ArrowDown => named(Named::Down),
        K::ArrowLeft => named(Named::Left),
        K::ArrowRight => named(Named::Right),
        K::F1 => named(Named::F(1)),
        K::F2 => named(Named::F(2)),
        K::F3 => named(Named::F(3)),
        K::F4 => named(Named::F(4)),
        K::F5 => named(Named::F(5)),
        K::F6 => named(Named::F(6)),
        K::F7 => named(Named::F(7)),
        K::F8 => named(Named::F(8)),
        K::F9 => named(Named::F(9)),
        K::F10 => named(Named::F(10)),
        K::F11 => named(Named::F(11)),
        K::F12 => named(Named::F(12)),
        other => {
            // Printable keys: only interesting when a non-shift modifier is
            // held, otherwise the character arrives as an `Event::Text`.
            if !ctrl && !alt && !sup {
                return None;
            }
            let c = printable(other)?;
            // A shifted symbol arrives as its own character: egui reports `Plus`
            // for shift+equals and `Colon` for shift+semicolon, so the shift is
            // already inside `c`. Carrying `mods.shift` as well counts it twice,
            // and then no binding spelled `<C-+>` can ever match, on any layout.
            // Letters are the exception -- `printable` lowercases them, so the
            // shift is the only thing telling `<C-A>` from `<C-a>`.
            let shift = mods.shift && c.is_ascii_alphabetic();
            Some(Key { code: Code::Char(c), ctrl, alt, shift, sup })
        }
    }
}

/// The chord behind one of egui's clipboard events, `events[at]`.
///
/// egui-winit turns Ctrl+C, X and V (Cmd on a Mac), Shift held or not, and on
/// Windows Ctrl+Insert, Shift+Delete and Shift+Insert, into `Copy`, `Cut` and
/// `Paste`, and never sends the key press. The event carries no modifiers:
/// they are taken from the release of the same key later in `events` when it
/// came in the same frame (a quick chord is often let go by then, and `now`,
/// the modifiers at the end of the frame, has lost them), else from `now`.
/// Ctrl+Insert comes back as Ctrl+C: egui gives the two one event.
#[cfg(feature = "egui")]
pub fn from_clipboard(events: &[egui::Event], at: usize, now: &egui::Modifiers) -> Option<Key> {
    use egui::Key as K;
    let (letter, bare) = match events.get(at)? {
        egui::Event::Copy => (K::C, K::Insert),
        egui::Event::Cut => (K::X, K::Delete),
        egui::Event::Paste(_) => (K::V, K::Insert),
        _ => return None,
    };
    let held = events[at + 1..]
        .iter()
        .find_map(|e| match e {
            egui::Event::Key { key, pressed: false, modifiers, .. } if *key == letter || *key == bare => Some(*modifiers),
            _ => None,
        })
        .unwrap_or(*now);
    let ctrl = held.ctrl || held.command || held.mac_cmd;
    let mods = egui::Modifiers { ctrl, shift: held.shift || !ctrl, ..held };
    from_egui(if ctrl { letter } else { bare }, &mods)
}

/// The character a key types without Shift (letters lowercase), for keys
/// with one.
#[cfg(feature = "egui")]
pub fn printable(key: egui::Key) -> Option<char> {
    use egui::Key as K;
    Some(match key {
        K::Space => ' ',
        K::Comma => ',',
        K::Period => '.',
        K::Semicolon => ';',
        K::Colon => ':',
        K::Backslash => '\\',
        K::Slash => '/',
        K::Pipe => '|',
        K::Questionmark => '?',
        K::Exclamationmark => '!',
        K::OpenBracket => '[',
        K::CloseBracket => ']',
        K::OpenCurlyBracket => '{',
        K::CloseCurlyBracket => '}',
        K::Backtick => '`',
        K::Minus => '-',
        K::Plus => '+',
        K::Equals => '=',
        K::Quote => '\'',
        K::Num0 => '0',
        K::Num1 => '1',
        K::Num2 => '2',
        K::Num3 => '3',
        K::Num4 => '4',
        K::Num5 => '5',
        K::Num6 => '6',
        K::Num7 => '7',
        K::Num8 => '8',
        K::Num9 => '9',
        K::A => 'a',
        K::B => 'b',
        K::C => 'c',
        K::D => 'd',
        K::E => 'e',
        K::F => 'f',
        K::G => 'g',
        K::H => 'h',
        K::I => 'i',
        K::J => 'j',
        K::K => 'k',
        K::L => 'l',
        K::M => 'm',
        K::N => 'n',
        K::O => 'o',
        K::P => 'p',
        K::Q => 'q',
        K::R => 'r',
        K::S => 's',
        K::T => 't',
        K::U => 'u',
        K::V => 'v',
        K::W => 'w',
        K::X => 'x',
        K::Y => 'y',
        K::Z => 'z',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `<A-G>` as written in a keymap is what Alt+Shift+G arrives as: the
    /// letter lowercased with Shift carried beside it (Q76's preview end).
    /// What a reader sees on macOS (Cmd is `C` there) and elsewhere.
    #[test]
    fn display_seq_spells_macos_keys_with_symbols() {
        let seq = |s: &str| -> Vec<Key> { s.split(' ').map(|k| Key::parse(k).unwrap()).collect() };
        let mac = |s: &str| display_seq_for(&seq(s), true);
        assert_eq!(mac("<C-q>"), "⌘Q");
        assert_eq!(mac("<C-A-x>"), "⌥⌘X");
        assert_eq!(mac("<C-A>"), "⇧⌘A");
        assert_eq!(mac("<A-g>"), "⌥G");
        assert_eq!(mac("<C-F5>"), "⌘F5");
        assert_eq!(mac("<Enter>"), "<Enter>");
        assert_eq!(mac("<BackTab>"), "<BackTab>");
        assert_eq!(mac("g g"), "gg");
        assert_eq!(mac("G"), "G");
        assert_eq!(display_seq_for(&seq("<C-q>"), false), "<C-q>");
    }

    #[test]
    #[cfg(feature = "egui")]
    fn alt_shift_letter_matches_its_capital() {
        let mods = egui::Modifiers { alt: true, shift: true, ..Default::default() };
        assert_eq!(from_egui(egui::Key::G, &mods), Some(Key::parse("<A-G>").unwrap()));
        let alt = egui::Modifiers { alt: true, ..Default::default() };
        assert_eq!(from_egui(egui::Key::G, &alt), Some(Key::parse("<A-g>").unwrap()));
        assert_ne!(Key::parse("<A-g>"), Key::parse("<A-G>"));
    }

    #[test]
    #[cfg(feature = "egui")]
    fn a_clipboard_event_is_its_chord_again() {
        let none = egui::Modifiers::NONE;
        let ctrl = egui::Modifiers::CTRL;
        let shift = egui::Modifiers::SHIFT;
        let up = |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers };
        let key = |events: &[egui::Event], now| from_clipboard(events, 0, &now).map(|k| k.to_string());
        let paste = || egui::Event::Paste("t".into());

        assert_eq!(key(&[paste()], ctrl), Some("<C-v>".into()));
        assert_eq!(key(&[paste()], ctrl | shift), Key::parse("<C-S-v>").map(|k| k.to_string()));
        assert_eq!(key(&[paste()], shift), Some("<S-Insert>".into()));
        assert_eq!(key(&[egui::Event::Cut], shift), Some("<S-Delete>".into()));
        assert_eq!(key(&[egui::Event::Copy], ctrl), Some("<C-c>".into()));
        assert_eq!(key(&[egui::Event::Cut], ctrl | egui::Modifiers::ALT), Some("<C-A-x>".into()));
        // Let go of within the frame: the release still says what was held.
        assert_eq!(key(&[paste(), up(egui::Key::V, ctrl | shift)], none), Key::parse("<C-S-v>").map(|k| k.to_string()));
        assert_eq!(key(&[paste(), up(egui::Key::Insert, shift)], none), Some("<S-Insert>".into()));
        // Another key's release says nothing about this one.
        assert_eq!(key(&[paste(), up(egui::Key::A, none)], ctrl), Some("<C-v>".into()));
        assert_eq!(key(&[egui::Event::Text("v".into())], ctrl), None);
    }

    #[test]
    fn parses_notation() {
        assert_eq!(Key::parse("a"), Some(Key::char('a')));
        assert_eq!(Key::parse("A"), Some(Key::char('A')));
        assert_eq!(Key::parse("<Space>"), Some(Key::char(' ')));
        assert_eq!(Key::parse("<Enter>"), Some(Key::plain(Code::Named(Named::Enter))));
        let c = Key::parse("<C-a>").unwrap();
        assert!(c.ctrl && c.code == Code::Char('a'));
        let cs = Key::parse("<C-S-Up>").unwrap();
        assert!(cs.ctrl && cs.shift && cs.code == Code::Named(Named::Up));
        assert_eq!(Key::parse("<F5>").unwrap().code, Code::Named(Named::F(5)));
    }

    /// A symbol reached with shift still matches a binding written without it.
    ///
    /// `<C-+>` and `<C-=>` both mean "bigger", because which physical key that
    /// is depends on the layout: on US `+` is shift+equals, on JIS it is
    /// shift+semicolon and `=` is shift+minus. Whichever one is pressed, egui
    /// reports the *character* -- `Plus` or `Equals` -- and `shift: true`
    /// beside it. While `from_egui` kept that shift in the `Key`, neither
    /// binding could ever match, on any keyboard: `<C-+>` was dead on US too.
    #[test]
    #[cfg(feature = "egui")]
    fn a_shifted_symbol_matches_the_binding_without_the_shift() {
        let shifted = egui::Modifiers { ctrl: true, shift: true, ..Default::default() };

        // JIS: `+` is shift+`;`, `=` is shift+`-`. US: `+` is shift+`=`.
        for (key, spelling) in [(egui::Key::Plus, "<C-+>"), (egui::Key::Equals, "<C-=>")] {
            let pressed = from_egui(key, &shifted).expect("a chord, so it is a key event");
            assert_eq!(Some(pressed), Key::parse(spelling), "`{spelling}` must match however the layout reaches that character",);
            assert!(!pressed.shift, "the shift is inside the character already");
        }

        // A letter is the exception: `printable` lowercases it, so the shift is
        // the only thing left saying `<C-A>` apart from `<C-a>`.
        let upper = from_egui(egui::Key::A, &shifted).unwrap();
        assert!(upper.shift, "a letter keeps its shift");
        assert_eq!(Some(upper), Key::parse("<C-A>"));
        assert_ne!(Some(upper), Key::parse("<C-a>"));
    }

    /// A shifted letter is spelled `T`, not `<S-t>`.
    ///
    /// Shift+T reaches the app as typed text, so the shift is already inside the
    /// character and the key carries no modifier. `<S-t>` is accepted by the
    /// parser -- it is valid notation -- and then sits in the keymap matching
    /// nothing, which looks like a binding that was ignored rather than one that
    /// was written wrong. Asserted so the two spellings stay visibly different.
    #[test]
    fn a_shifted_letter_is_the_uppercase_character() {
        let typed = Key::char('T'); // what `Event::Text("T")` produces
        assert_eq!(Key::parse("T"), Some(typed));
        assert!(!typed.shift, "the shift is in the character, not the modifier");

        let wrong = Key::parse("<S-t>").expect("valid notation, just never matched");
        assert_ne!(wrong, typed);
        assert!(wrong.shift && wrong.code == Code::Char('t'));

        // With another modifier held there is no text event, so `<C-S-t>` and
        // `<C-T>` are the same key and both do match.
        assert_eq!(Key::parse("<C-S-t>"), Key::parse("<C-T>"));
    }

    /// ito 0.5's `P-` (and `primary-`) reads as `C-`.
    #[test]
    fn the_old_primary_is_ctrl() {
        assert_eq!(Key::parse("<P-S-e>"), Key::parse("<C-S-e>"));
        assert_eq!(Key::parse("<primary-q>"), Key::parse("<C-q>"));
    }

    /// `C` is Cmd on macOS and Ctrl elsewhere, and Ctrl on macOS still
    /// matches it too.
    #[test]
    #[cfg(feature = "egui")]
    fn ctrl_is_cmd_on_macos() {
        let c = Key::parse("<C-q>").unwrap();
        let held = if cfg!(target_os = "macos") {
            egui::Modifiers { command: true, mac_cmd: true, ..Default::default() }
        } else {
            egui::Modifiers { command: true, ctrl: true, ..Default::default() }
        };
        assert_eq!(from_egui(egui::Key::Q, &held), Some(c));
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        assert_eq!(from_egui(egui::Key::Q, &ctrl), Some(c));
    }

    /// What a person reads names the OS's keys.
    #[test]
    fn keys_are_shown_by_the_os_names() {
        let show = |s: &str| Key::parse(s).unwrap().human();
        if cfg!(target_os = "macos") {
            assert_eq!(show("<C-S-p>"), "⇧⌘P");
            assert_eq!(show("<C-,>"), "⌘,");
        } else {
            assert_eq!(show("<C-S-p>"), "Ctrl+Shift+P");
            assert_eq!(show("<C-,>"), "Ctrl+,");
            assert_eq!(show("<BackTab>"), "Shift+Tab");
        }
        assert_eq!(show("<F1>"), "F1");
        assert_eq!(human_seq(&[Key::char('g'), Key::char('h')]), "g h");
    }

    #[test]
    fn round_trips() {
        for s in ["a", "<C-a>", "<Enter>", "<Space>", "<F12>", "<A-S-Left>", "<BackTab>", "<C-BackTab>"] {
            let k = Key::parse(s).unwrap();
            assert_eq!(Key::parse(&k.to_string()), Some(k), "{s}");
        }
    }

    #[test]
    #[cfg(feature = "egui")]
    fn shift_tab_matches_however_it_is_written() {
        let pressed = from_egui(egui::Key::Tab, &egui::Modifiers::SHIFT);
        for s in ["<BackTab>", "<S-Tab>", "<S-BackTab>", "<s-tab>"] {
            let k = Key::parse(s);
            assert_eq!(k, pressed, "{s}");
            assert_eq!(k.unwrap().to_string(), "<BackTab>", "{s}");
        }
        // Plain Tab stays Tab.
        assert_eq!(Key::parse("<Tab>"), from_egui(egui::Key::Tab, &egui::Modifiers::NONE));
    }
}
