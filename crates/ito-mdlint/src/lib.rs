//! Lint for a Markdown text. Pure functions, no window, no file reads: the text goes in, a list of [`Finding`]s comes
//! out, each with the byte range, a [`Kind`] (the app turns it into its own words), a [`Severity`] and, when there is
//! one, the fix as an [`ito_mdedit::Change`]. An editor shows the fixes as a diff and applies them as one undo step
//! ([`fix_all`] merges the fixes of a kind into a single `Change`).
//!
//! Structure: heading level jumps, empty and duplicate headings, unevenly indented and mixed-marker lists, table
//! rows with the wrong number of cells, trailing spaces, an unclosed code fence, link targets that do not exist
//! (the caller says which paths exist), undefined reference links.
//!
//! Japanese text: a space (or none) between full-width and half-width characters against a chosen rule, mixed
//! punctuation (`、。` against `，．`), full-width spaces and letters, a mix of `です・ます` and `である`.
//!
//! Front matter, fenced code, inline code and URLs are never checked as prose. CRLF and LF both work; a fix that
//! has to add a line break uses the text's own. The scan is one linear pass, cheap enough to run on a worker for a
//! 1 MiB text on every new version; [`lint_in`] keeps the findings that touch a byte range.
//!
//! All positions are byte offsets on `char` boundaries of the given text.

use std::ops::Range;

pub use ito_mdedit::Change;

mod japanese;
mod scan;
mod structure;

/// What a finding is about. The app maps each to its own words (`key` is a stable id for a `lang/*.toml`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Kind {
    /// `#` then `###`: a heading more than one level deeper than the one before. Fix: the level it would have.
    HeadingJump,
    /// A heading with no text.
    EmptyHeading,
    /// The same heading text twice at the same level under one parent.
    DuplicateHeading,
    /// A list item indented differently from its siblings (3 spaces where the others have 2). Fix: the siblings' indent.
    UnevenListIndent,
    /// Siblings that use `-` and `*` (or `.` and `)`). Fix: the first item's marker.
    MixedListMarker,
    /// A table row with a different number of cells than the header. Fix for a short row ending in `|`: empty cells.
    TableCellCount,
    /// Spaces or tabs at the end of a line (exactly two before more text is a line break and is left alone). Fix: remove.
    TrailingSpaces,
    /// A code fence that is never closed. Fix: a closing fence at the end.
    UnclosedFence,
    /// A link or image to a relative path for which the `exists` callback said no.
    MissingTarget,
    /// `[text][label]` with no `[label]: …` definition.
    UndefinedReference,
    /// Half-width letters or digits touching Japanese text, when the rule is [`Spacing::Space`]. Fix: insert a space.
    CjkSpaceMissing,
    /// A space between half-width letters or digits and Japanese text, when the rule is [`Spacing::NoSpace`]. Fix: remove.
    CjkSpaceExtra,
    /// `、。` mixed with `，．` (or the other style than the one chosen). Fix: the other style.
    MixedPunctuation,
    /// A full-width space (U+3000). Fix: a half-width space.
    FullwidthSpace,
    /// Full-width letters or digits (`ＡＢＣ１２３`). Fix: half-width.
    FullwidthAlnum,
    /// A sentence in the other of `です・ます` and `である` than the text's main one. No fix.
    MixedTone,
}

impl Kind {
    /// Every kind, in a fixed order.
    pub const ALL: [Kind; 16] = [
        Kind::HeadingJump,
        Kind::EmptyHeading,
        Kind::DuplicateHeading,
        Kind::UnevenListIndent,
        Kind::MixedListMarker,
        Kind::TableCellCount,
        Kind::TrailingSpaces,
        Kind::UnclosedFence,
        Kind::MissingTarget,
        Kind::UndefinedReference,
        Kind::CjkSpaceMissing,
        Kind::CjkSpaceExtra,
        Kind::MixedPunctuation,
        Kind::FullwidthSpace,
        Kind::FullwidthAlnum,
        Kind::MixedTone,
    ];

    /// A stable lower-case id such as `heading-jump`, for a language file or a settings key.
    pub fn key(self) -> &'static str {
        match self {
            Kind::HeadingJump => "heading-jump",
            Kind::EmptyHeading => "empty-heading",
            Kind::DuplicateHeading => "duplicate-heading",
            Kind::UnevenListIndent => "uneven-list-indent",
            Kind::MixedListMarker => "mixed-list-marker",
            Kind::TableCellCount => "table-cell-count",
            Kind::TrailingSpaces => "trailing-spaces",
            Kind::UnclosedFence => "unclosed-fence",
            Kind::MissingTarget => "missing-target",
            Kind::UndefinedReference => "undefined-reference",
            Kind::CjkSpaceMissing => "cjk-space-missing",
            Kind::CjkSpaceExtra => "cjk-space-extra",
            Kind::MixedPunctuation => "mixed-punctuation",
            Kind::FullwidthSpace => "fullwidth-space",
            Kind::FullwidthAlnum => "fullwidth-alnum",
            Kind::MixedTone => "mixed-tone",
        }
    }

    /// The kind with this [`key`](Kind::key).
    pub fn from_key(key: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == key)
    }

    /// True for the checks about Japanese prose, false for the structure checks.
    pub fn is_japanese(self) -> bool {
        matches!(
            self,
            Kind::CjkSpaceMissing
                | Kind::CjkSpaceExtra
                | Kind::MixedPunctuation
                | Kind::FullwidthSpace
                | Kind::FullwidthAlnum
                | Kind::MixedTone
        )
    }

    /// How serious the kind is unless the rules say otherwise.
    pub fn default_severity(self) -> Severity {
        match self {
            Kind::UnclosedFence | Kind::TableCellCount => Severity::Error,
            Kind::TrailingSpaces | Kind::FullwidthSpace | Kind::FullwidthAlnum | Kind::CjkSpaceMissing | Kind::CjkSpaceExtra => {
                Severity::Info
            }
            _ => Severity::Warning,
        }
    }

    pub(crate) fn index(self) -> usize {
        Kind::ALL.iter().position(|k| *k == self).unwrap_or(0)
    }
}

/// How serious a finding is. Ordered: `Info < Warning < Error`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// The rule for spaces between half-width letters or digits and Japanese characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Spacing {
    /// Do not check (the default: the house styles differ).
    #[default]
    Any,
    /// `日本語 ABC 日本語`: a space is required.
    Space,
    /// `日本語ABC日本語`: no space.
    NoSpace,
}

/// The rule for the punctuation of Japanese text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Punctuation {
    /// Flag the style that is the minority in the text (a tie counts as `、。`). Nothing if only one style is used.
    #[default]
    Auto,
    /// `、。` only: flag `，．`.
    Ideographic,
    /// `，．` only: flag `、。`.
    Western,
}

/// The rule for the sentence endings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Flag the sentences that are the minority (a tie goes to the style that comes first).
    #[default]
    Auto,
    /// `です・ます` only: flag `である`.
    Polite,
    /// `である` only: flag `です・ます`.
    Plain,
}

/// Which checks run, how serious each is, and the options of the Japanese checks.
#[derive(Clone, Debug)]
pub struct Rules {
    levels: [Option<Severity>; Kind::ALL.len()],
    /// Spaces between half-width letters or digits and Japanese text (kinds `CjkSpaceMissing` / `CjkSpaceExtra`).
    pub spacing: Spacing,
    /// Punctuation style (kind `MixedPunctuation`).
    pub punctuation: Punctuation,
    /// Sentence ending style (kind `MixedTone`).
    pub tone: Tone,
}

impl Default for Rules {
    /// Every kind on at its default severity; `spacing` is `Any`, `punctuation` and `tone` are `Auto`.
    fn default() -> Self {
        let mut levels = [None; Kind::ALL.len()];
        for k in Kind::ALL {
            levels[k.index()] = Some(k.default_severity());
        }
        Rules { levels, spacing: Spacing::Any, punctuation: Punctuation::Auto, tone: Tone::Auto }
    }
}

impl Rules {
    /// No check at all; turn on the ones wanted with [`set`](Rules::set).
    pub fn none() -> Self {
        Rules { levels: [None; Kind::ALL.len()], ..Rules::default() }
    }

    /// The severity of `kind`, or `None` when the check is off.
    pub fn level(&self, kind: Kind) -> Option<Severity> {
        self.levels[kind.index()]
    }

    /// Turn `kind` off (`None`) or on at a severity.
    pub fn set(&mut self, kind: Kind, level: Option<Severity>) -> &mut Self {
        self.levels[kind.index()] = level;
        self
    }

    /// Turn `kind` on at its default severity.
    pub fn enable(&mut self, kind: Kind) -> &mut Self {
        self.set(kind, Some(kind.default_severity()))
    }

    /// Turn `kind` off.
    pub fn disable(&mut self, kind: Kind) -> &mut Self {
        self.set(kind, None)
    }

    /// Turn every Japanese-text check off (for a text in another language).
    pub fn without_japanese(mut self) -> Self {
        for k in Kind::ALL.into_iter().filter(|k| k.is_japanese()) {
            self.levels[k.index()] = None;
        }
        self
    }
}

/// One problem found in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// Where it is (bytes of the text). Never spans a line break.
    pub range: Range<usize>,
    pub kind: Kind,
    pub severity: Severity,
    /// The fix, in bytes of the text that was linted, when there is one.
    pub fix: Option<Change>,
}

/// Lint `text`. `exists` is asked about each relative link target (percent-decoded, without `#fragment` and
/// `?query`, as written relative to the document); return true for a path that is there. The findings are sorted by
/// position.
pub fn lint(text: &str, rules: &Rules, exists: &dyn Fn(&str) -> bool) -> Vec<Finding> {
    let scan = scan::scan(text);
    let mut sink = Sink { rules, out: Vec::new() };
    structure::run(text, &scan, exists, &mut sink);
    japanese::run(text, &scan, &mut sink);
    let mut out = sink.out;
    out.sort_by_key(|f| (f.range.start, f.range.end));
    out
}

/// The findings of [`lint`] that touch `range` (an empty `range` matches the findings that contain its position).
/// The scan is still of the whole text: fences and front matter need the lines before.
pub fn lint_in(text: &str, rules: &Rules, exists: &dyn Fn(&str) -> bool, range: Range<usize>) -> Vec<Finding> {
    let mut v = lint(text, rules, exists);
    v.retain(|f| f.range.start < range.end.max(range.start + 1) && range.start < f.range.end.max(f.range.start + 1));
    v
}

/// The fixes of the findings of `kind`, in position order.
pub fn fixes(findings: &[Finding], kind: Kind) -> Vec<Change> {
    findings.iter().filter(|f| f.kind == kind).filter_map(|f| f.fix.clone()).collect()
}

/// All the fixes (of `kind`, or of every kind) as ONE change, so an editor applies "fix all" as one undo step. The
/// caret goes to the end of the replaced text. A fix that overlaps an earlier one is left out (lint again and apply
/// the rest). `None` when there is nothing to fix.
pub fn fix_all(text: &str, findings: &[Finding], kind: Option<Kind>) -> Option<Change> {
    let all: Vec<Change> = findings.iter().filter(|f| kind.is_none_or(|k| f.kind == k)).filter_map(|f| f.fix.clone()).collect();
    merge(text, &all)
}

/// Merge changes (all in bytes of `text`) into one change over the span from the first to the last. Overlapping
/// changes after the first of them are dropped. `None` for an empty list.
pub fn merge(text: &str, changes: &[Change]) -> Option<Change> {
    let mut v: Vec<&Change> = changes.iter().filter(|c| c.range.start <= c.range.end && c.range.end <= text.len()).collect();
    v.sort_by_key(|c| (c.range.start, c.range.end));
    let mut kept: Vec<&Change> = Vec::new();
    for c in v {
        if kept.last().is_none_or(|l| c.range.start >= l.range.end) {
            kept.push(c);
        }
    }
    let first = kept.first()?;
    let last = kept.last()?;
    let mut out = String::new();
    let mut pos = first.range.start;
    for c in &kept {
        out.push_str(&text[pos..c.range.start]);
        out.push_str(&c.text);
        pos = c.range.end;
    }
    let cursor = first.range.start + out.len();
    Some(Change { range: first.range.start..last.range.end, text: out, cursor })
}

/// Collects findings of the kinds that are on.
pub(crate) struct Sink<'a> {
    pub rules: &'a Rules,
    pub out: Vec<Finding>,
}

impl Sink<'_> {
    pub fn on(&self, kind: Kind) -> bool {
        self.rules.level(kind).is_some()
    }

    pub fn add(&mut self, kind: Kind, range: Range<usize>, fix: Option<Change>) {
        if let Some(severity) = self.rules.level(kind) {
            self.out.push(Finding { range, kind, severity, fix });
        }
    }

    /// A finding whose fix replaces `range` with `with`.
    pub fn replace(&mut self, kind: Kind, range: Range<usize>, with: impl Into<String>) {
        let text: String = with.into();
        let cursor = range.start + text.len();
        let fix = Change { range: range.clone(), text, cursor };
        self.add(kind, range, Some(fix));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn run(text: &str) -> Vec<Finding> {
        lint(text, &Rules::default(), &|_| true)
    }

    pub(crate) fn of(text: &str, kind: Kind) -> Vec<Finding> {
        run(text).into_iter().filter(|f| f.kind == kind).collect()
    }

    /// The text after applying every fix of `kind`.
    pub(crate) fn fixed(text: &str, kind: Kind) -> String {
        let f = run(text);
        match fix_all(text, &f, Some(kind)) {
            Some(c) => c.apply(text),
            None => text.to_string(),
        }
    }

    #[test]
    fn keys_round_trip_and_are_unique() {
        for k in Kind::ALL {
            assert_eq!(Kind::from_key(k.key()), Some(k));
        }
        assert_eq!(Kind::from_key("nope"), None);
        assert_eq!(Kind::ALL.iter().map(|k| k.index()).sum::<usize>(), (0..16).sum::<usize>());
    }

    #[test]
    fn rules_turn_kinds_on_and_off() {
        let mut r = Rules::none();
        assert!(Kind::ALL.iter().all(|k| r.level(*k).is_none()));
        r.enable(Kind::HeadingJump).set(Kind::EmptyHeading, Some(Severity::Error));
        let f = lint("# a\n### b\n##\n", &r, &|_| true);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].kind, Kind::HeadingJump);
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[1].severity, Severity::Error);
        let mut r = Rules::default();
        r.disable(Kind::HeadingJump);
        assert!(lint("# a\n### b\n", &r, &|_| true).is_empty());
    }

    #[test]
    fn clean_text_has_no_findings() {
        let t = "---\ntitle: x\n---\n\n# Title\n\n## One\n\n- a\n- b\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nText [l](#x) and `code`.\n";
        assert_eq!(run(t), vec![]);
    }

    #[test]
    fn findings_are_sorted_and_lint_in_filters() {
        let t = "# a\n### b\ntext  \n";
        let f = run(t);
        assert!(f.windows(2).all(|w| w[0].range.start <= w[1].range.start));
        let only = lint_in(t, &Rules::default(), &|_| true, 4..5);
        assert!(only.iter().all(|f| f.kind == Kind::HeadingJump));
        assert!(!only.is_empty());
        assert!(lint_in(t, &Rules::default(), &|_| true, 0..1).is_empty());
    }

    #[test]
    fn merge_makes_one_change_and_drops_overlaps() {
        let t = "abcdef";
        let c = |a: usize, b: usize, s: &str| Change { range: a..b, text: s.into(), cursor: a + s.len() };
        let m = merge(t, &[c(4, 5, "X"), c(0, 1, "YY"), c(0, 3, "Z")]).unwrap();
        assert_eq!(m.range, 0..5);
        assert_eq!(m.apply(t), "YYbcdXf");
        assert_eq!(m.cursor, m.text.len());
        assert_eq!(merge(t, &[]), None);
        assert_eq!(merge(t, &[c(2, 2, "-"), c(2, 2, "+")]).unwrap().apply(t), "ab-+cdef");
    }
}
