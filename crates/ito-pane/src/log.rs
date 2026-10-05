//! `Terminal::spawn`'s PTY log.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;



#[allow(unused_imports)]
use crate::{grid::*, keys::*, osc::*, shell::*, terminal::*};

/// A record of every byte that crosses the PTY, for when the pane and a
/// program in it disagree about what was said.
///
/// Pass a file path to `Terminal::spawn` (filer reads it from `FILER_PTY_LOG`) and each chunk is
/// appended as one line: milliseconds since the pane opened, the direction,
/// and the bytes with control characters spelled out. `out` is what the shell
/// side (on Windows, ConPTY) wrote to filer; `in` is what filer wrote to it,
/// split by origin into `in key`, `in paste` and `in reply` -- the last being
/// the terminal's own answers to a program's queries, which are the bytes
/// under suspicion when lazygit opens a menu at startup that nobody asked for.
///
/// The first line gives the moment the pane opened in Unix milliseconds, so a
/// line's time plus that is the wall clock `scripts/keyprobe.ps1` prints.
///
/// Off unless the variable is set, and nothing is read or written for it then.
/// A chunk is whatever one read or write happened to carry, so a sequence can
/// be split across two lines, and a multi-byte character cut at a chunk's edge
/// shows as U+FFFD.
pub(crate) type PtyLog = Option<Arc<Mutex<PtyLogFile>>>;

pub(crate) struct PtyLogFile {
    pub(crate) file: std::fs::File,
    pub(crate) start: Instant,
}

pub(crate) fn open_pty_log(path: Option<&Path>) -> PtyLog {
    let path = path?;
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
    let log = PtyLogFile { file, start: Instant::now() };
    let log = Arc::new(Mutex::new(log));
    if let Ok(mut l) = log.lock() {
        // The wall clock once, so the lines' milliseconds can be matched
        // against another program's (`scripts/keyprobe.ps1` prints this clock).
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let _ = writeln!(l.file, "== pane opened at {unix} (Unix ms; the times below count from here)");
    }
    Some(log)
}

pub(crate) fn log_pty(log: &PtyLog, dir: &str, bytes: &[u8]) {
    let Some(log) = log else { return };
    let Ok(mut l) = log.lock() else { return };
    let ms = l.start.elapsed().as_millis();
    let names = name_records(bytes);
    let names = if names.is_empty() { String::new() } else { format!("  ({})", names.join(" ")) };
    let _ = writeln!(l.file, "{ms:>8} {dir:<9} {}{names}", escape_bytes(bytes));
}

/// The keys in a chunk of win32-input-mode records, in words: `\e[66;48;98;1;2;1_`
/// is `Alt+b` (filer #243). Reading the six numbers by hand was the slow part
/// of every look at a key that went wrong. Empty unless the whole chunk is
/// records, which is how keys are sent; a release adds `up`.
///
/// The record is `CSI Vk ; Sc ; Uc ; Kd ; Cs ; Rc _`: the virtual key, the
/// scan code, the character, pressed (1) or released (0), the modifier state
/// (`RIGHT_ALT` 0x1, `LEFT_ALT` 0x2, `RIGHT_CTRL` 0x4, `LEFT_CTRL` 0x8,
/// `SHIFT` 0x10) and the repeat count.
pub(crate) fn name_records(bytes: &[u8]) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(bytes) else { return Vec::new() };
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some(body) = rest.strip_prefix("\x1b[") else { return Vec::new() };
        let Some(end) = body.find('_') else { return Vec::new() };
        let nums: Vec<u32> = match body[..end].split(';').map(str::parse).collect::<Result<_, _>>() {
            Ok(n) => n,
            Err(_) => return Vec::new(),
        };
        let [vk, _, uc, kd, cs, _] = nums[..] else { return Vec::new() };
        out.push(name_key(vk, uc, kd, cs));
        rest = &body[end + 1..];
    }
    out
}

fn name_key(vk: u32, uc: u32, kd: u32, cs: u32) -> String {
    let mut name = String::new();
    if cs & 0x0c != 0 {
        name.push_str("Ctrl+");
    }
    if cs & 0x03 != 0 {
        name.push_str("Alt+");
    }
    let key = match vk {
        0x08 => "Backspace".to_owned(),
        0x09 => "Tab".to_owned(),
        0x0d => "Enter".to_owned(),
        0x1b => "Esc".to_owned(),
        0x20 => "Space".to_owned(),
        0x21 => "PageUp".to_owned(),
        0x22 => "PageDown".to_owned(),
        0x23 => "End".to_owned(),
        0x24 => "Home".to_owned(),
        0x25 => "Left".to_owned(),
        0x26 => "Up".to_owned(),
        0x27 => "Right".to_owned(),
        0x28 => "Down".to_owned(),
        0x2d => "Insert".to_owned(),
        0x2e => "Delete".to_owned(),
        0x70..=0x87 => format!("F{}", vk - 0x6f),
        // A character says itself, already shifted (`B`, `!`), so Shift is
        // not named again for it.
        _ => match char::from_u32(uc).filter(|c| !c.is_control()) {
            Some(c) => return format!("{name}{c}{}", if kd == 0 { " up" } else { "" }),
            // Ctrl turns a letter into a control character; the key is the letter.
            None => char::from_u32(vk).filter(char::is_ascii_alphanumeric).map_or(format!("vk{vk}"), |c| c.to_ascii_lowercase().to_string()),
        },
    };
    if cs & 0x10 != 0 {
        name.push_str("Shift+");
    }
    format!("{name}{key}{}", if kd == 0 { " up" } else { "" })
}

/// Bytes as one readable line: `\e` for ESC, `\r` `\n` `\t`, `\xNN` for any
/// other control character, and `\\` for a backslash so that none of those
/// spellings can be mistaken for the text they stand in for.
pub fn escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for c in String::from_utf8_lossy(bytes).chars() {
        match c {
            '\x1b' => out.push_str("\\e"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c == '\x7f' => out.push_str(&format!("\\x{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
