use std::ffi::{OsStr, OsString};
use std::fs::{self, File, FileTimes};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use sha2::{Digest, Sha256};

use crate::error::{io_err, unsafe_err, Doing, Error, Unsafe};
use crate::pat::{name_matches, Hit};
use crate::{list_sorted, plat, stopped, End, Event, Matched, Sent, Skip};

/// Copies the files that match `from` into the folder `into`. The last part of `from` may hold `*` and `?`
/// (without them it is that one file); only regular files match (a file symlink is copied as its content),
/// folders are passed over. `into` is made when missing (also when nothing matches). `found == 0` is "no
/// match". Each file goes through a unique staging file and a rename (see the crate doc); with `verify` the
/// source and the copy are hashed (SHA-256) and compared, and the hash is in [`Sent::sha256`]. A destination
/// that is a link is refused (`Unsafe::Link`), as is one that is the source itself (`Unsafe::SameFile`). The
/// first failure ends the run. The write time of each file is kept (Unix: the permission bits too).
pub fn copy_matching(from: &Path, into: &Path, verify: bool, stop: &AtomicBool, on: &mut dyn FnMut(Event)) -> Matched {
    let mut m = Matched { found: 0, sent: Vec::new(), end: End::Done };
    m.end = run(from, into, verify, stop, on, &mut m);
    m
}

fn run(from: &Path, into: &Path, verify: bool, stop: &AtomicBool, on: &mut dyn FnMut(Event), m: &mut Matched) -> End {
    for p in [from, into] {
        if p.as_os_str().is_empty() {
            return End::Failed(unsafe_err(p, Unsafe::Empty));
        }
    }
    let files = match snapshot(from, on) {
        Ok(v) => v,
        Err(e) => return End::Failed(e),
    };
    m.found = files.len();
    if let Err(e) = fs::create_dir_all(into) {
        return End::Failed(io_err(into, Doing::MakeFolder, e));
    }
    for (name, src) in files {
        if stopped(stop) {
            return End::Stopped;
        }
        match send_one(&src, &name, into, verify, stop, on) {
            Ok(Some(sent)) => {
                on(Event::Sent(&sent));
                m.sent.push(sent);
            }
            Ok(None) => return End::Stopped,
            Err(e) => return End::Failed(e),
        }
    }
    End::Done
}

/// The matching files, taken before anything is written (so a staging file cannot match mid-run).
fn snapshot(from: &Path, on: &mut dyn FnMut(Event)) -> Result<Vec<(OsString, PathBuf)>, Error> {
    let (Some(last), parent) = (from.file_name(), from.parent()) else { return Err(unsafe_err(from, Unsafe::Root)) };
    let dir = match parent {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let wild = last.to_str().is_some_and(ito_match::has_wildcard);
    if !wild {
        return match fs::metadata(from) {
            Ok(m) if m.is_file() => Ok(vec![(last.to_owned(), from.to_path_buf())]),
            Ok(_) => Ok(Vec::new()),
            // The file is not there: that is "no match" only when its folder is. Unix says ENOTDIR (not
            // ENOENT) when the folder is a file; Windows says "path not found" for both.
            Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => match fs::metadata(dir) {
                Ok(d) if d.is_dir() => Ok(Vec::new()),
                Ok(_) => Err(io_err(dir, Doing::List, io::Error::new(io::ErrorKind::NotADirectory, "not a folder"))),
                Err(de) => Err(io_err(dir, Doing::List, de)),
            },
            Err(e) => Err(io_err(from, Doing::Read, e)),
        };
    }
    let pattern = last.to_str();
    let mut out = Vec::new();
    for e in list_sorted(dir).map_err(|e| io_err(dir, Doing::List, e))? {
        let name = e.file_name();
        let path = e.path();
        let ft = match e.file_type() {
            Ok(t) => t,
            // It went away during the listing.
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(io_err(&path, Doing::List, err)),
        };
        if ft.is_dir() {
            continue;
        }
        match name_matches(pattern, &name) {
            Hit::No => continue,
            Hit::NotUnicode => {
                on(Event::Skipped { path: &path, why: Skip::NotUnicode });
                continue;
            }
            Hit::Yes => {}
        }
        let regular = if ft.is_symlink() { fs::metadata(&path).is_ok_and(|m| m.is_file()) } else { ft.is_file() };
        if regular {
            out.push((name, path));
        } else if !ft.is_symlink() {
            on(Event::Skipped { path: &path, why: Skip::Special });
        } else {
            // A folder link, or a link that points nowhere.
            on(Event::Skipped { path: &path, why: Skip::Link });
        }
    }
    Ok(out)
}

/// One file. `Ok(None)` is a stop (the staging file is removed, the target is as it was).
fn send_one(
    src: &Path,
    name: &OsStr,
    into: &Path,
    verify: bool,
    stop: &AtomicBool,
    on: &mut dyn FnMut(Event),
) -> Result<Option<Sent>, Error> {
    let target = into.join(name);
    if let Ok(m) = fs::symlink_metadata(&target) {
        if m.file_type().is_symlink() {
            return Err(unsafe_err(&target, Unsafe::Link));
        }
        if plat::same_file(src, &target) {
            return Err(unsafe_err(&target, Unsafe::SameFile));
        }
    }
    let (file, staging) = create_staging(into, name).map_err(|e| io_err(&target, Doing::Copy, e))?;
    let r = stage_and_swap(src, &target, &staging, file, verify, stop, on);
    if !matches!(r, Ok(Some(_))) {
        if let Err(e) = fs::remove_file(&staging) {
            if e.kind() != io::ErrorKind::NotFound {
                on(Event::CleanupFailed { path: &staging, error: &e });
            }
        }
    }
    r.map(|done| done.map(|(bytes, sha256)| Sent { name: name.to_owned(), bytes, sha256 }))
}

/// A new file `<into>/.<name>.<16 hex>.part`; another name when one is taken.
fn create_staging(into: &Path, name: &OsStr) -> io::Result<(File, PathBuf)> {
    let mut last = None;
    for _ in 0..32 {
        let mut s = OsString::from(".");
        s.push(name);
        s.push(format!(".{:016x}.part", random()));
        let path = into.join(s);
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::from(io::ErrorKind::AlreadyExists)))
}

fn random() -> u64 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.finish()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Writes the source into the staging file (hashing when `verify`), checks it, and renames it over the target.
/// Returns the byte count and, with `verify`, the hash; `None` when the stop flag was seen (checked after
/// every chunk, of the copy and of the re-read, and right before the rename).
fn stage_and_swap(
    src: &Path,
    target: &Path,
    staging: &Path,
    mut out: File,
    verify: bool,
    stop: &AtomicBool,
    on: &mut dyn FnMut(Event),
) -> Result<Option<(u64, Option<String>)>, Error> {
    let mut input = File::open(src).map_err(|e| io_err(src, Doing::Read, e))?;
    let before = input.metadata().map_err(|e| io_err(src, Doing::Read, e))?;
    let mut hasher = verify.then(Sha256::new);
    let mut buf = vec![0u8; CHUNK];
    let mut total = 0u64;
    loop {
        let n = input.read(&mut buf).map_err(|e| io_err(src, Doing::Read, e))?;
        if n == 0 {
            break;
        }
        if let Some(h) = hasher.as_mut() {
            h.update(&buf[..n]);
        }
        out.write_all(&buf[..n]).map_err(|e| io_err(target, Doing::Copy, e))?;
        total += n as u64;
        #[cfg(test)]
        hook::stop_at(1, stop);
        if stopped(stop) {
            return Ok(None);
        }
    }
    #[cfg(test)]
    if hook::CHANGE_SOURCE.with(|c| c.get()) {
        let _ = fs::OpenOptions::new().append(true).open(src).and_then(|mut f| f.write_all(b"x"));
    }
    if verify {
        let handle_now = input.metadata().map_err(|e| io_err(src, Doing::Read, e))?;
        let path_now = fs::metadata(src).map_err(|e| io_err(src, Doing::Read, e))?;
        let moved = |m: &fs::Metadata| m.len() != before.len() || m.modified().ok() != before.modified().ok();
        if total != before.len() || moved(&handle_now) || moved(&path_now) {
            return Err(Error::SourceChanged { path: src.to_path_buf() });
        }
    }
    drop(input);
    if let Ok(t) = before.modified() {
        out.set_times(FileTimes::new().set_modified(t)).map_err(|e| io_err(target, Doing::SetTime, e))?;
    }
    #[cfg(unix)]
    out.set_permissions(before.permissions()).map_err(|e| io_err(target, Doing::SetAttributes, e))?;
    if verify {
        out.sync_all().map_err(|e| io_err(target, Doing::Sync, e))?;
    }
    drop(out);
    let mut sha = None;
    if verify {
        #[cfg(test)]
        if hook::CORRUPT_STAGING.with(|c| c.get()) {
            let _ = fs::OpenOptions::new().append(true).open(staging).and_then(|mut f| f.write_all(b"x"));
        }
        let from = hex(&hasher.take().map(Digest::finalize).unwrap_or_default());
        let Some(to) = hash_file(staging, stop).map_err(|e| io_err(staging, Doing::Hash, e))? else { return Ok(None) };
        if from != to {
            return Err(Error::Mismatch { path: target.to_path_buf(), from, to });
        }
        sha = Some(from);
    }
    #[cfg(test)]
    hook::stop_at(3, stop);
    if stopped(stop) {
        return Ok(None);
    }
    // A read-only target is made writable for the swap; it is put back if the rename does not happen.
    let lifted = match fs::symlink_metadata(target) {
        Ok(m) if !m.file_type().is_symlink() => plat::clear_readonly(target, &m).map_err(|e| io_err(target, Doing::SetAttributes, e))?,
        _ => false,
    };
    if let Err(e) = fs::rename(staging, target) {
        if lifted {
            if let Err(back) = plat::set_readonly(target) {
                on(Event::CleanupFailed { path: target, error: &back });
            }
        }
        return Err(io_err(target, Doing::Rename, e));
    }
    plat::sync_parent(target);
    Ok(Some((total, sha)))
}

const CHUNK: usize = 64 * 1024;

/// The SHA-256 (hex) of a file; `None` when the stop flag was seen after a chunk.
fn hash_file(path: &Path, stop: &AtomicBool) -> io::Result<Option<String>> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(Some(hex(&h.finalize())));
        }
        h.update(&buf[..n]);
        #[cfg(test)]
        hook::stop_at(2, stop);
        if stopped(stop) {
            return Ok(None);
        }
    }
}

#[cfg(test)]
pub(crate) mod hook {
    use std::cell::Cell;
    thread_local! {
        /// Appends a byte to the staged file before it is read back.
        pub static CORRUPT_STAGING: Cell<bool> = const { Cell::new(false) };
        /// Appends a byte to the source after it was read.
        pub static CHANGE_SOURCE: Cell<bool> = const { Cell::new(false) };
        /// Sets the stop flag at a point of the copy: 1 after the first chunk written, 2 after the first chunk
        /// read back for the hash, 3 right before the rename. 0 is off.
        pub static STOP_AT: Cell<u8> = const { Cell::new(0) };
    }

    /// Sets `stop` when the hook is armed for `point`.
    pub fn stop_at(point: u8, stop: &std::sync::atomic::AtomicBool) {
        if STOP_AT.with(|c| c.get()) == point {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
