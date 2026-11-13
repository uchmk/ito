//! Checks on the paths a caller names, before anything is listed or deleted.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::{io_err, unsafe_err, Doing, Error, Unsafe};

/// Whether the path, read as text only, is the top of a file system or a share: `/`, `C:\`, `C:`,
/// `\\?\C:\`, `\\server\share`, also `/a/..`. Nothing is looked up.
pub(crate) fn is_root(path: &Path) -> bool {
    let (mut anchored, mut names) = (false, 0usize);
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => anchored = true,
            Component::Normal(_) => names += 1,
            Component::ParentDir => names = names.saturating_sub(1),
            Component::CurDir => {}
        }
    }
    anchored && names == 0
}

/// The text-only part of the checks for a folder to be emptied: not empty, not a root.
pub(crate) fn check_empty_target(path: &Path) -> Result<(), Unsafe> {
    if path.as_os_str().is_empty() {
        Err(Unsafe::Empty)
    } else if is_root(path) {
        Err(Unsafe::Root)
    } else {
        Ok(())
    }
}

/// The folder named by the caller must be a real folder, not a link that `read_dir` would follow.
/// With `refuse_root` it is also not a root (also after resolving the real path).
pub(crate) fn check_real_folder(folder: &Path, refuse_root: bool) -> Result<(), Error> {
    if refuse_root {
        check_empty_target(folder).map_err(|why| unsafe_err(folder, why))?;
    } else if folder.as_os_str().is_empty() {
        return Err(unsafe_err(folder, Unsafe::Empty));
    }
    let meta = fs::symlink_metadata(folder).map_err(|e| io_err(folder, Doing::List, e))?;
    if meta.file_type().is_symlink() {
        return Err(unsafe_err(folder, Unsafe::Link));
    }
    if refuse_root {
        let real = folder.canonicalize().map_err(|e| io_err(folder, Doing::List, e))?;
        if is_root(&real) {
            return Err(unsafe_err(folder, Unsafe::Root));
        }
    }
    Ok(())
}

/// Lower-cased on Windows, where paths are not case-sensitive.
fn fold(p: &Path) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(p.to_string_lossy().to_lowercase())
    } else {
        p.to_path_buf()
    }
}

/// Whether two (canonical) paths are the same folder or one is inside the other.
pub(crate) fn overlap(a: &Path, b: &Path) -> bool {
    let (a, b) = (fold(a), fold(b));
    a.starts_with(&b) || b.starts_with(&a)
}

/// The real path of `p`; a path that does not exist yet gets its nearest existing parent resolved and the
/// missing names appended.
pub(crate) fn canonical_lenient(p: &Path) -> io::Result<PathBuf> {
    let first = match p.canonicalize() {
        Ok(c) => return Ok(c),
        Err(e) if e.kind() == io::ErrorKind::NotFound => e,
        Err(e) => return Err(e),
    };
    let mut rest: Vec<OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        let Some(name) = cur.file_name() else { return Err(first) };
        rest.push(name.to_owned());
        cur.pop();
        let base = if cur.as_os_str().is_empty() { PathBuf::from(".") } else { cur.clone() };
        if let Ok(mut out) = base.canonicalize() {
            out.extend(rest.iter().rev());
            return Ok(out);
        }
    }
}
