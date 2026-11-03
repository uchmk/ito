use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};
use unicode_segmentation::UnicodeSegmentation;

/// `r` without the white space at both ends.
fn trimmed(text: &str, r: Range<usize>) -> Range<usize> {
    let s = &text[r.clone()];
    let start = r.start + (s.len() - s.trim_start().len());
    let end = r.start + s.trim_end().len();
    start..end.max(start)
}

/// Runs of letters and numbers (a Japanese clause up to a punctuation mark or a blank is one run) and sentences
/// (up to `。` `！` `？` `!` `?`, or a `.` before a blank) inside the line `line`.
fn inline_ranges(text: &str, line: Range<usize>, out: &mut Vec<Range<usize>>) {
    let s = &text[line.clone()];
    let mut run: Option<Range<usize>> = None;
    for (i, w) in s.unicode_word_indices() {
        let r = line.start + i..line.start + i + w.len();
        match run.as_mut() {
            Some(cur) if cur.end == r.start => cur.end = r.end,
            _ => {
                out.extend(run.take());
                run = Some(r);
            }
        }
    }
    out.extend(run);
    let mut from = 0;
    let mut chars = s.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        let end = match c {
            '。' | '！' | '？' | '!' | '?' => true,
            '.' => chars.peek().is_none_or(|&(_, n)| n.is_whitespace()),
            _ => false,
        };
        if end {
            // Closing quotes and brackets belong to the sentence.
            while let Some(&(_, n)) = chars.peek() {
                if matches!(n, '」' | '』' | '）' | ')' | '"' | '”') {
                    chars.next();
                } else {
                    break;
                }
            }
            let to = chars.peek().map_or(s.len(), |&(j, _)| j);
            out.push(trimmed(text, line.start + from..line.start + to));
            from = to;
        }
    }
    if from < s.len() {
        out.push(trimmed(text, line.start + from..line.end));
    }
}

/// All the structural ranges of the text, and the words and sentences of the lines that touch `focus`.
fn candidates(text: &str, focus: &Range<usize>) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    // `Some(range)` for a heading or a code block (the range of its text), `Some(start)` for an item.
    enum Frame {
        Plain,
        Text(Option<Range<usize>>),
        Item(Option<usize>),
    }
    let mut stack: Vec<Frame> = Vec::new();
    for (event, r) in Parser::new_ext(text, ito_outline::options()).into_offset_iter() {
        match &event {
            Event::Start(tag) => {
                if let Some(Frame::Item(first @ None)) = stack.last_mut() {
                    *first = Some(r.start);
                }
                stack.push(match tag {
                    Tag::Heading { .. } | Tag::CodeBlock(_) => Frame::Text(None),
                    Tag::Item => Frame::Item(None),
                    _ => Frame::Plain,
                });
            }
            Event::End(_) => {
                match stack.pop() {
                    Some(Frame::Text(Some(t))) => out.push(t),
                    Some(Frame::Item(Some(s))) => out.push(s..r.end),
                    _ => {}
                }
                out.push(r.clone());
            }
            Event::Text(_) => {
                for f in stack.iter_mut().rev() {
                    match f {
                        Frame::Text(t) => {
                            *t = Some(t.as_ref().map_or(r.clone(), |t| t.start..r.end));
                            break;
                        }
                        Frame::Item(first @ None) => *first = Some(r.start),
                        _ => {}
                    }
                }
            }
            Event::Rule | Event::Code(_) => out.push(r.clone()),
            Event::TaskListMarker(_) => {
                if let Some(Frame::Item(first @ None)) = stack.last_mut() {
                    *first = Some(r.start);
                }
            }
            _ => {}
        }
    }
    for h in ito_outline::headings(text, 6) {
        out.push(h.section);
    }
    out.push(0..text.len());
    let mut out: Vec<_> = out.into_iter().map(|r| trimmed(text, r)).collect();
    out.push(0..text.len());
    // Words and sentences of the lines the focus touches.
    let from = text[..focus.start].rfind('\n').map_or(0, |i| i + 1);
    let to = text[focus.end..].find('\n').map_or(text.len(), |i| focus.end + i);
    let mut at = from;
    for line in text[from..to].split_inclusive('\n') {
        let end = at + line.trim_end_matches(['\n', '\r']).len();
        inline_ranges(text, at..end, &mut out);
        at += line.len();
    }
    out.retain(|r| r.start < r.end);
    out
}

/// The next larger structural range around `sel` (the caret is an empty range): word, sentence, then each thing it
/// sits in (emphasis or link, table cell, row, paragraph, list item, whole list or table, quote, code block, the
/// section under the nearest heading, the parent sections), and the whole text last. The ranges are exact, without
/// the line break at the end. `None` when `sel` is the whole text already, or the text is empty.
///
/// A Japanese run of letters between punctuation counts as one word, as no spaces mark it.
pub fn expand_selection(text: &str, sel: Range<usize>) -> Option<Range<usize>> {
    let sel = clamp(text, sel);
    candidates(text, &sel)
        .into_iter()
        .filter(|c| c.start <= sel.start && sel.end <= c.end && *c != sel)
        .min_by_key(|c| (c.end - c.start, c.start))
}

/// The next smaller structural range inside `sel`, among the ones [`expand_selection`] goes through: the largest
/// one that starts where `sel` starts (or, failing that, at the first start after it). `None` when there is nothing
/// smaller. An editor that expanded step by step can instead pop its own stack, which goes back through the same
/// ranges exactly.
pub fn shrink_selection(text: &str, sel: Range<usize>) -> Option<Range<usize>> {
    let sel = clamp(text, sel);
    candidates(text, &sel)
        .into_iter()
        .filter(|c| sel.start <= c.start && c.end <= sel.end && *c != sel)
        .min_by_key(|c| (c.start, std::cmp::Reverse(c.end)))
}

fn clamp(text: &str, sel: Range<usize>) -> Range<usize> {
    let end = sel.end.min(text.len());
    sel.start.min(end)..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grow(t: &str, from: Range<usize>) -> Vec<&str> {
        let mut out = Vec::new();
        let mut sel = from;
        while let Some(n) = expand_selection(t, sel) {
            out.push(&t[n.clone()]);
            sel = n;
        }
        out
    }

    #[test]
    fn a_word_grows_to_the_paragraph_the_section_and_the_text() {
        let t = "# One\n\nHello world. Second one.\n\n## Two\n\ntail\n";
        let at = t.find("world").unwrap() + 1;
        let steps = grow(t, at..at);
        assert_eq!(
            steps,
            vec![
                "world",
                "Hello world.",
                "Hello world. Second one.",
                "# One\n\nHello world. Second one.\n\n## Two\n\ntail",
                "# One\n\nHello world. Second one.\n\n## Two\n\ntail\n"
            ]
        );
    }

    #[test]
    fn list_items_cells_and_code() {
        let t = "- a *bold* b\n  - nested one\n- next\n\n| h1 | h2 |\n|----|----|\n| c1 | c2 |\n\n```\nlet x = 1;\n```\n";
        let at = t.find("nested").unwrap();
        let steps = grow(t, at..at);
        assert_eq!(steps[0], "nested");
        assert_eq!(steps[1], "nested one");
        assert_eq!(steps[2], "- nested one");
        // The item's text (after the marker), then the item with its marker.
        assert_eq!(steps[3], "a *bold* b\n  - nested one");
        assert_eq!(steps[4], "- a *bold* b\n  - nested one");
        assert!(steps[5].ends_with("- next"));
        let at = t.find("c2").unwrap();
        let steps = grow(t, at..at);
        assert_eq!(&steps[..3], &["c2", "| c1 | c2 |", "| h1 | h2 |\n|----|----|\n| c1 | c2 |"]);
        let at = t.find("x = 1").unwrap();
        let steps = grow(t, at..at);
        assert_eq!(&steps[..3], &["x", "let x = 1;", "```\nlet x = 1;\n```"]);
        let at = t.find("bold").unwrap();
        assert_eq!(&grow(t, at..at)[..3], &["bold", "*bold*", "- a *bold* b"]);
    }

    #[test]
    fn a_selection_grows_to_the_smallest_range_that_holds_it() {
        let t = "# H\n\none two\n\nthree\n";
        let a = t.find("one").unwrap();
        assert_eq!(expand_selection(t, a..a + 3).unwrap(), a..a + 7);
        // Two paragraphs: the section.
        let b = t.find("three").unwrap();
        assert_eq!(&t[expand_selection(t, a..b + 2).unwrap()], "# H\n\none two\n\nthree");
        assert_eq!(expand_selection(t, 0..t.len()), None);
        assert_eq!(expand_selection("", 0..0), None);
    }

    #[test]
    fn japanese_has_clauses_and_sentences() {
        let t = "今日は、いい天気です。明日も晴れる！\n\n次";
        let at = t.find("天気").unwrap();
        let steps = grow(t, at..at);
        assert_eq!(&steps[..3], &["いい天気です", "今日は、いい天気です。", "今日は、いい天気です。明日も晴れる！"]);
        assert_eq!(steps.last().unwrap(), &t);
    }

    #[test]
    fn headings_setext_crlf_and_nested_sections() {
        let t = "Title\r\n=====\r\n\r\nbody\r\n\r\n## Sub\r\n\r\nx\r\n";
        let at = t.find("Title").unwrap();
        let steps = grow(t, at..at);
        assert_eq!(&steps[..2], &["Title", "Title\r\n====="]);
        let at = t.find('x').unwrap();
        let steps = grow(t, at..at);
        assert_eq!(&steps[..3], &["x", "## Sub\r\n\r\nx", "Title\r\n=====\r\n\r\nbody\r\n\r\n## Sub\r\n\r\nx"]);
        let t = "# A\n\n## B\n\n### C\n\nc\n\n## D\n";
        let at = t.find('c').unwrap();
        let steps = grow(t, at..at);
        assert_eq!(&steps[..4], &["c", "### C\n\nc", "## B\n\n### C\n\nc", "# A\n\n## B\n\n### C\n\nc\n\n## D"]);
    }

    #[test]
    fn caret_at_the_edges() {
        let t = "abc def";
        assert_eq!(expand_selection(t, 0..0).unwrap(), 0..3);
        assert_eq!(expand_selection(t, 7..7).unwrap(), 4..7);
        assert_eq!(expand_selection(t, 3..3).unwrap(), 0..3);
        // On a blank between paragraphs: the next larger thing around it.
        let t = "a\n\nb\n";
        assert_eq!(expand_selection(t, 2..2).unwrap(), 0..4);
    }

    #[test]
    fn shrink_goes_back_in() {
        let t = "# H\n\none two. three\n\n## S\n";
        let sec = 0..t.find("\n\n## S").unwrap();
        let s1 = shrink_selection(t, sec.clone()).unwrap();
        assert_eq!(&t[s1.clone()], "# H");
        let para = t.find("one").unwrap()..t.find("\n\n## S").unwrap();
        let s2 = shrink_selection(t, para.clone()).unwrap();
        assert_eq!(&t[s2.clone()], "one two.");
        let s3 = shrink_selection(t, s2).unwrap();
        assert_eq!(&t[s3.clone()], "one");
        assert_eq!(shrink_selection(t, s3), None);
        assert_eq!(shrink_selection("", 0..0), None);
        assert_eq!(shrink_selection(t, 5..5), None);
    }
}
