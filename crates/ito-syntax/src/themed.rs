//! Coloured spans for a named syntect theme (feature `themes`): what an app
//! gets when it paints the colours of `base16-ocean.dark` or `InspiredGitHub`
//! itself, rather than mapping [`Tok`](crate::Tok) to its own theme.
//!
//! The themes are syntect's bundled set, loaded once per process. A name the
//! set does not know falls back to `base16-ocean.dark`, then to any theme.

use std::ops::Range;
use std::sync::OnceLock;

use syntect::highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference};

use crate::{find, syntaxes, MAX_LINE};

/// How a stretch of text is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub rgb: [u8; 3],
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// The coloured spans of a line: byte ranges of its text, in order, apart, and
/// covering all of it.
pub type Spans = Vec<(Range<usize>, Style)>;

fn themes() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

/// The names of the bundled themes, sorted.
pub fn theme_names() -> Vec<String> {
    let mut names: Vec<String> = themes().themes.keys().cloned().collect();
    names.sort();
    names
}

fn theme(name: &str) -> Option<&'static Theme> {
    let set = &themes().themes;
    set.get(name).or_else(|| set.get("base16-ocean.dark")).or_else(|| set.values().next())
}

fn rgb(c: syntect::highlighting::Color) -> [u8; 3] {
    [c.r, c.g, c.b]
}

/// The colour `theme` gives the first of `scopes` (`"comment"`, `"markup.heading"`)
/// that it paints differently from its default text, or `None`. For an app that
/// takes a few accents (a heading, a link, a dim) from the theme.
pub fn scope_color(theme_name: &str, scopes: &[&str]) -> Option<[u8; 3]> {
    let theme = theme(theme_name)?;
    let hl = Highlighter::new(theme);
    let base = rgb(hl.get_default().foreground);
    scopes.iter().find_map(|s| {
        let style = hl.style_for_stack(&[Scope::new(s).ok()?]);
        Some(rgb(style.foreground)).filter(|&c| c != base)
    })
}

/// The default text colour of the theme.
pub fn foreground(theme_name: &str) -> Option<[u8; 3]> {
    Some(rgb(Highlighter::new(theme(theme_name)?).get_default().foreground))
}

/// A reader of the lines of one file in order, carrying the parser state like
/// [`Stream`](crate::Stream), but giving the theme's colours.
pub struct Themed {
    syntax: &'static SyntaxReference,
    theme: &'static Theme,
    parse: ParseState,
    state: HighlightState,
}

impl Themed {
    /// For the language of `path`; `None` for one it does not know (the caller
    /// leaves the text plain).
    pub fn new(path: &str, theme_name: &str) -> Option<Themed> {
        let syntax = find(path)?;
        let theme = theme(theme_name)?;
        let state = HighlightState::new(&Highlighter::new(theme), ScopeStack::new());
        Some(Themed { syntax, theme, parse: ParseState::new(syntax), state })
    }

    /// Forget what the earlier lines opened.
    pub fn reset(&mut self) {
        self.parse = ParseState::new(self.syntax);
        self.state = HighlightState::new(&Highlighter::new(self.theme), ScopeStack::new());
    }

    /// The spans of the next line (without its newline). A line over
    /// [`MAX_LINE`](crate::MAX_LINE) bytes gives none and is not parsed.
    pub fn line(&mut self, text: &str) -> Spans {
        if text.len() > MAX_LINE {
            return Vec::new();
        }
        let mut src = String::with_capacity(text.len() + 1);
        src.push_str(text);
        src.push('\n');
        let Ok(ops) = self.parse.parse_line(&src, syntaxes()) else { return Vec::new() };
        let hl = Highlighter::new(self.theme);
        let end = text.len();
        let mut out: Spans = Vec::new();
        let mut at = 0;
        for (style, piece) in HighlightIterator::new(&mut self.state, &ops, &src, &hl) {
            let (from, to) = (at.min(end), (at + piece.len()).min(end));
            at += piece.len();
            if from >= to {
                continue;
            }
            let s = Style {
                rgb: rgb(style.foreground),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
                underline: style.font_style.contains(FontStyle::UNDERLINE),
            };
            match out.last_mut() {
                Some((r, t)) if *t == s => r.end = to,
                _ => out.push((from..to, s)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_covered_in_the_themes_colours() {
        let mut t = Themed::new("a.rs", "base16-ocean.dark").unwrap();
        let text = "fn main() { 1 }";
        let spans = t.line(text);
        assert_eq!(spans.first().map(|s| s.0.start), Some(0));
        assert_eq!(spans.last().map(|s| s.0.end), Some(text.len()));
        for w in spans.windows(2) {
            assert_eq!(w[0].0.end, w[1].0.start);
            assert_ne!(w[0].1, w[1].1);
        }
        assert!(spans.len() > 1);
        // Another theme paints the same text with other colours.
        let other = Themed::new("a.rs", "InspiredGitHub").unwrap().line(text);
        assert_ne!(spans, other);
    }

    #[test]
    fn state_runs_over_lines_and_unknowns_stay_plain() {
        let mut t = Themed::new("a.rs", "base16-ocean.dark").unwrap();
        t.line("/* open");
        let second = t.line("still comment */");
        let comment = scope_color("base16-ocean.dark", &["comment"]).unwrap();
        assert_eq!(second[0].1.rgb, comment);
        assert!(Themed::new("a.zzzunknown", "base16-ocean.dark").is_none());
        assert!(t.line(&"x ".repeat(2000)).is_empty());
    }

    #[test]
    fn themes_are_named_and_a_bad_name_falls_back() {
        assert!(theme_names().iter().any(|n| n == "base16-ocean.dark"));
        assert_eq!(foreground("no-such-theme"), foreground("base16-ocean.dark"));
        assert!(scope_color("base16-ocean.dark", &["no.such.scope"]).is_none());
    }
}
