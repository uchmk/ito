//! The diff / merge tools an app can offer, and finding which are installed.
//! Looking touches PATH and the disk, so do it on a worker thread
//! ([`detect_in_background`]), never on the UI thread.
//!
//! The command lines are templates for [`crate::argv`]; `{prog}` stands for
//! the program (a bare name found on PATH, or a quoted full path) and is
//! filled in by [`Found`].

use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// A diff / merge tool the crate knows the command lines of.
#[derive(Debug, PartialEq, Eq)]
pub struct Known {
    /// The name config.toml and the settings screen use (`diff_tool = "meld"`).
    pub name: &'static str,
    pub label: &'static str,
    /// Program names tried on PATH, best first.
    pub programs: &'static [&'static str],
    /// Where a Windows installer puts it: (environment variable, folder under it, file).
    pub installs: &'static [(&'static str, &'static str, &'static str)],
    pub diff: &'static str,
    /// Empty: the tool does no three-way merge.
    pub merge: &'static str,
}

// Command lines follow each tool's documentation. WinMerge's three-way line was
// run on a real install (WinMerge 2.16, 2026-10-24): it opens left | base | right.
pub const KNOWN: &[Known] = &[
    Known {
        name: "winmerge",
        label: "WinMerge",
        programs: &["WinMergeU"],
        installs: &[
            ("ProgramFiles", "WinMerge", "WinMergeU.exe"),
            ("ProgramFiles(x86)", "WinMerge", "WinMergeU.exe"),
            ("LOCALAPPDATA", "Programs\\WinMerge", "WinMergeU.exe"),
        ],
        diff: "{prog} -e -u {left} {right}",
        merge: "{prog} -e -u {left} {base} {right} -o {merged}",
    },
    Known {
        name: "beyondcompare",
        label: "Beyond Compare",
        programs: &["BComp", "bcomp"],
        installs: &[
            ("ProgramFiles", "Beyond Compare 5", "BComp.exe"),
            ("ProgramFiles", "Beyond Compare 4", "BComp.exe"),
            ("ProgramFiles(x86)", "Beyond Compare 4", "BComp.exe"),
        ],
        diff: "{prog} {left} {right}",
        merge: "{prog} {left} {right} {base} {merged}",
    },
    Known {
        name: "meld",
        label: "Meld",
        programs: &["meld"],
        installs: &[("ProgramFiles", "Meld", "Meld.exe"), ("ProgramFiles(x86)", "Meld", "Meld.exe")],
        diff: "{prog} {left} {right}",
        merge: "{prog} --output {merged} {left} {base} {right}",
    },
    Known {
        name: "kdiff3",
        label: "KDiff3",
        programs: &["kdiff3"],
        installs: &[("ProgramFiles", "KDiff3", "kdiff3.exe")],
        diff: "{prog} {left} {right}",
        merge: "{prog} {base} {left} {right} -o {merged}",
    },
    Known {
        name: "vscode",
        label: "VS Code",
        programs: &["code"],
        installs: &[("LOCALAPPDATA", "Programs\\Microsoft VS Code\\bin", "code.cmd")],
        diff: "{prog} --diff {left} {right}",
        merge: "{prog} --merge {left} {right} {base} {merged}",
    },
    Known {
        name: "git",
        label: "git difftool",
        programs: &["git"],
        installs: &[],
        diff: "{prog} difftool --no-index --no-prompt {left} {right}",
        merge: "",
    },
];

/// The known tool called `name`.
pub fn known(name: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|k| k.name == name)
}

/// A known tool that is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub tool: &'static Known,
    /// A bare name (found on PATH) or a quoted full path, ready for a template.
    pub program: String,
}

impl Found {
    /// The command line that opens two files.
    pub fn diff_command(&self) -> String {
        self.tool.diff.replace("{prog}", &self.program)
    }

    /// The command line that merges three files; none when the tool cannot.
    pub fn merge_command(&self) -> Option<String> {
        (!self.tool.merge.is_empty()).then(|| self.tool.merge.replace("{prog}", &self.program))
    }
}

/// The known tools that are installed, in the order of [`KNOWN`]. Reads PATH
/// and the disk: not for the UI thread.
pub fn detect() -> Vec<Found> {
    let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    detect_with(&dirs, &|k| std::env::var(k).ok())
}

/// [`detect`] on a thread of its own; the result arrives once on the receiver.
pub fn detect_in_background() -> mpsc::Receiver<Vec<Found>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(detect());
    });
    rx
}

/// [`detect`] over the given PATH folders and environment.
pub fn detect_with(path_dirs: &[PathBuf], env: &dyn Fn(&str) -> Option<String>) -> Vec<Found> {
    KNOWN
        .iter()
        .filter_map(|tool| {
            if let Some(name) = tool.programs.iter().find(|n| on_path(n, path_dirs).is_some()) {
                return Some(Found { tool, program: (*name).to_owned() });
            }
            tool.installs.iter().find_map(|(var, dir, file)| {
                let full = Path::new(&env(var)?).join(dir).join(file);
                full.is_file().then(|| Found { tool, program: format!("\"{}\"", full.display()) })
            })
        })
        .collect()
}

/// Where the program `name` is on `dirs` (with the executable extensions on Windows).
pub fn on_path(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat", ".com"] } else { &[""] };
    dirs.iter().find_map(|d| exts.iter().map(|x| d.join(format!("{name}{x}"))).find(|p| p.is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{argv, Vars};

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-openin-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn four() -> Vars {
        let s = |x: &str| Some(x.to_owned());
        Vars { left: s("L"), right: s("R"), base: s("B"), merged: s("M"), ..Vars::default() }
    }

    #[test]
    fn every_command_line_is_a_command_line() {
        for k in KNOWN {
            let f = Found { tool: k, program: "prog".into() };
            assert!(argv(&f.diff_command(), &four()).is_ok(), "{}", k.name);
            if let Some(m) = f.merge_command() {
                let a = argv(&m, &four()).unwrap();
                assert!(a.contains(&"M".to_owned()), "{}: merge writes the merged file", k.name);
            }
        }
    }

    #[test]
    fn names_are_unique_and_looked_up() {
        for (i, k) in KNOWN.iter().enumerate() {
            assert_eq!(known(k.name), Some(k));
            assert!(KNOWN[..i].iter().all(|o| o.name != k.name));
        }
        assert!(known("nope").is_none());
    }

    #[test]
    fn a_tool_on_path_is_found_by_its_bare_name() {
        let d = scratch("path");
        let file = if cfg!(windows) { "meld.exe" } else { "meld" };
        std::fs::write(d.join(file), "").unwrap();
        let found = detect_with(std::slice::from_ref(&d), &|_| None);
        assert_eq!(found.iter().map(|f| f.tool.name).collect::<Vec<_>>(), ["meld"]);
        assert_eq!(found[0].diff_command(), "meld {left} {right}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_installed_tool_off_path_is_found_by_a_quoted_full_path() {
        let d = scratch("install");
        let dir = d.join("Beyond Compare 5");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("BComp.exe"), "").unwrap();
        let root = d.to_string_lossy().into_owned();
        let found = detect_with(&[], &|k| (k == "ProgramFiles").then(|| root.clone()));
        assert_eq!(found.len(), 1);
        assert!(found[0].program.starts_with('"') && found[0].program.ends_with("BComp.exe\""));
        let a = argv(&found[0].merge_command().unwrap(), &four()).unwrap();
        assert!(a[0].ends_with("BComp.exe") && a[1..] == ["L", "R", "B", "M"], "{a:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nothing_installed_is_an_empty_list_and_git_cannot_merge() {
        assert!(detect_with(&[], &|_| None).is_empty());
        assert!(Found { tool: known("git").unwrap(), program: "git".into() }.merge_command().is_none());
    }

    #[test]
    fn background_detection_answers() {
        assert!(detect_in_background().recv().is_ok());
    }
}
