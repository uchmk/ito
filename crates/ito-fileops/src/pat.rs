//! File-name wildcards.

use std::ffi::OsStr;

use ito_match::glob_match;

/// How a name met the pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hit {
    Yes,
    No,
    /// The name is not valid Unicode and the pattern is not just `*`.
    NotUnicode,
}

/// A pattern of `None`, empty or `*` takes every name, even one that is not Unicode.
pub(crate) fn name_matches(pattern: Option<&str>, name: &OsStr) -> Hit {
    let Some(p) = pattern.filter(|p| !p.is_empty() && *p != "*") else { return Hit::Yes };
    let Some(n) = name.to_str() else { return Hit::NotUnicode };
    if matches(p, n, cfg!(windows)) {
        Hit::Yes
    } else {
        Hit::No
    }
}

/// `*` and `?` against a bare name. With `windows`: case-insensitive, and a pattern ending in `.*` also
/// takes a name without a dot that matches what is before it (`a.*` takes `a`), as `FindFirstFile` does.
pub(crate) fn matches(pattern: &str, name: &str, windows: bool) -> bool {
    if glob_match(pattern, name, windows) {
        return true;
    }
    windows && pattern.ends_with(".*") && !name.contains('.') && glob_match(&pattern[..pattern.len() - 2], name, true)
}
