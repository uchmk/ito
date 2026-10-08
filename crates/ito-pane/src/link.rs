//! What is under the pointer when it is clicked with Ctrl (Cmd on a Mac): a
//! web address, or a file's path with the line after it (`src/main.rs:42`,
//! `C:\dev\x.rs:7:3`). Found in one row of the screen as it is drawn; the
//! app decides what opening one means.

use alacritty_terminal::term::cell::Flags;

use crate::CellView;

/// A thing in the output that can be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// `https://…`, `http://…` or `file://…`, as written.
    Url(String),
    /// A path as written (maybe relative to the shell's folder), and the line
    /// and column after it, if any.
    Path { path: String, line: Option<u32>, column: Option<u32> },
}

/// The link at cell `col` of `row`, and the cells it covers (`start..end`).
pub fn link_at(row: &[CellView], col: usize) -> Option<(std::ops::Range<usize>, Link)> {
    // The text with the cell each character is in; the right half of a wide
    // character is no character of its own.
    let chars: Vec<(char, usize)> = row.iter().enumerate().filter(|(_, c)| !c.flags.contains(Flags::WIDE_CHAR_SPACER)).map(|(x, c)| (if c.c == '\0' { ' ' } else { c.c }, x)).collect();
    let at = chars.iter().rposition(|(_, x)| *x <= col)?;
    let text: Vec<char> = chars.iter().map(|(c, _)| *c).collect();
    let (start, end) = token(&text, at)?;
    let word: String = text[start..end].iter().collect();
    let (skip, len, link) = parse(&word)?;
    let first = start + skip;
    let last = first + len;
    if !(first..last).contains(&at) {
        return None;
    }
    let cells = chars[first].1..chars[last - 1].1 + 1;
    // A wide character at the end covers its spacer too.
    let cells = match row.get(cells.end).is_some_and(|c| c.flags.contains(Flags::WIDE_CHAR_SPACER)) {
        true => cells.start..cells.end + 1,
        false => cells,
    };
    Some((cells, link))
}

/// What ends a word a link can be in: spaces, quotes, brackets and the
/// lines boxes are drawn with.
fn stops(c: char) -> bool {
    c.is_whitespace() || "\"'`<>()[]{}|│┃║".contains(c) || ('\u{2500}'..='\u{257f}').contains(&c)
}

/// The word around `at`, if `at` is in one.
fn token(text: &[char], at: usize) -> Option<(usize, usize)> {
    if stops(*text.get(at)?) {
        return None;
    }
    let start = text[..at].iter().rposition(|c| stops(*c)).map_or(0, |k| k + 1);
    let end = text[at..].iter().position(|c| stops(*c)).map_or(text.len(), |k| at + k);
    Some((start, end))
}

/// The link in a word: where it starts in it, how many characters, and what.
fn parse(word: &str) -> Option<(usize, usize, Link)> {
    let chars: Vec<char> = word.chars().collect();
    let lower = word.to_ascii_lowercase();
    for scheme in ["https://", "http://", "file://"] {
        if let Some(byte) = lower.find(scheme) {
            let skip = word[..byte].chars().count();
            let rest: Vec<char> = chars[skip..].to_vec();
            let len = trim_end(&rest);
            if len <= scheme.len() {
                return None;
            }
            return Some((skip, len, Link::Url(rest[..len].iter().collect())));
        }
    }
    let len = trim_end(&chars);
    let body: String = chars[..len].iter().collect();
    let (path, line, column) = split_line(&body);
    looks_like_path(path).then(|| (0, len, Link::Path { path: path.to_owned(), line, column }))
}

/// How much of a word is left once the sentence's punctuation after it is
/// off: `see src/x.rs.` is `src/x.rs`.
fn trim_end(chars: &[char]) -> usize {
    let mut len = chars.len();
    while len > 0 && ".,;:!?'\"".contains(chars[len - 1]) {
        len -= 1;
    }
    len
}

/// `path:line:column`, `path:line`, `path(line,column)` or the path alone.
fn split_line(body: &str) -> (&str, Option<u32>, Option<u32>) {
    let num = |s: &str| (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).then(|| s.parse::<u32>().ok()).flatten();
    // MSBuild's and rustc's on Windows: `file(12,5)`; the brackets are
    // where a word stops, so only `file(12` can reach here.
    if let Some((p, rest)) = body.rsplit_once(':') {
        if let Some(n) = num(rest) {
            if let Some((p2, line)) = p.rsplit_once(':') {
                if let Some(l) = num(line) {
                    return (p2, Some(l), Some(n));
                }
            }
            return (p, Some(n), None);
        }
    }
    (body, None, None)
}

/// A path someone would want to open: with a folder in it, or a name with
/// an extension starting with a letter (`main.rs`, not `1.5`, `v0.53.8` or
/// `e.g`).
fn looks_like_path(p: &str) -> bool {
    if p.is_empty() || p.contains("://") || p.chars().all(|c| "./\\~".contains(c)) {
        return false;
    }
    if p.contains('/') || p.contains('\\') {
        // Something besides separators and dots, and not a fraction or a
        // date (`1/2`, `2026/10/08`).
        return p.chars().any(|c| c.is_alphabetic());
    }
    match p.rsplit_once('.') {
        Some((stem, ext)) => {
            // Two letters at least: not `e.g` or `i.e`.
            stem.chars().count() >= 2
                && stem.chars().any(|c| c.is_alphabetic())
                && !stem.starts_with(|c: char| c.is_ascii_digit() || c == 'v' && stem[1..].starts_with(|d: char| d.is_ascii_digit()))
                && (1..=8).contains(&ext.len())
                && ext.starts_with(|c: char| c.is_ascii_alphabetic())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::{Color, NamedColor};

    fn row(text: &str) -> Vec<CellView> {
        let cell = |c| CellView { c, fg: Color::Named(NamedColor::Foreground), bg: Color::Named(NamedColor::Background), flags: Flags::empty(), selected: false };
        let mut out = Vec::new();
        for c in text.chars() {
            if unicode_width(c) {
                out.push(CellView { flags: Flags::WIDE_CHAR, ..cell(c) });
                out.push(CellView { flags: Flags::WIDE_CHAR_SPACER, ..cell(' ') });
            } else {
                out.push(cell(c));
            }
        }
        out
    }

    fn unicode_width(c: char) -> bool {
        ('\u{3000}'..='\u{9fff}').contains(&c)
    }

    fn at(text: &str, col: usize) -> Option<(std::ops::Range<usize>, Link)> {
        link_at(&row(text), col)
    }

    fn path(p: &str, line: Option<u32>, column: Option<u32>) -> Link {
        Link::Path { path: p.into(), line, column }
    }

    #[test]
    fn web_addresses() {
        let text = "PR: https://github.com/uchmk/tsumugi/pull/12. done";
        assert_eq!(at(text, 10), Some((4..44, Link::Url("https://github.com/uchmk/tsumugi/pull/12".into()))));
        assert_eq!(at(text, 4).map(|x| x.1), Some(Link::Url("https://github.com/uchmk/tsumugi/pull/12".into())));
        assert_eq!(at(text, 47), None, "a word that is no link");
        assert_eq!(at(text, 3), None, "a space");
        // In brackets, and after a word joined to it.
        assert_eq!(at("(see http://localhost:3000)", 10).map(|x| x.1), Some(Link::Url("http://localhost:3000".into())));
        assert_eq!(at("url=https://x.dev/a", 12), Some((4..19, Link::Url("https://x.dev/a".into()))));
        assert_eq!(at("url=https://x.dev/a", 1), None, "before the address");
        assert_eq!(at("https://", 3), None);
    }

    #[test]
    fn paths_and_lines() {
        assert_eq!(at("error at src/main.rs:42:7: oops", 12), Some((9..25, path("src/main.rs", Some(42), Some(7)))));
        assert_eq!(at("  --> crates/tsumugi/src/keys.rs:301", 10).map(|x| x.1), Some(path("crates/tsumugi/src/keys.rs", Some(301), None)));
        assert_eq!(at(r"C:\dev\filer\src\app.rs:12 warning", 3).map(|x| x.1), Some(path(r"C:\dev\filer\src\app.rs", Some(12), None)));
        assert_eq!(at("Edited README.md.", 9).map(|x| x.1), Some(path("README.md", None, None)));
        assert_eq!(at("see ~/notes/todo", 6).map(|x| x.1), Some(path("~/notes/todo", None, None)));
        assert_eq!(at("│ src/a.rs │", 3).map(|x| x.1), Some(path("src/a.rs", None, None)), "inside a drawn box");
    }

    #[test]
    fn what_is_not_a_path() {
        for text in ["version 1.5 out", "v0.53.8", "1/2", "2026/10/08", "...", "e.g.", "a:b"] {
            for col in 0..text.len() {
                assert!(at(text, col).is_none_or(|(_, l)| !matches!(l, Link::Path { .. })), "{text} at {col}: {:?}", at(text, col));
            }
        }
    }

    #[test]
    fn wide_characters_count_two_cells() {
        // 日本 is four cells: the link starts at the sixth.
        let text = "日本 src/x.rs:3";
        assert_eq!(at(text, 7), Some((5..15, path("src/x.rs", Some(3), None))));
        assert_eq!(at(text, 1), None);
        // A path with a wide character in it covers its last spacer.
        assert_eq!(at("docs/仕様.md", 2).map(|x| x.0), Some(0..12));
    }
}
