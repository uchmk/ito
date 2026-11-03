use std::ops::Range;

use ito_outline::Heading;

use crate::{after_break, change_from_edits, Change};

/// How [`delete_heading`] takes a heading away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadingDelete {
    /// Only the heading's line; its body stays and joins the section above.
    LineOnly,
    /// The heading with its body and all its sub-sections.
    Section,
    /// Keep the title as a paragraph: the `#`s (or the underline) go.
    ToParagraph,
    /// Delete the heading's line and lift every heading below it one level, so they keep their shape.
    LiftChildren,
}

fn is_atx(text: &str, h: &Heading) -> bool {
    text.as_bytes()[h.range.start] == b'#'
}

/// Is the line before `pos` blank (or is `pos` the top of the text)?
fn blank_above(text: &str, pos: usize) -> bool {
    let before = &text[..pos];
    let before = before.strip_suffix('\n').unwrap_or(before);
    let before = before.strip_suffix('\r').unwrap_or(before);
    if pos == 0 {
        return true;
    }
    let line = before.rsplit('\n').next().unwrap_or("");
    line.trim().is_empty()
}

/// The range of the heading's lines with the line break, plus the blank lines after it when the line above is blank
/// too (so two blank lines do not pile up).
fn line_range(text: &str, h: &Heading) -> Range<usize> {
    let start = text[..h.range.start].rfind('\n').map_or(0, |i| i + 1);
    let mut end = after_break(text, h.range.end);
    if blank_above(text, start) {
        while end < text.len() {
            let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i);
            if !text[end..line_end].trim().is_empty() {
                break;
            }
            end = after_break(text, line_end);
        }
    }
    start..end
}

/// The edits that turn an ATX heading into its title: the `#`s and the spaces after them, and the closing `#`s.
fn atx_title_edits(text: &str, h: &Heading) -> Vec<(Range<usize>, String)> {
    let line = &text[h.range.clone()];
    let body = line.trim_start_matches('#');
    let head = line.len() - body.len() + (body.len() - body.trim_start_matches([' ', '\t']).len());
    let mut end = line.trim_end_matches([' ', '\t']).len();
    let trimmed = &line[head.min(end)..end];
    let hashes = trimmed.trim_end_matches('#');
    // Closing hashes count only after a blank (or when they are all there is).
    if hashes.len() < trimmed.len() && (hashes.is_empty() || hashes.ends_with([' ', '\t'])) {
        end = head.min(end) + hashes.trim_end_matches([' ', '\t']).len();
    }
    let s = h.range.start;
    let mut out = Vec::new();
    if head > 0 {
        out.push((s..s + head, String::new()));
    }
    if end < line.len() {
        out.push((s + end.max(head)..h.range.end, String::new()));
    }
    out
}

/// Where the underline of a setext heading starts, and where the line break before it starts.
fn underline(text: &str, h: &Heading) -> (usize, usize) {
    let start = text[h.range.start..h.range.end].rfind('\n').map_or(h.range.start, |i| h.range.start + i + 1);
    let mut brk = start.saturating_sub(1);
    if brk > h.range.start && text.as_bytes()[brk - 1] == b'\r' {
        brk -= 1;
    }
    (start, brk)
}

/// Take the heading that holds `caret` (the one whose section the caret is in) away without breaking the outline:
/// see [`HeadingDelete`]. `None` before the first heading. The caret stays on the same text, or goes to where the
/// deleted part was.
pub fn delete_heading(text: &str, caret: usize, how: HeadingDelete) -> Option<Change> {
    let caret = caret.min(text.len());
    let hs = ito_outline::headings(text, 6);
    let i = ito_outline::at_byte(&hs, caret)?;
    let h = &hs[i];
    match how {
        HeadingDelete::LineOnly => change_from_edits(text, &[(line_range(text, h), String::new())], caret),
        HeadingDelete::Section => {
            let mut r = h.section.clone();
            if r.end == text.len() {
                // The last section: do not leave the blank lines before it at the end of the text.
                let kept = text[..r.start].trim_end().len();
                if kept > 0 {
                    r.start = after_break(text, text[kept..].find('\n').map_or(text.len(), |j| kept + j)).min(r.start);
                }
            }
            change_from_edits(text, &[(r, String::new())], caret)
        }
        HeadingDelete::ToParagraph => {
            let edits = if is_atx(text, h) {
                atx_title_edits(text, h)
            } else {
                let (_, brk) = underline(text, h);
                vec![(brk..h.range.end, String::new())]
            };
            change_from_edits(text, &edits, caret)
        }
        HeadingDelete::LiftChildren => {
            let mut edits = vec![(line_range(text, h), String::new())];
            for c in hs[i + 1..].iter().take_while(|c| c.range.start < h.section.end) {
                if is_atx(text, c) {
                    edits.push((c.range.start..c.range.start + 1, String::new()));
                } else {
                    // A setext heading of level 2 (`---`) becomes level 1 (`===`).
                    let (u, _) = underline(text, c);
                    let line = &text[u..c.range.end];
                    let lead = line.len() - line.trim_start().len();
                    let run = line.trim().len();
                    edits.push((u + lead..u + lead + run, "=".repeat(run)));
                }
            }
            change_from_edits(text, &edits, caret)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# A\n\nintro\n\n## B\n\nbody\n\n### C\n\nc\n\n## D\n\nd\n";

    fn run(t: &str, at: &str, how: HeadingDelete) -> String {
        delete_heading(t, t.find(at).unwrap(), how).unwrap().apply(t)
    }

    #[test]
    fn the_line_only_goes_and_the_body_joins_the_section_above() {
        assert_eq!(run(DOC, "## B", HeadingDelete::LineOnly), "# A\n\nintro\n\nbody\n\n### C\n\nc\n\n## D\n\nd\n");
        // The first line of the text.
        assert_eq!(run(DOC, "# A", HeadingDelete::LineOnly), "intro\n\n## B\n\nbody\n\n### C\n\nc\n\n## D\n\nd\n");
        // A caret in the body works too.
        assert_eq!(run(DOC, "body", HeadingDelete::LineOnly), run(DOC, "## B", HeadingDelete::LineOnly));
    }

    #[test]
    fn a_section_goes_with_its_sub_sections() {
        assert_eq!(run(DOC, "## B", HeadingDelete::Section), "# A\n\nintro\n\n## D\n\nd\n");
        assert_eq!(run(DOC, "# A", HeadingDelete::Section), "");
        // The last section does not leave a blank line at the end.
        assert_eq!(run(DOC, "## D", HeadingDelete::Section), "# A\n\nintro\n\n## B\n\nbody\n\n### C\n\nc\n");
        // The deepest heading holding the caret is the one deleted.
        assert_eq!(run(DOC, "c\n\n##", HeadingDelete::Section), "# A\n\nintro\n\n## B\n\nbody\n\n## D\n\nd\n");
        assert!(delete_heading("text\n\n# A\n", 1, HeadingDelete::Section).is_none());
    }

    #[test]
    fn a_heading_becomes_a_paragraph() {
        assert_eq!(run(DOC, "## B", HeadingDelete::ToParagraph), "# A\n\nintro\n\nB\n\nbody\n\n### C\n\nc\n\n## D\n\nd\n");
        let t = "###   見出し ##  \n\ntext\n";
        assert_eq!(run(t, "見出し", HeadingDelete::ToParagraph), "見出し\n\ntext\n");
        // `#` in the title stays.
        assert_eq!(run("## C# lang\n", "C#", HeadingDelete::ToParagraph), "C# lang\n");
        assert_eq!(run("## C#\n", "C#", HeadingDelete::ToParagraph), "C#\n");
        assert_eq!(run("##\n\nx\n", "##", HeadingDelete::ToParagraph), "\n\nx\n");
        // A setext heading loses its underline (CRLF too).
        assert_eq!(run("Title\n=====\n\nx\n", "Title", HeadingDelete::ToParagraph), "Title\n\nx\n");
        assert_eq!(run("Two\r\nlines\r\n---\r\n\r\nx\r\n", "Two", HeadingDelete::ToParagraph), "Two\r\nlines\r\n\r\nx\r\n");
    }

    #[test]
    fn the_caret_keeps_its_text() {
        let t = "## Title\n\nbody\n";
        let at = t.find("tle").unwrap();
        let c = delete_heading(t, at, HeadingDelete::ToParagraph).unwrap();
        assert_eq!(&c.apply(t)[c.cursor..c.cursor + 3], "tle");
        let at = t.find("body").unwrap();
        let c = delete_heading(t, at, HeadingDelete::LineOnly).unwrap();
        assert_eq!(&c.apply(t)[c.cursor..], "body\n");
        let c = delete_heading(t, 2, HeadingDelete::Section).unwrap();
        assert_eq!((c.apply(t).as_str(), c.cursor), ("", 0));
    }

    #[test]
    fn the_children_are_lifted_one_level() {
        assert_eq!(run(DOC, "## B", HeadingDelete::LiftChildren), "# A\n\nintro\n\nbody\n\n## C\n\nc\n\n## D\n\nd\n");
        assert_eq!(run(DOC, "# A", HeadingDelete::LiftChildren), "intro\n\n# B\n\nbody\n\n## C\n\nc\n\n# D\n\nd\n");
        // A setext child of level 2 becomes level 1; one of level 1 is not a child.
        let t = "# Top\n\nSub\n---\n\nx\n\n# Next\n";
        assert_eq!(run(t, "# Top", HeadingDelete::LiftChildren), "Sub\n===\n\nx\n\n# Next\n");
        // Without children it is the same as deleting the line.
        assert_eq!(run("# A\n\nx\n", "# A", HeadingDelete::LiftChildren), "x\n");
    }

    #[test]
    fn code_fences_and_empty_text_are_left_alone() {
        let t = "# A\n\n```\n## not\n```\n\n## B\n";
        assert_eq!(run(t, "# A", HeadingDelete::LiftChildren), "```\n## not\n```\n\n# B\n");
        assert!(delete_heading("", 0, HeadingDelete::LineOnly).is_none());
        assert!(delete_heading("# A", 99, HeadingDelete::LineOnly).is_some());
    }
}
