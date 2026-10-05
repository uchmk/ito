//! What the shell says outside the grid: OSC 7 (where it is), OSC 133 (its
//! prompt is up), and the win32-input-mode request.

use std::path::{Path, PathBuf};



#[allow(unused_imports)]
use crate::{grid::*, keys::*, log::*, shell::*, terminal::*};

/// Longest OSC 7 worth waiting for. A path cannot sensibly be longer, and a
/// stream that never terminates one must not grow this for ever.
pub(crate) const MAX_OSC: usize = 4096;

/// Whether this read turned win32-input-mode on or off, going by the last
/// `\e[?9001h` or `\e[?9001l` in it. ConPTY asks for it as it starts, when
/// the terminal on the other side can send key records rather than VT.
///
/// `tail` keeps the end of the previous read, since a request can be cut in
/// two by where one read stopped.
/// Whether `chunk`, carried on from `tail`, holds a shell's own word that its
/// prompt is up: OSC 133's `A` (prompt starts) or `B` (input starts), which
/// shells set up for terminal integration send -- Windows Terminal's
/// guidance for pwsh, starship, and the like. A read can stop inside one.
pub(crate) fn scan_prompt_mark(tail: &mut Vec<u8>, chunk: &[u8]) -> bool {
    const MARKS: [&[u8]; 2] = [b"\x1b]133;A", b"\x1b]133;B"];
    tail.extend_from_slice(chunk);
    let found = MARKS.iter().any(|m| tail.windows(m.len()).any(|w| w == *m));
    let keep = tail.len().saturating_sub(MARKS[0].len() - 1);
    tail.drain(..keep);
    found
}

pub(crate) fn scan_win32_mode(tail: &mut Vec<u8>, chunk: &[u8]) -> Option<bool> {
    const SET: &[u8] = b"\x1b[?9001h";
    const RESET: &[u8] = b"\x1b[?9001l";
    tail.extend_from_slice(chunk);
    let mut last = None;
    let mut i = 0;
    while i + SET.len() <= tail.len() {
        if tail[i..].starts_with(SET) {
            last = Some(true);
            i += SET.len();
        } else if tail[i..].starts_with(RESET) {
            last = Some(false);
            i += RESET.len();
        } else {
            i += 1;
        }
    }
    let keep = tail.len().saturating_sub(SET.len() - 1);
    tail.drain(..keep);
    last
}

/// The terminal's answer to a program asking whether win32-input-mode is on
/// (`\e[?9001$p`). alacritty does not know the mode and says so (`0`, not
/// recognised), which told lazygit the mode did not exist while `<Esc>` was
/// already arriving in it. Said as it is instead: set (1) or reset (2).
pub(crate) fn answer_win32_query(reply: String, on: bool) -> String {
    match reply.as_str() {
        "\x1b[?9001;0$y" => format!("\x1b[?9001;{}$y", if on { 1 } else { 2 }),
        _ => reply,
    }
}

/// Pull the directories out of any OSC 7 sequences in `chunk`.
///
/// The shape is `ESC ] 7 ; file://host/path` closed by BEL or ST (`ESC \`).
///
/// A read stops wherever the pipe happened to fill, which can be anywhere —
/// including between the `ESC` and the `]`. So `carry` holds whatever of the
/// previous read could still matter, and each call works over the join: an
/// unfinished sequence, or the first bytes of a start marker. Everything
/// older is dropped, so the buffer does not grow with the output.
pub(crate) fn scan_osc7(carry: &mut Vec<u8>, chunk: &[u8]) -> Vec<PathBuf> {
    const START: &[u8] = b"\x1b]7;";
    let mut out = Vec::new();
    carry.extend_from_slice(chunk);

    let mut from = 0usize;
    // Where the kept tail begins once the scan runs out of complete sequences.
    let keep;
    loop {
        let Some(rel) = find(&carry[from..], START) else {
            // Nothing begun: only a split start marker could still matter.
            keep = from.max(carry.len().saturating_sub(START.len() - 1));
            break;
        };
        let at = from + rel;
        let body = at + START.len();
        match end_of_osc(&carry[body..]) {
            Some((end, skip)) => {
                if let Some(p) = from_file_url(&carry[body..body + end]) {
                    out.push(p);
                }
                from = body + end + skip;
            }
            None => {
                // Still waiting for the end. A sequence this long is not a
                // path, so give up rather than hold it for ever.
                keep = match carry.len() - at > MAX_OSC {
                    true => carry.len(),
                    false => at,
                };
                break;
            }
        }
    }
    carry.drain(..keep);
    out
}

/// Where an OSC body ends, and how many bytes the terminator takes.
pub(crate) fn end_of_osc(bytes: &[u8]) -> Option<(usize, usize)> {
    let bel = bytes.iter().position(|&b| b == 0x07);
    let st = find(bytes, b"\x1b\\");
    match (bel, st) {
        (Some(a), Some(b)) if a < b => Some((a, 1)),
        (Some(_), Some(b)) => Some((b, 2)),
        (Some(a), None) => Some((a, 1)),
        (None, Some(b)) => Some((b, 2)),
        (None, None) => None,
    }
}

pub(crate) fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The path out of a `file://host/path` URL, with percent-escapes undone.
///
/// The host is whatever machine the shell is on; a remote one names a path
/// this side cannot open, but there is no way to tell from here, so it is
/// taken at face value and simply fails to list if it is not there.
pub(crate) fn from_file_url(bytes: &[u8]) -> Option<PathBuf> {
    let s = std::str::from_utf8(bytes).ok()?.trim();
    let rest = s.strip_prefix("file://")?;
    // Past the host, which may be empty (`file:///home/…`).
    let path = &rest[rest.find('/')?..];
    let decoded = percent_decode(path);
    let decoded = decoded.trim_end_matches('/');
    if decoded.is_empty() {
        return Some(PathBuf::from("/"));
    }
    // A share: PowerShell writes `\\host\share` as `file://///host/share`, and
    // a UNC prefix needs exactly two slashes -- with three it read as
    // `\host\share`, a path on the current drive (#101, 29.4).
    if decoded.starts_with("//") {
        let unc = format!("//{}", decoded.trim_start_matches('/'));
        return Some(crate::util::normalize(Path::new(&unc)));
    }
    // Windows spells it `file:///C:/dir`, which is a path once the slash goes.
    let trimmed = match decoded.as_bytes() {
        [b'/', c, b':', ..] if c.is_ascii_alphabetic() => &decoded[1..],
        _ => decoded,
    };
    Some(crate::util::normalize(Path::new(trimmed)))
}

pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The notifications in `chunk`, carried on from `carry` as `scan_osc7`
/// does: what a program asks the terminal to tell the user. Three forms are
/// in use, and coding agents send them when they want a person (cmux reads
/// the same three):
///
/// - OSC 9 (`ESC ] 9 ; text BEL`), iTerm2's -- but not `9;4;`, ConEmu's
///   progress bar, which Windows Terminal also reads;
/// - OSC 777 (`ESC ] 777 ; notify ; title ; body BEL`), rxvt's;
/// - OSC 99 (`ESC ] 99 ; metadata ; payload ST`), kitty's.
pub(crate) fn scan_notices(carry: &mut Vec<u8>, chunk: &[u8]) -> Vec<String> {
    const START: &[u8] = b"\x1b]";
    let mut out = Vec::new();
    carry.extend_from_slice(chunk);
    let mut from = 0usize;
    let keep;
    loop {
        let Some(rel) = find(&carry[from..], START) else {
            keep = from.max(carry.len().saturating_sub(START.len() - 1));
            break;
        };
        let at = from + rel;
        let body = at + START.len();
        match end_of_osc(&carry[body..]) {
            Some((end, skip)) => {
                if let Some(text) = notice(&carry[body..body + end]) {
                    out.push(text);
                }
                from = body + end + skip;
            }
            None => {
                keep = if carry.len() - at > MAX_OSC { carry.len() } else { at };
                break;
            }
        }
    }
    carry.drain(..keep);
    out
}

/// The text of one OSC body, when it is a notification.
fn notice(body: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(body);
    if let Some(rest) = s.strip_prefix("9;") {
        return (!rest.starts_with("4;")).then(|| rest.to_owned());
    }
    if let Some(rest) = s.strip_prefix("777;notify;") {
        let (title, body) = rest.split_once(';').unwrap_or((rest, ""));
        return Some(match (title.is_empty(), body.is_empty()) {
            (false, false) => format!("{title}: {body}"),
            (true, _) => body.to_owned(),
            (_, true) => title.to_owned(),
        });
    }
    if let Some(rest) = s.strip_prefix("99;") {
        let (_, payload) = rest.split_once(';')?;
        return Some(payload.to_owned());
    }
    None
}

