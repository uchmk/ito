//! Local file chores without a shell: delete old files, empty a folder, copy a tree like
//! `robocopy <from> <into> /E`, and copy matching files into a folder with a SHA-256 check.
//!
//! Everything here blocks (disks, shares): call it from a worker thread or a child process. A stop flag is
//! checked between files and inside retry waits; [`empty`] removes one top-level item to its end (a big
//! subfolder too), [`copy_tree`] copies one file to its end, and [`copy_matching`] looks at the flag inside
//! each file's copy (after every chunk written and read back, and before the rename). Each operation returns
//! a summary that says how it ended ([`End`]), so what was done is known even when the run stopped or failed;
//! each step is also passed to the caller's `on` function as an [`Event`], for a log.
//!
//! # Links and roots
//!
//! * [`delete_old`] and [`empty`] never follow a link out of the folder. A symlink or junction inside is
//!   removed as a link; what it points to is untouched. The folder named by the caller must itself be a real
//!   folder, not a link (`Unsafe::Link`). [`empty`] also refuses the top of a drive or share (`Unsafe::Root`);
//!   [`delete_old`] allows it (files only, old ones only).
//! * The folder is read as `Path::components` reads it: a trailing separator, `.` and repeated separators are
//!   dropped before the link check (on Unix `link/` and `link/.` would make the check see the target), and the
//!   paths in the summary and the events use that form (`dir/./x` is `dir/x`). Only the last name is checked:
//!   links earlier in the path are resolved as the OS does, so on Unix `link/..` is the parent of the link's
//!   target.
//! * [`copy_tree`] follows the two roots the caller names, and refuses roots that are the same folder or one
//!   inside the other (`Unsafe::Overlap`). Inside the tree a file symlink is copied as its content (as
//!   robocopy does), a folder symlink or junction is not followed (`Skip::Link`; robocopy would follow it by
//!   default). A link inside `into` is never written through: it is left alone and counted as a mismatch. Links
//!   inside `into` are checked when the folder is listed and again right before each write; a link someone makes
//!   in between those two looks is not guarded (see the last bullet).
//! * [`copy_matching`] refuses a destination **file** `<into>/<name>` that is a link, and one that is the source
//!   itself. The folder `into` is followed like a root (it may be a junction).
//! * Nothing protects against someone else changing the roots or their parents during a run.
//!
//! # Wildcards
//!
//! `*` and `?` against the file name only. Case-insensitive on Windows, case-sensitive elsewhere. On Windows a
//! pattern ending in `.*` also takes a name without a dot that matches the part before it (`a.*` takes `a`,
//! `*.*` takes every file), as `FindFirstFile` does. 8.3 short names are not consulted (`*.htm` does not
//! take `x.html`). A name that is not valid Unicode matches only `*` (or no pattern); with any other pattern
//! it is skipped (`Skip::NotUnicode`).
//!
//! # What a copy keeps ([`copy_tree`])
//!
//! Like robocopy's default `/COPY:DAT /DCOPY:DA`. Windows: data, attributes and alternate streams
//! (`CopyFileExW`), then creation, access and write time from the source; a folder made gets the source
//! folder's attributes, no times. Unix: content and permission bits, then access and write time; no owner,
//! ACL or extended attributes. Two files are the same when size and last-write time are equal, exactly
//! (`TreeOptions::time_tolerance` widens that for FAT or some shares); attribute-only differences are not
//! copied. [`copy_matching`] keeps only the write time (and on Unix the permission bits) of each file: on
//! Windows the new file is made fresh, so it has no READONLY (a read-only target is replaced by a writable
//! file) and the source's attributes are not carried.
//!
//! [`copy_tree`] writes each file in place like robocopy (`std::fs::copy`). A copy that fails part-way (disk
//! full, a read error) can leave that destination file partly written (the next run copies it again, its size
//! or time differs). A destination file that is a hard link shares its data with its other names, which see the
//! new content. Use [`copy_matching`] when a target must be replaced whole. A single file's copy runs to its
//! end; `stop` is checked between files and between retries.
//!
//! # [`empty`]
//!
//! On Windows mount points and junctions are removed as links. On Unix mount points inside the folder are not
//! detected (std `remove_dir_all`, like `rm -rf` without `--one-file-system`): a file system mounted inside the
//! folder is emptied too. The fallback walk (after `remove_dir_all` fails on a read-only or busy item) goes by
//! path, so it is not safe against someone swapping a folder for a link meanwhile (the same "nobody else
//! changes it" rule as for the roots).
//!
//! # [`copy_matching`] and `verify`
//!
//! A file is written to a unique sibling `.<name>.<16 hex>.part` and renamed over the target, so a failed copy
//! never leaves a half-written target. With `verify` the source is read once while hashing, the staged file
//! is read back and hashed again, and the two SHA-256 are compared; a source whose size or write time changed
//! meanwhile is `SourceChanged`. The hash covers the main data stream only. Concurrent writers need their
//! own locking. On SMB there is no promise of crash durability.

mod delete;
mod error;
mod matching;
mod pat;
mod plat;
mod safe;
mod tree;

#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub use delete::{delete_old, empty};
pub use error::{Doing, Error, Unsafe};
pub use matching::copy_matching;
pub use tree::copy_tree;

/// Why a path was passed over.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// A symlink or junction that was not followed (`delete_old` leaves links alone).
    Link,
    /// A FIFO, socket or device.
    Special,
    /// A name that is not valid Unicode, with a pattern that is not just `*`.
    NotUnicode,
}

/// Each step as it happens, for the caller's log.
#[non_exhaustive]
#[derive(Clone, Copy, Debug)]
pub enum Event<'a> {
    /// `delete_old`: a file; `empty`: a top-level item.
    Deleted(&'a Path),
    Copied {
        from: &'a Path,
        to: &'a Path,
        bytes: u64,
    },
    /// `copy_tree`: skipped, same size and time.
    Same(&'a Path),
    /// `copy_tree`: a file only in `into` (kept).
    ExtraFile(&'a Path),
    /// `copy_tree`: a folder only in `into` (kept, not walked).
    ExtraDir(&'a Path),
    /// `copy_tree`: a file on one side, a folder or link on the other (left alone).
    Mismatch {
        from: &'a Path,
        to: &'a Path,
    },
    /// `copy_tree`: a folder made in `into`.
    MadeFolder(&'a Path),
    /// `copy_tree`: a file failed, `attempt` (1 for the first retry) is about to be made after the wait.
    Retrying {
        path: &'a Path,
        attempt: u32,
        error: &'a io::Error,
    },
    /// `copy_tree`: a file or folder failed after its retries; the copy goes on, as robocopy does.
    Failed {
        path: &'a Path,
        error: &'a io::Error,
    },
    Skipped {
        path: &'a Path,
        why: Skip,
    },
    /// `copy_matching`: one file done.
    Sent(&'a Sent),
    /// A step that only tidies (restoring read-only, removing a staging file) failed; the main result is unchanged.
    CleanupFailed {
        path: &'a Path,
        error: &'a io::Error,
    },
}

/// How an operation ended.
#[derive(Debug, Default)]
pub enum End {
    /// All of it was done.
    #[default]
    Done,
    /// The stop flag was set; what was done is in the summary.
    Stopped,
    /// The operation could not go on.
    Failed(Error),
}

impl End {
    pub fn error(&self) -> Option<&Error> {
        match self {
            End::Failed(e) => Some(e),
            _ => None,
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self, End::Done)
    }
}

/// [`delete_old`]: each path deleted, in name order.
#[derive(Debug)]
pub struct Deleted {
    pub paths: Vec<PathBuf>,
    pub end: End,
}

/// [`empty`]: how many top-level items were removed.
#[derive(Debug)]
pub struct Emptied {
    pub removed: usize,
    pub end: End,
}

/// One file of [`copy_matching`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    pub name: OsString,
    pub bytes: u64,
    /// With `verify`: the SHA-256 (hex) of the source, which the copy was found to equal.
    pub sha256: Option<String>,
}

/// [`copy_matching`]: `found == 0` is "no match".
#[derive(Debug)]
pub struct Matched {
    pub found: usize,
    pub sent: Vec<Sent>,
    pub end: End,
}

/// How [`copy_tree`] retries and compares.
#[derive(Clone, Debug)]
pub struct TreeOptions {
    /// More tries for a failed file (robocopy `/R:1`).
    pub retries: u32,
    /// Wait before each retry (robocopy `/W:1`); the wait polls the stop flag.
    pub wait: Duration,
    /// Two write times this close count as equal (robocopy `/FFT` is 2 s). Default 0.
    pub time_tolerance: Duration,
}

impl Default for TreeOptions {
    fn default() -> Self {
        TreeOptions { retries: 1, wait: Duration::from_secs(1), time_tolerance: Duration::ZERO }
    }
}

/// What [`copy_tree`] did.
#[derive(Debug, Default)]
pub struct TreeResult {
    /// Files copied.
    pub copied: u64,
    /// Files skipped as the same.
    pub same: u64,
    /// Files only in `into` that match the pattern (kept).
    pub extra_files: u64,
    /// Folders only in `into` (kept).
    pub extra_dirs: u64,
    /// Type collisions and links in `into`, left alone.
    pub mismatched: u64,
    /// Files (and folders) that failed after their retries.
    pub failed: u64,
    /// Links and special files, and names that are not Unicode, passed over.
    pub skipped: u64,
    pub folders_made: u64,
    /// Bytes copied.
    pub bytes: u64,
    pub(crate) flags: u8,
    pub end: End,
}

impl TreeResult {
    /// What robocopy's exit code would say: 0x01 a file copied, 0x02 an extra file or folder, 0x04 a mismatch,
    /// 0x08 a failure, and 0x10 when the copy could not start or go on (a missing source, an `into` that cannot
    /// be made, a refused root).
    pub fn robocopy_code(&self) -> i32 {
        i32::from(self.flags) | if matches!(self.end, End::Failed(_)) { 0x10 } else { 0 }
    }
}

pub(crate) const COPIED: u8 = 0x01;
pub(crate) const EXTRA: u8 = 0x02;
pub(crate) const MISMATCH: u8 = 0x04;
pub(crate) const FAILED: u8 = 0x08;

pub(crate) fn stopped(stop: &AtomicBool) -> bool {
    stop.load(Ordering::Relaxed)
}

/// Sleeps `wait`, looking at the stop flag often. True when stopped.
pub(crate) fn wait_polling(stop: &AtomicBool, wait: Duration) -> bool {
    let step = Duration::from_millis(20);
    let mut left = wait;
    while !left.is_zero() {
        if stopped(stop) {
            return true;
        }
        let d = left.min(step);
        std::thread::sleep(d);
        left -= d;
    }
    stopped(stop)
}

/// The entries of a folder in name order, so a run is the same every time.
pub(crate) fn list_sorted(dir: &Path) -> io::Result<Vec<std::fs::DirEntry>> {
    let mut v = std::fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    v.sort_by_key(|e| e.file_name());
    Ok(v)
}
