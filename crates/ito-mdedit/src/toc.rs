use std::collections::HashMap;
use std::ops::Range;

use ito_outline::Heading;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::blocks::top_blocks;
use crate::{change_from_edits, lines, Change};

/// The line that opens a table of contents.
const START: &str = "<!-- toc -->";
/// The line that closes it.
const END: &str = "<!-- /toc -->";

fn marker(line: &str) -> Option<bool> {
    let inner = line.trim().strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    match inner {
        "toc" => Some(true),
        "/toc" => Some(false),
        _ => None,
    }
}

/// The byte ranges of the code blocks (a marker line inside one is only an example).
fn code_ranges(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = None;
    for (event, r) in Parser::new_ext(text, ito_outline::options()).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => start = Some(r.start),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(s) = start.take() {
                    out.push(s..r.end);
                }
            }
            _ => {}
        }
    }
    out
}

/// The existing table of contents: from the start of the opening marker line to the end of the closing marker line.
/// `Ok(None)` when there is none, `Err` when the opening marker has no closing one.
fn find(text: &str) -> Result<Option<Range<usize>>, ()> {
    let code = code_ranges(text);
    let ls = lines(text);
    let mut open = None;
    for l in &ls {
        if code.iter().any(|c| c.contains(&l.start)) {
            continue;
        }
        match (marker(&text[l.clone()]), open) {
            (Some(true), None) => open = Some(l.start),
            (Some(false), Some(s)) => return Ok(Some(s..l.end)),
            _ => {}
        }
    }
    if open.is_some() {
        Err(())
    } else {
        Ok(None)
    }
}

fn is_mark(c: char) -> bool {
    matches!(c, '\u{0300}'..='\u{036F}' | '\u{3099}'..='\u{309A}' | '\u{FE00}'..='\u{FE0F}')
}

/// The anchor GitHub gives a heading: lower case, everything but letters, numbers, `_`, `-` and spaces dropped, spaces
/// become `-`.
fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c == ' ' {
            out.push('-');
        } else if c.is_alphanumeric() || is_mark(c) || c == '_' || c == '-' {
            out.push(c);
        }
    }
    out
}

/// The anchors of all headings in order; a repeated one gets `-1`, `-2`, ...
fn anchors(hs: &[Heading]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::with_capacity(hs.len());
    for h in hs {
        let original = slug(&h.title);
        let mut name = original.clone();
        while seen.contains_key(&name) {
            let n = seen.entry(original.clone()).or_insert(0);
            *n += 1;
            name = format!("{original}-{n}");
        }
        seen.insert(name.clone(), 0);
        out.push(name);
    }
    out
}

fn escape(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if matches!(c, '\\' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The text of a table of contents for the headings of `hs` that are not in `skip`, markers included.
fn build(hs: &[Heading], skip: &dyn Fn(&Heading) -> bool, eol: &str) -> (String, usize) {
    let names = anchors(hs);
    let mut out = String::from(START);
    out.push_str(eol);
    let mut stack: Vec<u8> = Vec::new();
    let mut n = 0;
    for (h, name) in hs.iter().zip(&names) {
        if skip(h) || h.title.is_empty() {
            continue;
        }
        while stack.last().is_some_and(|&l| l >= h.level) {
            stack.pop();
        }
        out.push_str(&"  ".repeat(stack.len()));
        out.push_str(&format!("- [{}](#{name}){eol}", escape(&h.title)));
        stack.push(h.level);
        n += 1;
    }
    out.push_str(END);
    (out, n)
}

/// `true` when only blanks lie between the end of the heading and `anchor`: the heading is the table's own title.
fn titles(text: &str, h: &Heading, anchor: usize) -> bool {
    h.range.end <= anchor && text[h.range.end..anchor].trim().is_empty()
}

/// Make a table of contents: a list of links to the headings (GitHub anchors, indented by heading level), between
/// `<!-- toc -->` and `<!-- /toc -->`.
///
/// If the text already has one, it is rebuilt from the current headings, wherever the caret is (`None` when nothing
/// changes). Otherwise it is inserted after the block holding the caret (after the front matter when the caret is in
/// it; at the top when the caret is above every block), and the caret goes to its first line. The front matter and the
/// heading right above the table (its own title) are left out of the list. `None` when there are no headings to list,
/// or when a `<!-- toc -->` has no `<!-- /toc -->`.
pub fn insert_toc(text: &str, caret: usize) -> Option<Change> {
    let caret = caret.min(text.len());
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let hs = ito_outline::headings(text, 6);
    match find(text).ok()? {
        Some(region) => {
            let (new, _) = build(&hs, &|h| titles(text, h, region.start), eol);
            if text[region.clone()] == new {
                return None;
            }
            change_from_edits(text, &[(region, new)], caret)
        }
        None => {
            let blocks = top_blocks(text);
            let at = blocks.iter().rposition(|(r, _)| r.start <= caret).map(|i| blocks[i].0.end);
            let anchor = at.unwrap_or(0);
            let (toc, n) = build(&hs, &|h| at.is_some() && titles(text, h, anchor), eol);
            if n == 0 {
                return None;
            }
            let (lead, tail) = match at {
                Some(_) => (format!("{eol}{eol}"), String::new()),
                None if text.is_empty() => (String::new(), eol.to_string()),
                None => (String::new(), format!("{eol}{eol}")),
            };
            let cursor = anchor + lead.len();
            Some(Change { range: anchor..anchor, text: format!("{lead}{toc}{tail}"), cursor })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toc_is_inserted_below_the_block_with_the_caret() {
        let t = "# Doc\n\nintro\n\n## A\n\n### A1\n\n## B\n\nend\n";
        let at = t.find("intro").unwrap() + 2;
        let c = insert_toc(t, at).unwrap();
        let out = c.apply(t);
        assert_eq!(
            out,
            "# Doc\n\nintro\n\n<!-- toc -->\n- [Doc](#doc)\n  - [A](#a)\n    - [A1](#a1)\n  - [B](#b)\n<!-- /toc -->\n\n## A\n\n### A1\n\n## B\n\nend\n"
        );
        assert!(out[c.cursor..].starts_with("<!-- toc -->"));
    }

    #[test]
    fn the_heading_right_above_is_the_tocs_own_title_and_front_matter_is_not_a_heading() {
        let t = "---\ntitle: x\n# not\n---\n\n## 目次\n\n# One\n\n## Two\n";
        let c = insert_toc(t, t.find("目次").unwrap()).unwrap();
        let out = c.apply(t);
        assert!(out.contains("## 目次\n\n<!-- toc -->\n- [One](#one)\n  - [Two](#two)\n<!-- /toc -->\n\n# One"));
        assert!(!out.contains("(#目次)"));
    }

    #[test]
    fn anchors_are_github_style_and_duplicates_are_numbered() {
        let t = "intro\n\n# 見出し\n\n## 見出し\n\n## 見出し\n\n# C++ & Rust: Q&A?\n\n# a_b - c\n\n# 見出し-1\n";
        let out = insert_toc(t, 0).unwrap().apply(t);
        assert!(out.contains("- [見出し](#見出し)\n"));
        assert!(out.contains("  - [見出し](#見出し-1)\n"));
        assert!(out.contains("  - [見出し](#見出し-2)\n"));
        assert!(out.contains("- [C++ & Rust: Q&A?](#c--rust-qa)\n"));
        assert!(out.contains("- [a_b - c](#a_b---c)\n"));
        // The written `見出し-1` is taken already, as GitHub does it.
        assert!(out.contains("(#見出し-1-1)"));
    }

    #[test]
    fn an_existing_toc_is_rebuilt_wherever_the_caret_is() {
        let t = "# A\n\ntext\n\n<!-- toc -->\n- [old](#old)\n<!-- /toc -->\n\n## B\n";
        let c = insert_toc(t, t.len()).unwrap();
        let out = c.apply(t);
        assert_eq!(out, "# A\n\ntext\n\n<!-- toc -->\n- [A](#a)\n  - [B](#b)\n<!-- /toc -->\n\n## B\n");
        assert_eq!(c.cursor, out.len());
        // Up to date: nothing to do.
        assert!(insert_toc(&out, 0).is_none());
        // The caret inside it goes to the top of it.
        let at = t.find("old").unwrap();
        assert_eq!(insert_toc(t, at).unwrap().cursor, t.find("<!-- toc").unwrap());
    }

    #[test]
    fn crlf_is_kept_and_a_marker_in_a_code_block_is_an_example() {
        let t = "# A\r\n\r\ntext\r\n\r\n## B\r\n";
        let out = insert_toc(t, t.find("text").unwrap()).unwrap().apply(t);
        assert_eq!(out, "# A\r\n\r\ntext\r\n\r\n<!-- toc -->\r\n- [A](#a)\r\n  - [B](#b)\r\n<!-- /toc -->\r\n\r\n## B\r\n");
        let t = "# A\n\n```\n<!-- toc -->\n```\n\n## B\n";
        let out = insert_toc(t, 0).unwrap().apply(t);
        assert!(out.starts_with("# A\n\n<!-- toc -->\n- [B](#b)"));
    }

    #[test]
    fn nothing_to_list_an_open_marker_and_the_edges() {
        assert!(insert_toc("", 0).is_none());
        assert!(insert_toc("just text\n", 3).is_none());
        assert!(insert_toc("# A\n\n<!-- toc -->\n\n## B\n", 0).is_none());
        // Above the first block (and in an empty-headed text): at the top.
        let t = "\n\n# A\n";
        let out = insert_toc(t, 0).unwrap().apply(t);
        assert_eq!(out, "<!-- toc -->\n- [A](#a)\n<!-- /toc -->\n\n\n\n# A\n");
        // A setext title is skipped as the block above; a bracket in a title is escaped.
        let t = "Top\n===\n\n### Deep [x]\n";
        let out = insert_toc(t, 0).unwrap().apply(t);
        assert_eq!(out, "Top\n===\n\n<!-- toc -->\n- [Deep \\[x\\]](#deep-x)\n<!-- /toc -->\n\n### Deep [x]\n");
    }
}
