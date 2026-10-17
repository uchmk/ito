//! Opening something in another program, from a command line the user can
//! change: a folder in a terminal or the file manager, a file in an editor
//! (at a line, when the editor can take one), two or three files in a diff
//! or merge tool. The line is a template of words (quotes group them) with
//! `{path}`, `{line}`, `{left}`, `{right}`, `{base}` and `{merged}` in it; no
//! shell is involved, and the program is started, not waited for.
//!
//! The pieces:
//! - [`argv`] / [`Vars`]: the template into a program and its arguments.
//! - [`lines`]: how each editor is told a line (the built-in table and the
//!   `[line_args]` of the app's config.toml).
//! - [`diff`]: the known diff / merge tools and finding which are installed
//!   (do it on a worker thread: [`diff::detect_in_background`]).
//! - [`config`]: the `[open]`, `[line_args]` and `[diff_tools.<name>]` tables
//!   of config.toml, with what to warn of, and the edits a settings screen
//!   makes to them (`ito-prefs` draws the card).
//! - [`open`], [`open_file`], [`open_diff`], [`open_merge`]: start the program.

pub mod config;
pub mod diff;
pub mod lines;

use std::path::Path;
use std::process::{Command, Stdio};

pub use config::{Config, Edit, OpenCfg, UserTool};
pub use lines::LineArgs;

/// Which program something is opened in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Terminal,
    Editor,
    Explorer,
    /// Two files side by side.
    Diff,
    /// Two versions of a file and their common base, into a merged file.
    Merge,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Terminal, Tool::Editor, Tool::Explorer, Tool::Diff, Tool::Merge];

    /// The key of `[open]` in config.toml.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Terminal => "terminal",
            Tool::Editor => "editor",
            Tool::Explorer => "explorer",
            Tool::Diff => "diff",
            Tool::Merge => "merge",
        }
    }
}

/// The command line used when config.toml says none. Empty for [`Tool::Merge`]:
/// there is no merge tool that can be assumed (see [`diff::detect`]).
pub fn default_command(tool: Tool) -> &'static str {
    match tool {
        Tool::Terminal => "tsumugi new {path}",
        Tool::Editor => os::EDITOR,
        Tool::Explorer => os::EXPLORER,
        Tool::Diff => "git difftool --no-index --no-prompt {left} {right}",
        Tool::Merge => "",
    }
}

/// What the placeholders of a template stand for. A value that is not set
/// makes a template that uses it an error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vars {
    pub path: Option<String>,
    pub line: Option<usize>,
    pub left: Option<String>,
    pub right: Option<String>,
    pub base: Option<String>,
    pub merged: Option<String>,
}

impl Vars {
    /// A folder or a file.
    pub fn path(p: &Path) -> Self {
        Vars { path: Some(p.to_string_lossy().into_owned()), ..Vars::default() }
    }

    /// The two sides of a diff.
    pub fn pair(left: &Path, right: &Path) -> Self {
        Vars { left: Some(left.to_string_lossy().into_owned()), right: Some(right.to_string_lossy().into_owned()), ..Vars::default() }
    }

    /// `None`: not a placeholder; `Some(None)`: a placeholder with no value.
    fn get(&self, name: &str) -> Option<Option<String>> {
        Some(match name {
            "path" => self.path.clone(),
            "line" => self.line.map(|n| n.to_string()),
            "left" => self.left.clone(),
            "right" => self.right.clone(),
            "base" => self.base.clone(),
            "merged" => self.merged.clone(),
            _ => return None,
        })
    }
}

/// The words of `template`; quotes group words, and a quote left open is an error.
pub fn words(template: &str) -> Result<Vec<String>, String> {
    let mut words: Vec<String> = Vec::new();
    let (mut cur, mut quoted, mut any) = (String::new(), false, false);
    for c in template.chars() {
        if c == '"' {
            quoted = !quoted;
            any = true;
        } else if c.is_whitespace() && !quoted {
            if any {
                words.push(std::mem::take(&mut cur));
                any = false;
            }
        } else {
            cur.push(c);
            any = true;
        }
    }
    if quoted {
        return Err(format!("unclosed quote: {template}"));
    }
    if any {
        words.push(cur);
    }
    if words.is_empty() {
        return Err("empty command".to_owned());
    }
    Ok(words)
}

/// `word` with each `{name}` it holds replaced from `vars` in one pass (a value
/// that holds a `{name}` is not expanded again). An unknown `{name}` stays.
fn fill(word: &str, vars: &Vars) -> Result<String, String> {
    let mut out = String::with_capacity(word.len());
    let mut rest = word;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').and_then(|close| vars.get(&after[..close]).map(|v| (close, v))) {
            Some((close, Some(v))) => {
                out.push_str(&v);
                rest = &after[close + 1..];
            }
            Some((close, None)) => return Err(format!("{{{}}} has no value here", &after[..close])),
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn mentions(words: &[String], name: &str) -> bool {
    let tag = format!("{{{name}}}");
    words.iter().any(|w| w.contains(&tag))
}

/// The program and arguments of `template` for `vars`. When the template has no
/// `{path}` the path is added at the end, and when it has neither `{left}` nor
/// `{right}` the two sides are (so a bare program name is a whole command).
pub fn argv(template: &str, vars: &Vars) -> Result<Vec<String>, String> {
    let words = words(template)?;
    let mut out = words.iter().map(|w| fill(w, vars)).collect::<Result<Vec<_>, _>>()?;
    if let (false, Some(p)) = (mentions(&words, "path"), &vars.path) {
        out.push(p.clone());
    }
    if let (false, false, Some(l), Some(r)) = (mentions(&words, "left"), mentions(&words, "right"), &vars.left, &vars.right) {
        out.push(l.clone());
        out.push(r.clone());
    }
    Ok(out)
}

/// An editor's command line for a file, and whether a line asked for was
/// passed (false when the editor takes none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    pub argv: Vec<String>,
    pub line_passed: bool,
}

/// [`Built`] for opening the file `path` with `template`, at `line` when one is
/// asked for and the editor takes it. A template that spells `{line}` itself is
/// taken as it is.
pub fn file_argv(template: &str, path: &Path, line: Option<usize>, custom: &LineArgs) -> Result<Built, String> {
    let ws = words(template)?;
    let vars = Vars { line, ..Vars::path(path) };
    let Some(n) = line else { return Ok(Built { argv: argv(template, &vars)?, line_passed: true }) };
    if mentions(&ws, "line") {
        return Ok(Built { argv: argv(template, &vars)?, line_passed: true });
    }
    let Some(how) = lines::line_arg(&ws[0], custom) else {
        return Ok(Built { argv: argv(template, &Vars::path(path))?, line_passed: false });
    };
    let token = how.path_token(n, &path.to_string_lossy());
    let mut out = vec![ws[0].clone()];
    out.extend(how.before(n).split_whitespace().map(str::to_owned));
    let mut put = false;
    for w in &ws[1..] {
        if w.contains("{path}") {
            put = true;
        }
        out.push(w.replace("{path}", &token));
    }
    if !put {
        out.push(token);
    }
    Ok(Built { argv: out, line_passed: true })
}

/// Starts `argv` (in `cwd`); does not wait for it, and no window flashes.
pub fn spawn(argv: &[String], cwd: Option<&Path>) -> Result<(), String> {
    let Some(program) = argv.first() else { return Err("empty command".to_owned()) };
    let mut c = Command::new(program);
    c.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(dir) = cwd {
        c.current_dir(dir);
    }
    hide_window(&mut c);
    let mut child = c.spawn().map_err(|e| format!("{program}: {e}"))?;
    // Reaped on a thread of its own, so nothing is left defunct.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(windows)]
fn hide_window(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_window(_: &mut Command) {}

/// Starts `template` for `vars`; `cwd` is where it runs (a folder, mostly).
pub fn open(template: &str, vars: &Vars, cwd: Option<&Path>) -> Result<(), String> {
    spawn(&argv(template, vars)?, cwd)
}

/// Opens the file `path` with `template`, at `line` when the editor takes it;
/// whether the line was passed.
pub fn open_file(template: &str, path: &Path, line: Option<usize>, custom: &LineArgs) -> Result<bool, String> {
    let b = file_argv(template, path, line, custom)?;
    spawn(&b.argv, path.parent())?;
    Ok(b.line_passed)
}

/// Opens `left` and `right` side by side with `template`.
pub fn open_diff(template: &str, left: &Path, right: &Path) -> Result<(), String> {
    open(template, &Vars::pair(left, right), None)
}

/// Opens a three-way merge of `left` and `right` over `base`, into `merged`.
pub fn open_merge(template: &str, left: &Path, right: &Path, base: &Path, merged: &Path) -> Result<(), String> {
    let s = |p: &Path| Some(p.to_string_lossy().into_owned());
    let v = Vars { left: s(left), right: s(right), base: s(base), merged: s(merged), ..Vars::default() };
    open(template, &v, None)
}

#[cfg(windows)]
mod os {
    pub const EDITOR: &str = "code.cmd {path}";
    pub const EXPLORER: &str = "explorer {path}";
}

#[cfg(target_os = "macos")]
mod os {
    pub const EDITOR: &str = "code {path}";
    pub const EXPLORER: &str = "open {path}";
}

#[cfg(not(any(windows, target_os = "macos")))]
mod os {
    pub const EDITOR: &str = "code {path}";
    pub const EXPLORER: &str = "xdg-open {path}";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vars {
        Vars::path(Path::new(s))
    }

    #[test]
    fn the_path_is_put_where_path_is_or_added() {
        assert_eq!(argv("code {path}", &p("/w/a b")).unwrap(), ["code", "/w/a b"]);
        assert_eq!(argv("code", &p("/w/a b")).unwrap(), ["code", "/w/a b"]);
        assert_eq!(argv("term --cwd={path} -x", &p("/w/a b")).unwrap(), ["term", "--cwd=/w/a b", "-x"]);
    }

    #[test]
    fn quotes_group_words_and_bad_lines_are_refused() {
        assert_eq!(argv("\"C:\\Program Files\\x.exe\" --new {path}", &p("/w")).unwrap(), ["C:\\Program Files\\x.exe", "--new", "/w"]);
        assert!(argv("  ", &p("/w")).is_err());
        assert!(argv("a \"b", &p("/w")).is_err());
    }

    #[test]
    fn both_sides_are_added_when_the_template_names_neither() {
        let v = Vars::pair(Path::new("a"), Path::new("b"));
        assert_eq!(argv("meld", &v).unwrap(), ["meld", "a", "b"]);
        assert_eq!(argv("tool {right} {left}", &v).unwrap(), ["tool", "b", "a"]);
        assert_eq!(argv("tool --left={left} {right}", &v).unwrap(), ["tool", "--left=a", "b"]);
    }

    #[test]
    fn a_placeholder_without_a_value_is_an_error_and_a_value_is_not_expanded_again() {
        assert!(argv("x {line}", &p("/w")).is_err());
        assert!(argv("x {merged}", &p("/w")).is_err());
        assert_eq!(argv("x {nope} {path}", &p("/w/{line}")).unwrap(), ["x", "{nope}", "/w/{line}"]);
    }

    #[test]
    fn every_default_but_merge_names_a_program() {
        for t in Tool::ALL {
            let d = default_command(t);
            if t == Tool::Merge {
                assert!(d.is_empty());
            } else {
                let v = Vars { path: Some("/w".into()), left: Some("a".into()), right: Some("b".into()), ..Vars::default() };
                assert!(argv(d, &v).is_ok(), "{t:?}");
            }
        }
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_panic() {
        assert!(open("ito-openin-no-such-program-xyz {path}", &Vars::path(&std::env::temp_dir()), None).is_err());
        assert!(spawn(&[], None).is_err());
    }

    #[test]
    fn a_line_goes_where_the_editor_wants_it() {
        let none = LineArgs::new();
        let b = file_argv("nvim {path}", Path::new("/a/b.rs"), Some(12), &none).unwrap();
        assert_eq!(b, Built { argv: ["nvim", "+12", "/a/b.rs"].map(String::from).to_vec(), line_passed: true });
        let b = file_argv("code.cmd", Path::new("/a/b.rs"), Some(12), &none).unwrap();
        assert_eq!(b.argv, ["code.cmd", "-g", "/a/b.rs:12"]);
        let b = file_argv("\"C:\\Program Files\\EmEditor\\EmEditor.exe\" {path}", Path::new("f"), Some(3), &none).unwrap();
        assert_eq!(b.argv, ["C:\\Program Files\\EmEditor\\EmEditor.exe", "/l", "3", "f"]);
        let b = file_argv("sakura", Path::new("f"), Some(6), &none).unwrap();
        assert_eq!(b.argv, ["sakura", "-Y=6", "f"]);
    }

    #[test]
    fn an_editor_that_takes_no_line_gets_the_path_and_says_so() {
        let none = LineArgs::new();
        let b = file_argv("notepad {path}", Path::new("f"), Some(12), &none).unwrap();
        assert_eq!(b, Built { argv: ["notepad", "f"].map(String::from).to_vec(), line_passed: false });
        assert!(file_argv("notepad", Path::new("f"), None, &none).unwrap().line_passed);
    }

    #[test]
    fn a_template_that_spells_line_is_taken_as_it_is() {
        let none = LineArgs::new();
        let b = file_argv("myed --at {line} {path}", Path::new("f"), Some(7), &none).unwrap();
        assert_eq!(b.argv, ["myed", "--at", "7", "f"]);
    }

    #[test]
    fn the_apps_own_line_arguments_win() {
        let mut custom = LineArgs::new();
        custom.insert("myed".into(), "--goto={line} {path}".into());
        let b = file_argv("myed", Path::new("f"), Some(7), &custom).unwrap();
        assert_eq!(b.argv, ["myed", "--goto=7", "f"]);
    }
}
