//! A child process an app can stop. [`run`] spawns it with piped output, reads
//! stdout and stderr on threads, hands stderr's lines to a callback as they
//! come (a progress line rewrites itself with `\r`, so `\r` ends a line like
//! `\n`) and, when the stop flag is set, kills the child *and everything it
//! started* (an ssh, a credential helper), which a plain `Child::kill` does
//! not.
//!
//! It blocks the calling thread, so call it from a worker, never the UI thread.
//! What the lines mean stays with the caller.

use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

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
        String::from_utf8_lossy(&self.stdout).into_owned()
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
pub fn run(mut cmd: Command, stop: &AtomicBool, on_line: &mut dyn FnMut(&str) -> bool) -> io::Result<Outcome> {
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
                let _ = err_tx.send(Piece::Line(String::from_utf8_lossy(line).into_owned()));
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

/// Give the command its own process group on Unix (so [`kill_tree`] can signal
/// them all) and no console window on Windows. [`run`] does this itself; it is
/// public for a caller that spawns on its own and stops with [`kill_tree`].
pub fn own_group(cmd: &mut Command) {
    sys::own_group(cmd);
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
