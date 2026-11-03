//! Markup on a selection: inline toggles (bold, italic, code, strike, link), line types (heading, quote, fence, bullet,
//! number, task) and checking / unchecking every task a selection touches. Each command is a pure function of the text
//! and a selection (byte range) and gives one [`SelChange`] (so one undo step) with the selection after the change.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

use crate::list::code_lines;
use crate::{lines, Change};

/// A change together with the selection the editor puts after it.
///
/// `change.cursor` is the end of `selection`. `unflanked` is only set by [`inline`]: the markup it put in will not
/// render as emphasis in CommonMark (see [`inline`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelChange {
    pub change: Change,
    /// The selection in the new text (collapsed to a caret when it is empty).
    pub selection: Range<usize>,
    pub unflanked: bool,
}

impl SelChange {
    /// The text with the change applied.
    pub fn apply(&self, text: &str) -> String {
        self.change.apply(text)
    }
}

/// An inline markup for [`inline`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inline {
    /// `**text**`
    Bold,
    /// `*text*`
    Italic,
    /// `` `text` `` (a longer backtick run when the text holds backticks)
    Code,
    /// `~~text~~`
    Strike,
    /// `[text](url)`
    Link,
}

/// A line type for [`line_type`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// `#` x N for 1 to 6; 0 makes the lines paragraphs (heading, list and task markers go; a quote stays).
    Heading(u8),
    /// `> `
    Quote,
    /// A code fence around the lines (or off, when the lines are in a fenced block).
    Fence,
    /// `- `
    Bullet,
    /// `1. `, `2. `, ... (the first line's number is kept; nested levels start at 1)
    Number,
    /// `- [ ] `
    Task,
}

type Ed = (Range<usize>, String);

// ---------------------------------------------------------------------------------------------------------------
// Plumbing

fn floor(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn norm(text: &str, sel: Range<usize>) -> Range<usize> {
    let (a, b) = if sel.start <= sel.end { (sel.start, sel.end) } else { (sel.end, sel.start) };
    floor(text, a)..floor(text, b)
}

/// The lines of `text` without their breaks; unlike [`lines`] an empty last line (after a final break) is there too, so
/// the caret at the end of the text has a line.
fn all_lines(text: &str) -> Vec<Range<usize>> {
    let mut v = lines(text);
    if text.is_empty() || text.ends_with('\n') {
        v.push(text.len()..text.len());
    }
    v
}

fn line_of(ls: &[Range<usize>], p: usize) -> usize {
    ls.partition_point(|l| l.start <= p).saturating_sub(1)
}

/// The first and last line a selection touches (a non-empty selection that ends at a line's start does not touch it).
fn touched(ls: &[Range<usize>], sel: &Range<usize>) -> (usize, usize) {
    let a = line_of(ls, sel.start);
    let mut b = line_of(ls, sel.end);
    if sel.end > sel.start && b > a && ls[b].start == sel.end {
        b -= 1;
    }
    (a, b)
}

fn eol(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

struct Plan {
    edits: Vec<Ed>,
}

fn plan(text: &str, mut edits: Vec<Ed>) -> Option<Plan> {
    edits.sort_by_key(|a| (a.0.start, a.0.end));
    edits.retain(|(r, s)| text[r.clone()] != *s);
    if edits.windows(2).any(|w| w[1].0.start < w[0].0.end) {
        return None;
    }
    if edits.is_empty() {
        None
    } else {
        Some(Plan { edits })
    }
}

impl Plan {
    /// Old byte to new byte. An insertion exactly at `p` counts when `after`. A byte inside a replaced range goes to the
    /// end of the replacement.
    fn map(&self, p: usize, after: bool) -> usize {
        let mut d = 0isize;
        for (r, s) in &self.edits {
            let delta = s.len() as isize - (r.end - r.start) as isize;
            if r.start == r.end {
                if r.start < p || (r.start == p && after) {
                    d += delta;
                }
            } else if p >= r.end {
                d += delta;
            } else if p > r.start {
                return (r.start as isize + d) as usize + s.len();
            }
        }
        (p as isize + d) as usize
    }

    fn finish(&self, text: &str, selection: Range<usize>, unflanked: bool) -> SelChange {
        let first = self.edits.first().unwrap().0.start;
        let last = self.edits.last().unwrap().0.end;
        let mut new = String::new();
        let mut at = first;
        for (r, s) in &self.edits {
            new.push_str(&text[at..r.start]);
            new.push_str(s);
            at = r.end;
        }
        SelChange { change: Change { range: first..last, text: new, cursor: selection.end }, selection, unflanked }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The shape of a line

struct Marker {
    ordered: bool,
    /// The bullet character, or the delimiter (`.` / `)`) of a number.
    ch: u8,
    number: Option<u64>,
}

struct Parts {
    /// End of the `> ` prefixes.
    quote_end: usize,
    /// End of the blanks after them (where a list marker, a heading or the text starts).
    indent_end: usize,
    marker: Option<Marker>,
    /// Whether the line has a task box, and whether it is checked; `task_at` is the byte of the `x` or the space.
    task: Option<bool>,
    task_at: usize,
    heading: u8,
    /// Where the text of the line starts.
    body: usize,
}

fn is_hr(s: &str) -> bool {
    let t: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && matches!(t[0], '-' | '*' | '_') && t.iter().all(|c| *c == t[0])
}

fn parse_line(line: &str) -> Parts {
    let b = line.as_bytes();
    let n = b.len();
    let (mut p, mut quote_end) = (0, 0);
    loop {
        let mut q = p;
        while q < n && b[q] == b' ' {
            q += 1;
        }
        if q < n && b[q] == b'>' {
            q += 1;
            if q < n && b[q] == b' ' {
                q += 1;
            }
            p = q;
            quote_end = q;
        } else {
            break;
        }
    }
    let mut q = p;
    while q < n && (b[q] == b' ' || b[q] == b'\t') {
        q += 1;
    }
    let indent_end = q;
    let mut parts = Parts { quote_end, indent_end, marker: None, task: None, task_at: 0, heading: 0, body: indent_end };
    let rest = &b[q..];
    let mut mend = None;
    if !rest.is_empty() && matches!(rest[0], b'-' | b'*' | b'+') && (rest.len() == 1 || rest[1] == b' ') && !is_hr(&line[q..]) {
        parts.marker = Some(Marker { ordered: false, ch: rest[0], number: None });
        mend = Some(q + 1 + usize::from(rest.len() > 1));
    } else {
        let d = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        if (1..=9).contains(&d) && rest.len() > d && matches!(rest[d], b'.' | b')') && (rest.len() == d + 1 || rest[d + 1] == b' ') {
            parts.marker = Some(Marker { ordered: true, ch: rest[d], number: line[q..q + d].parse().ok() });
            mend = Some(q + d + 1 + usize::from(rest.len() > d + 1));
        }
    }
    if let Some(e) = mend {
        parts.body = e;
        if n >= e + 3 && b[e] == b'[' && matches!(b[e + 1], b' ' | b'x' | b'X') && b[e + 2] == b']' && (n == e + 3 || b[e + 3] == b' ') {
            parts.task = Some(b[e + 1] != b' ');
            parts.task_at = e + 1;
            parts.body = e + 3 + usize::from(n > e + 3);
        }
    } else if !rest.is_empty() && rest[0] == b'#' {
        let h = rest.iter().take_while(|c| **c == b'#').count();
        if (1..=6).contains(&h) && (rest.len() == h || rest[h] == b' ') {
            parts.heading = h as u8;
            parts.body = q + h + usize::from(rest.len() > h);
        }
    }
    parts
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

// ---------------------------------------------------------------------------------------------------------------
// Parsed spans

#[derive(Default)]
struct Spans {
    code: Vec<Range<usize>>,
    bold: Vec<Range<usize>>,
    italic: Vec<Range<usize>>,
    strike: Vec<Range<usize>>,
    fenced: Vec<Range<usize>>,
    indented: Vec<Range<usize>>,
}

fn spans(text: &str) -> Spans {
    let mut s = Spans::default();
    for (ev, r) in Parser::new_ext(text, Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH).into_offset_iter() {
        match ev {
            Event::Code(_) => s.code.push(r),
            Event::Start(Tag::Strong) => s.bold.push(r),
            Event::Start(Tag::Emphasis) => s.italic.push(r),
            Event::Start(Tag::Strikethrough) => s.strike.push(r),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))) => s.fenced.push(r),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)) => s.indented.push(r),
            _ => {}
        }
    }
    s
}

// ---------------------------------------------------------------------------------------------------------------
// Inline

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

fn word_at(text: &str, p: usize, cell: &Range<usize>) -> Option<Range<usize>> {
    let (mut s, mut e) = (p, p);
    while s > cell.start {
        match text[..s].chars().next_back() {
            Some(c) if is_word(c) => s -= c.len_utf8(),
            _ => break,
        }
    }
    while e < cell.end {
        match text[e..].chars().next() {
            Some(c) if is_word(c) => e += c.len_utf8(),
            _ => break,
        }
    }
    (s < e).then_some(s..e)
}

fn trim_range(text: &str, r: Range<usize>) -> Range<usize> {
    let t = &text[r.clone()];
    let a = t.len() - t.trim_start().len();
    let b = t.trim_end().len();
    if a >= b {
        return r.start..r.start;
    }
    r.start + a..r.start + b
}

/// The cells of a table row between `from` and `to` (unescaped pipes outside code spans split them), or the whole
/// range when the line is not a table row.
fn cells(text: &str, from: usize, to: usize, code: &[Range<usize>]) -> Vec<Range<usize>> {
    if !text[from..to].trim_start().starts_with('|') {
        return std::iter::once(from..to).collect();
    }
    let mut out = Vec::new();
    let mut start = from;
    let b = text.as_bytes();
    for i in from..to {
        if b[i] == b'|' && (i == from || b[i - 1] != b'\\') && !code.iter().any(|c| c.start < i && i < c.end) {
            out.push(start..i);
            start = i + 1;
        }
    }
    out.push(start..to);
    out
}

/// The pieces of text the command works on, one per line (per cell in a table row), in order. A caret gives the word
/// around it, or an empty range.
fn segments(
    text: &str,
    ls: &[Range<usize>],
    code: &[bool],
    spans_code: &[Range<usize>],
    sel: &Range<usize>,
    first: usize,
    last: usize,
) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    for li in first..=last {
        if code[li] {
            continue;
        }
        let line = &ls[li];
        let cs = line.start + parse_line(&text[line.clone()]).body;
        let cs = cs.min(line.end);
        let cs_cells = cells(text, cs, line.end, spans_code);
        if sel.start == sel.end {
            let p = sel.start.clamp(cs, line.end);
            let cell = cs_cells.iter().find(|c| c.start <= p && p <= c.end).cloned().unwrap_or(cs..line.end);
            out.push(word_at(text, p, &cell).unwrap_or(p..p));
        } else {
            for c in cs_cells {
                let r = sel.start.max(c.start)..sel.end.min(c.end);
                if r.start >= r.end {
                    continue;
                }
                let r = trim_range(text, r);
                if r.is_empty()
                    || text[r.clone()].chars().all(|c| matches!(c, '-' | ':'))
                        && text[r.clone()].contains('-')
                        && cs_cells_is_table(text, cs, line.end)
                {
                    continue;
                }
                out.push(r);
            }
        }
    }
    out
}

fn cs_cells_is_table(text: &str, from: usize, to: usize) -> bool {
    text[from..to].trim_start().starts_with('|')
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace() && !c.is_control())
}

/// Whether the pair of markers of `m` bytes at `open` and `close` of `new` fails to open or close emphasis under the
/// flanking rules of CommonMark (a `**` next to Japanese punctuation and a letter, for example).
fn unflanked_pair(new: &str, open: usize, close: usize, m: usize) -> bool {
    let prev = |i: usize| new[..i].chars().next_back();
    let next = |i: usize| new[i..].chars().next();
    let ws = |c: Option<char>| c.is_none_or(char::is_whitespace);
    let punct = |c: Option<char>| c.is_some_and(is_punct);
    let (p, n) = (prev(open), next(open + m));
    let left = !ws(n) && (!punct(n) || ws(p) || punct(p));
    let (p, n) = (prev(close), next(close + m));
    let right = !ws(p) && (!punct(p) || ws(n) || punct(n));
    !(left && right)
}

fn is_url(s: &str) -> bool {
    !s.chars().any(char::is_whitespace) && ["http://", "https://", "mailto:", "ftp://", "www."].iter().any(|p| s.starts_with(p))
}

/// `[text](url)` spans in `from..to`: (the `[`, the `]`, the end after the `)`).
fn link_ranges(text: &str, from: usize, to: usize) -> Vec<(usize, usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = from;
    while i < to {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == b'[' && (i == from || b[i - 1] != b'!') {
            let (mut depth, mut j, mut close) = (0, i, None);
            while j < to {
                match b[j] {
                    b'\\' => j += 1,
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if let Some(c) = close.filter(|c| c + 1 < to && b[c + 1] == b'(') {
                let (mut depth, mut k) = (0, c + 1);
                while k < to {
                    match b[k] {
                        b'\\' => k += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                if k < to {
                    out.push((i, c, k + 1));
                    i = k + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

enum Found {
    /// The seg is inside markup: the edits that take it off and the range to keep selected.
    Marked(Vec<Ed>, Range<usize>),
    Plain,
    Abort,
}

fn code_found(text: &str, sp: &Spans, seg: &Range<usize>) -> Found {
    for c in &sp.code {
        let contained = if seg.is_empty() { c.start < seg.start && seg.start < c.end } else { c.start <= seg.start && seg.end <= c.end };
        if contained {
            let n = text[c.clone()].bytes().take_while(|b| *b == b'`').count();
            let inner = &text[c.start + n..c.end - n];
            let pad = usize::from(inner.len() >= 2 && inner.starts_with(' ') && inner.ends_with(' ') && !inner.trim().is_empty());
            let content = c.start + n + pad..c.end - n - pad;
            let edits = vec![(c.start..content.start, String::new()), (content.end..c.end, String::new())];
            let keep = if content.start <= seg.start && seg.end <= content.end { seg.clone() } else { content };
            return Found::Marked(edits, keep);
        }
        let inside = seg.start < c.end && c.start < seg.end;
        let covers = seg.start <= c.start && c.end <= seg.end;
        if inside && !covers {
            return Found::Abort;
        }
    }
    if seg.is_empty() {
        let p = seg.start;
        let l = text[..p].bytes().rev().take_while(|b| *b == b'`').count();
        let r = text[p..].bytes().take_while(|b| *b == b'`').count();
        let n = l.min(r);
        if n > 0 {
            return Found::Marked(vec![(p - n..p, String::new()), (p..p + n, String::new())], p..p);
        }
    }
    Found::Plain
}

fn emphasis_found(text: &str, sp: &Spans, kind: Inline, seg: &Range<usize>) -> Found {
    let (list, ch) = match kind {
        Inline::Bold => (&sp.bold, b'*'),
        Inline::Italic => (&sp.italic, b'*'),
        _ => (&sp.strike, b'~'),
    };
    let m_of = |r: &Range<usize>| match kind {
        Inline::Bold => 2,
        Inline::Italic => 1,
        _ => text[r.clone()].bytes().take_while(|b| *b == b'~').count().min(2),
    };
    let best = list
        .iter()
        .filter(|r| {
            let m = m_of(r);
            r.end - r.start >= 2 * m
                && if seg.is_empty() {
                    r.start + m <= seg.start && seg.end <= r.end - m
                } else {
                    r.start <= seg.start && seg.end <= r.end && seg.start < r.end - m && r.start + m < seg.end
                }
        })
        .min_by_key(|r| r.end - r.start);
    if let Some(r) = best {
        let m = m_of(r);
        let content = r.start + m..r.end - m;
        let edits = vec![(r.start..r.start + m, String::new()), (r.end - m..r.end, String::new())];
        let keep = seg.start.max(content.start)..seg.end.min(content.end).max(seg.start.max(content.start));
        return Found::Marked(edits, keep);
    }
    // Markers next to the seg that the parser does not read as emphasis (Japanese flanking), taken off as they are.
    let l = text[..seg.start].bytes().rev().take_while(|b| *b == ch).count();
    let r = text[seg.end..].bytes().take_while(|b| *b == ch).count();
    let k = l.min(r);
    let (m, marked) = match kind {
        Inline::Italic => (1, k % 2 == 1),
        _ => (2, k >= 2),
    };
    if marked {
        return Found::Marked(vec![(seg.start - m..seg.start, String::new()), (seg.end..seg.end + m, String::new())], seg.clone());
    }
    Found::Plain
}

/// Toggle an inline markup on the selection `sel` (bytes of `text`).
///
/// - A selection is wrapped once per line (per cell in a table row); the line markers (`- `, `1. `, `> `, `[ ] `,
///   `# `, the indent) stay outside, and so do the blanks at the ends of the selection.
/// - When every piece already has the markup (read by the CommonMark parser, or `**` / `~~` right around it), it is
///   taken off instead, the whole span at once. The selection after the change is the text inside the markup, so a
///   second call undoes the first.
/// - A caret works on the word around it (letters and digits, CJK included), or puts an empty pair in with the caret
///   between. A caret inside an empty pair takes it out.
/// - Code spans and fenced / indented code blocks are not markup: a caret or selection edge inside a code span, or only
///   code lines, gives `None`, except [`Inline::Code`], which toggles the span off.
/// - [`Inline::Code`] uses a backtick run longer than any in the text (`` ``a`b`` ``), padded with a space when the text
///   starts or ends with a backtick.
/// - [`Inline::Link`] works on one piece only (`None` for a selection over several lines): `[sel]()` with the caret in
///   the `()`; a URL gives `[](url)` with the caret in the `[]`; no text gives `[]()` with the caret in the `[]`. Inside
///   `[text](url)` it takes the link off and keeps the text.
/// - `SelChange::unflanked` is set when the `**` / `*` / `~~` put in will not render under CommonMark's flanking rules
///   (`**「強調」**です`: the closing `**` follows punctuation and precedes a letter). The text is not changed; the
///   caller may tell the user.
pub fn inline(text: &str, sel: Range<usize>, kind: Inline) -> Option<SelChange> {
    let sel = norm(text, sel);
    let ls = all_lines(text);
    let code = code_lines(text, &ls);
    let (first, last) = touched(&ls, &sel);
    let sp = spans(text);
    let segs = segments(text, &ls, &code, &sp.code, &sel, first, last);
    if segs.is_empty() {
        return None;
    }
    if kind != Inline::Code && segs.iter().any(|s| [s.start, s.end].iter().any(|e| sp.code.iter().any(|c| c.start < *e && *e < c.end))) {
        return None;
    }
    if kind == Inline::Link {
        return link(text, &ls, &sel, &segs);
    }
    let found: Vec<Found> =
        segs.iter().map(|s| if kind == Inline::Code { code_found(text, &sp, s) } else { emphasis_found(text, &sp, kind, s) }).collect();
    if found.iter().any(|f| matches!(f, Found::Abort)) {
        return None;
    }
    let off = found.iter().all(|f| matches!(f, Found::Marked(..)));
    let marker = match kind {
        Inline::Bold => "**",
        Inline::Italic => "*",
        _ => "~~",
    };
    let mut edits: Vec<Ed> = Vec::new();
    let mut contents: Vec<Range<usize>> = Vec::new();
    let mut added: Vec<(Range<usize>, usize)> = Vec::new();
    for (seg, f) in segs.iter().zip(found) {
        match f {
            Found::Marked(e, keep) => {
                if off {
                    edits.extend(e);
                }
                contents.push(keep);
            }
            Found::Plain => {
                let (open, close) = if kind == Inline::Code {
                    let t = &text[seg.clone()];
                    let run = t.split(|c| c != '`').map(str::len).max().unwrap_or(0);
                    let n = if run > 0 { run + 1 } else { 1 };
                    let pad = if t.starts_with('`') || t.ends_with('`') { " " } else { "" };
                    (format!("{}{pad}", "`".repeat(n)), format!("{pad}{}", "`".repeat(n)))
                } else {
                    (marker.to_string(), marker.to_string())
                };
                added.push((seg.clone(), open.len()));
                edits.push((seg.start..seg.start, open));
                edits.push((seg.end..seg.end, close));
                contents.push(seg.clone());
            }
            Found::Abort => unreachable!(),
        }
    }
    let plan = plan(text, edits)?;
    let (cs_old, ce_old) = (contents.first()?.start, contents.last()?.end);
    let (cs, ce) = (plan.map(cs_old, true), plan.map(ce_old, false).max(plan.map(cs_old, true)));
    let selection = if sel.start == sel.end {
        let c = match added.iter().find(|(seg, _)| seg.is_empty()) {
            Some((seg, open)) => plan.map(seg.start, false) + open,
            None => plan.map(sel.start, true).max(cs).min(ce),
        };
        c..c
    } else {
        cs..ce
    };
    let mut out = plan.finish(text, selection, false);
    if kind != Inline::Code && !off {
        let new = out.apply(text);
        out.unflanked = added
            .iter()
            .any(|(seg, m)| !seg.is_empty() && unflanked_pair(&new, plan.map(seg.start, true) - m, plan.map(seg.end, false), *m));
    }
    Some(out)
}

fn link(text: &str, ls: &[Range<usize>], sel: &Range<usize>, segs: &[Range<usize>]) -> Option<SelChange> {
    if segs.len() != 1 {
        return None;
    }
    let seg = &segs[0];
    let line = &ls[line_of(ls, seg.start)];
    let cs = (line.start + parse_line(&text[line.clone()]).body).min(line.end);
    let caret = sel.start == sel.end;
    if let Some(&(o, c, e)) = link_ranges(text, cs, line.end).iter().find(|(o, _, e)| *o <= seg.start && seg.end <= *e) {
        let plan = plan(text, vec![(o..o + 1, String::new()), (c..e, String::new())])?;
        let selection = if caret {
            let p = plan.map(sel.start, true).clamp(plan.map(o + 1, true), plan.map(c, false));
            p..p
        } else {
            plan.map(o + 1, true)..plan.map(c, false)
        };
        return Some(plan.finish(text, selection, false));
    }
    let (edits, caret_at): (Vec<Ed>, usize) = if seg.is_empty() {
        (vec![(seg.start..seg.start, "[]()".to_string())], 1)
    } else if is_url(&text[seg.clone()]) {
        (vec![(seg.start..seg.start, "[](".to_string()), (seg.end..seg.end, ")".to_string())], 1)
    } else {
        (vec![(seg.start..seg.start, "[".to_string()), (seg.end..seg.end, "]()".to_string())], usize::MAX)
    };
    let plan = plan(text, edits)?;
    let at = if caret_at == usize::MAX { plan.map(seg.end, false) + 2 } else { plan.map(seg.start, false) + caret_at };
    Some(plan.finish(text, at..at, false))
}

// ---------------------------------------------------------------------------------------------------------------
// Line types

/// The lines a line command works on: the touched lines that are not code and not blank (blank lines between them stay
/// when `inner`); a single blank line when that is all there is.
fn targets(text: &str, ls: &[Range<usize>], code: &[bool], first: usize, last: usize, inner: bool) -> Vec<usize> {
    let idx: Vec<usize> = (first..=last).filter(|&i| !code[i]).collect();
    let nb: Vec<usize> = idx.iter().copied().filter(|&i| !blank(&text[ls[i].clone()])).collect();
    if nb.is_empty() {
        return if idx.len() == 1 { idx } else { Vec::new() };
    }
    if inner {
        idx.into_iter().filter(|&i| i >= nb[0] && i <= nb[nb.len() - 1]).collect()
    } else {
        nb
    }
}

fn width(ws: &str) -> usize {
    ws.chars().fold(0, |w, c| if c == '\t' { (w / 4 + 1) * 4 } else { w + 1 })
}

/// Apply a line type to every line the selection touches. See [`Line`].
///
/// - Heading N on lines that are all heading N makes them paragraphs, otherwise sets the level (a list marker is
///   replaced). `Heading(0)` makes paragraphs. `Heading(7..)` is `None`.
/// - Bullet / Number / Task replace each other's markers, keep the indent and the `>` prefixes, and go back to
///   paragraphs when every line already has that type. Number counts from the first line's number (1 when it has none),
///   a level deeper than its predecessor starts at 1. Task lines already have their box and stay as they are.
/// - Quote adds `> ` to the lines without one (blank lines between lines stay in the quote), or takes one level off
///   when every line has it.
/// - Fence wraps the lines in a code fence (longer when they hold one) that carries the first line's `>` prefix and
///   indent, or takes the fence off when the lines are in a fenced block. A caret on a blank line gives an empty fence.
/// - Blank lines and code lines are left alone (except as above). Line breaks (CRLF) are kept.
///
/// The selection stays on the same text (a caret stays in its place after the new marker).
pub fn line_type(text: &str, sel: Range<usize>, kind: Line) -> Option<SelChange> {
    let sel = norm(text, sel);
    let ls = all_lines(text);
    let code = code_lines(text, &ls);
    let (first, last) = touched(&ls, &sel);
    let edits: Vec<Ed> = match kind {
        Line::Fence => return fence(text, &ls, &sel, first, last),
        Line::Heading(n) if n > 6 => return None,
        Line::Quote => quote(text, &ls, &code, first, last),
        _ => marker_edits(text, &ls, &code, first, last, kind),
    };
    let plan = plan(text, edits)?;
    Some(keep_selection(&plan, text, &sel))
}

fn keep_selection(plan: &Plan, text: &str, sel: &Range<usize>) -> SelChange {
    let selection = if sel.start == sel.end {
        let p = plan.map(sel.start, true);
        p..p
    } else {
        plan.map(sel.start, false)..plan.map(sel.end, true).max(plan.map(sel.start, false))
    };
    plan.finish(text, selection, false)
}

fn quote(text: &str, ls: &[Range<usize>], code: &[bool], first: usize, last: usize) -> Vec<Ed> {
    let t = targets(text, ls, code, first, last, true);
    let parts: Vec<Parts> = t.iter().map(|&i| parse_line(&text[ls[i].clone()])).collect();
    let all = !t.is_empty() && parts.iter().all(|p| p.quote_end > 0);
    let mut edits = Vec::new();
    for (&i, p) in t.iter().zip(&parts) {
        let l = &ls[i];
        if all {
            let line = &text[l.clone()];
            let at = line.find('>').unwrap_or(0);
            let end = at + 1 + usize::from(line[at + 1..].starts_with(' '));
            edits.push((l.start + at..l.start + end, String::new()));
        } else if p.quote_end == 0 {
            edits.push((l.start..l.start, if blank(&text[l.clone()]) { ">" } else { "> " }.to_string()));
        }
    }
    edits
}

fn marker_edits(text: &str, ls: &[Range<usize>], code: &[bool], first: usize, last: usize, kind: Line) -> Vec<Ed> {
    let t = targets(text, ls, code, first, last, false);
    let parts: Vec<Parts> = t.iter().map(|&i| parse_line(&text[ls[i].clone()])).collect();
    let is = |p: &Parts| match kind {
        Line::Heading(n) => n > 0 && p.heading == n,
        Line::Bullet => p.marker.as_ref().is_some_and(|m| !m.ordered) && p.task.is_none(),
        Line::Number => p.marker.as_ref().is_some_and(|m| m.ordered) && p.task.is_none(),
        _ => p.task.is_some(),
    };
    let plain = matches!(kind, Line::Heading(0)) || (!t.is_empty() && parts.iter().all(is));
    let start = parts.first().and_then(|p| p.marker.as_ref()).filter(|m| m.ordered).and_then(|m| m.number).unwrap_or(1);
    let first_delim = parts.iter().find_map(|p| p.marker.as_ref().filter(|m| m.ordered)).map_or('.', |m| m.ch as char);
    let mut stack: Vec<(usize, u64)> = Vec::new();
    let mut edits = Vec::new();
    for (&i, p) in t.iter().zip(&parts) {
        let l = &ls[i];
        let line = &text[l.clone()];
        let ind = if p.heading > 0 { "" } else { &line[p.quote_end..p.indent_end] };
        let bullet = match &p.marker {
            Some(m) if !m.ordered => m.ch as char,
            _ => '-',
        };
        let new = if plain {
            String::new()
        } else {
            match kind {
                Line::Heading(n) => format!("{} ", "#".repeat(n as usize)),
                Line::Bullet => format!("{ind}{bullet} "),
                Line::Task if p.task.is_some() => continue,
                Line::Task => format!("{ind}{bullet} [ ] "),
                _ => {
                    let w = width(ind);
                    while stack.last().is_some_and(|s| s.0 > w) {
                        stack.pop();
                    }
                    let n = match stack.last_mut() {
                        Some(s) if s.0 == w => {
                            s.1 += 1;
                            s.1
                        }
                        _ => {
                            let n = if stack.is_empty() { start } else { 1 };
                            stack.push((w, n));
                            n
                        }
                    };
                    let delim = first_delim;
                    format!("{ind}{n}{delim} ")
                }
            }
        };
        edits.push((l.start + p.quote_end..l.start + p.body, new));
    }
    edits
}

fn fence(text: &str, ls: &[Range<usize>], sel: &Range<usize>, first: usize, last: usize) -> Option<SelChange> {
    let sp = spans(text);
    let block_lines = |r: &Range<usize>| (line_of(ls, r.start), line_of(ls, r.end.saturating_sub(1).max(r.start)));
    if sp.indented.iter().any(|r| {
        let (a, b) = block_lines(r);
        a <= last && first <= b
    }) {
        return None;
    }
    if let Some(r) = sp.fenced.iter().find(|r| {
        let (a, b) = block_lines(r);
        a <= last && first <= b
    }) {
        let (a, b) = block_lines(r);
        let open = text[ls[a].clone()].trim_start_matches([' ', '>']);
        let ch = open.chars().next()?;
        if text[ls[a].start..r.start].trim_start_matches([' ', '>']).chars().any(|c| !c.is_whitespace()) {
            return None;
        }
        let close = (b > a)
            .then(|| text[ls[b].clone()].trim_start_matches([' ', '>']).trim_end())
            .filter(|t| t.len() >= 3 && t.chars().all(|c| c == ch))
            .map(|_| b);
        let brk_end = |i: usize| {
            let e = ls[i].end;
            if text[e..].starts_with("\r\n") {
                e + 2
            } else if text[e..].starts_with('\n') {
                e + 1
            } else {
                e
            }
        };
        let mut edits: Vec<Ed> = vec![(ls[a].start..brk_end(a).max(ls[a].end), String::new())];
        let content_end;
        let content_start = if a + 1 < ls.len() { ls[a + 1].start } else { ls[a].end };
        match close {
            Some(c) => {
                content_end = if c > a + 1 { ls[c - 1].end } else { ls[c].start };
                if brk_end(c) > ls[c].end {
                    edits.push((ls[c].start..brk_end(c), String::new()));
                } else {
                    edits.push((ls[c - 1].end..ls[c].end, String::new()));
                }
            }
            None => content_end = ls[b].end,
        }
        let plan = plan(text, edits)?;
        let selection = if sel.start == sel.end {
            let p = plan.map(sel.start, true);
            p..p
        } else {
            plan.map(content_start, true)..plan.map(content_end.max(content_start), false).max(plan.map(content_start, true))
        };
        return Some(plan.finish(text, selection, false));
    }
    let nl = eol(text);
    let prefix = &text[ls[first].start..ls[first].start + parse_line(&text[ls[first].clone()]).indent_end];
    let prefix: String = prefix.to_string();
    let run = text[ls[first].start..ls[last].end].split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let f = "`".repeat(run.max(2) + 1);
    let edits =
        vec![(ls[first].start..ls[first].start, format!("{prefix}{f}{nl}")), (ls[last].end..ls[last].end, format!("{nl}{prefix}{f}"))];
    let plan = plan(text, edits)?;
    let selection = if sel.start == sel.end {
        let p = if ls[first].is_empty() { ls[first].start + prefix.len() + f.len() + nl.len() } else { plan.map(sel.start, true) };
        p..p
    } else {
        plan.map(ls[first].start, true)..plan.map(ls[last].end, false)
    };
    Some(plan.finish(text, selection, false))
}

// ---------------------------------------------------------------------------------------------------------------
// Tasks

/// Set every task item the selection touches to checked (`[x]`) or unchecked (`[ ]`). Lines that are not tasks and
/// code are left alone. `None` when nothing changes. The selection stays as it was.
pub fn set_tasks(text: &str, sel: Range<usize>, checked: bool) -> Option<SelChange> {
    let sel = norm(text, sel);
    let ls = all_lines(text);
    let code = code_lines(text, &ls);
    let (first, last) = touched(&ls, &sel);
    let mut edits = Vec::new();
    for i in (first..=last).filter(|&i| !code[i]) {
        let l = &ls[i];
        let p = parse_line(&text[l.clone()]);
        if p.task.is_some() {
            let at = l.start + p.task_at;
            edits.push((at..at + 1, if checked { "x" } else { " " }.to_string()));
        }
    }
    let plan = plan(text, edits)?;
    Some(plan.finish(text, sel.clone(), false))
}

/// [`set_tasks`] with `true`.
pub fn check_tasks(text: &str, sel: Range<usize>) -> Option<SelChange> {
    set_tasks(text, sel, true)
}

/// [`set_tasks`] with `false`.
pub fn uncheck_tasks(text: &str, sel: Range<usize>) -> Option<SelChange> {
    set_tasks(text, sel, false)
}

/// Uncheck every task the selection touches when all of them are checked, else check them all. `None` when it touches no
/// task.
pub fn toggle_tasks(text: &str, sel: Range<usize>) -> Option<SelChange> {
    let s = norm(text, sel.clone());
    let ls = all_lines(text);
    let code = code_lines(text, &ls);
    let (first, last) = touched(&ls, &s);
    let tasks: Vec<bool> = (first..=last).filter(|&i| !code[i]).filter_map(|i| parse_line(&text[ls[i].clone()]).task).collect();
    if tasks.is_empty() {
        return None;
    }
    set_tasks(text, sel, !tasks.iter().all(|c| *c))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `a|b` marks the selection with `<` and `>` (a caret is `|`); returns (text, selection).
    fn parse(s: &str) -> (String, Range<usize>) {
        if !s.contains('<') {
            return match s.find('|') {
                Some(i) => (s.replacen('|', "", 1), i..i),
                None => (s.to_string(), 0..0),
            };
        }
        let a = s.find('<').unwrap();
        let t = s.replacen('<', "", 1);
        let b = t.rfind('>').unwrap();
        (format!("{}{}", &t[..b], &t[b + 1..]), a..b)
    }

    fn show(text: &str, sel: &Range<usize>) -> String {
        let mut s = text.to_string();
        if sel.start == sel.end {
            s.insert(sel.start, '|');
        } else {
            s.insert(sel.end, '>');
            s.insert(sel.start, '<');
        }
        s
    }

    fn run_in(s: &str, f: impl Fn(&str, Range<usize>) -> Option<SelChange>) -> Option<String> {
        let (t, sel) = parse(s);
        let c = f(&t, sel)?;
        let new = c.apply(&t);
        assert!(new.is_char_boundary(c.selection.start) && new.is_char_boundary(c.selection.end));
        assert_eq!(c.change.cursor, c.selection.end);
        Some(show(&new, &c.selection))
    }

    fn inl(s: &str, k: Inline) -> Option<String> {
        run_in(s, |t, r| inline(t, r, k))
    }

    fn lt(s: &str, k: Line) -> Option<String> {
        run_in(s, |t, r| line_type(t, r, k))
    }

    /// Applying twice returns the original (selection included).
    fn round_trip_inline(s: &str, k: Inline) {
        let (t, sel) = parse(s);
        let a = inline(&t, sel.clone(), k).unwrap();
        let t1 = a.apply(&t);
        let b = inline(&t1, a.selection.clone(), k).unwrap_or_else(|| panic!("second call None on {t1:?}"));
        assert_eq!(b.apply(&t1), t, "round trip of {s}");
    }

    fn round_trip_line(s: &str, k: Line) {
        let (t, sel) = parse(s);
        let a = line_type(&t, sel.clone(), k).unwrap();
        let t1 = a.apply(&t);
        let b = line_type(&t1, a.selection.clone(), k).unwrap_or_else(|| panic!("second call None on {t1:?}"));
        assert_eq!(b.apply(&t1), t, "round trip of {s}");
    }

    #[test]
    fn bold_on_a_selection_and_off_again() {
        assert_eq!(inl("a <b c> d", Inline::Bold).unwrap(), "a **<b c>** d");
        assert_eq!(inl("a **<b c>** d", Inline::Bold).unwrap(), "a <b c> d");
        round_trip_inline("a <b c> d", Inline::Bold);
        round_trip_inline("x <y> z", Inline::Italic);
        round_trip_inline("x <y> z", Inline::Strike);
        round_trip_inline("x <y> z", Inline::Code);
        round_trip_inline("x <y> z", Inline::Link);
    }

    #[test]
    fn every_kind_marks() {
        assert_eq!(inl("<a>", Inline::Italic).unwrap(), "*<a>*");
        assert_eq!(inl("<a>", Inline::Strike).unwrap(), "~~<a>~~");
        assert_eq!(inl("<a>", Inline::Code).unwrap(), "`<a>`");
        assert_eq!(inl("<a>", Inline::Link).unwrap(), "[a](|)");
    }

    #[test]
    fn blanks_at_the_ends_stay_outside() {
        assert_eq!(inl("a< b c >d", Inline::Bold).unwrap(), "a **<b c>** d");
    }

    #[test]
    fn caret_in_a_word_toggles_the_word() {
        assert_eq!(inl("one tw|o three", Inline::Bold).unwrap(), "one **tw|o** three");
        assert_eq!(inl("one **tw|o** three", Inline::Bold).unwrap(), "one tw|o three");
        assert_eq!(inl("one **two th|ree** x", Inline::Bold).unwrap(), "one two th|ree x");
        round_trip_inline("one tw|o three", Inline::Italic);
    }

    #[test]
    fn caret_without_a_word_puts_an_empty_pair() {
        assert_eq!(inl("a |", Inline::Bold).unwrap(), "a **|**");
        assert_eq!(inl("a **|**", Inline::Bold).unwrap(), "a |");
        assert_eq!(inl("|", Inline::Code).unwrap(), "`|`");
        assert_eq!(inl("`|`", Inline::Code).unwrap(), "|");
        assert_eq!(inl("|", Inline::Link).unwrap(), "[|]()");
        assert_eq!(inl("|", Inline::Italic).unwrap(), "*|*");
    }

    #[test]
    fn bold_and_italic_nest() {
        assert_eq!(inl("**<a>**", Inline::Italic).unwrap(), "***<a>***");
        assert_eq!(inl("***<a>***", Inline::Italic).unwrap(), "**<a>**");
        assert_eq!(inl("***<a>***", Inline::Bold).unwrap(), "*<a>*");
    }

    #[test]
    fn selection_that_includes_the_markers_takes_them_off() {
        assert_eq!(inl("x <**a**> y", Inline::Bold).unwrap(), "x <a> y");
        assert_eq!(inl("x <`a`> y", Inline::Code).unwrap(), "x <a> y");
    }

    #[test]
    fn one_wrap_per_line_and_prefixes_stay() {
        assert_eq!(inl("<- a\n- b\n- c>", Inline::Bold).unwrap(), "- **<a**\n- **b**\n- **c>**");
        assert_eq!(inl("<1. a\n2. b>", Inline::Italic).unwrap(), "1. *<a*\n2. *b>*");
        assert_eq!(inl("<> a\n> b>", Inline::Bold).unwrap(), "> **<a**\n> **b>**");
        assert_eq!(inl("<- [ ] a\n  - [x] b>", Inline::Strike).unwrap(), "- [ ] ~~<a~~\n  - [x] ~~b>~~");
        assert_eq!(inl("<# Title>", Inline::Bold).unwrap(), "# **<Title>**");
        round_trip_inline("<- a\n- b\n- c>", Inline::Bold);
        round_trip_inline("<> a\n> b>", Inline::Code);
        round_trip_inline("<1. a\n2. b>", Inline::Strike);
    }

    #[test]
    fn toggle_off_needs_every_line_marked() {
        // One line plain: the others stay, the plain one gets marked.
        assert_eq!(inl("<- **a**\n- b>", Inline::Bold).unwrap(), "- **<a**\n- **b>**");
        assert_eq!(inl("<- **a**\n- **b**>", Inline::Bold).unwrap(), "- <a\n- b>");
    }

    #[test]
    fn table_cells_are_wrapped_one_by_one() {
        assert_eq!(inl("<| a | b |>", Inline::Bold).unwrap(), "| **<a** | **b>** |");
        let t = "| a | b |
| - | - |
| x | y |";
        let c = inline(t, 0..t.len(), Inline::Bold).unwrap();
        assert_eq!(
            c.apply(t),
            "| **a** | **b** |
| - | - |
| **x** | **y** |"
        );
        // The delimiter row alone has nothing to mark.
        assert_eq!(
            inl(
                "| a | b |
<| - | - |>",
                Inline::Bold
            ),
            None
        );
    }

    #[test]
    fn caret_in_a_table_cell_marks_its_word() {
        let t = "| one | two |";
        let c = inline(t, 10..10, Inline::Bold).unwrap();
        assert_eq!(c.apply(t), "| one | **two** |");
    }

    #[test]
    fn nothing_inside_code() {
        assert_eq!(inl("`a <b> c`", Inline::Bold), None);
        assert_eq!(inl("`a| b`", Inline::Italic), None);
        assert_eq!(inl("```\n<a>\n```", Inline::Bold), None);
        assert_eq!(inl("```\n<a>\n```", Inline::Code), None);
        assert_eq!(inl("    <indented>", Inline::Bold), None);
        assert_eq!(inl("```\nf|oo\n```", Inline::Bold), None);
        // A selection over a fence skips the fenced lines.
        assert_eq!(inl("<a\n```\nx\n```\nb>", Inline::Bold).unwrap(), "**<a**\n```\nx\n```\n**b>**");
    }

    #[test]
    fn markers_inside_code_are_not_markup() {
        // The `**` inside the code span is not bold, so the word is wrapped.
        assert_eq!(inl("`**x**` and <y>", Inline::Bold).unwrap(), "`**x**` and **<y>**");
        // A whole code span can sit inside bold.
        assert_eq!(inl("<use `x` now>", Inline::Bold).unwrap(), "**<use `x` now>**");
    }

    #[test]
    fn code_span_toggles_off_with_a_caret_inside() {
        assert_eq!(inl("a `b|c` d", Inline::Code).unwrap(), "a b|c d");
        assert_eq!(inl("a <`bc`> d", Inline::Code).unwrap(), "a <bc> d");
        assert_eq!(inl("a ``<b`c>`` d", Inline::Code).unwrap(), "a <b`c> d");
    }

    #[test]
    fn backticks_in_the_text_get_a_longer_run() {
        assert_eq!(inl("<a`b>", Inline::Code).unwrap(), "``<a`b>``");
        assert_eq!(inl("<``a``>", Inline::Code).unwrap(), "<a>");
        assert_eq!(inl("<`a>", Inline::Code).unwrap(), "`` <`a> ``");
        round_trip_inline("<a`b>", Inline::Code);
        round_trip_inline("<`a>", Inline::Code);
        round_trip_inline("<a`>", Inline::Code);
    }

    #[test]
    fn link_shapes() {
        assert_eq!(inl("see <docs> now", Inline::Link).unwrap(), "see [docs](|) now");
        assert_eq!(inl("<https://example.com/a>", Inline::Link).unwrap(), "[|](https://example.com/a)");
        assert_eq!(inl("see [do|cs](http://x) now", Inline::Link).unwrap(), "see do|cs now");
        assert_eq!(inl("see <[docs](http://x)> now", Inline::Link).unwrap(), "see <docs> now");
        assert_eq!(inl("a |", Inline::Link).unwrap(), "a [|]()");
        assert_eq!(inl("wo|rd", Inline::Link).unwrap(), "[word](|)");
        // Several lines: no link.
        assert_eq!(inl("<a\nb>", Inline::Link), None);
    }

    #[test]
    fn japanese_text_and_flanking() {
        assert_eq!(inl("これは<強調>です", Inline::Bold).unwrap(), "これは**<強調>**です");
        let (t, sel) = parse("これは<強調>です");
        assert!(!inline(&t, sel, Inline::Bold).unwrap().unflanked);
        assert_eq!(inl("私は強|調です", Inline::Bold).unwrap(), "**私は強|調です**");
        // Closing marker after punctuation and before a letter.
        let (t, sel) = parse("<「強調」>です");
        let c = inline(&t, sel, Inline::Bold).unwrap();
        assert_eq!(c.apply(&t), "**「強調」**です");
        assert!(c.unflanked);
        // Opening marker next to punctuation after a letter.
        let (t, sel) = parse("私は<「強調」>");
        assert!(inline(&t, sel, Inline::Bold).unwrap().unflanked);
        // Fine with spaces around.
        let (t, sel) = parse("a <「強調」> b");
        assert!(!inline(&t, sel, Inline::Bold).unwrap().unflanked);
        // The unrendered form can still be taken off.
        assert_eq!(inl("**<「強調」>**です", Inline::Bold).unwrap(), "<「強調」>です");
        // Strike counts too.
        let (t, sel) = parse("<「強調」>です");
        assert!(inline(&t, sel, Inline::Strike).unwrap().unflanked);
    }

    #[test]
    fn cjk_bytes_are_whole_chars() {
        let (t, sel) = parse("日本<語の>文");
        let c = inline(&t, sel, Inline::Italic).unwrap();
        assert_eq!(c.apply(&t), "日本*語の*文");
        assert_eq!(&c.apply(&t)[c.selection.clone()], "語の");
        // A selection edge in the middle of a char snaps back.
        let c = inline("日本語", 1..9, Inline::Bold).unwrap();
        assert_eq!(c.apply("日本語"), "**日本語**");
    }

    #[test]
    fn crlf_is_kept() {
        let t = "- a\r\n- b\r\n";
        let c = inline(t, 0..9, Inline::Bold).unwrap();
        assert_eq!(c.apply(t), "- **a**\r\n- **b**\r\n");
        round_trip_inline("<> a\r\n> b>", Inline::Bold);
        let t = "x\r\n";
        let c = inline(t, 0..1, Inline::Code).unwrap();
        assert_eq!(c.apply(t), "`x`\r\n");
    }

    #[test]
    fn selection_over_a_line_end_with_cr() {
        let t = "ab\r\ncd";
        let c = inline(t, 0..3, Inline::Bold).unwrap();
        assert_eq!(c.apply(t), "**ab**\r\ncd");
    }

    #[test]
    fn text_end_and_empty_text() {
        assert_eq!(inl("abc\n|", Inline::Bold).unwrap(), "abc\n**|**");
        assert!(inline("", 0..0, Inline::Code).is_some());
    }

    // -----------------------------------------------------------------------------------------------------------

    #[test]
    fn headings() {
        assert_eq!(lt("a|b", Line::Heading(2)).unwrap(), "## a|b");
        assert_eq!(lt("## a|b", Line::Heading(2)).unwrap(), "a|b");
        assert_eq!(lt("# a|b", Line::Heading(3)).unwrap(), "### a|b");
        assert_eq!(lt("## a|b", Line::Heading(0)).unwrap(), "a|b");
        assert_eq!(lt("a", Line::Heading(7)), None);
        assert_eq!(lt("<a\nb>", Line::Heading(1)).unwrap(), "<# a\n# b>");
        assert_eq!(lt("<# a\n## b>", Line::Heading(2)).unwrap(), "<## a\n## b>");
        assert_eq!(lt("<## a\n## b>", Line::Heading(2)).unwrap(), "<a\nb>");
        assert_eq!(lt("> a|", Line::Heading(1)).unwrap(), "> # a|");
        assert_eq!(lt("- a|", Line::Heading(1)).unwrap(), "# a|");
        assert_eq!(lt("- [ ] a|", Line::Heading(0)).unwrap(), "a|");
        assert_eq!(lt("a|", Line::Heading(0)), None);
        round_trip_line("<a\nb>", Line::Heading(3));
    }

    #[test]
    fn quote() {
        assert_eq!(lt("a|", Line::Quote).unwrap(), "> a|");
        assert_eq!(lt("> a|", Line::Quote).unwrap(), "a|");
        assert_eq!(lt("<a\n\nb>", Line::Quote).unwrap(), "<> a\n>\n> b>");
        assert_eq!(lt("<> a\n>\n> b>", Line::Quote).unwrap(), "<a\n\nb>");
        assert_eq!(lt("<> a\nb>", Line::Quote).unwrap(), "<> a\n> b>");
        assert_eq!(lt("<> > a\n> b>", Line::Quote).unwrap(), "<> a\nb>");
        assert_eq!(lt("<- a\n  - b>", Line::Quote).unwrap(), "<> - a\n>   - b>");
        round_trip_line("<a\n\nb>", Line::Quote);
        round_trip_line("<- a\n  - b>", Line::Quote);
    }

    #[test]
    fn bullets_numbers_and_tasks_convert() {
        assert_eq!(lt("<a\nb>", Line::Bullet).unwrap(), "<- a\n- b>");
        assert_eq!(lt("<- a\n- b>", Line::Bullet).unwrap(), "<a\nb>");
        assert_eq!(lt("<- a\n- b>", Line::Number).unwrap(), "<1. a\n2. b>");
        assert_eq!(lt("<1. a\n2. b>", Line::Bullet).unwrap(), "<- a\n- b>");
        assert_eq!(lt("<- a\n- b>", Line::Task).unwrap(), "<- [ ] a\n- [ ] b>");
        assert_eq!(lt("<- [ ] a\n- [x] b>", Line::Task).unwrap(), "<a\nb>");
        assert_eq!(lt("<- [ ] a\n- [x] b>", Line::Bullet).unwrap(), "<- a\n- b>");
        assert_eq!(lt("<1. a\n2. b>", Line::Task).unwrap(), "<- [ ] a\n- [ ] b>");
        assert_eq!(lt("<- [x] a\n- [x] b>", Line::Number).unwrap(), "<1. a\n2. b>");
        assert_eq!(lt("<- a\n- [x] b>", Line::Task).unwrap(), "<- [ ] a\n- [x] b>");
        assert_eq!(lt("<* a\n* b>", Line::Task).unwrap(), "<* [ ] a\n* [ ] b>");
        assert_eq!(lt("<1) a\n2) b>", Line::Bullet).unwrap(), "<- a\n- b>");
        assert_eq!(lt("<a\n\nb>", Line::Bullet).unwrap(), "<- a\n\n- b>");
        assert_eq!(lt("# a|", Line::Bullet).unwrap(), "- a|");
        assert_eq!(lt("|", Line::Bullet).unwrap(), "- |");
    }

    #[test]
    fn numbers_are_consecutive_and_keep_the_start() {
        assert_eq!(lt("<a\nb\nc>", Line::Number).unwrap(), "<1. a\n2. b\n3. c>");
        assert_eq!(lt("<3. a\nb\nc>", Line::Number).unwrap(), "<3. a\n4. b\n5. c>");
        assert_eq!(lt("<1. a\nb\n7. c>", Line::Number).unwrap(), "<1. a\n2. b\n3. c>");
        assert_eq!(lt("<- a\n  - b\n  - c\n- d>", Line::Number).unwrap(), "<1. a\n  1. b\n  2. c\n2. d>");
        assert_eq!(
            lt("<1) a\nb>", Line::Number).unwrap(),
            "<1) a
2) b>"
        );
        round_trip_line("<a\nb\nc>", Line::Number);
        round_trip_line("<- a\n- b>", Line::Bullet);
        round_trip_line("<a\nb>", Line::Task);
    }

    #[test]
    fn indent_and_quote_prefix_stay_for_lists() {
        assert_eq!(lt("<  - a\n  - b>", Line::Number).unwrap(), "<  1. a\n  2. b>");
        assert_eq!(lt("<> a\n> b>", Line::Bullet).unwrap(), "<> - a\n> - b>");
        assert_eq!(lt("<> - a\n> - b>", Line::Task).unwrap(), "<> - [ ] a\n> - [ ] b>");
        assert_eq!(lt("<> - a\n> - b>", Line::Bullet).unwrap(), "<> a\n> b>");
        assert_eq!(lt("<  a\n  b>", Line::Bullet).unwrap(), "<  - a\n  - b>");
    }

    #[test]
    fn line_commands_skip_code() {
        assert_eq!(lt("<a\n```\nx\n```\nb>", Line::Bullet).unwrap(), "<- a\n```\nx\n```\n- b>");
        assert_eq!(lt("```\n<x>\n```", Line::Bullet), None);
        assert_eq!(lt("```\nx|\n```", Line::Heading(1)), None);
    }

    #[test]
    fn crlf_in_line_commands() {
        let t = "a\r\nb\r\n";
        let c = line_type(t, 0..4, Line::Number).unwrap();
        assert_eq!(c.apply(t), "1. a\r\n2. b\r\n");
        let c = line_type(t, 0..4, Line::Quote).unwrap();
        assert_eq!(c.apply(t), "> a\r\n> b\r\n");
        let c = line_type(t, 0..4, Line::Fence).unwrap();
        assert_eq!(c.apply(t), "```\r\na\r\nb\r\n```\r\n");
        let c2 = line_type(&c.apply(t), c.selection.clone(), Line::Fence).unwrap();
        assert_eq!(c2.apply(&c.apply(t)), t);
    }

    #[test]
    fn cjk_lines() {
        assert_eq!(lt("<日本語\n文章>", Line::Number).unwrap(), "<1. 日本語\n2. 文章>");
        assert_eq!(lt("日本|語", Line::Heading(1)).unwrap(), "# 日本|語");
    }

    #[test]
    fn fences() {
        assert_eq!(lt("<a\nb>", Line::Fence).unwrap(), "```\n<a\nb>\n```");
        assert_eq!(lt("```\n<a\nb>\n```", Line::Fence).unwrap(), "<a\nb>");
        assert_eq!(lt("```\na|\nb\n```\nafter", Line::Fence).unwrap(), "a|\nb\nafter");
        assert!(!lt(
            "<a
```
b
```>",
            Line::Fence
        )
        .unwrap()
        .contains(
            "```
```"
        ));
        assert_eq!(lt("<x ``` y>", Line::Fence).unwrap(), "````\n<x ``` y>\n````");
        assert_eq!(lt("|", Line::Fence).unwrap(), "```\n|\n```");
        assert_eq!(lt("> <a\n> b>", Line::Fence).unwrap(), "> ```\n<> a\n> b>\n> ```");
        let t = "> ```\n> a\n> b\n> ```";
        assert_eq!(line_type(t, 8..9, Line::Fence).unwrap().apply(t), "> a\n> b");
        assert_eq!(lt("```rust\nfn main() {}|\n```", Line::Fence).unwrap(), "fn main() {}|");
        // An unterminated fence only loses its opening line.
        assert_eq!(lt("```\nx|", Line::Fence).unwrap(), "x|");
        assert_eq!(lt("    code|", Line::Fence), None);
        round_trip_line("<a\nb>", Line::Fence);
        round_trip_line("<a>\n", Line::Fence);
        round_trip_line("<x ``` y>", Line::Fence);
        round_trip_line("> <a\n> b>\nrest", Line::Fence);
    }

    #[test]
    fn fence_without_trailing_break() {
        let t = "a";
        let c = line_type(t, 0..1, Line::Fence).unwrap();
        assert_eq!(c.apply(t), "```\na\n```");
    }

    // -----------------------------------------------------------------------------------------------------------

    fn tasks(s: &str, checked: bool) -> Option<String> {
        run_in(s, |t, r| set_tasks(t, r, checked))
    }

    #[test]
    fn tasks_in_a_selection() {
        assert_eq!(tasks("<- [ ] a\n- [x] b\n- plain\n- [ ] c>", true).unwrap(), "<- [x] a\n- [x] b\n- plain\n- [x] c>");
        assert_eq!(tasks("<- [ ] a\n- [x] b>", false).unwrap(), "<- [ ] a\n- [ ] b>");
        assert_eq!(tasks("<- [x] a>", true), None);
        assert_eq!(tasks("- [ ] a|", true).unwrap(), "- [x] a|");
        assert_eq!(tasks("<1. [ ] a\n   - [ ] b>", true).unwrap(), "<1. [x] a\n   - [x] b>");
        assert_eq!(tasks("<> - [ ] a\n> - [ ] b>", true).unwrap(), "<> - [x] a\n> - [x] b>");
        assert_eq!(tasks("<- [X] a>", false).unwrap(), "<- [ ] a>");
        assert_eq!(tasks("<- [ ] a\r\n- [ ] b\r\n>", true).unwrap(), "<- [x] a\r\n- [x] b\r\n>");
    }

    #[test]
    fn tasks_skip_code_and_other_lines() {
        assert_eq!(tasks("<- [ ] a\n```\n- [ ] b\n```\n- [ ] c>", true).unwrap(), "<- [x] a\n```\n- [ ] b\n```\n- [x] c>");
        assert_eq!(tasks("<```\n- [ ] b\n```>", true), None);
        assert_eq!(tasks("<[ ] not a list>", true), None);
        // The line after the selection is not touched.
        assert_eq!(tasks("<- [ ] a\n>- [ ] b", true).unwrap(), "<- [x] a\n>- [ ] b");
    }

    #[test]
    fn toggle_tasks_checks_then_unchecks() {
        let t = "- [x] a\n- [ ] b\n";
        let c = toggle_tasks(t, 0..t.len()).unwrap();
        assert_eq!(c.apply(t), "- [x] a\n- [x] b\n");
        let t2 = c.apply(t);
        let c = toggle_tasks(&t2, 0..t2.len()).unwrap();
        assert_eq!(c.apply(&t2), "- [ ] a\n- [ ] b\n");
        assert!(toggle_tasks("plain", 0..5).is_none());
    }
}
