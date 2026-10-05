//! egui's input as the bytes a shell expects (the `egui` feature): the key
//! handling of filer's `on_key_event` for a focused pane, without filer's
//! keymap. An app takes the chords that are its own first (`claim`), and the
//! rest goes to the shell.

use egui::{Event, Key, Modifiers};

use crate::{Mods, Special, Terminal};

/// Hand this frame's `events` to `term`. `claim` sees every key press first
/// and returns true for one the app keeps (a new tab, a split): that one goes
/// no further, and neither does the text it would have typed.
pub fn feed(term: &Terminal, events: &[Event], mut claim: impl FnMut(Key, Modifiers) -> bool) {
    // Windows sends a chord *and* the character it would have typed: `<A-b>`
    // arrives as a key with alt set and then as `Text("b")`. The chord has been
    // sent by the time the text turns up, so the text is dropped. Ctrl and Alt
    // together is AltGr (`@` on a German keyboard), where the text is the point.
    let mut swallow_text = false;
    for ev in events {
        match ev {
            Event::Key { key, pressed: true, modifiers, .. } => {
                if claim(*key, *modifiers) {
                    swallow_text = true;
                    continue;
                }
                swallow_text = modifiers.alt && !modifiers.ctrl;
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

fn send_chord(term: &Terminal, key: Key, claim: &mut impl FnMut(Key, Modifiers) -> bool) {
    let ctrl = Modifiers { ctrl: true, command: true, ..Default::default() };
    if !claim(key, ctrl) {
        if let Some(bytes) = key_bytes(term, key, ctrl) {
            term.send(bytes);
        }
    }
}

/// The bytes for one key press, or `None` for a key that is text (it arrives
/// as its own event) or nothing a shell wants.
///
/// Once the other end has asked for win32-input-mode (ConPTY does, as it
/// starts), every key whose VT form begins with ESC goes as a key record, as
/// Windows Terminal sends them: a tcell program then cannot mistake `<Esc>`
/// and the key after it for one sequence (filer Q27, #99).
pub fn key_bytes(term: &Terminal, key: Key, modifiers: Modifiers) -> Option<Vec<u8>> {
    let mods = Mods { ctrl: modifiers.command || modifiers.ctrl, alt: modifiers.alt, shift: modifiers.shift };
    let win32 = term.win32_input();
    let vt = match special(key, mods.shift) {
        // On Windows `Esc` goes as one win32-input-mode key press, not as a
        // plain ESC: ConPTY makes a press *and a release* out of a plain ESC,
        // and the release turns the key into an Alt prefix in a tcell program,
        // so `Esc` did nothing in lazygit. See `win32_key`.
        Some(Special::Escape) if cfg!(windows) => Some(crate::win32_key(0x1b, 1, 0x1b, mods)),
        Some(s) => Some(crate::encode(s, mods, term.with_grid(crate::app_cursor))),
        // A letter with Ctrl held is a control code; egui sends no text for it.
        None if mods.ctrl => key.name().chars().next().and_then(|c| crate::control_code(c.to_ascii_lowercase(), mods.alt)),
        // Alt without Ctrl sends no text either: readline's `Alt-b` and
        // `Alt-f` are made here.
        None if mods.alt => printable(key).map(|c| crate::meta_char(if mods.shift { c.to_ascii_uppercase() } else { c })),
        None => None,
    };
    // A chord with no record form (`Ctrl+[`, `Ctrl+Space`) keeps its VT bytes.
    match (win32, special(key, mods.shift)) {
        (true, Some(s)) => Some(crate::special_record(s, mods)),
        (true, None) if mods.ctrl || mods.alt => {
            printable(key).or_else(|| key.name().chars().next()).and_then(|c| crate::char_record(c, mods)).or(vt)
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

    #[test]
    fn shift_tab_is_its_own_key() {
        assert_eq!(special(Key::Tab, true), Some(Special::BackTab));
        assert_eq!(special(Key::Tab, false), Some(Special::Tab));
        assert_eq!(special(Key::A, false), None);
    }
}
