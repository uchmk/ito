//! Which `git` to start.

use std::ffi::{OsStr, OsString};
use std::sync::OnceLock;

/// The program to start for git: `"git"` everywhere except Windows, where it is
/// git's own `git.exe` (the one under `git --exec-path`) when that can be found,
/// and `"git"` when it cannot.
///
/// On Windows `git` on `PATH` is usually `Git\cmd\git.exe`, a wrapper that starts
/// the real git. The wrapper costs a second process and a `conhost.exe`
/// whatever creation flags it gets. Git's own exe started with `DETACHED_PROCESS`
/// has no console at all; with `CREATE_NO_WINDOW` it still makes one per call.
///
/// Skipping the wrapper keeps hooks, `ssh`, aliases, Git LFS and the credential
/// manager working, because git's own exe sets up `PATH` for its children
/// (measured on Git for Windows 2.52.0; the one difference is that `%HOME%\bin`
/// comes before `mingw64\bin` and `usr\bin`).
///
/// The first call runs `git --exec-path` and blocks, so call it from a worker,
/// like everything else that starts git. The answer is kept for the process.
pub fn git_program() -> &'static OsStr {
    static PROGRAM: OnceLock<OsString> = OnceLock::new();
    PROGRAM.get_or_init(find).as_os_str()
}

#[cfg(not(windows))]
fn find() -> OsString {
    OsString::from("git")
}

#[cfg(windows)]
fn find() -> OsString {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("git");
    cmd.arg("--exec-path").stdin(Stdio::null()).stderr(Stdio::null());
    crate::own_group(&mut cmd);
    match cmd.output() {
        Ok(out) if out.status.success() => own_exe(&out.stdout, &|p| p.is_file()),
        _ => None,
    }
    .unwrap_or_else(|| OsString::from("git"))
}

/// `<exec-path>\git.exe` from what `git --exec-path` printed (UTF-8, forward
/// slashes, a newline), if the answer is an absolute path and that file exists.
#[cfg(windows)]
fn own_exe(exec_path: &[u8], is_file: &dyn Fn(&std::path::Path) -> bool) -> Option<OsString> {
    use std::path::Path;
    let text = std::str::from_utf8(exec_path).ok()?.trim_end_matches(['\r', '\n']);
    let dir = Path::new(text);
    if !dir.is_absolute() {
        return None;
    }
    let exe = dir.join("git.exe");
    is_file(&exe).then(|| exe.into_os_string())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn yes(_: &Path) -> bool {
        true
    }

    #[test]
    fn a_normal_answer_gives_the_exe_under_it() {
        let want: OsString = Path::new("C:/Program Files/Git/mingw64/libexec/git-core").join("git.exe").into_os_string();
        assert_eq!(own_exe(b"C:/Program Files/Git/mingw64/libexec/git-core\n", &yes), Some(want.clone()));
        assert_eq!(own_exe(b"C:/Program Files/Git/mingw64/libexec/git-core\r\n", &yes), Some(want));
    }

    #[test]
    fn only_line_endings_are_stripped() {
        let want: OsString = Path::new("C:/a b /git-core").join("git.exe").into_os_string();
        assert_eq!(own_exe(b"C:/a b /git-core\n", &yes), Some(want));
    }

    #[test]
    fn a_missing_file_gives_none() {
        assert_eq!(own_exe(b"C:/Git/git-core\n", &|_| false), None);
    }

    #[test]
    fn answers_that_are_not_an_absolute_utf8_path_give_none() {
        assert_eq!(own_exe(b"", &yes), None);
        assert_eq!(own_exe(&[0xff, 0xfe, b'\n'], &yes), None);
        assert_eq!(own_exe(b"/usr/libexec/git-core\n", &yes), None);
        assert_eq!(own_exe(b"libexec/git-core\n", &yes), None);
    }

    #[test]
    fn git_program_starts_and_is_not_the_wrapper() {
        match Command::new("git").arg("--version").output() {
            Ok(o) if o.status.success() => {}
            other => {
                eprintln!("skipped: git is not usable here ({other:?})");
                return;
            }
        }
        let p = git_program();
        let out = Command::new(p).arg("--version").output().unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("git version"));
        assert!(!Path::new(p).ends_with("cmd/git.exe"), "{p:?}");
        // "git" would pass the lines above, so check that the own exe was really picked up.
        let exec = Command::new("git").arg("--exec-path").output().unwrap();
        if let Some(own) = own_exe(&exec.stdout, &|f| f.is_file()) {
            assert_eq!(p, own);
        }
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    #[test]
    fn git_program_is_git() {
        assert_eq!(super::git_program(), "git");
    }
}
