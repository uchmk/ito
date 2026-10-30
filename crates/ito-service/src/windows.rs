//! Windows: the Service Control Manager, the LSA policy (for the logon right) and the console.

use crate::{access_sddl, command_line, Error, Install, Start, State, Status, Stop};
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL, NTSTATUS};
use windows::Win32::Security::Authentication::Identity::{
    LsaAddAccountRights, LsaClose, LsaNtStatusToWinError, LsaOpenPolicy, LSA_HANDLE, LSA_OBJECT_ATTRIBUTES, LSA_UNICODE_STRING,
};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW};
use windows::Win32::Security::{LookupAccountNameW, PSECURITY_DESCRIPTOR, PSID, SID_NAME_USE};
use windows::Win32::System::Console::{GetConsoleMode, GetStdHandle, SetConsoleMode, CONSOLE_MODE, STD_INPUT_HANDLE};
use windows::Win32::System::Services::*;

const DELETE: u32 = 0x0001_0000;
const DACL_SECURITY_INFORMATION: u32 = 4;
const POLICY_CREATE_ACCOUNT: u32 = 0x10;
const POLICY_LOOKUP_NAMES: u32 = 0x800;
const ENABLE_ECHO_INPUT: u32 = 4;
const SDDL_REVISION_1: u32 = 1;

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_SERVICE_LOGON_FAILED: u32 = 1069;
const ERROR_SERVICE_DOES_NOT_EXIST: u32 = 1060;
const ERROR_SERVICE_ALREADY_RUNNING: u32 = 1056;
const ERROR_SERVICE_NOT_ACTIVE: u32 = 1062;
const ERROR_SERVICE_EXISTS: u32 = 1073;
const ERROR_DUPLICATE_SERVICE_NAME: u32 = 1078;
const ERROR_INVALID_SERVICE_ACCOUNT: u32 = 1057;
const ERROR_FAILED_SERVICE_CONTROLLER_CONNECT: u32 = 1063;
const ERROR_NONE_MAPPED: u32 = 1332;
const ERROR_SERVICE_SPECIFIC_ERROR: u32 = 1066;
const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;

fn code(e: &windows::core::Error) -> u32 {
    (e.code().0 as u32) & 0xFFFF
}

fn map(e: windows::core::Error) -> Error {
    match code(&e) {
        ERROR_ACCESS_DENIED => Error::NeedsAdmin,
        ERROR_SERVICE_DOES_NOT_EXIST => Error::NotInstalled,
        ERROR_SERVICE_EXISTS | ERROR_DUPLICATE_SERVICE_NAME => Error::AlreadyExists,
        ERROR_INVALID_SERVICE_ACCOUNT | ERROR_NONE_MAPPED => Error::BadAccount,
        ERROR_SERVICE_LOGON_FAILED => Error::LogonFailed,
        ERROR_FAILED_SERVICE_CONTROLLER_CONNECT => Error::NotService,
        n => Error::Failed(n),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// A service or SCM handle that closes itself.
struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenSCManagerW / OpenServiceW / CreateServiceW and is closed once.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn manager(access: u32) -> Result<Handle, Error> {
    // SAFETY: null machine and database names mean the local active database.
    unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), access) }.map(Handle).map_err(map)
}

fn open(name: &str, access: u32) -> Result<(Handle, Handle), Error> {
    let scm = manager(SC_MANAGER_CONNECT)?;
    let n = wide(name);
    // SAFETY: `n` is a nul-terminated wide string that outlives the call.
    let svc = unsafe { OpenServiceW(scm.0, PCWSTR(n.as_ptr()), access) }.map_err(map)?;
    Ok((scm, Handle(svc)))
}

// ---- the service side --------------------------------------------------------------------------

type Main = Box<dyn FnOnce(Stop) -> i32 + Send>;

static MAIN: Mutex<Option<Main>> = Mutex::new(None);
static STOP: Mutex<Option<Stop>> = Mutex::new(None);
static STATUS_HANDLE: AtomicIsize = AtomicIsize::new(0);
static CURRENT: AtomicU32 = AtomicU32::new(0);

fn report(state: SERVICE_STATUS_CURRENT_STATE, accepted: u32, win32: u32, specific: u32, hint: u32) {
    CURRENT.store(state.0, Ordering::SeqCst);
    let h = SERVICE_STATUS_HANDLE(STATUS_HANDLE.load(Ordering::SeqCst) as *mut c_void);
    let pending = state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING;
    let st = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if pending { 0 } else { accepted },
        dwWin32ExitCode: win32,
        dwServiceSpecificExitCode: specific,
        dwCheckPoint: if pending { 1 } else { 0 },
        dwWaitHint: hint,
    };
    // SAFETY: `h` was returned by RegisterServiceCtrlHandlerExW; `st` is a valid SERVICE_STATUS.
    unsafe {
        let _ = SetServiceStatus(h, &st);
    }
}

const ACCEPTED: u32 = SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN | SERVICE_ACCEPT_PRESHUTDOWN;

unsafe extern "system" fn handler(control: u32, _event: u32, _data: *mut c_void, _ctx: *mut c_void) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN | SERVICE_CONTROL_PRESHUTDOWN => {
            report(SERVICE_STOP_PENDING, ACCEPTED, 0, 0, 30_000);
            if let Some(s) = STOP.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                s.set();
            }
            0
        }
        SERVICE_CONTROL_INTERROGATE => {
            report(SERVICE_STATUS_CURRENT_STATE(CURRENT.load(Ordering::SeqCst)), ACCEPTED, 0, 0, 0);
            0
        }
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

unsafe extern "system" fn service_main(_argc: u32, argv: *mut PWSTR) {
    // SAFETY: the SCM passes at least the service name as argv[0], nul-terminated.
    let name = unsafe { *argv };
    // SAFETY: `name` is that name; `handler` has the LPHANDLER_FUNCTION_EX signature.
    let h = match unsafe { RegisterServiceCtrlHandlerExW(PCWSTR(name.0), Some(handler), None) } {
        Ok(h) => h,
        Err(_) => return,
    };
    STATUS_HANDLE.store(h.0 as isize, Ordering::SeqCst);
    report(SERVICE_START_PENDING, 0, 0, 0, 10_000);
    let stop = Stop::new();
    *STOP.lock().unwrap_or_else(|e| e.into_inner()) = Some(stop.clone());
    let main = MAIN.lock().unwrap_or_else(|e| e.into_inner()).take();
    report(SERVICE_RUNNING, ACCEPTED, 0, 0, 0);
    let exit = match main {
        Some(m) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m(stop))).unwrap_or(1),
        None => 0,
    };
    if exit == 0 {
        report(SERVICE_STOPPED, 0, 0, 0, 0);
    } else {
        report(SERVICE_STOPPED, 0, ERROR_SERVICE_SPECIFIC_ERROR, exit as u32, 0);
    }
}

pub fn run(name: &str, main: Main) -> Result<(), Error> {
    *MAIN.lock().unwrap_or_else(|e| e.into_inner()) = Some(main);
    let mut n = wide(name);
    let table = [
        SERVICE_TABLE_ENTRYW { lpServiceName: PWSTR(n.as_mut_ptr()), lpServiceProc: Some(service_main) },
        SERVICE_TABLE_ENTRYW { lpServiceName: PWSTR::null(), lpServiceProc: None },
    ];
    // SAFETY: the table is terminated by a null entry and `n` outlives the call, which blocks until the service ends.
    let r = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) };
    if r.is_err() {
        MAIN.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
    r.map_err(map)
}

// ---- managing ----------------------------------------------------------------------------------

/// The SID of `account` as a self-freeing buffer, and as the `S-1-5-…` string.
fn account_sid(account: &str) -> Result<(Vec<u8>, String), Error> {
    let name = wide(account);
    let (mut sid_len, mut dom_len) = (0u32, 0u32);
    let mut kind = SID_NAME_USE(0);
    // SAFETY: the first call only asks for the buffer sizes (it fails with ERROR_INSUFFICIENT_BUFFER).
    unsafe {
        let _ = LookupAccountNameW(PCWSTR::null(), PCWSTR(name.as_ptr()), None, &mut sid_len, None, &mut dom_len, &mut kind);
    }
    if sid_len == 0 {
        return Err(Error::BadAccount);
    }
    let mut sid = vec![0u8; sid_len as usize];
    let mut dom = vec![0u16; dom_len.max(1) as usize];
    // SAFETY: both buffers have the sizes the first call reported.
    unsafe {
        LookupAccountNameW(
            PCWSTR::null(),
            PCWSTR(name.as_ptr()),
            Some(PSID(sid.as_mut_ptr() as *mut c_void)),
            &mut sid_len,
            Some(PWSTR(dom.as_mut_ptr())),
            &mut dom_len,
            &mut kind,
        )
    }
    .map_err(|_| Error::BadAccount)?;
    let mut text = PWSTR::null();
    // SAFETY: `sid` holds a valid SID; the string is allocated by the system and freed below.
    unsafe { ConvertSidToStringSidW(PSID(sid.as_mut_ptr() as *mut c_void), &mut text) }.map_err(map)?;
    // SAFETY: `text` is a nul-terminated wide string from the call above.
    let s = unsafe { text.to_string() }.unwrap_or_default();
    // SAFETY: allocated with LocalAlloc by ConvertSidToStringSidW.
    unsafe {
        LocalFree(Some(HLOCAL(text.0 as *mut c_void)));
    }
    Ok((sid, s))
}

fn lsa_string(s: &mut Vec<u16>) -> LSA_UNICODE_STRING {
    // `s` has no terminator here; the lengths are in bytes.
    LSA_UNICODE_STRING { Length: (s.len() * 2) as u16, MaximumLength: (s.len() * 2) as u16, Buffer: PWSTR(s.as_mut_ptr()) }
}

/// Grant `SeServiceLogonRight` to the SID (what `services.msc` does when it saves an account).
fn grant_logon_right(sid: &mut [u8]) -> Result<(), Error> {
    let attrs = LSA_OBJECT_ATTRIBUTES { Length: std::mem::size_of::<LSA_OBJECT_ATTRIBUTES>() as u32, ..Default::default() };
    let mut policy = LSA_HANDLE::default();
    // SAFETY: a null system name is the local machine; `attrs` is zeroed apart from its length.
    let st = unsafe { LsaOpenPolicy(None, &attrs, POLICY_CREATE_ACCOUNT | POLICY_LOOKUP_NAMES, &mut policy) };
    lsa_ok(st)?;
    let mut right: Vec<u16> = "SeServiceLogonRight".encode_utf16().collect();
    let rights = [lsa_string(&mut right)];
    // SAFETY: `policy` is open; `sid` is a valid SID; `rights` points at `right`, which is alive.
    let st = unsafe { LsaAddAccountRights(policy, PSID(sid.as_mut_ptr() as *mut c_void), &rights) };
    // SAFETY: `policy` came from LsaOpenPolicy.
    unsafe {
        let _ = LsaClose(policy);
    }
    lsa_ok(st)
}

fn lsa_ok(st: NTSTATUS) -> Result<(), Error> {
    if st.0 >= 0 {
        return Ok(());
    }
    // SAFETY: a pure status conversion.
    match unsafe { LsaNtStatusToWinError(st) } {
        ERROR_ACCESS_DENIED => Err(Error::NeedsAdmin),
        n => Err(Error::Failed(n)),
    }
}

pub fn install(i: &Install) -> Result<(), Error> {
    let (mut sid, sid_text) = account_sid(&i.account)?;
    let scm = manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    grant_logon_right(&mut sid)?;

    let name = wide(&i.name);
    let display = wide(if i.display.is_empty() { &i.name } else { &i.display });
    let path = wide(&command_line(&i.exe, &i.args));
    let account = wide(&i.account);
    let mut password = wide(&i.password);
    let start = match i.start {
        Start::Auto | Start::AutoDelayed => SERVICE_AUTO_START,
        Start::Manual => SERVICE_DEMAND_START,
    };
    // SAFETY: every string is a nul-terminated wide string that outlives the call.
    let svc = unsafe {
        CreateServiceW(
            scm.0,
            PCWSTR(name.as_ptr()),
            PCWSTR(display.as_ptr()),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            start,
            SERVICE_ERROR_NORMAL,
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR(account.as_ptr()),
            PCWSTR(password.as_ptr()),
        )
    };
    password.fill(0);
    let svc = Handle(svc.map_err(map)?);

    let rest = (|| -> Result<(), Error> {
        let mut desc = wide(&i.description);
        let d = SERVICE_DESCRIPTIONW { lpDescription: PWSTR(desc.as_mut_ptr()) };
        // SAFETY: `d` points at `desc`, which is alive; the info level matches the struct.
        unsafe { ChangeServiceConfig2W(svc.0, SERVICE_CONFIG_DESCRIPTION, Some(&d as *const _ as *const c_void)) }.map_err(map)?;
        if i.start == Start::AutoDelayed {
            let info = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: true.into() };
            // SAFETY: as above, for the delayed-start info level.
            unsafe { ChangeServiceConfig2W(svc.0, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, Some(&info as *const _ as *const c_void)) }
                .map_err(map)?;
        }
        let sddl = wide(&access_sddl(&sid_text));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is a nul-terminated wide string; the descriptor is allocated by the system and freed below.
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut sd, None) }
            .map_err(map)?;
        // SAFETY: `sd` is a valid self-relative descriptor holding a DACL.
        let r = unsafe {
            SetServiceObjectSecurity(svc.0, windows::Win32::Security::OBJECT_SECURITY_INFORMATION(DACL_SECURITY_INFORMATION), sd)
        };
        // SAFETY: allocated with LocalAlloc by the conversion above.
        unsafe {
            LocalFree(Some(HLOCAL(sd.0)));
        }
        r.map_err(map)
    })();
    if rest.is_err() {
        // SAFETY: the handle has DELETE (SERVICE_ALL_ACCESS); a half-made service is worse than none.
        unsafe {
            let _ = DeleteService(svc.0);
        }
    }
    rest
}

pub fn uninstall(name: &str) -> Result<(), Error> {
    let (_scm, svc) = open(name, DELETE)?;
    // SAFETY: the handle has DELETE access.
    unsafe { DeleteService(svc.0) }.map_err(map)
}

pub fn start(name: &str) -> Result<(), Error> {
    let (_scm, svc) = open(name, SERVICE_START)?;
    // SAFETY: no arguments.
    match unsafe { StartServiceW(svc.0, None) } {
        Err(e) if code(&e) == ERROR_SERVICE_ALREADY_RUNNING => Ok(()),
        r => r.map_err(map),
    }
}

fn query(svc: &Handle) -> Result<Status, Error> {
    let mut buf = [0u8; std::mem::size_of::<SERVICE_STATUS_PROCESS>()];
    let mut needed = 0u32;
    // SAFETY: `buf` is exactly one SERVICE_STATUS_PROCESS.
    unsafe { QueryServiceStatusEx(svc.0, SC_STATUS_PROCESS_INFO, Some(&mut buf), &mut needed) }.map_err(map)?;
    // SAFETY: the call filled `buf` with a SERVICE_STATUS_PROCESS; read unaligned since a byte array has no alignment.
    let p: SERVICE_STATUS_PROCESS = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const SERVICE_STATUS_PROCESS) };
    Ok(Status { state: state_of(p.dwCurrentState.0), pid: p.dwProcessId })
}

/// `SERVICE_STOPPED` 1, `START_PENDING` 2, `STOP_PENDING` 3, `RUNNING` 4; the pause states count as running.
fn state_of(n: u32) -> State {
    match n {
        1 => State::Stopped,
        2 => State::Starting,
        3 => State::Stopping,
        _ => State::Running,
    }
}

pub fn stop(name: &str, wait: Duration) -> Result<(), Error> {
    let (_scm, svc) = open(name, SERVICE_STOP | SERVICE_QUERY_STATUS)?;
    let mut st = SERVICE_STATUS::default();
    // SAFETY: `st` receives the status.
    match unsafe { ControlService(svc.0, SERVICE_CONTROL_STOP, &mut st) } {
        Err(e) if code(&e) == ERROR_SERVICE_NOT_ACTIVE => return Ok(()),
        r => r.map_err(map)?,
    }
    let until = Instant::now() + wait;
    loop {
        if query(&svc)?.state == State::Stopped {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(Error::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn state(name: &str) -> Result<Option<Status>, Error> {
    match open(name, SERVICE_QUERY_STATUS) {
        Ok((_scm, svc)) => query(&svc).map(Some),
        Err(Error::NotInstalled) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn read_password(prompt: &str) -> Result<String, Error> {
    eprint!("{prompt}");
    // SAFETY: the standard input handle is valid for the life of the process (or an error).
    let h: HANDLE = unsafe { GetStdHandle(STD_INPUT_HANDLE) }.map_err(map)?;
    let mut mode = CONSOLE_MODE(0);
    // SAFETY: `mode` receives the mode; this fails when stdin is a pipe, which is read as is.
    let console = unsafe { GetConsoleMode(h, &mut mode) }.is_ok();
    if console {
        // SAFETY: the handle is a console input handle.
        unsafe { SetConsoleMode(h, CONSOLE_MODE(mode.0 & !ENABLE_ECHO_INPUT)) }.map_err(map)?;
    }
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    if console {
        // SAFETY: restore the mode read above.
        unsafe {
            let _ = SetConsoleMode(h, mode);
        }
        eprintln!();
    }
    read.map_err(|e| Error::Failed(e.raw_os_error().unwrap_or(0) as u32))?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states() {
        assert_eq!(state_of(1), State::Stopped);
        assert_eq!(state_of(2), State::Starting);
        assert_eq!(state_of(3), State::Stopping);
        assert_eq!(state_of(4), State::Running);
        assert_eq!(state_of(7), State::Running);
    }

    #[test]
    fn no_such_service() {
        assert_eq!(state("ito-service-test-no-such-service").unwrap(), None);
        assert_eq!(start("ito-service-test-no-such-service"), Err(Error::NotInstalled));
    }

    #[test]
    fn a_console_is_not_the_scm() {
        assert_eq!(crate::run("ito-service-test", |_| 0), Err(Error::NotService));
    }

    #[test]
    fn looks_up_the_current_account() {
        let user = std::env::var("USERNAME").unwrap();
        let (sid, text) = account_sid(&user).unwrap();
        assert!(!sid.is_empty());
        assert!(text.starts_with("S-1-5-"));
        assert_eq!(account_sid("no such account zzz").unwrap_err(), Error::BadAccount);
    }
}
