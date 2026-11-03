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
        self.human_on(cfg!(target_os = "macos"), false)
    }

    /// [`Key::human`] for an app that keeps Ctrl and Cmd apart on macOS
    /// (`C` Ctrl, `D` Cmd) when `apart`: there Ctrl is `⌃` and only Cmd is
    /// `⌘` (`⌃⌥⇧⌘` in that order). Elsewhere, or not `apart`, it is
    /// [`Key::human`].
    pub fn human_for(&self, apart: bool) -> String {
        self.human_on(cfg!(target_os = "macos"), apart)
    }

    /// [`Key::human_for`] with `mac` named, so either spelling can be tested
    /// anywhere.
    fn human_on(&self, mac: bool, apart: bool) -> String {
        let name = match self.code {
            Code::Char(' ') => "Space".to_string(),
            Code::Char(c) if self.ctrl || self.alt || self.sup => c.to_uppercase().to_string(),
            Code::Char(c) => c.to_string(),
            Code::Named(Named::BackTab) => "Tab".to_string(),
            Code::Named(n) => n.label(),
        };
        let shift = self.shift || self.code == Code::Named(Named::BackTab);
        if mac {
            let mut out = String::new();
            let (ctrl, cmd) = if apart { (self.ctrl, self.sup) } else { (false, self.ctrl || self.sup) };
            for (on, sign) in [(ctrl, '⌃'), (self.alt, '⌥'), (shift, '⇧'), (cmd, '⌘')] {
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
    human_seq_for(keys, false)
}

/// [`human_seq`] with each key as [`Key::human_for`] has it.
pub fn human_seq_for(keys: &[Key], apart: bool) -> String {
    keys.iter().map(|k| k.human_for(apart)).collect::<Vec<_>>().join(" ")
}

/// Fixed prose that names a key, the way the OS spells it: `Ctrl+Z to undo`
/// reads `Cmd+Z to undo` on macOS, where the keymap's `C` is Cmd. [`human_seq`]
/// is for a binding the keymap holds; this is for a sentence in a translation
/// table that cannot be built from one.
///
/// Only a `Ctrl+` that starts a word is read as a key. A chord that ends in
/// `Tab` (`Ctrl+Tab`, `Ctrl+Shift+Tab`) keeps Ctrl: switching pages is the
/// Control key on a Mac too. Elsewhere the text is returned as it is.
pub fn human_text(text: &str) -> std::borrow::Cow<'_, str> {
    human_text_for(text, cfg!(target_os = "macos"))
}

/// [`human_text`] with `mac` named, so either spelling can be tested anywhere.
pub fn human_text_for(text: &str, mac: bool) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if !mac || !text.contains("Ctrl+") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(i) = text[at..].find("Ctrl+").map(|i| i + at) {
        let word_start = text[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        // The rest of the chord: `Shift+`, `Alt+` … then the key itself.
        let mut rest = &text[i + "Ctrl+".len()..];
        while let Some(r) = ["Shift+", "Alt+", "Win+"].iter().find_map(|m| rest.strip_prefix(m)) {
            rest = r;
        }
        let tab = rest.strip_prefix("Tab").is_some_and(|r| r.chars().next().is_none_or(|c| !c.is_alphanumeric()));
        out.push_str(&text[at..i]);
        out.push_str(if word_start && !tab { "Cmd+" } else { "Ctrl+" });
        at = i + "Ctrl+".len();
    }
    out.push_str(&text[at..]);
    Cow::Owned(out)
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
            // shift is the only thing telling `<C-A>` from `<C-a>` -- and so is
            // Space, which types the same with Shift or without. Digits keep
            // their shift too: egui has no key for `%`, so Alt+Shift+5 is
            // `Num5` + shift and must differ from Alt+5 (`<A-S-5>` / `<A-5>`).
            let shift = mods.shift && (c.is_ascii_alphanumeric() || c == ' ');
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
    from_clipboard_for(events, at, now, false)
}

/// [`from_clipboard`] for an app that keeps Ctrl and Cmd apart (`apart`):
/// Cmd, which is what egui turns into the clipboard's events on a Mac, comes
/// back as `sup` (`<D-v>`), and Ctrl as `ctrl`.
#[cfg(feature = "egui")]
pub fn from_clipboard_for(events: &[egui::Event], at: usize, now: &egui::Modifiers, apart: bool) -> Option<Key> {
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
    let mut key = from_egui(if ctrl { letter } else { bare }, &mods)?;
    if apart && ctrl {
        key.ctrl = held.ctrl;
        key.sup = held.mac_cmd;
    }
    Some(key)
}

/// One thing the person did in a frame, in order: see [`read`].
#[cfg(feature = "egui")]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Input {
    /// A chord or a named key (from [`from_egui`], or [`from_clipboard`] for Cut / Copy / Paste).
    Key(Key),
    /// Typed text the app should insert.
    Text(String),
    /// An input-method event (`Event::Ime`: preedit, commit, enable, disable),
    /// at the place in the frame where it came. The app applies it in order
    /// with the `Key` and `Text` items around it (a commit followed by a
    /// space is the commit, then the space).
    Ime(egui::ImeEvent),
}

/// A frame's events as keys and text, in the order they came, for an app
/// that has a keymap and a text area.
///
/// Windows sends a chord *and* the character it would have typed: Alt+X is
/// `Key { X, alt }` and then `Text("x")` (egui-winit drops the text only for
/// Ctrl / Command chords). The text is dropped here when `took(&key)` says the
/// keymap runs that key, or when the key is an Alt chord without Ctrl. It is
/// kept for AltGr (Ctrl+Alt, which types `@` on a German keyboard) unless the
/// keymap took it, and for Alt+numpad codes (their text arrives after Alt is
/// let go). Typing `a`, `b`, Alt+X, `c` gives `Text("a")`, `Text("b")`,
/// `Key(<A-x>)`, `Text("c")`.
///
/// Ctrl+X / C / V (Cmd on a Mac), Shift held or not, arrive as `Cut` / `Copy`
/// / `Paste` with no key press; they come out as `Key` through
/// [`from_clipboard`]. `now` is the modifiers at the end of the frame.
///
/// Note that egui's default `Options::quit_shortcuts` is Command+Q (which
/// ignores Shift, so Ctrl+Shift+Q on Windows and Linux) and turns it into
/// `ViewportCommand::Close` before the app sees it. An app whose keymap uses
/// that chord sets `quit_shortcuts` to an empty list.
#[cfg(feature = "egui")]
pub fn read(events: &[egui::Event], now: &egui::Modifiers, took: impl FnMut(&Key) -> bool) -> Vec<Input> {
    read_for(events, now, took, cfg!(target_os = "macos"))
}

/// [`read`] with the platform spelled out: on a Mac (`mac`) Option is how
/// people type characters (`å`, `€`, `\` on a JIS keyboard), so an Alt chord's
/// text is kept unless `took` says the keymap runs that key.
#[cfg(feature = "egui")]
pub fn read_for(events: &[egui::Event], now: &egui::Modifiers, mut took: impl FnMut(&Key) -> bool, mac: bool) -> Vec<Input> {
    let mut out = Vec::new();
    let mut skip_text = false;
    for (i, ev) in events.iter().enumerate() {
        match ev {
            egui::Event::Key { pressed: true, .. } if is_numpad_code(&events[i..]) => {
                // Part of an Alt+numpad code: not a chord, and its text is kept.
                skip_text = false;
            }
            egui::Event::Ime(e) => out.push(Input::Ime(e.clone())),
            egui::Event::Key { key, pressed: true, modifiers, .. } => match from_egui(*key, modifiers) {
                Some(k) => {
                    let taken = took(&k);
                    let alt_chord = !mac && modifiers.alt && !modifiers.ctrl && !modifiers.command && !modifiers.mac_cmd;
                    skip_text = taken || alt_chord;
                    out.push(Input::Key(k));
                }
                None => skip_text = false,
            },
            // Letting go of Alt ends an Alt+numpad code: its text follows.
            egui::Event::Key { pressed: false, modifiers, .. } if !modifiers.alt => skip_text = false,
            egui::Event::Cut | egui::Event::Copy | egui::Event::Paste(_) => {
                if let Some(k) = from_clipboard(events, i, now) {
                    took(&k);
                    out.push(Input::Key(k));
                }
            }
            egui::Event::Text(t) => {
                let dropped = std::mem::take(&mut skip_text);
                out.extend((!dropped && !t.is_empty()).then(|| Input::Text(t.clone())));
            }
            _ => {}
        }
    }
    out
}

/// Whether `events[0]`, a key press, is a digit of an Alt+numpad code: Alt
/// held (nothing else), and in this frame the digits are let go with Alt up
/// and then the text arrives (`Alt+0165` types `¥` after Alt is released),
/// with no other key press in between. A code split over two frames is not
/// recognised.
#[cfg(feature = "egui")]
fn is_numpad_code(events: &[egui::Event]) -> bool {
    use egui::Key as K;
    let digit = |k: &K| matches!(k, K::Num0 | K::Num1 | K::Num2 | K::Num3 | K::Num4 | K::Num5 | K::Num6 | K::Num7 | K::Num8 | K::Num9);
    let bare_alt = |m: &egui::Modifiers| m.alt && !m.ctrl && !m.shift && !m.command && !m.mac_cmd;
    match events.first() {
        Some(egui::Event::Key { key, pressed: true, modifiers, .. }) if digit(key) && bare_alt(modifiers) => {}
        _ => return false,
    }
    let mut let_go = false;
    for e in &events[1..] {
        match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                if !digit(key) || !bare_alt(modifiers) {
                    return false;
                }
            }
            egui::Event::Key { pressed: false, modifiers, .. } => let_go |= !modifiers.alt,
            egui::Event::Text(_) => return let_go,
            _ => {}
        }
    }
    false
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

    #[cfg(feature = "egui")]
    fn press(key: egui::Key, mods: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }
    }

    #[cfg(feature = "egui")]
    fn alt() -> egui::Modifiers {
        egui::Modifiers { alt: true, ..Default::default() }
    }

    #[test]
    #[cfg(feature = "egui")]
    fn read_drops_the_text_of_an_alt_chord() {
        let ev = [press(egui::Key::X, alt()), egui::Event::Text("x".into())];
        assert_eq!(read(&ev, &alt(), |_| false), vec![Input::Key(Key::parse("<A-x>").unwrap())]);
        let shifted = egui::Modifiers { alt: true, shift: true, ..Default::default() };
        let ev = [press(egui::Key::Num5, shifted), egui::Event::Text("%".into())];
        assert_eq!(read(&ev, &shifted, |_| false).len(), 1);
    }

    #[test]
    #[cfg(feature = "egui")]
    fn read_keeps_altgr_text_unless_the_keymap_took_it() {
        let altgr = egui::Modifiers { alt: true, ctrl: true, ..Default::default() };
        let ev = [press(egui::Key::Q, altgr), egui::Event::Text("@".into())];
        let got = read(&ev, &altgr, |_| false);
        assert_eq!(got.len(), 2);
        assert_eq!(got[1], Input::Text("@".into()));
        assert_eq!(read(&ev, &altgr, |_| true).len(), 1);
    }

    #[test]
    #[cfg(feature = "egui")]
    fn read_keeps_a_plain_key_and_its_text_and_the_order() {
        let none = egui::Modifiers::default();
        let ev = [
            egui::Event::Text("a".into()),
            egui::Event::Text("b".into()),
            press(egui::Key::X, alt()),
            egui::Event::Text("x".into()),
            egui::Event::Text("c".into()),
        ];
        let got = read(&ev, &none, |_| false);
        assert_eq!(
            got,
            vec![Input::Text("a".into()), Input::Text("b".into()), Input::Key(Key::parse("<A-x>").unwrap()), Input::Text("c".into()),]
        );
        // A key the keymap took drops its text even without Alt.
        let ev = [press(egui::Key::F5, none), egui::Event::Text("x".into())];
        assert_eq!(read(&ev, &none, |_| true).len(), 1);
    }

    #[test]
    #[cfg(feature = "egui")]
    fn read_turns_cut_with_shift_into_its_chord() {
        let cs = egui::Modifiers { ctrl: true, command: true, shift: true, ..Default::default() };
        let got = read(&[egui::Event::Cut], &cs, |_| false);
        assert_eq!(got, vec![Input::Key(Key::parse("<C-S-x>").unwrap())]);
    }

    #[test]
    #[cfg(feature = "egui")]
    fn read_keeps_alt_numpad_text_after_alt_is_let_go() {
        let none = egui::Modifiers::default();
        let ev = [
            press(egui::Key::Num6, alt()),
            egui::Event::Key { key: egui::Key::Num6, physical_key: None, pressed: false, repeat: false, modifiers: none },
            egui::Event::Text("A".into()),
        ];
        assert_eq!(read(&ev, &none, |_| false).last(), Some(&Input::Text("A".into())));
    }

    #[cfg(feature = "egui")]
    fn release(key: egui::Key, mods: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: mods }
    }

    #[test]
    #[cfg(feature = "egui")]
    fn read_puts_ime_commits_in_event_order() {
        use egui::ImeEvent as I;
        let none = egui::Modifiers::default();
        let ev = [
            egui::Event::Ime(I::Preedit { text: String::new(), active_range_chars: None }),
            egui::Event::Ime(I::Commit("한".into())),
            press(egui::Key::Space, none),
            egui::Event::Text(" ".into()),
            release(egui::Key::Space, none),
        ];
        assert_eq!(
            read(&ev, &none, |_| false),
            vec![
                Input::Ime(I::Preedit { text: String::new(), active_range_chars: None }),
                Input::Ime(I::Commit("한".into())),
                Input::Text(" ".into()),
            ]
        );
        let ev = [egui::Event::Text("a".into()), egui::Event::Ime(I::Commit("日本".into()))];
        assert_eq!(read(&ev, &none, |_| false), vec![Input::Text("a".into()), Input::Ime(I::Commit("日本".into()))]);
        let ev = [egui::Event::Ime(I::Commit("日本".into())), press(egui::Key::A, none), egui::Event::Text("a".into())];
        assert_eq!(read(&ev, &none, |_| false), vec![Input::Ime(I::Commit("日本".into())), Input::Text("a".into())]);
    }

    #[test]
    #[cfg(feature = "egui")]
    fn on_a_mac_option_text_is_kept_unless_the_keymap_took_the_key() {
        let ev = [press(egui::Key::A, alt()), egui::Event::Text("å".into())];
        let got = read_for(&ev, &alt(), |_| false, true);
        assert_eq!(got, vec![Input::Key(Key::parse("<A-a>").unwrap()), Input::Text("å".into())]);
        assert_eq!(read_for(&ev, &alt(), |_| true, true), vec![Input::Key(Key::parse("<A-a>").unwrap())]);
        // Elsewhere an Alt chord's text is dropped.
        assert_eq!(read_for(&ev, &alt(), |_| false, false).len(), 1);
    }

    #[test]
    #[cfg(feature = "egui")]
    fn digit_keys_keep_shift() {
        assert_ne!(Key::parse("<A-S-5>"), Key::parse("<A-5>"));
        let shifted = egui::Modifiers { alt: true, shift: true, ..Default::default() };
        assert_eq!(from_egui(egui::Key::Num5, &shifted), Key::parse("<A-S-5>"));
        assert_eq!(from_egui(egui::Key::Num5, &alt()), Key::parse("<A-5>"));
        let k = Key::parse("<A-S-5>").unwrap();
        assert_eq!(Key::parse(&k.to_string()), Some(k));
    }

    #[test]
    #[cfg(feature = "egui")]
    fn an_alt_numpad_code_is_not_a_chord_and_keeps_its_text() {
        use egui::Key as K;
        let none = egui::Modifiers::default();
        let mut ev = Vec::new();
        for k in [K::Num0, K::Num1, K::Num6, K::Num5] {
            ev.push(press(k, alt()));
        }
        ev.push(release(K::Num5, none));
        ev.push(egui::Event::Text("¥".into()));
        let mut asked = Vec::new();
        let got = read(&ev, &none, |k| {
            asked.push(*k);
            true
        });
        assert_eq!(got, vec![Input::Text("¥".into())]);
        assert!(asked.is_empty());
        // A plain Alt+5 chord still runs its binding.
        let ev = [press(K::Num5, alt()), egui::Event::Text("5".into()), release(K::Num5, alt())];
        assert_eq!(read(&ev, &alt(), |_| true), vec![Input::Key(Key::parse("<A-5>").unwrap())]);
    }

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
        // Where Ctrl and Cmd are apart, Cmd is `D`.
        let cmd = egui::Modifiers { mac_cmd: true, command: true, ..Default::default() };
        let apart = |events: &[egui::Event], now| from_clipboard_for(events, 0, &now, true).map(|k| k.to_string());
        assert_eq!(apart(&[paste()], cmd), Some("<D-v>".into()));
        assert_eq!(apart(&[egui::Event::Copy], cmd | shift), Key::parse("<D-S-c>").map(|k| k.to_string()));
        assert_eq!(apart(&[paste()], ctrl), Some("<C-v>".into()));
        assert_eq!(apart(&[paste()], shift), Some("<S-Insert>".into()));
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

    /// Fixed prose says Cmd on a Mac and is left alone elsewhere.
    #[test]
    fn prose_names_the_native_modifier() {
        let mac = |s: &str| human_text_for(s, true).into_owned();
        assert_eq!(mac("Ctrl+Z to undo"), "Cmd+Z to undo");
        assert_eq!(mac("Ctrl+Shift+P  Ctrl+V"), "Cmd+Shift+P  Cmd+V");
        assert_eq!(mac("(Ctrl+Enter)"), "(Cmd+Enter)");
        assert_eq!(mac("Ctrl+Tab"), "Ctrl+Tab");
        assert_eq!(mac("Ctrl+Shift+Tab, Ctrl+K"), "Ctrl+Shift+Tab, Cmd+K");
        assert_eq!(mac("Ctrl+Tabs"), "Cmd+Tabs");
        assert_eq!(mac("XCtrl+K"), "XCtrl+K");
        assert_eq!(mac("j k"), "j k");
        assert!(matches!(human_text_for("Ctrl+Z", false), std::borrow::Cow::Borrowed("Ctrl+Z")));
        assert!(matches!(human_text_for("j k", true), std::borrow::Cow::Borrowed("j k")));
    }

    /// Where Ctrl and Cmd are apart on macOS, the two read apart too.
    #[test]
    fn ctrl_and_cmd_read_apart_where_they_are() {
        let mac = |s: &str, apart| Key::parse(s).unwrap().human_on(true, apart);
        assert_eq!(mac("<C-k>", false), "⌘K");
        assert_eq!(mac("<D-k>", false), "⌘K");
        assert_eq!(mac("<C-k>", true), "⌃K");
        assert_eq!(mac("<D-k>", true), "⌘K");
        assert_eq!(mac("<C-A-S-D-k>", true), "⌃⌥⇧⌘K");
        let pc = |s: &str| Key::parse(s).unwrap().human_on(false, true);
        assert_eq!(pc("<C-k>"), "Ctrl+K");
        assert_eq!(pc("<D-k>"), "Win+K");
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

    #[test]
    #[cfg(feature = "egui")]
    fn shift_space_keeps_its_shift() {
        let held = egui::Modifiers { ctrl: true, shift: true, ..Default::default() };
        assert_eq!(from_egui(egui::Key::Space, &held), Key::parse("<C-S-Space>"));
        assert_ne!(from_egui(egui::Key::Space, &held), Key::parse("<C-Space>"));
    }
}
