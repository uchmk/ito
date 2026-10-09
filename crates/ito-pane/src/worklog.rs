//! A work log: the pane's text in a plain-text file as it goes, so a
//! session that is still running has a readable record on disk, like a
//! terminal's log (Tera Term's): open it at any time and everything up to
//! now is there.
//!
//! The file is two parts. Lines that have scrolled off the screen into the
//! scrollback stop changing, so they are written once and stay. After them
//! comes the screen as it is, rewritten each time it changes. A line wrapped
//! across rows is one line, as in [`all_text`](crate::all_text). While a
//! full-screen program has the alternate screen nothing is taken, and the
//! file keeps the screen from before it.
//!
//! Which rows are new is counted while the scrollback is growing and the
//! pane keeps its size. Once the scrollback is full (old rows fall off the
//! top as new ones come), or a resize has reflowed it or pulled rows back
//! onto the screen, or it was cleared, the count says nothing, and the place
//! is found again by the last lines written: where they are nearest the
//! line count from before, since a command run twice prints the same lines
//! twice. If they are not there at all, everything is taken as new.

use std::cmp::Reverse;
use std::collections::VecDeque;
use std::io::{self, Seek, SeekFrom, Write};
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
    /// The bytes of the lines that stay; the screen comes after them.
    committed: u64,
    /// The screen as written after them.
    shown: String,
}

/// Where the log has got to.
#[derive(Default)]
pub(crate) struct Places {
    /// The last lines written, oldest first.
    tail: VecDeque<String>,
    /// The rows written, counted from the oldest in the scrollback, while
    /// that count means something (see `take`). Past the scrollback when the
    /// last line written is on the screen.
    seen: usize,
    /// The lines written, counted the same way: where to look for `tail`
    /// first.
    lines_seen: usize,
    /// The columns, screen rows and scrollback rows at the last look.
    cols: usize,
    rows: usize,
    history: usize,
    /// Nothing has been taken yet.
    fresh: bool,
}

impl WorkLog {
    /// A new log at `path` (created, or emptied when it is there).
    pub(crate) fn create(path: &Path) -> io::Result<Self> {
        let file = std::fs::File::create(path)?;
        let places = Places { fresh: true, ..Default::default() };
        Ok(Self { file, path: path.to_owned(), places, last: None, committed: 0, shown: String::new() })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Bring the file up to the pane, when it is time to look again.
    pub(crate) fn due<T: EventListener>(&mut self, term: &Term<T>, cap: usize) {
        if self.last.is_some_and(|t| t.elapsed() < EVERY) {
            return;
        }
        self.last = Some(Instant::now());
        self.finish(term, cap);
    }

    /// Bring the file up to the pane now: the last look before the log stops.
    pub(crate) fn finish<T: EventListener>(&mut self, term: &Term<T>, cap: usize) {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let new = self.places.take(term, cap);
        let screen = joined(&self.places.screen(term));
        if new.is_empty() && screen == self.shown {
            return;
        }
        let _ = self.put(&joined(&new), screen);
    }

    /// The lines that stay after those already there, then the screen in
    /// place of the one before.
    fn put(&mut self, new: &str, screen: String) -> io::Result<()> {
        self.file.set_len(self.committed)?;
        self.file.seek(SeekFrom::Start(self.committed))?;
        self.file.write_all(new.as_bytes())?;
        self.committed += new.len() as u64;
        self.file.write_all(screen.as_bytes())?;
        self.shown = screen;
        Ok(())
    }
}

/// Lines as a file holds them, each ended by a newline.
fn joined(lines: &[String]) -> String {
    let mut text = String::new();
    for l in lines {
        text.push_str(l);
        text.push('\n');
    }
    text
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
        let (cols, rows) = (grid.columns(), grid.screen_lines());
        // With the same size and a scrollback that only grew, a row keeps its
        // count from the oldest. A clear and as much output again within one
        // look would fool that, so the last line written must be where the
        // count says.
        let counted = !self.fresh
            && cols == self.cols
            && rows == self.rows
            && h >= self.history
            && h < cap
            && self.still_there(term, h);
        let new = if counted {
            let new = lines(term, self.seen as i32 - h as i32, -1);
            self.lines_seen += new.len();
            if let Some((_, end)) = new.last() {
                self.seen = (end + h as i32 + 1) as usize;
            }
            new
        } else {
            let (after, from, anchor) = self.find(term, h, cap);
            let new: Vec<(String, i32)> = after.into_iter().take_while(|(_, end)| *end < 0).collect();
            self.lines_seen = from + new.len();
            let end = new.last().map(|(_, end)| *end).or(anchor);
            self.seen = end.map_or(0, |end| (end + h as i32 + 1) as usize);
            new
        };
        self.fresh = false;
        (self.cols, self.rows, self.history) = (cols, rows, h);
        let out: Vec<String> = new.into_iter().map(|(s, _)| s).collect();
        for l in &out {
            self.tail.push_back(l.clone());
            if self.tail.len() > TAIL {
                self.tail.pop_front();
            }
        }
        out
    }

    /// Whether the line that ends where the count says the written ones end
    /// is the last one written.
    fn still_there<T: EventListener>(&self, term: &Term<T>, h: usize) -> bool {
        let Some(want) = self.tail.back() else { return true };
        let (top, end) = (-(h as i32), self.seen as i32 - h as i32 - 1);
        if end < top {
            return false;
        }
        let mut start = end;
        while start > top && wraps(term, start - 1) {
            start -= 1;
        }
        let mut line = String::new();
        for l in start..=end {
            row_text(term, l, &mut line);
        }
        line.trim_end() == want
    }

    /// The lines after the last ones written, when counting cannot say, with
    /// the index of the first of them and the row the last written one ends
    /// on; everything, and no row, when they are not there. Rows on the
    /// screen are looked at too, for a taller pane pulls rows back onto it.
    fn find<T: EventListener>(&self, term: &Term<T>, h: usize, cap: usize) -> (Vec<(String, i32)>, usize, Option<i32>) {
        let bottom = term.grid().screen_lines() as i32 - 1;
        let n = self.tail.len();
        let all = || lines(term, -(h as i32), bottom);
        if n == 0 {
            return (all(), 0, None);
        }
        let ends_tail = |got: &[(String, i32)], i: usize| got[i - n..i].iter().map(|(s, _)| s).eq(self.tail.iter());
        let cut = |mut got: Vec<(String, i32)>, at: Option<usize>| match at {
            Some(i) => {
                let anchor = got[i - 1].1;
                (got.split_off(i), i, Some(anchor))
            }
            None => (got, 0, None),
        };
        if h < cap {
            // Nothing has fallen off the top, so the lines are counted from
            // the same one as before.
            let got = all();
            let at = (n..=got.len())
                .filter(|&i| ends_tail(&got, i))
                .min_by_key(|&i| (i.abs_diff(self.lines_seen), Reverse(i)));
            return cut(got, at);
        }
        // Full: the count is off by however many lines fell off, so look
        // from the newest back in ever larger pieces.
        let mut span = 256;
        loop {
            let whole = span >= h;
            let got = if whole { all() } else { lines(term, -(span as i32), bottom) };
            // The first line of a piece may be the end of a longer one, so a
            // match must not start on it unless the piece is everything.
            let first = usize::from(!whole);
            let at = (first + n..=got.len()).rev().find(|&i| ends_tail(&got, i));
            if at.is_some() || whole {
                return cut(got, at);
            }
            span *= 4;
        }
    }

    /// The screen's lines from where the written ones end, the blank ones
    /// at the bottom left out.
    fn screen<T: EventListener>(&self, term: &Term<T>) -> Vec<String> {
        let grid = term.grid();
        let h = grid.history_size() as i32;
        let from = self.seen as i32 - h;
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

    /// A command run twice prints the same lines twice; after a resize the
    /// place is the one nearest where it was, not the second run.
    #[test]
    fn the_same_output_twice_and_a_resize_skip_nothing() {
        let mut t = term(20, 5);
        let mut p = Places { fresh: true, ..Default::default() };
        let run = |t: &mut _| {
            feed(t, "$ ll\r\n");
            for i in 0..10 {
                feed(t, &format!("file {i}\r\n"));
            }
        };
        run(&mut t);
        let mut got = p.take(&t, CAP);
        run(&mut t);
        feed(&mut t, "$ ");
        t.resize(crate::Size::new(15, 5));
        got.extend(p.take(&t, CAP));
        got.extend(p.screen(&t));
        let mut want = Vec::new();
        for _ in 0..2 {
            want.push("$ ll".to_owned());
            want.extend((0..10).map(|i| format!("file {i}")));
        }
        want.push("$".into());
        assert_eq!(got, want);
    }

    /// A taller pane pulls written rows back onto the screen; they are not
    /// written again.
    #[test]
    fn a_taller_pane_does_not_write_lines_again() {
        let mut t = term(20, 3);
        let mut p = Places { fresh: true, ..Default::default() };
        for i in 0..10 {
            feed(&mut t, &format!("line {i}\r\n"));
        }
        let mut got = p.take(&t, CAP);
        t.resize(crate::Size::new(20, 8));
        assert!(p.take(&t, CAP).is_empty(), "nothing new");
        assert_eq!(p.screen(&t), ["line 8", "line 9"]);
        for i in 10..20 {
            feed(&mut t, &format!("line {i}\r\n"));
        }
        got.extend(p.take(&t, CAP));
        got.extend(p.screen(&t));
        let want: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
        assert_eq!(got, want);
    }

    /// The file holds the screen while the log runs, and the lines that
    /// scroll off stay in it.
    #[test]
    fn the_file_has_everything_up_to_now() {
        let path = crate::util::test_dir("worklog").join("log.txt");
        let mut t = term(10, 3);
        let mut log = WorkLog::create(&path).unwrap();
        feed(&mut t, "one\r\ntwo");
        log.due(&t, CAP);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        feed(&mut t, "\r\nthree\r\nfour\r\n$ ");
        log.finish(&t, CAP);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\nthree\nfour\n$\n");
        feed(&mut t, "\x1b[2J\x1b[Hfive");
        log.finish(&t, CAP);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\nthree\nfour\n$\nfive\n");
    }
}
