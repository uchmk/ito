//! The headings of a Markdown text: level, plain title, where the heading is in the text (byte range and line) and
//! the *section* it owns (up to the next heading of the same or a higher level). An outline column, "move this
//! section up", and folding all read from this one list.
//!
//! Git, files and drawing are not here. The text is given.

use std::ops::Range;

pub mod column;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// The options Markdown is read with everywhere in uchmk: tables, footnotes, strike-through, task lists, GFM and
/// YAML / TOML front matter (so a `---` block is not read as headings).
pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
}

/// One heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    /// 1 for `#`, 6 for `######`.
    pub level: u8,
    /// Plain text: no marks, white space squeezed to single spaces, at most `MAX_TITLE` characters.
    pub title: String,
    /// The heading's own bytes (`## Title`, or the text and underline of a setext heading), without the line break.
    pub range: Range<usize>,
    /// 0-based line the heading starts on.
    pub line: usize,
    /// The heading and everything below it, up to the next heading of the same or a higher level (or the end).
    pub section: Range<usize>,
}

/// A title longer than this is cut; an outline elides to its column anyway.
pub const MAX_TITLE: usize = 120;

/// Every heading of `text` down to `max_level` (6 for all), in order. `section` of a heading ends where the next
/// heading of the same or a higher level starts, counting headings deeper than `max_level` too.
pub fn headings(text: &str, max_level: u8) -> Vec<Heading> {
    let max_level = max_level.clamp(1, 6);
    let mut all: Vec<Heading> = Vec::new();
    let mut open: Option<(u8, String, usize)> = None;
    for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => open = Some((level as u8, String::new(), range.start)),
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, title, _)) = open.as_mut() {
                    title.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, title, _)) = open.as_mut() {
                    title.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, title, start)) = open.take() {
                    let end = range.end;
                    let end = start + text[start..end].trim_end_matches(['\r', '\n']).len();
                    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
                    all.push(Heading {
                        level,
                        title: title.chars().take(MAX_TITLE).collect(),
                        range: start..end,
                        line: text[..start].bytes().filter(|&b| b == b'\n').count(),
                        section: start..text.len(),
                    });
                }
            }
            _ => {}
        }
    }
    for i in 0..all.len() {
        let level = all[i].level;
        if let Some(next) = all[i + 1..].iter().find(|h| h.level <= level) {
            all[i].section.end = next.section.start;
        }
    }
    all.retain(|h| h.level <= max_level);
    all
}

/// The index of the heading whose section holds `byte` and is the deepest, or `None` before the first heading.
/// A caret at the very end of the text (`byte == text.len()`) belongs to the sections that run to the end.
pub fn at_byte(headings: &[Heading], byte: usize) -> Option<usize> {
    let end = headings.iter().map(|h| h.section.end).max();
    headings
        .iter()
        .enumerate()
        .filter(|(_, h)| h.section.contains(&byte) || (byte == h.section.end && Some(byte) == end))
        .map(|(i, _)| i)
        .next_back()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\ntitle: x\n---\n\n# One\n\ntext\n\n## Two `code` *em*\n\nmore\n\n### Three\n\n# Four\nlast\n";

    #[test]
    fn headings_have_levels_titles_lines_and_sections() {
        let h = headings(DOC, 6);
        let t: Vec<_> = h.iter().map(|h| (h.level, h.title.as_str(), h.line)).collect();
        assert_eq!(t, vec![(1, "One", 4), (2, "Two code em", 8), (3, "Three", 12), (1, "Four", 14)]);
        assert_eq!(&DOC[h[0].range.clone()], "# One");
        // One owns Two and Three; Two owns Three; Three runs up to Four; Four to the end.
        assert_eq!(&DOC[h[0].section.clone()], "# One\n\ntext\n\n## Two `code` *em*\n\nmore\n\n### Three\n\n");
        assert_eq!(&DOC[h[1].section.clone()], "## Two `code` *em*\n\nmore\n\n### Three\n\n");
        assert_eq!(&DOC[h[2].section.clone()], "### Three\n\n");
        assert_eq!(&DOC[h[3].section.clone()], "# Four\nlast\n");
    }

    #[test]
    fn front_matter_and_fenced_code_are_not_headings() {
        let h = headings("---\n# not\n---\n\n```\n# nor this\n```\n\n# yes\n", 6);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].title, "yes");
    }

    #[test]
    fn setext_headings_and_a_depth_limit() {
        let text = "Title\n=====\n\nSub\n---\n\n#### Deep\n";
        assert_eq!(headings(text, 6).len(), 3);
        let h = headings(text, 2);
        assert_eq!(h.len(), 2);
        assert_eq!(&text[h[0].range.clone()], "Title\n=====");
        // A heading below the limit still ends the section above it.
        assert_eq!(&text[h[1].section.clone()], "Sub\n---\n\n#### Deep\n");
    }

    #[test]
    fn japanese_titles_and_crlf() {
        let text = "# はじめに\r\n\r\n本文\r\n\r\n## 設定\r\n";
        let h = headings(text, 6);
        assert_eq!(h[0].title, "はじめに");
        assert_eq!(&text[h[0].range.clone()], "# はじめに");
        assert_eq!(h[1].line, 4);
    }

    #[test]
    fn the_deepest_section_holding_a_byte() {
        let h = headings(DOC, 6);
        assert_eq!(at_byte(&h, 0), None);
        assert_eq!(at_byte(&h, DOC.find("more").unwrap()), Some(1));
        assert_eq!(at_byte(&h, DOC.find("last").unwrap()), Some(3));
    }

    #[test]
    fn a_caret_at_the_end_of_the_text() {
        let heading_last = "# A

text

## B";
        let h = headings(heading_last, 6);
        assert_eq!(at_byte(&h, heading_last.len()), Some(1));
        let body_last = "# A

## B
body
";
        let h = headings(body_last, 6);
        assert_eq!(at_byte(&h, body_last.len()), Some(1));
        assert_eq!(at_byte(&h, body_last.len() + 1), None);
        let h = headings(DOC, 6);
        assert_eq!(at_byte(&h, DOC.len()), Some(3));
        // A boundary inside the text still goes to the heading that starts there.
        assert_eq!(at_byte(&h, h[2].section.end), Some(3));
        let none = "just text
";
        assert_eq!(at_byte(&headings(none, 6), none.len()), None);
        assert_eq!(at_byte(&headings("", 6), 0), None);
    }
}
