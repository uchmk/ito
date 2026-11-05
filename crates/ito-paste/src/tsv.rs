//! Tab-separated text (a spreadsheet copied as plain text) -> a GFM table.

use crate::convert::escape;

/// A GFM table for tab-separated text, or `None` when the text is not a table: no tab at all, or
/// a line with a single cell that is not empty. The first line is the header. A cell in quotes
/// (the way a spreadsheet writes a cell holding a tab or a line break) is read as one cell.
pub fn tsv_to_table(text: &str) -> Option<String> {
    let mut rows = parse(text);
    while rows.last().is_some_and(|r| r.iter().all(String::is_empty)) {
        rows.pop();
    }
    let cols = rows.iter().map(Vec::len).max()?;
    if cols < 2 || rows.iter().any(|r| r.len() < 2 && !r.iter().all(String::is_empty)) {
        return None;
    }
    let mut lines = Vec::with_capacity(rows.len() + 1);
    for (i, row) in rows.iter().enumerate() {
        let mut cells: Vec<String> = row.iter().map(|c| cell(c)).collect();
        cells.resize(cols, String::new());
        lines.push(format!("| {} |", cells.join(" | ")));
        if i == 0 {
            lines.push(format!("| {} |", vec!["---"; cols].join(" | ")));
        }
    }
    Some(lines.join("\n"))
}

fn cell(c: &str) -> String {
    let lines: Vec<String> = c.split('\n').map(|l| escape(l.trim()).replace('|', "\\|")).collect();
    lines.join("<br>").trim_matches(|ch| ch == ' ').to_string()
}

fn parse(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut at_start = true;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if at_start => {
                loop {
                    match chars.next() {
                        Some('"') if chars.peek() == Some(&'"') => {
                            chars.next();
                            field.push('"');
                        }
                        Some('"') | None => break,
                        Some(ch) => field.push(ch),
                    }
                }
                at_start = false;
            }
            '\t' => {
                row.push(std::mem::take(&mut field));
                at_start = true;
            }
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                at_start = true;
            }
            _ => {
                field.push(c);
                at_start = false;
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreadsheet_rows_become_a_table() {
        assert_eq!(
            tsv_to_table("name\tqty\r\napple\t3\r\npear\t\r\n").unwrap(),
            "| name | qty |\n| --- | --- |\n| apple | 3 |\n| pear |  |"
        );
    }

    #[test]
    fn prose_is_not_a_table() {
        assert_eq!(tsv_to_table("just a line"), None);
        assert_eq!(tsv_to_table("a\tb\nno tab here"), None);
        assert_eq!(tsv_to_table(""), None);
    }

    #[test]
    fn quoted_cells_hold_tabs_quotes_and_breaks() {
        assert_eq!(
            tsv_to_table("a\t\"x\ty\"\n\"l1\nl2\"\t\"say \"\"hi\"\"\"").unwrap(),
            "| a | x\ty |\n| --- | --- |\n| l1<br>l2 | say \"hi\" |"
        );
    }

    #[test]
    fn pipes_and_marks_in_cells_are_escaped() {
        assert_eq!(tsv_to_table("a|b\t*c*").unwrap(), "| a\\|b | \\*c\\* |\n| --- | --- |");
    }

    #[test]
    fn blank_row_inside_is_kept_as_empty_cells() {
        assert_eq!(tsv_to_table("a\tb\n\nc\td").unwrap(), "| a | b |\n| --- | --- |\n|  |  |\n| c | d |");
    }
}
