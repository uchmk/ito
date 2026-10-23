//! Starting another uchmk app by its name, from wherever it is
//! (`ito_common::apps`: pinned in common.toml, registered by the app itself,
//! or on PATH). Nothing is waited for, no shell is involved and no window
//! flashes. The app's own command line decides what the arguments mean.
//!
//! ```no_run
//! # use std::path::Path;
//! ito_openin::app::open("kura", Path::new("C:/work/repo"))?;      // kura C:/work/repo
//! ito_openin::app::launch("michi", &["--repo", "C:/work/repo"], None)?;
//! if ito_openin::app::available("tsumugi") { /* show the menu item */ }
//! # Ok::<(), String>(())
//! ```

use std::path::{Path, PathBuf};

use ito_common::apps::{self, Source};

/// Whether `app` can be started now, e.g. to grey out a menu item.
/// Looks at the disk: not on every frame.
pub fn available(app: &str) -> bool {
    apps::find(app).is_some()
}

/// Where `app` would be started from.
pub fn exe(app: &str) -> Result<PathBuf, String> {
    apps::find(app).map(|f| f.exe).ok_or_else(|| not_found(app))
}

/// What to tell a person when `app` is nowhere.
pub fn not_found(app: &str) -> String {
    format!("{app} was not found: start it once, or set its path in Settings (Open in)")
}

/// Starts `app` with `args` (in `cwd`).
pub fn launch(app: &str, args: &[&str], cwd: Option<&Path>) -> Result<(), String> {
    let mut argv = vec![exe(app)?.to_string_lossy().into_owned()];
    argv.extend(args.iter().map(|a| (*a).to_owned()));
    crate::spawn(&argv, cwd)
}

/// Starts `app` with `path` as its one argument, in `path` when that is a
/// folder (else in the folder it is in).
pub fn open(app: &str, path: &Path) -> Result<(), String> {
    let cwd = if path.is_dir() { Some(path) } else { path.parent().filter(|p| !p.as_os_str().is_empty()) };
    launch(app, &[&path.to_string_lossy()], cwd)
}

/// The exe a first word of a command line stands for, when it is an app's bare
/// name pinned or registered (what [`crate::spawn`] does); `None` for anything
/// else, which is started as it is.
pub(crate) fn pinned_program(word: &str) -> Option<PathBuf> {
    let name = word.strip_suffix(".exe").unwrap_or(word);
    let f = apps::pinned(name)?;
    debug_assert!(matches!(f.source, Source::Setting | Source::Registered));
    Some(f.exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_that_are_not_app_names_are_left_alone() {
        for w in ["code.cmd", r"C:\Program Files\x\x.exe", "/usr/bin/git", "..", "Code"] {
            assert_eq!(pinned_program(w), None, "{w}");
        }
    }

    #[test]
    fn an_app_that_is_nowhere_says_so() {
        // Neither registered, pinned nor on PATH.
        let e = launch("uchmk-no-such-app-zz", &[], None).unwrap_err();
        assert!(e.contains("uchmk-no-such-app-zz was not found"), "{e}");
        assert!(!available("uchmk-no-such-app-zz"));
    }
}
