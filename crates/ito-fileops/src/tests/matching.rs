use std::fs;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use super::*;
use crate::matching::hook;
use crate::{copy_matching, End, Error, Unsafe};

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn dirs() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let (from, into) = (t.path().join("from"), t.path().join("into"));
    fs::create_dir(&from).unwrap();
    (t, from, into)
}

#[test]
fn a_wildcard_copies_files_only_and_keeps_the_write_time() {
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "abc");
    write(&from.join("b.txt"), "second");
    write(&from.join("c.log"), "no");
    fs::create_dir(from.join("d.txt")).unwrap();
    let old = SystemTime::now() - days(5);
    set_mtime(&from.join("a.txt"), old);
    let (events, mut on) = log();
    let r = copy_matching(&from.join("*.txt"), &into, false, &no_stop(), &mut on);
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!(r.found, 2);
    let got: Vec<_> = r.sent.iter().map(|s| (s.name.to_string_lossy().into_owned(), s.bytes, s.sha256.clone())).collect();
    assert_eq!(got, [("a.txt".to_string(), 3, None), ("b.txt".to_string(), 6, None)]);
    assert_eq!(names(&into), ["a.txt", "b.txt"], "a folder is not copied, no staging file is left");
    assert_eq!(mtime(&into.join("a.txt")), old);
    assert_eq!(events.borrow().iter().filter(|e| e.starts_with("Sent")).count(), 2);
}

#[test]
fn without_a_wildcard_it_is_that_one_file() {
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "abc");
    write(&from.join("b.txt"), "x");
    let r = copy_matching(&from.join("a.txt"), &into, true, &no_stop(), &mut |_| {});
    assert_eq!((r.found, r.sent.len()), (1, 1));
    assert_eq!(names(&into), ["a.txt"]);
    let r = copy_matching(&from.join("missing.txt"), &into, true, &no_stop(), &mut |_| {});
    assert!(r.end.is_done() && r.found == 0);
}

#[test]
fn no_match_is_its_own_result_and_into_is_made() {
    let (_t, from, into) = dirs();
    write(&from.join("a.log"), "x");
    let nested = into.join("deep/er");
    let r = copy_matching(&from.join("*.txt"), &nested, true, &no_stop(), &mut |_| {});
    assert!(r.end.is_done());
    assert_eq!((r.found, r.sent.len()), (0, 0));
    assert!(nested.is_dir());
}

#[test]
fn verify_gives_the_hash_and_leaves_no_staging_file() {
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "abc");
    let r = copy_matching(&from.join("*"), &into, true, &no_stop(), &mut |_| {});
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!(r.sent[0].sha256.as_deref(), Some(ABC_SHA256));
    assert_eq!(names(&into), ["a.txt"]);
    assert_eq!(read(&into.join("a.txt")), "abc");
    // A bigger file than one buffer.
    let big = "0123456789abcdef".repeat(20_000);
    write(&from.join("big.bin"), &big);
    let r = copy_matching(&from.join("big.bin"), &into, true, &no_stop(), &mut |_| {});
    assert_eq!(r.sent[0].bytes, big.len() as u64);
    assert_eq!(read(&into.join("big.bin")), big);
}

#[test]
fn without_verify_the_target_is_overwritten_through_staging() {
    let (t, from, into) = dirs();
    write(&from.join("a.txt"), "new");
    write(&into.join("a.txt"), "old and longer");
    fs::hard_link(into.join("a.txt"), t.path().join("old-name")).unwrap();
    let r = copy_matching(&from.join("a.txt"), &into, false, &no_stop(), &mut |_| {});
    assert!(r.end.is_done());
    assert_eq!(read(&into.join("a.txt")), "new");
    assert_eq!(read(&t.path().join("old-name")), "old and longer", "a new file was renamed in, the old data is untouched");
    assert_eq!(r.sent[0].sha256, None);
    assert_eq!(names(&into), ["a.txt"]);
}

#[test]
fn a_corrupted_staging_file_is_a_mismatch_with_both_hashes_and_the_target_survives() {
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "abc");
    write(&into.join("a.txt"), "previous");
    hook::CORRUPT_STAGING.with(|c| c.set(true));
    let r = copy_matching(&from.join("a.txt"), &into, true, &no_stop(), &mut |_| {});
    hook::CORRUPT_STAGING.with(|c| c.set(false));
    match &r.end {
        End::Failed(Error::Mismatch { from, to, path }) => {
            assert_eq!(from, ABC_SHA256);
            assert_ne!(to, from);
            assert_eq!(to.len(), 64);
            assert_eq!(path, &into.join("a.txt"));
        }
        other => panic!("{other:?}"),
    }
    assert!(r.sent.is_empty());
    assert_eq!(names(&into), ["a.txt"], "no staging file left");
    assert_eq!(read(&into.join("a.txt")), "previous", "the target is never deleted");
}

#[test]
fn a_source_that_changes_during_the_copy_is_reported() {
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "abc");
    hook::CHANGE_SOURCE.with(|c| c.set(true));
    let r = copy_matching(&from.join("a.txt"), &into, true, &no_stop(), &mut |_| {});
    hook::CHANGE_SOURCE.with(|c| c.set(false));
    assert!(matches!(r.end, End::Failed(Error::SourceChanged { .. })), "{:?}", r.end);
    assert!(names(&into).is_empty(), "staging removed, no target");
}

#[test]
fn an_existing_part_file_is_left_alone() {
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "abc");
    write(&into.join("a.txt.part"), "someone else's");
    write(&into.join(".a.txt.0123456789abcdef.part"), "someone else's too");
    let r = copy_matching(&from.join("a.txt"), &into, true, &no_stop(), &mut |_| {});
    assert!(r.end.is_done());
    assert_eq!(read(&into.join("a.txt.part")), "someone else's");
    assert_eq!(read(&into.join(".a.txt.0123456789abcdef.part")), "someone else's too");
    assert_eq!(names(&into).len(), 3);
}

#[test]
fn copying_into_the_source_folder_is_refused_as_the_same_file() {
    let (_t, from, _into) = dirs();
    write(&from.join("a.part"), "x");
    write(&from.join("b.part"), "y");
    let r = copy_matching(&from.join("*.part"), &from, false, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::SameFile, .. })), "{:?}", r.end);
    assert_eq!(names(&from), ["a.part", "b.part"]);
}

#[test]
fn a_staging_like_file_in_into_is_not_a_source_and_stays() {
    let (_t, from, into) = dirs();
    write(&from.join("a.part"), "x");
    write(&from.join("b.part"), "y");
    write(&into.join(".a.part.0123456789abcdef.part"), "someone else's");
    let r = copy_matching(&from.join("*.part"), &into, true, &no_stop(), &mut |_| {});
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!((r.found, r.sent.len()), (2, 2));
    assert_eq!(names(&into), [".a.part.0123456789abcdef.part", "a.part", "b.part"]);
    assert_eq!(read(&into.join(".a.part.0123456789abcdef.part")), "someone else's");
}

#[test]
fn stopping_between_files_leaves_no_staging_file() {
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "a");
    write(&from.join("b.txt"), "b");
    let stop = no_stop();
    let r = copy_matching(&from.join("*.txt"), &into, true, &stop, &mut |e| {
        if matches!(e, crate::Event::Sent(_)) {
            stop.store(true, Ordering::Relaxed);
        }
    });
    assert!(matches!(r.end, End::Stopped));
    assert_eq!(r.sent.len(), 1);
    assert_eq!(names(&into), ["a.txt"]);
}

#[test]
fn the_same_file_as_source_and_destination_is_refused() {
    let (_t, from, _into) = dirs();
    write(&from.join("a.txt"), "keep me");
    let r = copy_matching(&from.join("a.txt"), &from, true, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::SameFile, .. })), "{:?}", r.end);
    assert_eq!(read(&from.join("a.txt")), "keep me");
    assert_eq!(names(&from), ["a.txt"]);
}

#[test]
fn a_link_as_the_destination_is_refused_and_untouched() {
    let (t, from, into) = dirs();
    write(&from.join("a.txt"), "new");
    let outside = t.path().join("outside");
    write(&outside.join("keep.txt"), "keep");
    fs::create_dir(&into).unwrap();
    link_dir(&outside, &into.join("a.txt"));
    let r = copy_matching(&from.join("a.txt"), &into, false, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Link, .. })), "{:?}", r.end);
    assert_eq!(names(&outside), ["keep.txt"]);
    assert_eq!(read(&outside.join("keep.txt")), "keep");
    assert_eq!(names(&into), ["a.txt"]);
}

#[test]
fn a_file_link_as_the_destination_is_refused_too() {
    let (t, from, into) = dirs();
    write(&from.join("a.txt"), "new");
    let target = t.path().join("target.txt");
    write(&target, "old");
    fs::create_dir(&into).unwrap();
    if !link_file(&target, &into.join("a.txt")) {
        return;
    }
    let r = copy_matching(&from.join("a.txt"), &into, true, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Link, .. })));
    assert_eq!(read(&target), "old");
}

#[test]
fn a_source_file_link_is_copied_as_its_content() {
    let (t, from, into) = dirs();
    let target = t.path().join("target.txt");
    write(&target, "abc");
    if !link_file(&target, &from.join("l.txt")) {
        return;
    }
    let r = copy_matching(&from.join("*.txt"), &into, true, &no_stop(), &mut |_| {});
    assert_eq!(r.sent[0].sha256.as_deref(), Some(ABC_SHA256));
    assert!(!fs::symlink_metadata(into.join("l.txt")).unwrap().file_type().is_symlink());
}

#[test]
fn empty_paths_are_refused() {
    let (_t, from, _into) = dirs();
    let r = copy_matching(Path::new(""), &from, false, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Empty, .. })));
    let r = copy_matching(&from.join("*"), Path::new(""), false, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Unsafe { why: Unsafe::Empty, .. })));
}

#[test]
fn a_missing_source_folder_is_an_error_not_a_no_match() {
    let (t, _from, into) = dirs();
    let r = copy_matching(&t.path().join("gone").join("*.txt"), &into, false, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Io { doing: crate::Doing::List, .. })), "{:?}", r.end);
}

#[test]
fn a_missing_source_folder_without_a_wildcard_fails_the_same_way_and_makes_no_into() {
    let (t, from, into) = dirs();
    for src in [t.path().join("gone").join("a.txt"), t.path().join("gone").join("*.txt")] {
        let r = copy_matching(&src, &into, false, &no_stop(), &mut |_| {});
        assert!(matches!(r.end, End::Failed(Error::Io { doing: crate::Doing::List, .. })), "{src:?}: {:?}", r.end);
        assert!(!into.exists(), "{src:?}: into was made");
    }
    // The folder is a file.
    write(&from.join("f"), "x");
    let r = copy_matching(&from.join("f").join("a.txt"), &into, false, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Io { doing: crate::Doing::List, .. })), "{:?}", r.end);
    assert!(!into.exists());
    // An existing folder without that file is "no match".
    let r = copy_matching(&from.join("a.txt"), &into, false, &no_stop(), &mut |_| {});
    assert!(r.end.is_done() && r.found == 0);
    assert!(into.is_dir());
}

fn big() -> String {
    "0123456789abcdef".repeat(10_000) // 160,000 bytes: three chunks
}

/// Runs a copy that sets the stop flag at `point` and checks nothing was committed.
fn stopped_at(point: u8, verify: bool, old: Option<&str>) {
    let (_t, from, into) = dirs();
    write(&from.join("a.bin"), &big());
    if let Some(o) = old {
        write(&into.join("a.bin"), o);
    }
    let stop = no_stop();
    hook::STOP_AT.with(|c| c.set(point));
    let r = copy_matching(&from.join("a.bin"), &into, verify, &stop, &mut |_| {});
    hook::STOP_AT.with(|c| c.set(0));
    assert!(matches!(r.end, End::Stopped), "point {point}: {:?}", r.end);
    assert!(r.sent.is_empty());
    match old {
        Some(o) => assert_eq!((names(&into), read(&into.join("a.bin"))), (vec!["a.bin".to_string()], o.to_string())),
        None => assert!(names(&into).is_empty(), "point {point}: {:?}", names(&into)),
    }
}

#[test]
fn a_stop_inside_the_only_big_file_is_a_stop_and_commits_nothing() {
    stopped_at(1, false, None);
    stopped_at(1, true, Some("previous"));
}

#[test]
fn a_stop_during_the_reread_or_before_the_rename_commits_nothing() {
    stopped_at(2, true, Some("previous"));
    stopped_at(3, true, None);
    stopped_at(3, false, Some("previous"));
}

#[test]
fn spaces_quotes_and_non_ascii() {
    let (_t, from, into) = dirs();
    write(&from.join("名前 with 'quote'.txt"), "abc");
    let r = copy_matching(&from.join("*.txt"), &into, true, &no_stop(), &mut |_| {});
    assert_eq!(r.sent.len(), 1);
    assert!(into.join("名前 with 'quote'.txt").exists());
}

#[cfg(windows)]
#[test]
fn a_read_only_target_is_replaced_and_names_with_unpaired_surrogates_follow_the_rules() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "new");
    write(&into.join("a.txt"), "old");
    let mut perm = fs::metadata(into.join("a.txt")).unwrap().permissions();
    perm.set_readonly(true);
    fs::set_permissions(into.join("a.txt"), perm).unwrap();
    let r = copy_matching(&from.join("a.txt"), &into, true, &no_stop(), &mut |_| {});
    assert!(r.end.is_done(), "{:?}", r.end);
    assert_eq!(read(&into.join("a.txt")), "new");

    let odd = OsString::from_wide(&[0x62, 0xD800, 0x2E, 0x74, 0x78, 0x74]);
    fs::write(from.join(&odd), "z").unwrap();
    let (events, mut on) = log();
    let r = copy_matching(&from.join("b*.txt"), &into, false, &no_stop(), &mut on);
    assert_eq!(r.found, 0, "a name that is not Unicode is skipped by a pattern");
    assert!(events.borrow().iter().any(|e| e.contains("NotUnicode")));
    let r = copy_matching(&from.join("*"), &into, false, &no_stop(), &mut |_| {});
    assert_eq!(r.found, 2);
    assert!(into.join(&odd).exists());
}

#[cfg(windows)]
#[test]
fn a_failed_rename_leaves_a_read_only_target_read_only_and_no_staging_file() {
    let (_t, from, into) = dirs();
    write(&from.join("a.txt"), "new");
    write(&into.join("a.txt"), "old");
    set_read_only(&into.join("a.txt"), true);
    let held = lock_for_delete(&into.join("a.txt"));
    let r = copy_matching(&from.join("a.txt"), &into, false, &no_stop(), &mut |_| {});
    assert!(matches!(r.end, End::Failed(Error::Io { doing: crate::Doing::Rename, .. })), "{:?}", r.end);
    drop(held);
    assert!(is_read_only(&into.join("a.txt")), "READONLY was put back");
    assert_eq!(read(&into.join("a.txt")), "old");
    assert_eq!(names(&into), ["a.txt"], "no staging file left");
    set_read_only(&into.join("a.txt"), false);
}
