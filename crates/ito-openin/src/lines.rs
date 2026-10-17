//! How an editor is told which line to open at. The built-in table covers the
//! common editors; the app's `[line_args]` (keyed by [`editor_key`]) wins over it.

use std::collections::HashMap;

/// Line-jump argument templates from `[line_args]`, keyed by [`editor_key`]. A
/// template is written as the arguments themselves, e.g. `"/j{line} {path}"` or
/// `"-g {path}:{line}"`: exactly one token holds `{path}`.
pub type LineArgs = HashMap<String, String>;

/// The key an editor is looked up by: its file name, lowercased, without the
/// executable extension. `C:\bin\Sakura.exe` and `sakura` both give `sakura`.
pub fn editor_key(program: &str) -> String {
    // A Windows path is split by hand: on Unix `Path` does not know `\`.
    let name = program.rsplit(['/', '\\']).next().unwrap_or("").to_ascii_lowercase();
    let stem = [".exe", ".cmd", ".bat"].iter().find_map(|x| name.strip_suffix(x)).unwrap_or(&name);
    stem.to_owned()
}

/// Whether a `[line_args]` template can be used: exactly one token holds `{path}`.
pub fn template_is_valid(template: &str) -> bool {
    split_template(template).is_some()
}

/// The tokens before the path token, and the path token.
fn split_template(template: &str) -> Option<(Vec<&str>, &str)> {
    let mut before = Vec::new();
    let mut path = None;
    for tok in template.split_whitespace() {
        match (tok.contains("{path}"), path.is_some()) {
            (true, true) => return None,
            (true, false) => path = Some(tok),
            (false, _) => before.push(tok),
        }
    }
    Some((before, path?))
}

/// How one editor takes a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    /// A flag before the path: `nvim +12 file`, `sakura -Y=12 file`.
    Flag(&'static str),
    /// `code -g file:12`
    Goto,
    /// `hx file:12`
    Colon,
    /// A `[line_args]` template.
    Template(String),
}

impl How {
    /// The arguments that come before the path, with the line filled in.
    pub fn before(&self, line: usize) -> String {
        match self {
            How::Flag(f) => format!("{f}{line}"),
            How::Goto => "-g".to_owned(),
            How::Colon => String::new(),
            How::Template(t) => split_template(t).map(|(b, _)| b.join(" ").replace("{line}", &line.to_string())).unwrap_or_default(),
        }
    }

    /// The token that carries the path.
    pub fn path_token(&self, line: usize, path: &str) -> String {
        match self {
            How::Flag(_) => path.to_owned(),
            How::Goto | How::Colon => format!("{path}:{line}"),
            How::Template(t) => {
                let tok = split_template(t).map_or("{path}", |(_, p)| p);
                tok.replace("{line}", &line.to_string()).replace("{path}", path)
            }
        }
    }
}

/// How the editor `program` (a path or a bare name) takes a line; `None` when
/// it is not known to.
pub fn line_arg(program: &str, custom: &LineArgs) -> Option<How> {
    let name = editor_key(program);
    if name.is_empty() {
        return None;
    }
    if let Some(t) = custom.get(&name).filter(|t| template_is_valid(t)) {
        return Some(How::Template(t.clone()));
    }
    builtin(&name)
}

fn builtin(name: &str) -> Option<How> {
    Some(match name {
        "nvim" | "vim" | "vi" | "gvim" | "nano" | "emacs" | "emacsclient" | "micro" | "kak" => How::Flag("+"),
        // Editors common on Windows. Notepad has no line switch, so it stays out.
        "hidemaru" => How::Flag("/j"),
        // `-Y=`, not `-L=`: sakura takes `-L=` without complaining and does
        // nothing with it (checked on sakura 2.4.2: `-L=6` lands on line 1,
        // `-Y=6` on line 6).
        "sakura" => How::Flag("-Y="),
        "emeditor" => How::Flag("/l "),
        "notepad++" => How::Flag("-n"),
        "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => How::Goto,
        "hx" | "helix" | "subl" | "zed" => How::Colon,
        _ => return None,
    })
}

/// The editors with a built-in line argument, for a settings screen to list.
pub const BUILTIN_EDITORS: [&str; 20] = [
    "nvim",
    "vim",
    "vi",
    "gvim",
    "nano",
    "emacs",
    "emacsclient",
    "micro",
    "kak",
    "hidemaru",
    "sakura",
    "emeditor",
    "notepad++",
    "code",
    "code-insiders",
    "codium",
    "cursor",
    "windsurf",
    "hx",
    "zed",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_the_lowercase_name_without_the_extension() {
        assert_eq!(editor_key("C:\\bin\\Sakura.exe"), "sakura");
        assert_eq!(editor_key("/usr/bin/nvim"), "nvim");
        assert_eq!(editor_key("code.cmd"), "code");
        assert_eq!(editor_key(""), "");
    }

    #[test]
    fn a_template_needs_exactly_one_path_token() {
        assert!(template_is_valid("/j{line} {path}"));
        assert!(template_is_valid("-g {path}:{line}"));
        assert!(!template_is_valid("+{line}"));
        assert!(!template_is_valid("{path} {path}"));
    }

    #[test]
    fn the_builtin_table_and_the_listed_names_agree() {
        for n in BUILTIN_EDITORS {
            assert!(builtin(n).is_some(), "{n}");
        }
        assert!(builtin("notepad").is_none());
    }

    #[test]
    fn a_bad_custom_template_falls_back_to_the_builtin() {
        let mut c = LineArgs::new();
        c.insert("nvim".into(), "+{line}".into());
        assert_eq!(line_arg("nvim", &c), Some(How::Flag("+")));
        c.insert("nvim".into(), "--line={line} {path}".into());
        assert_eq!(line_arg("nvim", &c).unwrap().before(3), "--line=3");
    }
}
