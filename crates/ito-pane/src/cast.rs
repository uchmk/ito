//! Recording a pane as an asciinema v2 `.cast` file: a JSON header line,
//! then one `[seconds, "o", text]` line for each read of the shell's output
//! and `[seconds, "r", "COLSxLINES"]` for each resize. `asciinema play`
//! plays it back, and the player on a web page shows it.

use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
    /// A new file at `path`, its header written.
    pub(crate) fn create(path: &Path, cols: usize, lines: usize, title: &str) -> io::Result<Self> {
        let mut file = std::fs::File::create(path)?;
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        file.write_all(header(cols, lines, unix, title).as_bytes())?;
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
        assert_eq!(header(80, 24, 7, ""), "{\"version\": 2, \"width\": 80, \"height\": 24, \"timestamp\": 7, \"env\": {\"TERM\": \"xterm-256color\"}}\n");
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

    #[test]
    fn a_recording_is_a_playable_file() {
        let dir = std::env::temp_dir().join(format!("tsumugi-cast-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("a.cast");
        let mut c = CastFile::create(&path, 80, 24, "t").unwrap();
        c.output(b"hello\r\n");
        c.resize(100, 30);
        drop(c);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].ends_with(", \"o\", \"hello\\r\\n\"]"), "{}", lines[1]);
        assert!(lines[2].ends_with(", \"r\", \"100x30\"]"), "{}", lines[2]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
