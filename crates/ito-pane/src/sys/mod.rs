//! Everything that differs by OS, behind one set of names. The rest of the
//! crate calls `sys::child_pids` and the like and never says `cfg` itself.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

/// A process as the table of them says it: its parent, the name of its
/// program (no `.exe`), its arguments where the system tells them (Linux,
/// macOS; empty on Windows), and when it started where that is needed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub args: Vec<String>,
    /// When it started, in the system's own ticks (Windows: 100 ns since
    /// 1601). 0 where unknown, as off Windows, where a process whose parent
    /// ends is given another and `ppid` never names one that has gone.
    pub started: u64,
}

/// Every process under `root` in `table` (children, theirs, …), `root` not
/// among them.
///
/// On Windows a process keeps its parent's id after the parent ends, and the
/// id goes to another process. A shell given the id of explorer.exe's
/// long-gone parent had explorer.exe and all it started "under" it, and a
/// test's shell had the test itself, which closing the pane then ended
/// (tsumugi #175). So a process that started before the one it names as its
/// parent is not that one's child, where both times are known.
pub fn descendants(table: &[Proc], root: u32) -> Vec<&Proc> {
    let since = table.iter().find(|p| p.pid == root).map_or(0, |p| p.started);
    descendants_since(table, root, since)
}

/// [`descendants`] of a `root` that started at `since` (0: unknown), which
/// need not be in `table`: a shell that has just ended is not.
pub(crate) fn descendants_since(table: &[Proc], root: u32, since: u64) -> Vec<&Proc> {
    let mut out: Vec<&Proc> = Vec::new();
    let mut parents = vec![(root, since)];
    while let Some((parent, since)) = parents.pop() {
        let found: Vec<&Proc> = table
            .iter()
            .filter(|p| p.ppid == parent && p.pid != root && !older(p.started, since) && !out.iter().any(|o| o.pid == p.pid))
            .collect();
        for p in found {
            out.push(p);
            parents.push((p.pid, p.started));
        }
    }
    out
}

/// Whether a process that started at `child` is older than one that started
/// at `parent`, and so not its child; never when either is unknown (0).
pub(crate) fn older(child: u64, parent: u64) -> bool {
    child > 0 && parent > 0 && child < parent
}

/// A program's name as compared: the file's stem, lower case.
pub fn program_stem(path: &str) -> String {
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
pub fn child_pids(_pid: u32) -> Vec<u32> {
    Vec::new()
}

/// Nothing to restrict off Windows: there `conpty.dll` is not a thing.
#[cfg(not(windows))]
pub fn restrict_dll_search() {}

/// Off Windows a shell starts with Ctrl+C as the terminal sets it.
#[cfg(not(windows))]
pub fn let_ctrl_c_through() {}
