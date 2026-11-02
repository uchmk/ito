use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::fake::FakeS3;
use crate::*;

fn opts() -> Options {
    Options { timeout: Duration::from_secs(60), request_timeout: Duration::from_secs(20), part_size: 0, checksum: true }
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

fn bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

fn count(f: &FakeS3, needle: &str) -> usize {
    f.requests().iter().filter(|r| r.contains(needle)).count()
}

#[test]
fn small_file_goes_in_one_request_and_comes_back() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.bin");
    std::fs::write(&src, bytes(300_000)).unwrap();
    let mut seen = Vec::new();
    let sent = put(&f.target(), &src, "dir/a b.bin", &opts(), &never(), &mut |d, t| seen.push((d, t))).unwrap();
    assert_eq!((sent.bytes, sent.verified), (300_000, true));
    assert_eq!(sent.sha256, crate::sign::sha256_hex(&bytes(300_000)));
    assert!(f.checksum("dir/a b.bin").is_some());
    assert_eq!(seen, [(300_000, 300_000)]);
    assert_eq!(f.object("dir/a b.bin").unwrap(), bytes(300_000));
    assert_eq!(f.requests().len(), 1);

    let out = dir.path().join("sub").join("b.bin");
    let mut last = (0, 0);
    let got = get(&f.target(), "dir/a b.bin", &out, &opts(), &never(), &mut |d, t| last = (d, t)).unwrap();
    assert_eq!((got.bytes, last), (300_000, (300_000, 300_000)));
    assert_eq!((got.sha256, got.verified), (sent.sha256, true));
    assert_eq!(std::fs::read(&out).unwrap(), bytes(300_000));
    assert!(!dir.path().join("sub").join("b.bin.part").exists());
}

#[test]
fn a_key_ending_with_a_slash_gets_the_file_name() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.zip");
    std::fs::write(&src, b"zip").unwrap();
    put(&f.target(), &src, "backup/", &opts(), &never(), &mut |_, _| {}).unwrap();
    put(&f.target(), &src, "", &opts(), &never(), &mut |_, _| {}).unwrap();
    assert_eq!(f.keys(), ["a.zip", "backup/a.zip"]);
}

#[test]
fn a_big_file_goes_as_parts() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let data = bytes(11 * 1024 * 1024);
    std::fs::write(&src, &data).unwrap();
    let mut seen = Vec::new();
    let o = Options { part_size: MIN_PART, ..opts() };
    put(&f.target(), &src, "big.bin", &o, &never(), &mut |d, t| seen.push((d, t))).unwrap();
    assert!(f.object("big.bin").unwrap() == data);
    assert_eq!(count(&f, "partNumber"), 3);
    assert_eq!(f.open_uploads(), 0);
    let total = data.len() as u64;
    assert_eq!(seen.last(), Some(&(total, total)));
    assert_eq!(seen.len(), 3);
}

#[test]
fn a_failed_part_aborts_the_upload() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    std::fs::write(&src, bytes(11 * 1024 * 1024)).unwrap();
    f.fail("partNumber=2", 400, 1);
    let e = put(&f.target(), &src, "big.bin", &opts(), &never(), &mut |_, _| {}).unwrap_err();
    assert!(matches!(e, Error::Status { code: 400, .. }), "{e:?}");
    assert_eq!(f.open_uploads(), 0);
    assert!(f.keys().is_empty());
    assert_eq!(count(&f, "DELETE /bucket/big.bin?uploadId"), 1);
}

#[test]
fn stopping_between_parts_aborts_the_upload() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    std::fs::write(&src, bytes(11 * 1024 * 1024)).unwrap();
    let stop = never();
    let e = put(&f.target(), &src, "big.bin", &opts(), &stop, &mut |_, _| stop.store(true, Ordering::Relaxed)).unwrap_err();
    assert_eq!(e, Error::Cancelled);
    assert_eq!(f.open_uploads(), 0);
    assert!(f.keys().is_empty());
}

#[test]
fn a_part_that_fails_with_5xx_is_sent_again() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.txt");
    std::fs::write(&src, b"hello").unwrap();
    f.fail("PUT /bucket/a.txt", 503, 2);
    put(&f.target(), &src, "a.txt", &opts(), &never(), &mut |_, _| {}).unwrap();
    assert_eq!(f.object("a.txt").unwrap(), b"hello");
    assert_eq!(f.requests().len(), 3);
}

#[test]
fn a_refusal_is_not_retried() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.txt");
    std::fs::write(&src, b"hello").unwrap();
    f.fail("PUT /bucket/a.txt", 403, 1);
    let e = put(&f.target(), &src, "a.txt", &opts(), &never(), &mut |_, _| {}).unwrap_err();
    assert!(matches!(&e, Error::Auth(s) if s.contains("AccessDenied")), "{e:?}");
    assert!(!e.retryable());
    assert_eq!(f.requests().len(), 1);
}

#[test]
fn a_wrong_secret_is_refused() {
    let f = FakeS3::start();
    let Target::S3 { bucket, region, endpoint, path_style, key_id, session_token, .. } = f.target();
    let bad = Target::S3 { bucket, region, endpoint, path_style, key_id, secret: "other".into(), session_token };
    let e = delete(&bad, "x", &opts(), &never()).unwrap_err();
    assert!(matches!(&e, Error::Auth(s) if s.contains("SignatureDoesNotMatch")), "{e:?}");
}

#[test]
fn list_follows_pages_and_matches_wildcards() {
    let f = FakeS3::start();
    for k in ["logs/a.gz", "logs/b.gz", "logs/c.txt", "logs/2024/d.gz", "other/e.gz", "logs/f.gz"] {
        f.put_object(k, k.as_bytes());
    }
    f.set_page_size(2);
    let all = list(&f.target(), "logs/*.gz", &opts(), &never()).unwrap();
    let keys: Vec<_> = all.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["logs/2024/d.gz", "logs/a.gz", "logs/b.gz", "logs/f.gz"]);
    assert_eq!(all[1].size, 9);
    assert!(all[1].modified.is_some());
    assert!(count(&f, "list-type=2") >= 3);
    let one = list(&f.target(), "logs/c.txt", &opts(), &never()).unwrap();
    assert_eq!(one.len(), 1);
    assert!(list(&f.target(), "nothing/*", &opts(), &never()).unwrap().is_empty());
    let q = list(&f.target(), "logs/?.txt", &opts(), &never()).unwrap();
    assert_eq!(q.len(), 1);
}

#[test]
fn delete_removes_and_missing_is_fine() {
    let f = FakeS3::start();
    f.put_object("a", b"1");
    delete(&f.target(), "a", &opts(), &never()).unwrap();
    delete(&f.target(), "a", &opts(), &never()).unwrap();
    assert!(f.keys().is_empty());
    assert!(matches!(delete(&f.target(), "dir/", &opts(), &never()), Err(Error::Io(_))));
}

#[test]
fn a_missing_key_is_not_found_and_leaves_no_part_file() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.bin");
    let e = get(&f.target(), "nope", &out, &opts(), &never(), &mut |_, _| {}).unwrap_err();
    assert_eq!(e, Error::NotFound);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn a_raised_stop_flag_stops_every_call() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a");
    std::fs::write(&src, b"x").unwrap();
    let stop = AtomicBool::new(true);
    assert_eq!(put(&f.target(), &src, "a", &opts(), &stop, &mut |_, _| {}), Err(Error::Cancelled));
    assert_eq!(get(&f.target(), "a", &src, &opts(), &stop, &mut |_, _| {}), Err(Error::Cancelled));
    assert_eq!(list(&f.target(), "*", &opts(), &stop), Err(Error::Cancelled));
    assert_eq!(delete(&f.target(), "a", &opts(), &stop), Err(Error::Cancelled));
    assert!(f.requests().is_empty());
}

#[test]
fn a_slow_store_times_out() {
    let f = FakeS3::start();
    f.set_delay(Duration::from_millis(800));
    let dir = tempfile::tempdir().unwrap();
    let o = Options { request_timeout: Duration::from_millis(150), ..opts() };
    let e = get(&f.target(), "a", &dir.path().join("a"), &o, &never(), &mut |_, _| {}).unwrap_err();
    assert_eq!(e, Error::Timeout);
    assert!(e.retryable());
}

#[test]
fn the_whole_call_has_a_time_limit() {
    let f = FakeS3::start();
    let o = Options { timeout: Duration::ZERO, ..opts() };
    assert_eq!(list(&f.target(), "*", &o, &never()), Err(Error::Timeout));
}

#[test]
fn debug_hides_the_secrets() {
    let t = Target::S3 {
        bucket: "b".into(),
        region: "r".into(),
        endpoint: None,
        path_style: false,
        key_id: "AKIA1".into(),
        secret: "top-secret-value".into(),
        session_token: Some("token-value".into()),
    };
    let text = format!("{t:?}");
    assert!(!text.contains("top-secret-value") && !text.contains("token-value"), "{text}");
    assert!(text.contains("AKIA1"));
}

#[test]
fn bad_targets_are_refused_before_any_request() {
    let mk = |bucket: &str, endpoint: Option<&str>| Target::S3 {
        bucket: bucket.into(),
        region: "us-east-1".into(),
        endpoint: endpoint.map(str::to_owned),
        path_style: true,
        key_id: "k".into(),
        secret: "s".into(),
        session_token: None,
    };
    assert!(matches!(delete(&mk("", None), "a", &opts(), &never()), Err(Error::Io(_))));
    assert!(matches!(delete(&mk("b", Some("ftp://x")), "a", &opts(), &never()), Err(Error::Io(_))));
    assert!(matches!(delete(&mk("b", Some("https://")), "a", &opts(), &never()), Err(Error::Io(_))));
}

#[test]
fn an_unreachable_store_is_a_network_error() {
    let t = Target::S3 {
        bucket: "b".into(),
        region: "us-east-1".into(),
        endpoint: Some("http://127.0.0.1:1".into()),
        path_style: true,
        key_id: "k".into(),
        secret: "s".into(),
        session_token: None,
    };
    let e = list(&t, "*", &opts(), &never()).unwrap_err();
    assert!(e.retryable(), "{e:?}");
}

#[test]
fn a_multipart_upload_carries_part_checksums_and_the_whole_hash() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    let data = bytes(11 * 1024 * 1024);
    std::fs::write(&src, &data).unwrap();
    let o = Options { part_size: MIN_PART, ..opts() };
    let m = put(&f.target(), &src, "big.bin", &o, &never(), &mut |_, _| {}).unwrap();
    assert_eq!((m.bytes, m.verified), (data.len() as u64, true));
    assert_eq!(m.sha256, crate::sign::sha256_hex(&data));
    assert!(f.checksum("big.bin").unwrap().ends_with("-3"));
    // A multipart object's checksum is a checksum of checksums: the download cannot compare it, but still hashes.
    let out = dir.path().join("back.bin");
    let g = get(&f.target(), "big.bin", &out, &o, &never(), &mut |_, _| {}).unwrap();
    assert_eq!((g.sha256, g.verified), (m.sha256, false));
}

#[test]
fn damage_on_the_way_up_is_a_mismatch_and_makes_no_object() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.bin");
    std::fs::write(&src, bytes(1000)).unwrap();
    f.damage_in_transit(true);
    let e = put(&f.target(), &src, "a.bin", &opts(), &never(), &mut |_, _| {}).unwrap_err();
    assert!(matches!(e, Error::Mismatch(_)), "{e:?}");
    assert!(!e.retryable());
    assert!(f.keys().is_empty());
    // Without the checksum the same damage goes unseen: that is what the option is for.
    let o = Options { checksum: false, ..opts() };
    let m = put(&f.target(), &src, "a.bin", &o, &never(), &mut |_, _| {}).unwrap();
    assert!(!m.verified);
}

#[test]
fn damage_on_the_way_up_aborts_a_multipart_upload() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("big.bin");
    std::fs::write(&src, bytes(11 * 1024 * 1024)).unwrap();
    f.damage_in_transit(true);
    let e = put(&f.target(), &src, "big.bin", &opts(), &never(), &mut |_, _| {}).unwrap_err();
    assert!(matches!(e, Error::Mismatch(_)), "{e:?}");
    assert_eq!(f.open_uploads(), 0);
}

#[test]
fn a_damaged_download_is_a_mismatch_and_leaves_no_file() {
    let f = FakeS3::start();
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.bin");
    std::fs::write(&src, bytes(5000)).unwrap();
    put(&f.target(), &src, "a.bin", &opts(), &never(), &mut |_, _| {}).unwrap();
    f.corrupt_object("a.bin");
    let out = dir.path().join("out.bin");
    let e = get(&f.target(), "a.bin", &out, &opts(), &never(), &mut |_, _| {}).unwrap_err();
    assert!(matches!(e, Error::Mismatch(_)), "{e:?}");
    assert!(!out.exists() && !dir.path().join("out.bin.part").exists());
    let o = Options { checksum: false, ..opts() };
    let m = get(&f.target(), "a.bin", &out, &o, &never(), &mut |_, _| {}).unwrap();
    assert!(!m.verified);
}

#[test]
fn an_object_stored_without_a_checksum_downloads_unverified() {
    let f = FakeS3::start();
    f.put_object("old.bin", b"hello");
    let dir = tempfile::tempdir().unwrap();
    let m = get(&f.target(), "old.bin", &dir.path().join("o"), &opts(), &never(), &mut |_, _| {}).unwrap();
    assert_eq!((m.bytes, m.verified), (5, false));
    assert_eq!(m.sha256, crate::sign::sha256_hex(b"hello"));
}
