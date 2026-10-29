//! A line diff of two texts, for a screen that shows what changed: a job
//! definition edited by hand, a file about to be replaced. Git is not
//! involved; the two texts are given.
//!
//! [`diff_lines`] gives every line with its kind and its numbers in the old
//! and the new text; [`fold`] cuts the long runs of unchanged lines down to a
//! few lines around each change. [`ito-confirm`'s `Question::diff`] draws the
//! result.

use similar::{ChangeTag, TextDiff};

/// What happened to a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Same,
    Added,
    Removed,
}

impl Kind {
    /// The mark in front of a line, so the kind is not told by colour alone.
    pub fn mark(self) -> char {
        match self {
            Kind::Same => ' ',
            Kind::Added => '+',
            Kind::Removed => '-',
        }
    }
}

/// One line of the diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    /// The 1-based line number in the old text; `None` for an added line.
    pub old_no: Option<usize>,
    /// The 1-based line number in the new text; `None` for a removed line.
    pub new_no: Option<usize>,
    /// The line without its line ending.
    pub text: String,
}

/// A line, or the unchanged lines [`fold`] left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Line(Line),
    /// This many unchanged lines are not shown.
    Folded(usize),
}

/// Every line of `old` and `new` in order, marked same, added or removed.
/// Two equal texts give only `Same` lines; an empty text has no lines.
pub fn diff_lines(old: &str, new: &str) -> Vec<Line> {
    TextDiff::from_lines(old, new)
        .iter_all_changes()
        .map(|c| Line {
            kind: match c.tag() {
                ChangeTag::Equal => Kind::Same,
                ChangeTag::Insert => Kind::Added,
                ChangeTag::Delete => Kind::Removed,
            },
            old_no: c.old_index().map(|i| i + 1),
            new_no: c.new_index().map(|i| i + 1),
            text: c.value().trim_end_matches(['\n', '\r']).to_string(),
        })
        .collect()
}

/// How many lines were added and removed.
pub fn counts(lines: &[Line]) -> (usize, usize) {
    let n = |k| lines.iter().filter(|l| l.kind == k).count();
    (n(Kind::Added), n(Kind::Removed))
}

/// Keep the changed lines and `context` unchanged lines on each side of them;
/// a run of unchanged lines longer than that becomes one [`Item::Folded`]
/// (a run of one line is kept, since saying "1 line hidden" takes a line too).
/// With no change at all the result is a single `Folded` for the whole text,
/// or empty for an empty text.
pub fn fold(lines: &[Line], context: usize) -> Vec<Item> {
    let mut keep = vec![false; lines.len()];
    for (i, l) in lines.iter().enumerate() {
        if l.kind != Kind::Same {
            let from = i.saturating_sub(context);
            let to = (i + context + 1).min(lines.len());
            keep[from..to].fill(true);
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if keep[i] {
            out.push(Item::Line(lines[i].clone()));
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && !keep[i] {
            i += 1;
        }
        if i - start == 1 {
            out.push(Item::Line(lines[start].clone()));
        } else {
            out.push(Item::Folded(i - start));
        }
    }
    out
}

/// `diff_lines` then `fold`, for the common case.
pub fn diff(old: &str, new: &str, context: usize) -> Vec<Item> {
    fold(&diff_lines(old, new), context)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(lines: &[Line]) -> String {
        lines.iter().map(|l| l.kind.mark()).collect()
    }

    #[test]
    fn a_changed_line_is_removed_then_added_with_its_numbers() {
        let l = diff_lines("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(kinds(&l), " -+ ");
        assert_eq!((l[1].old_no, l[1].new_no, l[1].text.as_str()), (Some(2), None, "b"));
        assert_eq!((l[2].old_no, l[2].new_no, l[2].text.as_str()), (None, Some(2), "B"));
        assert_eq!((l[3].old_no, l[3].new_no), (Some(3), Some(3)));
        assert_eq!(counts(&l), (1, 1));
    }

    #[test]
    fn line_endings_are_not_part_of_the_text() {
        let l = diff_lines("a\r\nb", "a\r\nb\r\nc");
        assert!(l.iter().all(|x| !x.text.contains(['\r', '\n'])));
        assert_eq!(l.last().unwrap().text, "c");
    }

    #[test]
    fn equal_texts_have_only_same_lines_and_empty_ones_none() {
        assert_eq!(kinds(&diff_lines("a\nb\n", "a\nb\n")), "  ");
        assert!(diff_lines("", "").is_empty());
        assert_eq!(kinds(&diff_lines("", "x\n")), "+");
    }

    #[test]
    fn a_long_run_is_folded_to_the_context_around_the_change() {
        let old: String = (1..=20).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l10\n", "ten\n");
        let items = diff(&old, &new, 2);
        let folded: Vec<usize> = items.iter().filter_map(|i| if let Item::Folded(n) = i { Some(*n) } else { None }).collect();
        // l1..l7 before (7), l13..l20 after (8); two lines of context on each side stay.
        assert_eq!(folded, vec![7, 8]);
        let shown = items.iter().filter(|i| matches!(i, Item::Line(_))).count();
        assert_eq!(shown, 6); // l8 l9 -l10 +ten l11 l12
    }

    #[test]
    fn a_run_of_one_is_kept_instead_of_folded() {
        let items = diff("a\nb\nc\nd\ne\n", "X\nb\nc\nd\nY\n", 1);
        assert!(items.iter().all(|i| matches!(i, Item::Line(_))), "{items:?}");
    }

    #[test]
    fn no_change_folds_the_whole_text() {
        assert_eq!(diff("a\nb\nc\n", "a\nb\nc\n", 3), vec![Item::Folded(3)]);
        assert!(diff("", "", 3).is_empty());
    }
}
