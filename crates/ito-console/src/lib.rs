//! The console front of a windowed program on Windows, and writers for its
//! answers that let them go rather than panic.
//!
//! Each uchmk app's `<app>.exe` is a windowed program, so PowerShell does not
//! wait for it at the end of a pipeline and does not take its output:
//! `$v = & kura.exe --version` came back empty, and `kura env` printed after
//! the next prompt (kura's #160, #176, #183). Each release zip ships a small
//! console program beside it as `<app>.com`. `PATHEXT` lists `.COM` before
//! `.EXE`, so typing `<app>` finds that one first. It is the shape of Visual
//! Studio's `devenv.com` and `devenv.exe`. [`front`] is the whole of that
//! program but the app's own rule for which command lines answer in text.
//!
//! [`out`] and [`err`] are for the answers `<app>.exe` prints. Run without
//! the front, PowerShell hands it a pipe for `$v = & app.exe --version` and
//! closes it without waiting, and `println!` panics there (os error 232).
//!
//! ```no_run
//! // src/bin/app-com.rs
//! fn answers_in_text(args: &[std::ffi::OsString]) -> bool {
//!     ito_console::any_of(args, &["--version", "-V", "--help", "-h", "env"], &["--keys"])
//! }
//!
//! fn main() {
//!     ito_console::front("app", answers_in_text)
//! }
//! ```

use std::ffi::OsString;

#[cfg(windows)]
mod windows;

/// Runs `<app>.exe`, the program beside this one, for the command line this
/// one was given, and exits with what it should.
///
/// When `answers_in_text` says the command line is answered in text (the
/// version, the help, a report, a server that runs until it is stopped), it
/// runs the exe with the same arguments and the same standard handles, waits,
/// and passes on the exit code. Being a console program, this is what the
/// shell waits for. Anything else opens the window: this starts the exe and
/// returns as soon as the window is taking input, much as typing `<app>.exe`
/// always did. It waits that long, not less, so that a command line the exe
/// refuses before opening a window (`--keys "<Tab"`) still prints its one
/// line here and passes on its exit code.
///
/// The exe gets no handle of this program's but the three it is given.
/// A pipe's reader waits until every process holding the pipe has let go,
/// and with the window (or a server it started) holding the pipe of
/// `| Tee-Object` or `$id = & tsumugi new`, that was until they went
/// (kura's #192).
///
/// The window writes where this program's own output goes when that is a
/// console or a file. When it is a pipe, the window writes into a file of its
/// own in the temporary folder instead, which this copies to the pipe once
/// the window is up or has failed, and which goes when the window exits.
/// A pipe would be held by the window, and once this program had gone,
/// nobody would read it, and the window's next line would panic it; a file
/// never breaks and never fills.
///
/// Elsewhere than Windows there is nothing to split, and it says so and
/// exits 2.
pub fn front(app: &str, answers_in_text: impl Fn(&[OsString]) -> bool) -> ! {
    #[cfg(windows)]
    {
        windows::front(app, answers_in_text)
    }
    #[cfg(not(windows))]
    {
        let _ = answers_in_text;
        err(&format!("{app}-com is the console front for {app}.exe, and only does anything on Windows\n"));
        std::process::exit(2)
    }
}

/// Whether any argument names one of `answers`, read left to right the way an
/// option parser reads them: the argument after one of `takes_value` is its
/// value and never a command (`--keys env` types three letters). The rule of
/// kura's and yagura's fronts.
pub fn any_of(args: &[OsString], answers: &[&str], takes_value: &[&str]) -> bool {
    let mut args = args.iter().map(|a| a.to_str().unwrap_or(""));
    while let Some(a) = args.next() {
        if answers.contains(&a) {
            return true;
        }
        if takes_value.contains(&a) {
            args.next();
        }
    }
    false
}

/// Writes `text` to standard output as it is (no newline is added), and lets
/// it go when it cannot be written.
///
/// On Windows, a file or a pipe takes it as bytes. With neither, as for a
/// windowed program started from a console, it goes to the console of the
/// program that started this one, if there is one: `cmd`'s `kura.exe
/// --version` printed nothing before (kura's #81, #84, #88, #93). It is
/// written as UTF-16 there, so the console's code page does not matter.
pub fn out(text: &str) {
    write(text, Stream::Out);
}

/// [`out`] on standard error: for a line about the output rather than the
/// output itself (`wrote <path>`), which `$p = & app env --out x.txt` would
/// otherwise take as part of it.
pub fn err(text: &str) {
    write(text, Stream::Err);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    Out,
    Err,
}

fn write(text: &str, to: Stream) {
    #[cfg(windows)]
    if windows::to_console(text, to) {
        return;
    }
    let _ = match to {
        Stream::Out => put(&mut std::io::stdout().lock(), text.as_bytes()),
        Stream::Err => put(&mut std::io::stderr().lock(), text.as_bytes()),
    };
}

/// Writes all of `bytes` and flushes them, or stops at the first error and
/// says so. Nobody asks, but nothing panics.
fn put(to: &mut impl std::io::Write, bytes: &[u8]) -> std::io::Result<()> {
    to.write_all(bytes)?;
    to.flush()
}

/// What one of this program's standard handles is, as far as where the
/// window writes goes.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Console,
    /// A file, or a character device that is not a console (`NUL`).
    File,
    Pipe,
    /// Missing, or of a kind Windows does not say.
    None,
}

/// Where the window's output goes.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// This program's own: the window may keep it as long as it likes.
    Inherit,
    /// A file of the window's own, copied out here until the window is up.
    Spool,
    Null,
}

#[cfg(any(windows, test))]
fn route(kind: Kind) -> Route {
    match kind {
        Kind::Console | Kind::File => Route::Inherit,
        Kind::Pipe => Route::Spool,
        Kind::None => Route::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(args: &[&str]) -> Vec<OsString> {
        args.iter().map(Into::into).collect()
    }

    /// A command anywhere in the line counts, but not as an option's value.
    #[test]
    fn any_of_reads_past_option_values() {
        let answers = ["env", "--version"];
        let takes_value = ["--keys", "--cwd-file"];
        for yes in [&["env"][..], &["--version"], &["/tmp", "env"], &["--keys", "x", "env"]] {
            assert!(any_of(&line(yes), &answers, &takes_value), "{yes:?}");
        }
        for no in [&[][..], &["/tmp"], &["--keys", "env"], &["--cwd-file", "--version"], &["environment"]] {
            assert!(!any_of(&line(no), &answers, &takes_value), "{no:?}");
        }
    }

    /// The window keeps a console or a file, gets a file of its own for a
    /// pipe, and nothing for nothing.
    #[test]
    fn a_pipe_is_never_handed_to_the_window() {
        assert_eq!(route(Kind::Console), Route::Inherit);
        assert_eq!(route(Kind::File), Route::Inherit);
        assert_eq!(route(Kind::Pipe), Route::Spool);
        assert_eq!(route(Kind::None), Route::Null);
    }

    /// A writer that fails is let go, with no panic and no retry.
    #[test]
    fn a_closed_pipe_is_let_go() {
        struct Closed;
        impl std::io::Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
        }
        assert!(put(&mut Closed, b"app 1.0.0\n").is_err());
        let mut kept = Vec::new();
        put(&mut kept, b"app 1.0.0\n").unwrap();
        assert_eq!(kept, b"app 1.0.0\n");
    }
}
