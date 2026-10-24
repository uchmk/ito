//! Syntax kinds for code: which kind of token each stretch of a line is.
//! Only the kind (keyword, string, ...) is decided here; the app maps it to a
//! colour of the theme, so a light theme and a dark one both read well.
//!
//! The grammar set (bat's collection) is loaded once per process. A [`Stream`]
//! reads lines one after another and carries the parser state between them, so
//! a block comment or a multi-line string runs over lines. A diff hunk is read
//! as two streams (old side, new side); a hunk starts mid-file, so a string or
//! comment opened above it may be coloured wrongly: that is the price of
//! colouring without reading the whole file.

use std::ops::Range;
use std::sync::OnceLock;

use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// The kind of a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Keyword,
    Str,
    Comment,
    Number,
    /// `true`, `null`, constants.
    Const,
    Type,
    Func,
}

/// The spans of a line: byte ranges of its text, in order and apart.
pub type Ink = Vec<(Range<usize>, Tok)>;

/// A line longer than this (minified code) is left plain and not parsed.
pub const MAX_LINE: usize = 1500;

/// bat's collection, loaded once per process: loading it is slow enough to be
/// felt if every test or every file did it.
fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn find(path: &str) -> Option<&'static SyntaxReference> {
    let set = syntaxes();
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    // By the whole name first (`Dockerfile`, `Makefile`), then the extension.
    let found = set.find_syntax_by_extension(name).or_else(|| name.rsplit_once('.').and_then(|(_, e)| set.find_syntax_by_extension(e)));
    found.filter(|s| s.name != "Plain Text")
}

/// Whether a language is known for `path` (by file name, then extension).
pub fn knows(path: &str) -> bool {
    find(path).is_some()
}

/// A reader of the lines of one file (or one side of a hunk), in order.
pub struct Stream {
    syntax: &'static SyntaxReference,
    state: ParseState,
    stack: ScopeStack,
}

impl Stream {
    /// A stream for the language of `path`; `None` for a language it does not
    /// know (the caller leaves the text plain).
    pub fn new(path: &str) -> Option<Stream> {
        let syntax = find(path)?;
        Some(Stream { syntax, state: ParseState::new(syntax), stack: ScopeStack::new() })
    }

    /// Forget what the earlier lines opened (a new hunk, a new side).
    pub fn reset(&mut self) {
        self.state = ParseState::new(self.syntax);
        self.stack = ScopeStack::new();
    }

    /// The tokens of the next line (without its newline), advancing the
    /// parser. A line over [`MAX_LINE`] bytes gives no tokens and is not
    /// parsed.
    pub fn line(&mut self, text: &str) -> Ink {
        if text.len() > MAX_LINE {
            return Vec::new();
        }
        let set = syntaxes();
        let mut src = String::with_capacity(text.len() + 1);
        src.push_str(text);
        src.push('\n');
        let Ok(ops) = self.state.parse_line(&src, set) else { return Vec::new() };
        let mut out: Ink = Vec::new();
        let end = text.len();
        let mut put = |stack: &ScopeStack, from: usize, to: usize| {
            let (from, to) = (from.min(end), to.min(end));
            if from >= to {
                return;
            }
            let Some(tok) = stack.as_slice().iter().rev().find_map(|s| classify(&s.build_string())) else { return };
            match out.last_mut() {
                Some((r, t)) if r.end == from && *t == tok => r.end = to,
                _ => out.push((from..to, tok)),
            }
        };
        let mut at = 0;
        for (pos, op) in ops {
            put(&self.stack, at, pos);
            at = pos;
            // A malformed op only loses the colour.
            if self.stack.apply(&op).is_err() {
                return Vec::new();
            }
        }
        put(&self.stack, at, end);
        out
    }
}

/// The tokens of each of `lines`, read in order as one file. Empty for every
/// line when the language of `path` is not known.
pub fn highlight<'a>(path: &str, lines: impl IntoIterator<Item = &'a str>) -> Vec<Ink> {
    match Stream::new(path) {
        Some(mut s) => lines.into_iter().map(|l| s.line(l)).collect(),
        None => lines.into_iter().map(|_| Vec::new()).collect(),
    }
}

/// What a scope (`keyword.control.rust`) is.
pub fn classify(scope: &str) -> Option<Tok> {
    let is = |p: &str| scope == p || scope.strip_prefix(p).is_some_and(|r| r.starts_with('.'));
    if is("comment") {
        Some(Tok::Comment)
    } else if is("string") {
        Some(Tok::Str)
    } else if is("constant.numeric") {
        Some(Tok::Number)
    } else if is("constant.language") || is("constant.character") || is("constant.other") || is("support.constant") {
        Some(Tok::Const)
    } else if is("keyword.operator") || is("keyword.other.unit") {
        None
    } else if is("keyword") || is("storage") || is("entity.name.tag") {
        Some(Tok::Keyword)
    } else if is("entity.name.function") || is("support.function") || is("variable.function") || is("meta.function-call") {
        Some(Tok::Func)
    } else if ["entity.name.type", "entity.name.class", "entity.name.struct", "entity.name.enum", "entity.name.trait"]
        .iter()
        .chain(&["entity.name.namespace", "entity.other.inherited-class", "support.type", "support.class"])
        .any(|p| is(p))
    {
        Some(Tok::Type)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word<'a>(text: &'a str, ink: &Ink, tok: Tok) -> Vec<&'a str> {
        ink.iter().filter(|(_, t)| *t == tok).map(|(r, _)| &text[r.clone()]).collect()
    }

    #[test]
    fn scopes_become_kinds() {
        assert_eq!(classify("keyword.control.rust"), Some(Tok::Keyword));
        assert_eq!(classify("keyword.operator.arithmetic.rust"), None);
        assert_eq!(classify("string.quoted.double.rust"), Some(Tok::Str));
        assert_eq!(classify("comment.line.double-slash.rust"), Some(Tok::Comment));
        assert_eq!(classify("constant.numeric.integer.decimal.rust"), Some(Tok::Number));
        assert_eq!(classify("constant.language.boolean.rust"), Some(Tok::Const));
        assert_eq!(classify("entity.name.function.rust"), Some(Tok::Func));
        assert_eq!(classify("storage.type.struct.rust"), Some(Tok::Keyword));
        assert_eq!(classify("source.rust"), None);
        // A prefix of a name is not the name.
        assert_eq!(classify("stringent"), None);
    }

    #[test]
    fn rust_lines_are_read_in_order() {
        let lines = ["fn main() {", "    let x = 1; // old", "    let s = \"new\";", "}"];
        let ink = highlight("src/main.rs", lines);
        assert_eq!(word(lines[0], &ink[0], Tok::Keyword), ["fn"]);
        assert_eq!(word(lines[1], &ink[1], Tok::Keyword), ["let"]);
        assert_eq!(word(lines[1], &ink[1], Tok::Number), ["1"]);
        assert!(word(lines[1], &ink[1], Tok::Comment).iter().any(|c| c.contains("old")));
        assert_eq!(word(lines[2], &ink[2], Tok::Str).concat(), "\"new\"");
        // Ranges stay inside the text, in order and apart.
        for (l, ink) in lines.iter().zip(&ink) {
            let mut end = 0;
            for (r, _) in ink {
                assert!(r.start >= end && r.end <= l.len() && r.start < r.end);
                end = r.end;
            }
        }
    }

    #[test]
    fn an_unknown_language_or_giant_line_stays_plain() {
        assert!(!knows("notes.zzzunknown"));
        assert!(Stream::new("notes.zzzunknown").is_none());
        assert_eq!(highlight("notes.zzzunknown", ["fn main() {}"]), vec![Ink::new()]);
        let long = "let a = 1; ".repeat(300);
        assert!(highlight("a.rs", [long.as_str()])[0].is_empty());
    }

    #[test]
    fn a_block_comment_runs_over_lines_and_reset_forgets_it() {
        let ink = highlight("a.rs", ["/* one", "   two */ let a = 1;"]);
        assert_eq!(word("/* one", &ink[0], Tok::Comment), ["/* one"]);
        assert_eq!(word("   two */ let a = 1;", &ink[1], Tok::Keyword), ["let"]);
        let mut s = Stream::new("a.rs").unwrap();
        s.line("/* open");
        s.reset();
        assert_eq!(word("let a", &s.line("let a"), Tok::Keyword), ["let"]);
    }

    #[test]
    fn names_and_toml_are_known() {
        assert!(knows("Cargo.toml"));
        assert!(knows("C:\\dev\\x\\main.rs"));
        assert!(knows("Dockerfile"));
        assert!(!highlight("Cargo.toml", ["name = \"ito\""])[0].is_empty());
    }
}
