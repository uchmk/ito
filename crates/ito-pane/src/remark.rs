//! Putting the marks back after ConPTY draws its screen again (#16).
//!
//! ConPTY keeps a buffer the size of the window and nothing above it. When
//! the window changes size it wraps that buffer again and draws all of it
//! once more, without the OSC 133 the shell first wrote it with, so the rows
//! lose the links [`PromptLinks`](crate::osc) put on them: the last output,
//! the blocks and the jumps between prompts go with them. So the marks are
//! read off the screen as the grid is resized ([`resize`]), and put back on
//! the lines that read the same once ConPTY has drawn them ([`restore`]).

use std::time::Duration;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Flags, Hyperlink};
use alacritty_terminal::term::Term;

use crate::osc::is_mark;

/// How long after a resize what ConPTY draws is still taken for its
/// drawing of the screen, once the shell has written anything at all.
pub(crate) const REPAINT: Duration = Duration::from_secs(1);

/// The screen's lines before a resize (rows the window wrapped joined
/// back), each character with the mark it carried, if any.
pub(crate) struct Kept {
    lines: Vec<Vec<(char, Option<Hyperlink>)>>,
}

/// The screen's lines: the rows the window wrapped joined back, each
/// character with its link and where it is. The first starts at the top of
/// the screen, even when it goes on from a row above it.
fn lines<T: EventListener>(term: &Term<T>) -> Vec<Vec<(char, Option<Hyperlink>, Point)>> {
    let grid = term.grid();
    let mut lines = Vec::new();
    let mut line = Vec::new();
    for row in 0..grid.screen_lines() as i32 {
        for col in 0..grid.columns() {
            let point = Point::new(Line(row), Column(col));
            let cell = &grid[point];
            if !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                line.push((cell.c, cell.hyperlink(), point));
            }
        }
        if !grid[Line(row)][grid.last_column()].flags.contains(Flags::WRAPLINE) {
            lines.push(std::mem::take(&mut line));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// What a line reads, without the blanks at its end.
fn text(chars: impl Iterator<Item = char>) -> String {
    chars.collect::<String>().trim_end_matches([' ', '\0']).to_owned()
}

/// The screen's marks, to put back after the resize.
fn keep<T: EventListener>(term: &Term<T>) -> Kept {
    let lines =
        lines(term).into_iter().map(|line| line.into_iter().map(|(c, link, _)| (c, link.filter(|l| is_mark(l.uri())))).collect()).collect();
    Kept { lines }
}

/// The grid resized to `size`, and the screen's marks from before it, to
/// put back on ConPTY's drawing of the screen at the new size.
///
/// Narrower, the grid keeps the cursor on its row and moves the rows above
/// it up into the scrollback as they wrap; ConPTY moves its cursor down
/// instead while there is room below it, and draws those rows again on the
/// screen. Those in the scrollback lose their marks, or each would read as
/// a prompt of its own. (Not when the scrollback is full: then it does not
/// say how many rows went up.)
pub(crate) fn resize<T: EventListener>(term: &mut Term<T>, size: impl Dimensions) -> Kept {
    let kept = keep(term);
    let before = term.grid().history_size();
    term.resize(size);
    let grid = term.grid_mut();
    let moved = grid.history_size().saturating_sub(before);
    let below = grid.screen_lines().saturating_sub(grid.cursor.point.line.0.max(0) as usize + 1);
    for line in 1..=moved.min(below) {
        for col in 0..grid.columns() {
            let cell = &mut grid[Line(-(line as i32))][Column(col)];
            if cell.hyperlink().is_some_and(|l| is_mark(l.uri())) {
                cell.set_hyperlink(None);
            }
        }
    }
    kept
}

/// The marks put back on the screen's lines that read as a line did before
/// the resize, in the same order, and carry no mark yet: so it can be done
/// again as more of ConPTY's drawing comes. A cell with a program's own
/// link keeps it.
pub(crate) fn restore<T: EventListener>(term: &mut Term<T>, kept: &Kept) {
    if kept.lines.iter().all(|line| line.iter().all(|(_, mark)| mark.is_none())) {
        return;
    }
    let now = lines(term);
    let old: Vec<String> = kept.lines.iter().map(|line| text(line.iter().map(|&(c, _)| c))).collect();
    let new: Vec<String> = now.iter().map(|line| text(line.iter().map(|&(c, _, _)| c))).collect();
    let grid = term.grid_mut();
    for (o, n) in align(&old, &new) {
        let line = &now[n];
        if line.iter().any(|(_, link, _)| link.as_ref().is_some_and(|l| is_mark(l.uri()))) {
            continue;
        }
        for ((_, mark), (_, link, point)) in kept.lines[o].iter().zip(line) {
            if let (Some(mark), None) = (mark, link) {
                grid[*point].set_hyperlink(Some(mark.clone()));
            }
        }
    }
}

/// The lines of `old` and `new` that are the same and in the same order,
/// as many as can be (their longest common subsequence), as pairs of
/// indices. Of the ways to pair as many, the one with the latest lines:
/// ConPTY keeps the bottom of its buffer when it has too many rows. A
/// blank line is never one.
fn align(old: &[String], new: &[String]) -> Vec<(usize, usize)> {
    let same = |i: usize, j: usize| !old[i].is_empty() && old[i] == new[j];
    // `best[i][j]`: how many of `old[..i]` and `new[..j]` pair.
    let mut best = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in 1..=old.len() {
        for j in 1..=new.len() {
            best[i][j] = if same(i - 1, j - 1) { best[i - 1][j - 1] + 1 } else { best[i - 1][j].max(best[i][j - 1]) };
        }
    }
    let (mut i, mut j) = (old.len(), new.len());
    let mut pairs = Vec::new();
    while i > 0 && j > 0 {
        if same(i - 1, j - 1) {
            pairs.push((i - 1, j - 1));
            (i, j) = (i - 1, j - 1);
        } else if best[i - 1][j] >= best[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    pairs.reverse();
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{last_output, prompt_lines};
    use crate::osc::PromptLinks;
    use crate::testing;
    use crate::{Proxy, Size};

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    /// `text` written to the grid through the links, as the reader does.
    fn through(t: &mut Term<Proxy>, marks: &mut PromptLinks, text: &str) {
        let fed = marks.feed(text.as_bytes()).unwrap_or_else(|| text.as_bytes().to_vec());
        testing::feed(t, std::str::from_utf8(&fed).unwrap());
    }

    fn link(t: &Term<Proxy>, line: i32, col: usize) -> Option<String> {
        t.grid()[Line(line)][Column(col)].hyperlink().map(|l| l.uri().to_owned())
    }

    /// A prompt of two lines, then narrower: ConPTY draws the screen again
    /// without the OSC 133, over what the grid wrapped. With the marks back,
    /// the last output is the command's, without the command or the prompt.
    #[test]
    fn the_marks_come_back_after_conpty_draws_the_screen_again() {
        let mut t = testing::term(40, 8);
        let mut marks = PromptLinks::default();
        let prompt = "\x1b]133;D;0\x07\x1b]133;A\x07~/src on main\r\n> \x1b]133;B\x07";
        through(&mut t, &mut marks, &format!("{prompt}echo one; echo two\r\none\r\ntwo\r\n{prompt}"));
        assert_eq!(last_output(&t).as_deref(), Some("one\ntwo"));
        let kept = resize(&mut t, Size::new(16, 8));
        // The first prompt went up into the scrollback, and is drawn again.
        assert_eq!(t.grid().history_size(), 1);
        marks.resized();
        let rows = ["~/src on main", "> echo one; echo two", "one", "two", "~/src on main", "> "];
        let drawn: String = rows.iter().map(|row| format!("{row}\x1b[K\r\n")).collect();
        through(&mut t, &mut marks, &format!("\x1b[?25l\x1b[H{}\x1b[J\x1b[?25h", drawn.trim_end_matches("\r\n")));
        assert_ne!(last_output(&t).as_deref(), Some("one\ntwo"));
        restore(&mut t, &kept);
        assert_eq!(prompt_lines(&t), [0, 5]);
        assert_eq!(last_output(&t).as_deref(), Some("one\ntwo"));
        // Done again as more comes, it changes nothing.
        restore(&mut t, &kept);
        assert_eq!(prompt_lines(&t), [0, 5]);
        // The next command is marked as before.
        through(&mut t, &mut marks, &format!("echo three\r\nthree\r\n{prompt}"));
        assert_eq!(last_output(&t).as_deref(), Some("three"));
    }

    /// A line that reads otherwise now gets no mark, and a program's own
    /// link stays where it is.
    #[test]
    fn a_changed_line_and_a_programs_link_are_left_alone() {
        let mut t = testing::term(20, 4);
        let mut marks = PromptLinks::default();
        through(&mut t, &mut marks, "\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07a.txt\r\n\x1b]133;A\x07$ \x1b]133;B\x07");
        let kept = keep(&t);
        marks.resized();
        through(&mut t, &mut marks, "\x1b[H$ ls -l\x1b[K\r\n\x1b]8;;https://x/\x1b\\a.txt\x1b]8;;\x1b\\\x1b[K\r\n$ \x1b[K");
        assert_eq!(prompt_lines(&t), Vec::<i32>::new());
        restore(&mut t, &kept);
        assert_eq!(prompt_lines(&t), [2]);
        assert_eq!(link(&t, 0, 0), None);
        assert_eq!(link(&t, 1, 0).as_deref(), Some("https://x/"));
    }

    #[test]
    fn align_pairs_the_same_lines_in_order() {
        let old = strings(&["$ ls", "a", "b", "", "$"]);
        let new = strings(&["a", "b", "x", "", "$"]);
        assert_eq!(align(&old, &new), vec![(1, 0), (2, 1), (4, 4)]);
    }

    #[test]
    fn align_takes_the_latest_lines() {
        // Two prompts that read the same, and only one of them is left.
        let old = strings(&["$", "one", "$"]);
        let new = strings(&["$"]);
        assert_eq!(align(&old, &new), vec![(2, 0)]);
    }

    #[test]
    fn align_never_pairs_blank_lines() {
        assert_eq!(align(&strings(&["", ""]), &strings(&["", ""])), vec![]);
    }
}
