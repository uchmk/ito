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

/// A process as the table of them says it: its parent, the name of its
/// program (no `.exe`), and its arguments where the system tells them
/// (Linux, macOS; empty on Windows).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub args: Vec<String>,
}

/// Every process under `root` in `table` (children, theirs, …), `root` not
/// among them.
pub fn descendants(table: &[Proc], root: u32) -> Vec<&Proc> {
    let mut out: Vec<&Proc> = Vec::new();
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        let found: Vec<&Proc> = table.iter().filter(|p| p.ppid == parent && p.pid != root && !out.iter().any(|o| o.pid == p.pid)).collect();
        for p in found {
            out.push(p);
            parents.push(p.pid);
        }
    }
    out
}

/// A program's name as compared: the file's stem, lower case.
pub fn stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let file = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
    file.to_lowercase()
}

#[cfg(not(any(windows, unix)))]
pub fn process_table() -> Vec<Proc> {
    Vec::new()
}

#[cfg(not(any(windows, unix)))]
pub fn listening_ports(_pids: &[u32]) -> Vec<u16> {
    Vec::new()
}

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

#[cfg(not(any(windows, unix)))]
pub fn process_cwd(_pid: u32) -> Option<std::path::PathBuf> {
    None
}

/// Neither: no process table to read.
#[cfg(not(any(windows, unix)))]
pub fn children(_pid: u32) -> Vec<u32> {
    Vec::new()
}

/// Nothing to restrict off Windows: there `conpty.dll` is not a thing.
#[cfg(not(windows))]
pub fn restrict_dll_search() {}
