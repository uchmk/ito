//! Triggers, as iTerm2 has them: a regular expression that, when the output
//! matches it, colours what it matched ([`Highlight`], on the screen as
//! drawn) or says so (the lines [`Terminal::set_triggers`] is given a set
//! for, read as they are written, before the screen keeps them).
//!
//! [`Terminal::set_triggers`]: crate::Terminal::set_triggers

use crate::CellView;

/// The longest line read for a trigger, in bytes; the rest of a longer one
/// is not looked at (a progress bar or a minified file would make each
/// match slower than the line is worth).
const LONGEST: usize = 4096;

/// What an escape sequence in the output has got to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum State {
    #[default]
    Text,
    /// After ESC: one more character, or an intermediate and then one.
    Esc,
    /// A CSI, up to its final byte.
    Csi,
    /// An OSC, up to BEL or ST.
    Osc,
    /// A DCS, APC, PM or SOS, up to ST.
    Str,
    /// ESC inside one of those two: `\` ends it.
    OscEsc,
    StrEsc,
}

/// The output as plain lines: escape sequences taken out, a line ended by
/// `\n`, and a lone `\r` starting it again (what a progress bar redraws
/// with). Fed what is read, a piece at a time; a sequence or a line cut in
/// two by a read is carried to the next.
#[derive(Debug, Default)]
pub(crate) struct Lines {
    state: State,
    line: Vec<u8>,
    /// A `\r` has come and nothing after it yet: a `\n` ends the line, and
    /// anything else writes over it from the start.
    cr: bool,
    /// A CSI's parameters so far (`?1049` and the like), kept short.
    params: Vec<u8>,
    /// A full-screen program is on the alternate screen: what it draws
    /// there is not lines of output, and is not read.
    alt: bool,
}

impl Lines {
    /// The lines `chunk` ends, trimmed, leaving out empty ones.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in chunk {
            // CAN and SUB cut a sequence short, whatever it has got to.
            if matches!(b, 0x18 | 0x1a) {
                self.state = State::Text;
                continue;
            }
            self.state = match self.state {
                State::Text => match b {
                    0x1b => State::Esc,
                    b'\n' => {
                        self.end(&mut out);
                        State::Text
                    }
                    b'\r' => {
                        self.cr = true;
                        State::Text
                    }
                    0x08 => {
                        // Backspace takes back a whole character.
                        while let Some(last) = self.line.pop() {
                            if last & 0xc0 != 0x80 {
                                break;
                            }
                        }
                        State::Text
                    }
                    b'\t' | 0x20.. if b != 0x7f => {
                        if std::mem::take(&mut self.cr) {
                            self.line.clear();
                        }
                        if self.line.len() < LONGEST {
                            self.line.push(if b == b'\t' { b' ' } else { b });
                        }
                        State::Text
                    }
                    _ => State::Text,
                },
                State::Esc => match b {
                    b'[' => {
                        self.params.clear();
                        State::Csi
                    }
                    // A line ended by a sequence that did not finish.
                    b'\n' => {
                        self.end(&mut out);
                        State::Text
                    }
                    b']' => State::Osc,
                    b'P' | b'_' | b'^' | b'X' => State::Str,
                    // `ESC ( B` and its kind: the intermediate, then one more.
                    0x20..=0x2f => State::Esc,
                    0x1b => State::Esc,
                    _ => State::Text,
                },
                State::Csi => match b {
                    0x40..=0x7e => {
                        if matches!(b, b'h' | b'l') {
                            self.mode(b == b'h');
                        }
                        State::Text
                    }
                    0x1b => State::Esc,
                    _ => {
                        if self.params.len() < 32 {
                            self.params.push(b);
                        }
                        State::Csi
                    }
                },
                State::Osc => match b {
                    0x07 => State::Text,
                    0x1b => State::OscEsc,
                    _ => State::Osc,
                },
                State::OscEsc => match b {
                    b'\\' => State::Text,
                    _ => State::Osc,
                },
                State::Str => match b {
                    0x1b => State::StrEsc,
                    _ => State::Str,
                },
                State::StrEsc => match b {
                    b'\\' => State::Text,
                    _ => State::Str,
                },
            };
        }
        out
    }

    /// A private mode set (`h`) or reset (`l`): the alternate screen's
    /// (`?1049`, `?1047`, `?47`) is the one followed. Going either way, the
    /// line begun is left behind.
    fn mode(&mut self, on: bool) {
        let Some(list) = self.params.strip_prefix(b"?") else { return };
        if list.split(|b| *b == b';').any(|m| matches!(m, b"1049" | b"1047" | b"47")) {
            self.alt = on;
            self.line.clear();
            self.cr = false;
        }
    }

    fn end(&mut self, out: &mut Vec<String>) {
        self.cr = false;
        let text = String::from_utf8_lossy(&self.line);
        let text = text.trim();
        if !text.is_empty() && !self.alt {
            out.push(text.to_owned());
        }
        self.line.clear();
    }
}

/// A line of the output that triggers matched: which ones, by their
/// place in the set given, and the line as plain text.
#[derive(Clone, Debug, PartialEq)]
pub struct Triggered {
    pub rules: Vec<usize>,
    pub line: String,
}

/// The triggers a reader matches lines against; none set, it reads none.
pub(crate) type Set = std::sync::Arc<std::sync::Mutex<Option<regex::RegexSet>>>;

/// The reader's half: the lines `chunk` ends, matched against `set`.
pub(crate) fn scan(lines: &mut Lines, set: &Set, chunk: &[u8]) -> Vec<Triggered> {
    let set = set.lock().unwrap_or_else(|e| e.into_inner());
    let Some(set) = set.as_ref() else {
        // Nothing to match: nothing kept, and a set given later starts clean.
        *lines = Lines::default();
        return Vec::new();
    };
    lines
        .feed(chunk)
        .into_iter()
        .filter_map(|line| {
            let rules: Vec<usize> = set.matches(&line).into_iter().collect();
            (!rules.is_empty()).then_some(Triggered { rules, line })
        })
        .collect()
}

/// What a match of a trigger is drawn in: its text's colour, its
/// background's, or both (`[r, g, b]`).
#[derive(Clone, Debug)]
pub struct Highlight {
    pub regex: regex::Regex,
    pub fg: Option<[u8; 3]>,
    pub bg: Option<[u8; 3]>,
}

/// A colour as the settings write it, `#rrggbb`.
pub fn rgb(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let at = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([at(0)?, at(2)?, at(4)?])
}

/// A cell's colours from triggers: its text's and its background's.
pub type Paint = (Option<[u8; 3]>, Option<[u8; 3]>);

/// The colours `rules` give each cell of `row`: by cell, the text's and
/// the background's, `None` where no match covers it (or nothing matched
/// at all: an empty list). A row is matched alone, so a match is never cut
/// by where it wrapped from the line before -- nor found across it. Later
/// rules draw over earlier ones.
pub fn highlight_row(row: &[CellView], rules: &[Highlight]) -> Vec<Option<Paint>> {
    use alacritty_terminal::term::cell::Flags;
    if rules.is_empty() || row.iter().all(|c| c.c == ' ' || c.c == '\0') {
        return Vec::new();
    }
    // The row's text, and for each byte of it the cell it came from (the
    // right half of a wide character is not a character of its own).
    let mut text = String::with_capacity(row.len());
    let mut cell_of = Vec::with_capacity(row.len());
    for (x, c) in row.iter().enumerate() {
        if c.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let ch = if c.c == '\0' { ' ' } else { c.c };
        text.push(ch);
        cell_of.extend(std::iter::repeat_n(x, ch.len_utf8()));
    }
    // Matched as the line a trigger that tells hears it: without the
    // blanks either side, so `^` and `$` mean the same to both.
    let from = text.len() - text.trim_start().len();
    let line = text.trim();
    let mut out = Vec::new();
    for rule in rules {
        for m in rule.regex.find_iter(line) {
            if m.is_empty() {
                continue;
            }
            if out.is_empty() {
                out = vec![None; row.len()];
            }
            let first = cell_of[from + m.start()];
            let last = cell_of[from + m.end() - 1];
            // A wide character's spacer goes with it.
            let last = if row.get(last).is_some_and(|c| c.flags.contains(Flags::WIDE_CHAR)) { last + 1 } else { last };
            for slot in out.iter_mut().take((last + 1).min(row.len())).skip(first) {
                let (fg, bg) = slot.unwrap_or_default();
                *slot = Some((rule.fg.or(fg), rule.bg.or(bg)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::term::cell::Flags;
    use alacritty_terminal::vte::ansi::{Color, NamedColor};

    fn cells(s: &str) -> Vec<CellView> {
        let cell = |c, flags| CellView { c, fg: Color::Named(NamedColor::Foreground), bg: Color::Named(NamedColor::Background), flags, ul: None, selected: false };
        let mut out = Vec::new();
        for c in s.chars() {
            if unicode_wide(c) {
                out.push(cell(c, Flags::WIDE_CHAR));
                out.push(cell(' ', Flags::WIDE_CHAR_SPACER));
            } else {
                out.push(cell(c, Flags::empty()));
            }
        }
        out
    }

    fn unicode_wide(c: char) -> bool {
        ('\u{3000}'..='\u{9fff}').contains(&c)
    }

    fn rule(re: &str, fg: Option<[u8; 3]>, bg: Option<[u8; 3]>) -> Highlight {
        Highlight { regex: regex::Regex::new(re).unwrap(), fg, bg }
    }

    #[test]
    fn lines_take_out_escapes_and_end_at_newlines() {
        let mut l = Lines::default();
        assert_eq!(l.feed(b"\x1b[31mred\x1b[0m text\r\n\n  \nnext"), vec!["red text"]);
        assert_eq!(l.feed(b" line\n"), vec!["next line"]);
        // An OSC ended by BEL and by ST; a DCS; a charset choice.
        assert_eq!(l.feed(b"\x1b]0;title\x07a\x1b]8;;http://x\x1b\\b\x1bPq#0\x1b\\c\x1b(Bd\n"), vec!["abcd"]);
    }

    #[test]
    fn the_alternate_screen_is_not_read_and_a_cut_sequence_ends() {
        let mut l = Lines::default();
        assert_eq!(l.feed(b"before\n\x1b[?1049hdrawn\nmore\n\x1b[?1049lafter\n"), vec!["before", "after"]);
        assert_eq!(l.feed(b"\x1b[?1;47hx\n\x1b[?47l"), Vec::<String>::new());
        // ESC then a newline: the line ends. CAN cuts a CSI short.
        assert_eq!(l.feed(b"one\x1b\ntwo\x1b[12\x18three\n"), vec!["one", "twothree"]);
    }

    #[test]
    fn lines_carry_a_sequence_cut_by_a_read() {
        let mut l = Lines::default();
        assert!(l.feed(b"ok \x1b[3").is_empty());
        assert!(l.feed(b"2m").is_empty());
        assert_eq!(l.feed(b"done\n"), vec!["ok done"]);
    }

    #[test]
    fn a_lone_cr_starts_the_line_again_and_backspace_takes_a_character() {
        let mut l = Lines::default();
        assert_eq!(l.feed(b" 10%\r 50%\r100% done\n"), vec!["100% done"]);
        assert_eq!(l.feed("ab\u{3042}\x08c\x08\x08d\tx\n".as_bytes()), vec!["ad x"]);
    }

    #[test]
    fn scan_matches_whole_lines_against_the_set() {
        let set: Set = Default::default();
        let mut l = Lines::default();
        assert!(scan(&mut l, &set, b"error: half").is_empty());
        *set.lock().unwrap() = Some(regex::RegexSet::new(["error", "warn"]).unwrap());
        // What came while no set was given is not kept.
        assert_eq!(scan(&mut l, &set, b" line\nerror and warn\nfine\nwarn\n"), vec![
            Triggered { rules: vec![0, 1], line: "error and warn".into() },
            Triggered { rules: vec![1], line: "warn".into() },
        ]);
    }

    #[test]
    fn rgb_reads_hex_colours_only() {
        assert_eq!(rgb("#ff8000"), Some([255, 128, 0]));
        assert_eq!(rgb(" #00FFaa "), Some([0, 255, 170]));
        assert_eq!(rgb("ff8000"), None);
        assert_eq!(rgb("#fff"), None);
        assert_eq!(rgb("#gg0000"), None);
        assert_eq!(rgb("#ff80\u{e9}"), None);
    }

    #[test]
    fn highlight_row_colours_the_cells_matched() {
        let red = Some([255, 0, 0]);
        let blue = Some([0, 0, 255]);
        let row = cells("an error here");
        assert!(highlight_row(&row, &[rule("warn", red, None)]).is_empty());
        assert!(highlight_row(&row, &[]).is_empty());
        let got = highlight_row(&row, &[rule("error", red, None), rule("err", None, blue)]);
        assert_eq!(got.len(), row.len());
        assert_eq!(got[2], None);
        assert_eq!(got[3], Some((red, blue)));
        assert_eq!(got[6], Some((red, None)));
        assert_eq!(got[8], None);
        // A later rule's colour wins where both give one.
        let got = highlight_row(&row, &[rule("error", red, None), rule("ror", blue, None)]);
        assert_eq!((got[3], got[7]), (Some((red, None)), Some((blue, None))));
    }

    #[test]
    fn highlight_row_takes_a_wide_character_with_its_spacer() {
        let red = Some([255, 0, 0]);
        let row = cells("a\u{6f22}\u{5b57}b");
        let got = highlight_row(&row, &[rule("\u{5b57}", red, None)]);
        assert_eq!(got.iter().map(Option::is_some).collect::<Vec<_>>(), [false, false, false, true, true, false]);
        // A pattern that can match nothing never colours anything.
        assert!(highlight_row(&row, &[rule("x*", red, None)]).is_empty());
    }

    #[test]
    fn highlight_row_matches_the_line_without_its_blanks() {
        let red = Some([255, 0, 0]);
        // The row as the screen has it, padded to the pane's width.
        let row = cells("  done   ");
        let got = highlight_row(&row, &[rule("^done$", red, None)]);
        assert_eq!(got.iter().map(Option::is_some).collect::<Vec<_>>(), [false, false, true, true, true, true, false, false, false]);
    }
}
