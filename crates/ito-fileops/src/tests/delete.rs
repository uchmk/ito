use std::fs;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use super::*;
use crate::{delete_old, empty, Doing, End, Error, Unsafe};

fn old_file(dir: &Path, name: &str, age_days: u64, now: SystemTime) {
    let p = dir.join(name);
    write(&p, name);
    set_mtime(&p, now - days(age_days));
}

#[test]
fn delete_old_cuts_strictly_at_the_cutoff() {
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    old_file(t.path(), "old.txt", 40, now);
    old_file(t.path(), "new.txt", 10, now);
    let cutoff = now - days(30);
    for (name, t_) in [
        ("exact.txt", cutoff),
        ("just_older.txt", cutoff - Duration::from_millis(1)),
        ("just_newer.txt", cutoff + Duration::from_millis(1)),
    ] {
        write(&t.path().join(name), name);
        set_mtime(&t.path().join(name), t_);
    }
    let (events, mut on) = log();
    let r = delete_old(t.path(), "*", days(30), now, &no_stop(), &mut on);
    assert!(r.end.is_done(), "{:?}", r.end);
    let mut got: Vec<_> = r.paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    got.sort();
    assert_eq!(got, ["just_older.txt", "old.txt"]);
    assert_eq!(names(t.path()), ["exact.txt", "just_newer.txt", "new.txt"]);
    assert_eq!(events.borrow().len(), 2);
}

#[test]
fn delete_old_with_a_huge_age_deletes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    old_file(t.path(), "a.txt", 4000, now);
    let r = delete_old(t.path(), "*", Duration::MAX, now, &no_stop(), &mut |_| {});
    assert!(r.end.is_done() && r.paths.is_empty());
    assert_eq!(names(t.path()), ["a.txt"]);
}

#[test]
fn delete_old_pattern_and_case() {
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    for n in ["a.log", "B.LOG", "c.txt", "d.log.bak"] {
        old_file(t.path(), n, 40, now);
    }
    let r = delete_old(t.path(), "*.log", days(30), now, &no_stop(), &mut |_| {});
    assert!(r.end.is_done());
    if cfg!(windows) {
        assert_eq!(names(t.path()), ["c.txt", "d.log.bak"], "case-insensitive on Windows");
    } else {
        assert_eq!(names(t.path()), ["B.LOG", "c.txt", "d.log.bak"], "case-sensitive elsewhere");
    }
    let r = delete_old(t.path(), "?.txt", days(30), now, &no_stop(), &mut |_| {});
    assert_eq!(r.paths.len(), 1);
}

#[test]
fn delete_old_does_not_recurse_and_keeps_folders_and_links() {
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let outside = t.path().join("outside");
    old_file(&outside, "keep.log", 40, now);
    let work = t.path().join("work");
    fs::create_dir(&work).unwrap();
    old_file(&work, "a.log", 40, now);
    old_file(&work.join("sub"), "inner.log", 40, now);
    fs::create_dir(work.join("dir.log")).unwrap();
    link_dir(&outside, &work.join("link.log"));
    let (events, mut on) = log();
    let r = delete_old(&work, "*.log", days(30), now, &no_stop(), &mut on);
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!(r.paths.len(), 1);
    assert_eq!(names(&work), ["dir.log", "link.log", "sub"]);
    assert!(work.join("sub").join("inner.log").exists());
    assert!(outside.join("keep.log").exists(), "the target of a link survives");
    assert!(events.borrow().iter().any(|e| e.contains("Skipped") && e.contains("Link")), "{:?}", events.borrow());
}

#[test]
fn delete_old_refuses_a_link_as_the_folder_and_an_empty_path() {
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let real = t.path().join("real");
    old_file(&real, "a.log", 40, now);
    let link = t.path().join("link");
    link_dir(&real, &link);
    let r = delete_old(&link, "*", days(30), now, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Link, .. })), "{:?}", r.end);
    assert!(real.join("a.log").exists());
    let r = delete_old(Path::new(""), "*", days(30), now, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Empty, .. })));
    let r = delete_old(&t.path().join("missing"), "*", days(30), now, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Io { doing: Doing::List, .. })));
}

#[test]
fn delete_old_stops_on_the_flag() {
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    old_file(t.path(), "a.log", 40, now);
    let stop = no_stop();
    stop.store(true, Ordering::Relaxed);
    let r = delete_old(t.path(), "*", days(30), now, &stop, &mut |_| {});
    assert!(matches!(r.end, End::Stopped));
    assert!(t.path().join("a.log").exists());
}

#[cfg(windows)]
#[test]
fn delete_old_deletes_a_read_only_file_and_stops_at_a_locked_one() {
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    old_file(t.path(), "a.log", 40, now);
    let mut perm = fs::metadata(t.path().join("a.log")).unwrap().permissions();
    perm.set_readonly(true);
    fs::set_permissions(t.path().join("a.log"), perm).unwrap();
    old_file(t.path(), "b.log", 40, now);
    old_file(t.path(), "c.log", 40, now);
    let held = lock(&t.path().join("b.log"));
    let r = delete_old(t.path(), "*.log", days(30), now, &no_stop(), &mut |_| {});
    assert_eq!(r.paths, [t.path().join("a.log")], "the read-only file went");
    match &r.end {
        End::Failed(e @ Error::Io { doing: Doing::Delete, .. }) => assert_eq!(e.path(), t.path().join("b.log")),
        other => panic!("{other:?}"),
    }
    assert!(t.path().join("c.log").exists(), "the first failure stops the run");
    drop(held);
}

#[cfg(windows)]
#[test]
fn a_read_only_file_that_cannot_be_deleted_stays_read_only() {
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    old_file(t.path(), "a.log", 40, now);
    let p = t.path().join("a.log");
    set_read_only(&p, true);
    let held = lock_for_delete(&p);
    let r = delete_old(t.path(), "*.log", days(30), now, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Io { doing: Doing::Delete, .. })), "{:?}", r.end);
    drop(held);
    assert!(is_read_only(&p), "READONLY was put back");
    set_read_only(&p, false);
}

#[cfg(windows)]
#[test]
fn delete_old_names_with_unpaired_surrogates() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let name = OsString::from_wide(&[0x61, 0xD800, 0x2E, 0x74, 0x78, 0x74]);
    let p = t.path().join(&name);
    write_os(&p);
    set_mtime(&p, now - days(40));
    let (events, mut on) = log();
    let r = delete_old(t.path(), "*.txt", days(30), now, &no_stop(), &mut on);
    assert!(r.paths.is_empty() && p.exists(), "skipped by *.txt");
    assert!(events.borrow().iter().any(|e| e.contains("NotUnicode")));
    let r = delete_old(t.path(), "*", days(30), now, &no_stop(), &mut |_| {});
    assert_eq!(r.paths.len(), 1);
    assert!(!p.exists(), "matched by *");
}

#[cfg(windows)]
fn write_os(p: &Path) {
    fs::write(p, "x").unwrap();
}

#[cfg(unix)]
#[test]
fn delete_old_stops_at_the_first_failure_in_a_folder_without_write_permission() {
    use std::os::unix::fs::PermissionsExt;
    /// Puts the folder's mode back, also when an assert fails, so the temp folder can be removed.
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
    let t = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    let dir = t.path().join("d");
    old_file(&dir, "a.log", 40, now);
    old_file(&dir, "b.log", 40, now);
    let _guard = Restore(dir.clone());
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    if fs::write(dir.join("probe"), "x").is_ok() {
        eprintln!("note: running as root, a 0o555 folder still accepts files; test skipped");
        return;
    }
    let r = delete_old(&dir, "*", days(30), now, &no_stop(), &mut |_| {});
    assert!(r.paths.is_empty());
    match &r.end {
        End::Failed(e @ Error::Io { doing: Doing::Delete, .. }) => assert_eq!(e.path(), dir.join("a.log")),
        other => panic!("{other:?}"),
    }
}

// ---- empty ----

#[test]
fn empty_removes_everything_and_counts_top_level_items() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("d");
    write(&dir.join("a.txt"), "a");
    write(&dir.join(".hidden"), "h");
    write(&dir.join("sub/deep/x.txt"), "x");
    write(&dir.join("sub/y.txt"), "y");
    fs::create_dir_all(dir.join("emptydir")).unwrap();
    #[cfg(windows)]
    {
        let ok = std::process::Command::new("attrib").arg("+h").arg("+s").arg(dir.join(".hidden")).status().unwrap().success();
        assert!(ok);
    }
    let (events, mut on) = log();
    let r = empty(&dir, &no_stop(), &mut on);
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!(r.removed, 4);
    assert_eq!(events.borrow().len(), 4, "top-level items only");
    assert!(dir.exists() && names(&dir).is_empty());
}

#[test]
fn empty_removes_a_link_as_a_link() {
    let t = tempfile::tempdir().unwrap();
    let outside = t.path().join("outside");
    write(&outside.join("keep.txt"), "keep");
    write(&outside.join("sub/keep2.txt"), "keep");
    let dir = t.path().join("d");
    fs::create_dir(&dir).unwrap();
    link_dir(&outside, &dir.join("link"));
    // A link nested below a folder too.
    fs::create_dir(dir.join("sub")).unwrap();
    link_dir(&outside, &dir.join("sub").join("link2"));
    let r = empty(&dir, &no_stop(), &mut |_| {});
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!(r.removed, 2);
    assert!(names(&dir).is_empty());
    assert_eq!(read(&outside.join("keep.txt")), "keep");
    assert_eq!(read(&outside.join("sub/keep2.txt")), "keep");
}

#[test]
fn empty_removes_a_file_symlink_as_a_link() {
    let t = tempfile::tempdir().unwrap();
    let target = t.path().join("target.txt");
    write(&target, "keep");
    let dir = t.path().join("d");
    fs::create_dir(&dir).unwrap();
    if !link_file(&target, &dir.join("l.txt")) {
        return;
    }
    let r = empty(&dir, &no_stop(), &mut |_| {});
    assert!(r.end.is_done() && r.removed == 1);
    assert_eq!(read(&target), "keep");
}

#[test]
fn empty_refuses_a_link_as_the_folder_and_a_missing_one() {
    let t = tempfile::tempdir().unwrap();
    let outside = t.path().join("outside");
    write(&outside.join("keep.txt"), "keep");
    let link = t.path().join("link");
    link_dir(&outside, &link);
    let r = empty(&link, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Link, .. })), "{:?}", r.end);
    assert_eq!(read(&outside.join("keep.txt")), "keep");
    let r = empty(Path::new(""), &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Empty, .. })));
    let r = empty(&t.path().join("nope"), &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Io { doing: Doing::List, .. })));
}

#[test]
fn empty_stops_on_the_flag() {
    let t = tempfile::tempdir().unwrap();
    write(&t.path().join("d/a.txt"), "a");
    let stop = no_stop();
    stop.store(true, Ordering::Relaxed);
    let r = empty(&t.path().join("d"), &stop, &mut |_| {});
    assert!(matches!(r.end, End::Stopped) && r.removed == 0);
}

#[cfg(windows)]
#[test]
fn empty_removes_read_only_files_nested_and_top_level() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("d");
    write(&dir.join("top.txt"), "t");
    write(&dir.join("sub/deep/ro.txt"), "r");
    for p in [dir.join("top.txt"), dir.join("sub/deep/ro.txt")] {
        let mut perm = fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(&p, perm).unwrap();
    }
    let r = empty(&dir, &no_stop(), &mut |_| {});
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!(r.removed, 2);
    assert!(names(&dir).is_empty());
}

#[cfg(windows)]
#[test]
fn empty_puts_read_only_back_on_what_it_could_not_delete() {
    for rel in ["top.txt", "sub/deep/ro.txt"] {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("d");
        write(&dir.join(rel), "r");
        let p = dir.join(rel);
        set_read_only(&p, true);
        let held = lock_for_delete(&p);
        let r = empty(&dir, &no_stop(), &mut |_| {});
        assert!(matches!(r.end, End::Failed(Error::Io { doing: Doing::Delete, .. })), "{rel}: {:?}", r.end);
        drop(held);
        assert!(is_read_only(&p), "{rel}: READONLY was put back");
        set_read_only(&p, false);
    }
}

#[cfg(windows)]
#[test]
fn empty_fails_with_the_path_of_a_locked_file_inside() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("d");
    write(&dir.join("sub/locked.txt"), "l");
    let held = lock(&dir.join("sub/locked.txt"));
    let r = empty(&dir, &no_stop(), &mut |_| {});
    match &r.end {
        End::Failed(e @ Error::Io { doing: Doing::Delete, .. }) => assert_eq!(e.path(), dir.join("sub/locked.txt")),
        other => panic!("{other:?}"),
    }
    assert_eq!(r.removed, 0);
    drop(held);
}
