//! Tests. Every file operation happens inside a `tempfile` folder.

mod delete;
mod matching;
#[cfg(windows)]
mod oracle;
mod roots;
mod tree;

use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use crate::{Error, Event};

pub(crate) fn write(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(path, text).unwrap();
}

pub(crate) fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

pub(crate) fn set_mtime(path: &Path, t: SystemTime) {
    File::options().write(true).open(path).unwrap().set_modified(t).unwrap();
}

pub(crate) fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path).unwrap().modified().unwrap()
}

pub(crate) fn days(n: u64) -> Duration {
    Duration::from_secs(n * 86_400)
}

pub(crate) fn no_stop() -> AtomicBool {
    AtomicBool::new(false)
}

/// A log of events as text, one line each.
pub(crate) fn log() -> (std::rc::Rc<std::cell::RefCell<Vec<String>>>, impl FnMut(Event)) {
    let lines = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let l = lines.clone();
    (lines, move |e: Event| l.borrow_mut().push(format!("{e:?}")))
}

pub(crate) fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

/// A link to a folder: a junction on Windows (no admin needed), a symlink elsewhere.
pub(crate) fn link_dir(target: &Path, link: &Path) {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).output().unwrap();
        assert!(out.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&out.stdout));
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// A symlink to a file. Windows needs developer mode: false (with a note) when it cannot be made.
pub(crate) fn link_file(target: &Path, link: &Path) -> bool {
    #[cfg(windows)]
    let r = std::os::windows::fs::symlink_file(target, link);
    #[cfg(unix)]
    let r = std::os::unix::fs::symlink(target, link);
    if let Err(e) = &r {
        eprintln!("note: cannot make a file symlink here ({e}); this part of the test is skipped");
    }
    r.is_ok()
}

/// Holds a file open so that nobody can delete, rename or read it (Windows).
#[cfg(windows)]
pub(crate) fn lock(path: &Path) -> File {
    use std::os::windows::fs::OpenOptionsExt;
    File::options().read(true).write(true).share_mode(0).open(path).unwrap()
}

/// Holds a file open for reading with sharing for read only: it cannot be deleted or renamed, but its
/// attributes can be changed (Windows).
#[cfg(windows)]
pub(crate) fn lock_for_delete(path: &Path) -> File {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    File::options().read(true).share_mode(FILE_SHARE_READ).open(path).unwrap()
}

#[cfg(windows)]
pub(crate) fn set_read_only(path: &Path, on: bool) {
    let mut perm = fs::metadata(path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(on);
    fs::set_permissions(path, perm).unwrap();
}

#[cfg(windows)]
pub(crate) fn is_read_only(path: &Path) -> bool {
    fs::metadata(path).unwrap().permissions().readonly()
}

#[test]
fn error_words_in_both_languages() {
    use ito_common::ioerror;
    let source = io::Error::from(io::ErrorKind::PermissionDenied);
    let e = Error::Io { path: "C:\\x".into(), doing: crate::Doing::Delete, source };
    let src = io::Error::from(io::ErrorKind::PermissionDenied);
    assert_eq!(e.words("en"), format!("cannot delete C:\\x ({})", ioerror::words(&src, "en")));
    let ja = e.words("ja");
    assert!(ja.starts_with("C:\\x を消せません（") && ja.ends_with('）'), "{ja}");
    assert!(ja.contains(&ioerror::words(&src, "ja")));
    assert!(e.to_string().starts_with("cannot delete C:\\x: "), "{e}");
    assert!(e.to_string().contains(&ioerror::detail(&src)));
    assert!(std::error::Error::source(&e).is_some());
    let m = Error::Mismatch { path: "t".into(), from: "aa".into(), to: "bb".into() };
    assert!(m.to_string().contains("aa") && m.to_string().contains("bb"));
    assert!(m.words("ja").contains("SHA-256"));
}

#[test]
fn wildcard_rules() {
    use crate::pat::matches;
    assert!(matches("*.LOG", "a.log", true));
    assert!(!matches("*.LOG", "a.log", false));
    assert!(matches("a.*", "a", true), "a pattern ending in .* takes a name without a dot on Windows");
    assert!(!matches("a.*", "a", false));
    assert!(matches("*.*", "readme", true));
    assert!(!matches("*.*", "readme", false));
    assert!(!matches("a.*", "ab", true));
    assert!(!matches("*.htm", "x.html", true), "no 8.3 short names");
    assert!(matches("a?.txt", "ab.txt", false));
}
