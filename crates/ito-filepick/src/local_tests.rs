//! Tests of `list_local` and friends against a real folder under the system temp folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::local::home_place;
use crate::{list_local, local_places, local_style, Entry, Kind, ListError, PathError, PlaceKind, Style};

/// A fresh folder under the temp folder, removed on drop (also when a test fails).
struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir =
            // Short, so a Unix socket inside stays under the 104-byte `sun_path` of macOS.
            std::env::temp_dir().join(format!("ifp-{}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed), nanos % 1_000_000_000));
        fs::create_dir(&dir).expect("create the fixture folder");
        Fixture(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn text(&self) -> String {
        self.0.to_str().expect("a temp path is text").to_owned()
    }
    fn file(&self, name: &str, bytes: &[u8]) {
        fs::write(self.path(name), bytes).expect("write a file");
    }
    fn list(&self, max: usize) -> crate::Listing {
        list_local(&self.text(), max).expect("list the fixture")
    }
    fn find(&self, name: &str) -> Entry {
        self.list(1000).entries.into_iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} is not listed"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // A junction or a symlink to a folder is removed itself, never through.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn secs_of(path: &Path) -> Option<i64> {
    let t = fs::metadata(path).unwrap().modified().unwrap();
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => Some(d.as_secs() as i64),
        Err(_) => None,
    }
}

#[test]
fn lists_files_and_folders() {
    let f = Fixture::new();
    f.file("a.txt", b"hello");
    f.file("empty", b"");
    fs::create_dir(f.path("sub")).unwrap();
    let l = f.list(100);
    assert_eq!((l.entries.len(), l.truncated, l.left_out), (3, false, 0));
    let a = f.find("a.txt");
    assert_eq!((a.kind, a.size, a.link, a.hidden), (Kind::File, Some(5), false, false));
    assert_eq!(a.modified, secs_of(&f.path("a.txt")));
    assert!(a.modified.is_some());
    assert_eq!(f.find("empty").size, Some(0));
    let d = f.find("sub");
    assert_eq!((d.kind, d.size, d.link), (Kind::Dir, None, false));
    assert_eq!(d.modified, secs_of(&f.path("sub")));
}

#[test]
fn cap_and_truncated() {
    let f = Fixture::new();
    for i in 0..5 {
        f.file(&format!("f{i}"), b"x");
    }
    let l = f.list(3);
    assert_eq!((l.entries.len(), l.truncated), (3, true));
    let l = f.list(5);
    assert_eq!((l.entries.len(), l.truncated), (5, false));
    let l = f.list(0);
    assert_eq!((l.entries.len(), l.truncated), (0, true));
    let empty = Fixture::new();
    let l = empty.list(0);
    assert_eq!((l.entries.len(), l.truncated), (0, false));
}

/// Whether a creation error is a recognised "not supported here" (R14).
fn unsupported(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::InvalidFilename
        || matches!(e.raw_os_error(), Some(1314)) && cfg!(windows)
        || matches!(e.raw_os_error(), Some(92 | 84)) && cfg!(any(target_os = "macos", target_os = "linux"))
}

#[cfg(unix)]
#[test]
fn unix_links_and_special_files() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;
    let f = Fixture::new();
    f.file("target.txt", b"abc");
    fs::create_dir(f.path("target_dir")).unwrap();
    symlink(f.path("target.txt"), f.path("to_file")).unwrap();
    symlink(f.path("target_dir"), f.path("to_dir")).unwrap();
    symlink(f.path("missing"), f.path("broken")).unwrap();
    let a = f.find("to_file");
    assert_eq!((a.link, a.kind, a.size), (true, Kind::File, Some(3)));
    assert!(a.modified.is_some());
    let d = f.find("to_dir");
    assert_eq!((d.link, d.kind, d.size), (true, Kind::Dir, None));
    let b = f.find("broken");
    assert_eq!((b.link, b.kind, b.size, b.modified), (true, Kind::Unknown, None, None));
    // A socket is Other and is never opened.
    let _sock = UnixListener::bind(f.path("sock")).unwrap();
    let s = f.find("sock");
    assert_eq!((s.kind, s.size, s.link), (Kind::Other, None, false));
    // The dialog's job, not the engine's.
    f.file(".dot", b"");
    assert!(!f.find(".dot").hidden);
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let out = std::process::Command::new("cmd").arg("/c").arg("mklink").arg("/J").arg(link).arg(target).output().expect("run cmd");
    assert!(out.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&out.stdout));
}

#[cfg(windows)]
fn attrib(flag: &str, path: &Path, link: bool) {
    let mut c = std::process::Command::new("attrib");
    c.arg(flag).arg(path);
    if link {
        c.arg("/L");
    }
    assert!(c.output().expect("run attrib").status.success(), "attrib {flag} failed");
}

#[cfg(windows)]
#[test]
fn windows_junctions_symlinks_and_hidden() {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let f = Fixture::new();
    fs::create_dir(f.path("target_dir")).unwrap();
    f.file("target.txt", b"abcd");
    junction(&f.path("junc"), &f.path("target_dir"));
    let j = f.find("junc");
    assert_eq!((j.link, j.kind, j.size), (true, Kind::Dir, None));
    assert!(!j.hidden);

    // A junction that is hidden itself, while its target is not.
    attrib("+h", &f.path("junc"), true);
    assert!(f.find("junc").hidden);
    assert!(!f.find("target_dir").hidden);

    // A broken junction: the target folder is gone.
    fs::create_dir(f.path("gone")).unwrap();
    junction(&f.path("broken_junc"), &f.path("gone"));
    fs::remove_dir(f.path("gone")).unwrap();
    let b = f.find("broken_junc");
    assert_eq!((b.link, b.kind, b.size, b.modified), (true, Kind::Unknown, None, None));

    // Symlinks need Developer Mode (or elevation): skipped, with a note, when the OS says the privilege is missing.
    match symlink_file(f.path("target.txt"), f.path("sym_file")) {
        Ok(()) => {
            let s = f.find("sym_file");
            assert_eq!((s.link, s.kind, s.size), (true, Kind::File, Some(4)));
            symlink_dir(f.path("target_dir"), f.path("sym_dir")).unwrap();
            let s = f.find("sym_dir");
            assert_eq!((s.link, s.kind), (true, Kind::Dir));
        }
        // ERROR_PRIVILEGE_NOT_HELD only (R14).
        Err(e) if e.raw_os_error() == Some(1314) => println!("skipped the symlink part: cannot create symlinks here ({e})"),
        Err(e) => panic!("creating a symlink failed unexpectedly: {e}"),
    }
}

#[cfg(windows)]
#[test]
fn windows_hidden_attribute() {
    let f = Fixture::new();
    f.file("shown.txt", b"");
    f.file("secret.txt", b"");
    attrib("+h", &f.path("secret.txt"), false);
    assert!(f.find("secret.txt").hidden);
    assert!(!f.find("shown.txt").hidden);
}

#[test]
fn unreadable_name_is_left_out() {
    let f = Fixture::new();
    f.file("good", b"");
    #[cfg(unix)]
    let bad = {
        use std::os::unix::ffi::OsStrExt;
        std::ffi::OsStr::from_bytes(b"bad\xff").to_owned()
    };
    #[cfg(windows)]
    let bad = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0x61, 0xD800])
    };
    match fs::write(f.0.join(bad), b"") {
        Ok(()) => {}
        Err(e) if unsupported(&e) => return println!("skipped: the file system refuses the name ({e})"),
        Err(e) => panic!("creating the odd name failed unexpectedly: {e}"),
    }
    let l = f.list(100);
    assert_eq!(l.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["good"]);
    assert_eq!((l.left_out, l.truncated), (1, false));
    // It does not use up max.
    let l = f.list(1);
    assert_eq!((l.entries.len(), l.left_out, l.truncated), (1, 1, false));
}

#[test]
fn refused_forms_are_gate_errors() {
    // `None` is `NotAbsolute`.
    let refused: Vec<(&str, Option<PathError>)> = if cfg!(windows) {
        vec![
            ("C:foo", Some(PathError::DriveRelative)),
            ("\\foo", None),
            ("foo", None),
            (".\\foo", None),
            ("\\\\?\\C:\\x", Some(PathError::Verbatim)),
            ("\\\\.\\pipe\\", Some(PathError::Verbatim)),
            ("\\\\server", Some(PathError::NoShare)),
            ("\\\\server\\", Some(PathError::NoShare)),
            ("a\0b", Some(PathError::Nul)),
            ("C:\\a\0b", Some(PathError::Nul)),
        ]
    } else {
        vec![("foo", None), ("./foo", None), ("", None), ("a\0b", Some(PathError::Nul)), ("/a\0b", Some(PathError::Nul))]
    };
    for (path, expect) in refused {
        match (list_local(path, 10), expect) {
            (Err(ListError::NotAbsolute), None) => {}
            (Err(ListError::Path(got)), Some(want)) => assert_eq!(got, want, "{path:?}"),
            (other, _) => panic!("{path:?} gave {other:?}"),
        }
    }
}

#[test]
fn accepted_forms_reach_the_disk() {
    // An absolute path that does not exist is an Io error (the gate let it through), never a gate error.
    let f = Fixture::new();
    let missing = format!("{}{}no-such", f.text(), local_style().sep());
    assert!(matches!(list_local(&missing, 10), Err(ListError::Io(_))));
    #[cfg(windows)]
    {
        // Mixed separators pass as given.
        let mixed = f.text().replace('\\', "/");
        assert_eq!(list_local(&mixed, 10).unwrap().entries.len(), 0);
        // `C:` alone is read as the root of C.
        assert!(!list_local("C:", 5).unwrap().entries.is_empty());
    }
}

#[test]
fn io_errors() {
    let f = Fixture::new();
    f.file("plain", b"x");
    let missing = f.path("missing").to_str().unwrap().to_owned();
    let e = list_local(&missing, 10).unwrap_err();
    assert!(matches!(e, ListError::Io(_)));
    let ja = e.words("ja");
    assert!(!ja.is_empty() && !ja.to_lowercase().contains("os error"), "{ja}");
    assert!(std::error::Error::source(&e).is_some());
    let file = f.path("plain").to_str().unwrap().to_owned();
    assert!(matches!(list_local(&file, 10), Err(ListError::Io(_))));
}

#[test]
fn places() {
    let places = local_places();
    let home = home_place(ito_common::home_dir());
    if let Some(h) = &home {
        assert_eq!(places.last(), Some(h));
        assert_eq!(h.kind, PlaceKind::Home);
    } else {
        assert!(places.iter().all(|p| p.kind != PlaceKind::Home), "{places:?}");
    }
    #[cfg(windows)]
    {
        let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        assert!(places.iter().any(|p| p.kind == PlaceKind::Drive && p.path == format!("{sys}\\") && p.label == sys));
        assert!(places.iter().filter(|p| p.kind == PlaceKind::Drive).all(|p| p.path.len() == 3));
    }
    #[cfg(not(windows))]
    {
        assert_eq!((places[0].label.as_str(), places[0].path.as_str(), places[0].kind), ("/", "/", PlaceKind::Root));
    }
    assert_eq!(local_style(), if cfg!(windows) { Style::Windows } else { Style::Posix });
}

#[test]
fn list_error_words() {
    let abs = ListError::NotAbsolute;
    let (en, ja) = if cfg!(windows) {
        ("Use a full path starting at a drive, such as C:\\.", "ドライブから始まるパス（例: C:\\）で指定してください。")
    } else {
        ("Use a full path starting at /.", "/ から始まるパスで指定してください。")
    };
    assert_eq!(abs.words("en"), en);
    assert_eq!(abs.words("ja"), ja);
    assert_eq!(abs.words("JA"), ja);
    assert_eq!(abs.words("ja-JP"), ja);
    assert_eq!(abs.words("fr"), en);
    assert_eq!(abs.to_string(), en);

    for p in [PathError::NoShare, PathError::DriveRelative, PathError::Verbatim, PathError::Nul] {
        let e = ListError::Path(p.clone());
        assert_eq!(e.words("en"), crate::Words::en().path_error(&p));
        assert_eq!(e.words("ja"), crate::Words::ja().path_error(&p));
        assert_ne!(e.words("en"), e.words("ja"));
        assert_eq!(e.to_string(), e.words("en"));
    }

    let nf = io::Error::from(io::ErrorKind::NotFound);
    let (want_en, want_ja) = (ito_common::ioerror::words(&nf, "en"), ito_common::ioerror::words(&nf, "ja"));
    let e = ListError::Io(nf);
    assert_eq!((e.words("en"), e.words("ja")), (want_en, want_ja));
    assert_ne!(e.words("en"), e.words("ja"));

    let other = ListError::Io(io::Error::other("SENTINEL-OS-TEXT"));
    assert!(!other.words("en").contains("SENTINEL-OS-TEXT"));
    assert!(!other.words("ja").contains("SENTINEL-OS-TEXT"));
    assert!(!other.to_string().contains("SENTINEL-OS-TEXT"));
}

// ---- ListOptions: links, the network and devices ----

use crate::{check_local, list_local_with, local_places_with, ListOptions};

/// Makes a symbolic link; `None` (and a note) when the account may not make them.
fn make_symlink(target: &Path, link: &Path, dir: bool) -> Option<()> {
    #[cfg(windows)]
    let r = if dir { std::os::windows::fs::symlink_dir(target, link) } else { std::os::windows::fs::symlink_file(target, link) };
    #[cfg(unix)]
    let r = {
        let _ = dir;
        std::os::unix::fs::symlink(target, link)
    };
    match r {
        Ok(()) => Some(()),
        Err(e) if e.raw_os_error() == Some(1314) => {
            eprintln!("note: no right to make symbolic links here (os error 1314); skipping those asserts");
            None
        }
        Err(e) => panic!("symlink: {e}"),
    }
}

fn at_of(e: ListError) -> String {
    match e {
        ListError::Link { at } => at,
        other => panic!("not Link: {other:?}"),
    }
}

#[test]
fn check_local_fail_closed_and_last_may_be_missing() {
    let f = Fixture::new();
    let strict = ListOptions::strict(10);
    match check_local(&f.path("missing").join("x").to_string_lossy(), &strict) {
        Err(ListError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
        other => panic!("{other:?}"),
    }
    assert!(check_local(&f.path("new.zip").to_string_lossy(), &strict).is_ok());
    // list_local_with on a missing last item gets its error from read_dir.
    assert!(matches!(list_local_with(&f.path("new.zip").to_string_lossy(), &strict), Err(ListError::Io(_))));
    #[cfg(unix)]
    assert!(check_local("/", &strict).is_ok());
    #[cfg(windows)]
    {
        let root = format!("{}\\", &f.text()[..2]);
        assert!(check_local(&root, &strict).is_ok(), "the system drive must classify as local");
    }
}

#[test]
fn places_with_default_options_equal_places() {
    assert_eq!(local_places_with(&ListOptions::new(10)), local_places());
    // Strict never adds a place.
    let all = local_places();
    assert!(local_places_with(&ListOptions::strict(10)).iter().all(|p| all.contains(p)));
}

#[test]
fn strict_does_not_follow_link_entries() {
    let f = Fixture::new();
    fs::create_dir(f.path("real")).unwrap();
    f.file("data.txt", b"12345");
    let Some(()) = make_symlink(&f.path("real"), &f.path("dlink"), true) else { return };
    make_symlink(&f.path("data.txt"), &f.path("flink"), false).unwrap();
    let strict = list_local_with(&f.text(), &ListOptions::strict(100)).unwrap();
    let by = |n: &str| strict.entries.iter().find(|e| e.name == n).unwrap().clone();
    let (d, fl) = (by("dlink"), by("flink"));
    assert!(d.link && fl.link);
    assert_eq!((d.size, d.modified, fl.size, fl.modified), (None, None, None, None));
    if cfg!(windows) {
        assert_eq!((d.kind, fl.kind), (Kind::Dir, Kind::File));
    } else {
        assert_eq!((d.kind, fl.kind), (Kind::Unknown, Kind::Unknown));
    }
    // Not links: unchanged.
    assert_eq!(by("data.txt").size, Some(5));
    // The default listing follows.
    let dflt = f.find("flink");
    assert_eq!((dflt.kind, dflt.size, dflt.link), (Kind::File, Some(5), true));
    assert_eq!(f.find("dlink").kind, Kind::Dir);
}

#[test]
fn strict_refuses_a_path_through_a_symlink() {
    let f = Fixture::new();
    fs::create_dir(f.path("real")).unwrap();
    fs::create_dir(f.path("real").join("sub")).unwrap();
    let Some(()) = make_symlink(&f.path("real"), &f.path("link"), true) else { return };
    let strict = ListOptions::strict(100);
    let link = f.path("link").to_string_lossy().into_owned();
    let sep = std::path::MAIN_SEPARATOR;
    for p in [link.clone(), format!("{link}{sep}sub")] {
        assert_eq!(at_of(check_local(&p, &strict).unwrap_err()), link, "{p}");
        assert_eq!(at_of(list_local_with(&p, &strict).unwrap_err()), link, "{p}");
    }
    #[cfg(unix)]
    for p in [format!("{link}/"), format!("{link}/."), format!("{link}/../x")] {
        assert_eq!(at_of(check_local(&p, &strict).unwrap_err()), link, "{p}");
    }
    #[cfg(unix)]
    {
        // `real/../real` is not a link: it lists.
        let real = f.path("real").to_string_lossy().into_owned();
        assert!(list_local_with(&format!("{real}/../real"), &strict).is_ok());
    }
    // Allowed when links in the path are allowed.
    assert!(list_local_with(&link, &ListOptions::strict(100).links_in_path(true)).is_ok());
}

#[cfg(windows)]
mod windows_rules {
    use super::*;

    #[test]
    fn devices_and_bad_names_are_always_refused() {
        let f = Fixture::new();
        let t = f.text();
        for opts in [ListOptions::new(10), ListOptions::strict(10)] {
            assert!(matches!(check_local(&format!("{t}\\NUL"), &opts), Err(ListError::Device { .. })));
            assert!(matches!(check_local(&format!("{t}\\COM1"), &opts), Err(ListError::Device { .. })));
            assert!(matches!(check_local(&format!("{t}\\a:b"), &opts), Err(ListError::Name { .. })));
        }
        assert!(matches!(list_local(&format!("{t}\\NUL"), 10), Err(ListError::Device { .. })));
        assert!(matches!(list_local(&format!("{t}\\COM1"), 10), Err(ListError::Device { .. })));
        assert!(matches!(list_local(&format!("{t}\\a:b"), 10), Err(ListError::Name { .. })));
    }

    #[test]
    fn check_local_returns_the_absolute_form() {
        let f = Fixture::new();
        fs::create_dir(f.path("real")).unwrap();
        let t = f.text();
        let want = format!("{t}\\real");
        assert_eq!(check_local(&format!("{}/real", t.replace('\\', "/")), &ListOptions::new(1)).unwrap(), want);
        assert_eq!(check_local(&format!("{t}\\real."), &ListOptions::new(1)).unwrap(), want);
        assert!(list_local(&format!("{t}\\real."), 10).is_ok());
    }

    fn junction(f: &Fixture, link: &str, target: &str) {
        let out = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(f.path(link)).arg(f.path(target)).output().unwrap();
        assert!(out.status.success(), "mklink /J: {}", String::from_utf8_lossy(&out.stdout));
    }

    #[test]
    fn junctions() {
        let f = Fixture::new();
        fs::create_dir(f.path("real")).unwrap();
        fs::write(f.path("real").join("inner.txt"), b"abc").unwrap();
        fs::create_dir(f.path("real").join("sub")).unwrap();
        junction(&f, "junc", "real");
        let t = f.text();
        let dflt = f.find("junc");
        assert_eq!((dflt.link, dflt.kind), (true, Kind::Dir));
        assert!(dflt.modified.is_some());
        let strict = list_local_with(&t, &ListOptions::strict(100)).unwrap();
        let j = strict.entries.iter().find(|e| e.name == "junc").unwrap();
        assert_eq!((j.link, j.kind, j.size, j.modified), (true, Kind::Dir, None, None));
        let junc = format!("{t}\\junc");
        assert_eq!(at_of(list_local_with(&junc, &ListOptions::strict(10)).unwrap_err()), junc);
        assert_eq!(at_of(list_local_with(&format!("{junc}\\sub"), &ListOptions::strict(10)).unwrap_err()), junc);
        let names = |l: crate::Listing| l.entries.into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert!(names(list_local(&junc, 10).unwrap()).contains(&"inner.txt".to_owned()));
        // Win32 drops `junc\..` as text, so this is `real`.
        let l = list_local_with(&format!("{junc}\\..\\real"), &ListOptions::strict(10)).unwrap();
        assert!(names(l).contains(&"inner.txt".to_owned()));
    }

    #[test]
    fn unc_is_refused_by_name_without_waiting() {
        let start = std::time::Instant::now();
        let r = check_local(r"\\ito-test.invalid\share\x", &ListOptions::strict(10));
        let took = start.elapsed();
        eprintln!("UNC Network check took {took:?}");
        assert!(matches!(r, Err(ListError::Network)), "{r:?}");
        assert!(took < std::time::Duration::from_secs(2));
        assert!(matches!(list_local_with(r"\\ito-test.invalid\share", &ListOptions::new(1).network(false)), Err(ListError::Network)));
    }
}
