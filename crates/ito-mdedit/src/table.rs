use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use crate::list::code_lines;
use crate::{line_of, lines, Change};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Next,
    Prev,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    None,
    Left,
    Center,
    Right,
}

struct Table {
    /// Byte range of the whole table (first line start to last line end).
    range: Range<usize>,
    /// The line index of the first row in `lines`.
    first: usize,
    rows: Vec<Vec<String>>,
    /// What each line starts with before its first `|`: the indent and the `>` of a quote, kept as it was.
    prefixes: Vec<String>,
    /// The line break the first line of the table ends with; the whole table is written with it.
    eol: String,
    aligns: Vec<Align>,
    /// Row 1 is the delimiter row; the cells of `rows[1]` are not used.
    cols: usize,
}

/// The byte length of what a line starts with before its row: blanks and the `>` of a block quote.
fn prefix_len(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t', '>']).len()
}

/// The cells of the row on the line `r` of `text`, without the line's prefix.
fn row_cells(text: &str, r: &Range<usize>) -> Vec<String> {
    split_row(&text[r.start + prefix_len(&text[r.clone()])..r.end])
}

/// Split a row into trimmed cells at the `|` that is not escaped and not inside a code span.
fn split_row(line: &str) -> Vec<String> {
    let s = line.trim();
    let s = s.strip_prefix('|').unwrap_or(s);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let (mut esc, mut ticks) = (false, 0usize);
    let mut run = 0usize;
    for c in s.chars() {
        if c == '`' {
            run += 1;
        } else {
            if run > 0 {
                ticks = if ticks == 0 {
                    run
                } else if ticks == run {
                    0
                } else {
                    ticks
                };
                run = 0;
            }
        }
        if esc {
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '|' && ticks == 0 {
            cells.push(cur.trim().to_string());
            cur.clear();
            continue;
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() || cells.is_empty() {
        cells.push(cur.trim().to_string());
    }
    cells
}

fn delimiter(cells: &[String]) -> Option<Vec<Align>> {
    cells
        .iter()
        .map(|c| {
            let l = c.starts_with(':');
            let r = c.ends_with(':') && c.len() > 1;
            let core = c.trim_matches(':');
            if core.is_empty() && c.len() > 1 || !core.is_empty() && core.chars().all(|ch| ch == '-') {
                Some(match (l, r) {
                    (true, true) => Align::Center,
                    (true, false) => Align::Left,
                    (false, true) => Align::Right,
                    _ => Align::None,
                })
            } else {
                None
            }
        })
        .collect()
}

fn find(text: &str, byte: usize) -> Option<(Table, usize)> {
    let ls = lines(text);
    let at = line_of(&ls, byte.min(text.len()))?;
    // Table-like text in a code block (fenced or indented) is code, not a table.
    let code = code_lines(text, &ls);
    let is_row = |k: usize| !code[k] && text[ls[k].clone()].contains('|');
    if !is_row(at) {
        return None;
    }
    let (mut s, mut e) = (at, at);
    while s > 0 && is_row(s - 1) {
        s -= 1;
    }
    while e + 1 < ls.len() && is_row(e + 1) {
        e += 1;
    }
    // The table starts at the header whose next line is the delimiter row.
    let start = (s..e).find(|&k| delimiter(&row_cells(text, &ls[k + 1])).is_some() && text[ls[k + 1].clone()].contains('-'))?;
    if at < start {
        return None;
    }
    let aligns = delimiter(&row_cells(text, &ls[start + 1]))?;
    let rows: Vec<Vec<String>> = (start..=e).map(|k| row_cells(text, &ls[k])).collect();
    let prefixes: Vec<String> =
        (start..=e).map(|k| text[ls[k].start..ls[k].start + prefix_len(&text[ls[k].clone()])].to_string()).collect();
    let eol = text[ls[start].end..ls[start + 1].start].to_string();
    let cols = rows[0].len().max(aligns.len());
    Some((Table { range: ls[start].start..ls[e].end, first: start, rows, prefixes, eol, aligns, cols }, at - start))
}

fn pad(cell: &str, width: usize, align: Align) -> String {
    let gap = width.saturating_sub(cell.width());
    match align {
        Align::Right => format!("{}{cell}", " ".repeat(gap)),
        Align::Center => format!("{}{cell}{}", " ".repeat(gap / 2), " ".repeat(gap - gap / 2)),
        _ => format!("{cell}{}", " ".repeat(gap)),
    }
}

/// The formatted table and, for each row and column, where its cell's text starts and ends in the new text
/// (relative to the table's start).
fn render(t: &Table) -> (String, Vec<Vec<Range<usize>>>) {
    let widths: Vec<usize> = (0..t.cols)
        .map(|c| {
            let w = t.rows.iter().enumerate().filter(|(i, _)| *i != 1).map(|(_, r)| r.get(c).map_or(0, |x| x.width())).max().unwrap_or(0);
            w.max(3)
        })
        .collect();
    let mut out = String::new();
    let mut spans = Vec::new();
    for (i, row) in t.rows.iter().enumerate() {
        if i > 0 {
            out.push_str(&t.eol);
        }
        out.push_str(t.prefixes.get(i).or(t.prefixes.last()).map_or("", String::as_str));
        out.push('|');
        let mut rs = Vec::new();
        for (c, &w) in widths.iter().enumerate() {
            out.push(' ');
            let align = t.aligns.get(c).copied().unwrap_or(Align::None);
            let start = out.len();
            if i == 1 {
                let (l, r) = (matches!(align, Align::Left | Align::Center), matches!(align, Align::Right | Align::Center));
                let dashes = w - usize::from(l) - usize::from(r);
                out.push_str(&format!("{}{}{}", if l { ":" } else { "" }, "-".repeat(dashes), if r { ":" } else { "" }));
            } else {
                let cell = row.get(c).map_or("", String::as_str);
                out.push_str(&pad(cell, w, align));
            }
            rs.push(start..out.len());
            out.push_str(" |");
        }
        spans.push(rs);
    }
    (out, spans)
}

/// Line up the table holding `byte`: every column as wide as its widest cell (CJK counts two columns), the delimiter
/// row rewritten from its alignment colons. The caret stays in its cell. `None` when `byte` is in no table or it is
/// already tidy.
pub fn format_table(text: &str, byte: usize) -> Option<Change> {
    let byte = byte.min(text.len());
    let (t, row) = find(text, byte)?;
    let (new, spans) = render(&t);
    if new == text[t.range.clone()] {
        return None;
    }
    let col = cell_at(text, &t, row, byte);
    let cursor = t.range.start + spans[row][col].start;
    Some(Change { range: t.range, text: new, cursor })
}

/// The column of `byte` in the row at `row` of the table.
fn cell_at(text: &str, t: &Table, row: usize, byte: usize) -> usize {
    let ls = lines(text);
    let line = &text[ls[t.first + row].clone()];
    let rel = byte.saturating_sub(ls[t.first + row].start).min(line.len());
    let mut col = 0;
    let (mut esc, mut ticks, mut run) = (false, 0usize, 0usize);
    let p = prefix_len(line);
    let skip = p + usize::from(line[p..].starts_with('|'));
    for (i, c) in line.char_indices() {
        if i >= rel {
            break;
        }
        if i < skip {
            continue;
        }
        if c == '`' {
            run += 1;
        } else if run > 0 {
            ticks = if ticks == 0 {
                run
            } else if ticks == run {
                0
            } else {
                ticks
            };
            run = 0;
        }
        if esc {
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '|' && ticks == 0 {
            col += 1;
        }
    }
    col.min(t.cols.saturating_sub(1))
}

/// Move to the next or previous cell of the table holding `byte`, formatting the table on the way (the caret lands
/// at the start of the cell's text). Past the last cell, `Next` adds an empty row. `None` outside a table or at the
/// first cell going back.
pub fn next_cell(text: &str, byte: usize, step: Step) -> Option<Change> {
    let byte = byte.min(text.len());
    let (mut t, row) = find(text, byte)?;
    let col = cell_at(text, &t, row, byte);
    let (mut r, mut c) = (row, col);
    match step {
        Step::Next => {
            c += 1;
            if c >= t.cols {
                c = 0;
                r += 1;
                if r == 1 {
                    r = 2;
                }
            }
            if r >= t.rows.len() {
                t.rows.push(vec![String::new(); t.cols]);
                t.prefixes.push(t.prefixes.last().cloned().unwrap_or_default());
            }
        }
        Step::Prev => {
            if c == 0 {
                if r <= 2 && r != 0 {
                    r = 0;
                } else if r == 0 {
                    return None;
                } else {
                    r -= 1;
                }
                c = t.cols - 1;
            } else {
                c -= 1;
            }
        }
    }
    let (new, spans) = render(&t);
    let range = t.range.clone();
    let cursor = range.start + spans[r][c].start;
    Some(Change { range, text: new, cursor })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_line_up_with_wide_characters() {
        let t = "|名前|n|\n|:-|-:|\n|a|10|\n|日本語|2|\n";
        let c = format_table(t, 0).unwrap();
        assert_eq!(c.apply(t), "| 名前   |   n |\n| :----- | --: |\n| a      |  10 |\n| 日本語 |   2 |\n");
        assert!(format_table(&c.apply(t), 0).is_none());
        assert!(format_table("plain text\n", 0).is_none());
        assert!(format_table("a | b\nno delimiter\n", 0).is_none());
    }

    #[test]
    fn the_caret_stays_in_its_cell() {
        let t = "| a | b |\n|---|---|\n| long cell | x |\n";
        let at = t.rfind('x').unwrap();
        let c = format_table(t, at).unwrap();
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'x');
    }

    #[test]
    fn an_escaped_bar_and_a_code_span_do_not_split() {
        assert_eq!(split_row("| a \\| b | `c|d` | e |"), ["a \\| b", "`c|d`", "e"]);
    }

    #[test]
    fn tab_moves_through_cells_and_grows_the_table() {
        let t = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        let c = next_cell(t, 2, Step::Next).unwrap();
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'b');
        let at = t.find('2').unwrap();
        let c = next_cell(t, at, Step::Next).unwrap();
        let n = c.apply(t);
        assert_eq!(n, "| a   | b   |\n| --- | --- |\n| 1   | 2   |\n|     |     |\n");
        assert_eq!(c.cursor, n[..n.len() - 1].rfind('\n').unwrap() + 3);
        let c = next_cell(t, 2, Step::Prev);
        assert!(c.is_none());
        let at = t.find('1').unwrap();
        let c = next_cell(t, at, Step::Prev).unwrap();
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'b');
    }

    #[test]
    fn a_table_keeps_its_indent_in_a_list_item_and_a_quote() {
        let t = "- item\n  | a | b |\n  | --- | --- |\n  | c | d |\n";
        let want = "- item\n  | a   | b   |\n  | --- | --- |\n  | c   | d   |\n";
        let c = format_table(t, t.find('a').unwrap()).unwrap();
        assert_eq!(c.apply(t), want);
        let c = next_cell(t, t.find('a').unwrap(), Step::Next).unwrap();
        assert_eq!(c.apply(t), want);
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'b');
        // A new row past the last cell has the same indent.
        let c = next_cell(t, t.find('d').unwrap(), Step::Next).unwrap();
        let n = c.apply(t);
        assert_eq!(n, "- item\n  | a   | b   |\n  | --- | --- |\n  | c   | d   |\n  |     |     |\n");
        assert_eq!(c.cursor, n.rfind("\n  |     |").unwrap() + 5);
        // Going back works with the indent too.
        let c = next_cell(t, t.find('c').unwrap(), Step::Prev).unwrap();
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'b');

        let q = "> | a | b |\n> |---|---|\n> | c | d |\n";
        let c = format_table(q, 3).unwrap();
        assert_eq!(c.apply(q), "> | a   | b   |\n> | --- | --- |\n> | c   | d   |\n");
        let c = next_cell(q, q.find('d').unwrap(), Step::Next).unwrap();
        assert_eq!(c.apply(q), "> | a   | b   |\n> | --- | --- |\n> | c   | d   |\n> |     |     |\n");
        let c = next_cell(q, 3, Step::Next).unwrap();
        assert_eq!(c.apply(q).as_bytes()[c.cursor], b'b');
    }

    #[test]
    fn table_like_text_in_code_is_left_alone() {
        for eol in [
            "
", "
",
        ] {
            let table = format!("| a | b |{eol}|---|---|{eol}| 1 | 2 |{eol}");
            let real = format!("| x | y |{eol}|---|---|{eol}");
            for (open, pad) in [("```", ""), ("~~~", ""), ("", "    ")] {
                let code = if open.is_empty() {
                    table.lines().map(|l| format!("{pad}{l}{eol}")).collect::<String>()
                } else {
                    format!("{open}{eol}{table}{open}{eol}")
                };
                let t = format!("para{eol}{eol}{code}");
                for at in t.find("| a").unwrap()..t.len() {
                    assert!(format_table(&t, at).is_none(), "{t:?} {at}");
                    assert!(next_cell(&t, at, Step::Next).is_none(), "{t:?} {at}");
                    assert!(next_cell(&t, at, Step::Prev).is_none(), "{t:?} {at}");
                }
                // A real table right after the code still works.
                let sep = if open.is_empty() { format!("{eol}text{eol}{eol}") } else { String::new() };
                let t = format!("{code}{sep}{real}");
                let at = t.find("| x").unwrap() + 2;
                let c = format_table(&t, at).unwrap();
                assert!(c.apply(&t).ends_with(&format!("| x   | y   |{eol}| --- | --- |{eol}")), "{:?}", c.apply(&t));
                assert!(next_cell(&t, at, Step::Next).is_some());
            }
        }
    }

    #[test]
    fn crlf_stays_crlf() {
        let t = "| a | b |\r\n|---|---|\r\n| 1 | 2 |\r\n";
        let c = next_cell(t, 2, Step::Next).unwrap();
        assert_eq!(c.apply(t), "| a   | b   |\r\n| --- | --- |\r\n| 1   | 2   |\r\n");
        let c = next_cell(t, t.find('2').unwrap(), Step::Next).unwrap();
        assert_eq!(c.apply(t), "| a   | b   |\r\n| --- | --- |\r\n| 1   | 2   |\r\n|     |     |\r\n");
        // In a mixed text the first line of the table decides.
        let m = "| a | b |\n|---|---|\r\n| 1 | 2 |\n";
        assert_eq!(format_table(m, 2).unwrap().apply(m), "| a   | b   |\n| --- | --- |\n| 1   | 2   |\n");
        let m = "| a | b |\r\n|---|---|\n| 1 | 2 |\n";
        assert_eq!(format_table(m, 2).unwrap().apply(m), "| a   | b   |\r\n| --- | --- |\r\n| 1   | 2   |\n");
        // CRLF inside a list item.
        let l = "- x\r\n  | a | b |\r\n  |---|---|\r\n";
        assert_eq!(format_table(l, 8).unwrap().apply(l), "- x\r\n  | a   | b   |\r\n  | --- | --- |\r\n");
    }
}
