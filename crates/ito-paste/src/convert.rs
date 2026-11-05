//! HTML tree -> Markdown. Block elements become blocks joined by blank lines; everything
//! else is inline text with the Markdown specials escaped.

use crate::parse::{El, Node};

/// Stands for a `<br>` while inline text is built; turned into a hard break (or a space) at the end.
const BR: char = '\u{1}';

const BLOCKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "center",
    "details",
    "dd",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "html",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "ul",
];

/// Elements with no text a reader wants.
const SKIPPED: &[&str] = &["head", "script", "style", "template", "noscript", "title", "meta", "link", "svg", "select", "textarea"];

fn is_block(tag: &str) -> bool {
    BLOCKS.contains(&tag)
}

pub(crate) fn document(nodes: &[Node]) -> String {
    blocks(nodes, false)
}

/// Block children joined by blank lines. In a list item, a list sits right under its text.
fn blocks(nodes: &[Node], item: bool) -> String {
    let mut parts: Vec<(bool, String)> = Vec::new();
    let mut run: Vec<&Node> = Vec::new();
    for n in nodes {
        match n {
            Node::El(e) if SKIPPED.contains(&e.tag.as_str()) => {}
            Node::El(e) if is_block(&e.tag) => {
                flush(&mut run, &mut parts);
                block(e, &mut parts);
            }
            _ => run.push(n),
        }
    }
    flush(&mut run, &mut parts);
    let mut out = String::new();
    for (i, (is_list, text)) in parts.iter().enumerate() {
        if i > 0 {
            out.push_str(if item && (*is_list || parts[i - 1].0) { "\n" } else { "\n\n" });
        }
        out.push_str(text);
    }
    out
}

/// A run of inline nodes becomes one paragraph.
fn flush(run: &mut Vec<&Node>, parts: &mut Vec<(bool, String)>) {
    if run.is_empty() {
        return;
    }
    let text = flat(&inline_refs(run), "\\\n");
    run.clear();
    if text.is_empty() {
        return;
    }
    let text: Vec<String> = text.split('\n').map(escape_line_start).collect();
    parts.push((false, text.join("\n")));
}

fn block(e: &El, parts: &mut Vec<(bool, String)>) {
    let t = e.tag.as_str();
    match t {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let n = (t.as_bytes()[1] - b'0') as usize;
            let s = flat(&inline_nodes(&e.kids), " ");
            if !s.is_empty() {
                parts.push((false, format!("{} {s}", "#".repeat(n))));
            }
        }
        "ul" | "ol" => {
            let s = list(e);
            if !s.is_empty() {
                parts.push((true, s));
            }
        }
        "blockquote" => {
            let inner = blocks(&e.kids, false);
            if !inner.is_empty() {
                let quoted: Vec<String> =
                    inner.split('\n').map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect();
                parts.push((false, quoted.join("\n")));
            }
        }
        "pre" => {
            let s = code_block(e);
            if !s.is_empty() {
                parts.push((false, s));
            }
        }
        "hr" => parts.push((false, "---".to_string())),
        "table" => {
            let s = table(e);
            if !s.is_empty() {
                parts.push((false, s));
            }
        }
        _ => {
            let s = blocks(&e.kids, false);
            if !s.is_empty() {
                parts.push((false, s));
            }
        }
    }
}

fn list(e: &El) -> String {
    let ordered = e.tag == "ol";
    let mut n: usize = e.attr("start").and_then(|s| s.trim().parse().ok()).unwrap_or(1);
    let mut items: Vec<String> = Vec::new();
    for k in &e.kids {
        let Node::El(li) = k else { continue };
        if li.tag != "li" {
            continue;
        }
        let body = blocks(&li.kids, true);
        let marker = if ordered { format!("{n}. ") } else { "- ".to_string() };
        n += 1;
        let indent = " ".repeat(marker.len());
        let mut lines = body.split('\n');
        let mut s = format!("{marker}{}", lines.next().unwrap_or(""));
        for l in lines {
            s.push('\n');
            if !l.is_empty() {
                s.push_str(&indent);
                s.push_str(l);
            }
        }
        items.push(s.trim_end().to_string());
    }
    let loose = items.iter().any(|i| i.contains("\n\n"));
    items.join(if loose { "\n\n" } else { "\n" })
}

fn code_block(e: &El) -> String {
    let mut text = String::new();
    raw_text(&e.kids, &mut text);
    let text = text.replace("\r\n", "\n").replace('\u{a0}', " ");
    let text = text.strip_prefix('\n').unwrap_or(&text).trim_end_matches(['\n', ' ']);
    if text.trim().is_empty() {
        return String::new();
    }
    let lang = e
        .kids
        .iter()
        .find_map(|n| match n {
            Node::El(c) if c.tag == "code" => c.attr("class"),
            _ => None,
        })
        .and_then(|class| class.split_whitespace().find_map(|w| w.strip_prefix("language-").or_else(|| w.strip_prefix("lang-"))))
        .unwrap_or("");
    let fence = "`".repeat((longest_run(text, '`') + 1).max(3));
    format!("{fence}{lang}\n{text}\n{fence}")
}

fn raw_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::El(e) if e.tag == "br" => out.push('\n'),
            Node::El(e) if SKIPPED.contains(&e.tag.as_str()) => {}
            Node::El(e) => raw_text(&e.kids, out),
        }
    }
}

fn longest_run(s: &str, c: char) -> usize {
    let (mut best, mut cur) = (0, 0);
    for x in s.chars() {
        if x == c {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

fn collect_rows<'a>(nodes: &'a [Node], rows: &mut Vec<&'a El>) {
    for n in nodes {
        if let Node::El(e) = n {
            match e.tag.as_str() {
                "tr" => rows.push(e),
                "thead" | "tbody" | "tfoot" => collect_rows(&e.kids, rows),
                _ => {}
            }
        }
    }
}

fn align_of(c: &El) -> &'static str {
    let a = c.attr("align").map(str::to_ascii_lowercase).or_else(|| {
        let style = c.attr("style")?.to_ascii_lowercase().replace(' ', "");
        let i = style.find("text-align:")?;
        Some(style[i + 11..].split([';', '"']).next().unwrap_or("").to_string())
    });
    match a.as_deref() {
        Some("left") => ":---",
        Some("center") => ":---:",
        Some("right") => "---:",
        _ => "---",
    }
}

/// A GFM table; the first row is the header. A table of one cell is only a layout box.
fn table(e: &El) -> String {
    let mut rows = Vec::new();
    collect_rows(&e.kids, &mut rows);
    let cells: Vec<Vec<&El>> = rows
        .iter()
        .map(|r| {
            r.kids
                .iter()
                .filter_map(|n| match n {
                    Node::El(c) if c.tag == "td" || c.tag == "th" => Some(c),
                    _ => None,
                })
                .collect()
        })
        .collect();
    let cols = cells.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    if cells.len() == 1 && cols == 1 {
        return blocks(&cells[0][0].kids, false);
    }
    let mut lines = Vec::new();
    for (ri, row) in cells.iter().enumerate() {
        let mut v: Vec<String> = row.iter().map(|c| flat(&inline_nodes(&c.kids), "<br>").replace('|', "\\|")).collect();
        v.resize(cols, String::new());
        lines.push(format!("| {} |", v.join(" | ")));
        if ri == 0 {
            let mut d: Vec<&str> = row.iter().map(|c| align_of(c)).collect();
            d.resize(cols, "---");
            lines.push(format!("| {} |", d.join(" | ")));
        }
    }
    lines.join("\n")
}

fn inline_nodes(nodes: &[Node]) -> String {
    inline_refs(&nodes.iter().collect::<Vec<_>>())
}

fn inline_refs(nodes: &[&Node]) -> String {
    let mut out = String::new();
    for n in nodes {
        match n {
            Node::Text(t) => push_inline(&mut out, &escape(&collapse(t))),
            Node::El(e) => push_inline(&mut out, &inline_el(e)),
        }
    }
    out
}

/// Appends without doubling a space at the join (`"a "` + `" b"`) or leaving one beside a break.
fn push_inline(out: &mut String, s: &str) {
    if (out.ends_with(' ') || out.ends_with(BR)) && s.starts_with(' ') {
        out.push_str(&s[1..]);
    } else {
        out.push_str(s);
    }
}

fn inline_el(e: &El) -> String {
    let t = e.tag.as_str();
    if SKIPPED.contains(&t) {
        return String::new();
    }
    match t {
        "br" => BR.to_string(),
        "b" | "strong" => wrap(&inline_nodes(&e.kids), "**"),
        "i" | "em" | "cite" | "dfn" | "var" => wrap(&inline_nodes(&e.kids), "*"),
        "del" | "s" | "strike" => wrap(&inline_nodes(&e.kids), "~~"),
        "code" | "kbd" | "samp" | "tt" => {
            let mut s = String::new();
            raw_text(&e.kids, &mut s);
            code_span(&s)
        }
        "a" => link(e),
        "img" => image(e),
        "input" if e.attr("type").is_some_and(|ty| ty.eq_ignore_ascii_case("checkbox")) => {
            if e.attr("checked").is_some() { "[x] " } else { "[ ] " }.to_string()
        }
        _ if is_block(t) => format!(" {} ", inline_nodes(&e.kids)),
        _ => inline_nodes(&e.kids),
    }
}

/// `**text**`, with the spaces at either end kept outside the marks.
fn wrap(inner: &str, mark: &str) -> String {
    let t = inner.trim_matches(|c| c == ' ' || c == BR);
    if t.is_empty() {
        return inner.to_string();
    }
    let lead = if inner.starts_with(' ') { " " } else { "" };
    let trail = if inner.ends_with(' ') { " " } else { "" };
    format!("{lead}{mark}{t}{mark}{trail}")
}

fn code_span(s: &str) -> String {
    let s = s.replace(['\n', '\r'], " ").replace('\u{a0}', " ");
    if s.trim().is_empty() {
        return String::new();
    }
    let fence = "`".repeat(longest_run(&s, '`') + 1);
    let pad = if s.starts_with(['`', ' ']) || s.ends_with(['`', ' ']) { " " } else { "" };
    format!("{fence}{pad}{s}{pad}{fence}")
}

fn link(e: &El) -> String {
    let text = inline_nodes(&e.kids);
    let href = e.attr("href").unwrap_or("").trim();
    let lower = href.to_ascii_lowercase();
    let plain = href.is_empty() || href.starts_with('#') || ["javascript:", "vbscript:", "data:"].iter().any(|p| lower.starts_with(p));
    if plain {
        return text;
    }
    let shown = text.trim();
    if shown.is_empty() {
        return format!("<{}>", dest(href));
    }
    let autolink = lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:");
    if autolink && shown == escape(href) {
        return format!("<{}>", dest(href));
    }
    let lead = if text.starts_with(' ') { " " } else { "" };
    let trail = if text.ends_with(' ') { " " } else { "" };
    format!("{lead}[{shown}]({}){trail}", dest(href))
}

/// A link target that cannot end the link early.
fn dest(href: &str) -> String {
    href.replace(' ', "%20").replace('(', "%28").replace(')', "%29").replace('<', "%3C").replace('>', "%3E")
}

fn image(e: &El) -> String {
    let alt = e.attr("alt").unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    let alt = escape(&alt);
    let src = e.attr("src").unwrap_or("").trim();
    if src.is_empty() || src.to_ascii_lowercase().starts_with("data:") {
        return alt;
    }
    format!("![{alt}]({})", dest(src))
}

/// Runs of white space (and the no-break space) become one space; zero-width marks go.
pub(crate) fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
                space = true;
            }
        } else if c != '\u{200b}' && c != '\u{feff}' && c != BR {
            out.push(c);
            space = false;
        }
    }
    out
}

/// Escapes what would turn into Markdown inside a line. An underscore inside a word is left alone.
pub(crate) fn escape(s: &str) -> String {
    let ch: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    for (i, &c) in ch.iter().enumerate() {
        let prev = if i > 0 { Some(ch[i - 1]) } else { None };
        let next = ch.get(i + 1).copied();
        match c {
            '\\' | '*' | '`' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            '_' => {
                let inside = prev.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric);
                if !inside {
                    out.push('\\');
                }
                out.push('_');
            }
            '<' if next.is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?')) => out.push_str("\\<"),
            '~' if prev == Some('~') || next == Some('~') => out.push_str("\\~"),
            _ => out.push(c),
        }
    }
    out
}

/// Ends of the text trimmed and each break replaced with `br` (a hard break, a space or `<br>`).
fn flat(s: &str, br: &str) -> String {
    let mut t = s.trim_matches(|c| c == ' ' || c == BR).replace(" \u{1}", "\u{1}").replace("\u{1} ", "\u{1}");
    while t.contains("\u{1}\u{1}") {
        t = t.replace("\u{1}\u{1}", "\u{1}");
    }
    t.replace(BR, br)
}

/// A line of a paragraph must not start like a heading, a quote, a list or a rule.
fn escape_line_start(line: &str) -> String {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    let heading = (1..=6).contains(&hashes) && (line.len() == hashes || line[hashes..].starts_with(' '));
    let quote = line.starts_with('>');
    let bullet = line == "-" || line == "+" || line.starts_with("- ") || line.starts_with("+ ");
    let rule = !line.is_empty() && (line.chars().all(|c| c == '-') || line.chars().all(|c| c == '='));
    if heading || quote || bullet || rule {
        return format!("\\{line}");
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits) && matches!(line[digits..].chars().next(), Some('.' | ')')) {
        let after = &line[digits + 1..];
        if after.is_empty() || after.starts_with(' ') {
            return format!("{}\\{}", &line[..digits], &line[digits..]);
        }
    }
    line.to_string()
}
