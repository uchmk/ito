//! The opt-in store: one file per secret, encrypted by `systemd-creds --user` (systemd 256 or later). See
//! [`crate::Store::SystemdCreds`]. The pure parts (names, folder, version) build everywhere so they are tested
//! everywhere; the file and process parts are Linux only.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long one run of the tool may take.
const DEADLINE: Duration = Duration::from_secs(30);
/// The longest value systemd accepts (`CREDENTIAL_SIZE_MAX`).
const VALUE_MAX: usize = 1024 * 1024;
/// The largest encrypted file `get` opens (an encrypted 1 MiB value plus its header fits well inside).
const FILE_MAX: u64 = 2 * 1024 * 1024;
/// The oldest systemd that has `--user` credentials.
const SYSTEMD_MIN: u32 = 256;
/// The program before the action: `systemd-creds`, or a stand-in in the tests.
const TOOL: &[&str] = &["systemd-creds"];

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

/// `app` may be a folder name and the first part of a credential name (the first `_` is the boundary).
pub(crate) fn check_app(app: &str) -> io::Result<()> {
    if app.is_empty() || app.contains(['\0', '/', '\\', '_']) || app.starts_with('.') {
        return Err(bad(format!(
            "the app name {app:?} is not allowed here: it must not be empty, start with a dot, or contain NUL, `/`, `\\` or `_`"
        )));
    }
    Ok(())
}

fn check_name(name: &str) -> io::Result<()> {
    if name.is_empty() || name.contains(['\0', '/', '\\']) || name.starts_with('.') {
        return Err(bad(format!(
            "the secret name {name:?} is not allowed here: it must not be empty, start with a dot, or contain NUL, `/` or `\\`"
        )));
    }
    Ok(())
}

/// Keep ASCII letters, digits, `.`, `_` and `-`; write every other byte as `%XX` (uppercase hex).
pub(crate) fn enc(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The credential name, which is also the file name: `enc(app)_enc(name)`.
pub(crate) fn cred_name(app: &str, name: &str) -> io::Result<String> {
    check_app(app)?;
    check_name(name)?;
    let cred = format!("{}_{}", enc(app), enc(name));
    if cred.len() > 255 {
        return Err(bad(format!("the credential name for {app}/{name} is {} bytes; the limit is 255", cred.len())));
    }
    Ok(cred)
}

/// The folder from the two environment values: `xdg` when absolute, else `home` when absolute, else `None`.
pub(crate) fn folder_from(xdg: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>, app: &str) -> Option<PathBuf> {
    if check_app(app).is_err() {
        return None;
    }
    let abs = |v: Option<std::ffi::OsString>| v.map(PathBuf::from).filter(|p| p.is_absolute());
    let root = match abs(xdg) {
        Some(x) => x,
        None => abs(home)?.join(".local").join("share"),
    };
    Some(root.join("uchmk").join(app).join("credentials"))
}

/// What the probe found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Avail {
    Absent,
    TooOld(u32),
    Ready(u32),
}

/// The major version from `systemd 257 (257.9-1~deb13u1)` (the first line of `--version`).
pub(crate) fn parse_version(out: &str) -> Option<u32> {
    let line = out.lines().next()?;
    line.strip_prefix("systemd ")?.split_whitespace().next()?.parse().ok()
}

fn avail_of(n: u32) -> Avail {
    if n < SYSTEMD_MIN {
        Avail::TooOld(n)
    } else {
        Avail::Ready(n)
    }
}

/// The tool's own answer, or an error that cannot be told from a failure to run it.
pub(crate) fn probe(base: &[&str]) -> io::Result<Avail> {
    let mut c = Command::new(base[0]);
    c.args(&base[1..]).arg("--version");
    let ran = match run(c, Input::Null, Output::Piped, DEADLINE) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Avail::Absent),
        r => r?,
    };
    let text = String::from_utf8_lossy(&ran.stdout);
    if !ran.status.success() {
        let why = match said(&ran.stderr) {
            Some(w) => w,
            None => describe(ran.status),
        };
        return Err(io::Error::other(format!("cannot tell the systemd version: {} --version failed: {why}", base[0])));
    }
    match parse_version(&text) {
        Some(n) => Ok(avail_of(n)),
        None => Err(io::Error::other("cannot tell the systemd version: unexpected output of --version")),
    }
}

static AVAIL: Mutex<Option<Avail>> = Mutex::new(None);

/// The real tool's answer, cached when it is a stable one (an error is asked again next time).
fn availability() -> io::Result<Avail> {
    if let Some(a) = *AVAIL.lock().unwrap_or_else(|e| e.into_inner()) {
        return Ok(a);
    }
    let a = probe(TOOL)?;
    *AVAIL.lock().unwrap_or_else(|e| e.into_inner()) = Some(a);
    Ok(a)
}

/// The first non-blank line of stderr, trimmed and cut to 200 characters.
fn said(stderr: &[u8]) -> Option<String> {
    crate::said(stderr)
}

fn describe(st: ExitStatus) -> String {
    match st.code() {
        Some(n) => format!("exit {n}"),
        None => "ended by a signal".to_owned(),
    }
}

fn tool_failed(action: &str, cred: &str, st: ExitStatus, stderr: &[u8]) -> io::Error {
    match said(stderr) {
        Some(why) => io::Error::other(format!("systemd-creds {action} {cred} failed: {why}")),
        None => io::Error::other(format!("systemd-creds {action} {cred} failed ({})", describe(st))),
    }
}

pub(crate) enum Input<'a> {
    Null,
    Bytes(&'a [u8]),
    File(File),
}

pub(crate) enum Output {
    Piped,
    File(File),
}

pub(crate) struct Ran {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Run `cmd` with the given stdin and stdout, drain stderr (and stdout when piped) on threads so a full pipe never blocks
/// the child, and wait up to `deadline` (`TimedOut` after killing it). The child is always reaped.
pub(crate) fn run(mut cmd: Command, stdin: Input<'_>, stdout: Output, deadline: Duration) -> io::Result<Ran> {
    let bytes: Option<&[u8]> = match &stdin {
        Input::Bytes(b) => Some(*b),
        _ => None,
    };
    cmd.stdin(match stdin {
        Input::Null => Stdio::null(),
        Input::Bytes(_) => Stdio::piped(),
        Input::File(f) => Stdio::from(f),
    });
    cmd.stdout(match stdout {
        Output::Piped => Stdio::piped(),
        Output::File(f) => Stdio::from(f),
    });
    cmd.stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let sin = child.stdin.take();
    let sout = child.stdout.take();
    let serr = child.stderr.take();
    let start = Instant::now();
    std::thread::scope(|s| {
        let writer = s.spawn(move || -> io::Result<()> {
            if let (Some(mut i), Some(b)) = (sin, bytes) {
                match i.write_all(b) {
                    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                    r => r,
                }
            } else {
                Ok(())
            }
        });
        let drain = |p: Option<Box<dyn Read + Send>>| {
            s.spawn(move || {
                let mut v = Vec::new();
                if let Some(mut p) = p {
                    let _ = p.read_to_end(&mut v);
                }
                v
            })
        };
        let out_h = drain(sout.map(|p| Box::new(p) as Box<dyn Read + Send>));
        let err_h = drain(serr.map(|p| Box::new(p) as Box<dyn Read + Send>));
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Ok(st),
                Ok(None) if start.elapsed() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(io::Error::new(io::ErrorKind::TimedOut, "systemd-creds did not finish in 30 seconds"));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(e);
                }
            }
        };
        let w = writer.join().unwrap_or_else(|_| Err(io::Error::other("writer thread panicked")));
        let stdout = out_h.join().unwrap_or_default();
        let stderr = err_h.join().unwrap_or_default();
        let status = status?;
        w?;
        Ok(Ran { status, stdout, stderr })
    })
}

#[cfg(target_os = "linux")]
pub(crate) use ops::{delete, get, set};

#[cfg(target_os = "linux")]
mod ops {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn missing(e: io::Error) -> io::Error {
        if e.kind() == io::ErrorKind::NotFound {
            io::Error::new(io::ErrorKind::Unsupported, "no systemd-creds: it is not installed")
        } else {
            e
        }
    }

    /// The tool, the folder and where systemd may provide credentials (so tests need no process environment).
    pub(super) struct Creds<'a> {
        pub base: &'a [&'a str],
        pub folder: PathBuf,
        pub provided_dir: Option<PathBuf>,
        pub deadline: Duration,
    }

    /// An error for a name that `$CREDENTIALS_DIRECTORY` provides.
    fn check_provided(dir: Option<&Path>, app: &str, name: &str) -> io::Result<()> {
        let Some(dir) = dir.filter(|d| d.is_absolute()) else { return Ok(()) };
        let file = format!("{app}_{name}");
        match std::fs::symlink_metadata(dir.join(&file)) {
            Ok(_) => Err(crate::provided_error(&file)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io::Error::new(e.kind(), format!("cannot check the credential {file}: {e}"))),
        }
    }

    impl Creds<'_> {
        fn cmd(&self) -> Command {
            let mut c = Command::new(self.base[0]);
            c.args(&self.base[1..]);
            c
        }

        pub(super) fn get(&self, app: &str, name: &str) -> io::Result<Option<String>> {
            let cred = cred_name(app, name)?;
            let path = self.folder.join(&cred);
            match std::fs::symlink_metadata(&path) {
                Ok(m) if !m.file_type().is_file() => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{cred} is not a regular file")))
                }
                Ok(m) if m.len() > FILE_MAX => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{cred} is too large"))),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            }
            let file = match File::open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            };
            if !file.metadata()?.file_type().is_file() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{cred} is not a regular file")));
            }
            let mut c = self.cmd();
            c.args(["decrypt", "--user"]).arg(format!("--name={cred}")).args(["--newline=no", "-", "-"]);
            let ran = run(c, Input::File(file), Output::Piped, self.deadline).map_err(missing)?;
            if !ran.status.success() {
                return Err(tool_failed("decrypt", &cred, ran.status, &ran.stderr));
            }
            match String::from_utf8(ran.stdout) {
                Ok(s) => Ok(Some(s)),
                Err(_) => Err(io::Error::new(io::ErrorKind::InvalidData, format!("{cred} does not decrypt to UTF-8 text"))),
            }
        }

        pub(super) fn set(&self, app: &str, name: &str, value: &str) -> io::Result<()> {
            let cred = cred_name(app, name)?;
            check_provided(self.provided_dir.as_deref(), app, name)?;
            if value.len() > VALUE_MAX {
                return Err(bad(format!("the value is {} bytes; systemd accepts at most {VALUE_MAX}", value.len())));
            }
            let folder = &self.folder;
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(folder)?;
            let meta = std::fs::symlink_metadata(folder)?;
            if !meta.file_type().is_dir() {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} is not a folder", folder.display())));
            }
            if meta.permissions().mode() & 0o077 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} is open to other users; chmod 700 it", folder.display()),
                ));
            }
            let (temp, temp_path) = create_temp(folder)?;
            let done = (|| -> io::Result<()> {
                if meta.uid() != temp.metadata()?.uid() {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("{} belongs to another user", folder.display())));
                }
                let mut c = self.cmd();
                c.args(["encrypt", "--user"]).arg(format!("--name={cred}")).args(["-", "-"]);
                let ran = run(c, Input::Bytes(value.as_bytes()), Output::File(temp.try_clone()?), self.deadline).map_err(missing)?;
                if !ran.status.success() {
                    return Err(tool_failed("encrypt", &cred, ran.status, &ran.stderr));
                }
                temp.sync_all()
            })();
            if let Err(e) = done {
                let _ = std::fs::remove_file(&temp_path);
                return Err(e);
            }
            if let Err(e) = std::fs::rename(&temp_path, folder.join(&cred)) {
                let _ = std::fs::remove_file(&temp_path);
                return Err(e);
            }
            File::open(folder)
                .and_then(|f| f.sync_all())
                .map_err(|e| io::Error::new(e.kind(), format!("the new value is in place but the folder could not be synced: {e}")))
        }

        pub(super) fn delete(&self, app: &str, name: &str) -> io::Result<()> {
            let cred = cred_name(app, name)?;
            check_provided(self.provided_dir.as_deref(), app, name)?;
            match std::fs::remove_file(self.folder.join(cred)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        }
    }

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A new `.tmp-<pid>-<n>` file (0600) in `folder`.
    fn create_temp(folder: &Path) -> io::Result<(File, PathBuf)> {
        let mut last = None;
        for _ in 0..8 {
            let path = folder.join(format!(".tmp-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
            match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path) {
                Ok(f) => return Ok((f, path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::other("no temporary file name")))
    }

    /// The real tool and folder for `app`; `Ok(None)` when systemd-creds cannot be used (`get` then finds nothing).
    fn real<'a>(app: &str, get: bool) -> io::Result<Option<Creds<'a>>> {
        match super::availability()? {
            Avail::Ready(_) => {}
            Avail::Absent if get => return Ok(None),
            Avail::TooOld(_) if get => return Ok(None),
            Avail::Absent => return Err(io::Error::new(io::ErrorKind::Unsupported, "systemd-creds is not installed")),
            Avail::TooOld(n) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("systemd {n} is older than {SYSTEMD_MIN}, which user-scoped credentials need"),
                ))
            }
        }
        let Some(folder) = crate::credentials_dir(app) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no folder for credentials: set HOME or XDG_DATA_HOME"));
        };
        let provided_dir = std::env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from);
        Ok(Some(Creds { base: TOOL, folder, provided_dir, deadline: DEADLINE }))
    }

    /// `real(app, false)` never answers `None` (it is an error then), so this is only a guard.
    fn usable(c: Option<Creds<'_>>) -> io::Result<Creds<'_>> {
        c.ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "systemd-creds cannot be used here"))
    }

    pub(crate) fn get(app: &str, name: &str) -> io::Result<Option<String>> {
        cred_name(app, name)?;
        match real(app, true)? {
            Some(c) => c.get(app, name),
            None => Ok(None),
        }
    }

    pub(crate) fn set(app: &str, name: &str, value: &str) -> io::Result<()> {
        cred_name(app, name)?;
        let dir = std::env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from);
        check_provided(dir.as_deref(), app, name)?;
        usable(real(app, false)?)?.set(app, name, value)
    }

    pub(crate) fn delete(app: &str, name: &str) -> io::Result<()> {
        cred_name(app, name)?;
        let dir = std::env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from);
        check_provided(dir.as_deref(), app, name)?;
        usable(real(app, false)?)?.delete(app, name)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const SCRIPT: &str = r#"case "$1" in
encrypt) [ "$1 $2 $3 $4 $5" = "encrypt --user --name=$CRED - -" ] || { echo "bad argv" >&2; exit 9; }; base64 -w0 ;;
decrypt) [ "$1 $2 $3 $4 $5 $6" = "decrypt --user --name=$CRED --newline=no - -" ] || { echo "bad argv" >&2; exit 9; }; base64 -d ;;
esac"#;

        fn dir(tag: &str) -> PathBuf {
            let d = std::env::temp_dir().join(format!("ito-secret-creds-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }

        fn script(cred: &str) -> String {
            format!("CRED={cred}; {SCRIPT}")
        }

        fn names(d: &Path) -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        }

        #[test]
        fn values_round_trip_exactly() {
            let d = dir("rt");
            let cred = cred_name("app", "pw").unwrap();
            let s = script(&cred);
            let base = ["sh", "-c", s.as_str(), "systemd-creds"];
            let c = Creds { base: &base, folder: d.join("f"), provided_dir: None, deadline: DEADLINE };
            assert_eq!(c.get("app", "pw").unwrap(), None);
            for v in ["a\n", "", "日本語", "x\r\n\n", "plain"] {
                c.set("app", "pw", v).unwrap();
                assert_eq!(c.get("app", "pw").unwrap().as_deref(), Some(v), "{v:?}");
            }
            let f = d.join("f");
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(std::fs::metadata(f.join(&cred)).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(names(&f), vec![cred.clone()], "no temporary file is left");
            c.delete("app", "pw").unwrap();
            c.delete("app", "pw").unwrap();
            assert_eq!(c.get("app", "pw").unwrap(), None);
            assert!(names(&f).is_empty());
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn a_big_value_does_not_deadlock() {
            let d = dir("big");
            let cred = cred_name("app", "big").unwrap();
            let s = script(&cred);
            let base = ["sh", "-c", s.as_str(), "systemd-creds"];
            let c = Creds { base: &base, folder: d.join("f"), provided_dir: None, deadline: DEADLINE };
            let v = "x".repeat(200 * 1024);
            c.set("app", "big", &v).unwrap();
            assert_eq!(c.get("app", "big").unwrap().as_deref(), Some(v.as_str()));
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn a_failing_tool_is_an_error_that_names_why() {
            let d = dir("fail");
            let dec = ["sh", "-c", "echo 'Failed to decrypt' >&2; exit 1", "systemd-creds"];
            let c = Creds { base: &dec, folder: d.join("f"), provided_dir: None, deadline: DEADLINE };
            std::fs::create_dir_all(d.join("f")).unwrap();
            std::fs::write(d.join("f").join(cred_name("app", "x").unwrap()), "junk").unwrap();
            let e = c.get("app", "x").unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::Other);
            assert!(e.to_string().ends_with("failed: Failed to decrypt"), "{e}");
            let enc = ["sh", "-c", "echo 'no host key' >&2; exit 1", "systemd-creds"];
            let c = Creds { base: &enc, folder: d.join("g"), provided_dir: None, deadline: DEADLINE };
            let e = c.set("app", "x", "v").unwrap_err();
            assert!(e.to_string().ends_with("failed: no host key"), "{e}");
            assert!(names(&d.join("g")).is_empty(), "no file, no temporary file");
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn text_that_is_not_utf8_is_refused() {
            let d = dir("utf");
            let base = ["sh", "-c", "printf '\\377\\376'", "systemd-creds"];
            let c = Creds { base: &base, folder: d.join("f"), provided_dir: None, deadline: DEADLINE };
            std::fs::create_dir_all(d.join("f")).unwrap();
            std::fs::write(d.join("f").join(cred_name("app", "x").unwrap()), "junk").unwrap();
            assert_eq!(c.get("app", "x").unwrap_err().kind(), io::ErrorKind::InvalidData);
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn a_stored_symlink_is_not_followed() {
            let d = dir("link");
            let f = d.join("f");
            std::fs::create_dir_all(&f).unwrap();
            std::fs::write(d.join("target"), "x").unwrap();
            std::os::unix::fs::symlink(d.join("target"), f.join(cred_name("app", "x").unwrap())).unwrap();
            let base = ["sh", "-c", "cat", "systemd-creds"];
            let c = Creds { base: &base, folder: f, provided_dir: None, deadline: DEADLINE };
            assert_eq!(c.get("app", "x").unwrap_err().kind(), io::ErrorKind::InvalidData);
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn a_name_systemd_provides_is_not_ours_to_write() {
            let d = dir("prov");
            let provided = d.join("p");
            std::fs::create_dir_all(&provided).unwrap();
            std::fs::write(provided.join("app_n"), "v\n").unwrap();
            let base = ["sh", "-c", "exit 9", "systemd-creds"];
            let c = Creds { base: &base, folder: d.join("f"), provided_dir: Some(provided.clone()), deadline: DEADLINE };
            for e in [c.set("app", "n", "x").unwrap_err(), c.delete("app", "n").unwrap_err()] {
                assert_eq!(e.kind(), io::ErrorKind::ReadOnlyFilesystem);
                assert!(crate::is_provided(&e), "{e}");
            }
            assert_eq!(crate::read_credential(&provided, "app", "n").unwrap().as_deref(), Some("v"));
            assert!(!d.join("f").exists(), "nothing was written");
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn a_folder_open_to_others_is_refused() {
            let d = dir("open");
            let f = d.join("f");
            std::fs::create_dir_all(&f).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            let s = script(&cred_name("app", "n").unwrap());
            let base = ["sh", "-c", s.as_str(), "systemd-creds"];
            let c = Creds { base: &base, folder: f.clone(), provided_dir: None, deadline: DEADLINE };
            assert_eq!(c.set("app", "n", "v").unwrap_err().kind(), io::ErrorKind::PermissionDenied);
            assert!(names(&f).is_empty());
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn a_tool_that_hangs_is_killed() {
            let d = dir("slow");
            let base = ["sh", "-c", "exec sleep 5", "systemd-creds"];
            let c = Creds { base: &base, folder: d.join("f"), provided_dir: None, deadline: Duration::from_millis(200) };
            let e = c.set("app", "n", "v").unwrap_err();
            assert_eq!(e.kind(), io::ErrorKind::TimedOut);
            assert!(names(&d.join("f")).is_empty());
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn the_version_probe_reads_a_stand_in() {
            let sh = |script: &'static str| ["sh", "-c", script, "systemd-creds"];
            assert_eq!(probe(&sh("echo 'systemd 257 (x)'")).unwrap(), Avail::Ready(257));
            assert_eq!(probe(&sh("echo 'systemd 255 (x)'")).unwrap(), Avail::TooOld(255));
            assert!(probe(&sh("exit 3")).is_err());
            assert!(probe(&sh("echo hello")).is_err());
            assert_eq!(probe(&["ito-secret-no-such-tool-xyz"]).unwrap(), Avail::Absent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn names_are_checked() {
        for app in ["", "a_b", "a/b", "a\\b", "a\0b", ".a"] {
            assert_eq!(check_app(app).unwrap_err().kind(), io::ErrorKind::InvalidInput, "{app:?}");
            assert_eq!(cred_name(app, "n").unwrap_err().kind(), io::ErrorKind::InvalidInput, "{app:?}");
        }
        for name in ["", "a/b", "a\\b", "a\0b", ".a"] {
            assert_eq!(cred_name("app", name).unwrap_err().kind(), io::ErrorKind::InvalidInput, "{name:?}");
        }
        assert_eq!(cred_name("koyomi", "smtp").unwrap(), "koyomi_smtp");
        assert_eq!(cred_name("koyomi", "a.b-c_d").unwrap(), "koyomi_a.b-c_d");
    }

    #[test]
    fn enc_writes_other_bytes_as_percent_hex() {
        assert_eq!(enc("Az09._-"), "Az09._-");
        assert_eq!(enc("%"), "%25");
        assert_eq!(enc(":"), "%3A");
        assert_eq!(enc(" "), "%20");
        assert_eq!(enc("日"), "%E6%97%A5");
    }

    /// systemd's rule for a credential name: printable ASCII, no `/` or `:`, not `.` or `..`, 1 to 255 bytes.
    fn systemd_valid(s: &str) -> bool {
        (1..=255).contains(&s.len()) && s != "." && s != ".." && s.bytes().all(|b| (0x21..=0x7e).contains(&b) && b != b'/' && b != b':')
    }

    #[test]
    fn credential_names_do_not_collide_and_stay_valid() {
        let a = cred_name("a", "b_c").unwrap();
        let b = cred_name("a", "b%5Fc").unwrap();
        let c = cred_name("a", "b%255Fc").unwrap();
        assert!(a != b && b != c && a != c, "{a} {b} {c}");
        assert!(cred_name("a_b", "c").is_err(), "an app with `_` could take another app's names");
        for (app, name) in [("a", "b"), ("日本", "名前 x:y"), ("a", "%"), ("a.b", "x y z")] {
            let cred = cred_name(app, name).unwrap();
            assert!(systemd_valid(&cred), "{cred}");
        }
    }

    #[test]
    fn a_credential_name_is_at_most_255_bytes() {
        let ok = "x".repeat(253);
        assert_eq!(cred_name("a", &ok).unwrap().len(), 255);
        let long = "x".repeat(254);
        assert_eq!(cred_name("a", &long).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn the_folder_follows_xdg_then_home() {
        let t = std::env::temp_dir();
        let x = OsString::from(t.join("xdg"));
        let h = OsString::from(t.join("home"));
        assert_eq!(folder_from(Some(x.clone()), Some(h.clone()), "app"), Some(t.join("xdg").join("uchmk").join("app").join("credentials")));
        let home = Some(t.join("home").join(".local").join("share").join("uchmk").join("app").join("credentials"));
        assert_eq!(folder_from(None, Some(h.clone()), "app"), home);
        assert_eq!(folder_from(Some(OsString::new()), Some(h.clone()), "app"), home);
        assert_eq!(folder_from(Some(OsString::from("relative")), Some(h.clone()), "app"), home);
        assert_eq!(folder_from(None, None, "app"), None);
        assert_eq!(folder_from(Some(OsString::from("rel")), Some(OsString::from("rel")), "app"), None);
        assert_eq!(folder_from(Some(x), Some(h), "bad_app"), None);
    }

    #[test]
    fn the_version_is_read_from_the_first_line() {
        assert_eq!(parse_version("systemd 257 (257.9-1~deb13u1)\n+PAM +AUDIT\n"), Some(257));
        assert_eq!(parse_version("systemd 252 (252.33-1~deb12u1)"), Some(252));
        assert_eq!(avail_of(252), Avail::TooOld(252));
        assert_eq!(avail_of(256), Avail::Ready(256));
        assert_eq!(parse_version("garbage"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("systemd x"), None);
    }
}
