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

/// Every process, from one snapshot; Windows does not give the arguments
/// without reading the process's memory, so they are left empty.
pub fn process_table() -> Vec<super::Proc> {
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
        let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
        let name = super::stem(&String::from_utf16_lossy(&e.szExeFile[..len]));
        out.push(super::Proc { pid: e.th32ProcessID, ppid: e.th32ParentProcessID, name, args: Vec::new() });
        ok = unsafe { Process32NextW(snap, &mut e) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snap) };
    out
}

/// The TCP ports (IPv4 and IPv6) being listened on by any of `pids`, from
/// the system's table of listeners with their owners. Sorted, each once.
pub fn listening_ports(pids: &[u32]) -> Vec<u16> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    if pids.is_empty() {
        return Vec::new();
    }
    // AF_INET and AF_INET6.
    let mut ports = Vec::new();
    for (family, row_size) in [(2u32, std::mem::size_of::<MIB_TCPROW_OWNER_PID>()), (23u32, std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>())] {
        let mut size = 0u32;
        unsafe { GetExtendedTcpTable(None, &mut size, false, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
        if size == 0 {
            continue;
        }
        // u32s, for the table's alignment.
        let mut buf = vec![0u32; (size as usize).div_ceil(4) + 1];
        if unsafe { GetExtendedTcpTable(Some(buf.as_mut_ptr().cast()), &mut size, false, family, TCP_TABLE_OWNER_PID_LISTENER, 0) } != 0 {
            continue;
        }
        let count = buf[0] as usize;
        let rows = unsafe { buf.as_ptr().add(1).cast::<u8>() };
        for k in 0..count {
            let at = unsafe { rows.add(k * row_size) };
            let (port, pid) = if family == 2 {
                let r = unsafe { &*at.cast::<MIB_TCPROW_OWNER_PID>() };
                (r.dwLocalPort, r.dwOwningPid)
            } else {
                let r = unsafe { &*at.cast::<MIB_TCP6ROW_OWNER_PID>() };
                (r.dwLocalPort, r.dwOwningPid)
            };
            if pids.contains(&pid) {
                // The port is in network order in the low 16 bits.
                ports.push(u16::from_be(port as u16));
            }
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
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
