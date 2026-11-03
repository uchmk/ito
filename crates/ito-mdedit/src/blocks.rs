use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

use crate::{trim_end, Change};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Heading,
    Meta,
    Other,
}

/// The top-level blocks (paragraph, list, table, code, quote, rule, heading, front matter), each without its
/// trailing line breaks.
pub(crate) fn top_blocks(text: &str) -> Vec<(Range<usize>, Kind)> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut kind) = (0usize, 0, Kind::Other);
    for (event, r) in Parser::new_ext(text, ito_outline::options()).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    start = r.start;
                    kind = match tag {
                        Tag::Heading { .. } => Kind::Heading,
                        Tag::MetadataBlock(_) => Kind::Meta,
                        _ => Kind::Other,
                    };
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    out.push((trim_end(text, start..r.end), kind));
                }
            }
            Event::Rule if depth == 0 => out.push((trim_end(text, r), Kind::Other)),
            _ => {}
        }
    }
    out
}

/// Swap the two neighbours `a` (first) and `b` (second), keeping the gap between them. `byte` is in the one that
/// moves (`b` going up, `a` going down) and the caret keeps its place inside it.
fn swap(text: &str, a: Range<usize>, b: Range<usize>, up: bool, byte: usize) -> Change {
    let gap = &text[a.end..b.start];
    let mut new = String::with_capacity(b.end - a.start);
    new.push_str(&text[b.clone()]);
    new.push_str(gap);
    new.push_str(&text[a.clone()]);
    let cursor = if up {
        a.start + byte.clamp(b.start, b.end) - b.start
    } else {
        a.start + (b.end - b.start) + gap.len() + byte.clamp(a.start, a.end) - a.start
    };
    Change { range: a.start..b.end, text: new, cursor }
}

/// Move the block holding `byte` up or down past its neighbour. On a heading it moves the whole section (see
/// [`move_section`]). A block does not jump over a heading or the front matter.
pub fn move_block(text: &str, byte: usize, dir: Dir) -> Option<Change> {
    let byte = byte.min(text.len());
    let blocks = top_blocks(text);
    let i = blocks.iter().rposition(|(r, _)| r.start <= byte)?;
    match blocks[i].1 {
        Kind::Meta => None,
        Kind::Heading => move_section(text, byte, dir),
        Kind::Other => {
            let j = match dir {
                Dir::Up => i.checked_sub(1)?,
                Dir::Down => i + 1,
            };
            let (other, kind) = blocks.get(j)?;
            if *kind != Kind::Other {
                return None;
            }
            let this = blocks[i].0.clone();
            Some(match dir {
                Dir::Up => swap(text, other.clone(), this, true, byte),
                Dir::Down => swap(text, this, other.clone(), false, byte),
            })
        }
    }
}

/// Move the section (a heading and everything below it) holding `byte` past the sibling section of the same level.
/// `None` for the first or last sibling.
pub fn move_section(text: &str, byte: usize, dir: Dir) -> Option<Change> {
    let byte = byte.min(text.len());
    let hs = ito_outline::headings(text, 6);
    let i = ito_outline::at_byte(&hs, byte)?;
    let h = &hs[i];
    let core = |k: usize| trim_end(text, hs[k].section.clone());
    match dir {
        Dir::Down => {
            let j = hs.iter().position(|o| o.section.start == h.section.end && o.level == h.level)?;
            Some(swap(text, core(i), core(j), false, byte))
        }
        Dir::Up => {
            let j = hs.iter().position(|o| o.section.end == h.section.start && o.level == h.level)?;
            Some(swap(text, core(j), core(i), true, byte))
        }
    }
}

fn shift_section(text: &str, byte: usize, up: bool) -> Option<Change> {
    let byte = byte.min(text.len());
    let hs = ito_outline::headings(text, 6);
    let i = ito_outline::at_byte(&hs, byte)?;
    let sec = hs[i].section.clone();
    let core = trim_end(text, sec.clone());
    let inside: Vec<_> = hs.iter().filter(|h| h.range.start >= sec.start && h.range.start < sec.end).collect();
    for h in &inside {
        // A setext heading (a line of `===`) has no `#` to take away or add.
        if text.as_bytes()[h.range.start] != b'#' || (up && h.level == 1) || (!up && h.level == 6) {
            return None;
        }
    }
    let mut new = String::new();
    let (mut at, mut cursor) = (core.start, byte);
    for h in &inside {
        new.push_str(&text[at..h.range.start]);
        if up {
            at = h.range.start + 1;
            if h.range.start < byte {
                cursor -= 1;
            }
        } else {
            new.push('#');
            at = h.range.start;
            if h.range.start < byte {
                cursor += 1;
            }
        }
    }
    new.push_str(&text[at..core.end]);
    Some(Change { range: core, text: new, cursor })
}

/// Take one `#` off the section holding `byte` and every heading below it (`##` becomes `#`). `None` at level 1 or
/// when a heading is written with an underline.
pub fn promote_section(text: &str, byte: usize) -> Option<Change> {
    shift_section(text, byte, true)
}

/// Put one more `#` on the section holding `byte` and every heading below it. `None` past level 6.
pub fn demote_section(text: &str, byte: usize) -> Option<Change> {
    shift_section(text, byte, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paragraph_moves_past_its_neighbour_and_the_caret_follows() {
        let t = "one\n\ntwo\n\n- a\n- b\n";
        let at = t.find("two").unwrap() + 1;
        let c = move_block(t, at, Dir::Up).unwrap();
        assert_eq!(c.apply(t), "two\n\none\n\n- a\n- b\n");
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'w');
        let c = move_block(t, at, Dir::Down).unwrap();
        assert_eq!(c.apply(t), "one\n\n- a\n- b\n\ntwo\n");
        assert_eq!(&c.apply(t)[c.cursor..c.cursor + 2], "wo");
        assert!(move_block(t, 0, Dir::Up).is_none());
        assert!(move_block(t, t.len() - 1, Dir::Down).is_none());
    }

    #[test]
    fn a_block_does_not_cross_a_heading_or_front_matter() {
        let t = "---\na: 1\n---\n\ntext\n\n# H\n\nbody\n";
        assert!(move_block(t, t.find("text").unwrap(), Dir::Up).is_none());
        assert!(move_block(t, t.find("text").unwrap(), Dir::Down).is_none());
        assert!(move_block(t, 0, Dir::Down).is_none());
    }

    #[test]
    fn a_heading_moves_with_everything_below_it() {
        let t = "# A\n\na\n\n## A1\n\nx\n\n# B\n\nb\n";
        let c = move_block(t, 0, Dir::Down).unwrap();
        assert_eq!(c.apply(t), "# B\n\nb\n\n# A\n\na\n\n## A1\n\nx\n");
        assert_eq!(c.cursor, "# B\n\nb\n\n".len());
        let at = t.find("# B").unwrap();
        let c = move_block(t, at, Dir::Up).unwrap();
        assert_eq!(c.apply(t), "# B\n\nb\n\n# A\n\na\n\n## A1\n\nx\n");
        assert_eq!(c.cursor, 0);
        assert!(move_section(t, 0, Dir::Up).is_none());
        assert!(move_section(t, at, Dir::Down).is_none());
        assert!(move_section(t, t.find("## A1").unwrap(), Dir::Down).is_none());
    }

    #[test]
    fn sections_promote_and_demote_with_their_children() {
        let t = "# A\n\n## B\n\n### C\n\n## D\n";
        let c = demote_section(t, t.find('B').unwrap()).unwrap();
        assert_eq!(c.apply(t), "# A\n\n### B\n\n#### C\n\n## D\n");
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'B');
        let c = promote_section(t, t.find("## B").unwrap()).unwrap();
        assert_eq!(c.apply(t), "# A\n\n# B\n\n## C\n\n## D\n");
        assert!(promote_section(t, 0).is_none());
        assert!(demote_section("###### x\n", 0).is_none());
        assert!(promote_section("Title\n=====\n\n## x\n", 0).is_none());
    }
}
