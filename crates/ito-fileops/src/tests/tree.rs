use std::fs;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use super::*;
use crate::{copy_tree, Doing, End, Error, TreeOptions, TreeResult, Unsafe};

fn quick() -> TreeOptions {
    TreeOptions { retries: 1, wait: Duration::from_millis(50), time_tolerance: Duration::ZERO }
}

fn fixture(from: &Path) {
    let t0 = SystemTime::now() - days(3);
    for (rel, text) in [("a.txt", "alpha"), ("sub/b.txt", "bravo!"), ("sub/deep/c.log", "charlie")] {
        let p = from.join(rel);
        write(&p, text);
        set_mtime(&p, t0);
    }
    fs::create_dir_all(from.join("emptydir")).unwrap();
}

fn run(from: &Path, into: &Path, pattern: Option<&str>) -> TreeResult {
    copy_tree(from, into, pattern, &quick(), &no_stop(), &mut |_| {})
}

fn dirs() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let (from, into) = (t.path().join("from"), t.path().join("into"));
    fs::create_dir(&from).unwrap();
    (t, from, into)
}

#[test]
fn first_copy_then_second_run_is_all_same() {
    let (_t, from, into) = dirs();
    fixture(&from);
    let (events, mut on) = log();
    let r = copy_tree(&from, &into, None, &quick(), &no_stop(), &mut on);
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!((r.copied, r.same, r.folders_made, r.bytes), (3, 0, 3, 5 + 6 + 7));
    assert_eq!(r.robocopy_code(), 1);
    assert_eq!(read(&into.join("sub/deep/c.log")), "charlie");
    assert!(into.join("emptydir").is_dir(), "empty folders are made");
    assert_eq!(mtime(&into.join("a.txt")), mtime(&from.join("a.txt")), "the copy keeps the write time");
    assert!(events.borrow().iter().filter(|e| e.starts_with("Copied")).count() == 3);

    let r = run(&from, &into, None);
    assert_eq!((r.copied, r.same, r.folders_made), (0, 3, 0));
    assert_eq!(r.robocopy_code(), 0);
}

#[test]
fn a_changed_file_is_copied_again() {
    let (_t, from, into) = dirs();
    fixture(&from);
    run(&from, &into, None);
    write(&from.join("a.txt"), "alpha, changed");
    let r = run(&from, &into, None);
    assert_eq!((r.copied, r.same), (1, 2));
    assert_eq!(r.robocopy_code(), 1);
    assert_eq!(read(&into.join("a.txt")), "alpha, changed");
    // Same size, different time: copied too.
    let p = from.join("sub/b.txt");
    set_mtime(&p, SystemTime::now() - days(1));
    let r = run(&from, &into, None);
    assert_eq!(r.copied, 1);
}

#[test]
fn extras_are_counted_and_kept() {
    let (_t, from, into) = dirs();
    fixture(&from);
    run(&from, &into, None);
    write(&into.join("extra.txt"), "x");
    write(&into.join("extradir/inside.txt"), "x");
    let (events, mut on) = log();
    let r = copy_tree(&from, &into, None, &quick(), &no_stop(), &mut on);
    assert_eq!((r.extra_files, r.extra_dirs, r.copied), (1, 1, 0), "an extra folder is counted at its top");
    assert_eq!(r.robocopy_code(), 2);
    assert!(into.join("extra.txt").exists() && into.join("extradir/inside.txt").exists());
    let ev = events.borrow();
    assert!(ev.iter().any(|e| e.starts_with("ExtraFile")) && ev.iter().any(|e| e.starts_with("ExtraDir")));
}

#[test]
fn a_pattern_filters_files_but_not_folders() {
    let (_t, from, into) = dirs();
    fixture(&from);
    write(&into.join("stray.log"), "x");
    write(&into.join("stray.txt"), "x");
    let r = run(&from, &into, Some("*.txt"));
    assert_eq!((r.copied, r.extra_files), (2, 1), "only the .txt stray is an extra");
    assert!(!into.join("sub/deep/c.log").exists());
    assert!(into.join("sub/deep").is_dir() && into.join("emptydir").is_dir(), "folders are still made");
    assert_eq!(r.robocopy_code(), 3);
}

#[test]
fn a_file_against_a_folder_is_a_mismatch() {
    let (_t, from, into) = dirs();
    write(&from.join("x"), "file");
    fs::create_dir_all(from.join("y")).unwrap();
    write(&into.join("x/inner.txt"), "keep");
    write(&into.join("y"), "i am a file");
    let (events, mut on) = log();
    let r = copy_tree(&from, &into, None, &quick(), &no_stop(), &mut on);
    assert_eq!((r.mismatched, r.copied, r.failed), (2, 0, 0));
    assert_eq!(r.robocopy_code(), 4);
    assert_eq!(read(&into.join("x/inner.txt")), "keep");
    assert_eq!(read(&into.join("y")), "i am a file");
    assert_eq!(events.borrow().iter().filter(|e| e.starts_with("Mismatch")).count(), 2);
}

#[test]
fn a_link_inside_into_is_never_written_through() {
    let (t, from, into) = dirs();
    write(&from.join("sub/b.txt"), "b");
    write(&from.join("f.txt"), "f");
    let outside = t.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::create_dir(&into).unwrap();
    link_dir(&outside, &into.join("sub"));
    let r = run(&from, &into, None);
    assert_eq!((r.mismatched, r.copied), (1, 1));
    assert!(names(&outside).is_empty(), "nothing was written through the link");
    assert_eq!(r.robocopy_code(), 5);
}

#[test]
fn a_file_link_inside_into_is_not_written_through_either() {
    let (t, from, into) = dirs();
    write(&from.join("f.txt"), "new");
    let target = t.path().join("target.txt");
    write(&target, "old");
    fs::create_dir(&into).unwrap();
    if !link_file(&target, &into.join("f.txt")) {
        return;
    }
    let r = run(&from, &into, None);
    assert_eq!(r.mismatched, 1);
    assert_eq!(read(&target), "old");
}

#[test]
fn folder_links_in_the_source_are_not_followed_but_file_links_are_copied() {
    let (t, from, into) = dirs();
    let outside = t.path().join("outside");
    write(&outside.join("secret.txt"), "s");
    write(&from.join("real.txt"), "r");
    link_dir(&outside, &from.join("loop"));
    let (events, mut on) = log();
    let r = copy_tree(&from, &into, None, &quick(), &no_stop(), &mut on);
    assert_eq!((r.copied, r.skipped), (1, 1));
    assert!(!into.join("loop").exists());
    assert!(events.borrow().iter().any(|e| e.contains("Skipped") && e.contains("Link")));
    assert_eq!(read(&outside.join("secret.txt")), "s");

    let target = t.path().join("target.txt");
    write(&target, "content");
    if link_file(&target, &from.join("flink.txt")) {
        let r = run(&from, &into, None);
        assert_eq!(r.copied, 1);
        assert_eq!(read(&into.join("flink.txt")), "content", "a file link is copied as its content");
        assert!(!fs::symlink_metadata(into.join("flink.txt")).unwrap().file_type().is_symlink());
    }
}

#[test]
fn roots_that_overlap_or_are_empty_are_refused_before_anything_is_made() {
    let (t, from, _into) = dirs();
    fixture(&from);
    let cases = [
        (from.clone(), from.clone()),
        (from.clone(), from.join("sub").join("new")),
        (from.join("sub"), from.clone()),
        (from.clone(), from.join("sub").join("..")),
    ];
    for (a, b) in cases {
        let r = run(&a, &b, None);
        assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Overlap, .. })), "{a:?} -> {b:?}: {:?}", r.end);
        assert_eq!(r.robocopy_code(), 16);
    }
    assert!(!from.join("sub/new").exists());
    let r = run(Path::new(""), &t.path().join("x"), None);
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Empty, .. })));
    let r = run(&from, Path::new(""), None);
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Empty, .. })));
    // A sibling whose name starts the same is not an overlap.
    let sibling = t.path().join("from2");
    let r = run(&from, &sibling, None);
    assert!(r.end.is_done(), "{:?}", r.end);
}

#[test]
fn a_missing_or_file_source_is_fatal() {
    let (t, from, into) = dirs();
    let r = run(&t.path().join("nope"), &into, None);
    assert!(matches!(r.end, End::Failed(Error::Io { doing: Doing::List, .. })));
    assert_eq!(r.robocopy_code(), 16);
    write(&from.join("f"), "x");
    let r = run(&from.join("f"), &into, None);
    assert_eq!(r.robocopy_code(), 16);
    assert!(!into.exists());
}

#[test]
fn time_tolerance_just_inside_and_just_outside() {
    let (_t, from, into) = dirs();
    let base = SystemTime::now() - days(2);
    write(&from.join("a.txt"), "same size");
    set_mtime(&from.join("a.txt"), base);
    let two = Duration::from_secs(2);
    let opts = |tol| TreeOptions { time_tolerance: tol, ..quick() };
    for (offset, tol, same) in [
        (Duration::from_secs(1), two, true),
        (two, two, true),
        (two + Duration::from_millis(1), two, false),
        (Duration::from_secs(1), Duration::ZERO, false),
    ] {
        write(&into.join("a.txt"), "other siz");
        set_mtime(&into.join("a.txt"), base + offset);
        let r = copy_tree(&from, &into, None, &opts(tol), &no_stop(), &mut |_| {});
        assert_eq!((r.same, r.copied), if same { (1, 0) } else { (0, 1) }, "offset {offset:?}, tolerance {tol:?}");
    }
}

#[test]
fn a_stopped_copy_says_so() {
    let (_t, from, into) = dirs();
    fixture(&from);
    let stop = no_stop();
    stop.store(true, Ordering::Relaxed);
    let r = copy_tree(&from, &into, None, &quick(), &stop, &mut |_| {});
    assert!(matches!(r.end, End::Stopped));
    assert_eq!(r.copied, 0);
    assert_eq!(r.robocopy_code(), 0, "stopping adds no bit");
}

#[test]
fn stopping_after_the_first_file_leaves_the_rest() {
    let (_t, from, into) = dirs();
    fixture(&from);
    let stop = no_stop();
    let r = copy_tree(&from, &into, None, &quick(), &stop, &mut |e| {
        if matches!(e, crate::Event::Copied { .. }) {
            stop.store(true, Ordering::Relaxed);
        }
    });
    assert!(matches!(r.end, End::Stopped));
    assert_eq!(r.copied, 1);
}

#[test]
fn a_folder_whose_attributes_fail_is_a_failure_of_that_folder() {
    let (_t, from, into) = dirs();
    write(&from.join("sub/b.txt"), "b");
    crate::plat::hook::FAIL_DIR_ATTRIBUTES.with(|c| c.set(true));
    let (events, mut on) = log();
    let r = copy_tree(&from, &into, None, &quick(), &no_stop(), &mut on);
    crate::plat::hook::FAIL_DIR_ATTRIBUTES.with(|c| c.set(false));
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!((r.failed, r.copied, r.folders_made), (1, 1, 1));
    assert_eq!(r.robocopy_code(), 1 | 8);
    let ev = events.borrow();
    assert!(ev.iter().any(|e| e.starts_with("Failed")), "{ev:?}");
    assert!(!ev.iter().any(|e| e.starts_with("CleanupFailed")), "{ev:?}");
}

#[test]
fn a_destination_that_is_not_a_plain_file_is_found_right_before_writing() {
    let (t, _from, _into) = dirs();
    let file = t.path().join("f");
    write(&file, "x");
    assert!(!crate::plat::exists_not_regular(&file));
    let link = t.path().join("l");
    link_dir(t.path(), &link);
    assert!(crate::plat::exists_not_regular(&link));
    assert!(crate::plat::exists_not_regular(t.path()));
    assert!(!crate::plat::exists_not_regular(&t.path().join("nope")));
}

#[test]
fn a_destination_replaced_by_a_link_after_the_listing_is_not_written_through() {
    let (t, from, into) = dirs();
    write(&from.join("a.txt"), "new");
    write(&into.join("a.txt"), "old");
    // Same size: without a different time the two would be "the same" where file times are coarse (Linux).
    set_mtime(&from.join("a.txt"), SystemTime::now() - days(2));
    let outside = t.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let out2 = outside.clone();
    crate::tree::hook::BEFORE_WRITE.with(|h| {
        *h.borrow_mut() = Some(Box::new(move |dst: &Path| {
            fs::remove_file(dst).unwrap();
            link_dir(&out2, dst);
        }));
    });
    let (events, mut on) = log();
    let r = copy_tree(&from, &into, None, &quick(), &no_stop(), &mut on);
    crate::tree::hook::BEFORE_WRITE.with(|h| *h.borrow_mut() = None);
    assert_eq!((r.mismatched, r.copied, r.failed), (1, 0, 0), "{:?}", r.end);
    assert_eq!(r.robocopy_code(), 4);
    assert!(events.borrow().iter().any(|e| e.starts_with("Mismatch")));
    assert!(names(&outside).is_empty());
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    #[test]
    fn a_locked_source_is_retried_then_failed_and_the_copy_goes_on() {
        let (_t, from, into) = dirs();
        fixture(&from);
        let held = lock(&from.join("a.txt"));
        let (events, mut on) = log();
        let r = copy_tree(&from, &into, None, &quick(), &no_stop(), &mut on);
        assert!(r.end.is_done(), "{:?}", r.end);
        assert_eq!((r.failed, r.copied), (1, 2));
        assert_eq!(r.robocopy_code(), 9);
        let ev = events.borrow();
        assert_eq!(ev.iter().filter(|e| e.starts_with("Retrying")).count(), 1, "{ev:?}");
        assert!(ev.iter().any(|e| e.starts_with("Failed")));
        drop(held);
        drop(ev);
        let r = run(&from, &into, None);
        assert_eq!((r.copied, r.same, r.failed), (1, 2, 0), "the next run copies it");
    }

    #[test]
    fn a_retry_that_works_is_a_copy() {
        let (_t, from, into) = dirs();
        write(&from.join("a.txt"), "alpha");
        let mut held = Some(lock(&from.join("a.txt")));
        let opts = TreeOptions { retries: 1, wait: Duration::from_millis(300), time_tolerance: Duration::ZERO };
        let mut retrying = 0;
        let r = copy_tree(&from, &into, None, &opts, &no_stop(), &mut |e| {
            if matches!(e, crate::Event::Retrying { .. }) {
                retrying += 1;
                held.take(); // released as the retry is announced: no timing involved
            }
        });
        assert_eq!(retrying, 1);
        assert_eq!((r.copied, r.failed), (1, 0));
        assert_eq!(r.robocopy_code(), 1);
    }

    #[test]
    fn stopping_during_a_retry_wait_returns_quickly() {
        let (_t, from, into) = dirs();
        write(&from.join("a.txt"), "alpha");
        let _held = lock(&from.join("a.txt"));
        let opts = TreeOptions { retries: 1, wait: Duration::from_secs(30), time_tolerance: Duration::ZERO };
        let stop = std::sync::Arc::new(no_stop());
        let s2 = stop.clone();
        let setter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            s2.store(true, Ordering::Relaxed);
        });
        let started = std::time::Instant::now();
        let r = copy_tree(&from, &into, None, &opts, &stop, &mut |_| {});
        setter.join().unwrap();
        assert!(matches!(r.end, End::Stopped));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_read_only_destination_is_replaced_and_the_copy_gets_the_source_times() {
        let (_t, from, into) = dirs();
        write(&from.join("a.txt"), "new content");
        set_mtime(&from.join("a.txt"), SystemTime::now() - days(2));
        write(&into.join("a.txt"), "old");
        let mut perm = fs::metadata(into.join("a.txt")).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(into.join("a.txt"), perm).unwrap();
        let r = run(&from, &into, None);
        assert_eq!((r.copied, r.failed), (1, 0), "{:?}", r.end);
        assert_eq!(read(&into.join("a.txt")), "new content");
        assert_eq!(mtime(&into.join("a.txt")), mtime(&from.join("a.txt")));
        assert_eq!(run(&from, &into, None).same, 1);
    }

    #[test]
    fn a_read_only_source_file_is_copied_with_its_times() {
        let (_t, from, into) = dirs();
        write(&from.join("ro.txt"), "ro");
        set_mtime(&from.join("ro.txt"), SystemTime::now() - days(2));
        let mut perm = fs::metadata(from.join("ro.txt")).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(from.join("ro.txt"), perm).unwrap();
        let r = run(&from, &into, None);
        assert_eq!((r.copied, r.failed), (1, 0), "{:?}", r.end);
        assert!(fs::metadata(into.join("ro.txt")).unwrap().permissions().readonly());
        assert_eq!(mtime(&into.join("ro.txt")), mtime(&from.join("ro.txt")));
        assert_eq!(run(&from, &into, None).same, 1);
        // Leave nothing read-only behind for the cleanup.
        for p in [from.join("ro.txt"), into.join("ro.txt")] {
            let mut perm = fs::metadata(&p).unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            fs::set_permissions(&p, perm).unwrap();
        }
    }

    #[test]
    fn names_differing_only_in_case_are_the_same_name() {
        let (_t, from, into) = dirs();
        write(&from.join("a.txt"), "alpha");
        write(&into.join("A.TXT"), "other");
        let r = run(&from, &into, None);
        assert_eq!(r.extra_files, 0);
    }

    #[test]
    fn a_name_with_an_unpaired_surrogate_goes_with_star_and_is_skipped_by_a_pattern() {
        let (_t, from, into) = dirs();
        let name = OsString::from_wide(&[0x61, 0xD800, 0x2E, 0x74, 0x78, 0x74]);
        fs::write(from.join(&name), "x").unwrap();
        let r = run(&from, &into, Some("*.txt"));
        assert_eq!((r.copied, r.skipped), (0, 1));
        let r = run(&from, &into, Some("*"));
        assert_eq!(r.copied, 1);
        assert!(into.join(&name).exists());
    }

    #[test]
    fn two_different_non_unicode_names_are_not_the_same_name() {
        // Lossy conversion would turn both into the same text.
        let a = OsString::from_wide(&[0x61, 0xD800, 0x2E, 0x74, 0x78, 0x74]);
        let b = OsString::from_wide(&[0x61, 0xD801, 0x2E, 0x74, 0x78, 0x74]);
        assert_ne!(super::super::super::tree::key(&a), super::super::super::tree::key(&b));
        assert_eq!(super::super::super::tree::key(std::ffi::OsStr::new("AbC")), OsString::from("abc"));
        // A folder with one of the names must not block a file with the other.
        let (_t, from, into) = dirs();
        fs::write(from.join(&a), "x").unwrap();
        fs::create_dir_all(into.join(&b)).unwrap();
        let r = run(&from, &into, None);
        assert_eq!((r.copied, r.mismatched, r.extra_dirs), (1, 0, 1), "{:?}", r.end);
        assert!(into.join(&a).is_file());
    }

    #[test]
    fn a_junction_is_not_a_destination_and_is_not_cleared_through() {
        let (t, _from, _into) = dirs();
        let target = t.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = t.path().join("link");
        link_dir(&target, &link);
        assert!(crate::plat::exists_not_regular(&link));
        assert!(crate::plat::exists_not_regular(&target));
        assert!(!crate::plat::exists_not_regular(&t.path().join("missing")));
        let mut perm = fs::metadata(&target).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(&target, perm).unwrap();
        assert!(crate::plat::clear_dst_readonly(&link).unwrap().is_none());
        assert!(fs::metadata(&target).unwrap().permissions().readonly(), "the link target was not touched");
        let mut perm = fs::metadata(&target).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        fs::set_permissions(&target, perm).unwrap();
    }

    #[test]
    fn the_os_error_of_setting_folder_attributes_is_the_win32_code() {
        let (t, _from, _into) = dirs();
        let src = t.path().join("hidden");
        fs::create_dir(&src).unwrap();
        assert!(std::process::Command::new("attrib").arg("+h").arg(&src).status().unwrap().success());
        let meta = fs::metadata(&src).unwrap();
        let e = crate::plat::set_dir_attributes(&t.path().join("missing"), &meta).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(2), "ERROR_FILE_NOT_FOUND, not an HRESULT: {e:?}");
    }

    #[test]
    fn a_hidden_folder_keeps_its_attributes() {
        let (_t, from, into) = dirs();
        write(&from.join("h/x.txt"), "x");
        assert!(std::process::Command::new("attrib").arg("+h").arg(from.join("h")).status().unwrap().success());
        run(&from, &into, None);
        use std::os::windows::fs::MetadataExt;
        let attrs = fs::metadata(into.join("h")).unwrap().file_attributes();
        assert!(attrs & 0x2 != 0, "hidden attribute copied: {attrs:#x}");
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn permission_bits_and_times_are_kept_and_a_fifo_is_skipped() {
        let (_t, from, into) = dirs();
        write(&from.join("run.sh"), "#!/bin/sh");
        fs::set_permissions(from.join("run.sh"), fs::Permissions::from_mode(0o750)).unwrap();
        set_mtime(&from.join("run.sh"), SystemTime::now() - days(2));
        let made = std::process::Command::new("mkfifo").arg(from.join("pipe")).status().map(|s| s.success()).unwrap_or(false);
        let r = run(&from, &into, None);
        assert_eq!(r.copied, 1);
        assert_eq!(r.skipped, if made { 1 } else { 0 });
        assert_eq!(fs::metadata(into.join("run.sh")).unwrap().permissions().mode() & 0o777, 0o750);
        assert_eq!(mtime(&into.join("run.sh")), mtime(&from.join("run.sh")));
        assert_eq!(run(&from, &into, None).same, 1);
    }

    #[test]
    fn a_read_only_file_is_copied_again_when_the_source_changes() {
        let (_t, from, into) = dirs();
        write(&from.join("ro.txt"), "one");
        fs::set_permissions(from.join("ro.txt"), fs::Permissions::from_mode(0o444)).unwrap();
        if fs::OpenOptions::new().write(true).open(from.join("ro.txt")).is_ok() {
            eprintln!("note: running as root, a 0o444 file can still be written; test skipped");
            return;
        }
        let r = run(&from, &into, None);
        assert_eq!((r.copied, r.failed), (1, 0));
        assert_eq!(fs::metadata(into.join("ro.txt")).unwrap().permissions().mode() & 0o777, 0o444);
        // Change the source: content and a newer time, still 0o444.
        fs::set_permissions(from.join("ro.txt"), fs::Permissions::from_mode(0o644)).unwrap();
        write(&from.join("ro.txt"), "two, longer");
        set_mtime(&from.join("ro.txt"), SystemTime::now() + Duration::from_secs(60));
        fs::set_permissions(from.join("ro.txt"), fs::Permissions::from_mode(0o444)).unwrap();
        let r = run(&from, &into, None);
        assert_eq!((r.copied, r.failed), (1, 0), "{:?}", r.end);
        assert_eq!(read(&into.join("ro.txt")), "two, longer");
        assert_eq!(fs::metadata(into.join("ro.txt")).unwrap().permissions().mode() & 0o777, 0o444);
    }

    #[test]
    fn newlines_and_quotes_in_names() {
        let (_t, from, into) = dirs();
        write(&from.join("a\nb \"q\".txt"), "x");
        let r = run(&from, &into, None);
        assert_eq!(r.copied, 1);
        assert!(into.join("a\nb \"q\".txt").exists());
    }
}

#[test]
fn spaces_and_non_ascii_in_names() {
    let (_t, from, into) = dirs();
    write(&from.join("my folder/ファイル 1.txt"), "x");
    write(&from.join("it's.txt"), "y");
    let r = run(&from, &into, None);
    assert_eq!(r.copied, 2);
    assert!(into.join("my folder/ファイル 1.txt").exists());
}
