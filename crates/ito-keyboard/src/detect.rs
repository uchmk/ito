//! Which key set this machine has, as far as the OS says.
//!
//! This is a guess about the hardware (or, on Linux, about the default
//! layout), not the active key mapping; a person who knows better says so
//! in common.toml's `keyboard`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use crate::layout::Family;

/// The family of the keyboard, asked of the OS. Blocking: it may read files
/// or call the OS, so [`detected`] is the one to call from a UI.
///
/// Windows asks `GetKeyboardType` (7 is the Japanese keyboard), macOS the
/// keyboard type of the Carbon layout (`JIS `), Linux the default layout in
/// the environment and the usual files. Anything unknown is the US set.
pub fn detect() -> Family {
    imp::detect()
}

#[cfg(windows)]
mod imp {
    use crate::layout::Family;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetKeyboardType(n: i32) -> i32;
    }

    pub fn detect() -> Family {
        // SAFETY: a plain query with no pointers; 0 asks for the keyboard type.
        if unsafe { GetKeyboardType(0) } == 7 {
            Family::Jis
        } else {
            Family::Us
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use crate::layout::Family;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn LMGetKbdType() -> u8;
        fn KBGetLayoutType(i: i16) -> u32;
    }

    /// The four characters `JIS ` Carbon answers for a Japanese keyboard.
    const JIS: u32 = 0x4A49_5320;

    pub fn detect() -> Family {
        // SAFETY: both are plain queries with no pointers (Apple kept
        // `KBGetLayoutType` in 64-bit Carbon).
        let layout = unsafe { KBGetLayoutType(LMGetKbdType() as i16) };
        if layout == JIS {
            Family::Jis
        } else {
            Family::Us
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use crate::layout::Family;

    pub fn detect() -> Family {
        let read = |p: &str| std::fs::read_to_string(p).ok();
        if let Some(f) = std::env::var("XKB_DEFAULT_LAYOUT").ok().as_deref().and_then(super::parse_xkb_layout) {
            return f;
        }
        let from_files = read("/etc/default/keyboard")
            .and_then(|t| super::parse_default_keyboard(&t))
            .or_else(|| read("/etc/vconsole.conf").and_then(|t| super::parse_vconsole(&t)))
            .or_else(|| read("/etc/X11/xorg.conf.d/00-keyboard.conf").and_then(|t| super::parse_xorg(&t)));
        from_files.unwrap_or(Family::Us)
    }
}

/// The family of an XKB layout list (`jp`, `us,jp`): the first item decides.
/// `None` for an empty one.
pub fn parse_xkb_layout(s: &str) -> Option<Family> {
    let first = s.split(',').next()?.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if first.is_empty() {
        return None;
    }
    // `jp` and its variants (`jp(kana)`); anything else is the US set.
    let name = first.split('(').next().unwrap_or(first).trim();
    Some(if name.eq_ignore_ascii_case("jp") { Family::Jis } else { Family::Us })
}

/// The `KEY=value` lines of a shell-style file: comments and blanks gone,
/// quotes taken off, the last of a key winning.
fn assignments(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            // A comment after the value, outside the quotes.
            let v = v.trim();
            let v = match v.chars().next() {
                Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or("").to_owned(),
                _ => v.split('#').next().unwrap_or("").trim().to_owned(),
            };
            Some((k.trim().to_owned(), v))
        })
        .collect()
}

fn last<'a>(a: &'a [(String, String)], key: &str) -> Option<&'a str> {
    a.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// `/etc/default/keyboard`: `XKBLAYOUT`'s first item, else `XKBMODEL` (`jp106`).
pub fn parse_default_keyboard(text: &str) -> Option<Family> {
    let a = assignments(text);
    if let Some(f) = last(&a, "XKBLAYOUT").and_then(parse_xkb_layout) {
        return Some(f);
    }
    let model = last(&a, "XKBMODEL")?;
    Some(if model.to_ascii_lowercase().starts_with("jp") { Family::Jis } else { Family::Us })
}

/// `/etc/vconsole.conf`: `KEYMAP=jp106` is a Japanese keyboard.
pub fn parse_vconsole(text: &str) -> Option<Family> {
    let a = assignments(text);
    let map = last(&a, "KEYMAP")?.trim().to_ascii_lowercase();
    if map.is_empty() {
        return None;
    }
    Some(if map.starts_with("jp") { Family::Jis } else { Family::Us })
}

/// `/etc/X11/xorg.conf.d/00-keyboard.conf`: `Option "XkbLayout" "jp"`.
pub fn parse_xorg(text: &str) -> Option<Family> {
    text.lines().find_map(|l| {
        let l = l.split('#').next().unwrap_or("");
        let mut words = l.split('"').map(str::trim).filter(|w| !w.is_empty());
        if words.next()? != "Option" || !words.next()?.eq_ignore_ascii_case("XkbLayout") {
            return None;
        }
        parse_xkb_layout(words.next()?)
    })
}

static STARTED: AtomicBool = AtomicBool::new(false);
static FOUND: OnceLock<Family> = OnceLock::new();

/// The family [`detect`] found, or `None` until it has: this never blocks and
/// never touches the OS or the disk on the caller's thread. The first call
/// starts one thread that asks, and calls `wake` (a repaint request) when it
/// has the answer. The answer is kept for the process, so a keyboard swapped
/// while the app runs is not seen.
pub fn detected(wake: impl FnOnce() + Send + 'static) -> Option<Family> {
    if let Some(f) = FOUND.get() {
        return Some(*f);
    }
    if STARTED.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        let spawned = std::thread::Builder::new().name("ito-keyboard-detect".into()).spawn(move || {
            let _ = FOUND.set(detect());
            wake();
        });
        if spawned.is_err() {
            // No thread to ask on: the US set, and nothing more to wait for.
            let _ = FOUND.set(Family::Us);
        }
    }
    FOUND.get().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_xkb_layout_list_is_read_by_its_first_item() {
        assert_eq!(parse_xkb_layout("jp"), Some(Family::Jis));
        assert_eq!(parse_xkb_layout("us,jp"), Some(Family::Us));
        assert_eq!(parse_xkb_layout("jp,us"), Some(Family::Jis));
        assert_eq!(parse_xkb_layout(" \"jp\" "), Some(Family::Jis));
        assert_eq!(parse_xkb_layout("de"), Some(Family::Us));
        assert_eq!(parse_xkb_layout(""), None);
    }

    #[test]
    fn etc_default_keyboard_gives_the_layout_or_else_the_model() {
        let jp = "# KEYBOARD CONFIGURATION FILE\nXKBMODEL=\"pc105\"\nXKBLAYOUT=\"jp\"\nXKBVARIANT=\"\"\nXKBOPTIONS=\"\"\n";
        assert_eq!(parse_default_keyboard(jp), Some(Family::Jis));
        let both = "XKBLAYOUT=\"us,jp\"\nXKBMODEL=\"jp106\"\n";
        assert_eq!(parse_default_keyboard(both), Some(Family::Us));
        assert_eq!(parse_default_keyboard("XKBMODEL=\"jp106\"\n"), Some(Family::Jis));
        assert_eq!(parse_default_keyboard("XKBLAYOUT=\"\"\nXKBMODEL=jp106 # Japan\n"), Some(Family::Jis));
        assert_eq!(parse_default_keyboard("# XKBLAYOUT=\"jp\"\nXKBMODEL=\"pc105\"\n"), Some(Family::Us));
        assert_eq!(parse_default_keyboard("# nothing\n"), None);
    }

    #[test]
    fn etc_vconsole_conf_names_the_keymap() {
        assert_eq!(parse_vconsole("KEYMAP=jp106\nFONT=latarcyrheb-sun16\n"), Some(Family::Jis));
        assert_eq!(parse_vconsole("KEYMAP=\"us\"\n"), Some(Family::Us));
        assert_eq!(parse_vconsole("# KEYMAP=jp106\n"), None);
        assert_eq!(parse_vconsole("KEYMAP=\n"), None);
    }

    #[test]
    fn an_xorg_snippet_names_the_layout() {
        let jp = "Section \"InputClass\"\n        Identifier \"system-keyboard\"\n        MatchIsKeyboard \"on\"\n        Option \"XkbLayout\" \"jp\"\n        Option \"XkbModel\" \"jp106\"\nEndSection\n";
        assert_eq!(parse_xorg(jp), Some(Family::Jis));
        assert_eq!(parse_xorg(&jp.replace("\"jp\"\n        Option", "\"us,jp\"\n        Option")), Some(Family::Us));
        assert_eq!(parse_xorg("    # Option \"XkbLayout\" \"jp\"\n"), None);
        assert_eq!(parse_xorg("Section \"ServerLayout\"\nEndSection\n"), None);
    }
}
