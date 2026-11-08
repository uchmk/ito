//! A tiny child for tests: a terminal pane whose "shell" is this program records the argv and environment it was started
//! with, so a test can check that a Settings → Shell choice (program, arguments, variables) really reaches the child.
//! std only. An app adds `examples/term-probe.rs` = `fn main() { ito_keyscript::probe::main() }`.
//!
//! Environment: `ITO_PROBE_DIR` (output folder) and `ITO_PROBE_NONCE` (both required, non-empty, the nonce without control
//! characters, else nothing is printed and the exit code is 2); `ITO_PROBE_ENV` (comma-separated names of the variables to record).
//!
//! It writes `probe-<n>.txt` (`n` = the first number whose `.txt` and `.lock` are both absent; the file appears whole, by
//! rename) and only then prints `probe ready <n>`. Then it waits for its stdin to close, at most [`LIMIT`], and exits 0.
//! Exit 1 with `probe error: <why>` on stderr when the file could not be written (a `.tmp` or `.lock` may be left behind).

use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// How long it stays up after `probe ready` when its stdin never closes.
pub const LIMIT: Duration = Duration::from_secs(120);

/// The most probe numbers tried in one folder.
const MAX_N: u32 = 10000;

/// For `examples/term-probe.rs`: reads the real environment, argv and stdin, and exits with [`run`]'s code.
pub fn main() -> ! {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let var = |name: &str| std::env::var_os(name).map(|v| v.to_string_lossy().into_owned());
    let code = run(&var, &args, std::io::stdin(), &mut std::io::stdout(), &mut std::io::stderr(), LIMIT);
    std::process::exit(code)
}

/// The whole program with its inputs passed in; returns the exit code.
pub fn run(
    var: &dyn Fn(&str) -> Option<String>,
    args: &[String],
    stdin: impl Read + Send + 'static,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    limit: Duration,
) -> i32 {
    // A nonce with a control character would break the one-field-per-line file, so it counts as missing.
    let (Some(dir), Some(nonce)) = (
        var("ITO_PROBE_DIR").filter(|d| !d.is_empty()),
        var("ITO_PROBE_NONCE").filter(|n| !n.is_empty() && !n.chars().any(char::is_control)),
    ) else {
        return 2;
    };
    let env: Vec<(String, Option<String>)> = var("ITO_PROBE_ENV")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| (n.to_owned(), var(n)))
        .collect();
    let told = write_probe(Path::new(&dir), &nonce, args, &env, std::process::id()).and_then(|n| {
        writeln!(stdout, "probe ready {n}").and_then(|()| stdout.flush()).map_err(|e| format!("cannot write the ready line: {e}"))
    });
    if let Err(why) = told {
        let _ = writeln!(stderr, "probe error: {why}");
        return 1;
    }
    // Detached: a blocking read must not hold the time limit.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = stdin;
        let _ = std::io::copy(&mut stdin, &mut std::io::sink());
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(limit);
    0
}

/// Whether `p` does not exist (a symlink counts as existing). Any error but "not found" is a failure.
fn absent(p: &Path, shown: &str) -> Result<bool, String> {
    match std::fs::symlink_metadata(p) {
        Ok(_) => Ok(false),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(true),
        Err(e) => Err(format!("cannot check {shown}: {e}")),
    }
}

/// Writes one probe file in `dir`: `Ok(n)` for `probe-<n>.txt`, or the reason it could not (without the `probe error: `
/// prefix). The number is claimed with a `.lock` file made with `create_new`, so two probes started together get two numbers.
pub fn write_probe(dir: &Path, nonce: &str, args: &[String], env: &[(String, Option<String>)], pid: u32) -> Result<u32, String> {
    for n in 1..=MAX_N {
        let (txt, lock, tmp) = (format!("probe-{n}.txt"), format!("probe-{n}.lock"), format!("probe-{n}.tmp"));
        let (txt_p, lock_p) = (dir.join(&txt), dir.join(&lock));
        if !absent(&lock_p, &lock)? || !absent(&txt_p, &txt)? {
            continue;
        }
        match OpenOptions::new().write(true).create_new(true).open(&lock_p) {
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot take {lock}: {e}")),
        }
        if !absent(&txt_p, &txt)? {
            std::fs::remove_file(&lock_p).map_err(|e| format!("cannot remove {lock}: {e}"))?;
            continue;
        }
        let tmp_p = dir.join(&tmp);
        let mut body = format!("nonce: {nonce}\npid: {pid}\nargv: {}\n", json_argv(args));
        for (name, value) in env {
            body.push_str(&format!("env {name}: {}\n", value.as_deref().map_or_else(|| "(unset)".to_owned(), env_value)));
        }
        let mut f = std::fs::File::create(&tmp_p).map_err(|e| format!("cannot create {tmp}: {e}"))?;
        f.write_all(body.as_bytes()).map_err(|e| format!("cannot write {tmp}: {e}"))?;
        f.sync_all().map_err(|e| format!("cannot sync {tmp}: {e}"))?;
        drop(f);
        std::fs::rename(&tmp_p, &txt_p).map_err(|e| format!("cannot rename {tmp} to {txt}: {e}"))?;
        std::fs::remove_file(&lock_p).map_err(|e| format!("cannot remove {lock}: {e}"))?;
        return Ok(n);
    }
    Err(format!("no free probe number up to {MAX_N}"))
}

/// `["a","b c"]`: a JSON array of strings, no spaces after commas.
fn json_argv(args: &[String]) -> String {
    let items: Vec<String> = args.iter().map(|a| json_string(a)).collect();
    format!("[{}]", items.join(","))
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A variable's value on its one line: `\`, LF and CR escaped.
fn env_value(v: &str) -> String {
    v.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    /// A fresh folder, removed when dropped.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("ito-probe-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }

        fn names(&self) -> Vec<String> {
            let mut v: Vec<String> =
                std::fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: Vec<(String, String)> = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        move |k| m.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    /// Runs with an empty stdin; returns (exit code, stdout, stderr).
    fn go(var: &dyn Fn(&str) -> Option<String>, args: &[String]) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(var, args, std::io::empty(), &mut out, &mut err, Duration::from_secs(1));
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn two_runs_take_two_numbers_and_leave_no_lock_or_tmp() {
        let d = Dir::new("two");
        let dir = d.0.to_str().unwrap();
        let var = vars(&[("ITO_PROBE_DIR", dir), ("ITO_PROBE_NONCE", "N1"), ("ITO_PROBE_ENV", " A_SET, ,A_UNSET"), ("A_SET", "hello")]);
        let args = ["-NoLogo".to_owned(), "x y".to_owned(), "q\"\\".to_owned()];
        let (code, out, err) = go(&var, &args);
        assert_eq!((code, out.as_str(), err.as_str()), (0, "probe ready 1\n", ""));
        let (code, out, _) = go(&var, &args);
        assert_eq!((code, out.as_str()), (0, "probe ready 2\n"));
        assert_eq!(d.names(), ["probe-1.txt", "probe-2.txt"]);
        let text = std::fs::read_to_string(d.0.join("probe-1.txt")).unwrap();
        let want = format!(
            "nonce: N1\npid: {}\nargv: [\"-NoLogo\",\"x y\",\"q\\\"\\\\\"]\nenv A_SET: hello\nenv A_UNSET: (unset)\n",
            std::process::id()
        );
        assert_eq!(text, want);
    }

    #[test]
    fn an_existing_txt_or_a_stale_lock_is_skipped() {
        let d = Dir::new("skip");
        let dir = d.0.to_str().unwrap();
        let var = vars(&[("ITO_PROBE_DIR", dir), ("ITO_PROBE_NONCE", "N")]);
        std::fs::write(d.0.join("probe-1.txt"), "old").unwrap();
        assert_eq!(go(&var, &[]).1, "probe ready 2\n");
        std::fs::remove_file(d.0.join("probe-1.txt")).unwrap();
        std::fs::remove_file(d.0.join("probe-2.txt")).unwrap();
        std::fs::write(d.0.join("probe-1.lock"), "").unwrap();
        assert_eq!(go(&var, &[]).1, "probe ready 2\n");
        assert!(d.0.join("probe-1.lock").exists());
        assert!(!d.0.join("probe-1.txt").exists());
        assert!(std::fs::read_to_string(d.0.join("probe-2.txt")).unwrap().contains("argv: []\n"));
    }

    #[test]
    fn missing_or_empty_inputs_exit_2_silently() {
        let d = Dir::new("missing");
        let dir = d.0.to_str().unwrap();
        for var in [
            vars(&[("ITO_PROBE_NONCE", "N")]),
            vars(&[("ITO_PROBE_DIR", dir)]),
            vars(&[("ITO_PROBE_DIR", dir), ("ITO_PROBE_NONCE", "")]),
            vars(&[("ITO_PROBE_DIR", dir), ("ITO_PROBE_NONCE", "N\nnonce: M")]),
        ] {
            let (code, out, _) = go(&var, &[]);
            assert_eq!((code, out.as_str()), (2, ""));
        }
        assert!(d.names().is_empty());
    }

    #[test]
    fn a_folder_that_does_not_exist_is_an_error() {
        let d = Dir::new("nofolder");
        let gone = d.0.join("not-here");
        let var = vars(&[("ITO_PROBE_DIR", gone.to_str().unwrap()), ("ITO_PROBE_NONCE", "N")]);
        let (code, out, err) = go(&var, &[]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.starts_with("probe error: cannot take probe-1.lock:"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_folder_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("ro");
        std::fs::set_permissions(&d.0, std::fs::Permissions::from_mode(0o555)).unwrap();
        let restore = || std::fs::set_permissions(&d.0, std::fs::Permissions::from_mode(0o755)).unwrap();
        if std::fs::File::create(d.0.join("try")).is_ok() {
            restore();
            return;
        }
        let var = vars(&[("ITO_PROBE_DIR", d.0.to_str().unwrap()), ("ITO_PROBE_NONCE", "N")]);
        let (code, out, err) = go(&var, &[]);
        restore();
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.starts_with("probe error:"), "{err}");
    }

    struct Blocked;

    impl Read for Blocked {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_secs(30));
            Ok(0)
        }
    }

    #[test]
    fn a_blocked_stdin_does_not_hold_the_time_limit() {
        let d = Dir::new("wait");
        let var = vars(&[("ITO_PROBE_DIR", d.0.to_str().unwrap()), ("ITO_PROBE_NONCE", "N")]);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let t = Instant::now();
        let code = run(&var, &[], Blocked, &mut out, &mut err, Duration::from_millis(200));
        assert_eq!(code, 0);
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn escaping() {
        assert_eq!(json_argv(&[]), "[]");
        assert_eq!(json_string("a\"b\\c\nd\re\tf\u{1}g"), "\"a\\\"b\\\\c\\nd\\re\\tf\\u0001g\"");
        assert_eq!(json_string("日本"), "\"日本\"");
        assert_eq!(env_value("a\\b\nc\rd\"e"), "a\\\\b\\nc\\rd\"e");
    }
}
