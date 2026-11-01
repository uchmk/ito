//! Files that came from the network (a mail attachment, a download): mark them so the OS warns
//! before they run, and tell the kinds that run code so the app can refuse to open them directly.

use std::io;
use std::path::Path;

/// Extensions (lower case, last one only) of files that run as a program or script, or that
/// mount or carry one, or that the shell treats as a command (`.lnk`, `.url`).
const RUNS_CODE: &[&str] = &[
    "exe",
    "com",
    "scr",
    "pif",
    "bat",
    "cmd",
    "msi",
    "msp",
    "msc",
    "mst",
    "ps1",
    "ps1xml",
    "psm1",
    "psd1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "wsc",
    "hta",
    "cpl",
    "lnk",
    "url",
    "scf",
    "inf",
    "reg",
    "jar",
    "jnlp",
    "appref-ms",
    "application",
    "gadget",
    "chm",
    "iso",
    "img",
    "vhd",
    "vhdx",
    "dll",
    "ocx",
    "sys",
    "drv",
    "sh",
    "bash",
    "zsh",
    "command",
    "app",
    "pkg",
    "dmg",
    "workflow",
    "action",
    "terminal",
    "desktop",
    "run",
    "bin",
    "apk",
    "deb",
    "rpm",
    "appimage",
    "py",
    "pyw",
    "pl",
    "rb",
    "docm",
    "xlsm",
    "pptm",
    "dotm",
    "xlam",
    "ppam",
];

/// Whether a file with this name runs code when opened (a program, a script, a shortcut, a disk image
/// or an installer). Looks at the last extension only, ignoring case; a name without one is `false`.
///
/// Apps should not hand such a file to the default opener on a stranger's word: save it and let the
/// person open it themselves.
pub fn runs_code(name: &str) -> bool {
    let name = name.trim_end_matches([' ', '.']);
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    // `.exe` alone is a hidden file, not a program; Windows would run `x.exe` though.
    !stem.is_empty() && RUNS_CODE.contains(&ext.to_ascii_lowercase().as_str())
}

/// Mark `path` as downloaded from the Internet, as browsers do, so the OS asks before running it
/// (SmartScreen, Office's Protected View, Gatekeeper). `from` is where it came from (a URL), if known.
///
/// - Windows: the `Zone.Identifier` stream with `ZoneId=3` (and `HostUrl` when `from` is given).
/// - macOS: the `com.apple.quarantine` attribute.
/// - Elsewhere: nothing, `Ok(())`.
///
/// A file system that cannot hold the mark (FAT, exFAT, some network shares) is not an error: there
/// is nothing to mark and `Ok(())` comes back. The file must exist.
pub fn mark_from_web(path: &Path, from: Option<&str>) -> io::Result<()> {
    // Writing a stream to a missing path would create an empty file there.
    if !path.exists() {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    imp::mark(path, from)
}

#[cfg(windows)]
mod imp {
    use super::*;

    pub fn mark(path: &Path, from: Option<&str>) -> io::Result<()> {
        let mut text = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
        if let Some(url) = from {
            let url: String = url.chars().filter(|c| !c.is_control()).collect();
            if !url.is_empty() {
                text.push_str("HostUrl=");
                text.push_str(&url);
                text.push_str("\r\n");
            }
        }
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        match std::fs::write(&stream, text) {
            Ok(()) => Ok(()),
            // No streams here (FAT, exFAT, a share): ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED,
            // ERROR_INVALID_PARAMETER, ERROR_INVALID_NAME. Only when the file itself is there.
            Err(e) if path.exists() && matches!(e.raw_os_error(), Some(1 | 50 | 87 | 123)) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn mark(path: &Path, _from: Option<&str>) -> io::Result<()> {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let value = format!("0081;{secs:x};ito;");
        let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let name = c"com.apple.quarantine";
        // SAFETY: both strings are NUL-terminated and live through the call; the value is `value.len()` bytes.
        let r = unsafe { libc::setxattr(c_path.as_ptr(), name.as_ptr(), value.as_ptr().cast(), value.len(), 0, 0) };
        if r == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        // ENOTSUP / EPERM: a volume without extended attributes (FAT, some shares).
        if matches!(e.raw_os_error(), Some(libc::ENOTSUP | libc::EPERM)) && path.exists() {
            Ok(())
        } else {
            Err(e)
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::*;

    pub fn mark(_path: &Path, _from: Option<&str>) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_and_scripts_are_told_by_the_last_extension() {
        for n in
            ["a.exe", "A.EXE", "x.Bat", "run.js", "s.lnk", "web.url", "inst.msi", "a.tar.sh", "disk.ISO", "mac.dmg", "x.ps1", "doc.pdf.exe"]
        {
            assert!(runs_code(n), "{n}");
        }
        for n in ["a.pdf", "photo.jpg", "notes.txt", "exe", "a.exe.txt", ".exe", "report.docx", "a.zip", ""] {
            assert!(!runs_code(n), "{n}");
        }
    }

    #[test]
    fn trailing_dots_and_spaces_do_not_hide_a_program() {
        assert!(runs_code("evil.exe "));
        assert!(runs_code("evil.exe."));
    }

    #[test]
    fn marking_a_real_file_works_and_a_missing_one_does_not_pretend() {
        let d = std::env::temp_dir().join(format!("ito-web-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.txt");
        std::fs::write(&f, "x").unwrap();
        mark_from_web(&f, Some("https://example.com/a.txt\r\nZoneId=0")).unwrap();
        mark_from_web(&f, None).unwrap();
        #[cfg(windows)]
        {
            let mut s = f.as_os_str().to_owned();
            s.push(":Zone.Identifier");
            let t = std::fs::read_to_string(&s).unwrap();
            assert!(t.starts_with("[ZoneTransfer]\r\nZoneId=3\r\n"), "{t:?}");
            assert_eq!(t.matches("ZoneId").count(), 1, "a URL cannot add lines: {t:?}");
        }
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "x", "the file itself is untouched");
        assert!(mark_from_web(&d.join("missing.txt"), None).is_err());
        assert!(!d.join("missing.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
