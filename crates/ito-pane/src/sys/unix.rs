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
