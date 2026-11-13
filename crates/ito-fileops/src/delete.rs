use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use crate::error::{io_err, Doing, Error};
use crate::pat::{name_matches, Hit};
use crate::{list_sorted, plat, safe, stopped, Deleted, Emptied, End, Event, Skip};

/// Deletes the regular files in `folder` (not below it) whose name matches `pattern` and whose last-write
/// time is strictly before `now - older_than`. Links and special files are passed over (`Skipped`); folders
/// are ignored. A file whose time cannot be read is an error. The first failure ends the run with that path.
pub fn delete_old(
    folder: &Path,
    pattern: &str,
    older_than: Duration,
    now: SystemTime,
    stop: &AtomicBool,
    on: &mut dyn FnMut(Event),
) -> Deleted {
    let mut out = Deleted { paths: Vec::new(), end: End::Done };
    out.end = delete_old_in(folder, pattern, older_than, now, stop, on, &mut out.paths);
    out
}

fn delete_old_in(
    folder: &Path,
    pattern: &str,
    older_than: Duration,
    now: SystemTime,
    stop: &AtomicBool,
    on: &mut dyn FnMut(Event),
    paths: &mut Vec<PathBuf>,
) -> End {
    let folder = match safe::check_real_folder(folder, false) {
        Ok(f) => f,
        Err(e) => return End::Failed(e),
    };
    let entries = match list_sorted(&folder) {
        Ok(v) => v,
        Err(e) => return End::Failed(io_err(&folder, Doing::List, e)),
    };
    // Nothing is old enough when the cut-off is before the beginning of time.
    let Some(cutoff) = now.checked_sub(older_than) else { return End::Done };
    for entry in entries {
        if stopped(stop) {
            return End::Stopped;
        }
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            // It went away during the run.
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return End::Failed(io_err(&path, Doing::Read, e)),
        };
        if ft.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let hit = name_matches(Some(pattern), &name);
        if hit == Hit::No {
            continue;
        }
        if hit == Hit::NotUnicode {
            on(Event::Skipped { path: &path, why: Skip::NotUnicode });
            continue;
        }
        if ft.is_symlink() {
            on(Event::Skipped { path: &path, why: Skip::Link });
            continue;
        }
        if !ft.is_file() {
            on(Event::Skipped { path: &path, why: Skip::Special });
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => return End::Failed(io_err(&path, Doing::Read, e)),
        };
        let modified = match meta.modified() {
            Ok(t) => t,
            Err(e) => return End::Failed(io_err(&path, Doing::Read, e)),
        };
        if modified >= cutoff {
            continue;
        }
        if let Err(e) = remove_file(&path, &meta, on) {
            return End::Failed(e);
        }
        paths.push(path.clone());
        on(Event::Deleted(&path));
    }
    End::Done
}

/// Deletes one file. Windows: READONLY is cleared first and put back (best effort) when the delete fails.
fn remove_file(path: &Path, meta: &fs::Metadata, on: &mut dyn FnMut(Event)) -> Result<(), Error> {
    let cleared = plat::clear_readonly(path, meta).map_err(|e| io_err(path, Doing::SetAttributes, e))?;
    fs::remove_file(path).map_err(|e| {
        put_back(path, cleared, on);
        io_err(path, Doing::Delete, e)
    })
}

/// Sets READONLY again after a failed delete; a failure to do so is only reported as a cleanup failure.
fn put_back(path: &Path, cleared: bool, on: &mut dyn FnMut(Event)) {
    if cleared {
        if let Err(e) = plat::set_readonly(path) {
            on(Event::CleanupFailed { path, error: &e });
        }
    }
}

/// Removes everything inside `folder`, hidden and system items too, recursively; the folder stays. A link
/// inside is removed as a link. Refuses an empty path, a link, and the top of a drive or share. Returns the
/// number of top-level items removed. Windows: the read-only attribute is cleared on what is deleted.
/// Unix: no chmod at all (deleting depends on the parent folder's permissions).
pub fn empty(folder: &Path, stop: &AtomicBool, on: &mut dyn FnMut(Event)) -> Emptied {
    let mut out = Emptied { removed: 0, end: End::Done };
    out.end = empty_in(folder, stop, on, &mut out.removed);
    out
}

fn empty_in(folder: &Path, stop: &AtomicBool, on: &mut dyn FnMut(Event), removed: &mut usize) -> End {
    let folder = match safe::check_real_folder(folder, true) {
        Ok(f) => f,
        Err(e) => return End::Failed(e),
    };
    let entries = match list_sorted(&folder) {
        Ok(v) => v,
        Err(e) => return End::Failed(io_err(&folder, Doing::List, e)),
    };
    for entry in entries {
        if stopped(stop) {
            return End::Stopped;
        }
        let path = entry.path();
        if let Err(e) = remove_entry(&path, on) {
            return End::Failed(e);
        }
        *removed += 1;
        on(Event::Deleted(&path));
    }
    End::Done
}

/// One item: a link as a link, a folder with everything in it, anything else as a file.
fn remove_entry(path: &Path, on: &mut dyn FnMut(Event)) -> Result<(), Error> {
    let meta = fs::symlink_metadata(path).map_err(|e| io_err(path, Doing::Read, e))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        return plat::remove_link(path, &meta).map_err(|e| io_err(path, Doing::Delete, e));
    }
    if ft.is_dir() {
        // `remove_dir_all` removes links inside as links. When it fails (a read-only file on Windows, a
        // file in use) walk the rest ourselves, which also names the path that failed.
        return match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(_) => walk_remove(path, on),
        };
    }
    remove_file(path, &meta, on)
}

fn walk_remove(dir: &Path, on: &mut dyn FnMut(Event)) -> Result<(), Error> {
    let entries = list_sorted(dir).map_err(|e| io_err(dir, Doing::List, e))?;
    for entry in entries {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| io_err(&path, Doing::Read, e))?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            plat::remove_link(&path, &meta).map_err(|e| io_err(&path, Doing::Delete, e))?;
        } else if ft.is_dir() {
            walk_remove(&path, on)?;
        } else {
            remove_file(&path, &meta, on)?;
        }
    }
    let meta = fs::symlink_metadata(dir).map_err(|e| io_err(dir, Doing::Read, e))?;
    let cleared = plat::clear_readonly(dir, &meta).map_err(|e| io_err(dir, Doing::SetAttributes, e))?;
    fs::remove_dir(dir).map_err(|e| {
        put_back(dir, cleared, on);
        io_err(dir, Doing::Delete, e)
    })
}
