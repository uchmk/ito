//! The pure parts of a Markdown writing box: wrapping a selection for Ctrl+B / I / U and the toolbar, a small Markdown
//! reader ([`blocks`], [`spans`]) and its two outputs, HTML ([`to_html`]) and plain text ([`to_plain`]).
//! Underline has no Markdown, so it is the tag `<u>…</u>`, kept as typed and read back as [`Span::underline`].
//! Cut out of minato's composer. With the feature `egui`: [`Md`] (the preview painter) and [`Writing`] (the box).

#[cfg(feature = "egui")]
mod edit;
#[cfg(feature = "egui")]
mod paint;
#[cfg(feature = "egui")]
pub use edit::{Out, Writing};
#[cfg(feature = "egui")]
pub use paint::{layout, Md};

/// `text` with `left` and `right` put around the characters `a..b` (counted
/// in chars). Returns the new text and the selection, still on the same
/// characters.
pub fn wrap(text: &str, a: usize, b: usize, left: &str, right: &str) -> (String, usize, usize) {
    let (a, b) = (a.min(b), a.max(b));
    let chars: Vec<char> = text.chars().collect();
    let b = b.min(chars.len());
    let a = a.min(b);
    let mut out: String = chars[..a].iter().collect();
    out.push_str(left);
    out.extend(&chars[a..b]);
    out.push_str(right);
    out.extend(&chars[b..]);
    let n = left.chars().count();
    (out, a + n, b + n)
}

/// A link around the selection; the selection moves to the empty `()`
/// so the address can be typed.
pub fn link(text: &str, a: usize, b: usize) -> (String, usize, usize) {
    let (out, _, e) = wrap(text, a, b, "[", "](url)");
    // Select the word `url`.
    (out, e + 2, e + 5)
}

/// A button of the writing box's toolbar (GitHub's, less the slash).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Heading,
    Bold,
    Italic,
    Quote,
    Code,
    Link,
    Bullets,
    Numbers,
    Tasks,
    Mention,
    Reference,
    /// `<u>…</u>` (Markdown has no underline).
    Underline,
    /// `~~…~~`.
    Strike,
}

impl Tool {
    pub const ALL: [Tool; 13] = [
        Tool::Heading,
        Tool::Bold,
        Tool::Italic,
        Tool::Quote,
        Tool::Code,
        Tool::Link,
        Tool::Bullets,
        Tool::Numbers,
        Tool::Tasks,
        Tool::Mention,
        Tool::Reference,
        Tool::Underline,
        Tool::Strike,
    ];
}

/// What a toolbar button does to `text` with the characters `a..b`
/// selected: the new text and selection.
pub fn tool(text: &str, a: usize, b: usize, t: Tool) -> (String, usize, usize) {
    let (a, b) = (a.min(b), a.max(b));
    match t {
        Tool::Bold => wrap(text, a, b, "**", "**"),
        Tool::Italic => wrap(text, a, b, "_", "_"),
        Tool::Underline => wrap(text, a, b, "<u>", "</u>"),
        Tool::Strike => wrap(text, a, b, "~~", "~~"),
        Tool::Link => link(text, a, b),
        Tool::Code if text.chars().skip(a).take(b - a).any(|c| c == '\n') => wrap(text, a, b, "```\n", "\n```"),
        Tool::Code => wrap(text, a, b, "`", "`"),
        Tool::Mention => wrap(text, b, b, "@", ""),
        Tool::Reference => wrap(text, b, b, "#", ""),
        Tool::Heading => lines(text, a, b, |_| "### ".into()),
        Tool::Quote => lines(text, a, b, |_| "> ".into()),
        Tool::Bullets => lines(text, a, b, |_| "- ".into()),
        Tool::Numbers => lines(text, a, b, |k| format!("{}. ", k + 1)),
        Tool::Tasks => lines(text, a, b, |_| "- [ ] ".into()),
    }
}

/// Enter pressed with the cursor at char `at` in a list item (`- `, `* `, `- [ ] `, `1. `): the text and cursor
/// after the line breaks and the next item's marker (a number goes up by one, a task starts unticked) is put
/// there. Enter on an item with nothing in it takes the marker away instead, which ends the list. `None`: the
/// line is not an item, or the cursor is inside its marker, so the field does its plain newline.
pub fn continue_list(text: &str, at: usize) -> Option<(String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let at = at.min(chars.len());
    let start = chars[..at].iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1);
    let end = chars[at..].iter().position(|&c| c == '\n').map_or(chars.len(), |i| at + i);
    let line: String = chars[start..end].iter().collect();
    let indent_len = line.chars().take_while(|&c| c == ' ' || c == '\t').count();
    let indent: String = line.chars().take(indent_len).collect();
    let rest: String = line.chars().skip(indent_len).collect();
    let (marker, next) = if let Some(m) = ["- [ ] ", "- [x] ", "- [X] "].iter().find(|m| rest.starts_with(**m)) {
        (m.to_string(), "- [ ] ".to_string())
    } else if rest.starts_with("- ") || rest.starts_with("* ") {
        (rest[..2].to_string(), rest[..2].to_string())
    } else {
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 || !rest[digits..].starts_with(". ") {
            return None;
        }
        let n: u64 = rest[..digits].parse().ok()?;
        (rest[..digits + 2].to_string(), format!("{}. ", n + 1))
    };
    let body_from = start + indent_len + marker.chars().count();
    if at < body_from {
        return None;
    }
    if chars[body_from..end].iter().all(|c| c.is_whitespace()) {
        // An empty item: the marker goes, the line is left blank.
        let mut out: String = chars[..start].iter().collect();
        out.extend(&chars[end..]);
        return Some((out, start));
    }
    let mut out: String = chars[..at].iter().collect();
    let ins = format!("\n{indent}{next}");
    out.push_str(&ins);
    out.extend(&chars[at..]);
    Some((out, at + ins.chars().count()))
}

/// `mark(k)` put at the start of each line the selection touches (the k-th
/// of them); the selection moves to keep the same text.
fn lines(text: &str, a: usize, b: usize, mark: impl Fn(usize) -> String) -> (String, usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let b = b.min(chars.len());
    let a = a.min(b);
    let start = chars[..a].iter().rposition(|&c| c == '\n').map_or(0, |i| i + 1);
    // The starts of the lines from `start` to the one `b` is on.
    let mut starts = vec![start];
    starts.extend((start..b).filter(|&i| chars[i] == '\n').map(|i| i + 1));
    let mut out = String::new();
    let (mut na, mut nb) = (a, b);
    let mut from = 0;
    for (k, &at) in starts.iter().enumerate() {
        out.extend(&chars[from..at]);
        let m = mark(k);
        let n = m.chars().count();
        out.push_str(&m);
        if at <= a {
            na += n;
        }
        nb += n;
        from = at;
    }
    out.extend(&chars[from..]);
    (out, na, nb)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, String),
    Bullet(String),
    /// A numbered item: its number as written, and the text.
    Numbered(String, String),
    /// A task: whether it is ticked, and the text.
    Task(bool, String),
    Quote(String),
    Code(String),
    /// A table's rows, the cells of each (the line under the head dropped).
    Table(Vec<Vec<String>>),
    /// `---`.
    Rule,
    /// The `<summary>` of a `<details>`: what follows is shown open under it.
    Summary(String),
    Para(String),
}

/// A small reading of Markdown: headings, bullets, numbered items, tasks,
/// quotes, fenced code, tables, rules and paragraphs. HTML comments (issue
/// templates leave them) and lines that are only a tag are dropped; a
/// `<summary>` is a block of its own, and the tags GitHub draws inline
/// (`<b>`, `<code>`, `<a href>`, `<br>` …) become their Markdown. Inline
/// marks stay as typed; [`spans`] reads them.
pub fn blocks(text: &str) -> Vec<Block> {
    read(text, " ")
}

/// [`blocks`] with the line breaks inside a paragraph kept instead of folded into spaces.
pub fn blocks_hard(text: &str) -> Vec<Block> {
    read(text, &BR.to_string())
}

fn read(text: &str, sep: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    let mut para: Vec<&str> = Vec::new();
    let mut table: Vec<Vec<String>> = Vec::new();
    let flush = |para: &mut Vec<&str>, table: &mut Vec<Vec<String>>, out: &mut Vec<Block>| {
        if !para.is_empty() {
            out.push(Block::Para(para.join(sep)));
            para.clear();
        }
        if !table.is_empty() {
            out.push(Block::Table(std::mem::take(table)));
        }
    };
    let text = without_html(&without_comments(text));
    for line in text.lines() {
        if let Some(buf) = &mut code {
            if line.trim_start().starts_with("```") {
                out.push(Block::Code(buf.join("\n")));
                code = None;
            } else {
                buf.push(line);
            }
            continue;
        }
        let t = line.trim();
        if t.starts_with('|') {
            if !para.is_empty() {
                out.push(Block::Para(para.join(sep)));
                para.clear();
            }
            let cells: Vec<String> = t.trim_matches('|').split('|').map(|c| c.trim().to_string()).collect();
            // The line under the head (`|---|:--:|`) only says how to align.
            if !cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':'))) {
                table.push(cells);
            }
            continue;
        }
        if let Some(s) = t.strip_prefix("<summary>").and_then(|r| r.strip_suffix("</summary>")) {
            flush(&mut para, &mut table, &mut out);
            out.push(Block::Summary(s.trim().to_string()));
        } else if t.starts_with("```") {
            flush(&mut para, &mut table, &mut out);
            code = Some(Vec::new());
        } else if t.is_empty() || is_tag(t) {
            flush(&mut para, &mut table, &mut out);
        } else if let Some(h) = heading(t) {
            flush(&mut para, &mut table, &mut out);
            out.push(h);
        } else if t.len() >= 3 && ['-', '*', '_'].iter().any(|&m| t.chars().all(|c| c == m)) {
            flush(&mut para, &mut table, &mut out);
            out.push(Block::Rule);
        } else if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
            flush(&mut para, &mut table, &mut out);
            let rest = rest.trim();
            let task =
                [("[ ] ", false), ("[x] ", true), ("[X] ", true)].into_iter().find_map(|(m, on)| rest.strip_prefix(m).map(|r| (on, r)));
            out.push(match task {
                Some((on, r)) => Block::Task(on, r.trim().to_string()),
                None => Block::Bullet(rest.to_string()),
            });
        } else if let Some((n, rest)) = numbered(t) {
            flush(&mut para, &mut table, &mut out);
            out.push(Block::Numbered(n, rest));
        } else if let Some(rest) = t.strip_prefix('>') {
            flush(&mut para, &mut table, &mut out);
            out.push(Block::Quote(rest.trim().to_string()));
        } else {
            if !table.is_empty() {
                out.push(Block::Table(std::mem::take(&mut table)));
            }
            para.push(t);
        }
    }
    if let Some(buf) = code {
        out.push(Block::Code(buf.join("\n")));
    }
    flush(&mut para, &mut table, &mut out);
    for b in &mut out {
        match b {
            Block::Heading(_, s)
            | Block::Bullet(s)
            | Block::Numbered(_, s)
            | Block::Task(_, s)
            | Block::Quote(s)
            | Block::Summary(s)
            | Block::Para(s) => {
                *s = s.replace(BR, "\n");
            }
            Block::Table(rows) => rows.iter_mut().flatten().for_each(|s| *s = s.replace(BR, "\n")),
            Block::Code(_) | Block::Rule => {}
        }
    }
    out
}

/// `text` without its `<!-- … -->`.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        match rest[i..].find("-->") {
            Some(j) => rest = &rest[i + j + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `text` with each `<summary>…</summary>` on a line of its own (whatever
/// was around it on the line goes on lines before and after) and the inline
/// tags made Markdown; fenced code is left as it is.
fn without_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.lines() {
        let fence = line.trim_start().starts_with("```");
        fenced ^= fence;
        if fence || fenced {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let low = line.to_ascii_lowercase();
        let open = low.find("<summary").and_then(|i| low[i..].find('>').map(|j| (i, i + j + 1)));
        match open {
            Some((i, a)) => {
                let (inner, after) = match low[a..].find("</summary>") {
                    Some(j) => (&line[a..a + j], &line[a + j + "</summary>".len()..]),
                    None => (&line[a..], ""),
                };
                let inner = html_inline(inner).replace(BR, " ");
                for l in [html_inline(&line[..i]), format!("<summary>{}</summary>", inner.trim()), html_inline(after)] {
                    out.push_str(&l);
                    out.push('\n');
                }
            }
            None => {
                out.push_str(&html_inline(line));
                out.push('\n');
            }
        }
    }
    out
}

/// One line's inline HTML as Markdown: `<b>` `<strong>` bold, `<i>` `<em>`
/// italic, `<code>` `<kbd>` code, `<s>` `<del>` struck, `<a href>` a link,
/// `<img alt>` its alt, `<br>` a new line; the tags that only shape the page
/// (`<details>`, `<p>`, `<div>`, `<sub>` …) go. Other `<…>` (`Vec<T>`) and
/// what is in backticks stay as typed.
fn html_inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut code = false;
    let mut href: Vec<Option<String>> = Vec::new();
    let mut rest = line;
    while let Some(ch) = rest.chars().next() {
        if ch == '`' {
            code = !code;
        }
        if ch == '<' && !code {
            if let Some((len, close, name, tag)) = tag_at(rest) {
                let md = match name.as_str() {
                    "b" | "strong" => "**".to_string(),
                    "i" | "em" => "*".to_string(),
                    "code" | "kbd" | "samp" | "tt" => "`".to_string(),
                    "s" | "del" | "strike" => "~~".to_string(),
                    "u" | "ins" => (if close { "</u>" } else { "<u>" }).to_string(),
                    "br" => BR.to_string(),
                    "img" => attr(tag, "alt").unwrap_or_default(),
                    "a" if !close => {
                        let h = attr(tag, "href");
                        let open = if h.is_some() { "[" } else { "" };
                        href.push(h);
                        open.to_string()
                    }
                    "a" => match href.pop().flatten() {
                        Some(h) => format!("]({h})"),
                        None => String::new(),
                    },
                    _ => String::new(),
                };
                out.push_str(&md);
                rest = &rest[len..];
                continue;
            }
        }
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

/// A `<br>` until the lines are read, then a new line in its block.
const BR: char = '\u{2028}';

/// The tags [`html_inline`] knows.
const TAGS: [&str; 31] = [
    "a", "b", "strong", "i", "em", "code", "kbd", "samp", "tt", "s", "del", "strike", "br", "img", "details", "summary", "p", "div",
    "span", "sub", "sup", "ins", "u", "small", "picture", "source", "center", "ul", "ol", "li", "hr",
];

/// A known tag at the start of `s`: its length in bytes, whether it closes,
/// its name in lower case, and the whole tag.
fn tag_at(s: &str) -> Option<(usize, bool, String, &str)> {
    let end = s.find('>')?;
    let tag = &s[..=end];
    let inner = tag[1..end].trim_end_matches('/').trim();
    let (close, inner) = match inner.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, inner),
    };
    let name: String = inner.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let after = &inner[name.len()..];
    let known = TAGS.contains(&name.as_str()) && (after.is_empty() || after.starts_with(char::is_whitespace));
    known.then_some((end + 1, close, name, tag))
}

/// The value of `name="…"` (or `'…'`) in a tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let low = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find(name).map(|i| i + from) {
        from = i + name.len();
        let before_ok = low[..i].ends_with(char::is_whitespace);
        let rest = low[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let at = tag.len() - rest.len() + 1;
        let v = tag[at..].trim_start();
        let q = v.chars().next()?;
        return if q == '"' || q == '\'' {
            v[1..].find(q).map(|j| v[1..1 + j].to_string())
        } else {
            Some(v.chars().take_while(|c| !c.is_whitespace() && *c != '>').collect())
        };
    }
    None
}

/// A line that is only an HTML tag (`<details>`, `</summary>`, `<br/>`).
fn is_tag(t: &str) -> bool {
    t.starts_with('<')
        && t.ends_with('>')
        && t.matches('<').count() == 1
        && t[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/')
}

/// `12. text` as its number and text.
fn numbered(t: &str) -> Option<(String, String)> {
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    let rest = t[digits..].strip_prefix(". ").or_else(|| t[digits..].strip_prefix(") "))?;
    (digits > 0 && digits < 10).then(|| (t[..digits].to_string(), rest.trim().to_string()))
}

/// A run of text with the same inline marks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub underline: bool,
    pub code: bool,
    /// `![alt](src)`: `text` is `[alt]` and `link` is the source.
    pub image: bool,
    /// Where a link goes.
    pub link: Option<String>,
}

/// The inline marks of one block: `**bold**` / `__bold__`, `_italic_` /
/// `*italic*`, `~~struck~~`, `` `code` ``, `[text](url)` and `![alt](url)`
/// (shown as its alt). A mark that is never closed stays as typed.
pub fn spans(s: &str) -> Vec<Span> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out: Vec<Span> = Vec::new();
    let mut cur = Span::default();
    fn push(out: &mut Vec<Span>, cur: &mut Span) {
        if !cur.text.is_empty() {
            let next = Span { text: String::new(), ..cur.clone() };
            out.push(std::mem::replace(cur, next));
        }
    }
    let closes = |from: usize, mark: &[char]| (from..n).any(|j| chars[j..].starts_with(mark));
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let rest = &chars[i..];
        if c == '\\' && i + 1 < n && chars[i + 1].is_ascii_punctuation() {
            cur.text.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '`' {
            if let Some(j) = (i + 1..n).find(|&j| chars[j] == '`') {
                push(&mut out, &mut cur);
                out.push(Span { text: chars[i + 1..j].iter().collect(), code: true, ..cur.clone() });
                i = j + 1;
                continue;
            }
        }
        let image = c == '!' && rest.get(1) == Some(&'[');
        if c == '[' || image {
            let open = i + usize::from(image);
            let close = (open + 1..n).find(|&j| chars[j] == ']');
            let end = close.filter(|&k| chars.get(k + 1) == Some(&'(')).and_then(|k| (k + 2..n).find(|&j| chars[j] == ')').map(|e| (k, e)));
            if let Some((close, end)) = end {
                push(&mut out, &mut cur);
                let label: String = chars[open + 1..close].iter().collect();
                let url: String = chars[close + 2..end].iter().collect();
                let text = if image { format!("[{}]", if label.is_empty() { "image" } else { &label }) } else { label };
                out.push(Span { text, link: Some(url), image, ..cur.clone() });
                i = end + 1;
                continue;
            }
        }
        if c == '<' {
            let open: [char; 3] = ['<', 'u', '>'];
            let shut: [char; 4] = ['<', '/', 'u', '>'];
            if rest.starts_with(&open) && (cur.underline || closes(i + 3, &shut)) {
                push(&mut out, &mut cur);
                cur.underline = true;
                i += 3;
                continue;
            }
            if cur.underline && rest.starts_with(&shut) {
                push(&mut out, &mut cur);
                cur.underline = false;
                i += 4;
                continue;
            }
        }
        let pair = rest.get(..2).filter(|p| p[0] == p[1] && matches!(p[0], '*' | '_' | '~'));
        if let Some(pair) = pair {
            let strike = pair[0] == '~';
            let on = if strike { cur.strike } else { cur.bold };
            if on || closes(i + 2, pair) {
                push(&mut out, &mut cur);
                if strike {
                    cur.strike = !on;
                } else {
                    cur.bold = !on;
                }
            } else {
                cur.text.extend(pair);
            }
            i += 2;
            continue;
        }
        if c == '*' || c == '_' {
            let inside = c == '_' && i > 0 && chars[i - 1].is_alphanumeric() && chars.get(i + 1).is_some_and(|x| x.is_alphanumeric());
            let opens = chars.get(i + 1).is_some_and(|x| !x.is_whitespace()) && closes(i + 1, &[c]);
            if !inside && (cur.italic || opens) {
                push(&mut out, &mut cur);
                cur.italic = !cur.italic;
                i += 1;
                continue;
            }
        }
        cur.text.push(c);
        i += 1;
    }
    push(&mut out, &mut cur);
    out
}

/// HTML-safe text.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A link target that cannot run script.
fn safe_url(u: &str) -> String {
    let low: String = u.chars().filter(|c| !c.is_whitespace() && !c.is_control()).collect::<String>().to_ascii_lowercase();
    if ["javascript:", "vbscript:", "data:"].iter().any(|p| low.starts_with(p)) {
        "#".to_owned()
    } else {
        esc(u.trim())
    }
}

fn inline_html(s: &str, image: &dyn Fn(&str) -> String) -> String {
    let mut out = String::new();
    for sp in spans(s) {
        let mut t = if sp.image {
            let alt = sp.text.trim_start_matches('[').trim_end_matches(']');
            let src = image(sp.link.as_deref().unwrap_or(""));
            out.push_str(&format!("<img src=\"{}\" alt=\"{}\">", safe_url(&src), esc(alt)));
            continue;
        } else {
            esc(&sp.text).replace('\n', "<br>")
        };
        if sp.code {
            t = format!("<code>{t}</code>");
        }
        if sp.bold {
            t = format!("<strong>{t}</strong>");
        }
        if sp.italic {
            t = format!("<em>{t}</em>");
        }
        if sp.underline {
            t = format!("<u>{t}</u>");
        }
        if sp.strike {
            t = format!("<s>{t}</s>");
        }
        if let Some(url) = &sp.link {
            t = format!("<a href=\"{}\">{t}</a>", safe_url(url));
        }
        out.push_str(&t);
    }
    out
}

#[derive(PartialEq, Clone, Copy)]
enum Open {
    None,
    Ul,
    Ol,
    Quote,
}

fn shut(out: &mut String, open: &mut Open) {
    out.push_str(match open {
        Open::Ul => "</ul>\n",
        Open::Ol => "</ol>\n",
        Open::Quote => "</blockquote>\n",
        Open::None => "",
    });
    *open = Open::None;
}

fn begin(out: &mut String, open: &mut Open, want: Open) {
    if *open != want {
        shut(out, open);
        out.push_str(match want {
            Open::Ul => "<ul>\n",
            Open::Ol => "<ol>\n",
            Open::Quote => "<blockquote>\n",
            Open::None => "",
        });
        *open = want;
    }
}

/// Markdown as HTML (for a mail body): headings, lists (task items as `☐` / `☑`), quotes, code, tables, rules and
/// paragraphs, with a single line break inside a paragraph kept as `<br>`. Text is escaped, and a `javascript:` link
/// becomes `#`. `image` turns an image's source into the `src` to write (`cid:…`, a data URL …); pass
/// `&|s| s.to_owned()` to leave it.
pub fn to_html(text: &str, image: &dyn Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut open = Open::None;
    for b in blocks_hard(text) {
        match &b {
            Block::Bullet(s) => {
                begin(&mut out, &mut open, Open::Ul);
                out.push_str(&format!("<li>{}</li>\n", inline_html(s, image)));
            }
            Block::Task(on, s) => {
                begin(&mut out, &mut open, Open::Ul);
                out.push_str(&format!("<li>{} {}</li>\n", if *on { "\u{2611}" } else { "\u{2610}" }, inline_html(s, image)));
            }
            Block::Numbered(_, s) => {
                begin(&mut out, &mut open, Open::Ol);
                out.push_str(&format!("<li>{}</li>\n", inline_html(s, image)));
            }
            Block::Quote(s) => {
                begin(&mut out, &mut open, Open::Quote);
                out.push_str(&format!("<p>{}</p>\n", inline_html(s, image)));
            }
            Block::Heading(n, s) => {
                shut(&mut out, &mut open);
                out.push_str(&format!("<h{n}>{}</h{n}>\n", inline_html(s, image)));
            }
            Block::Summary(s) => {
                shut(&mut out, &mut open);
                out.push_str(&format!("<p><strong>{}</strong></p>\n", inline_html(s, image)));
            }
            Block::Para(s) => {
                shut(&mut out, &mut open);
                out.push_str(&format!("<p>{}</p>\n", inline_html(s, image)));
            }
            Block::Code(c) => {
                shut(&mut out, &mut open);
                out.push_str(&format!("<pre><code>{}</code></pre>\n", esc(c)));
            }
            Block::Rule => {
                shut(&mut out, &mut open);
                out.push_str("<hr>\n");
            }
            Block::Table(rows) => {
                shut(&mut out, &mut open);
                out.push_str("<table>\n");
                for (i, r) in rows.iter().enumerate() {
                    let tag = if i == 0 { "th" } else { "td" };
                    out.push_str("<tr>");
                    for c in r {
                        out.push_str(&format!("<{tag}>{}</{tag}>", inline_html(c, image)));
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</table>\n");
            }
        }
    }
    shut(&mut out, &mut open);
    out
}

fn inline_plain(s: &str) -> String {
    let mut out = String::new();
    for sp in spans(s) {
        match &sp.link {
            Some(url) if !sp.image && *url != sp.text => out.push_str(&format!("{} ({url})", sp.text)),
            _ => out.push_str(&sp.text),
        }
    }
    out
}

/// Markdown as plain text (for the text part of a mail): the marks are dropped, `- `, `1.` and `> ` stay, a link is
/// `text (url)`; blocks are apart by a blank line and the lines of a list or quote are together.
pub fn to_plain(text: &str) -> String {
    let mut out = String::new();
    let mut prev = "";
    for b in blocks_hard(text) {
        let (kind, lines): (&str, Vec<String>) = match &b {
            Block::Heading(_, s) | Block::Summary(s) | Block::Para(s) => ("para", vec![inline_plain(s)]),
            Block::Bullet(s) => ("list", vec![format!("- {}", inline_plain(s))]),
            Block::Numbered(n, s) => ("list", vec![format!("{n}. {}", inline_plain(s))]),
            Block::Task(on, s) => ("list", vec![format!("- [{}] {}", if *on { "x" } else { " " }, inline_plain(s))]),
            Block::Quote(s) => ("quote", vec![format!("> {}", inline_plain(s))]),
            Block::Code(c) => ("code", vec![c.clone()]),
            Block::Rule => ("rule", vec!["---".to_owned()]),
            Block::Table(rows) => {
                ("table", rows.iter().map(|r| r.iter().map(|c| inline_plain(c)).collect::<Vec<_>>().join(" | ")).collect())
            }
        };
        if !out.is_empty() {
            out.push_str(if prev == kind && (kind == "list" || kind == "quote") { "\n" } else { "\n\n" });
        }
        out.push_str(&lines.join("\n"));
        prev = kind;
    }
    out
}

fn heading(t: &str) -> Option<Block> {
    let n = t.chars().take_while(|&c| c == '#').count();
    let rest = t[n..].strip_prefix(' ')?;
    (1..=6).contains(&n).then(|| Block::Heading(n as u8, rest.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_goes_on_with_the_list() {
        let c = |t: &str| continue_list(t, t.chars().count());
        assert_eq!(c("- a"), Some(("- a\n- ".into(), 6)));
        assert_eq!(c("* a"), Some(("* a\n* ".into(), 6)));
        assert_eq!(c("- [ ] a"), Some(("- [ ] a\n- [ ] ".into(), 14)));
        assert_eq!(c("- [x] a"), Some(("- [x] a\n- [ ] ".into(), 14)));
        assert_eq!(c("1. a\n2. b"), Some(("1. a\n2. b\n3. ".into(), 13)));
        assert_eq!(c("9. a").unwrap().0, "9. a\n10. ");
        assert_eq!(c("  - a").unwrap().0, "  - a\n  - ");
        assert_eq!(c("plain"), None);
        assert_eq!(c("1.5 x"), None);
        // An empty item ends the list.
        assert_eq!(c("- a\n- "), Some(("- a\n".into(), 4)));
        assert_eq!(c("1. a\n2. "), Some(("1. a\n".into(), 5)));
        // Inside the marker: the plain newline. In the middle of the text: the rest moves down.
        assert_eq!(continue_list("- a", 1), None);
        assert_eq!(continue_list("- ab", 3), Some(("- a\n- b".into(), 6)));
    }

    #[test]
    fn wrap_keeps_the_selection_on_the_same_characters() {
        let (t, a, b) = wrap("say hello now", 4, 9, "**", "**");
        assert_eq!(t, "say **hello** now");
        assert_eq!(&t[a..b], "hello");
        let (t, a, b) = wrap("日本語", 1, 2, "_", "_");
        assert_eq!(t, "日_本_語");
        assert_eq!((a, b), (2, 3));
    }

    #[test]
    fn wrap_with_nothing_selected_puts_the_marks_together() {
        assert_eq!(wrap("ab", 1, 1, "*", "*"), ("a**b".to_string(), 2, 2));
        assert_eq!(wrap("ab", 9, 9, "*", "*").0, "ab**");
    }

    #[test]
    fn a_link_selects_the_address_word() {
        let (t, a, b) = link("see docs", 4, 8);
        assert_eq!(t, "see [docs](url)");
        assert_eq!(t.chars().skip(a).take(b - a).collect::<String>(), "url");
    }

    #[test]
    fn markdown_blocks_are_read() {
        let got = blocks("# Title\nsome\ntext\n\n- one\n* two\n> said\n```\nlet a;\n\nb\n```\nend");
        assert_eq!(
            got,
            vec![
                Block::Heading(1, "Title".into()),
                Block::Para("some text".into()),
                Block::Bullet("one".into()),
                Block::Bullet("two".into()),
                Block::Quote("said".into()),
                Block::Code("let a;\n\nb".into()),
                Block::Para("end".into()),
            ]
        );
        assert_eq!(blocks("#nospace"), vec![Block::Para("#nospace".into())]);
        assert_eq!(blocks("```\nopen"), vec![Block::Code("open".into())]);
    }

    #[test]
    fn markdown_lists_tables_and_comments() {
        let got = blocks(
            "<!-- template\nhint -->\n1. one\n2) two\n- [ ] todo\n- [x] done\n---\n<details>\n| a | b |\n|---|:-:|\n| 1 | 2 |\nafter",
        );
        assert_eq!(
            got,
            vec![
                Block::Numbered("1".into(), "one".into()),
                Block::Numbered("2".into(), "two".into()),
                Block::Task(false, "todo".into()),
                Block::Task(true, "done".into()),
                Block::Rule,
                Block::Table(vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into()]]),
                Block::Para("after".into()),
            ]
        );
        assert_eq!(blocks("a <!-- open"), vec![Block::Para("a".into())]);
    }

    #[test]
    fn details_and_inline_html() {
        let got = blocks(
            "<details><summary>Last <b>24</b> runs</summary>\n\n- one\n</details>\n<p align=\"center\">a<br>b <code>x<y</code></p>\n",
        );
        assert_eq!(got, vec![Block::Summary("Last **24** runs".into()), Block::Bullet("one".into()), Block::Para("a\nb `x<y`".into())]);
        let got = blocks("<details>\n<summary>Log</summary>\n\n```\n<b>kept</b>\n```\n</details>");
        assert_eq!(got, vec![Block::Summary("Log".into()), Block::Code("<b>kept</b>".into())]);
        assert_eq!(html_inline("see <a href=\"https://x/1\">#1</a> and <img src=x alt='logo'>"), "see [#1](https://x/1) and logo");
        assert_eq!(html_inline("Vec<T> and `<b>` stay"), "Vec<T> and `<b>` stay");
        assert_eq!(html_inline("a <unknown> <a>plain</a>"), "a <unknown> plain");
    }

    #[test]
    fn inline_marks_are_read() {
        let s = spans("say **bold** and _it_ with `co*de*` and [link](https://x.test) ~~no~~");
        let plain: Vec<(&str, bool, bool, bool, bool)> = s.iter().map(|x| (x.text.as_str(), x.bold, x.italic, x.code, x.strike)).collect();
        assert_eq!(
            plain,
            vec![
                ("say ", false, false, false, false),
                ("bold", true, false, false, false),
                (" and ", false, false, false, false),
                ("it", false, true, false, false),
                (" with ", false, false, false, false),
                ("co*de*", false, false, true, false),
                (" and ", false, false, false, false),
                ("link", false, false, false, false),
                (" ", false, false, false, false),
                ("no", false, false, false, true),
            ]
        );
        assert_eq!(s[7].link.as_deref(), Some("https://x.test"));
        // Unclosed marks and snake_case stay as typed.
        let t: String = spans("a ** b and snake_case_name and 2 * 3").iter().map(|x| x.text.clone()).collect();
        assert_eq!(t, "a ** b and snake_case_name and 2 * 3");
        assert_eq!(spans("![shot](u.png)")[0].text, "[shot]");
        assert_eq!(spans("\\*not\\*")[0].text, "*not*");
    }

    #[test]
    fn the_toolbar_marks_lines_and_words() {
        assert_eq!(tool("ab", 0, 2, Tool::Bold).0, "**ab**");
        assert_eq!(tool("ab", 1, 1, Tool::Mention), ("a@b".to_string(), 2, 2));
        assert_eq!(tool("a\nb", 0, 3, Tool::Code).0, "```\na\nb\n```");
        assert_eq!(tool("one\ntwo\nthree", 1, 5, Tool::Numbers), ("1. one\n2. two\nthree".to_string(), 4, 11));
        assert_eq!(tool("x\ny", 2, 2, Tool::Quote), ("x\n> y".to_string(), 4, 4));
        assert_eq!(tool("", 0, 0, Tool::Tasks), ("- [ ] ".to_string(), 6, 6));
        assert_eq!(tool("hi", 2, 2, Tool::Heading).0, "### hi");
    }

    #[test]
    fn underline_and_strike() {
        assert_eq!(tool("ab", 0, 2, Tool::Underline).0, "<u>ab</u>");
        assert_eq!(tool("ab", 0, 2, Tool::Strike).0, "~~ab~~");
        let s = spans("a <u>b **c**</u> d <u>open");
        let got: Vec<(&str, bool, bool)> = s.iter().map(|x| (x.text.as_str(), x.underline, x.bold)).collect();
        assert_eq!(got, vec![("a ", false, false), ("b ", true, false), ("c", true, true), (" d <u>open", false, false)]);
        assert_eq!(html_inline("x <ins>y</ins> <u>z</u>"), "x <u>y</u> <u>z</u>");
    }

    #[test]
    fn html_output() {
        let keep = |s: &str| s.to_owned();
        let h = to_html("# T\nline one\nline two\n\n- a\n- [x] b\n\n> q\n> r\n\n1. x\n\n```\n<b>&\n```\n---\n| h |\n|---|\n| c |", &keep);
        assert_eq!(
            h,
            "<h1>T</h1>\n<p>line one<br>line two</p>\n<ul>\n<li>a</li>\n<li>\u{2611} b</li>\n</ul>\n<blockquote>\n<p>q</p>\n<p>r</p>\n</blockquote>\n\
<ol>\n<li>x</li>\n</ol>\n<pre><code>&lt;b&gt;&amp;</code></pre>\n<hr>\n<table>\n<tr><th>h</th></tr>\n<tr><td>c</td></tr>\n</table>\n"
        );
        let h = to_html("**b** _i_ <u>u</u> ~~s~~ `c` [l](https://x/?a=1&b=2) ![alt](pic.png) <script>", &|s| format!("cid:{s}"));
        assert_eq!(
            h,
            "<p><strong>b</strong> <em>i</em> <u>u</u> <s>s</s> <code>c</code> <a href=\"https://x/?a=1&amp;b=2\">l</a> \
<img src=\"cid:pic.png\" alt=\"alt\"> &lt;script&gt;</p>\n"
        );
        assert!(to_html("[x](javascript:alert(1))", &keep).contains("href=\"#\""));
        assert!(to_html("[x](JaVa\tScript:1)", &keep).contains("href=\"#\""));
    }

    #[test]
    fn plain_output() {
        let t = to_plain(
            "# Title\nsome **bold**\ntext\n\n- a\n- [ ] b\n1. c\n> q\n\n[site](https://x.test) and [https://y.test](https://y.test)\n---",
        );
        assert_eq!(t, "Title\n\nsome bold\ntext\n\n- a\n- [ ] b\n1. c\n\n> q\n\nsite (https://x.test) and https://y.test\n\n---");
    }
}
