//! A table handed over to people who live in Excel: [`Table::csv`] (UTF-8 with a BOM, CRLF, RFC 4180) and
//! [`Table::xlsx`] (one sheet, bold frozen header row, columns fitted, dates as real dates). Pure Rust; Excel is not needed.
//!
//! ```
//! use ito_table::{Stamp, Table};
//! let mut t = Table::new(&["date", "job", "seconds"]);
//! t.row([Stamp::date(2026, 11, 1).into(), "closing".into(), 12.5.into()]);
//! assert!(t.csv_string().starts_with('\u{feff}'));
//! ```
//!
//! - **Typed cells**: [`Cell::Text`], [`Cell::Number`], [`Cell::Stamp`]. A number stays a number and a date a date in
//!   `.xlsx` (sortable, filterable); in CSV they are `12.5` and `2026-11-01 02:00:00`.
//! - **CSV and formulas**: a text cell that starts with `=`, `+`, `@`, a tab or a CR (or `-` followed by more text) would be run
//!   as a formula when Excel opens the CSV, so it gets a leading `'`. `.xlsx` text is always a string and is written as given.
//! - **Writing a file** goes beside it first and is renamed over, so a reader never sees half a file. It blocks: call it from a
//!   worker thread.

use std::fs;
use std::io;
use std::path::Path;

use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, XlsxError};

/// A date, or a date and a time (local, no zone), for a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// Whether the time of day counts; a date alone shows no time.
    pub time: bool,
}

impl Stamp {
    pub fn date(year: u16, month: u8, day: u8) -> Self {
        Self { year, month, day, hour: 0, minute: 0, second: 0, time: false }
    }

    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Self {
        Self { year, month, day, hour, minute, second, time: true }
    }

    fn text(&self) -> String {
        if self.time {
            format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", self.year, self.month, self.day, self.hour, self.minute, self.second)
        } else {
            format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
        }
    }

    fn excel(&self) -> Result<ExcelDateTime, XlsxError> {
        let date = ExcelDateTime::from_ymd(self.year, self.month, self.day)?;
        if self.time {
            date.and_hms(u16::from(self.hour), self.minute, self.second)
        } else {
            Ok(date)
        }
    }
}

/// One cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Text(String),
    Number(f64),
    Stamp(Stamp),
}

impl From<&str> for Cell {
    fn from(s: &str) -> Self {
        Cell::Text(s.to_owned())
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Self {
        Cell::Text(s)
    }
}

impl From<f64> for Cell {
    fn from(n: f64) -> Self {
        Cell::Number(n)
    }
}

impl From<Stamp> for Cell {
    fn from(s: Stamp) -> Self {
        Cell::Stamp(s)
    }
}

macro_rules! number_from {
    ($($t:ty),*) => {$(
        impl From<$t> for Cell {
            fn from(n: $t) -> Self {
                Cell::Number(n as f64)
            }
        }
    )*};
}
number_from!(i32, i64, u32, u64, usize);

/// A header row and the rows under it.
#[derive(Clone, Debug)]
pub struct Table {
    header: Vec<String>,
    rows: Vec<Vec<Cell>>,
    sheet: String,
}

impl Table {
    pub fn new(header: &[&str]) -> Self {
        Self { header: header.iter().map(|h| h.to_string()).collect(), rows: Vec::new(), sheet: "Sheet1".into() }
    }

    /// The name of the sheet in `.xlsx` (at most 31 characters, none of `[]:*?/\`; the invalid ones are dropped).
    pub fn sheet(mut self, name: &str) -> Self {
        let name: String = name.chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect();
        self.sheet = if name.is_empty() { "Sheet1".into() } else { name };
        self
    }

    /// Add a row. A row shorter than the header is padded with empty cells; a longer one is cut.
    pub fn row<I: IntoIterator<Item = Cell>>(&mut self, cells: I) -> &mut Self {
        let mut row: Vec<Cell> = cells.into_iter().take(self.header.len()).collect();
        row.resize(self.header.len(), Cell::Text(String::new()));
        self.rows.push(row);
        self
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The CSV: BOM first (Excel reads UTF-8 only with it), CRLF after every line including the last.
    pub fn csv_string(&self) -> String {
        let mut out = String::from('\u{feff}');
        let line = |out: &mut String, fields: Vec<String>| {
            out.push_str(&fields.iter().map(|f| quote(f)).collect::<Vec<_>>().join(","));
            out.push_str("\r\n");
        };
        line(&mut out, self.header.iter().map(|h| guard(h)).collect());
        for row in &self.rows {
            line(&mut out, row.iter().map(csv_field).collect());
        }
        out
    }

    pub fn csv(&self, path: &Path) -> io::Result<()> {
        write_atomic(path, self.csv_string().as_bytes())
    }

    /// The workbook as bytes.
    pub fn xlsx_bytes(&self) -> Result<Vec<u8>, XlsxError> {
        let mut book = Workbook::new();
        let sheet = book.add_worksheet();
        sheet.set_name(&self.sheet)?;
        let bold = Format::new().set_bold();
        let day = Format::new().set_num_format("yyyy-mm-dd");
        let moment = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
        let mut width: Vec<usize> = self.header.iter().map(|h| display_width(h)).collect();
        for (c, h) in self.header.iter().enumerate() {
            sheet.write_string_with_format(0, c as u16, h, &bold)?;
        }
        for (r, row) in self.rows.iter().enumerate() {
            let r = r as u32 + 1;
            for (c, cell) in row.iter().enumerate() {
                let col = c as u16;
                let shown = match cell {
                    Cell::Text(t) => {
                        sheet.write_string(r, col, t)?;
                        display_width(t.lines().next().unwrap_or_default())
                    }
                    Cell::Number(n) => {
                        sheet.write_number(r, col, *n)?;
                        n.to_string().len()
                    }
                    Cell::Stamp(s) => {
                        sheet.write_datetime_with_format(r, col, s.excel()?, if s.time { &moment } else { &day })?;
                        s.text().len()
                    }
                };
                width[c] = width[c].max(shown);
            }
        }
        for (c, w) in width.iter().enumerate() {
            sheet.set_column_width(c as u16, (*w as f64 + 2.0).min(80.0))?;
        }
        sheet.set_freeze_panes(1, 0)?;
        book.save_to_buffer()
    }

    pub fn xlsx(&self, path: &Path) -> io::Result<()> {
        let bytes = self.xlsx_bytes().map_err(io::Error::other)?;
        write_atomic(path, &bytes)
    }
}

/// Wide (East Asian) characters take two columns of width.
fn display_width(s: &str) -> usize {
    s.chars().map(|c| if (c as u32) < 0x2e80 { 1 } else { 2 }).sum()
}

fn csv_field(cell: &Cell) -> String {
    match cell {
        Cell::Text(t) => guard(t),
        Cell::Number(n) => n.to_string(),
        Cell::Stamp(s) => s.text(),
    }
}

/// Keep Excel from running text as a formula.
fn guard(text: &str) -> String {
    let risky = match text.chars().next() {
        Some('=' | '+' | '@' | '\t' | '\r') => true,
        Some('-') => text.chars().count() > 1,
        _ => false,
    };
    if risky {
        format!("'{text}")
    } else {
        text.to_owned()
    }
}

fn quote(field: &str) -> String {
    if field.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.ito-table.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Table {
        let mut t = Table::new(&["日付", "ジョブ", "秒"]);
        t.row([Stamp::new(2026, 11, 1, 2, 0, 0).into(), "月次, \"締め\"".into(), 12.5.into()]);
        t.row(["x".into()]);
        t
    }

    #[test]
    fn csv_has_a_bom_crlf_and_rfc_4180_quoting() {
        let csv = sample().csv_string();
        assert_eq!(csv, "\u{feff}日付,ジョブ,秒\r\n2026-11-01 02:00:00,\"月次, \"\"締め\"\"\",12.5\r\nx,,\r\n");
    }

    #[test]
    fn a_short_row_is_padded_and_a_long_one_cut() {
        let mut t = Table::new(&["a", "b"]);
        t.row(["1".into(), "2".into(), "3".into()]).row(["only".into()]);
        assert_eq!(t.csv_string(), "\u{feff}a,b\r\n1,2\r\nonly,\r\n");
    }

    #[test]
    fn text_that_would_be_a_formula_is_defused_in_csv_only() {
        let mut t = Table::new(&["v"]);
        for s in ["=1+1", "+1", "@x", "-5x", "-", "plain"] {
            t.row([s.into()]);
        }
        t.row([(-5).into()]);
        assert_eq!(t.csv_string(), "\u{feff}v\r\n'=1+1\r\n'+1\r\n'@x\r\n'-5x\r\n-\r\nplain\r\n-5\r\n");
    }

    #[test]
    fn a_date_alone_has_no_time_in_csv() {
        let mut t = Table::new(&["d"]);
        t.row([Stamp::date(2026, 1, 2).into()]);
        assert!(t.csv_string().ends_with("2026-01-02\r\n"));
    }

    #[test]
    fn the_workbook_is_a_zip_with_the_sheet_and_real_dates() {
        let bytes = sample().sheet("履歴").xlsx_bytes().unwrap();
        assert_eq!(&bytes[..2], b"PK");
        assert!(bytes.windows(8).any(|w| w == b"xl/workb"), "has workbook part");
        assert!(sample().sheet("a/b[c]").sheet == "abc");
        assert_eq!(Table::new(&[]).sheet("///").sheet, "Sheet1");
    }

    #[test]
    fn files_are_written_beside_and_renamed() {
        let d = std::env::temp_dir().join(format!("ito-table-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        sample().csv(&d.join("a").join("t.csv")).unwrap();
        sample().xlsx(&d.join("a").join("t.xlsx")).unwrap();
        assert!(fs::read(d.join("a/t.csv")).unwrap().starts_with(&[0xef, 0xbb, 0xbf]));
        assert_eq!(&fs::read(d.join("a/t.xlsx")).unwrap()[..2], b"PK");
        assert_eq!(fs::read_dir(d.join("a")).unwrap().count(), 2, "no temp file left");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_impossible_date_is_an_error_not_a_panic() {
        let mut t = Table::new(&["d"]);
        t.row([Stamp::date(2026, 13, 40).into()]);
        assert!(t.xlsx_bytes().is_err());
    }
}
