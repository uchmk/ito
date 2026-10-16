//! The Windows half: starting `<app>.exe`, the handles it gets, and the
//! console for [`crate::out`] and [`crate::err`].

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use windows::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    GetFileType, FILE_ATTRIBUTE_TEMPORARY, FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_CHAR,
    FILE_TYPE_DISK, FILE_TYPE_PIPE,
};
use windows::Win32::System::Console::{
    AttachConsole, GetConsoleMode, GetStdHandle, WriteConsoleW, ATTACH_PARENT_PROCESS, CONSOLE_MODE, STD_ERROR_HANDLE, STD_HANDLE,
    STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows::Win32::System::Threading::{WaitForInputIdle, WaitForSingleObject};

use crate::{route, Kind, Route, Stream};

pub(crate) fn front(app: &str, answers_in_text: impl Fn(&[OsString]) -> bool) -> ! {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let exe = match std::env::current_exe() {
        Ok(me) => me.with_file_name(format!("{app}.exe")),
        Err(e) => fail(&format!("{app}.com: cannot tell where it is: {e}")),
    };
    if !exe.is_file() {
        fail(&format!("{app}.com: no {app}.exe beside it ({})", exe.display()));
    }
    // Before either kind of start: `Command` hands a child every inheritable
    // handle, not only the three it is given. It duplicates the ones it is
    // told to inherit, so `status()` still passes them on.
    keep_own_handles();
    let mut cmd = Command::new(&exe);
    cmd.args(&args);
    if answers_in_text(&args) {
        match cmd.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(e) => fail(&format!("{app}.com: {}: {e}", exe.display())),
        }
    }
    let (out, out_spool) = for_the_window(app, Stream::Out);
    let (err, err_spool) = for_the_window(app, Stream::Err);
    cmd.stdin(Stdio::null()).stdout(out).stderr(err);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => fail(&format!("{app}.com: {}: {e}", exe.display())),
    };
    // The window has its own handles to the spools now; these are this
    // program's, and going with them leaves the window's the last.
    drop(cmd);
    let code = until_the_window_is_up(&mut child);
    for (spool, to) in [(out_spool, Stream::Out), (err_spool, Stream::Err)] {
        if let Some(spool) = spool {
            copy_out(spool, to);
        }
    }
    std::process::exit(code)
}

fn fail(why: &str) -> ! {
    crate::err(&format!("{why}\n"));
    std::process::exit(2)
}

fn std_handle(which: Stream) -> STD_HANDLE {
    match which {
        Stream::Out => STD_OUTPUT_HANDLE,
        Stream::Err => STD_ERROR_HANDLE,
    }
}

/// This program's standard handle, when it has one.
fn handle(which: STD_HANDLE) -> Option<HANDLE> {
    // SAFETY: asks for this process's own standard handle; a missing one is
    // null or invalid, not an error.
    unsafe { GetStdHandle(which) }.ok().filter(|h| !h.is_invalid() && !h.0.is_null())
}

fn kind(which: Stream) -> Kind {
    let Some(h) = handle(std_handle(which)) else {
        return Kind::None;
    };
    // SAFETY: a handle this process holds; both only ask about it.
    match unsafe { GetFileType(h) } {
        FILE_TYPE_CHAR => {
            let mut mode = CONSOLE_MODE::default();
            match unsafe { GetConsoleMode(h, &mut mode) } {
                Ok(()) => Kind::Console,
                Err(_) => Kind::File,
            }
        }
        FILE_TYPE_DISK => Kind::File,
        FILE_TYPE_PIPE => Kind::Pipe,
        _ => Kind::None,
    }
}

/// Makes this program's standard handles stay here.
fn keep_own_handles() {
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if let Some(h) = handle(which) {
            // SAFETY: this process's own handle; only its inheritance changes.
            let _ = unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) };
        }
    }
}

/// The handle the window writes `which` to, and the spool to copy out here
/// when there is one. Without a spool, a pipe becomes this console, or
/// nothing when there is none.
fn for_the_window(app: &str, which: Stream) -> (Stdio, Option<File>) {
    match route(kind(which)) {
        Route::Inherit => (Stdio::inherit(), None),
        Route::Spool => match spool(app, which) {
            Ok((write, read)) => (write.into(), Some(read)),
            Err(_) => match OpenOptions::new().read(true).write(true).open("CONOUT$") {
                Ok(console) => (console.into(), None),
                Err(_) => (Stdio::null(), None),
            },
        },
        Route::Null => (Stdio::null(), None),
    }
}

/// A file in the temporary folder for the window to write `which` to.
fn spool(app: &str, which: Stream) -> std::io::Result<(File, File)> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let name = match which {
        Stream::Out => "out",
        Stream::Err => "err",
    };
    spool_at(&std::env::temp_dir().join(format!("{app}-com-{}-{nanos}.{name}", std::process::id())))
}

/// Makes the file at `path`, and gives a handle to write to it and one of
/// its own to read it back from the start, whatever the other has written.
/// The file goes once every handle to it has, so when the window exits,
/// however it does, nothing is left behind.
fn spool_at(path: &Path) -> std::io::Result<(File, File)> {
    let share = (FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0;
    let write = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(share)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE.0)
        .attributes(FILE_ATTRIBUTE_TEMPORARY.0)
        .open(path)?;
    // Opened before the window starts: once every write handle has gone,
    // the file can no longer be opened, only read through what is open.
    let read = OpenOptions::new().read(true).share_mode(share).open(path)?;
    Ok((write, read))
}

/// Copies what the window has written so far to this program's own output.
fn copy_out(mut spool: File, to: Stream) {
    let mut text = Vec::new();
    let _ = spool.read_to_end(&mut text);
    if text.is_empty() {
        return;
    }
    let _ = match to {
        Stream::Out => crate::put(&mut std::io::stdout().lock(), &text),
        Stream::Err => crate::put(&mut std::io::stderr().lock(), &text),
    };
}

/// Waits for the window to start taking input, then lets it be: the prompt
/// comes back with the app still open. If `<app>.exe` exits first, it
/// refused the command line or failed, and its exit code is this one's. The
/// cap is for a machine so slow that neither has happened yet (tsumugi's
/// `attach` asks its server before the window starts).
fn until_the_window_is_up(child: &mut Child) -> i32 {
    let process = HANDLE(child.as_raw_handle());
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(1),
            Ok(None) => {}
            Err(_) => return 0,
        }
        // SAFETY: the child's process handle, open for as long as `child` is.
        match unsafe { WaitForInputIdle(process, 50) } {
            // Idle: its message loop is running and waiting for input -- or
            // it has exited, which WaitForInputIdle also answers with 0
            // (kura's CI caught that). So look again, giving an exit that is
            // already under way a moment to land.
            0 => {
                if unsafe { WaitForSingleObject(process, 250) } == WAIT_OBJECT_0 {
                    return match child.try_wait() {
                        Ok(Some(status)) => status.code().unwrap_or(1),
                        _ => 1,
                    };
                }
                return 0;
            }
            // WAIT_TIMEOUT: not yet.
            0x102 => {}
            // WAIT_FAILED: no message queue to wait on, which is a debug
            // build (a console program). Give a refusal its moment, then go.
            _ => {
                std::thread::sleep(std::time::Duration::from_millis(300));
                return match child.try_wait() {
                    Ok(Some(status)) => status.code().unwrap_or(1),
                    _ => 0,
                };
            }
        }
        if start.elapsed() > std::time::Duration::from_secs(10) {
            return 0;
        }
    }
}

/// Writes `text` to a console when `to` is neither a file nor a pipe and
/// there is one, joining the console of the program that started this one
/// if this one has none. Says whether it did.
pub(crate) fn to_console(text: &str, to: Stream) -> bool {
    if matches!(kind(to), Kind::File | Kind::Pipe) {
        return false;
    }
    static JOIN: std::sync::Once = std::sync::Once::new();
    // SAFETY: fails harmlessly when this program has a console already or
    // its parent has none.
    JOIN.call_once(|| {
        let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    });
    let Ok(file) = OpenOptions::new().read(true).write(true).open("CONOUT$") else {
        return false;
    };
    let console = HANDLE(file.as_raw_handle());
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut rest = &wide[..];
    while !rest.is_empty() {
        let mut done = 0u32;
        // SAFETY: a console handle open for writing, held by `file` until
        // the end of this function.
        if unsafe { WriteConsoleW(console, rest, Some(&mut done), None) }.is_err() || done == 0 {
            break;
        }
        rest = &rest[(done as usize).min(rest.len())..];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("ito-console-{label}-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// The reader starts at the beginning and sees what was written since,
    /// a write after the reader has gone still succeeds, and the file goes
    /// with the last handle.
    #[test]
    fn a_spool_reads_back_and_goes_with_its_handles() {
        let path = temp("spool");
        let (mut write, mut read) = spool_at(&path).unwrap();
        write.write_all(b"app: --keys: no such key\n").unwrap();
        let mut said = String::new();
        read.read_to_string(&mut said).unwrap();
        assert_eq!(said, "app: --keys: no such key\n");
        write.write_all(b"later\n").unwrap();
        read.read_to_string(&mut said).unwrap();
        assert!(said.ends_with("later\n"), "{said:?}");
        drop(read);
        write.write_all(b"nobody reads this\n").unwrap();
        drop(write);
        assert!(!path.exists());
    }

    /// What a child process wrote and exited with is still read back,
    /// although its handle (the last one to write) has gone.
    #[test]
    fn a_spool_keeps_what_an_exited_child_wrote() {
        let path = temp("child");
        let (write, mut read) = spool_at(&path).unwrap();
        let status = Command::new("cmd").args(["/c", "echo refused"]).stdout(write).stderr(Stdio::null()).status().unwrap();
        assert!(status.success());
        let mut said = String::new();
        read.read_to_string(&mut said).unwrap();
        assert_eq!(said.trim_end(), "refused");
        drop(read);
        assert!(!path.exists());
    }
}
