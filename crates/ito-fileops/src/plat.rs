//! What differs between Windows and Unix.

use std::fs::{self, File, FileTimes, Metadata};
use std::io;
use std::path::Path;

/// Removes a link (a symlink or a junction) as a link; what it points to is not touched.
pub(crate) fn remove_link(path: &Path, meta: &Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if meta.file_type().is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    let _ = meta;
    fs::remove_file(path)
}

/// Windows: clears only the read-only attribute of a file or folder (never call it on a link).
/// Returns whether it was set. Unix: nothing (deleting depends on the parent folder).
pub(crate) fn clear_readonly(path: &Path, meta: &Metadata) -> io::Result<bool> {
    #[cfg(windows)]
    {
        if meta.permissions().readonly() {
            let mut p = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)] // Windows only: this clears the attribute
            p.set_readonly(false);
            fs::set_permissions(path, p)?;
            return Ok(true);
        }
    }
    let _ = (path, meta);
    Ok(false)
}

/// Windows: sets the read-only attribute again (best effort, after a failed copy).
pub(crate) fn set_readonly(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let mut p = fs::metadata(path)?.permissions();
        p.set_readonly(true);
        fs::set_permissions(path, p)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}

/// What to put back on a destination file whose write protection was lifted for a copy.
pub(crate) struct Restore {
    /// Unix: the permission bits it had.
    #[cfg(unix)]
    mode: u32,
}

/// True when something is at `path` that is not a plain file (a symlink, a junction, a folder, a special file).
/// Looks at the entry itself, never through a link. A missing path is false.
pub(crate) fn exists_not_regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| !m.file_type().is_file())
}

/// A destination file about to be replaced in place: lifts what would stop `fs::copy` from opening it for
/// writing, and returns what to put back if the copy fails. Windows: clears READONLY. Unix: adds the owner
/// write bit (`fs::copy` opens with write + truncate, so a 0o444 file would fail for ever). Works on the entry
/// itself and does nothing for anything that is not a regular file (never through a link).
pub(crate) fn clear_dst_readonly(dst: &Path) -> io::Result<Option<Restore>> {
    let m = match fs::symlink_metadata(dst) {
        Ok(m) if m.file_type().is_file() => m,
        _ => return Ok(None),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = m.permissions().mode();
        if mode & 0o200 == 0 {
            fs::set_permissions(dst, fs::Permissions::from_mode(mode | 0o200))?;
            return Ok(Some(Restore { mode }));
        }
        Ok(None)
    }
    #[cfg(not(unix))]
    {
        Ok(clear_readonly(dst, &m)?.then_some(Restore {}))
    }
}

/// Puts back what [`clear_dst_readonly`] lifted (best effort, after a failed copy).
pub(crate) fn restore_dst(dst: &Path, r: &Restore) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dst, fs::Permissions::from_mode(r.mode))
    }
    #[cfg(not(unix))]
    {
        let _ = r;
        set_readonly(dst)
    }
}

/// After a copy: the source's times on the copy. Windows: creation, access and write time through a handle
/// that asks only for `FILE_WRITE_ATTRIBUTES`, so a read-only copy still gets them. Unix: access and write
/// time through a read-only handle.
pub(crate) fn set_times(dst: &Path, src: &Metadata) -> io::Result<()> {
    let mut times = FileTimes::new();
    if let Ok(t) = src.modified() {
        times = times.set_modified(t);
    }
    if let Ok(t) = src.accessed() {
        times = times.set_accessed(t);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};
        const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
        if let Ok(t) = src.created() {
            times = times.set_created(t);
        }
        let f = fs::OpenOptions::new().access_mode(FILE_WRITE_ATTRIBUTES).open(dst)?;
        f.set_times(times)
    }
    #[cfg(not(windows))]
    {
        File::open(dst)?.set_times(times)
    }
}

/// Windows: the attributes (hidden, system, archive, read-only, not-indexed) of a folder made by a copy.
/// Unix: nothing.
pub(crate) fn set_dir_attributes(dst: &Path, src: &Metadata) -> io::Result<()> {
    #[cfg(test)]
    if hook::FAIL_DIR_ATTRIBUTES.with(|c| c.get()) {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::MetadataExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_FLAGS_AND_ATTRIBUTES};
        // READONLY | HIDDEN | SYSTEM | ARCHIVE | NOT_CONTENT_INDEXED
        const KEEP: u32 = 0x1 | 0x2 | 0x4 | 0x20 | 0x2000;
        let attrs = src.file_attributes() & KEEP;
        if attrs == 0 {
            return Ok(());
        }
        let wide: Vec<u16> = dst.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        unsafe { SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(attrs)) }.map_err(win_error)
    }
    #[cfg(not(windows))]
    {
        let _ = (dst, src);
        Ok(())
    }
}

/// The Win32 error code of a `windows` error as an `io::Error` (so `ito_common::ioerror` reads it). An error
/// that is not a Win32 one keeps its text.
#[cfg(windows)]
pub(crate) fn win_error(e: windows::core::Error) -> io::Error {
    match windows::Win32::Foundation::WIN32_ERROR::from_error(&e) {
        Some(code) => io::Error::from_raw_os_error(code.0 as i32),
        None => io::Error::other(e.message()),
    }
}

/// Whether two paths are the same file: Unix by device and inode, Windows by volume serial and file index,
/// otherwise (a file that cannot be opened) by their real paths.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(x), Ok(y)) = (fs::metadata(a), fs::metadata(b)) {
            return x.dev() == y.dev() && x.ino() == y.ino();
        }
    }
    #[cfg(windows)]
    {
        if let (Some(x), Some(y)) = (win_id(a), win_id(b)) {
            return x == y;
        }
    }
    matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

#[cfg(windows)]
fn win_id(p: &Path) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    let f = File::open(p).ok()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open for the call and `info` is a valid out value.
    unsafe { GetFileInformationByHandle(HANDLE(f.as_raw_handle()), &mut info) }.ok()?;
    Some((info.dwVolumeSerialNumber, (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)))
}

/// Unix: flushes the folder entry of a renamed file (best effort).
pub(crate) fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        if let Ok(d) = File::open(if dir.as_os_str().is_empty() { Path::new(".") } else { dir }) {
            let _ = d.sync_all();
        }
    }
    let _ = path;
}

#[cfg(test)]
pub(crate) mod hook {
    use std::cell::Cell;
    thread_local! {
        /// Makes `set_dir_attributes` fail.
        pub static FAIL_DIR_ATTRIBUTES: Cell<bool> = const { Cell::new(false) };
    }
}
