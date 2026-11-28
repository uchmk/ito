//! Secrets kept in the OS credential store instead of a file: a Slack or
//! Discord webhook address (it carries its token), a password a job needs.
//! `get`, `set` and `delete` by `(app, name)`; the store is Windows'
//! Credential Manager, macOS' Keychain, and on Linux the Secret Service
//! (through `secret-tool` of libsecret, the value passed on its stdin, never
//! on a command line). Where no store runs, every call is an `Err` that says
//! so. Secrets are written only to the OS store; nothing falls back to a file
//! for storing.
//!
//! The one exception is reading, on Linux: a service that systemd started
//! with `LoadCredential=` finds each credential as a file
//! `$CREDENTIALS_DIRECTORY/<app>_<name>`, and `get` looks there first. That
//! folder is a read-only source: this crate never writes it, and `set` and
//! `delete` do not touch it.
//!
//! These block (the store may ask the user to unlock), so call them from a
//! worker thread, not the UI thread.

use std::io;

/// The store's name for `(app, name)`.
fn target(app: &str, name: &str) -> String {
    format!("uchmk/{app}/{name}")
}

/// The secret kept as `name` for `app`; `None` when there is none.
///
/// On Linux a systemd credential (`$CREDENTIALS_DIRECTORY/<app>_<name>`) wins over the store; the store is asked
/// only when there is no such credential. A credential that exists but cannot be read is an `Err`, not a fall-through.
pub fn get(app: &str, name: &str) -> io::Result<Option<String>> {
    #[cfg(target_os = "linux")]
    if let Some(dir) = std::env::var_os("CREDENTIALS_DIRECTORY") {
        if let Some(value) = read_credential(std::path::Path::new(&dir), app, name)? {
            return Ok(Some(value));
        }
    }
    sys::get(&target(app, name))
}

/// The credential file `<app>_<name>` in `dir`, with one final line end removed. `None` when `dir` is not absolute, when
/// `app` or `name` could name another place (`/`, `\`, NUL), when `app` has a `_` (the first `_` must be the boundary), or
/// when there is no such file. Any other read error is an `Err` that names the file but never shows its contents.
#[cfg(any(test, target_os = "linux"))]
fn read_credential(dir: &std::path::Path, app: &str, name: &str) -> io::Result<Option<String>> {
    if !dir.is_absolute() {
        return Ok(None);
    }
    if app.contains(['/', '\\', '\0']) || name.contains(['/', '\\', '\0']) || app.contains('_') {
        return Ok(None);
    }
    let file = format!("{app}_{name}");
    match std::fs::read_to_string(dir.join(&file)) {
        Ok(text) => Ok(Some(text.strip_suffix('\n').map_or(text.as_str(), |t| t.strip_suffix('\r').unwrap_or(t)).to_owned())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io::Error::new(e.kind(), format!("cannot read the credential {file}: {e}"))),
    }
}

/// Keep `value` as `name` for `app`, replacing what was there.
pub fn set(app: &str, name: &str, value: &str) -> io::Result<()> {
    sys::set(&target(app, name), value)
}

/// Forget `name` of `app`; no error when there was none. An `Err` means the store failed (a locked keyring, no store
/// running) and the secret may still be there.
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

    /// The program and the arguments before the action: `secret-tool`, or a stand-in in the tests.
    const TOOL: &[&str] = &["secret-tool"];

    fn tool(base: &[&str]) -> Command {
        let mut c = Command::new(base[0]);
        c.args(&base[1..]);
        c
    }

    pub fn get(target: &str) -> io::Result<Option<String>> {
        get_with(TOOL, target)
    }

    pub(crate) fn get_with(base: &[&str], target: &str) -> io::Result<Option<String>> {
        let out = tool(base).args(["lookup", "ito", target]).stdin(Stdio::null()).output().map_err(missing)?;
        super::lookup_said(out.status.code(), &out.stdout, &out.stderr)
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
        delete_with(TOOL, target)
    }

    pub(crate) fn delete_with(base: &[&str], target: &str) -> io::Result<()> {
        let out = tool(base).args(["clear", "ito", target]).stdin(Stdio::null()).output().map_err(missing)?;
        super::clear_said(out.status.code(), &out.stderr)
    }
}

// What `secret-tool` says, read as libsecret's tool/secret-tool.c writes it: `lookup` and `clear` exit 1 in silence when
// nothing matched, and exit 1 after a line on stderr when the store failed (a locked keyring, no D-Bus session). Any
// other word on stderr with a failing exit is taken as a failure too: the secret may still be there.

/// `secret-tool lookup`'s answer: the secret on stdout, `None` for a silent "no such entry", `Err` for anything else.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn lookup_said(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> io::Result<Option<String>> {
    match (code, said(stderr)) {
        (Some(0), _) => Ok(Some(String::from_utf8_lossy(stdout).into_owned())),
        (Some(1), None) if stdout.is_empty() => Ok(None),
        (_, why) => Err(failed("lookup", why)),
    }
}

/// `secret-tool clear`'s answer: `Ok` when it cleared or there was nothing to clear, `Err` when it failed.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn clear_said(code: Option<i32>, stderr: &[u8]) -> io::Result<()> {
    match (code, said(stderr)) {
        (Some(0), _) | (Some(1), None) => Ok(()),
        (_, why) => Err(failed("clear", why)),
    }
}

/// The first non-blank line the tool wrote to stderr, trimmed and cut to 200 characters (it names the failure, never a
/// secret).
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn said(stderr: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(200).collect())
}

#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn failed(action: &str, why: Option<String>) -> io::Error {
    match why {
        Some(why) => io::Error::other(format!("secret-tool {action} failed: {why}")),
        None => io::Error::other(format!("secret-tool {action} failed (is the keyring unlocked?)")),
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

    #[test]
    fn clear_tells_nothing_to_clear_from_a_failure() {
        assert!(clear_said(Some(0), b"").is_ok(), "cleared");
        assert!(clear_said(Some(1), b"").is_ok(), "nothing matched: secret-tool exits 1 in silence");
        assert!(clear_said(Some(1), b" \n").is_ok(), "blank lines say nothing");
        let e = clear_said(Some(1), b"secret-tool: Cannot autolaunch D-Bus without X11 $DISPLAY\n").unwrap_err();
        assert_eq!(e.to_string(), "secret-tool clear failed: secret-tool: Cannot autolaunch D-Bus without X11 $DISPLAY");
        assert!(clear_said(Some(2), b"").is_err(), "another exit code");
        assert!(clear_said(None, b"").is_err(), "ended by a signal");
    }

    #[test]
    fn lookup_tells_no_entry_from_a_failure() {
        assert_eq!(lookup_said(Some(0), b"pw", b"").unwrap().as_deref(), Some("pw"));
        assert_eq!(lookup_said(Some(0), b"", b"").unwrap().as_deref(), Some(""), "an empty secret, as on Windows and macOS");
        assert_eq!(lookup_said(Some(1), b"", b"").unwrap(), None);
        let e = lookup_said(Some(1), b"", b"secret-tool: Cannot create an item in a locked collection\n").unwrap_err();
        assert!(e.to_string().ends_with("locked collection"), "{e}");
        assert!(lookup_said(Some(1), b"x", b"").is_err());
        assert!(lookup_said(None, b"", b"").is_err());
        assert_eq!(said(&[b'x'; 300]).map(|s| s.chars().count()), Some(200), "a long line is cut");
    }

    /// The real processes, with `sh` standing in for `secret-tool` (`$0` is the stand-in's name, `$1` the action).
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn a_stand_in_tool_is_read_the_same() {
        let sh = |script: &'static str| ["sh", "-c", script, "secret-tool"];
        assert!(sys::delete_with(&sh("exit 1"), "uchmk/a/b").is_ok());
        assert!(sys::delete_with(&sh("[ \"$1 $2 $3\" = 'clear ito uchmk/a/b' ] || exit 9"), "uchmk/a/b").is_ok());
        let e = sys::delete_with(&sh("echo 'secret-tool: no D-Bus' >&2; exit 1"), "uchmk/a/b").unwrap_err();
        assert!(e.to_string().ends_with("no D-Bus"), "{e}");
        assert_eq!(sys::get_with(&sh("printf pw"), "uchmk/a/b").unwrap().as_deref(), Some("pw"));
        assert_eq!(sys::get_with(&sh("exit 1"), "uchmk/a/b").unwrap(), None);
        assert!(sys::get_with(&sh("echo 'secret-tool: locked' >&2; exit 1"), "uchmk/a/b").is_err());
    }

    fn cred_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ito-secret-cred-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_credential_loses_one_final_line_end() {
        let d = cred_dir("eol");
        for (file, text, want) in [
            ("app_a", "v\n", "v"),
            ("app_b", "v\r\n", "v"),
            ("app_c", "a\n\n", "a\n"),
            ("app_d", "a\nb  ", "a\nb  "),
            ("app_e", "v", "v"),
            ("app_f", "", ""),
        ] {
            std::fs::write(d.join(file), text).unwrap();
            let name = &file[4..];
            assert_eq!(read_credential(&d, "app", name).unwrap().as_deref(), Some(want), "{file}");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_credential_app_has_no_underscore_but_a_name_may() {
        let d = cred_dir("us");
        std::fs::write(d.join("a_b_c"), "x").unwrap();
        std::fs::write(d.join("a_b"), "y").unwrap();
        assert_eq!(read_credential(&d, "a_b", "c").unwrap(), None);
        assert_eq!(read_credential(&d, "a", "b_c").unwrap().as_deref(), Some("x"));
        assert_eq!(read_credential(&d, "a", "missing").unwrap(), None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_credential_stays_inside_its_folder() {
        // A relative folder that really holds the file (tests run in the crate's folder), so only the check can make it `None`.
        let relative = std::path::PathBuf::from(format!("relative-credentials-{}", std::process::id()));
        std::fs::create_dir_all(&relative).unwrap();
        std::fs::write(relative.join("app_x"), "s").unwrap();
        let got = read_credential(&relative, "app", "x");
        std::fs::remove_dir_all(&relative).unwrap();
        assert_eq!(got.unwrap(), None);
        let d = cred_dir("trav");
        let inner = d.join("inner");
        // A real file at the place each refused name would reach, so only the check can make it `None`.
        for (folder, file) in [("app_x", "y"), ("a", "b_x")] {
            std::fs::create_dir_all(inner.join(folder)).unwrap();
            std::fs::write(inner.join(folder).join(file), "s").unwrap();
        }
        // Windows cannot make a folder whose name ends in dots or a file with `\` in its name, so the two `..` names prove
        // the check only on Unix.
        #[cfg(unix)]
        {
            std::fs::create_dir_all(inner.join("app_..")).unwrap();
            std::fs::write(inner.join("app_..").join("secret"), "s").unwrap();
            std::fs::write(inner.join("app_..\\secret"), "s").unwrap();
        }
        assert_eq!(read_credential(&inner, "app", "../secret").unwrap(), None);
        assert_eq!(read_credential(&inner, "app", "..\\secret").unwrap(), None);
        assert_eq!(read_credential(&inner, "app", "x/y").unwrap(), None);
        assert_eq!(read_credential(&inner, "a/b", "x").unwrap(), None);
        // No file can have a NUL in its name: without the check the read fails and this `unwrap` panics.
        assert_eq!(read_credential(&inner, "app", "x\0").unwrap(), None);
        std::fs::write(inner.join("app_z"), "s").unwrap();
        assert_eq!(read_credential(&inner, "app", "z").unwrap().as_deref(), Some("s"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn an_unreadable_credential_is_an_error_without_its_contents() {
        let d = cred_dir("bad");
        std::fs::create_dir(d.join("app_dir")).unwrap();
        let e = read_credential(&d, "app", "dir").unwrap_err();
        assert!(e.to_string().contains("app_dir"), "{e}");
        std::fs::write(d.join("app_bin"), b"hunter2\xff\xfe").unwrap();
        let e = read_credential(&d, "app", "bin").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("app_bin"), "{e}");
        assert!(!e.to_string().contains("hunter2"), "{e}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
