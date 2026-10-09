//! egui's input as the bytes a shell expects (the `egui` feature): the key
//! handling of filer's `on_key_event` for a focused pane, without filer's
//! keymap. An app takes the chords that are its own first (`claim`), and the
//! rest goes to the shell.

use std::cell::RefCell;
use std::collections::HashMap;

use egui::{Event, Key, Modifiers};

use crate::kitty::{self, KeyEvent, KittyKey};
use crate::{Mods, Pane, Special};

thread_local! {
    /// The keys whose press went to a pane as a kitty escape code, by pane,
    /// with the modifiers they were pressed with: only those have their
    /// release told, so a chord the app kept (a new tab) does not reach the
    /// program as a release with no press.
    static HELD: RefCell<HashMap<(u64, Key), Modifiers>> = RefCell::new(HashMap::new());
}

/// Hand this frame's `events` to `term`. `claim` sees every key press first
/// and returns true for one the app keeps (a new tab, a split): that one goes
/// no further, and neither does the text it would have typed. The pane is
/// told apart by where it is in memory; an app whose panes move (in a map
/// that grows) names them with [`feed_as`].
pub fn feed<P: Pane + ?Sized>(term: &P, events: &[Event], claim: impl FnMut(Key, Modifiers) -> bool) {
    feed_as(term, term as *const P as *const () as usize as u64, events, claim)
}

/// The keys `id` holds down as a program asking for their releases saw
/// them pressed, let go: told to `term` as released, and forgotten. For
/// when the keys go elsewhere (another pane, another window) before they are
/// let go, which this pane would otherwise never hear of.
pub fn let_go<P: Pane + ?Sized>(term: &P, id: u64) {
    let flags = if term.win32_input() { 0 } else { term.kitty_flags() };
    for (key, mods) in forget(id) {
        if flags != 0 {
            if let Some(bytes) = kitty_bytes(key, mods, KeyEvent::Release, None, flags).filter(|b| !b.is_empty()) {
                term.send(bytes);
            }
        }
    }
}

/// Forget the keys `id` holds (a pane that has gone), returning them.
pub fn forget(id: u64) -> Vec<(Key, Modifiers)> {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let keys: Vec<(Key, Modifiers)> = h.iter().filter(|((of, _), _)| *of == id).map(|((_, k), m)| (*k, *m)).collect();
        h.retain(|(of, _), _| *of != id);
        keys
    })
}

/// [`feed`], the pane named `id` (a session's id) for the keys it holds.
pub fn feed_as<P: Pane + ?Sized>(term: &P, id: u64, events: &[Event], mut claim: impl FnMut(Key, Modifiers) -> bool) {
    // Windows sends a chord *and* the character it would have typed: `<A-b>`
    // arrives as a key with alt set and then as `Text("b")`. The chord has been
    // sent by the time the text turns up, so the text is dropped. Ctrl and Alt
    // together is AltGr (`@` on a German keyboard), where the text is the point.
    let mut swallow_text = false;
    let flags = if term.win32_input() { 0 } else { term.kitty_flags() };
    for (k, ev) in events.iter().enumerate() {
        match ev {
            Event::Key { key, pressed: false, modifiers, .. } if flags != 0 => {
                if let Some(pressed) = HELD.with(|h| h.borrow_mut().remove(&(id, *key))) {
                    // A modifier let go first (Ctrl before `c`) can leave the
                    // key one that tells no release alone: then as pressed.
                    let bytes = kitty_bytes(*key, *modifiers, KeyEvent::Release, None, flags).filter(|b| !b.is_empty());
                    if let Some(bytes) = bytes.or_else(|| kitty_bytes(*key, pressed, KeyEvent::Release, None, flags).filter(|b| !b.is_empty())) {
                        term.send(bytes);
                    }
                }
            }
            Event::Key { key, pressed: true, repeat, modifiers, .. } => {
                if claim(*key, *modifiers) {
                    swallow_text = true;
                    continue;
                }
                swallow_text = modifiers.alt && !modifiers.ctrl;
                // AltGr: Ctrl and Alt on a printable key that types text
                // (the next event). The text is the key; its Ctrl+Alt chord
                // as well sent `\x1b\x11` before an `@` (the source review,
                // 2026-10-07). A Ctrl+Alt chord that types nothing still goes.
                let altgr = modifiers.ctrl && modifiers.alt && special(*key, modifiers.shift).is_none() && matches!(events.get(k + 1), Some(Event::Text(t)) if !t.is_empty());
                if altgr {
                    continue;
                }
                // A program that asked for kitty's keys gets them so, the
                // text the key typed going with it rather than after it.
                if flags != 0 {
                    let text = match events.get(k + 1) {
                        Some(Event::Text(t)) => Some(t.as_str()),
                        _ => None,
                    };
                    let event = if *repeat { KeyEvent::Repeat } else { KeyEvent::Press };
                    if let Some(bytes) = kitty_bytes(*key, *modifiers, event, text, flags) {
                        swallow_text = text.is_some();
                        if !bytes.is_empty() {
                            HELD.with(|h| h.borrow_mut().insert((id, *key), *modifiers));
                            term.send(bytes);
                        }
                        continue;
                    }
                }
                if let Some(bytes) = key_bytes(term, *key, *modifiers) {
                    term.send(bytes);
                }
            }
            Event::Text(_) if swallow_text => swallow_text = false,
            Event::Text(text) => term.send(text.clone().into_bytes()),
            // egui-winit turns the clipboard chords into these and never sends
            // the key press, so `<C-c>` -- the key that stops a command -- would
            // never reach the shell. Put the chord back.
            Event::Copy => send_chord(term, Key::C, &mut claim),
            Event::Cut => send_chord(term, Key::X, &mut claim),
            Event::Paste(text) => term.paste(text),
            _ => {}
        }
    }
}

fn send_chord<P: Pane + ?Sized>(term: &P, key: Key, claim: &mut impl FnMut(Key, Modifiers) -> bool) {
    let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
    if !claim(key, ctrl) {
        if let Some(bytes) = key_bytes(term, key, ctrl) {
            term.send(bytes);
        }
    }
}

/// A key under kitty's keyboard protocol (`kitty::encode`): `None` when it
/// goes the legacy way.
fn kitty_bytes(key: Key, modifiers: Modifiers, event: KeyEvent, text: Option<&str>, flags: u8) -> Option<Vec<u8>> {
    let mods = Mods { ctrl: modifiers.command || modifiers.ctrl, alt: modifiers.alt, shift: modifiers.shift };
    let modifier = [Key::ShiftLeft, Key::ControlLeft, Key::AltLeft, Key::SuperLeft, Key::ShiftRight, Key::ControlRight, Key::AltRight, Key::SuperRight]
        .iter()
        .position(|m| *m == key)
        .map(|i| KittyKey::Modifier(kitty::MODIFIER_KEYS[i]));
    let kk = modifier.or_else(|| special(key, mods.shift).map(KittyKey::Special)).or_else(|| printable(key).map(KittyKey::Char))?;
    kitty::encode(kk, mods, event, text, flags)
}

/// The bytes for one key press, or `None` for a key that is text (it arrives
/// as its own event) or nothing a shell wants.
///
/// Once the other end has asked for win32-input-mode (ConPTY does, as it
/// starts), every key whose VT form begins with ESC goes as a key record, as
/// Windows Terminal sends them: a tcell program then cannot mistake `<Esc>`
/// and the key after it for one sequence (filer Q27, #99).
pub fn key_bytes<P: Pane + ?Sized>(term: &P, key: Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    let mods = Mods { ctrl: modifiers.command || modifiers.ctrl, alt: modifiers.alt, shift: modifiers.shift };
    let win32 = term.win32_input();
    let flags = if win32 { 0 } else { term.kitty_flags() };
    if flags != 0 {
        if let Some(bytes) = kitty_bytes(key, modifiers, KeyEvent::Press, None, flags) {
            return (!bytes.is_empty()).then_some(bytes);
        }
    }
    let vt = match special(key, mods.shift) {
        // On Windows `Esc` goes as one win32-input-mode key press, not as a
        // plain ESC: ConPTY makes a press *and a release* out of a plain ESC,
        // and the release turns the key into an Alt prefix in a tcell program,
        // so `Esc` did nothing in lazygit. See `win32_key`.
        Some(Special::Escape) if cfg!(windows) => Some(crate::win32_key(0x1b, 1, 0x1b, mods)),
        Some(s) => Some(crate::encode(s, mods, term.screen().app_cursor)),
        // A letter with Ctrl held is a control code; egui sends no text for it.
        // By the key's character, never its name: egui sends Shift and Ctrl
        // themselves as keys too, and `ShiftLeft` with Ctrl held went to the
        // shell as Ctrl+S, which stops a terminal's output.
        None if mods.ctrl => printable(key).and_then(|c| crate::control_code(c, mods.alt)),
        // Alt without Ctrl sends no text either: readline's `Alt-b` and
        // `Alt-f` are made here.
        None if mods.alt => printable(key).map(|c| crate::meta_char(if mods.shift { c.to_ascii_uppercase() } else { c })),
        None => None,
    };
    // A chord with no record form (`Ctrl+[`, `Ctrl+Space`) keeps its VT bytes.
    match (win32, special(key, mods.shift)) {
        (true, Some(s)) => Some(crate::special_record(s, mods)),
        (true, None) if mods.ctrl || mods.alt => {
            printable(key).and_then(|c| crate::char_record(c, mods)).or(vt)
        }
        _ => vt,
    }
}

/// The egui keys a terminal has an escape sequence for.
pub fn special(key: Key, shift: bool) -> Option<Special> {
    use Key as K;
    use Special as S;
    Some(match key {
        K::Enter => S::Enter,
        K::Backspace => S::Backspace,
        // Shift+Tab is its own sequence, not Tab with a modifier on it.
        K::Tab if shift => S::BackTab,
        K::Tab => S::Tab,
        K::Escape => S::Escape,
        K::ArrowUp => S::Up,
        K::ArrowDown => S::Down,
        K::ArrowRight => S::Right,
        K::ArrowLeft => S::Left,
        K::Home => S::Home,
        K::End => S::End,
        K::PageUp => S::PageUp,
        K::PageDown => S::PageDown,
        K::Insert => S::Insert,
        K::Delete => S::Delete,
        K::F1 => S::F(1),
        K::F2 => S::F(2),
        K::F3 => S::F(3),
        K::F4 => S::F(4),
        K::F5 => S::F(5),
        K::F6 => S::F(6),
        K::F7 => S::F(7),
        K::F8 => S::F(8),
        K::F9 => S::F(9),
        K::F10 => S::F(10),
        K::F11 => S::F(11),
        K::F12 => S::F(12),
        _ => return None,
    })
}

/// The character an unshifted key types on a US layout, for chords that send
/// no text of their own (filer's `keys::printable`).
pub fn printable(key: Key) -> Option<char> {
    use Key as K;
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
        _ => {
            let name = key.name();
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => c.to_ascii_lowercase(),
                _ => return None,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    /// A pane that only records what is sent to it.
    #[derive(Default)]
    struct Sent(std::cell::RefCell<Vec<u8>>, u8);

    impl crate::Pane for Sent {
        fn resize(&mut self, _: crate::Size, _: (u16, u16)) {}
        fn screen(&self) -> crate::Screen {
            crate::Screen::default()
        }
        fn scrolled_back(&self) -> usize {
            0
        }
        fn scroll(&self, _: alacritty_terminal::grid::Scroll) {}
        fn select(&self, _: (usize, usize), _: bool, _: bool) {}
        fn select_word(&self, _: (usize, usize)) {}
        fn clear_selection(&self) {}
        fn selection(&self) -> Option<String> {
            None
        }
        fn send(&self, bytes: Vec<u8>) {
            self.0.borrow_mut().extend(bytes);
        }
        fn paste(&self, _: &str) {}
        fn win32_input(&self) -> bool {
            false
        }
        fn kitty_flags(&self) -> u8 {
            self.1
        }
    }

    fn key(k: egui::Key, modifiers: egui::Modifiers, pressed: bool) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers }
    }

    fn sent(p: &Sent) -> String {
        String::from_utf8(p.0.borrow().clone()).unwrap()
    }

    /// Once a program asked to have keys told apart, Shift+Enter is its own
    /// key; text and AltGr still type as text, and Ctrl+C (which egui turns
    /// into a copy) goes as a code too.
    #[test]
    fn kitty_keys_when_a_program_asked() {
        let shift = egui::Modifiers { shift: true, ..Default::default() };
        let p = Sent(Default::default(), crate::kitty::DISAMBIGUATE);
        super::feed(&p, &[key(egui::Key::Enter, shift, true), key(egui::Key::Enter, shift, false)], |_, _| false);
        super::feed(&p, &[key(egui::Key::A, shift, true), egui::Event::Text("A".into())], |_, _| false);
        let altgr = egui::Modifiers { ctrl: true, alt: true, ..Default::default() };
        super::feed(&p, &[key(egui::Key::Q, altgr, true), egui::Event::Text("@".into())], |_, _| false);
        super::feed(&p, &[egui::Event::Copy], |_, _| false);
        assert_eq!(sent(&p), "\x1b[13;2uA@\x1b[99;5u");
        let p = Sent::default();
        super::feed(&p, &[key(egui::Key::Enter, shift, true)], |_, _| false);
        assert_eq!(sent(&p), "\r", "nobody asked: as before");
    }

    /// Releases go for the keys whose press went, and not for a chord the
    /// app kept; with every key asked for, the text rides in the code.
    #[test]
    fn kitty_releases_and_text() {
        let none = egui::Modifiers::default();
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        let p = Sent(Default::default(), crate::kitty::DISAMBIGUATE | crate::kitty::EVENT_TYPES);
        super::feed(&p, &[key(egui::Key::Escape, none, true), key(egui::Key::Escape, none, false)], |_, _| false);
        super::feed(&p, &[key(egui::Key::T, ctrl, true), key(egui::Key::T, ctrl, false)], |k, _| k == egui::Key::T);
        assert_eq!(sent(&p), "\x1b[27u\x1b[27;1:3u");
        let p = Sent(Default::default(), 31);
        super::feed(&p, &[key(egui::Key::A, none, true), egui::Event::Text("a".into())], |_, _| false);
        assert_eq!(sent(&p), "\x1b[97;1;97u");
    }

    /// Ctrl let go before `c` still tells `c`'s release (as it was pressed),
    /// and a key held while the keys went elsewhere is let go by `let_go`,
    /// once; another pane's held keys are its own.
    #[test]
    fn a_held_key_is_always_let_go() {
        let none = egui::Modifiers::default();
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
        let p = Sent(Default::default(), crate::kitty::DISAMBIGUATE | crate::kitty::EVENT_TYPES);
        super::feed_as(&p, 901, &[key(egui::Key::C, ctrl, true), key(egui::Key::C, none, false)], |_, _| false);
        assert_eq!(sent(&p), "\x1b[99;5u\x1b[99;5:3u");
        p.0.borrow_mut().clear();
        super::feed_as(&p, 901, &[key(egui::Key::Escape, none, true)], |_, _| false);
        super::feed_as(&p, 902, &[key(egui::Key::Escape, none, true)], |_, _| false);
        super::let_go(&p, 901);
        super::let_go(&p, 901);
        assert_eq!(sent(&p), "\x1b[27u\x1b[27u\x1b[27;1:3u");
        assert_eq!(super::forget(902), vec![(egui::Key::Escape, none)]);
    }

    /// A pane whose program did not ask hears nothing of the keys coming
    /// and going (the fake answers no, as every pane does by default).
    #[test]
    fn focus_is_told_only_when_asked() {
        let p = Sent::default();
        crate::Pane::focus(&p, true);
        assert!(p.0.borrow().is_empty());
    }

    /// AltGr+Q on a German keyboard (Ctrl+Alt with `@` as its text) types
    /// `@` alone; a Ctrl+Alt chord with no text still goes as the chord.
    #[test]
    fn altgr_types_its_character_only() {
        let both = egui::Modifiers { ctrl: true, alt: true, ..Default::default() };
        let key = |k| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: both };
        let p = Sent::default();
        super::feed(&p, &[key(egui::Key::Q), egui::Event::Text("@".into())], |_, _| false);
        assert_eq!(p.0.borrow().as_slice(), b"@");
        let p = Sent::default();
        super::feed(&p, &[key(egui::Key::X)], |_, _| false);
        assert_eq!(p.0.borrow().as_slice(), b"\x1b\x18", "Ctrl+Alt+X as ESC Ctrl+X");
    }
    use super::*;

    /// Letters and digits by name, punctuation by the table.
    #[test]
    fn keys_type_their_characters() {
        assert_eq!(printable(Key::B), Some('b'));
        assert_eq!(printable(Key::Num7), Some('7'));
        assert_eq!(printable(Key::Slash), Some('/'));
        assert_eq!(printable(Key::F1), None, "F1 is no character");
        assert_eq!(printable(Key::Enter), None);
    }

    /// egui 0.36 sends the modifier keys themselves; they type nothing.
    #[test]
    fn a_modifier_key_is_no_character() {
        assert_eq!(printable(Key::ShiftLeft), None);
        assert_eq!(printable(Key::ControlLeft), None);
    }

    #[test]
    fn shift_tab_is_its_own_key() {
        assert_eq!(special(Key::Tab, true), Some(Special::BackTab));
        assert_eq!(special(Key::Tab, false), Some(Special::Tab));
        assert_eq!(special(Key::A, false), None);
    }
}
