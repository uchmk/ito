//! The structure checks: headings, lists, tables, fences, trailing spaces, links.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use ito_mdedit::Change;

use crate::scan::{self, Class, Item, Line, Scan};
use crate::{Kind, Sink};

pub(crate) fn run(text: &str, sc: &Scan, exists: &dyn Fn(&str) -> bool, sink: &mut Sink) {
    headings(text, sc, sink);
    lists(text, sc, sink);
    tables(text, sc, sink);
    trailing(text, sc, sink);
    links(text, sc, exists, sink);
    if let Some(run) = &sc.unclosed {
        if sink.on(Kind::UnclosedFence) {
            let ch = &text[run.clone()][..1];
            let len = run.len();
            let mut add = String::new();
            if !text.is_empty() && !text.ends_with('\n') {
                add.push_str(sc.eol);
            }
            add.push_str(&ch.repeat(len));
            add.push_str(sc.eol);
            let end = text.len();
            let cursor = end + add.len();
            sink.add(Kind::UnclosedFence, run.clone(), Some(Change { range: end..end, text: add, cursor }));
        }
    }
}

fn norm_text(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn headings(text: &str, sc: &Scan, sink: &mut Sink) {
    let mut prev: Option<u8> = None;
    let mut stack: Vec<(u8, usize)> = Vec::new();
    let mut seen: HashSet<(usize, u8, String)> = HashSet::new();
    let mut next_id = 1;
    for line in &sc.lines {
        let (level, marker, title, atx) = match &line.class {
            Class::Atx { level, marker, text: t } => (*level, marker.clone(), t.clone(), true),
            Class::Setext { level, text: t } => (*level, t.clone(), t.clone(), false),
            _ => continue,
        };
        let empty = title.is_empty();
        if empty {
            sink.add(Kind::EmptyHeading, line.range.clone(), None);
        }
        let mut effective = level;
        if let Some(p) = prev {
            if level > p + 1 {
                effective = p + 1;
                if atx {
                    sink.replace(Kind::HeadingJump, marker.clone(), "#".repeat(effective as usize));
                } else {
                    sink.add(Kind::HeadingJump, marker.clone(), None);
                }
            }
        }
        prev = Some(effective);
        if !empty && sink.on(Kind::DuplicateHeading) {
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            let parent = stack.last().map_or(0, |(_, id)| *id);
            if !seen.insert((parent, level, norm_text(&text[title.clone()]))) {
                sink.add(Kind::DuplicateHeading, title.clone(), None);
            }
            stack.push((level, next_id));
            next_id += 1;
        }
    }
}

struct Open {
    indent: usize,
    content: usize,
    ordered: bool,
    mch: char,
}

fn lists(text: &str, sc: &Scan, sink: &mut Sink) {
    let mut stack: Vec<Open> = Vec::new();
    for line in &sc.lines {
        match &line.class {
            Class::Item(it) => item(it, &mut stack, sink),
            Class::Blank | Class::Code | Class::Front => {}
            _ => {
                let cols = scan::leading(&text[line.range.clone()]).0;
                while stack.last().is_some_and(|t| t.content > cols) {
                    stack.pop();
                }
            }
        }
    }
}

fn item(it: &Item, stack: &mut Vec<Open>, sink: &mut Sink) {
    if it.indent >= 4 && stack.is_empty() {
        return; // indented code
    }
    while stack.last().is_some_and(|t| t.indent > it.indent) {
        stack.pop();
    }
    let sibling = match stack.last() {
        Some(top) if top.indent == it.indent => Some(top.indent),
        Some(top) if it.indent < top.content => {
            if it.spaces_only {
                sink.replace(Kind::UnevenListIndent, it.indent_range.clone(), " ".repeat(top.indent));
            } else {
                sink.add(Kind::UnevenListIndent, it.indent_range.clone(), None);
            }
            Some(top.indent)
        }
        _ => None,
    };
    match sibling {
        None => stack.push(Open { indent: it.indent, content: it.content, ordered: it.ordered, mch: it.mch }),
        Some(_) => {
            let top = stack.last_mut().expect("a sibling has a list");
            if top.ordered == it.ordered {
                if top.mch != it.mch {
                    let r = if it.ordered { it.marker.end - 1..it.marker.end } else { it.marker.clone() };
                    sink.replace(Kind::MixedListMarker, r, top.mch.to_string());
                }
            } else {
                top.ordered = it.ordered;
                top.mch = it.mch;
            }
            top.content = it.content;
        }
    }
}

/// The number of cells of a table row, and whether it ends in an unescaped `|`.
fn cells(row: &str) -> (usize, bool) {
    let t = row.trim();
    let b = t.as_bytes();
    let unescaped_pipe_at = |i: usize| b[i] == b'|' && b[..i].iter().rev().take_while(|c| **c == b'\\').count() % 2 == 0;
    let ends = !b.is_empty() && unescaped_pipe_at(b.len() - 1);
    let start = usize::from(b.first() == Some(&b'|'));
    let end = if ends && b.len() > start { b.len() - 1 } else { b.len() };
    if start > end {
        return (1, ends);
    }
    let n = (start..end).filter(|&i| unescaped_pipe_at(i)).count() + 1;
    (n, ends)
}

fn is_delimiter_row(row: &str) -> bool {
    let t = row.trim();
    if !t.contains('|') || t.is_empty() {
        return false;
    }
    let inner = t.trim_start_matches('|');
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').all(|c| {
        let c = c.trim();
        let c = c.strip_prefix(':').unwrap_or(c);
        let c = c.strip_suffix(':').unwrap_or(c);
        !c.is_empty() && c.bytes().all(|b| b == b'-')
    })
}

fn tables(text: &str, sc: &Scan, sink: &mut Sink) {
    if !sink.on(Kind::TableCellCount) {
        return;
    }
    let lines: &[Line] = &sc.lines;
    let mut i = 0;
    while i + 1 < lines.len() {
        let is_para = |l: &Line| l.class == Class::Para && scan::leading(&text[l.range.clone()]).0 < 4;
        let (h, d) = (&lines[i], &lines[i + 1]);
        let (hs, ds) = (&text[h.range.clone()], &text[d.range.clone()]);
        if is_para(h) && is_para(d) && hs.contains('|') && is_delimiter_row(ds) {
            let (n, _) = cells(hs);
            let (dn, _) = cells(ds);
            if dn != n {
                sink.add(Kind::TableCellCount, d.range.clone(), None);
                i += 2;
                continue;
            }
            let mut j = i + 2;
            while j < lines.len() && is_para(&lines[j]) {
                let r = lines[j].range.clone();
                let row = &text[r.clone()];
                let (c, ends) = cells(row);
                if c != n {
                    let fix = if c < n && ends {
                        let at = r.start + row.trim_end().len();
                        let add = " |".repeat(n - c);
                        let cursor = at + add.len();
                        Some(Change { range: at..at, text: add, cursor })
                    } else {
                        None
                    };
                    sink.add(Kind::TableCellCount, r, fix);
                }
                j += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

fn trailing(text: &str, sc: &Scan, sink: &mut Sink) {
    if !sink.on(Kind::TrailingSpaces) {
        return;
    }
    for (i, line) in sc.lines.iter().enumerate() {
        if matches!(line.class, Class::Code | Class::Front) {
            continue;
        }
        let s = &text[line.range.clone()];
        let trimmed = s.trim_end_matches([' ', '\t']);
        let n = s.len() - trimmed.len();
        if n == 0 {
            continue;
        }
        let spaces_only = !s[trimmed.len()..].contains('\t');
        let continues = sc.lines.get(i + 1).is_some_and(|l| l.class != Class::Blank);
        if n == 2 && spaces_only && !trimmed.is_empty() && continues && !matches!(line.class, Class::Atx { .. }) {
            continue; // a hard line break
        }
        let r = line.range.start + trimmed.len()..line.range.end;
        sink.replace(Kind::TrailingSpaces, r, "");
    }
}

/// Is `dest` a relative path (no scheme, not `#frag`, not `//host`)? The path decoded, without fragment and query.
fn local_path(dest: &str) -> Option<String> {
    if dest.starts_with('#') || dest.starts_with("//") {
        return None;
    }
    if let Some(colon) = dest.find(':') {
        let scheme = &dest[..colon];
        if !scheme.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        {
            return None;
        }
    }
    let end = dest.find(['#', '?']).unwrap_or(dest.len());
    let p = &dest[..end];
    if p.is_empty() {
        return None;
    }
    Some(percent_decode(p))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn links(text: &str, sc: &Scan, exists: &dyn Fn(&str) -> bool, sink: &mut Sink) {
    let want_target = sink.on(Kind::MissingTarget);
    let want_ref = sink.on(Kind::UndefinedReference);
    if !want_target && !want_ref {
        return;
    }
    let prose = |l: &Line| !matches!(l.class, Class::Code | Class::Front | Class::Blank | Class::Hr | Class::SetextUnderline);
    let mut defs: HashSet<String> = HashSet::new();
    if want_ref {
        for l in sc.lines.iter().filter(|l| prose(l)) {
            if let Some((label, _)) = scan::ref_def(&text[l.range.clone()]) {
                defs.insert(label);
            }
        }
    }
    let mut cache: HashMap<String, bool> = HashMap::new();
    for l in sc.lines.iter().filter(|l| prose(l)) {
        let s = &text[l.range.clone()];
        let base = l.range.start;
        let spans = scan::code_spans(s);
        if want_target {
            let mut dests: Vec<Range<usize>> = scan::link_dests(s, &spans);
            if let Some((_, rest)) = scan::ref_def(s) {
                let r = &s[rest.clone()];
                let t = r.trim_start();
                let start = rest.start + (r.len() - t.len());
                let t = t.strip_prefix('<').map_or(t, |x| x.split('>').next().unwrap_or(x));
                let word = t.split_whitespace().next().unwrap_or("");
                let off = if s[start..].starts_with('<') { start + 1 } else { start };
                if !word.is_empty() {
                    dests.push(off..off + word.len());
                }
            }
            for d in dests {
                if let Some(p) = local_path(&s[d.clone()]) {
                    let ok = *cache.entry(p.clone()).or_insert_with(|| exists(&p));
                    if !ok {
                        sink.add(Kind::MissingTarget, base + d.start..base + d.end, None);
                    }
                }
            }
        }
        if want_ref {
            let b = s.as_bytes();
            let mut i = 0;
            while i + 1 < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if b[i] == b']' && b[i + 1] == b'[' && !spans.iter().any(|r| r.contains(&i)) {
                    if let Some(close) = s[i + 2..].find(']') {
                        let label = &s[i + 2..i + 2 + close];
                        if !label.is_empty() && !label.starts_with('^') && !defs.contains(&scan::normalize_label(label)) {
                            sink.add(Kind::UndefinedReference, base + i + 2..base + i + 2 + close, None);
                        }
                        i += 2 + close;
                        continue;
                    }
                }
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{fixed, of, run};
    use crate::*;

    fn ranges(text: &str, kind: Kind) -> Vec<String> {
        of(text, kind).iter().map(|f| text[f.range.clone()].to_string()).collect()
    }

    #[test]
    fn heading_jump_and_fix() {
        let t = "# A\n\n### B\n\n#### C\n";
        assert_eq!(ranges(t, Kind::HeadingJump), vec!["###", "####"]);
        assert_eq!(fixed(t, Kind::HeadingJump), "# A\n\n## B\n\n### C\n");
        // Going up or down by one is fine, and the first heading may be any level.
        assert!(of("### A\n## B\n# C\n## D\n", Kind::HeadingJump).is_empty());
        // After a fix the next heading is judged against the fixed level.
        assert_eq!(fixed("# A\n#### B\n##### C\n######\n", Kind::HeadingJump), "# A\n## B\n### C\n####\n");
    }

    #[test]
    fn heading_jump_setext_has_no_fix() {
        let t = "Title\n=====\n\nSub\n---\n\n#### Deep\n";
        assert!(of(t, Kind::HeadingJump).len() == 1);
        let t2 = "# A\n\nB\n---\n\nC\n=\n";
        assert!(of(t2, Kind::HeadingJump).is_empty());
        let t3 = "# A\n\nB\n===\n";
        assert!(of(t3, Kind::HeadingJump).is_empty());
        let t4 = "# A\n### B\n";
        assert!(of(t4, Kind::HeadingJump)[0].fix.is_some());
    }

    #[test]
    fn empty_heading() {
        assert_eq!(ranges("#\n## \ntext\n# ok\n", Kind::EmptyHeading), vec!["#", "## "]);
        assert!(of("# a #\n#hashtag\n", Kind::EmptyHeading).is_empty());
        assert_eq!(ranges("# #\n", Kind::EmptyHeading), vec!["# #"]);
    }

    #[test]
    fn duplicate_heading_under_one_parent() {
        let t = "# A\n## Same\n## Same\n# B\n## Same\n";
        let f = of(t, Kind::DuplicateHeading);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].range, 15..19);
        // Case and spacing do not make a difference; different levels do.
        assert_eq!(of("# A\n## x  y\n## X Y\n### x y\n", Kind::DuplicateHeading).len(), 1);
        // The same title under different parents is fine, and at the top level it is a duplicate.
        assert_eq!(of("# T\n# T\n", Kind::DuplicateHeading).len(), 1);
        assert!(of("# A\n## x\n# B\n## x\n", Kind::DuplicateHeading).is_empty());
        // A heading in a code fence does not count.
        assert!(of("# A\n```\n# A\n```\n", Kind::DuplicateHeading).is_empty());
    }

    #[test]
    fn uneven_list_indent() {
        let t = "- a\n  - b\n   - c\n  - d\n";
        assert_eq!(ranges(t, Kind::UnevenListIndent), vec!["   "]);
        assert_eq!(fixed(t, Kind::UnevenListIndent), "- a\n  - b\n  - c\n  - d\n");
        // A sibling at one space under a top-level list.
        assert_eq!(fixed("- a\n - b\n- c\n", Kind::UnevenListIndent), "- a\n- b\n- c\n");
        // Children at a clean indent are not siblings.
        assert!(of("- a\n  - b\n    - c\n- d\n", Kind::UnevenListIndent).is_empty());
        assert!(of("1. a\n   - b\n2. c\n", Kind::UnevenListIndent).is_empty());
        // Indented code is not a list.
        assert!(of("text\n\n    - not a list\n     - nor this\n", Kind::UnevenListIndent).is_empty());
        // A tab cannot be fixed by rewriting spaces.
        let f = of("- a\n  - b\n\t- c\n", Kind::UnevenListIndent);
        assert!(f.is_empty() || f[0].fix.is_none());
    }

    #[test]
    fn mixed_list_markers() {
        let t = "- a\n* b\n+ c\n";
        assert_eq!(ranges(t, Kind::MixedListMarker), vec!["*", "+"]);
        assert_eq!(fixed(t, Kind::MixedListMarker), "- a\n- b\n- c\n");
        assert_eq!(fixed("1. a\n2) b\n", Kind::MixedListMarker), "1. a\n2. b\n");
        // Nested levels have their own marker; bullets and numbers are different lists.
        assert!(of("- a\n  * b\n- c\n", Kind::MixedListMarker).is_empty());
        assert!(of("- a\n\n1. b\n", Kind::MixedListMarker).is_empty());
        // A paragraph ends the list.
        assert!(of("- a\n\ntext\n\n* b\n", Kind::MixedListMarker).is_empty());
        // A rule is not a list item.
        assert!(of("- a\n***\n- b\n", Kind::MixedListMarker).is_empty());
    }

    #[test]
    fn table_cell_count() {
        let t = "| a | b | c |\n|---|---|---|\n| 1 | 2 |\n| 1 | 2 | 3 | 4 |\n| 1 | 2 | 3 |\n";
        let f = of(t, Kind::TableCellCount);
        assert_eq!(f.len(), 2);
        assert_eq!(fixed(t, Kind::TableCellCount), "| a | b | c |\n|---|---|---|\n| 1 | 2 | |\n| 1 | 2 | 3 | 4 |\n| 1 | 2 | 3 |\n");
        assert!(f[1].fix.is_none());
        // Without outer pipes, and an escaped pipe is not a separator.
        assert!(of("a | b\n--|--\n1 | 2\n", Kind::TableCellCount).is_empty());
        assert!(of("| a | b |\n|---|---|\n| x \\| y | z |\n", Kind::TableCellCount).is_empty());
        // A delimiter row of another width: not a table.
        assert_eq!(of("| a | b |\n|---|\n", Kind::TableCellCount).len(), 1);
        // A table ends at a blank line.
        assert!(of("| a |\n|---|\n\n| x | y |\n", Kind::TableCellCount).is_empty());
        // Tables in a fence are ignored.
        assert!(of("```\n| a | b |\n|---|---|\n| 1 |\n```\n", Kind::TableCellCount).is_empty());
    }

    #[test]
    fn trailing_spaces() {
        assert_eq!(fixed("a \nb\n", Kind::TrailingSpaces), "a\nb\n");
        assert_eq!(fixed("a   \nb\n", Kind::TrailingSpaces), "a\nb\n");
        assert_eq!(fixed("a\t\nb\n", Kind::TrailingSpaces), "a\nb\n");
        assert_eq!(fixed("a\n   \nb\n", Kind::TrailingSpaces), "a\n\nb\n");
        // Exactly two spaces before more text is a hard line break.
        assert!(of("a  \nb\n", Kind::TrailingSpaces).is_empty());
        // ... but not at the end of a paragraph or of the text.
        assert_eq!(of("a  \n\nb\n", Kind::TrailingSpaces).len(), 1);
        assert_eq!(of("a  ", Kind::TrailingSpaces).len(), 1);
        // Code is left alone.
        assert!(of("```\na   \n```\n", Kind::TrailingSpaces).is_empty());
        assert!(of("---\nk: v  \n---\n", Kind::TrailingSpaces).is_empty());
    }

    #[test]
    fn crlf_is_handled() {
        let t = "# A\r\n### B\r\ntext \r\n\r\n- a\r\n * b\r\n";
        assert_eq!(fixed(t, Kind::HeadingJump), "# A\r\n## B\r\ntext \r\n\r\n- a\r\n * b\r\n");
        assert_eq!(fixed(t, Kind::TrailingSpaces), "# A\r\n### B\r\ntext\r\n\r\n- a\r\n * b\r\n");
        assert_eq!(fixed(t, Kind::UnevenListIndent), "# A\r\n### B\r\ntext \r\n\r\n- a\r\n* b\r\n");
        // Hard break and a clean CRLF text.
        assert!(run("a  \r\nb\r\n").is_empty());
        let open = "```rust\r\nfn x() {}";
        assert_eq!(fixed(open, Kind::UnclosedFence), "```rust\r\nfn x() {}\r\n```\r\n");
    }

    #[test]
    fn unclosed_fence() {
        let f = of("text\n\n````\ncode\n```\n", Kind::UnclosedFence);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].range, 6..10);
        assert_eq!(fixed("```\ncode\n", Kind::UnclosedFence), "```\ncode\n```\n");
        assert_eq!(fixed("~~~\ncode", Kind::UnclosedFence), "~~~\ncode\n~~~\n");
        assert!(of("```\ncode\n```\n", Kind::UnclosedFence).is_empty());
        // A longer closing fence closes; a shorter one does not.
        assert!(of("```\ncode\n`````\n", Kind::UnclosedFence).is_empty());
        assert_eq!(of("````\ncode\n```\n", Kind::UnclosedFence).len(), 1);
        // A fence with an info string and a different character inside.
        assert!(of("~~~\n```\n~~~\n", Kind::UnclosedFence).is_empty());
        // Inline triple backticks on one line are not a fence.
        assert!(of("a ```x``` b\n```x``` y\n", Kind::UnclosedFence).is_empty());
    }

    #[test]
    fn front_matter_is_not_checked() {
        let t = "---\n# not a heading\n### x\ntitle: [a](missing.md) \n---\n# Real\n";
        assert!(run(t).is_empty());
        // An opening line without a closing one is a rule, not front matter.
        assert!(!of("---\n\n# A\n### B\n", Kind::HeadingJump).is_empty());
    }

    #[test]
    fn missing_link_targets() {
        let t = "[a](ok.md) [b](gone.md#sec) ![i](img/x%20y.png) [c](https://x.y/z) [d](#top) [e](mailto:a@b.c) [f](<sp ace.md>)\n";
        let exists = |p: &str| matches!(p, "ok.md" | "img/x y.png");
        let f = lint(t, &Rules::default(), &exists);
        let got: Vec<&str> = f.iter().filter(|f| f.kind == Kind::MissingTarget).map(|f| &t[f.range.clone()]).collect();
        assert_eq!(got, vec!["gone.md#sec", "sp ace.md"]);
    }

    #[test]
    fn link_targets_in_code_are_ignored() {
        let t = "`[a](gone.md)`\n\n```\n[a](gone.md)\n```\n\nSee `x` and [a](gone.md).\n";
        let f = lint(t, &Rules::default(), &|_| false);
        assert_eq!(f.iter().filter(|f| f.kind == Kind::MissingTarget).count(), 1);
    }

    #[test]
    fn reference_definitions_and_uses() {
        let t = "[x][a] [y][B] [z][nope] [w][^n]\n\n[a]: ok.md\n[b]: gone.md \"title\"\n";
        let f = lint(t, &Rules::default(), &|p| p == "ok.md");
        let refs: Vec<&str> = f.iter().filter(|f| f.kind == Kind::UndefinedReference).map(|f| &t[f.range.clone()]).collect();
        assert_eq!(refs, vec!["nope"]);
        let missing: Vec<&str> = f.iter().filter(|f| f.kind == Kind::MissingTarget).map(|f| &t[f.range.clone()]).collect();
        assert_eq!(missing, vec!["gone.md"]);
    }

    #[test]
    fn percent_decode_edges() {
        assert_eq!(super::percent_decode("a%2"), "a%2");
        assert_eq!(super::percent_decode("a%zzb"), "a%zzb");
        assert_eq!(super::percent_decode("%E6%97%A5.md"), "日.md");
        assert_eq!(super::percent_decode("100%"), "100%");
    }
}
