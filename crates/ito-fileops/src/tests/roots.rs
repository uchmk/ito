//! The checks on the paths a caller names. They are pure (no listing, no deleting), so real roots are safe to test.

use std::path::Path;

use crate::safe::{check_empty_target, check_real_folder, is_root, overlap};
use crate::{Error, Unsafe};

#[test]
fn roots_are_found_from_the_text() {
    assert!(is_root(Path::new("/")));
    assert!(is_root(Path::new("/a/..")));
    assert!(!is_root(Path::new("/a")));
    assert!(!is_root(Path::new(".")));
    assert!(!is_root(Path::new("a/b")));
    assert_eq!(check_empty_target(Path::new("")), Err(Unsafe::Empty));
    assert_eq!(check_empty_target(Path::new("/")), Err(Unsafe::Root));
    assert_eq!(check_empty_target(Path::new("/a")), Ok(()));
}

#[cfg(windows)]
#[test]
fn windows_roots() {
    for p in [
        "C:\\",
        "C:/",
        "C:",
        "c:\\",
        "\\\\?\\C:\\",
        "\\\\server\\share",
        "\\\\server\\share\\",
        "\\\\?\\UNC\\server\\share\\",
        "C:\\x\\..",
        "C:\\",
    ] {
        assert!(is_root(Path::new(p)), "{p} is a root");
        assert_eq!(check_empty_target(Path::new(p)), Err(Unsafe::Root), "{p}");
    }
    for p in ["C:\\x", "\\\\server\\share\\x", "\\\\?\\C:\\x", "x", "C:x"] {
        assert!(!is_root(Path::new(p)), "{p} is not a root");
    }
    // The real check refuses a root before it lists or deletes anything.
    for p in ["C:\\", "\\\\?\\C:\\", "\\\\server\\share"] {
        match check_real_folder(Path::new(p), true) {
            Err(Error::Unsafe { why: Unsafe::Root, .. }) => {}
            other => panic!("{p}: {other:?}"),
        }
    }
    // The check for delete_old (`refuse_root` false) only needs a real folder, so a drive root passes it. It
    // only looks (symlink_metadata); nothing is listed or deleted here.
    let drive = format!("{}\\", std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()));
    assert!(check_real_folder(Path::new(&drive), false).is_ok(), "{drive}");
    assert!(check_real_folder(Path::new(""), false).is_err());
}

#[test]
fn empty_path_is_refused_everywhere() {
    for refuse_root in [true, false] {
        match check_real_folder(Path::new(""), refuse_root) {
            Err(Error::Unsafe { why: Unsafe::Empty, .. }) => {}
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn overlap_is_by_component() {
    assert!(overlap(Path::new("/a/b"), Path::new("/a/b")));
    assert!(overlap(Path::new("/a/b"), Path::new("/a/b/c")));
    assert!(overlap(Path::new("/a/b/c"), Path::new("/a/b")));
    assert!(!overlap(Path::new("/a/b"), Path::new("/a/bc")));
    assert!(!overlap(Path::new("/a/b"), Path::new("/a/c")));
}
