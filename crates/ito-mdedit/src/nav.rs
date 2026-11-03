use pulldown_cmark::{Event, Parser, Tag};

use crate::Dir;

/// What [`next`] looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The start of a heading.
    Heading,
    /// The start of a list (a nested list is part of its list).
    List,
    /// The start of a table.
    Table,
    /// The start of a code block (the opening fence).
    Code,
    /// The start of an unfinished task item (`- [ ]`).
    OpenTask,
    /// The heading that owns the caret's section; when the caret is already on that heading line, its parent. `dir`
    /// is not used.
    Parent,
    /// The first heading below the section that holds the caret. `dir` is not used.
    Child,
}

/// The byte to put the caret at to go to the next (`Dir::Down`) or previous (`Dir::Up`) `what` from `caret`, or `None`
/// when there is none. The text is not changed. For `Up`, the thing the caret is inside counts when the caret is
/// below its start, so `Up` from inside a list goes to the start of that list.
pub fn next(text: &str, caret: usize, what: Target, dir: Dir) -> Option<usize> {
    let caret = caret.min(text.len());
    let starts = match what {
        Target::Parent => return parent(text, caret),
        Target::Child => return child(text, caret),
        Target::Heading => ito_outline::headings(text, 6).iter().map(|h| h.range.start).collect(),
        _ => starts(text, what),
    };
    match dir {
        Dir::Down => starts.into_iter().find(|&s| s > caret),
        Dir::Up => starts.into_iter().rev().find(|&s| s < caret),
    }
}

fn starts(text: &str, what: Target) -> Vec<usize> {
    let mut out = Vec::new();
    let (mut list_depth, mut items) = (0usize, Vec::new());
    for (event, r) in Parser::new_ext(text, ito_outline::options()).into_offset_iter() {
        match event {
            Event::Start(Tag::List(_)) => {
                if list_depth == 0 && what == Target::List {
                    out.push(r.start);
                }
                list_depth += 1;
            }
            Event::End(pulldown_cmark::TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) => items.push(r.start),
            Event::End(pulldown_cmark::TagEnd::Item) => {
                items.pop();
            }
            Event::TaskListMarker(false) if what == Target::OpenTask => {
                if let Some(&s) = items.last() {
                    out.push(s);
                }
            }
            Event::Start(Tag::Table(_)) if what == Target::Table => out.push(r.start),
            Event::Start(Tag::CodeBlock(_)) if what == Target::Code => out.push(r.start),
            _ => {}
        }
    }
    out
}

fn parent(text: &str, caret: usize) -> Option<usize> {
    let hs = ito_outline::headings(text, 6);
    let i = ito_outline::at_byte(&hs, caret)?;
    let h = &hs[i];
    if caret > h.range.end {
        return Some(h.range.start);
    }
    hs[..i].iter().rev().find(|p| p.level < h.level).map(|p| p.range.start)
}

fn child(text: &str, caret: usize) -> Option<usize> {
    let hs = ito_outline::headings(text, 6);
    let i = ito_outline::at_byte(&hs, caret)?;
    hs.get(i + 1).filter(|c| c.range.start < hs[i].section.end).map(|c| c.range.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# One\n\ntext\n\n- [x] done\n- [ ] todo\n  - [ ] nested\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n## 二\n\n- plain\n\n### Three\n\n# Four\n";

    fn at(s: &str) -> usize {
        DOC.find(s).unwrap()
    }

    #[test]
    fn headings_forward_and_back() {
        let down = |c| next(DOC, c, Target::Heading, Dir::Down);
        assert_eq!(down(0), Some(at("## 二")));
        assert_eq!(down(at("## 二")), Some(at("### Three")));
        assert_eq!(down(at("# Four")), None);
        let up = |c| next(DOC, c, Target::Heading, Dir::Up);
        assert_eq!(up(at("## 二") + 3), Some(at("## 二")));
        assert_eq!(up(at("## 二")), Some(0));
        assert_eq!(up(0), None);
        assert_eq!(next("", 0, Target::Heading, Dir::Down), None);
    }

    #[test]
    fn lists_tables_and_code() {
        assert_eq!(next(DOC, 0, Target::List, Dir::Down), Some(at("- [x]")));
        assert_eq!(next(DOC, at("- [x]"), Target::List, Dir::Down), Some(at("- plain")));
        assert_eq!(next(DOC, at("nested"), Target::List, Dir::Up), Some(at("- [x]")));
        assert_eq!(next(DOC, 0, Target::Table, Dir::Down), Some(at("| a")));
        assert_eq!(next(DOC, at("| a") + 4, Target::Table, Dir::Up), Some(at("| a")));
        assert_eq!(next(DOC, 0, Target::Code, Dir::Down), Some(at("```rust")));
        assert_eq!(next(DOC, at("fn main"), Target::Code, Dir::Up), Some(at("```rust")));
        assert_eq!(next(DOC, at("```rust"), Target::Code, Dir::Down), None);
        assert_eq!(next(DOC, 0, Target::Table, Dir::Up), None);
    }

    #[test]
    fn open_tasks_including_nested_ones() {
        assert_eq!(next(DOC, 0, Target::OpenTask, Dir::Down), Some(at("- [ ] todo")));
        assert_eq!(next(DOC, at("- [ ] todo"), Target::OpenTask, Dir::Down), Some(at("- [ ] nested")));
        assert_eq!(next(DOC, at("- [ ] nested"), Target::OpenTask, Dir::Down), None);
        assert_eq!(next(DOC, at("| a"), Target::OpenTask, Dir::Up), Some(at("- [ ] nested")));
        assert_eq!(next("- [x] a\n", 0, Target::OpenTask, Dir::Down), None);
    }

    #[test]
    fn parent_and_child_sections() {
        // In the body: the heading itself; on the heading line: its parent.
        assert_eq!(next(DOC, at("- plain"), Target::Parent, Dir::Up), Some(at("## 二")));
        assert_eq!(next(DOC, at("## 二") + 2, Target::Parent, Dir::Up), Some(0));
        assert_eq!(next(DOC, 0, Target::Parent, Dir::Up), None);
        assert_eq!(next(DOC, at("### Three"), Target::Parent, Dir::Up), Some(at("## 二")));
        assert_eq!(next(DOC, at("text"), Target::Child, Dir::Down), Some(at("## 二")));
        assert_eq!(next(DOC, at("- plain"), Target::Child, Dir::Down), Some(at("### Three")));
        assert_eq!(next(DOC, at("# Four"), Target::Child, Dir::Down), None);
        assert_eq!(next("text\n\n# A\n", 1, Target::Child, Dir::Down), None);
    }

    #[test]
    fn setext_crlf_and_a_heading_in_a_fence() {
        let t = "Title\r\n=====\r\n\r\n```\r\n# no\r\n```\r\n\r\nSub\r\n---\r\n";
        assert_eq!(next(t, 0, Target::Heading, Dir::Down), Some(t.find("Sub").unwrap()));
        assert_eq!(next(t, t.len(), Target::Heading, Dir::Up), Some(t.find("Sub").unwrap()));
        assert_eq!(next(t, 0, Target::Code, Dir::Down), Some(t.find("```").unwrap()));
    }

    #[test]
    fn a_caret_at_column_0_of_a_prefixed_heading_is_on_that_heading() {
        let t = "# P\n\nbody\n\n> ## Q\n\nq\n\n### R\n";
        let q = t.find("> ##").unwrap();
        assert_eq!(next(t, q, Target::Parent, Dir::Up), Some(0));
        assert_eq!(next(t, q, Target::Child, Dir::Down), Some(t.find("### R").unwrap()));
        let t = " ## A\n\nbody\n";
        assert_eq!(next(t, 0, Target::Parent, Dir::Up), None);
    }
}
