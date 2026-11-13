//! Paths on the engine's machine, written the way that machine writes them.
//!
//! The window may be Windows while the listing is a Debian machine's, so nothing here uses `std::path`: every join,
//! split and comparison goes through [`Style`]. Paths are **lexical**, like a shell's `cd`: `..` takes off the last part
//! as written (even when that part is a link), `.` and repeated separators fold, nothing goes above the root, and a
//! trailing separator is dropped except on a root. The engine is the authority on what a path finally means.

use std::borrow::Cow;

/// How paths are written. This is data about the machine that lists, never the OS this program runs on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Style {
    /// `C:\dir\file`, `\\server\share\dir`. Both `\` and `/` are read, `\` is written. Names compare without case.
    Windows,
    /// `/dir/file`. Only `/` separates (`\` is an ordinary character). Names compare exactly.
    Posix,
}

/// Why a typed path could not be read.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum PathError {
    /// `\\server` or `\\server\` with no share.
    NoShare,
    /// `C:foo`: a path relative to a drive's own current folder, which only the engine's shell knows.
    DriveRelative,
    /// `\\?\...`, `\\.\...`, `\??\...`: the verbatim and device forms.
    Verbatim,
    /// A NUL character.
    Nul,
}

/// A path split into its root (with the trailing separator) and its parts.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Abs {
    root: String,
    parts: Vec<String>,
}

/// How a typed path starts.
enum Start<'a> {
    /// Complete: a root and what follows it.
    Abs { root: String, rest: &'a str },
    /// A leading single `\` (Windows): the root of the base.
    Rooted(&'a str),
    /// Relative to the base.
    Relative(&'a str),
}

impl Style {
    /// The separator written between parts.
    pub fn sep(self) -> char {
        match self {
            Style::Windows => '\\',
            Style::Posix => '/',
        }
    }

    /// Whether `c` separates parts when a path is read.
    pub fn is_sep(self, c: char) -> bool {
        match self {
            Style::Windows => c == '\\' || c == '/',
            Style::Posix => c == '/',
        }
    }

    /// The root of nothing in particular: `/` or `C:\`.
    pub fn default_root(self) -> &'static str {
        match self {
            Style::Windows => "C:\\",
            Style::Posix => "/",
        }
    }

    fn split_start(self, s: &str) -> Result<Start<'_>, PathError> {
        if s.contains('\0') {
            return Err(PathError::Nul);
        }
        match self {
            Style::Posix => Ok(if s.starts_with('/') { Start::Abs { root: "/".into(), rest: s } } else { Start::Relative(s) }),
            Style::Windows => {
                let b: Vec<char> = s.chars().take(4).collect();
                let sep = |i: usize| b.get(i).is_some_and(|&c| c == '\\' || c == '/');
                let at = |i: usize, want: char| b.get(i) == Some(&want);
                let dot_or_q = |i: usize| at(i, '?') || at(i, '.');
                if sep(0) && sep(1) && dot_or_q(2) && (b.len() == 3 || sep(3)) || sep(0) && at(1, '?') && at(2, '?') && sep(3) {
                    return Err(PathError::Verbatim);
                }
                if sep(0) && sep(1) {
                    // Both separators are one byte.
                    let body = &s[2..];
                    let (server, after) = body.split_once(['\\', '/']).unwrap_or((body, ""));
                    let (share, rest) = after.split_once(['\\', '/']).unwrap_or((after, ""));
                    if server.is_empty() || share.is_empty() {
                        return Err(PathError::NoShare);
                    }
                    // `rest` begins after the separator that ended the share, so a separator is put back for the fold.
                    return Ok(Start::Abs { root: format!("\\\\{server}\\{share}\\"), rest });
                }
                if sep(0) {
                    return Ok(Start::Rooted(&s[1..]));
                }
                if b.first().is_some_and(|c| c.is_ascii_alphabetic()) && at(1, ':') {
                    if b.len() == 2 || sep(2) {
                        let letter = b[0].to_ascii_uppercase();
                        return Ok(Start::Abs { root: format!("{letter}:\\"), rest: &s[2..] });
                    }
                    return Err(PathError::DriveRelative);
                }
                Ok(Start::Relative(s))
            }
        }
    }

    fn fold(self, parts: &mut Vec<String>, rest: &str) {
        for seg in rest.split(|c| self.is_sep(c)) {
            match seg {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                name => parts.push(name.to_owned()),
            }
        }
    }

    /// `path` as a root and parts, when it is absolute.
    fn abs_of(self, path: &str) -> Option<Abs> {
        match self.split_start(path).ok()? {
            Start::Abs { root, rest } => {
                let mut parts = Vec::new();
                self.fold(&mut parts, rest);
                Some(Abs { root, parts })
            }
            _ => None,
        }
    }

    fn render(self, abs: &Abs) -> String {
        let mut out = abs.root.clone();
        out.push_str(&abs.parts.join(&self.sep().to_string()));
        out
    }

    /// Whether `typed` names a place by itself (a drive, a UNC share, a leading separator), without needing a base.
    pub fn is_absolute(self, typed: &str) -> bool {
        matches!(self.split_start(typed), Ok(Start::Abs { .. } | Start::Rooted(_)) | Err(PathError::DriveRelative | PathError::Verbatim))
    }

    /// Whether `typed` has a separator in it.
    pub fn has_sep(self, typed: &str) -> bool {
        typed.chars().any(|c| self.is_sep(c))
    }

    /// What the person typed, read against `base` (an absolute folder): a complete path in this style's own form.
    ///
    /// `C:` alone is `C:\` (as Explorer's address bar does). `\x` takes the root of `base`. `C:foo`, `\\?\...`, `\\.\...`
    /// and `\??\...` are refused, and so is a UNC path with no share. Trailing dots and spaces in a part are kept as typed.
    pub fn resolve(self, typed: &str, base: &str) -> Result<String, PathError> {
        let mut abs = match self.split_start(typed)? {
            Start::Abs { root, rest } => {
                let mut parts = Vec::new();
                self.fold(&mut parts, rest);
                Abs { root, parts }
            }
            Start::Rooted(rest) => {
                let root = self.abs_of(base).map_or_else(|| self.default_root().to_owned(), |a| a.root);
                let mut parts = Vec::new();
                self.fold(&mut parts, rest);
                Abs { root, parts }
            }
            Start::Relative(rest) => {
                let mut abs = self.abs_of(base).unwrap_or_else(|| Abs { root: self.default_root().to_owned(), parts: Vec::new() });
                self.fold(&mut abs.parts, rest);
                abs
            }
        };
        abs.parts.retain(|p| !p.is_empty());
        Ok(self.render(&abs))
    }

    /// `dir` and one name written together. `name` is used as it is; `dir` is not normalised.
    pub fn join(self, dir: &str, name: &str) -> String {
        if dir.ends_with(|c| self.is_sep(c)) {
            format!("{dir}{name}")
        } else {
            format!("{dir}{}{name}", self.sep())
        }
    }

    /// The folder above `path`; `None` at a root (or when `path` is not absolute).
    pub fn parent(self, path: &str) -> Option<String> {
        let mut abs = self.abs_of(path)?;
        abs.parts.pop()?;
        Some(self.render(&abs))
    }

    /// The last part of `path`; `None` at a root.
    pub fn last_part(self, path: &str) -> Option<String> {
        self.abs_of(path)?.parts.pop()
    }

    /// The root of `path` (`/`, `C:\`, `\\server\share\`); `None` when it is not absolute.
    pub fn root_of(self, path: &str) -> Option<String> {
        self.abs_of(path).map(|a| a.root)
    }

    /// Whether `path` is a root.
    pub fn is_root(self, path: &str) -> bool {
        self.abs_of(path).is_some_and(|a| a.parts.is_empty())
    }

    /// The parts of `path` from the root, each with its label and the path up to and including it:
    /// `C:\Users\a` is `C:` `Users` `a`; a UNC path starts with `\\server\share`; `/var/log` is `/` `var` `log`.
    pub fn crumbs(self, path: &str) -> Vec<(String, String)> {
        let Some(abs) = self.abs_of(path) else { return Vec::new() };
        let label = match self {
            Style::Posix => "/".to_owned(),
            Style::Windows => abs.root.trim_end_matches('\\').to_owned(),
        };
        let mut out = vec![(label, abs.root.clone())];
        let mut at = Abs { root: abs.root.clone(), parts: Vec::new() };
        for part in &abs.parts {
            at.parts.push(part.clone());
            out.push((part.clone(), self.render(&at)));
        }
        out
    }

    /// Whether two names are the same name: without case on Windows.
    pub fn same_name(self, a: &str, b: &str) -> bool {
        match self {
            Style::Posix => a == b,
            Style::Windows => a == b || a.to_lowercase() == b.to_lowercase(),
        }
    }

    /// Whether two paths are the same place, read lexically (a convenience, not file identity).
    pub fn same(self, a: &str, b: &str) -> bool {
        match (self.abs_of(a), self.abs_of(b)) {
            (Some(x), Some(y)) => self.same_abs(&x, &y),
            _ => a == b,
        }
    }

    fn same_abs(self, x: &Abs, y: &Abs) -> bool {
        self.same_name(&x.root, &y.root)
            && x.parts.len() == y.parts.len()
            && x.parts.iter().zip(&y.parts).all(|(p, q)| self.same_name(p, q))
    }

    /// Whether `path` is `folder` or inside it, on component boundaries (`/var/lib2` is not under `/var/lib`).
    pub fn is_under(self, path: &str, folder: &str) -> bool {
        let (Some(p), Some(f)) = (self.abs_of(path), self.abs_of(folder)) else { return false };
        self.same_name(&p.root, &f.root)
            && f.parts.len() <= p.parts.len()
            && f.parts.iter().zip(&p.parts).all(|(a, b)| self.same_name(a, b))
    }

    /// How many parts `path` has below its root (for choosing the deepest place that holds a path).
    pub fn depth(self, path: &str) -> usize {
        self.abs_of(path).map_or(0, |a| a.parts.len())
    }
}

/// `name` for the screen: a control character shows as a visible stand-in (C0 as the U+2400 block, DEL as U+2421). What the
/// name is (the string that is returned) never changes.
pub fn display_name(name: &str) -> Cow<'_, str> {
    if !name.chars().any(char::is_control) {
        return Cow::Borrowed(name);
    }
    Cow::Owned(
        name.chars()
            .map(|c| match c {
                '\u{0}'..='\u{1f}' => char::from_u32(0x2400 + c as u32).unwrap_or(c),
                '\u{7f}' => '\u{2421}',
                c => c,
            })
            .collect(),
    )
}

/// Whether `name` is one path component in `style`: not empty, not `.` or `..`, no NUL, no separator.
pub fn is_component(style: Style, name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('\0') && !name.chars().any(|c| style.is_sep(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Style = Style::Windows;
    const P: Style = Style::Posix;

    #[test]
    fn posix_resolve_join_parent() {
        assert_eq!(P.resolve("../x/y.txt", "/var/lib"), Ok("/var/x/y.txt".into()));
        assert_eq!(P.resolve("/a//b/./c/", "/"), Ok("/a/b/c".into()));
        assert_eq!(P.resolve("..", "/"), Ok("/".into()), "never above the root");
        assert_eq!(P.resolve("../../../..", "/a/b"), Ok("/".into()));
        assert_eq!(P.resolve("", "/a"), Ok("/a".into()));
        assert_eq!(P.resolve("~", "/a"), Ok("/a/~".into()), "~ is not expanded");
        assert_eq!(P.resolve("a\\b", "/x"), Ok("/x/a\\b".into()), "a backslash is an ordinary character");
        assert_eq!(P.join("/", "a"), "/a");
        assert_eq!(P.join("/var", "a"), "/var/a");
        assert_eq!(P.parent("/var/log"), Some("/var".into()));
        assert_eq!(P.parent("/var"), Some("/".into()));
        assert_eq!(P.parent("/"), None);
        assert_eq!(P.parent("rel"), None);
        assert_eq!(P.resolve("a\0b", "/"), Err(PathError::Nul));
    }

    #[test]
    fn posix_dotdot_is_lexical_even_over_a_link_name() {
        // `link` may be a symlink to /etc/x: the shell's `cd ..` from `/tmp/link` still goes to /tmp.
        assert_eq!(P.resolve("..", "/tmp/link"), Ok("/tmp".into()));
    }

    #[test]
    fn windows_roots() {
        assert_eq!(W.resolve("c:/", "C:\\x"), Ok("C:\\".into()));
        assert_eq!(W.resolve("c:", "D:\\x"), Ok("C:\\".into()), "bare C: is the root");
        assert_eq!(W.resolve("d:\\a/b", "C:\\"), Ok("D:\\a\\b".into()));
        assert_eq!(W.resolve("\\x", "D:\\a\\b"), Ok("D:\\x".into()), "a leading single separator takes the base's root");
        assert_eq!(W.resolve("\\x", "\\\\srv\\sh\\a"), Ok("\\\\srv\\sh\\x".into()), "a UNC base too");
        assert_eq!(W.resolve("..\\..", "C:\\a"), Ok("C:\\".into()));
        assert_eq!(W.resolve("..", "C:\\"), Ok("C:\\".into()), "never above a drive root");
        assert_eq!(W.resolve("sub", "\\\\srv\\sh\\"), Ok("\\\\srv\\sh\\sub".into()));
        assert_eq!(W.resolve("..\\..\\..", "\\\\srv\\sh\\a"), Ok("\\\\srv\\sh\\".into()), "never above the share");
    }

    #[test]
    fn windows_refused_forms() {
        assert_eq!(W.resolve("\\\\server", "C:\\"), Err(PathError::NoShare));
        assert_eq!(W.resolve("\\\\server\\", "C:\\"), Err(PathError::NoShare));
        assert_eq!(W.resolve("\\\\", "C:\\"), Err(PathError::NoShare));
        assert_eq!(W.resolve("C:foo", "C:\\"), Err(PathError::DriveRelative));
        assert_eq!(W.resolve("\\\\?\\C:\\x", "C:\\"), Err(PathError::Verbatim));
        assert_eq!(W.resolve("//?/C:/x", "C:\\"), Err(PathError::Verbatim));
        assert_eq!(W.resolve("\\\\.\\pipe\\x", "C:\\"), Err(PathError::Verbatim));
        assert_eq!(W.resolve("\\??\\C:\\x", "C:\\"), Err(PathError::Verbatim));
        assert!(W.is_absolute("C:foo"), "refused, but not a plain name");
    }

    #[test]
    fn windows_keeps_trailing_dots_and_spaces() {
        assert_eq!(W.resolve("a. \\b..", "C:\\"), Ok("C:\\a. \\b..".into()));
        assert_eq!(W.resolve("...", "C:\\x"), Ok("C:\\x\\...".into()), "... is a name, not a parent");
    }

    #[test]
    fn crumbs() {
        let c = |s: Style, p: &str| s.crumbs(p).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert_eq!(c(W, "C:\\Users\\a"), ["C:", "Users", "a"]);
        assert_eq!(c(W, "\\\\srv\\share\\x"), ["\\\\srv\\share", "x"]);
        assert_eq!(c(P, "/var/log"), ["/", "var", "log"]);
        assert_eq!(c(P, "/"), ["/"]);
        assert_eq!(W.crumbs("C:\\Users\\a")[1].1, "C:\\Users");
        assert_eq!(W.crumbs("C:\\Users\\a")[0].1, "C:\\");
        assert!(P.crumbs("rel").is_empty());
    }

    #[test]
    fn same_and_under() {
        assert!(W.same("c:\\USERS\\", "C:/users"));
        assert!(!P.same("/Users", "/users"));
        assert!(P.same("/a/b/", "/a//b"));
        assert!(P.is_under("/var/lib/x", "/var/lib"));
        assert!(P.is_under("/var/lib", "/var/lib"));
        assert!(!P.is_under("/var/lib2", "/var/lib"));
        assert!(P.is_under("/anything", "/"));
        assert!(W.is_under("C:\\Users\\a", "c:\\users"));
        assert!(!W.is_under("D:\\Users", "C:\\Users"));
        assert!(W.is_under("\\\\srv\\sh\\a", "\\\\SRV\\sh\\"));
        assert!(W.same_name("README.md", "readme.MD"));
        assert!(!P.same_name("README.md", "readme.MD"));
    }

    #[test]
    fn display_stand_ins_keep_the_identity() {
        assert_eq!(display_name("a\tb\u{7f}\u{1b}"), "a\u{2409}b\u{2421}\u{241b}");
        assert!(matches!(display_name("plain"), Cow::Borrowed(_)));
    }

    #[test]
    fn components() {
        assert!(is_component(P, "a\\b"));
        assert!(!is_component(W, "a\\b"));
        assert!(!is_component(P, "a/b"));
        assert!(!is_component(W, "a/b"));
        for bad in ["", ".", "..", "a\0"] {
            assert!(!is_component(P, bad), "{bad:?}");
        }
    }
}
