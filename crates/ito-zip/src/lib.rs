//! Zip files for the apps: put files in, list what is inside, read one back. Pure Rust (`zip` with
//! deflate), the same on Windows, Linux and macOS; Explorer opens the result (names are UTF-8).
//!
//! - [`add`] / [`add_dir`] add to a zip that may already exist. They write `<zip>.part`, flush it to the
//!   disk and rename it over the zip, so a crash or a full disk leaves the old zip whole and no `.part`.
//!   Entries already in the zip are copied as they are (not recompressed).
//! - [`list`] and [`read`] / [`read_to`] look inside.
//! - Entry names are relative paths with `/`; a name with `..`, a leading `/`, a drive or `\` is refused
//!   ([`Error::BadName`]) so nothing can be written outside where the zip is unpacked.
//!
//! Times are stored as UTC (a zip has no zone). Every call blocks: use a worker thread.

use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// Why a call failed. `Display` is a short Japanese sentence fit for a log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A file could not be read or written (the zip itself, or a source file).
    Io(String),
    /// The zip is damaged or is not a zip.
    Corrupt(String),
    /// No such entry (for [`read`]), or no such source file.
    NotFound(String),
    /// The name is already in the zip and [`OnExisting::Refuse`] is set.
    Exists(String),
    /// The entry name is not a safe relative path.
    BadName(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(m) => write!(f, "ファイルを扱えません: {m}"),
            Error::Corrupt(m) => write!(f, "zip が壊れています: {m}"),
            Error::NotFound(n) => write!(f, "見つかりません: {n}"),
            Error::Exists(n) => write!(f, "zip にすでにあります: {n}"),
            Error::BadName(n) => write!(f, "zip に入れられない名前です: {n}"),
        }
    }
}

impl std::error::Error for Error {}

fn io_err(path: &Path, e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::NotFound {
        Error::NotFound(path.display().to_string())
    } else {
        Error::Io(format!("{}: {e}", path.display()))
    }
}

fn zip_err(path: &Path, e: ZipError) -> Error {
    match e {
        ZipError::Io(e) => io_err(path, e),
        ZipError::FileNotFound => Error::NotFound(path.display().to_string()),
        other => Error::Corrupt(format!("{}: {other}", path.display())),
    }
}

/// What to do when a name is already in the zip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnExisting {
    /// The new file takes the old one's place.
    #[default]
    Replace,
    /// Stop with [`Error::Exists`]; the zip is left as it was.
    Refuse,
}

/// One entry of a zip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The path inside, with `/`.
    pub name: String,
    /// Size when unpacked.
    pub size: u64,
    /// Size in the zip.
    pub packed: u64,
    pub modified: Option<SystemTime>,
    pub is_dir: bool,
}

/// What an add did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Added {
    /// Names written (new or replacing).
    pub added: Vec<String>,
    /// Names that were in the zip and are replaced.
    pub replaced: Vec<String>,
    /// Bytes of the added files before compression.
    pub bytes: u64,
}

/// A file to add: where it is, and the name it gets inside.
#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
    pub name: String,
}

impl Source {
    pub fn new(path: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        Source { path: path.into(), name: name.into() }
    }
}

/// `name` as a safe entry name: relative, `/` separated, no empty, `.` or `..` parts.
fn check_name(name: &str) -> Result<(), Error> {
    let bad = name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', '\0'])
        || name.split('/').any(|p| p.is_empty() || p == "." || p == "..")
        || name.split('/').next().is_some_and(|p| p.len() == 2 && p.ends_with(':'));
    if bad {
        Err(Error::BadName(name.to_string()))
    } else {
        Ok(())
    }
}

fn to_zip_time(t: SystemTime) -> DateTime {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86400, secs % 86400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = (yoe + era * 400 + i64::from(m <= 2)) as u16;
    DateTime::from_date_and_time(y, m, d, (rem / 3600) as u8, (rem % 3600 / 60) as u8, (rem % 60) as u8).unwrap_or_default()
}

fn from_zip_time(t: DateTime) -> Option<SystemTime> {
    let (y, m, d) = (i64::from(t.year()), i64::from(t.month()), i64::from(t.day()));
    // Days since 1970-01-01 from a civil date.
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86400 + i64::from(t.hour()) * 3600 + i64::from(t.minute()) * 60 + i64::from(t.second());
    u64::try_from(secs).ok().map(|s| UNIX_EPOCH + Duration::from_secs(s))
}

fn open_archive(zip: &Path) -> Result<ZipArchive<BufReader<File>>, Error> {
    let f = File::open(zip).map_err(|e| io_err(zip, e))?;
    ZipArchive::new(BufReader::new(f)).map_err(|e| zip_err(zip, e))
}

/// The entries of a zip, in the order they are stored.
pub fn list(zip: &Path) -> Result<Vec<Entry>, Error> {
    let mut a = open_archive(zip)?;
    let mut out = Vec::with_capacity(a.len());
    for i in 0..a.len() {
        let f = a.by_index_raw(i).map_err(|e| zip_err(zip, e))?;
        let name = f.name().map_err(|e| zip_err(zip, e))?.into_owned();
        out.push(Entry {
            name,
            size: f.size(),
            packed: f.compressed_size(),
            modified: f.last_modified().and_then(from_zip_time),
            is_dir: f.is_dir(),
        });
    }
    Ok(out)
}

/// One entry's content.
pub fn read(zip: &Path, name: &str) -> Result<Vec<u8>, Error> {
    let mut v = Vec::new();
    read_to(zip, name, &mut v)?;
    Ok(v)
}

/// One entry's content, written to `out`; returns the byte count. [`Error::NotFound`] if the name is not in the zip.
pub fn read_to(zip: &Path, name: &str, out: &mut dyn Write) -> Result<u64, Error> {
    let mut a = open_archive(zip)?;
    let mut f = match a.by_name(name) {
        Ok(f) => f,
        Err(ZipError::FileNotFound) => return Err(Error::NotFound(name.to_string())),
        Err(e) => return Err(zip_err(zip, e)),
    };
    io::copy(&mut f, out).map_err(|e| Error::Io(format!("{name}: {e}")))
}

/// Add files to the zip (made if it is not there). All or nothing: on any error the zip is untouched.
pub fn add(zip: &Path, files: &[Source], on: OnExisting) -> Result<Added, Error> {
    let mut seen = HashSet::new();
    for s in files {
        check_name(&s.name)?;
        if !seen.insert(s.name.as_str()) {
            return Err(Error::Exists(s.name.clone()));
        }
    }
    rewrite(
        zip,
        on,
        |w, opts| {
            let mut bytes = 0;
            for s in files {
                let f = File::open(&s.path).map_err(|e| io_err(&s.path, e))?;
                let meta = f.metadata().map_err(|e| io_err(&s.path, e))?;
                let o = opts.last_modified_time(meta.modified().map(to_zip_time).unwrap_or_default()).large_file(meta.len() >= 0xFFFF_FFFF);
                w.start_file(s.name.as_str(), o).map_err(|e| zip_err(zip, e))?;
                bytes += io::copy(&mut BufReader::new(f), w).map_err(|e| io_err(&s.path, e))?;
            }
            Ok(bytes)
        },
        files.iter().map(|s| s.name.as_str()).collect(),
    )
}

/// Add in-memory content: `(name, bytes)` pairs. Same rules as [`add`].
pub fn add_bytes(zip: &Path, items: &[(&str, &[u8])], on: OnExisting) -> Result<Added, Error> {
    let mut seen = HashSet::new();
    for (n, _) in items {
        check_name(n)?;
        if !seen.insert(*n) {
            return Err(Error::Exists((*n).to_string()));
        }
    }
    let now = to_zip_time(SystemTime::now());
    rewrite(
        zip,
        on,
        |w, opts| {
            let mut bytes = 0;
            for (n, data) in items {
                w.start_file(*n, opts.last_modified_time(now)).map_err(|e| zip_err(zip, e))?;
                w.write_all(data).map_err(|e| Error::Io(format!("{n}: {e}")))?;
                bytes += data.len() as u64;
            }
            Ok(bytes)
        },
        items.iter().map(|(n, _)| *n).collect(),
    )
}

/// Add every file under `dir` (subfolders too) as `<prefix>/<path under dir>`; an empty `prefix` puts them at the top.
/// Files come in name order. Empty folders are not stored. Same rules as [`add`].
pub fn add_dir(zip: &Path, dir: &Path, prefix: &str, on: OnExisting) -> Result<Added, Error> {
    let prefix = prefix.trim_matches('/');
    let mut files = Vec::new();
    walk(dir, prefix, &mut files)?;
    add(zip, &files, on)
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<Source>) -> Result<(), Error> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(|e| io_err(dir, e))?.collect::<Result<_, _>>().map_err(|e| io_err(dir, e))?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let inside = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let path = e.path();
        // `metadata` follows links: a link to a file is stored as that file, a link to a folder is walked.
        let meta = std::fs::metadata(&path).map_err(|e| io_err(&path, e))?;
        if meta.is_dir() {
            walk(&path, &inside, out)?;
        } else if meta.is_file() {
            out.push(Source { path, name: inside });
        }
    }
    Ok(())
}

/// Write `<zip>.part` = the old entries (minus the replaced) + what `put` writes, then rename it over the zip.
fn rewrite(
    zip: &Path,
    on: OnExisting,
    put: impl FnOnce(&mut ZipWriter<BufWriter<File>>, SimpleFileOptions) -> Result<u64, Error>,
    names: Vec<&str>,
) -> Result<Added, Error> {
    let mut old = if zip.exists() { Some(open_archive(zip)?) } else { None };
    let mut added = Added::default();
    let incoming: HashSet<&str> = names.iter().copied().collect();
    let mut keep = Vec::new();
    if let Some(a) = old.as_mut() {
        for i in 0..a.len() {
            let n = a.by_index_raw(i).map_err(|e| zip_err(zip, e))?.name().map_err(|e| zip_err(zip, e))?.into_owned();
            if incoming.contains(n.as_str()) {
                if on == OnExisting::Refuse {
                    return Err(Error::Exists(n));
                }
                added.replaced.push(n);
            } else {
                keep.push(i);
            }
        }
    }
    let part = {
        let mut s = zip.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s)
    };
    let work = || -> Result<(), Error> {
        let f = File::create(&part).map_err(|e| io_err(&part, e))?;
        let mut w = ZipWriter::new(BufWriter::new(f));
        if let Some(a) = old.as_mut() {
            for &i in &keep {
                let f = a.by_index_raw(i).map_err(|e| zip_err(zip, e))?;
                w.raw_copy_file(f).map_err(|e| zip_err(zip, e))?;
            }
        }
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        added.bytes = put(&mut w, opts)?;
        let mut out = w.finish().map_err(|e| zip_err(zip, e))?;
        out.flush().map_err(|e| io_err(&part, e))?;
        out.get_ref().sync_all().map_err(|e| io_err(&part, e))?;
        Ok(())
    };
    let done = work();
    drop(old);
    let renamed = done.and_then(|()| std::fs::rename(&part, zip).map_err(|e| io_err(zip, e)));
    if let Err(e) = renamed {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    added.added = names.into_iter().map(String::from).collect();
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-zip-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn adds_lists_and_reads_back() {
        let d = tmp("basic");
        let z = d.join("a.zip");
        let r =
            add_bytes(&z, &[("history/2026/11/01.toml", b"x = 1\n"), ("日本語/ログ.txt", "こんにちは".as_bytes())], OnExisting::Replace)
                .unwrap();
        assert_eq!(r.added.len(), 2);
        assert_eq!(r.bytes, 6 + 15);
        let l = list(&z).unwrap();
        assert_eq!(l.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["history/2026/11/01.toml", "日本語/ログ.txt"]);
        assert_eq!(l[0].size, 6);
        assert!(l[0].modified.unwrap() > UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        assert_eq!(read(&z, "日本語/ログ.txt").unwrap(), "こんにちは".as_bytes());
        assert_eq!(read(&z, "none"), Err(Error::NotFound("none".into())));
        assert!(!d.join("a.zip.part").exists());
    }

    #[test]
    fn a_second_add_keeps_the_first_and_replace_or_refuse_works() {
        let d = tmp("again");
        let z = d.join("a.zip");
        add_bytes(&z, &[("a.txt", b"one"), ("b.txt", b"two")], OnExisting::Replace).unwrap();
        let r = add_bytes(&z, &[("b.txt", b"TWO"), ("c.txt", b"three")], OnExisting::Replace).unwrap();
        assert_eq!(r.replaced, ["b.txt"]);
        assert_eq!(read(&z, "a.txt").unwrap(), b"one");
        assert_eq!(read(&z, "b.txt").unwrap(), b"TWO");
        assert_eq!(list(&z).unwrap().len(), 3);
        let before = std::fs::read(&z).unwrap();
        assert_eq!(add_bytes(&z, &[("a.txt", b"x"), ("d.txt", b"y")], OnExisting::Refuse), Err(Error::Exists("a.txt".into())));
        assert_eq!(std::fs::read(&z).unwrap(), before, "a refused add leaves the zip as it was");
        assert!(!d.join("a.zip.part").exists());
    }

    #[test]
    fn a_folder_goes_in_with_relative_names() {
        let d = tmp("dir");
        let src = d.join("logs");
        std::fs::create_dir_all(src.join("2026-11-01")).unwrap();
        std::fs::write(src.join("2026-11-01").join("job.log"), "line\n".repeat(1000)).unwrap();
        std::fs::write(src.join("top.log"), "t").unwrap();
        let z = d.join("a.zip");
        let r = add_dir(&z, &src, "logs", OnExisting::Replace).unwrap();
        assert_eq!(r.added, ["logs/2026-11-01/job.log", "logs/top.log"]);
        let l = list(&z).unwrap();
        assert!(l[0].packed < l[0].size, "deflated");
        assert_eq!(read(&z, "logs/top.log").unwrap(), b"t");
        let top = d.join("top.zip");
        add_dir(&top, &src, "", OnExisting::Replace).unwrap();
        assert!(read(&top, "top.log").is_ok());
    }

    #[test]
    fn unsafe_names_and_missing_files_are_refused_without_touching_the_zip() {
        let d = tmp("bad");
        let z = d.join("a.zip");
        for n in ["../x", "/x", "a/../b", "a\\b", "C:/x", "", "a//b", "./a"] {
            assert_eq!(add_bytes(&z, &[(n, b"x")], OnExisting::Replace), Err(Error::BadName(n.into())), "{n}");
        }
        assert!(!z.exists());
        let r = add(&z, &[Source::new(d.join("missing.txt"), "m.txt")], OnExisting::Replace);
        assert!(matches!(r, Err(Error::NotFound(_))), "{r:?}");
        assert!(!z.exists() && !d.join("a.zip.part").exists());
        assert!(matches!(list(&z), Err(Error::NotFound(_))));
    }

    #[test]
    fn a_damaged_zip_is_reported_and_kept() {
        let d = tmp("corrupt");
        let z = d.join("a.zip");
        std::fs::write(&z, b"this is not a zip file at all").unwrap();
        assert!(matches!(list(&z), Err(Error::Corrupt(_))));
        assert!(matches!(add_bytes(&z, &[("a", b"x")], OnExisting::Replace), Err(Error::Corrupt(_))));
        assert_eq!(std::fs::read(&z).unwrap(), b"this is not a zip file at all");
    }

    #[test]
    fn times_round_trip_through_the_dos_format() {
        let t = UNIX_EPOCH + Duration::from_secs(1_793_491_200 + 3600 * 5 + 61); // 2026-11-01 05:01:01 UTC
        let back = from_zip_time(to_zip_time(t)).unwrap();
        assert_eq!(back, UNIX_EPOCH + Duration::from_secs(1_793_491_200 + 3600 * 5 + 60), "seconds are kept in steps of 2");
    }
}
