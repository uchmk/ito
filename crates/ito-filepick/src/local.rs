//! Listing a folder of this machine for a [`Picker`](crate::Picker) on another machine (feature `local`).
//!
//! The engine side of the exchange: the window forwards a [`Want::List`](crate::Want) path, the engine calls
//! [`list_local`] on a worker thread and sends the [`Listing`] back (feature `serde` gives JSON), and the window calls
//! `set_listing`. [`local_style`] and [`local_places`] tell the window how this machine writes paths and what to offer
//! in the places column.

use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::path::Gate;
use crate::{Entry, Kind, Listing, PathError, Place, PlaceKind, Style, Words};

/// How many items past `max` are read before a listing gives up (names that cannot be sent and failing reads count).
const READ_SLACK: usize = 1024;

/// The path style of the machine this program runs on (`Windows` on Windows, `Posix` elsewhere): what an engine tells
/// the window so its `Picker` reads paths the way this machine writes them.
pub fn local_style() -> Style {
    if cfg!(windows) {
        Style::Windows
    } else {
        Style::Posix
    }
}

/// Why [`list_local`] could not list a folder.
#[derive(Debug)]
#[non_exhaustive]
pub enum ListError {
    /// Relative, or (Windows) `\dir`: both depend on the engine's current folder or drive, which the person never sees.
    NotAbsolute,
    /// A form that is refused: `C:foo` (`DriveRelative`), `\\?\...` `\\.\...` `\??\...` (`Verbatim`), `\\server`
    /// (`NoShare`), a NUL (`Nul`).
    Path(PathError),
    /// The OS could not read the folder (not found, no permission, not a folder, drive not ready, ...).
    Io(io::Error),
}

impl ListError {
    /// A sentence for the person in `lang` (Japanese when it starts with `ja`, in any case; anything else English).
    /// `Path` uses the crate's [`Words::path_error`]; `Io` uses `ito_common::ioerror::words` (no OS text);
    /// `NotAbsolute` names this machine's form (starting at a drive such as `C:\`, or at `/`).
    pub fn words(&self, lang: &str) -> String {
        let ja = lang.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("ja"));
        let (words, code) = if ja { (Words::ja(), "ja") } else { (Words::en(), "en") };
        match self {
            ListError::NotAbsolute => match (cfg!(windows), ja) {
                (true, true) => "ドライブから始まるパス（例: C:\\）で指定してください。".to_owned(),
                (true, false) => "Use a full path starting at a drive, such as C:\\.".to_owned(),
                (false, true) => "/ から始まるパスで指定してください。".to_owned(),
                (false, false) => "Use a full path starting at /.".to_owned(),
            },
            ListError::Path(p) => words.path_error(p),
            ListError::Io(e) => ito_common::ioerror::words(e, code),
        }
    }
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.words("en"))
    }
}

impl std::error::Error for ListError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ListError::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// Lists one folder of this machine for a `Picker` on another machine.
///
/// It **blocks** (disk, maybe a network share): call it from a worker thread.
///
/// - `path` must be absolute in this machine's style ([`local_style`]). The gate is lexical only and runs before the
///   disk is touched: a NUL, a relative path, `\dir` or `C:foo`, the verbatim and device forms and a UNC path with no
///   share are refused. `C:` alone is read as `C:\`. Nothing else is rewritten: reserved device names (`C:\x\CON`),
///   trailing dots and spaces, and other Win32 rules are left to Windows, which returns an error or lists what it means.
/// - **No containment and no authorization.** It follows links and lists any absolute path the process can read. The app
///   decides who may ask (koyomi, for example, only for an Admin). `..` is resolved by the OS (through links), unlike the
///   picker's lexical `resolve`; the picker never sends `..`.
/// - At most `max` entries are returned. `truncated` means the folder has more entries or the listing stopped before the
///   end: reading stops as soon as one more valid entry exists, or after `max + 1024` items of any kind (so a huge folder,
///   or a stream of failing reads, is not read to the end). `left_out` counts what was seen and left out: names that are
///   not valid Unicode, and reads that failed. They do not use up `max`.
/// - Each entry is the entry's own: `link` is a symlink or a junction, and then `kind`, `size` and `modified` are those of
///   the target (`Unknown` and `None` when it cannot be read). `size` is for files only. `modified` is Unix seconds.
///   `hidden` is the Windows hidden attribute (a dot name is the dialog's business).
pub fn list_local(path: &str, max: usize) -> Result<Listing, ListError> {
    let path = local_style().gate_local(path).map_err(|g| match g {
        Gate::NotAbsolute => ListError::NotAbsolute,
        Gate::Path(p) => ListError::Path(p),
    })?;
    let dir = fs::read_dir(&path).map_err(ListError::Io)?;
    Ok(collect(dir.map(|item| item.map(entry_of).map_err(|_| ())), max))
}

/// The cap and the read budget, over what the iterator yields: `Err` is a failed read, `Ok(None)` a name that cannot be
/// sent, `Ok(Some)` an entry. The iterator is pulled lazily, so reading stops when this does.
fn collect(items: impl Iterator<Item = Result<Option<Entry>, ()>>, max: usize) -> Listing {
    let budget = max.saturating_add(READ_SLACK);
    let mut listing = Listing::default();
    for (read, item) in items.enumerate() {
        if read >= budget {
            listing.truncated = true;
            break;
        }
        match item {
            Ok(Some(entry)) => {
                if listing.entries.len() >= max {
                    listing.truncated = true;
                    break;
                }
                listing.entries.push(entry);
            }
            Ok(None) | Err(()) => listing.left_out = listing.left_out.saturating_add(1),
        }
    }
    listing
}

fn entry_of(de: fs::DirEntry) -> Option<Entry> {
    let name = de.file_name().into_string().ok()?;
    let ft = de.file_type().ok();
    let own = de.metadata().ok();
    let link = ft.or_else(|| own.as_ref().map(|m| m.file_type())).is_some_and(|t| t.is_symlink());
    let hidden = own.as_ref().is_some_and(is_hidden);
    // What the kind, size and time are read from: the entry itself, or what a link points at.
    let source = if link { fs::metadata(de.path()).ok() } else { own };
    let kind_of = |t: fs::FileType| {
        if t.is_dir() {
            Kind::Dir
        } else if t.is_file() {
            Kind::File
        } else {
            Kind::Other
        }
    };
    let (kind, size, modified) = match source {
        Some(m) => {
            let kind = kind_of(m.file_type());
            (kind, (kind == Kind::File).then_some(m.len()), m.modified().ok().and_then(unix_secs))
        }
        // No metadata: a non-link keeps what its file type says; a link we could not follow is Unknown.
        None => (if link { None } else { ft.map(kind_of) }.unwrap_or(Kind::Unknown), None, None),
    };
    Some(Entry { name, kind, link, hidden, size, modified })
}

#[cfg(windows)]
fn is_hidden(m: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x2 != 0
}

#[cfg(not(windows))]
fn is_hidden(_: &fs::Metadata) -> bool {
    false
}

/// Unix seconds: whole seconds rounded down (toward minus infinity before 1970); `None` when it does not fit.
fn unix_secs(t: SystemTime) -> Option<i64> {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).ok(),
        Err(e) => {
            let d = e.duration();
            let secs = i64::try_from(d.as_secs()).ok()?.checked_add(i64::from(d.subsec_nanos() > 0))?;
            secs.checked_neg()
        }
    }
}

/// The places column for this machine, best effort. Windows: each assigned drive letter (`C:\`, label `C:`; the drives
/// are not touched, so an empty card reader or a lost network drive cannot stall it), then home. Elsewhere: `/`, then
/// home. Home's label is the last part of its path, its kind `Home`; it is left out when it is not known or not usable.
/// Labels are language-free; the app may rename them.
pub fn local_places() -> Vec<Place> {
    let mut places = system_places();
    places.extend(home_place(ito_common::home_dir()));
    places
}

#[cfg(windows)]
fn system_places() -> Vec<Place> {
    // SAFETY: GetLogicalDrives takes no arguments and only reads the drive bitmask.
    let mask = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    drive_places(mask)
}

#[cfg(not(windows))]
fn system_places() -> Vec<Place> {
    vec![Place::new("/", "/", PlaceKind::Root)]
}

/// One place per bit of a `GetLogicalDrives` mask (bit 0 is `A:`), in letter order.
#[cfg(any(windows, test))]
fn drive_places(mask: u32) -> Vec<Place> {
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| {
            let letter = char::from(b'A' + i);
            Place::new(&format!("{letter}:"), &format!("{letter}:\\"), PlaceKind::Drive)
        })
        .collect()
}

/// The home place for a home folder, if it is a usable one: absolute, valid Unicode, and passing the gate.
pub(crate) fn home_place(home: Option<PathBuf>) -> Option<Place> {
    let text = home?.into_os_string().into_string().ok()?;
    let style = local_style();
    let path = style.gate_local(&text).ok()?;
    let label = style.last_part(&path).unwrap_or_else(|| path.clone());
    Some(Place::new(&label, &path, PlaceKind::Home))
}

#[cfg(test)]
mod seam_tests {
    use super::*;

    fn e(n: &str) -> Result<Option<Entry>, ()> {
        Ok(Some(Entry::file(n)))
    }

    #[test]
    fn cap_looks_one_ahead() {
        let five = || (0..5).map(|i| e(&format!("f{i}")));
        let l = collect(five(), 3);
        assert_eq!((l.entries.len(), l.truncated), (3, true));
        let l = collect(five(), 5);
        assert_eq!((l.entries.len(), l.truncated), (5, false));
        let l = collect(five(), 6);
        assert_eq!((l.entries.len(), l.truncated), (5, false));
    }

    #[test]
    fn max_zero() {
        let l = collect((0..2).map(|i| e(&format!("f{i}"))), 0);
        assert_eq!((l.entries.len(), l.truncated), (0, true));
        let l = collect(std::iter::empty(), 0);
        assert_eq!((l.entries.len(), l.truncated), (0, false));
    }

    #[test]
    fn unsendable_names_do_not_use_up_max() {
        let items = vec![e("a"), Ok(None), e("b"), Err(()), Ok(None), e("c")];
        let l = collect(items.into_iter(), 3);
        assert_eq!((l.entries.len(), l.truncated, l.left_out), (3, false, 3));
        let items = vec![Ok(None), e("a"), Err(()), e("b"), e("c")];
        let l = collect(items.into_iter(), 2);
        assert_eq!((l.entries.len(), l.truncated, l.left_out), (2, true, 2));
    }

    #[test]
    fn budget_stops_a_stream_of_errors_and_of_bad_names() {
        let pulled = std::cell::Cell::new(0usize);
        let errs = std::iter::repeat_with(|| {
            pulled.set(pulled.get() + 1);
            Err(())
        });
        let l = collect(errs, 3);
        assert!(l.truncated && l.entries.is_empty());
        assert_eq!(l.left_out as usize, 3 + READ_SLACK);
        assert_eq!(pulled.get(), 3 + READ_SLACK + 1);
        let l = collect(std::iter::repeat(Ok(None)), 0);
        assert!(l.truncated);
        assert_eq!(l.left_out as usize, READ_SLACK);
    }

    #[test]
    fn exactly_the_budget_is_not_truncated() {
        let l = collect((0..READ_SLACK).map(|_| Err(())), 0);
        assert!(!l.truncated);
        assert_eq!(l.left_out as usize, READ_SLACK);
    }

    #[test]
    fn unix_secs_boundaries() {
        use std::time::Duration;
        assert_eq!(unix_secs(UNIX_EPOCH), Some(0));
        assert_eq!(unix_secs(UNIX_EPOCH + Duration::from_millis(999)), Some(0));
        assert_eq!(unix_secs(UNIX_EPOCH + Duration::from_millis(1500)), Some(1));
        assert_eq!(unix_secs(UNIX_EPOCH - Duration::from_millis(500)), Some(-1));
        assert_eq!(unix_secs(UNIX_EPOCH - Duration::from_secs(1)), Some(-1));
        assert_eq!(unix_secs(UNIX_EPOCH - Duration::from_millis(1001)), Some(-2));
    }

    #[test]
    fn drive_masks() {
        assert!(drive_places(0).is_empty());
        assert_eq!(drive_places(1 << 2), vec![Place::new("C:", "C:\\", PlaceKind::Drive)]);
        let acz = drive_places(1 | 1 << 2 | 1 << 25);
        let paths: Vec<_> = acz.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, ["A:\\", "C:\\", "Z:\\"]);
        assert_eq!(acz[2].label, "Z:");
    }

    #[test]
    fn home_place_cases() {
        assert_eq!(home_place(None), None);
        assert_eq!(home_place(Some(PathBuf::new())), None);
        assert_eq!(home_place(Some(PathBuf::from("relative/home"))), None);
        #[cfg(windows)]
        {
            assert_eq!(
                home_place(Some(PathBuf::from("C:\\Users\\koyomi"))),
                Some(Place::new("koyomi", "C:\\Users\\koyomi", PlaceKind::Home))
            );
            assert_eq!(home_place(Some(PathBuf::from("C:\\"))), Some(Place::new("C:\\", "C:\\", PlaceKind::Home)));
            assert_eq!(home_place(Some(PathBuf::from("\\\\?\\C:\\Users\\x"))), None);
            assert_eq!(home_place(Some(PathBuf::from("C:x"))), None);
            use std::os::windows::ffi::OsStringExt;
            assert_eq!(home_place(Some(PathBuf::from(std::ffi::OsString::from_wide(&[0x43, 0x3A, 0x5C, 0xD800])))), None);
        }
        #[cfg(unix)]
        {
            assert_eq!(home_place(Some(PathBuf::from("/home/koyomi"))), Some(Place::new("koyomi", "/home/koyomi", PlaceKind::Home)));
            assert_eq!(home_place(Some(PathBuf::from("/"))), Some(Place::new("/", "/", PlaceKind::Home)));
            use std::os::unix::ffi::OsStringExt;
            assert_eq!(home_place(Some(PathBuf::from(std::ffi::OsString::from_vec(b"/home/\xff".to_vec())))), None);
        }
    }
}
