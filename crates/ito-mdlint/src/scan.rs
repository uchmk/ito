//! The line scan: every line of the text classified (front matter, code, heading, list item, ...), and the inline
//! masks (code spans, link targets, URLs, tags) that prose checks skip.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    /// Indent in columns (a tab counts to the next multiple of 4).
    pub indent: usize,
    /// The leading white space as bytes, and whether it is spaces only (so a fix may rewrite it).
    pub indent_range: Range<usize>,
    pub spaces_only: bool,
    /// The marker (`-`, `*`, `+`, `12.`, `3)`).
    pub marker: Range<usize>,
    pub ordered: bool,
    /// The bullet character, or the delimiter (`.` / `)`) of an ordered marker.
    pub mch: char,
    /// Column where the text of the item starts.
    pub content: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Front,
    /// A fence line or a line inside a fence.
    Code,
    Blank,
    Atx {
        level: u8,
        marker: Range<usize>,
        text: Range<usize>,
    },
    Setext {
        level: u8,
        text: Range<usize>,
    },
    SetextUnderline,
    Hr,
    Item(Item),
    Para,
}

pub(crate) struct Line {
    /// Without the line break.
    pub range: Range<usize>,
    pub class: Class,
}

pub(crate) struct Scan {
    pub lines: Vec<Line>,
    /// `"\r\n"` when the text has one, else `"\n"`.
    pub eol: &'static str,
    /// The opening fence run of a fence that is never closed.
    pub unclosed: Option<Range<usize>>,
}

/// Columns and bytes of the leading white space.
pub(crate) fn leading(s: &str) -> (usize, usize, bool) {
    let mut cols = 0;
    let mut bytes = 0;
    let mut spaces_only = true;
    for b in s.bytes() {
        match b {
            b' ' => cols += 1,
            b'\t' => {
                cols = (cols / 4 + 1) * 4;
                spaces_only = false;
            }
            _ => break,
        }
        bytes += 1;
    }
    (cols, bytes, spaces_only)
}

pub(crate) fn scan(text: &str) -> Scan {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut pos = 0;
    let mut crlf = false;
    for raw in text.split_inclusive('\n') {
        let next = pos + raw.len();
        let mut e = next;
        if raw.ends_with('\n') {
            e -= 1;
            if raw.ends_with("\r\n") {
                e -= 1;
                crlf = true;
            }
        }
        ranges.push(pos..e);
        pos = next;
    }
    let n = ranges.len();
    let mut classes: Vec<Option<Class>> = vec![None; n];

    // Front matter: `---` (or `+++`) on the first line and the same (or `...`) later.
    if n > 0 {
        let first = text[ranges[0].clone()].trim_end();
        if first == "---" || first == "+++" {
            let close = (1..n).find(|&j| {
                let t = text[ranges[j].clone()].trim_end();
                t == first || (first == "---" && t == "...")
            });
            if let Some(j) = close {
                for c in classes.iter_mut().take(j + 1) {
                    *c = Some(Class::Front);
                }
            }
        }
    }

    let mut fence: Option<(u8, usize)> = None;
    let mut open_run: Option<Range<usize>> = None;
    for i in 0..n {
        if classes[i].is_some() {
            continue;
        }
        let r = ranges[i].clone();
        let s = &text[r.clone()];
        if let Some((ch, len)) = fence {
            let t = s.trim();
            if t.len() >= len && t.bytes().all(|b| b == ch) {
                fence = None;
                open_run = None;
            }
            classes[i] = Some(Class::Code);
            continue;
        }
        let (_, lb, _) = leading(s);
        let rest = &s[lb..];
        if let Some(ch) = rest.bytes().next().filter(|b| *b == b'`' || *b == b'~') {
            let run = rest.bytes().take_while(|b| *b == ch).count();
            let info = &rest[run..];
            if run >= 3 && !(ch == b'`' && info.contains('`')) {
                fence = Some((ch, run));
                open_run = Some(r.start + lb..r.start + lb + run);
                classes[i] = Some(Class::Code);
                continue;
            }
        }
        classes[i] = Some(classify(s, r.start));
    }
    let mut classes: Vec<Class> = classes.into_iter().map(|c| c.unwrap_or(Class::Para)).collect();

    // Setext headings: a single paragraph line over `===` or `---`.
    for i in 0..n.saturating_sub(1) {
        if classes[i] != Class::Para || (i > 0 && matches!(classes[i - 1], Class::Para | Class::Item(_))) {
            continue;
        }
        let under = text[ranges[i + 1].clone()].trim();
        let (_, lb, _) = leading(&text[ranges[i + 1].clone()]);
        if lb >= 4 || under.is_empty() {
            continue;
        }
        let level = if matches!(classes[i + 1], Class::Para) && under.bytes().all(|b| b == b'=') {
            1
        } else if matches!(classes[i + 1], Class::Para | Class::Hr) && under.bytes().all(|b| b == b'-') {
            2
        } else {
            continue;
        };
        let s = &text[ranges[i].clone()];
        let (_, lb, _) = leading(s);
        if lb >= 4 {
            continue;
        }
        let t = s.trim_end();
        classes[i] = Class::Setext { level, text: ranges[i].start + lb..ranges[i].start + t.len() };
        classes[i + 1] = Class::SetextUnderline;
    }

    let lines = ranges.into_iter().zip(classes).map(|(range, class)| Line { range, class }).collect();
    Scan { lines, eol: if crlf { "\r\n" } else { "\n" }, unclosed: if fence.is_some() { open_run } else { None } }
}

fn classify(s: &str, base: usize) -> Class {
    if s.trim().is_empty() {
        return Class::Blank;
    }
    let (cols, lb, _) = leading(s);
    let rest = &s[lb..];
    if cols < 4 {
        let hashes = rest.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&hashes) && rest.as_bytes().get(hashes).is_none_or(|b| *b == b' ' || *b == b'\t') {
            let after = &rest[hashes..];
            let lead_ws = after.len() - after.trim_start().len();
            let mut t = after.trim();
            let stripped = t.trim_end_matches('#');
            if stripped.len() != t.len() && (stripped.is_empty() || stripped.ends_with([' ', '\t'])) {
                t = stripped.trim_end();
            }
            let start = base + lb + hashes + lead_ws;
            return Class::Atx { level: hashes as u8, marker: base + lb..base + lb + hashes, text: start..start + t.len() };
        }
        if is_hr(rest) {
            return Class::Hr;
        }
    }
    match parse_item(s, base) {
        Some(it) => Class::Item(it),
        None => Class::Para,
    }
}

fn is_hr(rest: &str) -> bool {
    let t: String = rest.chars().filter(|c| *c != ' ' && *c != '\t').collect();
    let mut chars = t.chars();
    match chars.next() {
        Some(c @ ('-' | '*' | '_')) => t.len() >= 3 && t.chars().all(|x| x == c),
        _ => false,
    }
}

fn parse_item(s: &str, base: usize) -> Option<Item> {
    let (cols, lb, spaces_only) = leading(s);
    let rest = &s[lb..];
    let b = rest.as_bytes();
    let first = *b.first()?;
    let (mlen, ordered, mch) = if matches!(first, b'-' | b'*' | b'+') {
        (1, false, first as char)
    } else {
        let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
        match b.get(digits) {
            Some(d @ (b'.' | b')')) if (1..=9).contains(&digits) => (digits + 1, true, *d as char),
            _ => return None,
        }
    };
    let after = &rest[mlen..];
    if !(after.is_empty() || after.starts_with(' ') || after.starts_with('\t')) {
        return None;
    }
    let spaces = after.bytes().take_while(|c| *c == b' ').count();
    let w = if after.trim().is_empty() || spaces == 0 || spaces > 4 { 1 } else { spaces };
    Some(Item {
        indent: cols,
        indent_range: base..base + lb,
        spaces_only,
        marker: base + lb..base + lb + mlen,
        ordered,
        mch,
        content: cols + mlen + w,
    })
}

/// Backtick code spans of one line (a run closed by a run of the same length on the same line), as ranges of `s`.
pub(crate) fn code_spans(s: &str) -> Vec<Range<usize>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'`' => {
                let run = b[i..].iter().take_while(|c| **c == b'`').count();
                let mut j = i + run;
                let mut closed = None;
                while j < b.len() {
                    if b[j] == b'`' {
                        let r2 = b[j..].iter().take_while(|c| **c == b'`').count();
                        if r2 == run {
                            closed = Some(j + r2);
                            break;
                        }
                        j += r2;
                    } else {
                        j += 1;
                    }
                }
                match closed {
                    Some(e) => {
                        out.push(i..e);
                        i = e;
                    }
                    None => i += run,
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Where the destination of an inline link starts and ends, for each `](dest)` outside the given spans.
pub(crate) fn link_dests(s: &str, spans: &[Range<usize>]) -> Vec<Range<usize>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == b']' && b[i + 1] == b'(' && !spans.iter().any(|r| r.contains(&i)) && s[..i].contains('[') {
            let mut j = i + 2;
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            let (start, end);
            if b.get(j) == Some(&b'<') {
                start = j + 1;
                let mut k = start;
                while k < b.len() && b[k] != b'>' && b[k] != b'\n' {
                    k += 1;
                }
                end = k;
            } else {
                start = j;
                let mut depth = 0usize;
                let mut k = j;
                while k < b.len() {
                    match b[k] {
                        b'\\' => k += 1,
                        b'(' => depth += 1,
                        b')' if depth == 0 => break,
                        b')' => depth -= 1,
                        b' ' | b'\t' => break,
                        _ => {}
                    }
                    k += 1;
                }
                end = k.min(b.len());
            }
            if end > start {
                out.push(start..end);
            }
            i = end.max(i + 2);
            continue;
        }
        i += 1;
    }
    out
}

/// The parts of a prose line that are not prose: code spans, link destinations (`](…)`), `<autolinks>` and tags, bare
/// URLs, and the target of a reference definition. Sorted, as ranges of `s`.
pub(crate) fn masks(s: &str) -> Vec<Range<usize>> {
    let spans = code_spans(s);
    let mut out = spans.clone();
    out.extend(link_dests(s, &spans).into_iter().map(|r| r.start.saturating_sub(2)..(r.end + 1).min(s.len())));
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if spans.iter().any(|r| r.contains(&i)) {
            i += 1;
            continue;
        }
        if b[i] == b'<' && b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'/' || *c == b'!') {
            if let Some(off) = s[i..].find('>') {
                out.push(i..i + off + 1);
                i += off + 1;
                continue;
            }
        }
        if (b[i..].starts_with(b"http://") || b[i..].starts_with(b"https://")) && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let len = s[i..]
                .find(|c: char| !c.is_ascii() || c.is_whitespace() || matches!(c, ')' | '>' | ']' | '"' | '\''))
                .unwrap_or(s.len() - i);
            out.push(i..i + len);
            i += len.max(1);
            continue;
        }
        i += 1;
    }
    // Reference definition: `[label]: target`.
    if let Some(def) = ref_def(s) {
        out.push(def.1);
    }
    out.sort_by_key(|r| r.start);
    out
}

/// `[label]: target` at the start of a line: the normalised label and the byte range of the rest of the line.
pub(crate) fn ref_def(s: &str) -> Option<(String, Range<usize>)> {
    let (_, lb, _) = leading(s);
    if lb >= 4 {
        return None;
    }
    let rest = &s[lb..];
    let inner = rest.strip_prefix('[')?;
    let close = inner.find(']')?;
    let label = &inner[..close];
    if label.is_empty() || label.starts_with('^') || label.contains('[') {
        return None;
    }
    inner[close + 1..].strip_prefix(':')?;
    Some((normalize_label(label), lb + 1 + close + 2..s.len()))
}

pub(crate) fn normalize_label(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}
