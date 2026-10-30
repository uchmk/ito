//! Windows: start a child as another account with `CreateProcessWithLogonW`, its standard handles
//! pipes of ours and the child in a job object of its own (`TerminateJobObject` ends the tree, and
//! the job's `KILL_ON_JOB_CLOSE` ends it if we die). `taskkill` cannot be used for it: this process
//! may not touch another account's processes by id, only through the handles it was given.

use std::ffi::{c_void, OsStr};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::os::windows::process::ExitStatusExt;
use std::process::{Command, ExitStatus};

use crate::{Logon, Proc};

type Handle = *mut c_void;

#[repr(C)]
struct SecurityAttributes {
    length: u32,
    descriptor: *mut c_void,
    inherit: i32,
}

#[repr(C)]
struct StartupInfo {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_chars: u32,
    y_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    reserved2_len: u16,
    reserved2: *mut u8,
    std_input: Handle,
    std_output: Handle,
    std_error: Handle,
}

#[repr(C)]
struct ProcessInformation {
    process: Handle,
    thread: Handle,
    process_id: u32,
    thread_id: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreatePipe(read: *mut Handle, write: *mut Handle, attrs: *const SecurityAttributes, size: u32) -> i32;
    fn CloseHandle(h: Handle) -> i32;
    fn SetHandleInformation(h: Handle, mask: u32, flags: u32) -> i32;
    fn ResumeThread(thread: Handle) -> u32;
    fn TerminateProcess(process: Handle, code: u32) -> i32;
    fn TerminateJobObject(job: Handle, code: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn WaitForSingleObject(h: Handle, millis: u32) -> u32;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn LogonUserW(user: *const u16, domain: *const u16, password: *const u16, logon_type: u32, provider: u32, token: *mut Handle) -> i32;
    fn CreateProcessWithLogonW(
        user: *const u16,
        domain: *const u16,
        password: *const u16,
        logon_flags: u32,
        application: *const u16,
        command_line: *mut u16,
        creation_flags: u32,
        environment: *const c_void,
        current_directory: *const u16,
        startup: *const StartupInfo,
        info: *mut ProcessInformation,
    ) -> i32;
}

#[link(name = "userenv")]
extern "system" {
    fn CreateEnvironmentBlock(environment: *mut *mut c_void, token: Handle, inherit: i32) -> i32;
    fn DestroyEnvironmentBlock(environment: *mut c_void) -> i32;
}

const LOGON32_LOGON_INTERACTIVE: u32 = 2;
const LOGON_WITH_PROFILE: u32 = 1;
const CREATE_SUSPENDED: u32 = 0x4;
const CREATE_UNICODE_ENVIRONMENT: u32 = 0x400;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const STARTF_USESTDHANDLES: u32 = 0x100;
const HANDLE_FLAG_INHERIT: u32 = 1;
const WAIT_OBJECT_0: u32 = 0;

/// A wide string that is overwritten when it goes away (the password).
struct Secret(Vec<u16>);

impl Drop for Secret {
    fn drop(&mut self) {
        for c in &mut self.0 {
            // SAFETY: a valid element; volatile so the clearing is not optimised away.
            unsafe { std::ptr::write_volatile(c, 0) };
        }
    }
}

/// The started child: its process and the job that holds it and whatever it starts.
pub struct Process {
    process: OwnedHandle,
    job: OwnedHandle,
}

impl Proc for Process {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        use std::os::windows::io::AsRawHandle;
        let h = self.process.as_raw_handle();
        // SAFETY: the process handle is open as long as `self` is.
        unsafe {
            if WaitForSingleObject(h, 0) != WAIT_OBJECT_0 {
                return Ok(None);
            }
            let mut code = 0u32;
            if GetExitCodeProcess(h, &mut code) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Some(ExitStatus::from_raw(code)))
        }
    }

    fn kill(&mut self) {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: both handles are open as long as `self` is.
        unsafe {
            TerminateJobObject(self.job.as_raw_handle(), 1);
            TerminateProcess(self.process.as_raw_handle(), 1);
        }
    }
}

/// A pipe with the parent's end not inheritable: `(parent's end, child's end)`.
fn pipe(parent_reads: bool) -> io::Result<(OwnedHandle, OwnedHandle)> {
    use std::os::windows::io::AsRawHandle;
    let sa = SecurityAttributes { length: size_of::<SecurityAttributes>() as u32, descriptor: std::ptr::null_mut(), inherit: 1 };
    let (mut r, mut w): (Handle, Handle) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: out-pointers to two handles; they are owned straight away.
    let (r, w) = unsafe {
        if CreatePipe(&mut r, &mut w, &sa, 0) == 0 {
            return Err(io::Error::last_os_error());
        }
        (OwnedHandle::from_raw_handle(r), OwnedHandle::from_raw_handle(w))
    };
    let (parent, child) = if parent_reads { (r, w) } else { (w, r) };
    // SAFETY: the handle is open; only its inherit flag changes.
    if unsafe { SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((parent, child))
}

fn wide(s: &str, what: &str) -> io::Result<Vec<u16>> {
    if s.contains('\0') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{what} has a NUL character")));
    }
    Ok(s.encode_utf16().chain(Some(0)).collect())
}

/// `arg` quoted the way the C runtime (and `std`) reads a command line back.
fn push_arg(out: &mut Vec<u16>, arg: &OsStr) {
    let w: Vec<u16> = arg.encode_wide().collect();
    let quote = w.is_empty() || w.iter().any(|&c| matches!(c, 0x20 | 0x09 | 0x0a | 0x0b));
    if quote {
        out.push(b'"' as u16);
    }
    let mut backslashes = 0;
    for &c in &w {
        if c == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        if c == b'"' as u16 {
            out.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
        }
        backslashes = 0;
        out.push(c);
    }
    if quote {
        out.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
        out.push(b'"' as u16);
    } else {
        out.extend(std::iter::repeat_n(b'\\' as u16, backslashes));
    }
}

fn command_line(cmd: &Command) -> io::Result<Vec<u16>> {
    let mut line = Vec::new();
    push_arg(&mut line, cmd.get_program());
    for a in cmd.get_args() {
        line.push(b' ' as u16);
        push_arg(&mut line, a);
    }
    if line.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the command has a NUL character"));
    }
    line.push(0);
    Ok(line)
}

/// `(user, domain)` for the call: `DOMAIN\user` is split, `user@domain` goes whole with no domain.
fn account<'a>(logon: &Logon<'a>) -> (&'a str, Option<&'a str>) {
    if logon.domain.is_some() {
        return (logon.user, logon.domain);
    }
    match logon.user.split_once('\\') {
        Some((d, u)) => (u, Some(d)),
        None => (logon.user, None),
    }
}

/// The environment block for the child: the account's own, with `cmd`'s changes over it.
fn environment(cmd: &Command, user: &[u16], domain: Option<&[u16]>, password: &Secret) -> io::Result<Vec<u16>> {
    let mut token: Handle = std::ptr::null_mut();
    // SAFETY: NUL-terminated strings and an out-pointer to the token.
    let ok = unsafe {
        LogonUserW(
            user.as_ptr(),
            domain.map_or(std::ptr::null(), |d| d.as_ptr()),
            password.0.as_ptr(),
            LOGON32_LOGON_INTERACTIVE,
            0,
            &mut token,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut block: *mut c_void = std::ptr::null_mut();
    // SAFETY: the token is open; the out-pointer receives a block we destroy below.
    let made = unsafe { CreateEnvironmentBlock(&mut block, token, 0) };
    let err = (made == 0).then(io::Error::last_os_error);
    // SAFETY: the token was made above and is not used again.
    unsafe { CloseHandle(token) };
    if let Some(e) = err {
        return Err(e);
    }
    // SAFETY: the block is a sequence of NUL-terminated strings ended by an empty one; read up to there.
    let own = unsafe {
        let p = block.cast::<u16>();
        let mut n = 0;
        while *p.add(n) != 0 {
            while *p.add(n) != 0 {
                n += 1;
            }
            n += 1;
        }
        std::slice::from_raw_parts(p, n).to_vec()
    };
    // SAFETY: made by CreateEnvironmentBlock and copied out above.
    unsafe { DestroyEnvironmentBlock(block) };
    let changes = cmd.get_envs().map(|(k, v)| (k.encode_wide().collect::<Vec<u16>>(), v.map(|v| v.encode_wide().collect::<Vec<u16>>())));
    Ok(overlay(&own, changes))
}

/// The upper-cased name of a `NAME=value` entry (a leading `=`, as in `=C:=C:\`, belongs to the name).
fn name_of(entry: &[u16]) -> String {
    let at = entry.iter().skip(1).position(|&c| c == b'=' as u16).map_or(entry.len(), |i| i + 1);
    String::from_utf16_lossy(&entry[..at]).to_uppercase()
}

/// `block` (entries ended by NUL) with `changes` over it (`Some` sets, `None` removes; names compare
/// ignoring case), as a block again: sorted by name, each entry ended by NUL and the whole by one more.
fn overlay(block: &[u16], changes: impl IntoIterator<Item = (Vec<u16>, Option<Vec<u16>>)>) -> Vec<u16> {
    let mut entries: Vec<Vec<u16>> = block.split(|&c| c == 0).filter(|e| !e.is_empty()).map(<[u16]>::to_vec).collect();
    for (name, value) in changes {
        let key = String::from_utf16_lossy(&name).to_uppercase();
        entries.retain(|e| name_of(e) != key);
        if let Some(value) = value {
            let mut e = name;
            e.push(b'=' as u16);
            e.extend(value);
            entries.push(e);
        }
    }
    entries.sort_by_cached_key(|e| name_of(e));
    let mut out: Vec<u16> = entries.into_iter().flat_map(|e| e.into_iter().chain(Some(0))).collect();
    if out.is_empty() {
        out.push(0);
    }
    out.push(0);
    out
}

/// Start `cmd` as `logon`'s account, suspended, put it in its own job, then let it run.
/// Returns the process and the read ends of its stdout and stderr.
pub fn spawn(cmd: &Command, logon: &Logon) -> io::Result<(Process, File, File)> {
    use std::os::windows::io::AsRawHandle;
    let (user, domain) = account(logon);
    let user = wide(user, "the user name")?;
    let domain = domain.map(|d| wide(d, "the domain")).transpose()?;
    let password = Secret(wide(logon.password, "the password")?);
    let mut line = command_line(cmd)?;
    let env = if cmd.get_envs().next().is_some() { Some(environment(cmd, &user, domain.as_deref(), &password)?) } else { None };
    let dir = cmd.get_current_dir().map(|d| d.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<u16>>());

    let (stdin_parent, stdin_child) = pipe(false)?;
    let (stdout_parent, stdout_child) = pipe(true)?;
    let (stderr_parent, stderr_child) = pipe(true)?;
    // Nothing to say to the child: it reads the end of its input at once.
    drop(stdin_parent);

    let job = crate::sys::make_job();
    if job.is_null() {
        return Err(io::Error::other("could not create a job object for the child"));
    }
    // SAFETY: a job handle just made and not shared; owned from here.
    let job = unsafe { OwnedHandle::from_raw_handle(job) };

    // SAFETY: all-zero is a valid StartupInfo (null pointers, no flags).
    let mut si: StartupInfo = unsafe { std::mem::zeroed() };
    si.cb = size_of::<StartupInfo>() as u32;
    si.flags = STARTF_USESTDHANDLES;
    si.std_input = stdin_child.as_raw_handle();
    si.std_output = stdout_child.as_raw_handle();
    si.std_error = stderr_child.as_raw_handle();
    let mut info = ProcessInformation { process: std::ptr::null_mut(), thread: std::ptr::null_mut(), process_id: 0, thread_id: 0 };
    // SAFETY: every pointer is to a NUL-terminated wide string or a struct that lives through the
    // call; `line` is writable as the call asks; the environment is null (the account's own) or the block built above.
    let ok = unsafe {
        CreateProcessWithLogonW(
            user.as_ptr(),
            domain.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            password.0.as_ptr(),
            LOGON_WITH_PROFILE,
            std::ptr::null(),
            line.as_mut_ptr(),
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            env.as_ref().map_or(std::ptr::null(), |e| e.as_ptr().cast()),
            dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            &si,
            &mut info,
        )
    };
    let failure = (ok == 0).then(io::Error::last_os_error);
    drop(password);
    if let Some(e) = failure {
        return Err(e);
    }
    // SAFETY: both handles were just handed to us by the call and are not shared.
    let (process, thread) = unsafe { (OwnedHandle::from_raw_handle(info.process), OwnedHandle::from_raw_handle(info.thread)) };
    // The child's ends are in the child now; ours must go, or the pipes never end.
    drop((stdin_child, stdout_child, stderr_child));

    // SAFETY: both handles are open.
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
        let e = io::Error::last_os_error();
        // SAFETY: the process handle is open; it never ran.
        unsafe { TerminateProcess(process.as_raw_handle(), 1) };
        return Err(io::Error::new(e.kind(), format!("could not put the child in a job: {e}")));
    }
    // SAFETY: the suspended main thread of the child we made.
    unsafe { ResumeThread(thread.as_raw_handle()) };
    drop(thread);
    Ok((Process { process, job }, File::from(stdout_parent), File::from(stderr_parent)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quoted(program: &str, args: &[&str]) -> String {
        let mut c = Command::new(program);
        c.args(args);
        String::from_utf16(&command_line(&c).unwrap()).unwrap().trim_end_matches('\0').to_owned()
    }

    #[test]
    fn a_command_line_is_quoted_the_way_the_runtime_reads_it_back() {
        assert_eq!(quoted(r"C:\Program Files\a.exe", &["x", "two words", ""]), r#""C:\Program Files\a.exe" x "two words" """#);
        assert_eq!(quoted("a", &[r#"say "hi""#]), r#"a "say \"hi\"""#);
        assert_eq!(quoted("a", &[r"dir\ with\", r"back\\slash"]), r#"a "dir\ with\\" back\\slash"#);
        assert_eq!(quoted("a", &[r#"q\"x"#]), r#"a q\\\"x"#);
    }

    #[test]
    fn changes_are_laid_over_the_accounts_block_ignoring_case() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let block: Vec<u16> =
            ["=C:=C:/x", "Path=a", "TEMP=t"].iter().flat_map(|e| w(e).into_iter().chain(Some(0))).chain(Some(0)).collect();
        let out = overlay(&block, [(w("path"), Some(w("b"))), (w("Temp"), None), (w("New"), Some(w("1")))]);
        let text = String::from_utf16(&out).unwrap();
        assert_eq!(text, "=C:=C:/x New=1 path=b  ");
        assert_eq!(overlay(&[0, 0], [(w("A"), None)]), [0, 0]);
    }

    #[test]
    fn the_account_is_split_from_domain_backslash_user_only() {
        let l = |user, domain| Logon { user, domain, password: "" };
        assert_eq!(account(&l(r"CORP\batch", None)), ("batch", Some("CORP")));
        assert_eq!(account(&l("batch@corp.example", None)), ("batch@corp.example", None));
        assert_eq!(account(&l("batch", Some("PC"))), ("batch", Some("PC")));
        assert_eq!(account(&l("batch", None)), ("batch", None));
    }
}
