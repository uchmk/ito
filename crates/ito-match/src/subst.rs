//! sed-style substitution over a text (`s/pat/rep/[gi]` without the spelling):
//! the same regex dialect as [`crate::Matcher`], with lookaround and
//! backreferences, and `$1` / `${name}` in the replacement.

use std::ops::Range;

use fancy_regex::Regex;

/// How the search half of a [`Subst`] is read when it comes from a search bar
/// rather than a `s/pat/rep/` spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Query {
    /// The query is a regular expression; otherwise it is literal text.
    pub regex: bool,
    /// Only whole words match (no word character right before or after).
    pub whole_word: bool,
    /// `None` is smart case, as [`crate::Matcher::new`] has it; `Some(true)` ignores case.
    pub ignore_case: Option<bool>,
    /// Every match, not just the first.
    pub all: bool,
}

impl Default for Query {
    fn default() -> Self {
        Self { regex: true, whole_word: false, ignore_case: None, all: true }
    }
}

/// A compiled substitution.
#[derive(Clone, Debug)]
pub struct Subst {
    re: Regex,
    rep: String,
    all: bool,
}

impl Subst {
    /// `all` replaces every match, not just the first; `ignore_case` prefixes `(?i)`.
    pub fn new(pattern: &str, replacement: &str, all: bool, ignore_case: bool) -> Result<Self, String> {
        let pattern = if ignore_case { format!("(?i){pattern}") } else { pattern.to_owned() };
        let re = Regex::new(&pattern).map_err(|e| e.to_string())?;
        Ok(Self { re, rep: replacement.to_owned(), all })
    }

    /// A substitution for what a search bar holds: the same smart case and
    /// `(?m)` as [`crate::Matcher::new`], so the matches replaced are the ones
    /// highlighted. The replacement is literal text when `!q.regex`.
    pub fn from_query(query: &str, replacement: &str, q: Query) -> Result<Self, String> {
        let body = if q.regex { query.to_owned() } else { fancy_regex::escape(query).into_owned() };
        let body = if q.whole_word { format!(r"(?<!\w)(?:{body})(?!\w)") } else { body };
        let ci = q.ignore_case.unwrap_or_else(|| !crate::matcher::has_capital(query));
        let pattern = if ci { format!("(?mi){body}") } else { format!("(?m){body}") };
        let re = Regex::new(&pattern).map_err(|e| e.to_string().lines().next().unwrap_or("invalid pattern").trim().to_owned())?;
        let rep = if q.regex { replacement.to_owned() } else { replacement.replace('$', "$$") };
        Ok(Self { re, rep, all: q.all })
    }

    /// Every replacement `apply` would make, as `(byte range, new text)`,
    /// ascending and not overlapping, with `$1` / `${name}` expanded. Empty
    /// matches are included (`^` -> `> ` quotes lines). A runaway pattern ends the list.
    pub fn edits(&self, text: &str) -> Vec<(Range<usize>, String)> {
        self.edits_in(text, 0..text.len())
    }

    /// Like [`Self::edits`] but only matches lying wholly inside `range`
    /// (a selection). Lookaround and anchors still see the whole `text`.
    pub fn edits_in(&self, text: &str, range: Range<usize>) -> Vec<(Range<usize>, String)> {
        let mut out = Vec::new();
        let end = range.end.min(text.len());
        let mut pos = range.start;
        while pos <= end && text.is_char_boundary(pos) {
            let Ok(Some(caps)) = self.re.captures_from_pos(text, pos) else { break };
            let Some(m) = caps.get(0) else { break };
            if m.end() > end {
                break;
            }
            let mut rep = String::new();
            caps.expand(&self.rep, &mut rep);
            out.push((m.range(), rep));
            if !self.all {
                break;
            }
            pos = if m.end() > m.start() {
                m.end()
            } else {
                match text[m.end()..].chars().next() {
                    Some(c) => m.end() + c.len_utf8(),
                    None => break,
                }
            };
        }
        out
    }

    /// `text` with the match(es) replaced. A runaway pattern counts as no match.
    pub fn apply(&self, text: &str) -> String {
        let out = if self.all { self.re.replace_all(text, self.rep.as_str()) } else { self.re.replace(text, self.rep.as_str()) };
        out.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_or_all() {
        assert_eq!(Subst::new("a", "b", false, false).unwrap().apply("aaa"), "baa");
        assert_eq!(Subst::new("a", "b", true, false).unwrap().apply("aaa"), "bbb");
    }

    #[test]
    fn case_is_ignored_on_request() {
        assert_eq!(Subst::new("img", "pic", false, false).unwrap().apply("IMG_1"), "IMG_1");
        assert_eq!(Subst::new("img", "pic", false, true).unwrap().apply("IMG_1"), "pic_1");
    }

    #[test]
    fn lookaround_and_groups() {
        assert_eq!(Subst::new(r"(?<=a)b", "X", false, false).unwrap().apply("ab cb"), "aX cb");
        assert_eq!(Subst::new(r"(\d+)-(\d+)", "$2-$1", false, false).unwrap().apply("12-34"), "34-12");
        assert_eq!(Subst::new(r"(a)\1", "!", false, false).unwrap().apply("aab"), "!b");
    }

    fn run(s: &Subst, text: &str) -> String {
        let mut out = text.to_owned();
        for (r, t) in s.edits(text).into_iter().rev() {
            out.replace_range(r, &t);
        }
        out
    }

    #[test]
    fn edits_agree_with_apply() {
        for (p, r, all) in [("a", "b", true), ("a", "b", false), (r"(\d+)-(\d+)", "$2-$1", true), ("x*", "-", true)] {
            let s = Subst::new(p, r, all, false).unwrap();
            for t in ["aaa", "12-34 5-6", "abc", "a\nb\n", "日本a語"] {
                assert_eq!(run(&s, t), s.apply(t), "{p} {t:?}");
            }
        }
    }

    #[test]
    fn edits_carry_ranges_and_expansions() {
        let s = Subst::new(r"(?<k>\w)=(\d)", "${k}:$2", true, false).unwrap();
        assert_eq!(s.edits("a=1 b=2"), vec![(0..3, "a:1".to_owned()), (4..7, "b:2".to_owned())]);
        let first = Subst::new("a", "b", false, false).unwrap();
        assert_eq!(first.edits("aa").len(), 1);
    }

    #[test]
    fn empty_matches_are_edits_and_terminate() {
        let q = Subst::from_query("^", "> ", Query::default()).unwrap();
        let e: Vec<usize> = q.edits("ab\ncd").into_iter().map(|(r, _)| r.start).collect();
        assert_eq!(e, vec![0, 3]);
        assert_eq!(Subst::from_query("x*", "-", Query::default()).unwrap().edits("日本").len(), 3);
    }

    #[test]
    fn edits_in_a_selection() {
        let s = Subst::from_query("a", "X", Query::default()).unwrap();
        let e = s.edits_in("aaaa", 1..3);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].0, 1..2);
        assert!(s.edits_in("aaaa", 4..4).is_empty());
        let b = Subst::from_query(r"(?<=a)b", "X", Query::default()).unwrap();
        assert_eq!(b.edits_in("ab", 1..2).len(), 1, "lookaround sees outside the selection");
        assert!(Subst::from_query("ab", "X", Query::default()).unwrap().edits_in("ab", 0..1).is_empty());
    }

    #[test]
    fn query_options() {
        let q = |query, o: Query| Subst::from_query(query, "[$0]", o).unwrap();
        assert_eq!(q("img", Query::default()).apply("IMG img"), "[IMG] [img]");
        assert_eq!(q("Img", Query::default()).apply("IMG Img"), "IMG [Img]");
        let lit = Query { regex: false, ..Query::default() };
        assert_eq!(Subst::from_query("a.b", "$", lit).unwrap().apply("a.b axb"), "$ axb");
        let w = Query { whole_word: true, ..Query::default() };
        assert_eq!(q("cat", w).apply("cat concat cat_ cat"), "[cat] concat cat_ [cat]");
        assert!(Subst::from_query("(", "", Query::default()).is_err());
    }

    #[test]
    fn a_bad_pattern_says_why() {
        assert!(Subst::new("(", "", false, false).is_err());
    }
}
