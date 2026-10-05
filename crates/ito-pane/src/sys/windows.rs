//! Windows: ConPTY's processes, and the DLL search order.

/// End `pid` and everything under it, children first so none is left to be
/// re-parented, and wait briefly for each to be gone.
pub fn end_tree(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };
    for child in children(pid) {
        end_tree(child);
    }
    let Ok(h) = (unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) }) else { return };
    unsafe {
        if TerminateProcess(h, 1).is_ok() {
            WaitForSingleObject(h, 1000);
        }
        let _ = CloseHandle(h);
    }
}

/// The processes whose parent is `pid`. Read when a key asks, not every frame:
/// one snapshot of the process table on Windows, `/proc` on Linux, `pgrep` on
/// macOS. Empty when the platform will not say.
pub fn children(pid: u32) -> Vec<u32> {
    named_children(pid).into_iter().map(|(child, _)| child).collect()
}

/// [`children`] with each one's executable name (`OpenConsole.exe`).
pub fn named_children(pid: u32) -> Vec<(u32, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut ok = unsafe { Process32FirstW(snap, &mut e) }.is_ok();
    while ok {
        if e.th32ParentProcessID == pid && e.th32ProcessID != pid {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..len])));
        }
        ok = unsafe { Process32NextW(snap, &mut e) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snap) };
    out
}

/// This process's console hosts as they are now: what [`end_new_consoles`]
/// compares against after a pane failed to start.
pub fn consoles() -> Vec<u32> {
    named_children(std::process::id()).into_iter().filter(|(_, name)| super::is_console_host(name)).map(|(pid, _)| pid).collect()
}

/// End the console hosts this process has gained since `before`: the
/// pseudoconsole of a pane whose shell failed to start. `alacritty_terminal`
/// creates the pseudoconsole, then the shell; when the shell fails it returns
/// the error without closing the pseudoconsole, and its `OpenConsole.exe`
/// (`conhost.exe` for the ConPTY built into Windows) lived until filer quit,
/// one per attempt (filer's TODO, finding 3 of the 2026-10 x64 run).
pub fn end_new_consoles(before: &[u32]) {
    for pid in super::strays(before, &named_children(std::process::id())) {
        end_tree(pid);
    }
}

/// Another process's folder is not to be had on Windows without reading its
/// memory; a shell that says where it is (OSC 7) is the way there.
pub fn process_cwd(_pid: u32) -> Option<std::path::PathBuf> {
    None
}

/// Where a DLL loaded by name may come from: the folder the exe is in and
/// System32, and nowhere else. `alacritty_terminal` loads `conpty.dll` by name,
/// and the default search also tries the working directory and every folder on
/// the `PATH`. A `filer.exe` with no `conpty.dll` beside it ran on WezTerm's
/// from the `PATH`, or on one left in the folder it was started from (filer
/// #184). Without one beside the exe, the pane uses the ConPTY built into
/// Windows. Call it first thing in `main`, before anything loads a DLL.
pub fn restrict_dll_search() {
    use windows::Win32::System::LibraryLoader::{SetDefaultDllDirectories, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS};
    // Fails only before Windows 8 (or 7 without KB2533623), which egui does
    // not run on either; there the old search order simply stays.
    let _ = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) };
}
