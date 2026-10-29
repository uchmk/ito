//! Who other than this user can change a file: for an app that runs what a
//! file says (a job definition, a command template), a file someone else can
//! write is a way to run commands as this user.
//!
//! `others_can_write` answers for one path and the folders above it (up to
//! the home folder, else the root), because whoever can replace the folder's
//! entry can replace the file too. The OS-specific reading is in the two
//! modules below, so the app asks one question on every OS.

use std::io;
use std::path::{Path, PathBuf};

/// One thing another account can do to a path, for a warning that says which
/// file and who.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The file, or the folder above it, where the write access is.
    pub path: PathBuf,
    /// Who: `DOMAIN\name` on Windows; `everyone`, `group <gid>` or `uid <uid>` on Unix.
    pub who: String,
}

/// Who other than the current user (and the system's own accounts) can write
/// `path`, or replace it through a parent folder. Empty: only this user.
///
/// - Windows: allow entries in the DACL that give write, append, delete,
///   `WRITE_DAC` or `WRITE_OWNER` (on a folder above: delete-child, delete or
///   those two) to anyone but this user, `Administrators`, `SYSTEM` and
///   `TrustedInstaller`; and an owner that is none of them. Deny entries are
///   not weighed, so a file that allows then denies the same account is still
///   reported.
/// - Unix: group- or other-writable (a folder with the sticky bit is left
///   alone), or an owner that is neither this user nor root. On Linux a
///   group-writable file whose group is this user's own primary group is left
///   alone: with a umask of 002 that is every file the user makes.
///
/// A path that does not exist is an `Err` (`NotFound`).
pub fn others_can_write(path: &Path) -> io::Result<Vec<String>> {
    let mut who: Vec<String> = findings(path)?.into_iter().map(|f| f.who).collect();
    who.sort();
    who.dedup();
    Ok(who)
}

/// Like `others_can_write`, with the path each one was found on.
pub fn findings(path: &Path) -> io::Result<Vec<Finding>> {
    let path = path.canonicalize()?;
    // The folders above the home folder are the system's: look no higher.
    let home = crate::home_dir().and_then(|h| h.canonicalize().ok()).filter(|h| path.starts_with(h));
    let mut out = Vec::new();
    for (i, p) in path.ancestors().enumerate() {
        if i > 0 && home.as_deref().is_some_and(|h| !p.starts_with(h)) {
            break;
        }
        sys::check(p, i == 0, &mut out)?;
    }
    Ok(out)
}

/// Open `path` to everyone (`Everyone` can write on Windows, `o+w` on Unix),
/// so a test of an app can build the file `others_can_write` reports.
pub fn open_to_everyone(path: &Path) -> io::Result<()> {
    sys::open_to_everyone(path)
}

#[cfg(unix)]
mod sys {
    use super::Finding;
    use std::io;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;

    const STICKY: u32 = 0o1000;

    pub fn check(p: &Path, target: bool, out: &mut Vec<Finding>) -> io::Result<()> {
        let m = p.metadata()?;
        let mode = m.mode();
        // SAFETY: geteuid and getegid take no arguments and cannot fail.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        let mut add = |who: String| out.push(Finding { path: p.to_path_buf(), who });
        if m.uid() != uid && m.uid() != 0 {
            add(format!("uid {}", m.uid()));
        }
        // A folder with the sticky bit lets no one remove what is not theirs;
        // writing into the target itself is still writing.
        let sticky_folder = !target && m.is_dir() && mode & STICKY != 0;
        if !sticky_folder {
            if mode & 0o002 != 0 {
                add("everyone".into());
            }
            let own_group = cfg!(target_os = "linux") && m.gid() == gid;
            if mode & 0o020 != 0 && !own_group {
                add(format!("group {}", m.gid()));
            }
        }
        Ok(())
    }

    pub fn open_to_everyone(path: &Path) -> io::Result<()> {
        let mode = path.metadata()?.permissions().mode();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o002))
    }
}

#[cfg(windows)]
mod sys {
    use super::Finding;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::OnceLock;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        GetAce, GetTokenInformation, LookupAccountSidW, TokenUser, ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SID_NAME_USE, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // The access bits, by value: they are the same on a file and a folder.
    const WRITE_DATA: u32 = 0x2; // a folder: add a file
    const APPEND_DATA: u32 = 0x4; // a folder: add a subfolder
    const DELETE_CHILD: u32 = 0x40;
    const DELETE: u32 = 0x1_0000;
    const WRITE_DAC: u32 = 0x4_0000;
    const WRITE_OWNER: u32 = 0x8_0000;
    const GENERIC_ALL: u32 = 0x1000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const ALLOWED: u8 = 0; // ACCESS_ALLOWED_ACE_TYPE

    // What lets an account change the target itself, and what lets it replace
    // the target by changing the folder it is in.
    const TARGET: u32 = WRITE_DATA | APPEND_DATA | DELETE_CHILD | DELETE | WRITE_DAC | WRITE_OWNER | GENERIC_ALL | GENERIC_WRITE;
    const ABOVE: u32 = DELETE_CHILD | DELETE | WRITE_DAC | WRITE_OWNER | GENERIC_ALL | GENERIC_WRITE;

    // Accounts that may write: the system's own, whose power over the files
    // is the same as the user's already. CREATOR OWNER (S-1-3-0) is only a
    // template for what a new file inherits.
    const SYSTEMS: [&str; 4] = [
        "S-1-5-18",                                                       // SYSTEM
        "S-1-5-32-544",                                                   // Administrators
        "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464", // TrustedInstaller
        "S-1-3-0",                                                        // CREATOR OWNER
    ];

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    /// `S-1-5-…` for a SID.
    fn sid_string(sid: PSID) -> Option<String> {
        let mut s = PWSTR::null();
        // SAFETY: `sid` is a valid SID the caller got from the OS; `s` is freed below.
        unsafe { ConvertSidToStringSidW(sid, &raw mut s).ok()? };
        // SAFETY: on success `s` is a NUL-terminated string allocated by the OS.
        let text = unsafe { s.to_string().ok() };
        // SAFETY: `s` came from ConvertSidToStringSidW, which wants LocalFree.
        unsafe { LocalFree(Some(HLOCAL(s.0.cast()))) };
        text
    }

    /// `DOMAIN\name` for a SID, else its `S-1-…` text.
    fn name_of(sid: PSID, text: &str) -> String {
        let (mut name, mut domain) = ([0u16; 256], [0u16; 256]);
        let (mut nl, mut dl) = (name.len() as u32, domain.len() as u32);
        let mut kind = SID_NAME_USE(0);
        // SAFETY: the buffers and their lengths are ours; `sid` is valid.
        let ok = unsafe {
            LookupAccountSidW(
                PCWSTR::null(),
                sid,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut nl,
                Some(PWSTR(domain.as_mut_ptr())),
                &raw mut dl,
                &raw mut kind,
            )
        };
        if ok.is_err() || nl == 0 {
            return text.to_string();
        }
        let n = String::from_utf16_lossy(&name[..nl as usize]);
        let d = String::from_utf16_lossy(&domain[..dl as usize]);
        if d.is_empty() {
            n
        } else {
            format!("{d}\\{n}")
        }
    }

    /// This user's SID text, once.
    fn me() -> &'static str {
        static ME: OnceLock<String> = OnceLock::new();
        ME.get_or_init(|| {
            let mut token = HANDLE::default();
            // SAFETY: the current process handle is a pseudo handle; `token` is closed below.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }.is_err() {
                return String::new();
            }
            let mut need = 0u32;
            // SAFETY: asks only for the size (a null buffer).
            let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &raw mut need) };
            // u64s, so the TOKEN_USER in the buffer is aligned.
            let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
            // SAFETY: `buf` is at least `need` bytes.
            let got = unsafe { GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), need, &raw mut need) };
            let text = if got.is_ok() {
                // SAFETY: on success the buffer starts with a TOKEN_USER, and its SID points into the buffer.
                let user = unsafe { &*buf.as_ptr().cast::<TOKEN_USER>() };
                sid_string(user.User.Sid).unwrap_or_default()
            } else {
                String::new()
            };
            // SAFETY: `token` was opened above.
            let _ = unsafe { CloseHandle(token) };
            text
        })
    }

    fn may_write(text: &str) -> bool {
        text == me() || SYSTEMS.contains(&text)
    }

    pub fn check(p: &Path, target: bool, out: &mut Vec<Finding>) -> io::Result<()> {
        let w = wide(p);
        let (mut owner, mut dacl, mut sd) = (PSID::default(), std::ptr::null_mut::<ACL>(), PSECURITY_DESCRIPTOR::default());
        // SAFETY: `w` is NUL-terminated; the out pointers are ours; the security descriptor is freed below.
        let err = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(w.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&raw mut owner),
                None,
                Some(&raw mut dacl),
                None,
                &raw mut sd,
            )
        };
        if err.0 != 0 {
            return Err(io::Error::from_raw_os_error(err.0 as i32));
        }
        let mut add = |sid: PSID, text: &str| out.push(Finding { path: p.to_path_buf(), who: name_of(sid, text) });
        if let Some(text) = sid_string(owner) {
            if !may_write(&text) {
                add(owner, &text);
            }
        }
        let mask = if target { TARGET } else { ABOVE };
        // A null DACL grants everyone everything.
        if dacl.is_null() {
            out.push(Finding { path: p.to_path_buf(), who: "Everyone".into() });
        } else {
            // SAFETY: `dacl` points into `sd`, which is alive.
            let count = unsafe { (*dacl).AceCount };
            for i in 0..u32::from(count) {
                let mut ace = std::ptr::null_mut();
                // SAFETY: `i` is below the ACE count; `ace` points into the ACL.
                if unsafe { GetAce(dacl, i, &raw mut ace) }.is_err() {
                    continue;
                }
                // SAFETY: every ACE starts with the header; the type is read before the rest is trusted.
                let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
                if ace.Header.AceType != ALLOWED || ace.Mask & mask == 0 {
                    continue;
                }
                let sid = PSID((&raw const ace.SidStart).cast_mut().cast());
                if let Some(text) = sid_string(sid) {
                    if !may_write(&text) {
                        add(sid, &text);
                    }
                }
            }
        }
        // SAFETY: `sd` was allocated by GetNamedSecurityInfoW, which wants LocalFree.
        unsafe { LocalFree(Some(HLOCAL(sd.0))) };
        Ok(())
    }

    pub fn open_to_everyone(path: &Path) -> io::Result<()> {
        // icacls ships with Windows; the grant is an explicit entry for Everyone (S-1-1-0).
        let st = crate::access::quiet(std::process::Command::new("icacls").arg(path).args(["/grant", "*S-1-1-0:(M)"]))?;
        if st {
            Ok(())
        } else {
            Err(io::Error::other("icacls failed"))
        }
    }
}

/// Run a command without a window and say whether it exited 0.
#[cfg(windows)]
fn quiet(cmd: &mut std::process::Command) -> io::Result<bool> {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    Ok(cmd.status()?.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ito-access-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let dir = temp("missing");
        let e = others_can_write(&dir.join("nope.toml")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_made_by_this_user_is_theirs_alone() {
        let dir = temp("own");
        let f = dir.join("jobs.toml");
        std::fs::write(&f, "x = 1\n").unwrap();
        assert_eq!(others_can_write(&f).unwrap(), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_opened_to_everyone_is_reported_with_its_path() {
        let dir = temp("open");
        let f = dir.join("jobs.toml");
        std::fs::write(&f, "x = 1\n").unwrap();
        open_to_everyone(&f).unwrap();
        let who = others_can_write(&f).unwrap();
        assert!(!who.is_empty(), "{who:?}");
        let found = findings(&f).unwrap();
        assert!(found.iter().all(|x| x.path == f.canonicalize().unwrap()), "{found:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_folder_opened_to_everyone_puts_the_file_in_it_at_risk() {
        let dir = temp("above");
        let f = dir.join("jobs.toml");
        std::fs::write(&f, "x = 1\n").unwrap();
        // On Unix a folder that others can write; on Windows an entry that
        // lets Everyone write the folder (add files, delete children).
        open_to_everyone(&dir).unwrap();
        let found = findings(&f).unwrap();
        assert!(found.iter().any(|x| x.path == dir.canonicalize().unwrap()), "{found:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
