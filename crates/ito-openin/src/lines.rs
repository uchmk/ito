//! How an editor is told which line to open at. The built-in table covers the
//! common editors; the app's `[line_args]` (keyed by [`editor_key`]) wins over it. A column goes the same way:
//! `{col}` (characters) and `{bcol}` (bytes) in a template, or the built-in form of an editor that takes one.

use std::collections::HashMap;

/// Line-jump argument templates from `[line_args]`, keyed by [`editor_key`]. A
/// template is written as the arguments themselves, e.g. `"/j{line} {path}"` or
/// `"-g {path}:{line}"`: exactly one token holds `{path}`. `{col}` and `{bcol}` are the column from 1,
/// in characters and in bytes.
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

/// A column, from 1, counted both ways: editors differ (VS Code, nano and most GUI editors count characters; Vim's
/// `cursor()` and Kakoune count bytes), so the app says where the caret is once and ito picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Col {
    /// Unicode scalar values, from 1.
    pub chars: usize,
    /// UTF-8 bytes, from 1.
    pub bytes: usize,
}

impl Col {
    /// Both counts given (an editor buffer already has them, e.g. `ito_textbuf::pos`).
    pub fn new(chars: usize, bytes: usize) -> Col {
        Col { chars: chars.max(1), bytes: bytes.max(1) }
    }

    /// The column that is the `chars`-th character (from 1) of `line_text` (the line without its break); the byte
    /// count is worked out from the text. Past the end of the line it is the end of the line.
    pub fn in_line(line_text: &str, chars: usize) -> Col {
        let chars = chars.max(1);
        let bytes = line_text.chars().take(chars - 1).map(char::len_utf8).sum::<usize>() + 1;
        Col { chars, bytes }
    }
}

/// Where in a file to open it: a line from 1, and a column when the app knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub line: usize,
    pub col: Option<Col>,
}

impl Pos {
    pub fn line(line: usize) -> Pos {
        Pos { line, col: None }
    }
    pub fn new(line: usize, col: Col) -> Pos {
        Pos { line, col: Some(col) }
    }
}

/// How one editor takes a line (and a column, when it can).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    /// A flag before the path, a line only: `vi +12 file`.
    Flag(&'static str),
    /// `code -g file:12:5`
    Goto,
    /// `hx file:12:5`
    Colon,
    /// A `[line_args]` template. It takes a column when it spells `{col}` or `{bcol}`.
    Template(String),
    /// Vim: `nvim "+call cursor(12,5)" file` (a byte column); `+12` without one.
    Cursor,
    /// One flag with both numbers: `nano +12,5 file`, `emacs +12:5 file`. `bytes`: the column counts bytes.
    Joined { prefix: &'static str, sep: &'static str, bytes: bool },
    /// A flag each: `notepad++ -n12 -c5 file`, `sakura -Y=12 -X=5 file`.
    Pair { line: &'static str, col: &'static str },
}

fn split_words(s: String) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

fn fill_pos(t: &str, line: usize, col: Option<Col>) -> String {
    // A template that spells a column the caller did not give lands at the start of the line.
    let c = col.unwrap_or(Col { chars: 1, bytes: 1 });
    t.replace("{line}", &line.to_string()).replace("{bcol}", &c.bytes.to_string()).replace("{col}", &c.chars.to_string())
}

impl How {
    /// The arguments that come before the path, with the line (and column) filled in.
    pub fn args(&self, line: usize, col: Option<Col>) -> Vec<String> {
        match self {
            How::Flag(f) => split_words(format!("{f}{line}")),
            How::Goto => vec!["-g".to_owned()],
            How::Colon => Vec::new(),
            How::Template(t) => split_template(t).map(|(b, _)| b.iter().map(|w| fill_pos(w, line, col)).collect()).unwrap_or_default(),
            How::Cursor => vec![match col {
                Some(c) => format!("+call cursor({line},{})", c.bytes),
                None => format!("+{line}"),
            }],
            How::Joined { prefix, sep, bytes } => split_words(match col {
                Some(c) => format!("{prefix}{line}{sep}{}", if *bytes { c.bytes } else { c.chars }),
                None => format!("{prefix}{line}"),
            }),
            How::Pair { line: lf, col: cf } => {
                let mut out = split_words(format!("{lf}{line}"));
                if let Some(c) = col {
                    out.extend(split_words(format!("{cf}{}", c.chars)));
                }
                out
            }
        }
    }

    /// The arguments that come before the path, for a line only, as one string.
    pub fn before(&self, line: usize) -> String {
        self.args(line, None).join(" ")
    }

    /// The token that carries the path.
    pub fn token(&self, line: usize, col: Option<Col>, path: &str) -> String {
        match self {
            How::Goto | How::Colon => match col {
                Some(c) => format!("{path}:{line}:{}", c.chars),
                None => format!("{path}:{line}"),
            },
            How::Template(t) => {
                let tok = split_template(t).map_or("{path}", |(_, p)| p);
                fill_pos(tok, line, col).replace("{path}", path)
            }
            _ => path.to_owned(),
        }
    }

    /// The token that carries the path, for a line only.
    pub fn path_token(&self, line: usize, path: &str) -> String {
        self.token(line, None, path)
    }

    /// Whether this editor can be told a column. When it cannot, the line is passed and the column is dropped.
    pub fn takes_col(&self) -> bool {
        match self {
            How::Flag(_) => false,
            How::Template(t) => t.contains("{col}") || t.contains("{bcol}"),
            _ => true,
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
    let joined = |prefix, sep, bytes| How::Joined { prefix, sep, bytes };
    Some(match name {
        // A plain `vi` may be a classic one that does not know `:call`.
        "vi" => How::Flag("+"),
        "nvim" | "vim" | "gvim" => How::Cursor,
        "nano" => joined("+", ",", false),
        "emacs" | "emacsclient" | "micro" => joined("+", ":", false),
        "kak" => joined("+", ":", true),
        // Editors common on Windows. Notepad has no line switch, so it stays out.
        "hidemaru" => joined("/j", ",", false),
        // `-Y=`, not `-L=`: sakura takes `-L=` without complaining and does
        // nothing with it (checked on sakura 2.4.2: `-L=6` lands on line 1,
        // `-Y=6` on line 6).
        "sakura" => How::Pair { line: "-Y=", col: "-X=" },
        "emeditor" => How::Pair { line: "/l ", col: "/cl " },
        "notepad++" => How::Pair { line: "-n", col: "-c" },
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
        assert_eq!(line_arg("nvim", &c), Some(How::Cursor));
        c.insert("nvim".into(), "--line={line} {path}".into());
        assert_eq!(line_arg("nvim", &c).unwrap().before(3), "--line=3");
    }

    fn col(text: &str, chars: usize) -> Option<Col> {
        Some(Col::in_line(text, chars))
    }

    #[test]
    fn a_column_is_counted_in_chars_and_in_bytes() {
        assert_eq!(Col::in_line("abc", 2), Col { chars: 2, bytes: 2 });
        // "あい" is 3 bytes each: the 3rd character starts at byte 7.
        assert_eq!(Col::in_line("あいう", 3), Col { chars: 3, bytes: 7 });
        assert_eq!(Col::in_line("あ", 99), Col { chars: 99, bytes: 4 }, "past the end");
        assert_eq!(Col::in_line("x", 0), Col { chars: 1, bytes: 1 });
        assert_eq!(Col::new(0, 0), Col { chars: 1, bytes: 1 });
    }

    #[test]
    fn each_builtin_editor_gets_its_own_form_of_line_and_column() {
        let c = col("あいう", 3); // chars 3, bytes 7
        let args = |n: &str| builtin(n).unwrap().args(12, c);
        assert_eq!(args("nvim"), ["+call cursor(12,7)"], "vim counts bytes, and the call stays one argument");
        assert_eq!(args("gvim"), ["+call cursor(12,7)"]);
        assert_eq!(args("vi"), ["+12"], "a classic vi is not told a column");
        assert_eq!(args("nano"), ["+12,3"]);
        assert_eq!(args("emacsclient"), ["+12:3"]);
        assert_eq!(args("micro"), ["+12:3"]);
        assert_eq!(args("kak"), ["+12:7"], "kakoune counts bytes");
        assert_eq!(args("hidemaru"), ["/j12,3"]);
        assert_eq!(args("sakura"), ["-Y=12", "-X=3"]);
        assert_eq!(args("emeditor"), ["/l", "12", "/cl", "3"]);
        assert_eq!(args("notepad++"), ["-n12", "-c3"]);
        assert_eq!(args("code"), ["-g"]);
        let tok = |n: &str| builtin(n).unwrap().token(12, c, "f");
        assert_eq!(tok("code"), "f:12:3");
        assert_eq!(tok("hx"), "f:12:3");
        assert_eq!(tok("zed"), "f:12:3");
        assert_eq!(tok("subl"), "f:12:3");
        assert_eq!(tok("nvim"), "f");
    }

    #[test]
    fn without_a_column_every_editor_gets_the_line_form_it_had() {
        for (n, want) in
            [("nvim", "+12"), ("nano", "+12"), ("kak", "+12"), ("hidemaru", "/j12"), ("sakura", "-Y=12"), ("notepad++", "-n12")]
        {
            assert_eq!(builtin(n).unwrap().args(12, None), [want], "{n}");
        }
        assert_eq!(builtin("emeditor").unwrap().before(12), "/l 12");
        assert_eq!(builtin("code").unwrap().token(12, None, "f"), "f:12");
    }

    #[test]
    fn only_some_editors_take_a_column() {
        assert!(builtin("code").unwrap().takes_col() && builtin("nvim").unwrap().takes_col());
        assert!(!builtin("vi").unwrap().takes_col());
        assert!(!How::Template("-l {line} {path}".into()).takes_col());
        assert!(How::Template("-l {line}:{col} {path}".into()).takes_col());
        assert!(How::Template("--byte {bcol} {path}".into()).takes_col());
    }

    #[test]
    fn a_template_fills_col_and_bcol_and_falls_back_to_the_line_start() {
        let h = How::Template("--at {line}:{col}/{bcol} {path}".into());
        assert_eq!(h.args(4, col("あい", 2)), ["--at", "4:2/4"]);
        assert_eq!(h.args(4, None), ["--at", "4:1/1"]);
        let h = How::Template("-g {path}:{line}:{col}".into());
        assert_eq!(h.token(4, col("ab", 2), "f"), "f:4:2");
    }
}
