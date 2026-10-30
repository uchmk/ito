//! Windows: run a program elevated through the UAC prompt (`ShellExecuteExW` with the `runas` verb)
//! and wait for it, and tell whether this process is elevated already (`TokenElevation`).

use std::ffi::{c_void, OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

type Handle = *mut c_void;

#[repr(C)]
struct ShellExecuteInfo {
    size: u32,
    mask: u32,
    hwnd: Handle,
    verb: *const u16,
    file: *const u16,
    parameters: *const u16,
    directory: *const u16,
    show: i32,
    instance: Handle,
    id_list: *mut c_void,
    class: *const u16,
    key_class: Handle,
    hot_key: u32,
    icon: Handle,
    process: Handle,
}

#[link(name = "kernel32")]
extern "system" {
    fn CloseHandle(h: Handle) -> i32;
    fn WaitForSingleObject(h: Handle, millis: u32) -> u32;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
    fn GetCurrentProcess() -> Handle;
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteExW(info: *mut ShellExecuteInfo) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn GetTokenInformation(token: Handle, class: i32, info: *mut c_void, len: u32, returned: *mut u32) -> i32;
}

const SEE_MASK_NOCLOSEPROCESS: u32 = 0x40;
const SEE_MASK_NOASYNC: u32 = 0x100;
const SW_SHOWNORMAL: i32 = 1;
const INFINITE: u32 = u32::MAX;
const ERROR_CANCELLED: i32 = 1223;
const TOKEN_QUERY: u32 = 0x8;
const TOKEN_ELEVATION: i32 = 20;

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

/// One argument as `CommandLineToArgvW` reads it back.
pub(crate) fn quote(arg: &OsStr) -> OsString {
    let text: Vec<u16> = arg.encode_wide().collect();
    let plain = !text.is_empty() && !text.iter().any(|&c| c == b' ' as u16 || c == b'\t' as u16 || c == b'"' as u16);
    if plain {
        return arg.to_os_string();
    }
    let mut out: Vec<u16> = vec![b'"' as u16];
    let mut slashes = 0;
    for &c in &text {
        if c == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if c == b'"' as u16 {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        }
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    std::os::windows::ffi::OsStringExt::from_wide(&out)
}

pub(crate) fn run_elevated(exe: &Path, args: &[OsString]) -> io::Result<i32> {
    let mut line = OsString::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            line.push(" ");
        }
        line.push(quote(a));
    }
    let verb = wide(OsStr::new("runas"));
    let file = wide(exe.as_os_str());
    let params = wide(&line);
    let mut info = ShellExecuteInfo {
        size: std::mem::size_of::<ShellExecuteInfo>() as u32,
        mask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        hwnd: std::ptr::null_mut(),
        verb: verb.as_ptr(),
        file: file.as_ptr(),
        parameters: params.as_ptr(),
        directory: std::ptr::null(),
        show: SW_SHOWNORMAL,
        instance: std::ptr::null_mut(),
        id_list: std::ptr::null_mut(),
        class: std::ptr::null(),
        key_class: std::ptr::null_mut(),
        hot_key: 0,
        icon: std::ptr::null_mut(),
        process: std::ptr::null_mut(),
    };
    // SAFETY: `info` and the wide strings it points to live through the call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(ERROR_CANCELLED) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the elevation prompt was declined"));
        }
        return Err(err);
    }
    if info.process.is_null() {
        return Err(io::Error::other("the elevated program did not give a process to wait for"));
    }
    // SAFETY: a process handle ShellExecuteExW returned (NOCLOSEPROCESS); closed once below.
    let code = unsafe {
        WaitForSingleObject(info.process, INFINITE);
        let mut code = 0u32;
        let got = GetExitCodeProcess(info.process, &mut code);
        let err = io::Error::last_os_error();
        CloseHandle(info.process);
        if got == 0 {
            return Err(err);
        }
        code
    };
    Ok(code as i32)
}

pub(crate) fn is_elevated() -> bool {
    let mut token: Handle = std::ptr::null_mut();
    // SAFETY: the pseudo handle of this process; the token is closed before returning.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = 0u32;
        let mut returned = 0u32;
        let ok = GetTokenInformation(token, TOKEN_ELEVATION, (&mut elevation as *mut u32).cast(), 4, &mut returned);
        CloseHandle(token);
        ok != 0 && elevation != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_round_trips_the_awkward_cases() {
        let q = |s: &str| quote(OsStr::new(s)).to_string_lossy().into_owned();
        assert_eq!(q("plain"), "plain");
        assert_eq!(q(""), "\"\"");
        assert_eq!(q("a b"), "\"a b\"");
        assert_eq!(q("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(q("C:\\Program Files\\"), "\"C:\\Program Files\\\\\"");
        assert_eq!(q("C:\\dir\\file"), "C:\\dir\\file");
    }

    #[test]
    fn this_process_can_be_asked_whether_it_is_elevated() {
        let _ = is_elevated();
    }
}
