//! What config.toml says about opening things, and the edits a settings screen
//! makes to it.
//!
//! ```toml
//! [open]
//! terminal  = "tsumugi new {path}"
//! editor    = "code.cmd {path}"
//! explorer  = "explorer {path}"
//! diff      = ""            # a full command line, or leave it to diff_tool
//! merge     = ""
//! diff_tool = "meld"        # a name from `diff::KNOWN`, from [diff_tools], or "auto"
//! system    = ["mp4", "pdf"] # opened the system's own way, not in the editor
//!                           # (`OpenCfg::opens_with_system`; not there: SYSTEM_EXTENSIONS, []: none)
//!
//! [line_args]               # how an editor is told a line (wins over the built-in table)
//! myed = "--goto={line} {path}"
//!
//! [diff_tools.mytool]       # a tool of one's own
//! diff  = "mytool {left} {right}"
//! merge = "mytool {base} {left} {right} -o {merged}"
//! ```
//!
//! A key that is not there, or is empty, keeps the default; keys this crate
//! does not know (an app's own, like kura's `[open].rules`) are left alone.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::Deserialize;

use crate::diff::{self, Found};
use crate::lines::{self, LineArgs};
use crate::{argv, default_command, Tool, Vars};

/// `[open]`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct OpenCfg {
    pub terminal: String,
    pub editor: String,
    pub explorer: String,
    pub diff: String,
    pub merge: String,
    pub diff_tool: String,
    /// Extensions (no dot, any case) opened the system's own way instead of in
    /// the editor. Not there: [`SYSTEM_EXTENSIONS`]; `[]`: none.
    pub system: Option<Vec<String>>,
}

/// What `[open] system` holds when config.toml does not say: videos, music,
/// pictures, documents, archives and installers.
pub const SYSTEM_EXTENSIONS: [&str; 40] = [
    "mp4", "mkv", "mov", "avi", "webm", "wmv", "flv", "m4v", "mpg", "mpeg", "mp3", "wav", "flac", "m4a", "ogg", "aac", "opus", "png",
    "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "ico", "heic", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "zip", "7z", "rar",
    "iso", "exe", "msi", "psd",
];

impl OpenCfg {
    /// Whether `path` is of a kind `[open] system` names, by its extension
    /// (any case, a leading dot in the list is ignored).
    pub fn opens_with_system(&self, path: &Path) -> bool {
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return false };
        let is = |s: &str| s.trim_start_matches('.').eq_ignore_ascii_case(ext);
        match &self.system {
            Some(list) => list.iter().any(|s| is(s)),
            None => SYSTEM_EXTENSIONS.iter().any(|s| is(s)),
        }
    }
}

/// `[diff_tools.<name>]`: a diff / merge tool the user added.
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct UserTool {
    pub diff: String,
    pub merge: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct File {
    open: OpenCfg,
    line_args: HashMap<String, String>,
    diff_tools: BTreeMap<String, UserTool>,
}

/// The `diff_tool` value that picks the first tool found installed.
pub const AUTO: &str = "auto";

/// A change a settings screen makes. [`Config::check`] says whether it is
/// acceptable, [`Config::apply`] makes it in memory, and [`Edit::writes`] says
/// what to put in the file (the app does that with `ito_common::set_key` /
/// `remove_key`, which keep the comments around).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// `[open].<key>` of a tool; empty goes back to the default.
    Command(Tool, String),
    /// `[open].diff_tool`; empty goes back to the default.
    DiffTool(String),
    /// Add or change `[diff_tools.<name>]`.
    UserTool(String, UserTool),
    /// Remove `[diff_tools.<name>]`.
    RemoveUserTool(String),
    /// Add or change `[line_args].<editor>`.
    LineArg(String, String),
    /// Remove `[line_args].<editor>`.
    RemoveLineArg(String),
}

/// One thing to do to the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Write {
    /// `ito_common::set_key(text, Some(table), key, value)`; `value` is TOML.
    Set { table: String, key: String, value: String },
    /// `ito_common::remove_key(text, Some(table), key)`.
    Remove { table: String, key: String },
}

fn quoted(s: &str) -> String {
    toml::Value::String(s.to_owned()).to_string()
}

fn set(table: &str, key: &str, value: &str) -> Write {
    Write::Set { table: table.to_owned(), key: key.to_owned(), value: quoted(value) }
}

fn remove(table: &str, key: &str) -> Write {
    Write::Remove { table: table.to_owned(), key: key.to_owned() }
}

impl Edit {
    /// What to do to the file. An empty value removes the key, so the default
    /// comes back.
    pub fn writes(&self) -> Vec<Write> {
        let put = |table: &str, key: &str, v: &str| if v.trim().is_empty() { remove(table, key) } else { set(table, key, v.trim()) };
        match self {
            Edit::Command(t, v) => vec![put("open", t.key(), v)],
            Edit::DiffTool(v) => vec![put("open", "diff_tool", v)],
            Edit::UserTool(name, t) => {
                let table = format!("diff_tools.{name}");
                vec![put(&table, "diff", &t.diff), put(&table, "merge", &t.merge)]
            }
            Edit::RemoveUserTool(name) => {
                let table = format!("diff_tools.{name}");
                vec![remove(&table, "diff"), remove(&table, "merge")]
            }
            Edit::LineArg(editor, t) => vec![set("line_args", &lines::editor_key(editor), t.trim())],
            Edit::RemoveLineArg(editor) => vec![remove("line_args", &lines::editor_key(editor))],
        }
    }
}

/// A name for `[diff_tools.<name>]`: letters, digits, `-` and `_`, and none of
/// the built-in names (those would hide the tool the user means).
pub fn tool_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && diff::known(name).is_none()
        && name != AUTO
}

/// Every placeholder set, so a template is checked for its shape alone.
fn probe() -> Vars {
    let s = |x: &str| Some(x.to_owned());
    Vars { path: s("x"), line: Some(1), left: s("l"), right: s("r"), base: s("b"), merged: s("m") }
}

fn command_ok(line: &str) -> Result<(), String> {
    argv(line, &probe()).map(|_| ())
}

/// Everything config.toml says about opening things.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub open: OpenCfg,
    /// `[line_args]`, keyed by `lines::editor_key`.
    pub line_args: LineArgs,
    pub diff_tools: BTreeMap<String, UserTool>,
}

impl Config {
    /// The tables of the config.toml `text` (none: no file), with what to warn
    /// of. A line that is not a command line is warned of and set back.
    pub fn read(text: Option<&str>) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let f = match text.map(toml::from_str::<File>) {
            Some(Ok(f)) => f,
            Some(Err(e)) => {
                warnings.push(format!("config.toml: {}", e.message()));
                File::default()
            }
            None => File::default(),
        };
        let mut c = Config { open: f.open, ..Config::default() };
        for t in Tool::ALL {
            let line = c.slot(t).trim().to_owned();
            if !line.is_empty() && command_ok(&line).is_err() {
                warnings.push(format!("config.toml: [open] `{}` is not a command line: {line}", t.key()));
                c.slot_mut(t).clear();
            }
        }
        for (editor, template) in f.line_args {
            if lines::template_is_valid(&template) {
                c.line_args.insert(editor.to_ascii_lowercase(), template);
            } else {
                warnings.push(format!("config.toml: [line_args] `{editor}` needs exactly one {{path}}: {template}"));
            }
        }
        for (name, mut t) in f.diff_tools {
            if !tool_name_ok(&name) {
                warnings.push(format!("config.toml: [diff_tools.{name}] is not a usable name"));
                continue;
            }
            for (key, line) in [("diff", &mut t.diff), ("merge", &mut t.merge)] {
                if !line.trim().is_empty() && command_ok(line).is_err() {
                    warnings.push(format!("config.toml: [diff_tools.{name}] `{key}` is not a command line: {line}"));
                    line.clear();
                }
            }
            // A table whose keys were all taken out (`Edit::RemoveUserTool`) is a tool removed.
            if !(t.diff.trim().is_empty() && t.merge.trim().is_empty()) {
                c.diff_tools.insert(name, t);
            }
        }
        (c, warnings)
    }

    fn slot(&self, t: Tool) -> &String {
        match t {
            Tool::Terminal => &self.open.terminal,
            Tool::Editor => &self.open.editor,
            Tool::Explorer => &self.open.explorer,
            Tool::Diff => &self.open.diff,
            Tool::Merge => &self.open.merge,
        }
    }

    fn slot_mut(&mut self, t: Tool) -> &mut String {
        match t {
            Tool::Terminal => &mut self.open.terminal,
            Tool::Editor => &mut self.open.editor,
            Tool::Explorer => &mut self.open.explorer,
            Tool::Diff => &mut self.open.diff,
            Tool::Merge => &mut self.open.merge,
        }
    }

    /// The command line for `t`: the one written, else (for diff and merge) the
    /// chosen `diff_tool`, else the default. `found` is [`diff::detect`]'s answer
    /// (empty until it has come). Empty for a merge nothing is set up for.
    pub fn command(&self, t: Tool, found: &[Found]) -> String {
        let written = self.slot(t).trim();
        if !written.is_empty() {
            return written.to_owned();
        }
        match t {
            Tool::Diff | Tool::Merge => self.tool_command(t == Tool::Merge, found).unwrap_or_else(|| default_command(t).to_owned()),
            _ => default_command(t).to_owned(),
        }
    }

    /// The command line of the chosen `diff_tool` for a diff (`merge` false) or a merge.
    fn tool_command(&self, merge: bool, found: &[Found]) -> Option<String> {
        let name = self.open.diff_tool.trim();
        let name = name.to_ascii_lowercase();
        let line = |f: &Found| if merge { f.merge_command() } else { Some(f.diff_command()) };
        if name.is_empty() {
            return None;
        }
        if name == AUTO {
            // git is always there to fall back on, so it is not an answer to "auto".
            return found.iter().filter(|f| f.tool.name != "git").find_map(line);
        }
        if let Some(u) = self.diff_tools.get(&name) {
            let l = if merge { &u.merge } else { &u.diff };
            return (!l.trim().is_empty()).then(|| l.trim().to_owned());
        }
        found.iter().find(|f| f.tool.name == name).and_then(line)
    }

    /// Whether `t` has a command to run (a merge may have none).
    pub fn can_open(&self, t: Tool, found: &[Found]) -> bool {
        !self.command(t, found).is_empty()
    }

    /// Opens the file `path` in the editor, at `line` when it takes one; whether the line was passed.
    pub fn open_file(&self, path: &Path, line: Option<usize>) -> Result<bool, String> {
        crate::open_file(&self.command(Tool::Editor, &[]), path, line, &self.line_args)
    }

    /// Whether `edit` can be made: its command lines read, its names are usable.
    pub fn check(&self, edit: &Edit) -> Result<(), String> {
        match edit {
            Edit::Command(_, v) | Edit::DiffTool(v) if v.trim().is_empty() => Ok(()),
            Edit::Command(_, v) => command_ok(v),
            Edit::DiffTool(_) => Ok(()),
            Edit::UserTool(name, t) => {
                if !tool_name_ok(name) {
                    return Err(format!("`{name}` is not a usable name (letters, digits, - and _, not a built-in tool)"));
                }
                if t.diff.trim().is_empty() && t.merge.trim().is_empty() {
                    return Err("a diff or a merge command is needed".to_owned());
                }
                [&t.diff, &t.merge].into_iter().filter(|l| !l.trim().is_empty()).try_for_each(|l| command_ok(l))
            }
            Edit::LineArg(editor, t) => {
                if lines::editor_key(editor).is_empty() {
                    return Err("an editor's program name is needed".to_owned());
                }
                if lines::template_is_valid(t) {
                    Ok(())
                } else {
                    Err("the line argument needs exactly one {path}, e.g. `-g {path}:{line}`".to_owned())
                }
            }
            Edit::RemoveUserTool(_) | Edit::RemoveLineArg(_) => Ok(()),
        }
    }

    /// Makes `edit` in memory (after [`Config::check`]; an edit that fails it is not made).
    pub fn apply(&mut self, edit: &Edit) -> Result<(), String> {
        self.check(edit)?;
        match edit {
            Edit::Command(t, v) => *self.slot_mut(*t) = v.trim().to_owned(),
            Edit::DiffTool(v) => self.open.diff_tool = v.trim().to_owned(),
            Edit::UserTool(name, t) => {
                self.diff_tools.insert(name.clone(), UserTool { diff: t.diff.trim().to_owned(), merge: t.merge.trim().to_owned() });
            }
            Edit::RemoveUserTool(name) => {
                self.diff_tools.remove(name);
            }
            Edit::LineArg(editor, t) => {
                self.line_args.insert(lines::editor_key(editor), t.trim().to_owned());
            }
            Edit::RemoveLineArg(editor) => {
                self.line_args.remove(&lines::editor_key(editor));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str) -> Found {
        Found { tool: diff::known(name).unwrap(), program: name.to_owned() }
    }

    #[test]
    fn system_extensions_default_and_override() {
        let (c, _) = Config::read(None);
        assert!(c.open.opens_with_system(Path::new("/v/Clip.MP4")), "any case");
        assert!(!c.open.opens_with_system(Path::new("/src/main.rs")));
        assert!(!c.open.opens_with_system(Path::new("/src/Makefile")));
        let (c, w) = Config::read(Some(
            "[open]
system = [\".rs\"]
",
        ));
        assert!(w.is_empty());
        assert!(c.open.opens_with_system(Path::new("a.rs")) && !c.open.opens_with_system(Path::new("a.mp4")));
        let (c, _) = Config::read(Some(
            "[open]
system = []
",
        ));
        assert!(!c.open.opens_with_system(Path::new("a.mp4")));
    }

    #[test]
    fn what_is_not_written_keeps_the_default() {
        let (c, w) = Config::read(None);
        assert!(w.is_empty());
        assert_eq!(c.command(Tool::Terminal, &[]), "tsumugi new {path}");
        assert_eq!(c.command(Tool::Merge, &[]), "");
        assert!(!c.can_open(Tool::Merge, &[]));
        let (c, w) = Config::read(Some("[open]\neditor = \"zed {path}\"\nexplorer = \"\"\nrules = [1, 2]\n"));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.command(Tool::Editor, &[]), "zed {path}");
        assert_eq!(c.command(Tool::Explorer, &[]), default_command(Tool::Explorer));
    }

    #[test]
    fn a_bad_line_is_warned_of_and_set_back() {
        let (c, w) = Config::read(Some("[open]\neditor = \"a \\\"b\"\n\n[line_args]\nx = \"+{line}\"\nY = \"-l {line} {path}\"\n"));
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(c.command(Tool::Editor, &[]), default_command(Tool::Editor));
        assert_eq!(c.line_args.get("y").map(String::as_str), Some("-l {line} {path}"));
        assert!(!c.line_args.contains_key("x"));
    }

    #[test]
    fn the_chosen_tool_gives_the_diff_and_the_merge() {
        let (c, _) = Config::read(Some("[open]\ndiff_tool = \"meld\"\n"));
        let f = [found("git"), found("meld")];
        assert_eq!(c.command(Tool::Diff, &f), "meld {left} {right}");
        assert!(c.command(Tool::Merge, &f).starts_with("meld --output"));
        // not installed: the default for a diff, nothing for a merge.
        assert_eq!(c.command(Tool::Diff, &[]), default_command(Tool::Diff));
        assert_eq!(c.command(Tool::Merge, &[]), "");
        // a line written wins.
        let (c, _) = Config::read(Some("[open]\ndiff_tool = \"meld\"\ndiff = \"mine {left} {right}\"\n"));
        assert_eq!(c.command(Tool::Diff, &f), "mine {left} {right}");
    }

    #[test]
    fn auto_takes_the_first_installed_but_not_git() {
        let (c, _) = Config::read(Some("[open]\ndiff_tool = \"auto\"\n"));
        assert_eq!(c.command(Tool::Diff, &[found("git"), found("kdiff3")]), "kdiff3 {left} {right}");
        assert_eq!(c.command(Tool::Diff, &[found("git")]), default_command(Tool::Diff));
    }

    #[test]
    fn a_tool_of_ones_own_is_chosen_by_its_name() {
        let text = "[open]\ndiff_tool = \"mine\"\n\n[diff_tools.mine]\ndiff = \"mine {left} {right}\"\nmerge = \"mine {base} {left} {right} -o {merged}\"\n\n[diff_tools.\"bad name\"]\ndiff = \"x\"\n\n[diff_tools.meld]\ndiff = \"x\"\n";
        let (c, w) = Config::read(Some(text));
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(c.command(Tool::Diff, &[]), "mine {left} {right}");
        assert!(c.command(Tool::Merge, &[]).ends_with("-o {merged}"));
    }

    #[test]
    fn edits_are_checked_applied_and_turned_into_writes() {
        let (mut c, _) = Config::read(None);
        assert!(c.apply(&Edit::Command(Tool::Editor, "a \"b".into())).is_err());
        assert_eq!(c.open.editor, "");
        c.apply(&Edit::Command(Tool::Editor, " zed {path} ".into())).unwrap();
        assert_eq!(c.open.editor, "zed {path}");
        let user = UserTool { diff: "mine {left} {right}".into(), merge: String::new() };
        c.apply(&Edit::UserTool("mine".into(), user.clone())).unwrap();
        assert_eq!(c.diff_tools["mine"], user);
        assert!(c.apply(&Edit::UserTool("meld".into(), user.clone())).is_err());
        assert!(c.apply(&Edit::UserTool("x".into(), UserTool::default())).is_err());
        c.apply(&Edit::LineArg("C:\\bin\\MyEd.exe".into(), "--at={line} {path}".into())).unwrap();
        assert!(c.line_args.contains_key("myed"));
        assert!(c.apply(&Edit::LineArg("myed".into(), "--at={line}".into())).is_err());
        c.apply(&Edit::RemoveLineArg("myed".into())).unwrap();
        c.apply(&Edit::RemoveUserTool("mine".into())).unwrap();
        assert!(c.line_args.is_empty() && c.diff_tools.is_empty());
    }

    #[test]
    fn writes_set_a_toml_string_or_remove_the_key() {
        assert_eq!(
            Edit::Command(Tool::Editor, "\"C:\\a b\\e.exe\" {path}".into()).writes(),
            [Write::Set { table: "open".into(), key: "editor".into(), value: quoted("\"C:\\a b\\e.exe\" {path}") }]
        );
        assert_eq!(Edit::Command(Tool::Diff, "  ".into()).writes(), [Write::Remove { table: "open".into(), key: "diff".into() }]);
        let w = Edit::UserTool("mine".into(), UserTool { diff: "d".into(), merge: String::new() }).writes();
        assert_eq!(w[0], Write::Set { table: "diff_tools.mine".into(), key: "diff".into(), value: "\"d\"".into() });
        assert_eq!(w[1], Write::Remove { table: "diff_tools.mine".into(), key: "merge".into() });
        assert_eq!(Edit::RemoveLineArg("Sakura.exe".into()).writes(), [Write::Remove { table: "line_args".into(), key: "sakura".into() }]);
    }

    #[test]
    fn a_written_value_reads_back() {
        // What `writes` produces is valid TOML that `read` takes back as typed.
        let line = "\"C:\\Program Files\\x.exe\" --new {path}";
        let Write::Set { value, .. } = &Edit::Command(Tool::Terminal, line.into()).writes()[0] else { panic!() };
        let (c, w) = Config::read(Some(&format!("[open]\nterminal = {value}\n")));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.open.terminal, line);
    }
}
