use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, FileType, Metadata};
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::error::{io_err, unsafe_err, Doing, Unsafe};
use crate::pat::{name_matches, Hit};
use crate::{list_sorted, plat, safe, stopped, wait_polling, End, Event, Skip, TreeOptions, TreeResult, COPIED, EXTRA, FAILED, MISMATCH};

/// Copies `from` into `into` like `robocopy <from> <into> [<pattern>] /E /R:1 /W:1`: recursive, empty folders
/// too, files only in `into` kept, a file with the same size and write time skipped, a failed file tried
/// again after a wait and then counted (the copy goes on). `pattern` filters file names only; folders are
/// still walked and made. `from` must be a folder; `into` is made when missing. See the crate doc for links,
/// overlap and what a copy keeps. The result's [`TreeResult::robocopy_code`] is robocopy's exit code.
pub fn copy_tree(
    from: &Path,
    into: &Path,
    pattern: Option<&str>,
    opts: &TreeOptions,
    stop: &AtomicBool,
    on: &mut dyn FnMut(Event),
) -> TreeResult {
    let mut cx = Cx { pattern, opts, stop, on, res: TreeResult::default(), halted: false };
    if let Err(e) = start(from, into) {
        cx.res.end = End::Failed(e);
        return cx.res;
    }
    cx.walk(from, into);
    if cx.halted {
        cx.res.end = End::Stopped;
    }
    cx.res
}

/// Checks the roots and makes `into`.
fn start(from: &Path, into: &Path) -> Result<(), crate::Error> {
    for p in [from, into] {
        if p.as_os_str().is_empty() {
            return Err(unsafe_err(p, Unsafe::Empty));
        }
    }
    let meta = fs::metadata(from).map_err(|e| io_err(from, Doing::List, e))?;
    if !meta.is_dir() {
        return Err(io_err(from, Doing::List, io::Error::new(io::ErrorKind::NotADirectory, "not a folder")));
    }
    let real_from = from.canonicalize().map_err(|e| io_err(from, Doing::List, e))?;
    let real_into = safe::canonical_lenient(into).map_err(|e| io_err(into, Doing::MakeFolder, e))?;
    if safe::overlap(&real_from, &real_into) {
        return Err(unsafe_err(into, Unsafe::Overlap));
    }
    fs::create_dir_all(into).map_err(|e| io_err(into, Doing::MakeFolder, e))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Dir,
    File,
    Link,
    Other,
}

fn kind_of(t: &FileType) -> Kind {
    if t.is_symlink() {
        Kind::Link
    } else if t.is_dir() {
        Kind::Dir
    } else if t.is_file() {
        Kind::File
    } else {
        Kind::Other
    }
}

/// The kind of a destination entry; one whose type cannot be read is left alone as `Other`.
fn dst_kind(e: &fs::DirEntry) -> Kind {
    e.file_type().map_or(Kind::Other, |t| kind_of(&t))
}

/// The key two names are the same under: case folded on Windows. A name that is not valid Unicode is its own
/// key (unchanged), so it can never be the same as another name.
pub(crate) fn key(name: &OsStr) -> OsString {
    match name.to_str() {
        Some(s) if cfg!(windows) => OsString::from(s.to_lowercase()),
        _ => name.to_owned(),
    }
}

struct Cx<'a> {
    pattern: Option<&'a str>,
    opts: &'a TreeOptions,
    stop: &'a AtomicBool,
    on: &'a mut dyn FnMut(Event),
    res: TreeResult,
    halted: bool,
}

impl Cx<'_> {
    fn halt_if_stopped(&mut self) -> bool {
        if !self.halted && stopped(self.stop) {
            self.halted = true;
        }
        self.halted
    }

    fn failed(&mut self, path: &Path, e: &io::Error) {
        self.res.failed += 1;
        self.res.flags |= FAILED;
        (self.on)(Event::Failed { path, error: e });
    }

    fn mismatch(&mut self, from: &Path, to: &Path) {
        self.res.mismatched += 1;
        self.res.flags |= MISMATCH;
        (self.on)(Event::Mismatch { from, to });
    }

    fn skipped(&mut self, path: &Path, why: Skip) {
        self.res.skipped += 1;
        (self.on)(Event::Skipped { path, why });
    }

    fn walk(&mut self, src: &Path, dst: &Path) {
        if self.halt_if_stopped() {
            return;
        }
        let src_entries = match list_sorted(src) {
            Ok(v) => v,
            Err(e) => return self.failed(src, &e),
        };
        let dst_entries = match list_sorted(dst) {
            Ok(v) => v,
            Err(e) => return self.failed(dst, &e),
        };
        let dst_kinds: HashMap<OsString, Kind> = dst_entries.iter().map(|e| (key(&e.file_name()), dst_kind(e))).collect();
        let src_keys: HashSet<OsString> = src_entries.iter().map(|e| key(&e.file_name())).collect();

        for e in &src_entries {
            if self.halt_if_stopped() {
                return;
            }
            let name = e.file_name();
            let (sp, dp) = (e.path(), dst.join(&name));
            let dk = dst_kinds.get(&key(&name)).copied();
            let ft = match e.file_type() {
                Ok(t) => t,
                // It went away during the run.
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    self.failed(&sp, &err);
                    continue;
                }
            };
            match kind_of(&ft) {
                Kind::Dir => self.dir(&sp, &dp, dk),
                Kind::File => match e.metadata() {
                    Ok(m) => self.file(&sp, &dp, &name, &m, dk),
                    Err(err) => self.failed(&sp, &err),
                },
                Kind::Link => match fs::metadata(&sp) {
                    Ok(m) if m.is_file() => self.file(&sp, &dp, &name, &m, dk),
                    // A folder link or junction, or one that points nowhere.
                    _ => self.skipped(&sp, Skip::Link),
                },
                Kind::Other => match name_matches(self.pattern, &name) {
                    Hit::Yes => self.skipped(&sp, Skip::Special),
                    Hit::NotUnicode => self.skipped(&sp, Skip::NotUnicode),
                    Hit::No => {}
                },
            }
        }
        if self.halted {
            return;
        }
        for e in &dst_entries {
            let name = e.file_name();
            if src_keys.contains(&key(&name)) {
                continue;
            }
            let p = e.path();
            let dir_like = match dst_kind(e) {
                Kind::Dir => true,
                Kind::Link => fs::metadata(&p).is_ok_and(|m| m.is_dir()),
                _ => false,
            };
            if dir_like {
                self.res.extra_dirs += 1;
                self.res.flags |= EXTRA;
                (self.on)(Event::ExtraDir(&p));
            } else if name_matches(self.pattern, &name) == Hit::Yes {
                self.res.extra_files += 1;
                self.res.flags |= EXTRA;
                (self.on)(Event::ExtraFile(&p));
            }
        }
    }

    fn dir(&mut self, sp: &Path, dp: &Path, dk: Option<Kind>) {
        match dk {
            Some(Kind::Dir) => self.walk(sp, dp),
            None => {
                if let Err(e) = fs::create_dir(dp) {
                    return self.failed(dp, &e);
                }
                self.res.folders_made += 1;
                (self.on)(Event::MadeFolder(dp));
                self.walk(sp, dp);
                if let Ok(m) = fs::metadata(sp) {
                    if let Err(e) = plat::set_dir_attributes(dp, &m) {
                        self.failed(dp, &e);
                    }
                }
            }
            Some(_) => self.mismatch(sp, dp),
        }
    }

    fn file(&mut self, sp: &Path, dp: &Path, name: &OsStr, meta: &Metadata, dk: Option<Kind>) {
        match name_matches(self.pattern, name) {
            Hit::No => return,
            Hit::NotUnicode => return self.skipped(sp, Skip::NotUnicode),
            Hit::Yes => {}
        }
        match dk {
            None => {}
            Some(Kind::File) => {
                if let Ok(dm) = fs::metadata(dp) {
                    if same(meta, &dm, self.opts.time_tolerance) {
                        self.res.same += 1;
                        (self.on)(Event::Same(dp));
                        return;
                    }
                }
            }
            Some(_) => return self.mismatch(sp, dp),
        }
        self.copy_with_retries(sp, dp, meta);
    }

    fn copy_with_retries(&mut self, sp: &Path, dp: &Path, meta: &Metadata) {
        let mut attempt = 0u32;
        loop {
            #[cfg(test)]
            hook::before_write(dp);
            // The destination as it is now, not as it was listed: a link made since is not written through.
            if plat::exists_not_regular(dp) {
                return self.mismatch(sp, dp);
            }
            let err = match self.copy_file(sp, dp, meta) {
                Ok(bytes) => {
                    self.res.copied += 1;
                    self.res.bytes += bytes;
                    self.res.flags |= COPIED;
                    (self.on)(Event::Copied { from: sp, to: dp, bytes });
                    return;
                }
                Err(e) => e,
            };
            if attempt >= self.opts.retries {
                return self.failed(sp, &err);
            }
            attempt += 1;
            (self.on)(Event::Retrying { path: sp, attempt, error: &err });
            if wait_polling(self.stop, self.opts.wait) {
                self.halted = true;
                return;
            }
        }
    }

    fn copy_file(&mut self, sp: &Path, dp: &Path, meta: &Metadata) -> io::Result<u64> {
        let lifted = plat::clear_dst_readonly(dp)?;
        let r = fs::copy(sp, dp).and_then(|n| plat::set_times(dp, meta).map(|()| n));
        if let (true, Some(l)) = (r.is_err(), &lifted) {
            if let Err(e) = plat::restore_dst(dp, l) {
                (self.on)(Event::CleanupFailed { path: dp, error: &e });
            }
        }
        r
    }
}

/// Equal size and write time (within `tolerance`).
fn same(a: &Metadata, b: &Metadata, tolerance: Duration) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let (Ok(x), Ok(y)) = (a.modified(), b.modified()) else { return false };
    let gap = x.duration_since(y).or_else(|_| y.duration_since(x)).unwrap_or(Duration::MAX);
    gap <= tolerance
}

#[cfg(test)]
pub(crate) mod hook {
    use std::cell::RefCell;
    use std::path::Path;
    type Hook = Box<dyn FnMut(&Path)>;
    thread_local! {
        /// Runs right before each file is written, to change the destination after it was listed.
        pub static BEFORE_WRITE: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }
    pub fn before_write(dst: &Path) {
        BEFORE_WRITE.with(|h| {
            if let Some(f) = h.borrow_mut().as_mut() {
                f(dst);
            }
        });
    }
}
