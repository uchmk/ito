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
    let _ = writeln!(l.file, "{ms:>8} {dir:<9} {}", escape_bytes(bytes));
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
