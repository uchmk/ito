//! Recording a pane as an asciinema v2 `.cast` file: a JSON header line,
//! then one `[seconds, "o", text]` line for each read of the shell's output
//! and `[seconds, "r", "COLSxLINES"]` for each resize. `asciinema play`
//! plays it back, and the player on a web page shows it.
//!
//! The first output is the screen as it was when the recording began
//! ([`screen`]), drawn again from its cells: without it, a recording made
//! of a prompt that is only waiting plays back as an empty screen.

use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor};

/// The recording a pane is making, shared between the reader (which writes
/// the output) and the pane (which starts, stops and resizes it).
pub(crate) type Cast = Arc<Mutex<Option<CastFile>>>;

pub(crate) struct CastFile {
    file: std::fs::File,
    start: Instant,
    /// The end of a read that stopped inside a character, for the next.
    partial: Vec<u8>,
}

impl CastFile {
    /// A new file at `path`, its header written, then `screen` (the screen
    /// as it is, see [`screen`]) as the first output, at no time at all.
    pub(crate) fn create(path: &Path, cols: usize, lines: usize, title: &str, screen: &str) -> io::Result<Self> {
        let mut file = std::fs::File::create(path)?;
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        file.write_all(header(cols, lines, unix, title).as_bytes())?;
        if !screen.is_empty() {
            file.write_all(event(0.0, "o", screen).as_bytes())?;
        }
        Ok(Self { file, start: Instant::now(), partial: Vec::new() })
    }

    /// The shell's output, whole characters only: a character cut at the
    /// end waits for the rest.
    pub(crate) fn output(&mut self, bytes: &[u8]) {
        let text = whole_utf8(&mut self.partial, bytes);
        if !text.is_empty() {
            self.event("o", &text);
        }
    }

    pub(crate) fn resize(&mut self, cols: usize, lines: usize) {
        self.event("r", &format!("{cols}x{lines}"));
    }

    fn event(&mut self, kind: &str, data: &str) {
        let line = event(self.start.elapsed().as_secs_f64(), kind, data);
        let _ = self.file.write_all(line.as_bytes());
    }
}

pub(crate) fn header(cols: usize, lines: usize, unix: u64, title: &str) -> String {
    let title = if title.is_empty() { String::new() } else { format!(", \"title\": {}", json_string(title)) };
    format!("{{\"version\": 2, \"width\": {cols}, \"height\": {lines}, \"timestamp\": {unix}{title}, \"env\": {{\"TERM\": \"xterm-256color\"}}}}\n")
}

pub(crate) fn event(seconds: f64, kind: &str, data: &str) -> String {
    format!("[{seconds:.6}, {}, {}]\n", json_string(kind), json_string(data))
}

/// The screen of `term` as output that draws it again on an empty one: each
/// row from its first column, the colours and looks of the cells as SGR, the
/// blanks at the end of a row left out, then the cursor where it is (hidden
/// when the program hid it). What is in the scrollback is not drawn; the
/// alternate screen, while a full-screen program has it, is.
pub(crate) fn screen<T: EventListener>(term: &Term<T>) -> String {
    let grid = term.grid();
    let mut out = String::from("\x1b[0m\x1b[2J");
    for row in 0..grid.screen_lines() {
        let cells = &grid[Line(row as i32)];
        let blank = |c: &Cell| c.c == ' ' && c.zerowidth().is_none() && sgr(c) == sgr(&Cell::default());
        let end = (0..grid.columns()).rev().find(|&c| !blank(&cells[Column(c)])).map_or(0, |c| c + 1);
        if end == 0 {
            continue;
        }
        out.push_str(&format!("\x1b[{};1H", row + 1));
        let mut look = sgr(&Cell::default());
        for col in 0..end {
            let cell = &cells[Column(col)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let now = sgr(cell);
            if now != look {
                out.push_str(&format!("\x1b[{now}m"));
                look = now;
            }
            out.push(if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) { ' ' } else { cell.c });
            out.extend(cell.zerowidth().into_iter().flatten());
        }
        out.push_str("\x1b[0m");
    }
    let at = grid.cursor.point;
    out.push_str(&format!("\x1b[{};{}H", at.line.0 + 1, at.column.0 + 1));
    if !term.mode().contains(TermMode::SHOW_CURSOR) {
        out.push_str("\x1b[?25l");
    }
    out
}

/// The SGR parameters that give a cell its colours and looks, from nothing
/// (`0` first).
fn sgr(cell: &Cell) -> String {
    let mut p = vec!["0".to_owned()];
    let f = cell.flags;
    for (flag, code) in
        [(Flags::BOLD, "1"), (Flags::DIM, "2"), (Flags::ITALIC, "3"), (Flags::INVERSE, "7"), (Flags::HIDDEN, "8"), (Flags::STRIKEOUT, "9")]
    {
        if f.contains(flag) {
            p.push(code.to_owned());
        }
    }
    let line = [
        (Flags::DOUBLE_UNDERLINE, "4:2"),
        (Flags::UNDERCURL, "4:3"),
        (Flags::DOTTED_UNDERLINE, "4:4"),
        (Flags::DASHED_UNDERLINE, "4:5"),
        (Flags::UNDERLINE, "4"),
    ];
    if let Some((_, code)) = line.iter().find(|(flag, _)| f.contains(*flag)) {
        p.push((*code).to_owned());
    }
    p.extend(colour(cell.fg, "38", Some((30, 90))));
    p.extend(colour(cell.bg, "48", Some((40, 100))));
    p.extend(cell.underline_color().and_then(|c| colour(c, "58", None)));
    p.join(";")
}

/// A colour as SGR: one of the sixteen named ones from `eight` (where the
/// eight and the bright eight start) when there is one for it, else after
/// `ext` (38, 48 or 58) as one of 256 or as RGB. Nothing for the default.
fn colour(c: Color, ext: &str, eight: Option<(u8, u8)>) -> Option<String> {
    let n = match c {
        Color::Spec(rgb) => return Some(format!("{ext};2;{};{};{}", rgb.r, rgb.g, rgb.b)),
        Color::Indexed(i) => return Some(format!("{ext};5;{i}")),
        Color::Named(n) if (n as usize) < 16 => n as u8,
        // The dim eight are the renderer's, never a cell's; the plain eight.
        Color::Named(n) if (NamedColor::DimBlack..=NamedColor::DimWhite).contains(&n) => (n as usize - NamedColor::DimBlack as usize) as u8,
        Color::Named(_) => return None,
    };
    Some(match eight {
        Some((base, _)) if n < 8 => (base + n).to_string(),
        Some((_, bright)) => (bright + n - 8).to_string(),
        None => format!("{ext};5;{n}"),
    })
}

/// `bytes` after what `partial` kept, as text up to the last whole
/// character; what is left of a cut one goes back into `partial`. Bytes that
/// are not UTF-8 at all become U+FFFD.
pub(crate) fn whole_utf8(partial: &mut Vec<u8>, bytes: &[u8]) -> String {
    partial.extend_from_slice(bytes);
    let keep = match std::str::from_utf8(partial) {
        Ok(_) => 0,
        // Cut at the end (no error length): keep the start of the character.
        Err(e) if e.error_len().is_none() => partial.len() - e.valid_up_to(),
        Err(_) => 0,
    };
    let rest = partial.split_off(partial.len() - keep);
    let text = String::from_utf8_lossy(partial).into_owned();
    *partial = rest;
    text
}

/// A JSON string, quoted, with what JSON will not take as it is escaped.
pub(crate) fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_json_lines() {
        assert_eq!(event(1.5, "o", "a\"b\\\x1b[0m\r\n"), "[1.500000, \"o\", \"a\\\"b\\\\\\u001b[0m\\r\\n\"]\n");
        assert_eq!(
            header(80, 24, 7, ""),
            "{\"version\": 2, \"width\": 80, \"height\": 24, \"timestamp\": 7, \"env\": {\"TERM\": \"xterm-256color\"}}\n"
        );
        assert!(header(80, 24, 7, "pwsh").contains(", \"title\": \"pwsh\","));
    }

    #[test]
    fn a_character_cut_in_two_waits_for_its_end() {
        let mut partial = Vec::new();
        let a = "あ".as_bytes();
        assert_eq!(whole_utf8(&mut partial, &[b'x', a[0], a[1]]), "x");
        assert_eq!(partial, &a[..2]);
        assert_eq!(whole_utf8(&mut partial, &[a[2], b'y']), "あy");
        assert!(partial.is_empty());
        assert_eq!(whole_utf8(&mut partial, &[0xff, b'z']), "\u{fffd}z");
    }

    /// The screen comes back as rows from their first column with their
    /// looks, the blanks at the end and the empty rows left out, and the
    /// cursor where it was.
    #[test]
    fn the_screen_is_drawn_again_from_its_cells() {
        let mut t = crate::testing::term(20, 4);
        crate::testing::feed(
            &mut t,
            "$ ls\r\n\x1b[1;31mred\x1b[0m \x1b[4:3;38;2;1;2;3mx\x1b[0m\x1b[44m  \x1b[0m\r\n\r\n\x1b[93;48;5;200mあ\x1b[0mb",
        );
        assert_eq!(
            screen(&t),
            "\x1b[0m\x1b[2J\x1b[1;1H$ ls\x1b[0m\x1b[2;1H\x1b[0;1;31mred\x1b[0m \x1b[0;4:3;38;2;1;2;3mx\x1b[0;44m  \x1b[0m\
             \x1b[4;1H\x1b[0;93;48;5;200mあ\x1b[0mb\x1b[0m\x1b[4;4H"
        );
        crate::testing::feed(&mut t, "\x1b[?25l\x1b[2;2H");
        assert!(screen(&t).ends_with("\x1b[2;2H\x1b[?25l"));
    }

    #[test]
    fn a_recording_is_a_playable_file() {
        let dir = std::env::temp_dir().join(format!("tsumugi-cast-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("a.cast");
        let mut c = CastFile::create(&path, 80, 24, "t", "\x1b[2J$ ").unwrap();
        c.output(b"hello\r\n");
        c.resize(100, 30);
        drop(c);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1], "[0.000000, \"o\", \"\\u001b[2J$ \"]");
        assert!(lines[2].ends_with(", \"o\", \"hello\\r\\n\"]"), "{}", lines[2]);
        assert!(lines[3].ends_with(", \"r\", \"100x30\"]"), "{}", lines[3]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
