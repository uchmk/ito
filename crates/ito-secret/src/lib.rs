//! Secrets kept in the OS credential store instead of a file: a Slack or
//! Discord webhook address (it carries its token), a password a job needs.
//! `get`, `set` and `delete` by `(app, name)`; the store is Windows'
//! Credential Manager, macOS' Keychain, and on Linux the Secret Service
//! (through `secret-tool` of libsecret, the value passed on its stdin, never
//! on a command line). Where no store runs, every call is an `Err` that says
//! so; nothing falls back to a file.
//!
//! These block (the store may ask the user to unlock), so call them from a
//! worker thread, not the UI thread.

use std::io;

/// The store's name for `(app, name)`.
fn target(app: &str, name: &str) -> String {
    format!("uchmk/{app}/{name}")
}

/// The secret kept as `name` for `app`; `None` when there is none.
pub fn get(app: &str, name: &str) -> io::Result<Option<String>> {
    sys::get(&target(app, name))
}

/// Keep `value` as `name` for `app`, replacing what was there.
pub fn set(app: &str, name: &str, value: &str) -> io::Result<()> {
    sys::set(&target(app, name), value)
}

/// Forget `name` of `app`; no error when there was none.
pub fn delete(app: &str, name: &str) -> io::Result<()> {
    sys::delete(&target(app, name))
}

/// A secret as it is shown in a log or on a screen: `***`.
pub const MASK: &str = "***";

/// `text` with every occurrence of each of `secrets` replaced by [`MASK`]
/// (empty secrets are skipped; the longest goes first so one inside another
/// leaves nothing of it).
pub fn mask(text: &str, secrets: &[&str]) -> String {
    let mut s: Vec<&str> = secrets.iter().copied().filter(|s| !s.is_empty()).collect();
    s.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = text.to_owned();
    for x in s {
        out = out.replace(x, MASK);
    }
    out
}

#[cfg(windows)]
mod sys {
    use std::io;
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    pub fn get(target: &str) -> io::Result<Option<String>> {
        let name = HSTRING::from(target);
        let mut p: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `name` outlives the call; on success `p` is a credential freed below.
        match unsafe { CredReadW(&name, CRED_TYPE_GENERIC, None, &mut p) } {
            Ok(()) => {
                // SAFETY: CredReadW filled `p`; the blob is `CredentialBlobSize` bytes.
                let text = unsafe {
                    let c = &*p;
                    let bytes = if c.CredentialBlob.is_null() {
                        &[][..]
                    } else {
                        std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
                    };
                    String::from_utf8_lossy(bytes).into_owned()
                };
                // SAFETY: `p` came from CredReadW.
                unsafe { CredFree(p as *const _) };
                Ok(Some(text))
            }
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(None),
            Err(e) => Err(io::Error::other(e)),
        }
    }

    pub fn set(target: &str, value: &str) -> io::Result<()> {
        let mut name: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
        let mut blob = value.as_bytes().to_vec();
        let cred = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(name.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        // SAFETY: `cred` points at `name` and `blob`, both alive for the call.
        unsafe { CredWriteW(&cred, 0) }.map_err(io::Error::other)
    }

    pub fn delete(target: &str) -> io::Result<()> {
        // SAFETY: plain call with a live string.
        match unsafe { CredDeleteW(&HSTRING::from(target), CRED_TYPE_GENERIC, None) } {
            Err(e) if e.code() != ERROR_NOT_FOUND.to_hresult() => Err(io::Error::other(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
    use std::io;

    /// errSecItemNotFound.
    const NOT_FOUND: i32 = -25300;

    pub fn get(target: &str) -> io::Result<Option<String>> {
        match get_generic_password(target, "ito") {
            Ok(b) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(io::Error::other(e)),
        }
    }

    pub fn set(target: &str, value: &str) -> io::Result<()> {
        set_generic_password(target, "ito", value.as_bytes()).map_err(io::Error::other)
    }

    pub fn delete(target: &str) -> io::Result<()> {
        match delete_generic_password(target, "ito") {
            Err(e) if e.code() != NOT_FOUND => Err(io::Error::other(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod sys {
    use std::io::{self, Write};
    use std::process::{Command, Stdio};

    fn missing(e: io::Error) -> io::Error {
        if e.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "no secret store: install secret-tool (libsecret-tools) and run a keyring such as gnome-keyring",
            )
        } else {
            e
        }
    }

    pub fn get(target: &str) -> io::Result<Option<String>> {
        let out = Command::new("secret-tool")
            .args(["lookup", "ito", target])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(missing)?;
        // `lookup` exits 1 with nothing printed when there is no such entry.
        if out.status.success() && !out.stdout.is_empty() {
            Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
        } else if out.status.code() == Some(1) && out.stdout.is_empty() {
            Ok(None)
        } else {
            Err(io::Error::other("secret-tool lookup failed (is the keyring unlocked?)"))
        }
    }

    pub fn set(target: &str, value: &str) -> io::Result<()> {
        let mut child = Command::new("secret-tool")
            .args(["store", "--label", target, "ito", target])
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(missing)?;
        child.stdin.take().expect("piped").write_all(value.as_bytes())?;
        if child.wait()?.success() {
            Ok(())
        } else {
            Err(io::Error::other("secret-tool store failed (is the keyring unlocked?)"))
        }
    }

    pub fn delete(target: &str) -> io::Result<()> {
        // `clear` of an entry that is not there is not an error worth reporting.
        Command::new("secret-tool").args(["clear", "ito", target]).stdin(Stdio::null()).stderr(Stdio::null()).status().map_err(missing)?;
        Ok(())
    }
}

#[cfg(not(any(windows, unix)))]
mod sys {
    use std::io;

    fn no() -> io::Error {
        io::Error::new(io::ErrorKind::Unsupported, "no secret store on this system")
    }
    pub fn get(_: &str) -> io::Result<Option<String>> {
        Err(no())
    }
    pub fn set(_: &str, _: &str) -> io::Result<()> {
        Err(no())
    }
    pub fn delete(_: &str) -> io::Result<()> {
        Err(no())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_replaces_every_secret_longest_first() {
        assert_eq!(mask("pw=abc and abcd", &["abc", "abcd", ""]), "pw=*** and ***");
        assert_eq!(mask("nothing", &["x"]), "nothing");
    }

    /// Against the real store; skipped where none runs (CI on Linux).
    #[test]
    fn a_secret_goes_in_comes_out_and_is_forgotten() {
        let (app, name) = ("ito-secret-test", format!("t{}", std::process::id()));
        match set(app, &name, "s3cret ünï") {
            Err(e) if cfg!(not(windows)) => return eprintln!("skipped: {e}"),
            r => r.unwrap(),
        }
        assert_eq!(get(app, &name).unwrap().as_deref(), Some("s3cret ünï"));
        set(app, &name, "second").unwrap();
        assert_eq!(get(app, &name).unwrap().as_deref(), Some("second"));
        delete(app, &name).unwrap();
        assert_eq!(get(app, &name).unwrap(), None);
        delete(app, &name).unwrap();
    }
}
