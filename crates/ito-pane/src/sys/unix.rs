//! Unix: the shell's children, from `/proc` on Linux and `pgrep` on macOS.

/// The processes whose parent is `pid`. Read when a key asks, not every frame:
/// one snapshot of the process table on Windows, `/proc` on Linux, `pgrep` on
/// macOS. Empty when the platform will not say.
pub fn children(pid: u32) -> Vec<u32> {
    #[cfg(target_os = "linux")]
    {
        let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
        dir.flatten()
            .filter_map(|d| d.file_name().to_str()?.parse::<u32>().ok())
            .filter(|&child| {
                // `pid (comm) state ppid ...`; the name may hold spaces and
                // parentheses, so the fields are counted from the last `)`.
                std::fs::read_to_string(format!("/proc/{child}/stat")).ok().is_some_and(|stat| {
                    stat.rsplit_once(')')
                        .and_then(|(_, rest)| rest.split_whitespace().nth(1)?.parse::<u32>().ok())
                        == Some(pid)
                })
            })
            .collect()
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("pgrep")
            .args(["-P", &pid.to_string()])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect())
            .unwrap_or_default()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        Vec::new()
    }
}

/// Every process: `/proc` on Linux, one `ps` on macOS.
pub fn process_table() -> Vec<super::Proc> {
    #[cfg(target_os = "linux")]
    {
        let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
        dir.flatten()
            .filter_map(|d| d.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                let (head, rest) = stat.rsplit_once(')')?;
                let name = head.split_once('(').map_or("", |(_, n)| n).to_owned();
                let ppid = rest.split_whitespace().nth(1)?.parse().ok()?;
                let args = std::fs::read(format!("/proc/{pid}/cmdline")).map(|b| b.split(|c| *c == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect()).unwrap_or_default();
                Some(super::Proc { pid, ppid, name, args })
            })
            .collect()
    }
    #[cfg(target_os = "macos")]
    {
        let Ok(out) = std::process::Command::new("ps").args(["-A", "-o", "pid=,ppid=,args="]).output() else { return Vec::new() };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                let pid = it.next()?.parse().ok()?;
                let ppid = it.next()?.parse().ok()?;
                let args: Vec<String> = it.map(str::to_owned).collect();
                let name = args.first().map(|a| super::stem(a)).unwrap_or_default();
                Some(super::Proc { pid, ppid, name, args })
            })
            .collect()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Vec::new()
    }
}

/// The TCP ports being listened on by any of `pids`: `/proc/net` on Linux,
/// `lsof` on macOS. Sorted, each once.
pub fn listening_ports(pids: &[u32]) -> Vec<u16> {
    if pids.is_empty() {
        return Vec::new();
    }
    #[cfg(target_os = "linux")]
    {
        // The sockets' inodes each process holds, then which of them listen.
        let mut inodes = std::collections::HashSet::new();
        for pid in pids {
            let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else { continue };
            for fd in fds.flatten() {
                if let Some(n) = std::fs::read_link(fd.path()).ok().and_then(|l| l.to_str()?.strip_prefix("socket:[")?.strip_suffix(']')?.parse::<u64>().ok()) {
                    inodes.insert(n);
                }
            }
        }
        let mut ports: Vec<u16> = ["/proc/net/tcp", "/proc/net/tcp6"].iter().filter_map(|f| std::fs::read_to_string(f).ok()).flat_map(|t| listening_in(&t, &inodes)).collect();
        ports.sort_unstable();
        ports.dedup();
        ports
    }
    #[cfg(target_os = "macos")]
    {
        let list: Vec<String> = pids.iter().map(u32::to_string).collect();
        let Ok(out) = std::process::Command::new("lsof").args(["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-Fn", "-p", &list.join(",")]).output() else { return Vec::new() };
        let mut ports: Vec<u16> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_prefix('n')?.rsplit(':').next()?.parse().ok()).collect();
        ports.sort_unstable();
        ports.dedup();
        ports
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Vec::new()
    }
}

/// The ports of `/proc/net/tcp`'s lines that listen (state `0A`) on a
/// socket among `inodes`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn listening_in(table: &str, inodes: &std::collections::HashSet<u64>) -> Vec<u16> {
    table
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (local, state, inode) = (f.get(1)?, f.get(3)?, f.get(9)?.parse::<u64>().ok()?);
            (*state == "0A" && inodes.contains(&inode)).then(|| u16::from_str_radix(local.rsplit(':').next()?, 16).ok()).flatten()
        })
        .collect()
}

/// Hang up the shell's children, as a terminal closing would; see the
/// `Drop` for `Terminal`. Only macOS needs it: there the PTY's child is
/// `login`, which waits for the shell before the PTY can close.
#[cfg(target_os = "macos")]
pub fn hang_up_children(pid: u32) {
    for child in children(pid) {
        let _ = std::process::Command::new("/bin/kill").args(["-HUP", &child.to_string()]).status();
    }
}

/// End the shell: hang it up, as a terminal closing does, and if it is still
/// there half a second later, kill it. alacritty's PTY hangs up the shell
/// as it is dropped and then *waits* for it; a bash that took the hangup and
/// went on reading left that wait, and the tsumugi server's lock with it,
/// hanging for good (seen under Xvfb, 2026-10-06).
pub fn end_shell(pid: u32) {
    let pid_s = pid.to_string();
    let _ = std::process::Command::new("kill").args(["-HUP", &pid_s]).status();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if alive(pid) {
        let _ = std::process::Command::new("kill").args(["-KILL", &pid_s]).status();
    }
}

/// Whether `pid` is running: not gone, and not a zombie waiting to be reaped
/// (which is what a shell that has exited is until the PTY waits for it).
fn alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| stat.rsplit_once(')').and_then(|(_, rest)| rest.trim_start().chars().next()))
            .is_some_and(|state| state != 'Z' && state != 'X')
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .is_ok_and(|s| !s.is_empty() && !s.starts_with('Z'))
    }
}

/// The folder process `pid` is in: `/proc` on Linux, `lsof` on macOS.
pub fn process_cwd(pid: u32) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let out = std::process::Command::new("lsof").args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).lines().find_map(|l| l.strip_prefix('n')).map(std::path::PathBuf::from)
    }
}
