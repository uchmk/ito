//! The few path helpers the pane needs, copied from filer's `util.rs` so the
//! crate stands alone. Keep them in step with filer's until filer uses these.

use std::path::{Component, Path, PathBuf};

/// Where the program `exe` is: a name with a folder in it where it points, a bare one along `PATH` (on Windows with
/// `.exe` added to a name without an extension, as CreateProcess does).
/// On Windows only `.exe` counts as a suffix, because the shell is started by its bare name through CreateProcess,
/// which appends nothing else and never reads App Paths (so `pwsh.cmd` is not `pwsh`).
pub fn locate(exe: &str) -> Option<std::path::PathBuf> {
    const EXE: &[&str] = if cfg!(windows) { &[".exe"] } else { &[] };
    ito_common::which::on_path_in(exe, &ito_common::which::path_dirs(), EXE).map(|p| p.path)
}

/// Lexically normalize a path (resolve `.`/`..`) without touching the filesystem.
///
/// `..` never climbs past a root, so a drive root, a UNC share root (`\\host\share`)
/// and `/` all stay put — the same as the OS resolves them.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    // Roots pushed so far (a prefix and/or a separator), and the components
    // above them that `..` is allowed to pop.
    let mut rooted = false;
    let mut depth = 0usize;
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if depth > 0 {
                    out.pop();
                    depth -= 1;
                } else if !rooted {
                    // Relative path: keep climbing, there is nothing to pop.
                    out.push("..");
                }
                // At a root `..` is the root itself; drop it.
            }
            // Only Windows produces a prefix, and only there can it be spelled
            // with forward slashes (`//host/share`); settle on one spelling so
            // paths compare and display the same either way.
            Component::Prefix(_) => {
                out.push(backslashed(c.as_os_str()));
                rooted = true;
            }
            Component::RootDir => {
                out.push(c.as_os_str());
                rooted = true;
            }
            Component::Normal(s) => {
                out.push(s);
                depth += 1;
            }
        }
    }
    if out.as_os_str().is_empty() {
        return PathBuf::from(".");
    }
    if host_only_unc(path) {
        // `\\host` without a share is no prefix to `std`, which would leave us
        // with `\host` — a different place. Keep the pair the user typed.
        let mut s = std::ffi::OsString::from(r"\");
        s.push(out.as_os_str());
        return PathBuf::from(s);
    }
    out
}

fn backslashed(s: &std::ffi::OsStr) -> std::ffi::OsString {
    match s.to_str() {
        Some(t) if t.contains('/') => std::ffi::OsString::from(t.replace('/', r"\")),
        _ => s.to_os_string(),
    }
}

/// `\\host` or `//host`: the start of a UNC path that names no share yet.
/// `\\host` with no share after it — a server, not a directory.
pub fn host_only_unc(path: &Path) -> bool {
    if !cfg!(windows) {
        // A leading `//` is an ordinary path elsewhere.
        return false;
    }
    let b = path.as_os_str().as_encoded_bytes();
    b.len() > 2
        && matches!(b[0], b'\\' | b'/')
        && matches!(b[1], b'\\' | b'/')
        && !matches!(b[2], b'\\' | b'/')
        && !matches!(path.components().next(), Some(Component::Prefix(_)))
}

pub fn file_name(path: &Path) -> String {
    if let Some(n) = path.file_name() {
        return n.to_string_lossy().into_owned();
    }
    // A share root is named after its share. `std` has no file name for one,
    // folding the host and the share into a single prefix, and the fallback
    // below — the path itself, which is what a drive root wants — listed every
    // share on a server under its full address instead of its name.
    if let Some(n) = unc_share(path) {
        return n;
    }
    // Root of a drive: `C:\` is what it is called.
    path.to_string_lossy().into_owned()
}

/// The share out of a share root: `\\host\share` → `share`. `None` for
/// anything else, including `\\host`, which names no share, and a path inside
/// a share, which has an ordinary file name.
///
/// Written against the string rather than the components, for the same reason
/// as [`unc_host`]: it can then be reasoned about and tested anywhere.
fn unc_share(path: &Path) -> Option<String> {
    let s = path.to_str()?;
    let rest = s.strip_prefix(r"\\").or_else(|| s.strip_prefix("//"))?;
    let mut parts = rest.split(['\\', '/']).filter(|p| !p.is_empty());
    parts.next()?;
    let share = parts.next()?;
    parts.next().is_none().then(|| share.to_owned())
}

/// A fresh, empty directory for one test, named after the test's thread so two
/// tests never share one (filer's `util::test_dir`). Removed again when the
/// test binary exits, unless `TSUMUGI_KEEP_TEST_DIRS` is set.
#[cfg(test)]
pub fn test_dir(what: &str) -> PathBuf {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        unsafe extern "C" {
            fn atexit(cb: extern "C" fn()) -> i32;
        }
        unsafe { atexit(remove_own_test_dirs) };
    });
    let who =
        std::thread::current().name().unwrap_or("main").replace("::", "-").replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "_");
    let dir = std::env::temp_dir().join(format!("{TEST_DIR_PREFIX}{what}-{who}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temp directory for the test");
    dir
}

#[cfg(test)]
const TEST_DIR_PREFIX: &str = "ito-pane-test-";

#[cfg(test)]
extern "C" fn remove_own_test_dirs() {
    if std::env::var_os("TSUMUGI_KEEP_TEST_DIRS").is_some() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    let suffix = format!("-{}", std::process::id());
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(TEST_DIR_PREFIX) && name.ends_with(&suffix) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}
