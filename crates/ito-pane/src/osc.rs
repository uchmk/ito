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

/// How many times `chunk`, carried on from `tail`, asks the terminal its name
/// and version (XTVERSION: `\e[>q`, or `\e[>0q`). alacritty's parser does not
/// know the question and would let it pass unanswered; neovim, tmux, notcurses
/// and yazi ask it first and wait a moment for an answer. A read can stop
/// inside one.
pub(crate) fn scan_xtversion(tail: &mut Vec<u8>, chunk: &[u8]) -> usize {
    const ASKS: [&[u8]; 2] = [b"\x1b[>q", b"\x1b[>0q"];
    tail.extend_from_slice(chunk);
    let (mut found, mut seen) = (0, 0);
    let mut i = 0;
    while i < tail.len() {
        match ASKS.iter().find(|a| tail[i..].starts_with(a)) {
            Some(a) => {
                found += 1;
                i += a.len();
                seen = i;
            }
            None => i += 1,
        }
    }
    // A question counted is not carried on, to be counted again.
    let keep = tail.len().saturating_sub(ASKS[1].len() - 1).max(seen);
    tail.drain(..keep);
    found
}

/// The answer to XTVERSION, in the form xterm, kitty and WezTerm give it:
/// a DCS `>|` with the name and version.
pub(crate) fn xtversion() -> Vec<u8> {
    format!("\x1bP>|tsumugi {}\x1b\\", env!("CARGO_PKG_VERSION")).into_bytes()
}

/// What goes back for a query alacritty answered, put right where it says
/// less than the pane does: win32-input-mode (below), and the primary device
/// attributes, which say sixel (4) as well as VT220 (62) and colour (22) --
/// img2sixel, chafa and lsix ask before they draw.
pub(crate) fn answer_query(reply: String, win32_on: bool) -> String {
    match reply.as_str() {
        "\x1b[?6c" => "\x1b[?62;4;22c".to_owned(),
        _ => answer_win32_query(reply, win32_on),
    }
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


/// The link a prompt's first row carries; see [`PromptLinks`]. After the
/// first command it is followed by `?exit=` and the exit code the shell
/// gave for the command before (OSC 133 `D`), when it gave one.
pub const PROMPT_LINK: &str = "tsumugi:prompt";
/// The link the first row of a command's output carries (after OSC 133 `C`).
pub const OUTPUT_LINK: &str = "tsumugi:output";

/// A link tsumugi put on the grid, not a program: drawn as nothing.
pub fn is_mark(uri: &str) -> bool {
    uri.starts_with("tsumugi:")
}

/// The exit code of the command before a prompt, from its link's uri.
pub fn exit_of(uri: &str) -> Option<i32> {
    uri.strip_prefix(PROMPT_LINK)?.strip_prefix("?exit=")?.parse().ok()
}

/// Marks where each prompt and each command's output is, in the grid
/// itself. The bytes go to the parser on the reader thread with the grid
/// locked, so this side never knows which row a mark lands on; but
/// alacritty keeps an OSC 8 link on every cell written under it, scrolls it
/// with the cell and rewraps it. So after each OSC 133 `A` an OSC 8 link to
/// [`PROMPT_LINK`] is opened (with the exit code the last `D` gave), and
/// after each `C` one to [`OUTPUT_LINK`]; each is closed at the next OSC 133
/// or its first newline, whichever is first. The rows are then found by
/// their links ([`prompt_lines`](crate::prompt_lines),
/// [`blocks`](crate::blocks)). Nothing is drawn for them.
#[derive(Default)]
pub(crate) struct PromptLinks {
    /// Inside an OSC: the start of what it says, enough to tell `133;A` and
    /// read `133;D;<code>`.
    osc: Option<Vec<u8>>,
    /// The last byte was ESC.
    esc: bool,
    /// A link is open.
    open: bool,
    /// The exit code the last `133;D` gave, for the next prompt's link.
    exit: Option<i32>,
}

const CLOSE: &[u8] = b"\x1b]8;;\x1b\\";
/// How much of an OSC is kept: `133;D;-2147483648` and a little.
const KEEP: usize = 24;

fn open(uri: &str) -> Vec<u8> {
    format!("\x1b]8;id=tsumugi-mark;{uri}\x1b\\").into_bytes()
}

impl PromptLinks {
    /// `chunk` with the links put in, or `None` when it needs none.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        let mut out: Option<Vec<u8>> = None;
        for (i, &b) in chunk.iter().enumerate() {
            let mut add: Vec<u8> = Vec::new();
            match self.osc.take() {
                Some(mut said) => {
                    let mut end = false;
                    if self.esc {
                        self.esc = false;
                        match b {
                            b'\\' => end = true,
                            // Another sequence cut the OSC short.
                            b']' => self.osc = Some(Vec::new()),
                            0x1b => self.esc = true,
                            _ => {}
                        }
                    } else if b == 0x07 {
                        end = true;
                    } else {
                        if b == 0x1b {
                            self.esc = true;
                        } else if said.len() < KEEP {
                            said.push(b);
                        }
                        self.osc = Some(std::mem::take(&mut said));
                    }
                    if end && said.starts_with(b"133;") {
                        if self.open {
                            add.extend_from_slice(CLOSE);
                            self.open = false;
                        }
                        if said.starts_with(b"133;A") {
                            let uri = match self.exit.take() {
                                Some(code) => format!("{PROMPT_LINK}?exit={code}"),
                                None => PROMPT_LINK.to_owned(),
                            };
                            add.extend(open(&uri));
                            self.open = true;
                        } else if said.starts_with(b"133;C") {
                            add.extend(open(OUTPUT_LINK));
                            self.open = true;
                        } else if let Some(rest) = said.strip_prefix(b"133;D") {
                            // `133;D;<code>`, maybe more after another `;`.
                            let code = rest.strip_prefix(b";").and_then(|r| r.split(|&b| b == b';').next());
                            self.exit = code.and_then(|c| std::str::from_utf8(c).ok()).and_then(|c| c.parse().ok());
                        }
                    }
                }
                None if self.esc => {
                    self.esc = false;
                    if b == b']' {
                        self.osc = Some(Vec::new());
                    } else if b == 0x1b {
                        self.esc = true;
                    }
                }
                None if b == 0x1b => self.esc = true,
                None if b == b'\n' && self.open => {
                    // Before the newline, so only the first row has it.
                    self.open = false;
                    let o = out.get_or_insert_with(|| chunk[..i].to_vec());
                    o.extend_from_slice(CLOSE);
                }
                None => {}
            }
            if let Some(o) = out.as_mut() {
                o.push(b);
            }
            if !add.is_empty() {
                let o = out.get_or_insert_with(|| chunk[..=i].to_vec());
                o.extend_from_slice(&add);
            }
        }
        out
    }
}
