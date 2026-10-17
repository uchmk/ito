//! sed-style substitution over a text (`s/pat/rep/[gi]` without the spelling):
//! the same regex dialect as [`crate::Matcher`], with lookaround and
//! backreferences, and `$1` / `${name}` in the replacement.

use fancy_regex::Regex;

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

    #[test]
    fn a_bad_pattern_says_why() {
        assert!(Subst::new("(", "", false, false).is_err());
    }
}
