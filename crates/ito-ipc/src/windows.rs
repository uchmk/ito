//! A named pipe, opened for overlapped I/O.
//!
//! Overlapped because the reader and the writer run on two threads: on a
//! handle opened the ordinary way, Windows serialises the two, so a read
//! waiting for the server would hold up every write behind it. Each read or
//! write here starts an overlapped operation and waits for it, which reads
//! like blocking I/O and lets the other direction go on meanwhile.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, WaitNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::CreateEventW;
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

use super::Address;

/// `\\.\pipe\<app>-<user>`.
pub fn default_address(app: &str) -> PathBuf {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
    PathBuf::from(format!(r"\\.\pipe\{app}-{user}"))
}

/// A handle closed when dropped.
struct Owned(HANDLE);

// A pipe or event handle may be used and closed from any thread.
unsafe impl Send for Owned {}
unsafe impl Sync for Owned {}

impl Drop for Owned {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn os_error(e: windows::core::Error) -> io::Error {
    // A Win32 error comes wrapped as HRESULT 0x8007xxxx; the low word is it.
    io::Error::from_raw_os_error(e.code().0 & 0xffff)
}

fn is(e: &windows::core::Error, code: windows::Win32::Foundation::WIN32_ERROR) -> bool {
    e.code() == code.to_hresult()
}

/// Run one overlapped operation on `h` to the end.
fn overlapped(h: HANDLE, op: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>) -> io::Result<u32> {
    let event = Owned(unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(os_error)?);
    let mut ov = OVERLAPPED { hEvent: event.0, ..Default::default() };
    match op(&mut ov) {
        Ok(()) => {}
        Err(e) if is(&e, ERROR_IO_PENDING) => {}
        Err(e) => return Err(os_error(e)),
    }
    let mut n = 0u32;
    unsafe { GetOverlappedResult(h, &ov, &mut n, true) }.map_err(os_error)?;
    Ok(n)
}

/// Owner and SYSTEM only. The default for a named pipe also lets every user
/// on the machine read it, and a session's screen is not theirs to read.
const OWNER_ONLY: &str = "D:P(A;;GA;;;SY)(A;;GA;;;OW)";

fn create_instance(name: &HSTRING, first: bool) -> io::Result<Owned> {
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&HSTRING::from(OWNER_ONLY), SDDL_REVISION_1, &mut sd, None) }
        .map_err(os_error)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    let mut open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        // Refused when another server already owns the name.
        open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    let h = unsafe { CreateNamedPipeW(name, open, mode, PIPE_UNLIMITED_INSTANCES, 64 << 10, 64 << 10, 0, Some(&sa)) };
    let err = io::Error::last_os_error();
    unsafe { LocalFree(Some(HLOCAL(sd.0))) };
    if h == INVALID_HANDLE_VALUE {
        return Err(match (first, err.raw_os_error()) {
            (true, Some(5)) => io::Error::new(io::ErrorKind::AddrInUse, "another program is already listening at this pipe"),
            _ => err,
        });
    }
    Ok(Owned(h))
}

pub struct Listener {
    name: HSTRING,
    /// The instance made at `bind`, which the first `accept` waits on.
    first: Mutex<Option<Owned>>,
}

impl Listener {
    pub fn bind(at: &Address) -> io::Result<Self> {
        let name = HSTRING::from(at.0.as_os_str());
        let first = create_instance(&name, true)?;
        Ok(Self { name, first: Mutex::new(Some(first)) })
    }

    pub fn accept(&self) -> io::Result<Conn> {
        let pipe = match self.first.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(p) => p,
            None => create_instance(&self.name, false)?,
        };
        let h = pipe.0;
        match overlapped(h, |ov| unsafe { ConnectNamedPipe(h, Some(ov)) }) {
            Ok(_) => {}
            // The client got there between the instance and the wait.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_CONNECTED.0 as i32) => {}
            Err(e) => return Err(e),
        }
        Ok(Conn(Arc::new(pipe)))
    }
}

pub fn connect(at: &Address) -> io::Result<Conn> {
    let name = HSTRING::from(at.0.as_os_str());
    for _ in 0..10 {
        let opened = unsafe {
            CreateFileW(
                &name,
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
        };
        match opened {
            Ok(h) => return Ok(Conn(Arc::new(Owned(h)))),
            // Every instance is taken this moment; the server makes another.
            Err(e) if is(&e, ERROR_PIPE_BUSY) => {
                let _ = unsafe { WaitNamedPipeW(&name, 2000) };
            }
            Err(e) => return Err(os_error(e)),
        }
    }
    Err(io::Error::new(io::ErrorKind::TimedOut, "the server at this pipe stayed busy"))
}

pub struct Conn(Arc<Owned>);

struct Half(Arc<Owned>);

impl Read for Half {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let h = self.0 .0;
        match overlapped(h, |ov| unsafe { ReadFile(h, Some(buf), None, Some(ov)) }) {
            Ok(n) => Ok(n as usize),
            // The other end closed: the end of the stream.
            Err(e) if e.raw_os_error() == Some(ERROR_BROKEN_PIPE.0 as i32) => Ok(0),
            Err(e) => Err(e),
        }
    }
}

impl Write for Half {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let h = self.0 .0;
        match overlapped(h, |ov| unsafe { WriteFile(h, Some(buf), None, Some(ov)) }) {
            Ok(n) => Ok(n as usize),
            Err(e) if matches!(e.raw_os_error(), Some(c) if c == ERROR_NO_DATA.0 as i32 || c == ERROR_BROKEN_PIPE.0 as i32) => {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, e))
            }
            Err(e) => Err(e),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Conn {
    pub fn split(self) -> io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>)> {
        Ok((Box::new(Half(self.0.clone())), Box::new(Half(self.0))))
    }
}
