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
