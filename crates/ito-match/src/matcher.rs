//! The one rule every search shares: `f`, `s`, `S`, `/` and `?` all take a
//! regular expression, so a plain word is a substring match and `\.log$` or
//! `^a.*b` work in every one of them without a mode to switch.
//!
//! Smart case, as the rest of filer does it: a query with no capital letter
//! ignores case, one with a capital does not. A capital only counts when it is
//! a letter the query asks for -- `\S` and `\W` are classes, not capitals.

use std::ops::Range;

use fancy_regex::Regex;

use crate::fuzzy;

#[derive(Clone, Debug)]
enum Kind {
    Regex(Regex),
    /// `F`: the letters in order, line by line (`core::fuzzy`).
    Fuzzy { needle: String, case_sensitive: bool },
}

#[derive(Clone, Debug)]
pub struct Matcher {
    kind: Kind,
}

/// Whether `query` names a capital letter itself, skipping what follows a `\`.
fn has_capital(query: &str) -> bool {
    let mut chars = query.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c.is_uppercase() {
            return true;
        }
    }
    false
}

impl Matcher {
    /// `Err` is the engine's own words for what is wrong with `query`, one line.
    pub fn new(query: &str) -> Result<Self, String> {
        // `(?m)`: `^` and `$` mean the start and end of a line, which is what
        // anyone searching a file's text means by them; on a name they change
        // nothing, as a name has one line.
        let pattern = if has_capital(query) { format!("(?m){query}") } else { format!("(?mi){query}") };
        Regex::new(&pattern)
            .map(|re| Self { kind: Kind::Regex(re) })
            .map_err(|e| e.to_string().lines().next().unwrap_or("invalid pattern").trim().to_owned())
    }

    /// The letters of `query` in order, anywhere in a line (`F`). Smart case
    /// as `new` has it. Never fails: any text is a fuzzy query.
    pub fn fuzzy(query: &str) -> Self {
        let case_sensitive = fuzzy::is_case_sensitive(query, true, false);
        let needle = if case_sensitive { query.to_owned() } else { query.to_lowercase() };
        Self { kind: Kind::Fuzzy { needle, case_sensitive } }
    }

    /// A runaway pattern (the engine gives up after a million backtracks) counts as no match.
    pub fn is_match(&self, text: &str) -> bool {
        match &self.kind {
            Kind::Regex(re) => re.is_match(text).unwrap_or(false),
            Kind::Fuzzy { .. } => text.lines().any(|l| self.fuzzy_hit(l).is_some()),
        }
    }

    fn fuzzy_hit(&self, line: &str) -> Option<fuzzy::Hit> {
        let Kind::Fuzzy { needle, case_sensitive } = &self.kind else { return None };
        if needle.is_empty() {
            return None;
        }
        if *case_sensitive {
            fuzzy::match_str(needle, line, true)
        } else {
            fuzzy::match_str(needle, &line.to_lowercase(), true)
        }
    }

    /// Where `text` matches, as byte ranges, left to right. An empty match
    /// (`x*` on `abc`) marks nothing and is left out. A fuzzy match marks the
    /// matched letters of each line that has them.
    pub fn ranges(&self, text: &str) -> Vec<Range<usize>> {
        match &self.kind {
            Kind::Regex(re) => re.find_iter(text).filter_map(Result::ok).filter(|m| m.start() < m.end()).map(|m| m.range()).collect(),
            Kind::Fuzzy { .. } => {
                let mut out: Vec<Range<usize>> = Vec::new();
                let mut base = 0;
                for line in text.split_inclusive('\n') {
                    let body = line.trim_end_matches(['\n', '\r']);
                    if let Some(hit) = self.fuzzy_hit(body) {
                        let at: Vec<(usize, usize)> = body.char_indices().map(|(b, c)| (b, b + c.len_utf8())).collect();
                        for p in hit.positions {
                            // The lowercased line can be longer than the line
                            // (a few capitals fold to two characters); skip past the end.
                            let Some(&(s, e)) = at.get(p) else { continue };
                            match out.last_mut() {
                                Some(last) if last.end == base + s => last.end = base + e,
                                _ => out.push(base + s..base + e),
                            }
                        }
                    }
                    base += line.len();
                }
                out
            }
        }
    }

    /// The same places as character offsets, which is how a name is drawn
    /// (`core::fuzzy::Hit::positions` is the same shape).
    pub fn positions(&self, text: &str) -> Vec<usize> {
        let ranges = self.ranges(text);
        if ranges.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut ranges = ranges.into_iter().peekable();
        for (ci, (bi, _)) in text.char_indices().enumerate() {
            while ranges.peek().is_some_and(|r| bi >= r.end) {
                ranges.next();
            }
            match ranges.peek() {
                Some(r) if bi >= r.start => out.push(ci),
                Some(_) => {}
                None => break,
            }
        }
        out
    }

    /// The score of `text` as a fuzzy match, or `None` when it is not one.
    /// `F` sorts its results by this; a regular expression has no score.
    pub fn score(&self, text: &str) -> Option<i32> {
        match &self.kind {
            Kind::Fuzzy { .. } => text.lines().filter_map(|l| self.fuzzy_hit(l)).map(|h| h.score).max(),
            Kind::Regex(_) => self.is_match(text).then_some(0),
        }
    }

    pub fn is_fuzzy(&self) -> bool {
        matches!(self.kind, Kind::Fuzzy { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_word_is_a_substring_match() {
        let m = Matcher::new("awa").unwrap();
        assert!(m.is_match("awake.txt"));
        assert!(!m.is_match("a-w-a.txt"), "no longer a match for letters in order");
        assert_eq!(m.positions("xxawa"), vec![2, 3, 4]);
    }

    #[test]
    fn case_is_ignored_until_a_capital_is_typed() {
        assert!(Matcher::new("readme").unwrap().is_match("README.md"));
        assert!(!Matcher::new("Readme").unwrap().is_match("README.md"));
        assert!(Matcher::new("README").unwrap().is_match("README.md"));
        // `\S` is a class, not a capital: the rest is still case-blind.
        assert!(Matcher::new(r"\Sead").unwrap().is_match("README.md"));
    }

    #[test]
    fn a_regular_expression_works() {
        let m = Matcher::new(r"\.log$").unwrap();
        assert!(m.is_match("auto-wintest-arm.log"));
        assert!(!m.is_match("auto-wintest-arm.log.1"));
        assert!(Matcher::new("^a.*z$").unwrap().is_match("abcz"));
    }

    #[test]
    fn a_line_anchor_means_a_line_of_text() {
        let m = Matcher::new("^beta").unwrap();
        assert!(m.is_match("alpha\nbeta\n"));
        assert_eq!(m.ranges("alpha\nbeta\n"), vec![6..10]);
    }

    #[test]
    fn a_broken_pattern_says_why() {
        let e = Matcher::new("(").unwrap_err();
        assert!(!e.is_empty() && !e.contains('\n'), "{e:?}");
        assert!(Matcher::new("[a-").is_err());
    }

    #[test]
    fn positions_count_characters_not_bytes() {
        let m = Matcher::new("ログ").unwrap();
        assert_eq!(m.positions("a-ログ-b"), vec![2, 3]);
        assert_eq!(m.ranges("a-ログ"), vec![2..8]);
        let all = Matcher::new("a").unwrap();
        assert_eq!(all.positions("banana"), vec![1, 3, 5]);
    }

    #[test]
    fn fuzzy_marks_the_letters_line_by_line() {
        let m = Matcher::fuzzy("ab");
        assert!(m.is_match("xx
axxb
"));
        assert!(!m.is_match("ba
xx"));
        assert_eq!(m.ranges("xx
axxb
"), vec![3..4, 6..7]);
        assert_eq!(m.positions("a-b"), vec![0, 2]);
        assert!(m.score("ab").unwrap() > m.score("a--------b").unwrap());
        assert!(Matcher::fuzzy("").score("ab").is_none());
        assert!(Matcher::fuzzy("AB").score("ab").is_none(), "a capital makes it case-sensitive");
    }

    #[test]
    fn an_empty_match_marks_nothing() {
        let m = Matcher::new("x*").unwrap();
        assert!(m.is_match("abc"));
        assert!(m.ranges("abc").is_empty());
        assert!(m.positions("abc").is_empty());
    }
}
