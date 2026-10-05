//! Everything that differs by OS, behind one set of names. The rest of the
//! crate calls `sys::children` and the like and never says `cfg` itself.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

/// Whether a process is a pseudoconsole's host: `OpenConsole.exe` for the
/// ConPTY shipped beside the exe, `conhost.exe` for the one built into Windows.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_console_host(name: &str) -> bool {
    name.eq_ignore_ascii_case("OpenConsole.exe") || name.eq_ignore_ascii_case("conhost.exe")
}

/// The console hosts among `now` that were not in `before`.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn strays(before: &[u32], now: &[(u32, String)]) -> Vec<u32> {
    now.iter().filter(|(pid, name)| is_console_host(name) && !before.contains(pid)).map(|(pid, _)| *pid).collect()
}

/// No pseudoconsole host to leave behind off Windows.
#[cfg(not(windows))]
pub fn consoles() -> Vec<u32> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn end_new_consoles(_before: &[u32]) {}

/// Neither: no process table to read.
#[cfg(not(any(windows, unix)))]
pub fn children(_pid: u32) -> Vec<u32> {
    Vec::new()
}

/// Nothing to restrict off Windows: there `conpty.dll` is not a thing.
#[cfg(not(windows))]
pub fn restrict_dll_search() {}
