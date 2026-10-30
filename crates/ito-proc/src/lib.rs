//! A child process an app can stop. [`run`] spawns it with piped output, reads
//! stdout and stderr on threads, hands stderr's lines to a callback as they
//! come (a progress line rewrites itself with `\r`, so `\r` ends a line like
//! `\n`) and, when the stop flag is set, kills the child *and everything it
//! started* (an ssh, a credential helper), which a plain `Child::kill` does
//! not.
//!
//! [`run_lines`] is the same for a child that talks a lot: stdout comes line by
//! line too and nothing is collected unless the callback keeps it.
//!
//! A child that is not UTF-8 (a `.bat` or an old exe on Japanese Windows writes
//! CP932) is read with [`run_with`] / [`run_lines_with`] and an [`Encoding`];
//! [`run`] and [`run_lines`] read UTF-8 and replace bad bytes. The raw bytes of
//! stdout are always in [`Done::stdout`].
//!
//! It blocks the calling thread, so call it from a worker, never the UI thread.
//! What the lines mean stays with the caller.

use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How the bytes a child writes become text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-8, bad bytes replaced. What [`run`] and [`run_lines`] use.
    #[default]
    Utf8,
    /// Shift_JIS (Windows-31J, CP932), on any system.
    ShiftJis,
    /// The system's console code page (`GetOEMCP` on Windows: CP932 on Japanese
    /// Windows, CP437 and the like elsewhere there); UTF-8 on other systems.
    Console,
    /// UTF-8 for a line that is valid UTF-8, the console code page for one
    /// that is not. A line is decided on its own, so a child that mixes both
    /// still reads right.
    Auto,
}

impl Encoding {
    /// `bytes` (one line, or all of a pipe) as text.
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Encoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Encoding::ShiftJis => encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes).0.into_owned(),
            Encoding::Console => console().decode_without_bom_handling(bytes).0.into_owned(),
            Encoding::Auto => match std::str::from_utf8(bytes) {
                Ok(t) => t.to_owned(),
                Err(_) => console().decode_without_bom_handling(bytes).0.into_owned(),
            },
        }
    }
}

/// The encoding of the system's console code page.
fn console() -> &'static encoding_rs::Encoding {
    #[cfg(windows)]
    {
        extern "system" {
            fn GetOEMCP() -> u32;
        }
        // SAFETY: a plain call without arguments.
        from_code_page(unsafe { GetOEMCP() })
    }
    #[cfg(not(windows))]
    encoding_rs::UTF_8
}

/// The encoding for a Windows code page number; Western ones that
/// `encoding_rs` has no table for (437, 850 …) read as windows-1252.
#[cfg_attr(not(windows), allow(dead_code))]
fn from_code_page(cp: u32) -> &'static encoding_rs::Encoding {
    match cp {
        932 => encoding_rs::SHIFT_JIS,
        936 | 54936 => encoding_rs::GB18030,
        949 => encoding_rs::EUC_KR,
        950 => encoding_rs::BIG5,
        65001 => encoding_rs::UTF_8,
        874 => encoding_rs::WINDOWS_874,
        1250..=1258 => encoding_rs::Encoding::for_label(format!("windows-{cp}").as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252),
        _ => encoding_rs::WINDOWS_1252,
    }
}

/// How a [`run`] ended.
#[derive(Debug)]
pub enum Outcome {
    /// The child exited by itself, whatever the status.
    Done(Done),
    /// The stop flag was set and the process tree was killed.
    Stopped,
}

/// What a child that ran to its end said.
#[derive(Debug)]
pub struct Done {
    pub status: ExitStatus,
    /// Everything it wrote to stdout.
    pub stdout: Vec<u8>,
    /// The stderr lines the callback asked to keep.
    pub lines: Vec<String>,
}

impl Done {
    /// Whether it exited with status 0.
    pub fn ok(&self) -> bool {
        self.status.success()
    }

    /// stdout as text, bad bytes replaced.
    pub fn stdout_text(&self) -> String {
        self.stdout_as(Encoding::Utf8)
    }

    /// stdout as text in `enc`.
    pub fn stdout_as(&self, enc: Encoding) -> String {
        enc.decode(&self.stdout)
    }
}

/// How long the readers get after the child is gone: a helper it started can
/// still hold the pipes open, and must not hold the caller with them.
const GRACE: Duration = Duration::from_secs(1);

/// How often the stop flag and the child are looked at.
const POLL: Duration = Duration::from_millis(100);

enum Piece {
    Line(String),
    ErrEnd,
    Out(Vec<u8>),
    OutEnd,
}

/// Run `cmd` to its end or until `stop` is set.
///
/// stdout is collected whole. Each line of stderr is passed to `on_line` on the
/// calling thread; it returns whether to keep the line in [`Done::lines`]
/// (a progress line that was shown is not kept, the error text is).
/// The child gets its own process group (Unix) and no console window (Windows).
pub fn run(cmd: Command, stop: &AtomicBool, on_line: &mut dyn FnMut(&str) -> bool) -> io::Result<Outcome> {
    run_with(cmd, stop, Encoding::Utf8, on_line)
}

/// [`run`], reading stderr's lines in `enc`. stdout stays bytes
/// ([`Done::stdout_as`]).
pub fn run_with(mut cmd: Command, stop: &AtomicBool, enc: Encoding, on_line: &mut dyn FnMut(&str) -> bool) -> io::Result<Outcome> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    own_group(&mut cmd);
    let mut child = cmd.spawn()?;
    let (tx, rx) = mpsc::channel();
    let mut stderr = child.stderr.take().expect("piped");
    let err_tx = tx.clone();
    let err = std::thread::Builder::new().name("ito-proc-stderr".into()).spawn(move || {
        let (mut buf, mut line) = ([0u8; 4096], Vec::new());
        let send = |line: &mut Vec<u8>| {
            if !line.is_empty() {
                let _ = err_tx.send(Piece::Line(enc.decode(line)));
                line.clear();
            }
        };
        while let Ok(n @ 1..) = stderr.read(&mut buf) {
            for &b in &buf[..n] {
                if b == b'\r' || b == b'\n' {
                    send(&mut line);
                } else {
                    line.push(b);
                }
            }
        }
        send(&mut line);
        let _ = err_tx.send(Piece::ErrEnd);
    });
    let mut stdout = child.stdout.take().expect("piped");
    let out_tx = tx.clone();
    let out = std::thread::Builder::new().name("ito-proc-stdout".into()).spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = stdout.read(&mut buf) {
            let _ = out_tx.send(Piece::Out(buf[..n].to_vec()));
        }
        let _ = out_tx.send(Piece::OutEnd);
    });
    if let Err(e) = err.and(out) {
        kill_tree(&mut child);
        return Err(e);
    }
    let (mut stdout, mut lines) = (Vec::new(), Vec::new());
    let (mut err_end, mut out_end) = (false, false);
    let mut exited: Option<(ExitStatus, Instant)> = None;
    loop {
        if stop.load(Ordering::Relaxed) {
            kill_tree(&mut child);
            return Ok(Outcome::Stopped);
        }
        // `tx` is still here, so this waits rather than seeing the readers gone.
        match rx.recv_timeout(POLL) {
            Ok(Piece::Line(l)) => {
                if on_line(&l) {
                    lines.push(l);
                }
            }
            Ok(Piece::ErrEnd) => err_end = true,
            Ok(Piece::Out(b)) => stdout.extend(b),
            Ok(Piece::OutEnd) => out_end = true,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
        if exited.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => exited = Some((status, Instant::now())),
                Ok(None) => {}
                Err(e) => {
                    kill_tree(&mut child);
                    return Err(e);
                }
            }
        }
        let Some((status, at)) = exited else { continue };
        if (err_end && out_end) || at.elapsed() > GRACE {
            while let Ok(p) = rx.try_recv() {
                match p {
                    Piece::Line(l) if on_line(&l) => lines.push(l),
                    Piece::Out(b) => stdout.extend(b),
                    _ => {}
                }
            }
            return Ok(Outcome::Done(Done { status, stdout, lines }));
        }
    }
}

/// Which pipe a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

/// How a [`run_lines`] ended, with the lines the callback asked to keep.
#[derive(Debug)]
pub struct Lines {
    /// How it exited; `None` when the stop flag killed it.
    pub status: Option<ExitStatus>,
    /// The kept lines, in the order they came.
    pub kept: Vec<(Stream, String)>,
}

impl Lines {
    /// Whether the child exited by itself with status 0.
    pub fn ok(&self) -> bool {
        self.status.is_some_and(|s| s.success())
    }

    /// Whether the stop flag ended it.
    pub fn stopped(&self) -> bool {
        self.status.is_none()
    }
}

enum Seen {
    Line(Stream, String),
    End(Stream),
}

/// Read `r` to its end, sending each line (ended by CR or LF, empty ones dropped).
fn split_lines(mut r: impl Read, which: Stream, enc: Encoding, tx: mpsc::Sender<Seen>) {
    let (mut buf, mut line) = ([0u8; 4096], Vec::new());
    let send = |line: &mut Vec<u8>| {
        if !line.is_empty() {
            let _ = tx.send(Seen::Line(which, enc.decode(line)));
            line.clear();
        }
    };
    while let Ok(n @ 1..) = r.read(&mut buf) {
        for &b in &buf[..n] {
            if b == b'\r' || b == b'\n' {
                send(&mut line);
            } else {
                line.push(b);
            }
        }
    }
    send(&mut line);
    let _ = tx.send(Seen::End(which));
}

/// Like [`run`], but stdout comes line by line too, split at CR and LF like
/// stderr, and nothing is collected whole: a chatty child costs no memory but
/// the lines `on_line` keeps. `on_line` runs on the calling thread with the
/// pipe and the line, and returns whether to keep it in [`Lines::kept`]; a caller
/// that writes the lines away or shows them live keeps none.
///
/// The kept lines come back whether the child exited or was stopped, so a job
/// killed by its timeout still shows what it said before.
pub fn run_lines(cmd: Command, stop: &AtomicBool, on_line: &mut dyn FnMut(Stream, &str) -> bool) -> io::Result<Lines> {
    run_lines_with(cmd, stop, Encoding::Utf8, on_line)
}

/// [`run_lines`], reading both pipes' lines in `enc`.
pub fn run_lines_with(
    mut cmd: Command,
    stop: &AtomicBool,
    enc: Encoding,
    on_line: &mut dyn FnMut(Stream, &str) -> bool,
) -> io::Result<Lines> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    own_group(&mut cmd);
    let mut child = cmd.spawn()?;
    let (tx, rx) = mpsc::channel();
    let (stdout, stderr) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
    let (out_tx, err_tx) = (tx.clone(), tx.clone());
    let out = std::thread::Builder::new().name("ito-proc-stdout".into()).spawn(move || split_lines(stdout, Stream::Out, enc, out_tx));
    let err = std::thread::Builder::new().name("ito-proc-stderr".into()).spawn(move || split_lines(stderr, Stream::Err, enc, err_tx));
    if let Err(e) = out.and(err) {
        kill_tree(&mut child);
        return Err(e);
    }
    let mut kept = Vec::new();
    let (mut out_end, mut err_end) = (false, false);
    let mut exited: Option<(ExitStatus, Instant)> = None;
    let mut take = |kept: &mut Vec<(Stream, String)>, p: Seen, out_end: &mut bool, err_end: &mut bool| match p {
        Seen::Line(w, l) => {
            if on_line(w, &l) {
                kept.push((w, l));
            }
        }
        Seen::End(Stream::Out) => *out_end = true,
        Seen::End(Stream::Err) => *err_end = true,
    };
    loop {
        if stop.load(Ordering::Relaxed) {
            kill_tree(&mut child);
            // What the readers already got is still the caller's.
            while let Ok(p) = rx.try_recv() {
                take(&mut kept, p, &mut out_end, &mut err_end);
            }
            return Ok(Lines { status: None, kept });
        }
        if let Ok(p) = rx.recv_timeout(POLL) {
            take(&mut kept, p, &mut out_end, &mut err_end);
        }
        if exited.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => exited = Some((status, Instant::now())),
                Ok(None) => {}
                Err(e) => {
                    kill_tree(&mut child);
                    return Err(e);
                }
            }
        }
        let Some((status, at)) = exited else { continue };
        if (out_end && err_end) || at.elapsed() > GRACE {
            while let Ok(p) = rx.try_recv() {
                take(&mut kept, p, &mut out_end, &mut err_end);
            }
            return Ok(Lines { status: Some(status), kept });
        }
    }
}

/// Give the command its own process group on Unix (so [`kill_tree`] can signal
/// them all) and no console window on Windows. [`run`] does this itself; it is
/// public for a caller that spawns on its own and stops with [`kill_tree`].
pub fn own_group(cmd: &mut Command) {
    sys::own_group(cmd);
}

/// Start `cmd` as a background process that outlives the caller and holds
/// none of its pipes: stdio null, no console window, its own process group
/// (Windows: detached; Unix: a new session's group). On Windows a child is
/// otherwise handed every inheritable handle of the caller even with
/// `Stdio::null()`, so whoever waits for the caller's stdout/stderr pipe (a
/// test, `$(app list)`) would never see EOF while the daemon lives. The
/// caller's own standard handles are made non-inheritable for the spawn and
/// put back after. The child is not waited for.
pub fn spawn_detached(mut cmd: Command) -> io::Result<()> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    sys::detach(&mut cmd);
    let _guard = sys::HoldHandles::new();
    cmd.spawn().map(drop)
}

/// Kill the child and every process it started, then reap it.
pub fn kill_tree(child: &mut Child) {
    sys::kill_tree(child);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
mod sys {
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn own_group(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    pub fn detach(cmd: &mut Command) {
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(which: u32) -> *mut std::ffi::c_void;
        fn GetHandleInformation(h: *mut std::ffi::c_void, flags: *mut u32) -> i32;
        fn SetHandleInformation(h: *mut std::ffi::c_void, mask: u32, flags: u32) -> i32;
    }
    const HANDLE_FLAG_INHERIT: u32 = 1;

    /// The caller's standard handles, not inheritable until dropped.
    pub struct HoldHandles(Vec<(*mut std::ffi::c_void, u32)>);

    impl HoldHandles {
        pub fn new() -> Self {
            let mut held = Vec::new();
            // STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE: -10, -11, -12.
            for which in [-10i32, -11, -12] {
                // SAFETY: this process's own standard handles; only their inherit flag changes.
                unsafe {
                    let h = GetStdHandle(which as u32);
                    let mut flags = 0u32;
                    if !h.is_null() && h as isize != -1 && GetHandleInformation(h, &mut flags) != 0 && flags & HANDLE_FLAG_INHERIT != 0 {
                        SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
                        held.push((h, flags & HANDLE_FLAG_INHERIT));
                    }
                }
            }
            HoldHandles(held)
        }
    }

    impl Drop for HoldHandles {
        fn drop(&mut self) {
            for &(h, flag) in &self.0 {
                // SAFETY: the same handles, still open; their inherit flag is put back.
                unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT, flag) };
            }
        }
    }

    pub fn kill_tree(child: &mut Child) {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
}

#[cfg(unix)]
mod sys {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    pub fn own_group(cmd: &mut Command) {
        cmd.process_group(0);
    }

    pub fn detach(cmd: &mut Command) {
        cmd.process_group(0);
    }

    pub struct HoldHandles;

    impl HoldHandles {
        pub fn new() -> Self {
            HoldHandles
        }
    }

    pub fn kill_tree(child: &mut Child) {
        // A negative pid is the whole group, which the child leads.
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(not(any(windows, unix)))]
mod sys {
    use std::process::{Child, Command};

    pub fn own_group(_: &mut Command) {}
    pub fn detach(_: &mut Command) {}
    pub struct HoldHandles;
    impl HoldHandles {
        pub fn new() -> Self {
            HoldHandles
        }
    }
    pub fn kill_tree(_: &mut Child) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command that writes `out` to stdout and `err` to stderr, then exits `code`.
    fn say(out: &str, err: &str, code: i32) -> Command {
        #[cfg(windows)]
        {
            let mut c = Command::new("cmd");
            c.args(["/C", &format!("(echo {out}) & (echo {err} 1>&2) & exit /b {code}")]);
            c
        }
        #[cfg(not(windows))]
        {
            let mut c = Command::new("sh");
            c.args(["-c", &format!("echo {out}; echo {err} >&2; exit {code}")]);
            c
        }
    }

    #[test]
    fn bytes_are_read_in_the_encoding_asked_for() {
        // 日本語 in CP932.
        let sjis = [0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA];
        assert_eq!(Encoding::ShiftJis.decode(&sjis), "日本語");
        assert!(Encoding::Utf8.decode(&sjis).contains('\u{FFFD}'));
        // Auto: valid UTF-8 stays, anything else is the console's.
        assert_eq!(Encoding::Auto.decode("日本語 ok".as_bytes()), "日本語 ok");
        assert_eq!(Encoding::Auto.decode(b"plain"), "plain");
        #[cfg(not(windows))]
        assert!(Encoding::Auto.decode(&sjis).contains('\u{FFFD}'));
        assert_eq!(from_code_page(932).name(), "Shift_JIS");
        assert_eq!(from_code_page(437).name(), "windows-1252");
        assert_eq!(from_code_page(1251).name(), "windows-1251");
    }

    #[test]
    #[cfg(unix)]
    fn run_lines_with_decodes_a_child_that_writes_shift_jis() {
        let stop = AtomicBool::new(false);
        let mut c = Command::new("sh");
        c.args(["-c", r"printf '\223\372\226\173\214\352\nok\n'"]);
        let mut got = Vec::new();
        run_lines_with(c, &stop, Encoding::ShiftJis, &mut |_, l| {
            got.push(l.to_owned());
            false
        })
        .unwrap();
        assert_eq!(got, ["日本語", "ok"]);
    }

    #[test]
    fn a_detached_process_starts_and_a_missing_one_is_an_error() {
        spawn_detached(say("x", "y", 0)).unwrap();
        assert!(spawn_detached(Command::new("ito-proc-no-such-program")).is_err());
    }

    fn sleeper() -> Command {
        #[cfg(windows)]
        {
            let mut c = Command::new("ping");
            c.args(["-n", "60", "127.0.0.1"]);
            c
        }
        #[cfg(not(windows))]
        {
            let mut c = Command::new("sleep");
            c.arg("60");
            c
        }
    }

    /// Set `stop` after a moment while `job` runs.
    fn stopped_soon(job: Command) -> (io::Result<Outcome>, Duration) {
        let stop = AtomicBool::new(false);
        let began = Instant::now();
        let r = std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                stop.store(true, Ordering::Relaxed);
            });
            run(job, &stop, &mut |_| true)
        });
        (r, began.elapsed())
    }

    #[test]
    fn it_collects_stdout_and_the_kept_lines_and_the_status() {
        let stop = AtomicBool::new(false);
        let mut seen = Vec::new();
        let r = run(say("hello", "oops", 3), &stop, &mut |l| {
            seen.push(l.trim().to_owned());
            true
        })
        .unwrap();
        let Outcome::Done(d) = r else { panic!("{r:?}") };
        assert!(!d.ok());
        assert_eq!(d.status.code(), Some(3));
        assert_eq!(d.stdout_text().trim(), "hello");
        assert_eq!(seen, ["oops"]);
        assert_eq!(d.lines.len(), 1);
    }

    #[test]
    fn a_line_the_callback_takes_is_not_kept() {
        let stop = AtomicBool::new(false);
        let Outcome::Done(d) = run(say("x", "progress", 0), &stop, &mut |_| false).unwrap() else { panic!() };
        assert!(d.ok());
        assert!(d.lines.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn a_carriage_return_ends_a_line_like_a_newline() {
        let stop = AtomicBool::new(false);
        let mut c = Command::new("sh");
        c.args(["-c", r"printf 'a 1%%\ra 2%%\ra 3%%\nend\n' >&2"]);
        let Outcome::Done(d) = run(c, &stop, &mut |_| true).unwrap() else { panic!() };
        assert_eq!(d.lines, ["a 1%", "a 2%", "a 3%", "end"]);
    }

    #[test]
    fn run_lines_gives_both_pipes_line_by_line() {
        let stop = AtomicBool::new(false);
        let mut seen = Vec::new();
        let r = run_lines(say("hello", "oops", 3), &stop, &mut |w, l| {
            seen.push((w, l.trim().to_owned()));
            w == Stream::Err
        })
        .unwrap();
        assert!(!r.ok() && !r.stopped());
        assert_eq!(r.status.and_then(|s| s.code()), Some(3));
        assert!(seen.contains(&(Stream::Out, "hello".into())) && seen.contains(&(Stream::Err, "oops".into())), "{seen:?}");
        // Only what the callback kept comes back.
        assert_eq!(r.kept.len(), 1);
        assert_eq!(r.kept[0].0, Stream::Err);
    }

    #[test]
    fn run_lines_keeps_what_was_said_before_a_stop() {
        #[cfg(windows)]
        let job = {
            let mut c = Command::new("cmd");
            c.args(["/C", "echo before & ping -n 60 127.0.0.1"]);
            c
        };
        #[cfg(not(windows))]
        let job = {
            let mut c = Command::new("sh");
            c.args(["-c", "echo before; sleep 60"]);
            c
        };
        let stop = AtomicBool::new(false);
        let r = std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(1500));
                stop.store(true, Ordering::Relaxed);
            });
            run_lines(job, &stop, &mut |_, _| true)
        })
        .unwrap();
        assert!(r.stopped() && !r.ok());
        assert!(r.kept.iter().any(|(w, l)| *w == Stream::Out && l.trim() == "before"), "{:?}", r.kept);
    }

    #[test]
    fn a_missing_program_is_an_error() {
        let stop = AtomicBool::new(false);
        assert!(run(Command::new("ito-proc-no-such-program"), &stop, &mut |_| true).is_err());
    }

    #[test]
    fn the_stop_flag_kills_a_long_job_at_once() {
        let (r, took) = stopped_soon(sleeper());
        assert!(matches!(r.unwrap(), Outcome::Stopped));
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    #[test]
    fn stopping_kills_what_the_child_started() {
        // cmd / sh starts the sleeper, which holds the pipes; killing only the parent would leave it.
        #[cfg(windows)]
        let c = {
            let mut c = Command::new("cmd");
            c.args(["/C", "ping -n 60 127.0.0.1"]);
            c
        };
        #[cfg(not(windows))]
        let c = {
            let mut c = Command::new("sh");
            c.args(["-c", "sleep 60 & wait"]);
            c
        };
        let (r, took) = stopped_soon(c);
        assert!(matches!(r.unwrap(), Outcome::Stopped));
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    #[test]
    #[cfg(unix)]
    fn a_helper_holding_the_pipes_does_not_hold_the_caller() {
        // The shell exits at once; the background sleep keeps stdout and stderr open.
        let stop = AtomicBool::new(false);
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 30 & echo done"]);
        let began = Instant::now();
        let Outcome::Done(d) = run(c, &stop, &mut |_| true).unwrap() else { panic!() };
        assert!(d.ok());
        assert_eq!(d.stdout_text().trim(), "done");
        assert!(began.elapsed() < Duration::from_secs(10));
    }
}
