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

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

#[cfg(feature = "themes")]
pub mod themed;

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

/// A name a line declares: `fn main` gives `main` with kind `function`. The
/// kind is one of `function`, `class`, `struct`, `enum`, `union`, `trait`,
/// `interface`, `impl`, `namespace`, `mod`, `macro`, `type`, and `table` (a
/// TOML table); the app picks the kinds it lists and nests them by indent. It is
/// read from the whole scope stack (see [`decl_kind`]), so a name that only
/// refers to something (a call, a type annotation) is not a declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decl {
    pub range: Range<usize>,
    pub kind: &'static str,
}

/// A line longer than this (minified code) is left plain and not parsed.
pub const MAX_LINE: usize = 2000;

/// bat's collection, loaded once per process: loading it is slow enough to be
/// felt if every test or every file did it.
pub(crate) fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

pub(crate) fn find(path: &str) -> Option<&'static SyntaxReference> {
    let set = syntaxes();
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    // By the whole name first (`Dockerfile`, `Makefile`), then the extension.
    let found = set.find_syntax_by_extension(name).or_else(|| name.rsplit_once('.').and_then(|(_, e)| set.find_syntax_by_extension(e)));
    found.filter(|s| s.name != "Plain Text")
}

/// The grammar for a language name or extension as written in a Markdown fence
/// or an HTML `class="language-…"` (`rust`, `py3`, `shell`, `yml`, `c++`). Common
/// aliases are mapped first; `text`, `plain` and the like are no language.
pub(crate) fn find_token(token: &str) -> Option<&'static SyntaxReference> {
    let set = syntaxes();
    let lower = token.to_ascii_lowercase();
    let alias = match lower.as_str() {
        "text" | "txt" | "plain" | "plaintext" | "none" | "output" => return None,
        "shell" | "console" | "shellsession" | "sh-session" | "zsh" | "ksh" | "fish" => "bash",
        "jsonc" | "json5" | "jsonl" | "ndjson" => "json",
        "yml" => "yaml",
        "jsx" | "mjs" | "cjs" | "node" => "js",
        "docker" | "containerfile" => "dockerfile",
        "c++" => "cpp",
        "golang" => "go",
        "rs" => "rust",
        "py3" | "python3" => "python",
        "hcl" | "tf" => "terraform",
        "cmd" | "batch" => "bat",
        "patch" => "diff",
        "html5" | "xhtml" | "vue-html" => "html",
        "cfg" | "conf" | "dosini" => "ini",
        other => other,
    };
    set.find_syntax_by_token(alias).or_else(|| set.find_syntax_by_token(token)).filter(|s| s.name != "Plain Text")
}

/// The language word of a fence's info string (`rust`, `rust,ignore`, `{.python}`).
pub fn fence_lang(info: &str) -> Option<&str> {
    info.split(|c: char| c.is_whitespace() || c == ',' || c == '{' || c == '}').find(|s| !s.is_empty()).map(|l| l.trim_start_matches('.'))
}

/// Whether a language name (a fence's, an HTML class's) is known.
pub fn knows_token(token: &str) -> bool {
    find_token(token).is_some()
}

/// The language word of a `#!` first line (`#!/usr/bin/env python3` gives
/// `python3`, `#!/bin/bash -e` gives `bash`); `None` for any other line. For a
/// file whose name says no language (see [`knows`]).
pub fn shebang_token(first: &str) -> Option<&str> {
    first.strip_prefix("#!")?.split_whitespace().rfind(|w| !w.starts_with('-') && *w != "env")?.rsplit('/').next()
}

impl Stream {
    /// A stream for the language a `#!` first line names.
    pub fn for_shebang(first: &str) -> Option<Stream> {
        Stream::for_token(shebang_token(first)?)
    }

    /// A stream for a language name or extension (`rust`, `py3`, `shell`).
    pub fn for_token(token: &str) -> Option<Stream> {
        let syntax = find_token(token)?;
        Some(Stream { syntax, state: ParseState::new(syntax), stack: ScopeStack::new() })
    }

    /// A stream for a fence's info string (`rust,ignore`, `{.python}`).
    pub fn for_fence(info: &str) -> Option<Stream> {
        Stream::for_token(fence_lang(info)?)
    }
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
        self.line_decls(text).0
    }

    /// The same one parse as [`line`](Stream::line), also giving the
    /// declarations of the line (byte ranges, in order and apart).
    pub fn line_decls(&mut self, text: &str) -> (Ink, Vec<Decl>) {
        if text.len() > MAX_LINE {
            return (Vec::new(), Vec::new());
        }
        let set = syntaxes();
        let mut src = String::with_capacity(text.len() + 1);
        src.push_str(text);
        src.push('\n');
        let Ok(ops) = self.state.parse_line(&src, set) else { return (Vec::new(), Vec::new()) };
        let mut out: Ink = Vec::new();
        let mut decls: Vec<Decl> = Vec::new();
        let end = text.len();
        let mut put = |stack: &ScopeStack, from: usize, to: usize| {
            let (from, to) = (from.min(end), to.min(end));
            if from >= to {
                return;
            }
            if let Some(kind) = decl_kind(stack.as_slice()) {
                match decls.last_mut() {
                    Some(d) if d.range.end == from && d.kind == kind => d.range.end = to,
                    _ => decls.push(Decl { range: from..to, kind }),
                }
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
                return (Vec::new(), Vec::new());
            }
        }
        put(&self.stack, at, end);
        (out, decls)
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

const DECL_TAGS: [&str; 10] = ["class", "struct", "enum", "union", "trait", "interface", "impl", "namespace", "macro", "type"];

/// What the innermost `entity.name` on `stack` declares, if anything. Only the
/// stack tells a definition from a mention: TypeScript's and PHP's grammars name
/// a call `entity.name.function` inside `meta.function-call`, and TypeScript names
/// the types it refers to `entity.name.type` inside `meta.type.annotation`,
/// `meta.return.type` or `meta.type.parameters` (and a bare `entity.name.type`
/// in `source.ts` / `source.tsx`); none of those declare anything.
/// `entity.name.module` is `mod`, `type.alias` is `type`, `type.module` is
/// `namespace`, and `type.<class|struct|…>` is the word after `type`.
pub fn decl_kind(stack: &[Scope]) -> Option<&'static str> {
    let scope = |n: &str| Scope::new(n).ok();
    let entity = scope("entity.name")?;
    let name = stack.iter().rev().find(|sc| entity.is_prefix_of(**sc))?;
    let within = |names: &[&str]| names.iter().filter_map(|n| scope(n)).any(|p| stack.iter().any(|sc| p.is_prefix_of(*sc)));
    let full = name.build_string();
    let mut atoms = full.split('.').skip(2);
    let tag = |t: &str| DECL_TAGS.iter().copied().find(|&k| k == t);
    match atoms.next()? {
        "function" => (!within(&["meta.function-call"])).then_some("function"),
        "table" => Some("table"),
        "module" => Some("mod"),
        "type" => {
            if within(&["meta.type.annotation", "meta.return.type", "meta.type.parameters"]) {
                return None;
            }
            match atoms.next() {
                Some("alias") => Some("type"),
                Some("module") => Some("namespace"),
                Some(sub) if tag(sub).is_some() => tag(sub),
                _ if within(&["source.ts", "source.tsx"]) => None,
                _ => Some("type"),
            }
        }
        kind => tag(kind),
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
    fn declarations_come_from_the_same_parse() {
        let mut s = Stream::new("a.rs").unwrap();
        let text = "pub fn main() {";
        let (ink, decls) = s.line_decls(text);
        assert!(!ink.is_empty());
        assert_eq!(decls.iter().map(|d| (&text[d.range.clone()], d.kind)).collect::<Vec<_>>(), [("main", "function")]);
        let (_, d) = s.line_decls("struct Point {");
        assert_eq!((d.len(), d[0].kind), (1, "struct"));
        let (_, d) = s.line_decls("    let x = 1;");
        assert!(d.is_empty());
        assert_eq!(s.line_decls("}").1, []);
    }

    fn kinds(path: &str, src: &str) -> Vec<(String, &'static str)> {
        let mut s = Stream::new(path).unwrap();
        src.lines().flat_map(|l| s.line_decls(l).1.into_iter().map(move |d| (l[d.range].to_owned(), d.kind))).collect()
    }

    #[test]
    fn typescript_mentions_are_not_declarations() {
        let src = "const x = new Foo<Bar>();
let y: Map<string, Foo> = z as Baz;
function f(a: Foo): Bar { g(); return x.y(); }
export class C extends D implements I {
  method(x: number) {}
}
interface I { m(): void }
type T = A | B;
enum E { A }
namespace N {}
const arrow = (x) => x;
";
        let names: Vec<_> = kinds("a.ts", src).into_iter().filter(|(_, k)| *k != "function").map(|(n, k)| format!("{k} {n}")).collect();
        assert_eq!(names, ["class C", "interface I", "type T", "enum E", "namespace N"]);
        let fns: Vec<_> = kinds("a.ts", src).into_iter().filter(|(_, k)| *k == "function").map(|d| d.0).collect();
        assert_eq!(fns, ["f", "method", "m", "arrow"]);
        assert!(kinds("a.tsx", "export function App(): JSX.Element { return <Foo /> }").iter().all(|(n, _)| n == "App"));
    }

    #[test]
    fn a_shebang_names_the_language() {
        assert_eq!(shebang_token("#!/usr/bin/env python3"), Some("python3"));
        assert_eq!(shebang_token("#!/bin/bash -e"), Some("bash"));
        assert_eq!(shebang_token("fn main() {}"), None);
        assert!(Stream::for_shebang("#!/usr/bin/env python3").is_some());
        assert!(Stream::for_shebang("#!/usr/bin/env nosuchlang").is_none());
    }

    #[test]
    fn rust_and_go_kinds() {
        let rs = kinds(
            "a.rs",
            "pub struct App {
}
enum E { A }
trait T { fn t(&self); }
impl App {
  fn new() {}
}
mod tests {}
",
        );
        let rs: Vec<_> = rs.iter().map(|(n, k)| format!("{k} {n}")).collect();
        assert_eq!(rs, ["struct App", "enum E", "trait T", "function t", "impl App", "function new", "mod tests"]);
        let go = kinds(
            "a.go",
            "func main() { f() }
type S struct {}
",
        );
        assert_eq!(go, [("main".to_owned(), "function"), ("S".to_owned(), "type")]);
    }

    #[test]
    fn a_toml_table_is_a_table() {
        let mut s = Stream::new("Cargo.toml").unwrap();
        let text = "[package]";
        let (_, d) = s.line_decls(text);
        assert!(d.iter().any(|d| d.kind == "table"), "{d:?}");
    }

    #[test]
    fn fence_words_find_languages() {
        assert_eq!(fence_lang("rust,ignore"), Some("rust"));
        assert_eq!(fence_lang("{.python}"), Some("python"));
        assert_eq!(fence_lang("  "), None);
        assert!(knows_token("rust") && knows_token("py3") && knows_token("shell") && knows_token("yml") && knows_token("RS"));
        assert!(!knows_token("text") && !knows_token("zzzunknown"));
        let ink = Stream::for_fence("rust,ignore").unwrap().line("let a = 1;");
        assert_eq!(word("let a = 1;", &ink, Tok::Keyword), ["let"]);
    }

    #[test]
    fn names_and_toml_are_known() {
        assert!(knows("Cargo.toml"));
        assert!(knows("C:\\dev\\x\\main.rs"));
        assert!(knows("Dockerfile"));
        assert!(!highlight("Cargo.toml", ["name = \"ito\""])[0].is_empty());
    }
}
