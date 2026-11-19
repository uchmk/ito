//! Finding a program: on `PATH` and, on Windows, in App Paths.
//!
//! [`which`] answers "is it installed" the way the Windows shell (`start`) does: `PATH` with `PATHEXT`, then the
//! App Paths registry keys. [`on_path`] is `PATH` alone. Both read the disk ([`which`] also the registry), so they
//! are not for the UI thread: a name that is missing checks every `PATH` folder times every suffix.
//!
//! Two rules hold for every entry point:
//!
//! * A name whose extension is one of the suffixes (`code.cmd`) is taken as it is. A name without an extension is
//!   tried only with the suffixes (`notepad` finds `notepad.exe`, never an extensionless `notepad` script), as cmd's
//!   "command search sequence" has it. A name with some other extension is tried with the suffixes, then as it is
//!   (`python3.12` finds `python3.12.exe`, `tool.py` finds `tool.py`). Where there are no suffixes (Unix) the name is
//!   taken as it is.
//! * An empty `PATH` entry is the current directory (POSIX `execvp`'s meaning), and on Unix there is no check of the
//!   execute bit: a file is a file.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// Where a program was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// The name had a folder in it (absolute or relative): the file is where it points, with a suffix by the same
    /// rules as on PATH (`C:\x\pwsh` finds `C:\x\pwsh.exe`).
    Given,
    /// A folder on PATH.
    Path,
    /// Windows' App Paths in the registry (HKCU, then HKLM).
    AppPaths,
}

/// A program that was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub path: PathBuf,
    pub via: Via,
}

/// PATH (with PATHEXT on Windows), then App Paths on Windows: the places the Windows shell (`start`) looks.
/// For "is it installed". A program found in App Paths does not start by its bare name through
/// `std::process::Command` / CreateProcess (they never read App Paths): start it by `path`.
/// Reads the disk and the registry: not for the UI thread (a missing name checks every PATH folder x suffix).
pub fn which(name: &str) -> Option<Program> {
    let bare = kind(name)?;
    if let Some(p) = on_path(name) {
        return Some(p);
    }
    if bare {
        return app_path(name).map(|path| Program { path, via: Via::AppPaths });
    }
    None
}

/// PATH only, with PATHEXT's suffixes on Windows.
pub fn on_path(name: &str) -> Option<Program> {
    let exts = path_exts();
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    on_path_in(name, &path_dirs(), &exts)
}

/// The PATH search over given folders and suffixes: no environment, no registry (tests, and callers with their own policy).
///
/// A name with a folder in it is looked for where it points (relative to the current directory when relative) and
/// gives [`Via::Given`]. A bare name is looked for in each folder in order, and in each folder with each candidate in
/// order: the first file wins ([`Via::Path`]); a directory of that name is skipped. A suffix that does not start with
/// `.` or contains a path separator is ignored.
pub fn on_path_in(name: &str, dirs: &[PathBuf], exts: &[&str]) -> Option<Program> {
    let bare = kind(name)?;
    let names = candidates(name, exts);
    if !bare {
        return names.into_iter().map(PathBuf::from).find(|p| p.is_file()).map(|path| Program { path, via: Via::Given });
    }
    dirs.iter().find_map(|d| names.iter().map(|n| d.join(n)).find(|p| p.is_file())).map(|path| Program { path, via: Via::Path })
}

/// The App Paths entry for a bare `name` (Windows; `None` elsewhere). `.exe` is added when `name` has no extension.
///
/// ito's policy (Microsoft does not specify HKCU/HKLM collisions or stale entries): HKCU, then HKLM; the first root
/// whose default value, cleaned, is an absolute path naming an existing file wins. A root with no key, no default
/// value, an empty or relative value, or a missing file is skipped, so a stale entry does not hide a good one.
pub fn app_path(name: &str) -> Option<PathBuf> {
    if kind(name)? {
        app_path_impl(name)
    } else {
        None
    }
}

/// The PATH folders, in order (empty when PATH is unset).
pub fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
}

/// The suffixes a name without an extension may take: PATHEXT on Windows, lowercased; none elsewhere.
pub fn path_exts() -> Vec<String> {
    if cfg!(windows) {
        parse_exts(std::env::var("PATHEXT").ok().as_deref())
    } else {
        Vec::new()
    }
}

const DEFAULT_EXTS: &str = ".com;.exe;.bat;.cmd;.vbs;.vbe;.js;.jse;.wsf;.wsh;.msc";

fn usable_ext(e: &str) -> bool {
    e.starts_with('.') && !e.contains(['/', '\\'])
}

fn parse_exts(raw: Option<&str>) -> Vec<String> {
    let from = |s: &str| -> Vec<String> { s.split(';').map(|e| e.trim().to_ascii_lowercase()).filter(|e| usable_ext(e)).collect() };
    let got = raw.map(from).unwrap_or_default();
    if got.is_empty() {
        from(DEFAULT_EXTS)
    } else {
        got
    }
}

/// `Some(true)` for a bare name, `Some(false)` for one with a folder in it, `None` for one that is refused.
fn kind(name: &str) -> Option<bool> {
    if name.is_empty() || name.contains("://") {
        return None;
    }
    let mut comps = Path::new(name).components();
    let one = matches!((comps.next(), comps.next()), (Some(Component::Normal(_)), None));
    let bare = one && !name.contains('/') && !(cfg!(windows) && name.contains('\\'));
    if bare {
        if name.contains(':') {
            return None;
        }
        if cfg!(windows) && name.chars().any(|c| c.is_control() || "<>\"|?*".contains(c)) {
            return None;
        }
    }
    Some(bare)
}

/// The file names to try for `name`, in order (cmd's command search sequence; see the module's rules).
fn candidates(name: &str, exts: &[&str]) -> Vec<OsString> {
    let exts: Vec<&&str> = exts.iter().filter(|e| usable_ext(e)).collect();
    let ext = Path::new(name).extension().map(|e| format!(".{}", e.to_string_lossy()));
    let known = ext.as_deref().is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)));
    if known || exts.is_empty() {
        return vec![OsString::from(name)];
    }
    let mut names: Vec<OsString> = exts.iter().map(|e| OsString::from(format!("{name}{e}"))).collect();
    if ext.is_some() {
        names.push(OsString::from(name));
    }
    names
}

/// Trim whitespace and NULs, strip one pair of surrounding double quotes, trim again; nothing left is `None`.
#[cfg_attr(not(windows), allow(dead_code))]
fn clean_value(raw: &str) -> Option<String> {
    let trim = |s: &str| s.trim_matches(|c: char| c.is_whitespace() || c == '\0').to_owned();
    let s = trim(raw);
    let s = match s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) if s.len() >= 2 => trim(inner),
        _ => s,
    };
    (!s.is_empty()).then_some(s)
}

#[cfg(windows)]
const APP_PATHS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";

#[cfg(windows)]
fn app_path_impl(name: &str) -> Option<PathBuf> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let key = if Path::new(name).extension().is_some() { name.to_owned() } else { format!("{name}.exe") };
    app_path_from(&[(HKEY_CURRENT_USER, APP_PATHS), (HKEY_LOCAL_MACHINE, APP_PATHS)], &key)
}

#[cfg(not(windows))]
fn app_path_impl(_name: &str) -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn app_path_from(roots: &[(windows::Win32::System::Registry::HKEY, &str)], key: &str) -> Option<PathBuf> {
    roots.iter().find_map(|(root, base)| {
        let raw = read_default(*root, &format!(r"{base}\{key}"))?;
        let text = clean_value(&raw.to_string_lossy())?;
        // Keep the OS string itself when cleaning changed nothing (it may not be valid Unicode).
        let path = if raw.to_string_lossy() == text { raw } else { PathBuf::from(text) };
        (path.is_absolute() && path.is_file()).then_some(path)
    })
}

/// The default value of `root\subkey` as a string (REG_EXPAND_SZ expanded), or `None`.
#[cfg(windows)]
fn read_default(root: windows::Win32::System::Registry::HKEY, subkey: &str) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_MORE_DATA;
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};

    let sub: Vec<u16> = std::ffi::OsStr::new(subkey).encode_wide().chain(Some(0)).collect();
    let mut size = 0u32;
    // SAFETY: `sub` is NUL-terminated and outlives the call; `size` is a valid out pointer.
    let status = unsafe { RegGetValueW(root, PCWSTR(sub.as_ptr()), PCWSTR::null(), RRF_RT_REG_SZ, None, None, Some(&raw mut size)) };
    if status.is_err() {
        return None;
    }
    for _ in 0..2 {
        let mut buf = vec![0u16; size as usize / 2 + 1];
        let mut cap = (buf.len() * 2) as u32;
        // SAFETY: `buf` has `cap` bytes; `cap` is a valid in/out pointer; `sub` as above.
        let status = unsafe {
            RegGetValueW(root, PCWSTR(sub.as_ptr()), PCWSTR::null(), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&raw mut cap))
        };
        if status == ERROR_MORE_DATA {
            size = cap;
            continue;
        }
        if status.is_err() {
            return None;
        }
        let n = (cap as usize / 2).min(buf.len());
        // An expanded REG_EXPAND_SZ reports the unexpanded size, so the text can end before `n`: stop at the first NUL.
        let wide = &buf[..n];
        let wide = &wide[..wide.iter().position(|&u| u == 0).unwrap_or(wide.len())];
        return Some(PathBuf::from(OsString::from_wide(wide)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let d = std::env::temp_dir().join(format!("ito-which-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
        fn dir(&self, sub: &str) -> PathBuf {
            let d = self.0.join(sub);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn touch(p: &Path) {
        std::fs::write(p, "").unwrap();
    }
    const EXE_CMD: &[&str] = &[".exe", ".cmd"];

    #[test]
    fn a_name_without_extension_takes_a_suffix_never_none() {
        let s = Scratch::new("suffix");
        let d = s.dir("a");
        touch(&d.join("notepad"));
        assert_eq!(on_path_in("notepad", std::slice::from_ref(&d), EXE_CMD), None);
        touch(&d.join("notepad.exe"));
        let got = on_path_in("notepad", std::slice::from_ref(&d), EXE_CMD).unwrap();
        assert_eq!(got, Program { path: d.join("notepad.exe"), via: Via::Path });
    }

    #[test]
    fn a_name_with_an_extension_is_taken_as_is() {
        let s = Scratch::new("ext");
        let d = s.dir("a");
        touch(&d.join("code.cmd"));
        touch(&d.join("tool.py"));
        let dirs = [d.clone()];
        assert_eq!(on_path_in("code.cmd", &dirs, EXE_CMD).unwrap().path, d.join("code.cmd"));
        assert_eq!(on_path_in("tool.py", &dirs, EXE_CMD).unwrap().path, d.join("tool.py"));
    }

    #[test]
    fn a_dotted_name_takes_a_suffix_first() {
        let s = Scratch::new("dotted");
        let d = s.dir("a");
        touch(&d.join("python3.12.exe"));
        touch(&d.join("python3.12"));
        let dirs = [d.clone()];
        assert_eq!(on_path_in("python3.12", &dirs, EXE_CMD).unwrap().path, d.join("python3.12.exe"));
        assert_eq!(on_path_in("python3.12", &dirs, &[]).unwrap().path, d.join("python3.12"));
        assert_eq!(candidates("tool.py", EXE_CMD), ["tool.py.exe", "tool.py.cmd", "tool.py"]);
        assert_eq!(candidates("code.CMD", EXE_CMD), ["code.CMD"]);
        assert_eq!(candidates(".tool", EXE_CMD), [".tool.exe", ".tool.cmd"]);
    }

    #[test]
    fn folders_come_before_suffixes() {
        let s = Scratch::new("major");
        let (d1, d2) = (s.dir("one"), s.dir("two"));
        touch(&d1.join("tool.cmd"));
        touch(&d2.join("tool.exe"));
        let dirs = [d1.clone(), d2.clone()];
        assert_eq!(on_path_in("tool", &dirs, EXE_CMD).unwrap().path, d1.join("tool.cmd"));
        touch(&d1.join("tool.exe"));
        assert_eq!(on_path_in("tool", &dirs, EXE_CMD).unwrap().path, d1.join("tool.exe"));
    }

    #[test]
    fn no_suffixes_take_the_name_and_skip_a_directory() {
        let s = Scratch::new("none");
        let (d1, d2) = (s.dir("one"), s.dir("two"));
        std::fs::create_dir_all(d1.join("tool")).unwrap();
        touch(&d2.join("tool"));
        assert_eq!(on_path_in("tool", &[d1, d2.clone()], &[]).unwrap().path, d2.join("tool"));
    }

    #[test]
    fn a_name_with_a_folder_is_given() {
        let s = Scratch::new("given");
        let d = s.dir("a");
        touch(&d.join("tool.exe"));
        let full = d.join("tool.exe");
        let got = on_path_in(full.to_str().unwrap(), &[], EXE_CMD).unwrap();
        assert_eq!(got, Program { path: full, via: Via::Given });
        let bare = d.join("tool");
        let got = on_path_in(bare.to_str().unwrap(), &[], &[".exe"]).unwrap();
        assert_eq!(got, Program { path: d.join("tool.exe"), via: Via::Given });
    }

    #[test]
    fn a_leading_dot_name_is_bare() {
        let s = Scratch::new("dot");
        let d = s.dir("a");
        touch(&d.join(".tool"));
        let got = on_path_in(".tool", std::slice::from_ref(&d), &[]).unwrap();
        assert_eq!(got.via, Via::Path);
    }

    #[test]
    fn refused_names() {
        for n in ["", "ms-settings:display", "https://example.com/x"] {
            assert_eq!(on_path_in(n, &[PathBuf::from(".")], &[]), None, "{n}");
            assert_eq!(app_path(n), None, "{n}");
            assert_eq!(which(n), None, "{n}");
        }
        if cfg!(windows) {
            for n in ["a|b", "a?"] {
                assert_eq!(kind(n), None, "{n}");
                assert_eq!(app_path(n), None, "{n}");
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_path_is_given() {
        let s = Scratch::new("verbatim");
        let d = s.dir("a");
        touch(&d.join("tool.exe"));
        let name = format!(r"\\?\{}\tool.exe", d.display());
        let got = on_path_in(&name, &[], EXE_CMD).unwrap();
        assert_eq!(got.via, Via::Given);
    }

    #[test]
    fn bad_suffixes_are_ignored() {
        let s = Scratch::new("badext");
        let d = s.dir("a");
        touch(&d.join("tool.exe"));
        touch(&d.join("toolexe"));
        assert_eq!(on_path_in("tool", std::slice::from_ref(&d), &["/../x", "exe", ".exe"]).unwrap().path, d.join("tool.exe"));
    }

    #[test]
    fn pathext_is_parsed() {
        assert_eq!(parse_exts(Some(".COM;.EXE;;.Bat")), [".com", ".exe", ".bat"]);
        let default = parse_exts(None);
        assert_eq!(default.len(), 11);
        assert_eq!(default[1], ".exe");
        assert_eq!(parse_exts(Some("")), default);
        assert_eq!(parse_exts(Some("exe;/x")), default);
    }

    #[test]
    fn values_are_cleaned() {
        assert_eq!(clean_value("\"C:\\P F\\x.exe\"").as_deref(), Some("C:\\P F\\x.exe"));
        assert_eq!(clean_value("  x.exe \0").as_deref(), Some("x.exe"));
        assert_eq!(clean_value("\"\""), None);
        assert_eq!(clean_value(""), None);
    }

    #[test]
    fn an_unknown_app_path_is_none() {
        assert_eq!(app_path("ito-no-such-program-243"), None);
    }

    #[cfg(windows)]
    mod registry {
        use super::*;
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::*;

        fn wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(Some(0)).collect()
        }

        struct Tree(String);
        impl Drop for Tree {
            fn drop(&mut self) {
                let w = wide(&self.0);
                // SAFETY: `w` is NUL-terminated and outlives the call.
                unsafe {
                    let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(w.as_ptr()));
                    let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(w.as_ptr()));
                }
            }
        }

        /// `<tree>\<base>\App Paths\tool.exe` with an optional default value; returns the base subkey.
        fn entry(tree: &Tree, base: &str, value: Option<(&str, REG_VALUE_TYPE)>) -> String {
            let app_paths = format!(r"{}\{base}\App Paths", tree.0);
            let sub = wide(&format!(r"{app_paths}\tool.exe"));
            let mut key = HKEY::default();
            // SAFETY: `sub` is NUL-terminated; `key` is a valid out pointer; the key is closed below.
            unsafe {
                let st = RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(sub.as_ptr()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_WRITE,
                    None,
                    &mut key,
                    None,
                );
                assert!(st.is_ok(), "create: {st:?}");
                if let Some((v, ty)) = value {
                    let w = wide(v);
                    let bytes: Vec<u8> = w.iter().flat_map(|u| u.to_le_bytes()).collect();
                    let st = RegSetValueExW(key, PCWSTR::null(), None, ty, Some(&bytes));
                    assert!(st.is_ok(), "set: {st:?}");
                }
                let _ = RegCloseKey(key);
            }
            app_paths
        }

        fn tree(tag: &str) -> Tree {
            Tree(format!(r"Software\uchmk-ito-test-243-{tag}-{}", std::process::id()))
        }

        #[test]
        fn an_expand_value_is_expanded_and_unquoted() {
            let t = tree("expand");
            let a = entry(&t, "a", Some((r#""%SystemRoot%\System32\cmd.exe""#, REG_EXPAND_SZ)));
            let got = app_path_from(&[(HKEY_CURRENT_USER, a.as_str())], "tool.exe").unwrap();
            assert!(got.is_absolute() && got.is_file(), "{got:?}");
            assert!(got.to_string_lossy().to_ascii_lowercase().ends_with(r"system32\cmd.exe"));
        }

        #[test]
        fn a_stale_root_falls_through() {
            let t = tree("fall");
            let a = entry(&t, "a", Some((r"C:\ito-no-such-dir-243\x.exe", REG_SZ)));
            let b = entry(&t, "b", Some((r"%SystemRoot%\System32\cmd.exe", REG_EXPAND_SZ)));
            let got = app_path_from(&[(HKEY_CURRENT_USER, a.as_str()), (HKEY_CURRENT_USER, b.as_str())], "tool.exe").unwrap();
            assert!(got.is_file());
        }

        #[test]
        fn no_value_or_a_relative_value_is_skipped() {
            let t = tree("skip");
            let a = entry(&t, "a", None);
            let b = entry(&t, "b", Some(("cmd.exe", REG_SZ)));
            assert_eq!(app_path_from(&[(HKEY_CURRENT_USER, a.as_str())], "tool.exe"), None);
            assert_eq!(app_path_from(&[(HKEY_CURRENT_USER, b.as_str())], "tool.exe"), None);
            assert_eq!(app_path_from(&[(HKEY_CURRENT_USER, a.as_str())], "nothing.exe"), None);
        }
    }
}
