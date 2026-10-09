//! A work log: the pane's text appended to a plain-text file as it goes, so
//! a session that is still running has a readable record on disk.
//!
//! Lines go to the file as they scroll off the screen into the scrollback,
//! which is when they stop changing; the screen itself is written when the
//! log stops or the pane ends. A line wrapped across rows is one line, as in
//! [`all_text`](crate::all_text). While a full-screen program has the
//! alternate screen nothing is taken, and its screen is not written.
//!
//! Which scrollback rows are new is counted while the scrollback is growing.
//! Once it is full (old rows fall off the top as new ones come), or a resize
//! has reflowed it, or it was cleared, the count says nothing, and the place
//! is found again by the last lines written. If they are not there at all,
//! everything in the scrollback is taken as new.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};

/// How many of the last lines written are kept to find the place by.
const TAIL: usize = 6;

/// How often a running log looks at the pane, at most.
const EVERY: Duration = Duration::from_millis(500);

pub(crate) struct WorkLog {
    file: std::fs::File,
    path: PathBuf,
    places: Places,
    last: Option<Instant>,
}

/// Where the log has got to in the scrollback.
#[derive(Default)]
pub(crate) struct Places {
    /// The last lines written, oldest first.
    tail: VecDeque<String>,
    /// The scrollback rows written, counted from the oldest, while that
    /// count means something (see `take`).
    seen: usize,
    /// The columns the rows were counted at.
    cols: usize,
    /// Nothing has been taken yet.
    fresh: bool,
}

impl WorkLog {
    /// A new log at `path` (created, or emptied when it is there).
    pub(crate) fn create(path: &Path) -> io::Result<Self> {
        let file = std::fs::File::create(path)?;
        Ok(Self { file, path: path.to_owned(), places: Places { fresh: true, ..Default::default() }, last: None })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The lines new since the last time, when it is time to look again.
    pub(crate) fn due<T: EventListener>(&mut self, term: &Term<T>, cap: usize) -> Vec<String> {
        if self.last.is_some_and(|t| t.elapsed() < EVERY) {
            return Vec::new();
        }
        self.last = Some(Instant::now());
        self.places.take(term, cap)
    }

    /// The last lines: what is new in the scrollback, then the screen.
    pub(crate) fn last_lines<T: EventListener>(&mut self, term: &Term<T>, cap: usize) -> Vec<String> {
        let mut out = self.places.take(term, cap);
        if !term.mode().contains(TermMode::ALT_SCREEN) {
            out.extend(self.places.screen(term));
        }
        out
    }

    pub(crate) fn write(&mut self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        let mut text = String::new();
        for l in lines {
            text.push_str(l);
            text.push('\n');
        }
        let _ = self.file.write_all(text.as_bytes());
    }
}

impl Places {
    /// The scrollback's lines not yet written; a line still wrapping onto
    /// the screen waits.
    pub(crate) fn take<T: EventListener>(&mut self, term: &Term<T>, cap: usize) -> Vec<String> {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return Vec::new();
        }
        let grid = term.grid();
        let h = grid.history_size();
        let cols = grid.columns();
        let counted = !self.fresh && cols == self.cols && h >= self.seen && h < cap;
        let found = if counted {
            lines(term, self.seen as i32 - h as i32, -1)
        } else {
            self.find(term, h)
        };
        self.fresh = false;
        self.cols = cols;
        // Where the last whole line ends, as a count from the oldest row.
        if let Some((_, end)) = found.last() {
            self.seen = (end + h as i32 + 1) as usize;
        } else if !counted {
            self.seen = h;
            // A row wrapping onto the screen is not written yet: count back
            // to the start of its line.
            let mut l = -1;
            while l >= -(h as i32) && wraps(term, l) {
                self.seen -= 1;
                l -= 1;
            }
        }
        let out: Vec<String> = found.into_iter().map(|(s, _)| s).collect();
        for l in &out {
            self.tail.push_back(l.clone());
            if self.tail.len() > TAIL {
                self.tail.pop_front();
            }
        }
        out
    }

    /// The new lines when counting cannot say: those after the last lines
    /// written, looked for from the newest back in ever larger pieces, or
    /// everything when they are not there.
    fn find<T: EventListener>(&self, term: &Term<T>, h: usize) -> Vec<(String, i32)> {
        let all = || lines(term, -(h as i32), -1);
        if self.tail.is_empty() {
            return all();
        }
        let mut span = 256;
        loop {
            let whole = span >= h;
            let got = if whole { all() } else { lines(term, -(span as i32), -1) };
            // The first line of a piece may be the end of a longer one, so a
            // match must not start on it unless the piece is everything.
            let first = usize::from(!whole);
            let n = self.tail.len();
            let at = (first + n..=got.len()).rev().find(|&i| got[i - n..i].iter().map(|(s, _)| s).eq(self.tail.iter()));
            if let Some(i) = at {
                return got.into_iter().skip(i).collect();
            }
            if whole {
                return got;
            }
            span *= 4;
        }
    }

    /// The screen's lines from where the scrollback's written ones end, the
    /// blank ones at the bottom left out.
    fn screen<T: EventListener>(&self, term: &Term<T>) -> Vec<String> {
        let grid = term.grid();
        let h = grid.history_size() as i32;
        let from = (self.seen as i32 - h).min(0);
        let mut out = whole_and_rest(term, from, grid.screen_lines() as i32 - 1);
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        out
    }
}

/// Whether row `l` goes on into the next.
fn wraps<T: EventListener>(term: &Term<T>, l: i32) -> bool {
    let grid = term.grid();
    grid[Line(l)][Column(grid.columns() - 1)].flags.contains(Flags::WRAPLINE)
}

fn row_text<T: EventListener>(term: &Term<T>, l: i32, into: &mut String) {
    let grid = term.grid();
    let row = &grid[Line(l)];
    for c in 0..grid.columns() {
        let cell = &row[Column(c)];
        if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            into.push(cell.c);
        }
    }
}

/// The whole lines in rows `lo..=hi`, each with the row it ends on. A line
/// that goes on past `hi` is left out.
fn lines<T: EventListener>(term: &Term<T>, lo: i32, hi: i32) -> Vec<(String, i32)> {
    let mut out = Vec::new();
    let mut line = String::new();
    for l in lo..=hi {
        row_text(term, l, &mut line);
        if !wraps(term, l) {
            out.push((line.trim_end().to_owned(), l));
            line.clear();
        }
    }
    out
}

/// The lines in rows `lo..=hi`, the last one too however it ends.
fn whole_and_rest<T: EventListener>(term: &Term<T>, lo: i32, hi: i32) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for l in lo..=hi {
        row_text(term, l, &mut line);
        if !wraps(term, l) || l == hi {
            out.push(line.trim_end().to_owned());
            line.clear();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{feed, term};

    /// The scrollback's cap in `testing::term`.
    const CAP: usize = 200;

    #[test]
    fn lines_go_out_as_they_scroll_off() {
        let mut t = term(10, 3);
        let mut p = Places { fresh: true, ..Default::default() };
        feed(&mut t, "one\r\ntwo\r\n");
        assert!(p.take(&t, CAP).is_empty(), "all still on screen");
        feed(&mut t, "three\r\nfour\r\n");
        assert_eq!(p.take(&t, CAP), ["one", "two"]);
        assert!(p.take(&t, CAP).is_empty(), "nothing twice");
        feed(&mut t, "five\r\n");
        assert_eq!(p.take(&t, CAP), ["three"]);
        assert_eq!(p.screen(&t), ["four", "five"]);
    }

    /// A long line is one line, and waits while its end is on screen.
    #[test]
    fn a_wrapped_line_waits_for_its_end() {
        let mut t = term(10, 3);
        let mut p = Places { fresh: true, ..Default::default() };
        feed(&mut t, "a\r\nb\r\n0123456789abcdefghij0123456789ABC");
        // `a` and `b` and the first rows of the long line are off screen.
        assert_eq!(p.take(&t, CAP), ["a", "b"]);
        feed(&mut t, "\r\nend\r\nx\r\ny\r\n");
        assert_eq!(p.take(&t, CAP), ["0123456789abcdefghij0123456789ABC", "end"]);
        assert_eq!(p.screen(&t), ["x", "y"]);
    }

    /// With the scrollback full the count says nothing; the last lines
    /// written find the place.
    #[test]
    fn a_full_scrollback_finds_its_place_by_the_last_lines() {
        let mut t = term(12, 3);
        let mut p = Places { fresh: true, ..Default::default() };
        let mut got = Vec::new();
        for i in 0..600 {
            feed(&mut t, &format!("line {i}\r\n"));
            if i % 37 == 0 {
                got.extend(p.take(&t, CAP));
            }
        }
        got.extend(p.take(&t, CAP));
        got.extend(p.screen(&t));
        // The first look came before the scrollback filled, and from then on
        // each look found where the last one ended.
        let want: Vec<String> = (0..600).map(|i| format!("line {i}")).collect();
        assert_eq!(got, want);
    }

    /// A narrower pane reflows the scrollback; nothing comes twice.
    #[test]
    fn a_resize_does_not_write_lines_again() {
        let mut t = term(20, 3);
        let mut p = Places { fresh: true, ..Default::default() };
        for i in 0..8 {
            feed(&mut t, &format!("a longer line {i}\r\n"));
        }
        let mut got = p.take(&t, CAP);
        t.resize(crate::Size::new(8, 3));
        feed(&mut t, "after\r\n");
        got.extend(p.take(&t, CAP));
        got.extend(p.screen(&t));
        let mut want: Vec<String> = (0..8).map(|i| format!("a longer line {i}")).collect();
        want.push("after".into());
        assert_eq!(got, want);
    }

    /// A full-screen program's screen is not taken.
    #[test]
    fn the_alternate_screen_is_left_out() {
        let mut t = term(10, 3);
        let mut p = Places { fresh: true, ..Default::default() };
        feed(&mut t, "\x1b[?1049h");
        for i in 0..6 {
            feed(&mut t, &format!("tui {i}\r\n"));
        }
        assert!(p.take(&t, CAP).is_empty());
        feed(&mut t, "\x1b[?1049lshell\r\n");
        let mut got = p.take(&t, CAP);
        got.extend(p.screen(&t));
        assert_eq!(got, ["shell"]);
    }
}
