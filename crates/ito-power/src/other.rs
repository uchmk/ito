//! macOS and Linux: a hold is a child process (`caffeinate`, `systemd-inhibit`) killed on drop.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::WhyNot;

#[derive(Debug)]
pub struct Hold(Option<Child>);

impl Hold {
    pub fn active(&self) -> bool {
        self.0.is_some()
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        if let Some(c) = &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[derive(Debug)]
pub struct Timer {
    pub when: SystemTime,
}

pub fn hold(reason: &str) -> Hold {
    let mut cmd;
    if cfg!(target_os = "macos") {
        // -i: no idle sleep. Runs until it is killed.
        cmd = Command::new("caffeinate");
        cmd.arg("-i");
    } else {
        cmd = Command::new("systemd-inhibit");
        cmd.args(["--what=sleep:idle", "--who=ito-power", "--mode=block"]).arg(format!("--why={reason}")).args(["sleep", "infinity"]);
    }
    Hold(cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok())
}

pub fn wake_at(_: SystemTime) -> Result<Timer, WhyNot> {
    Err(WhyNot::Unsupported)
}

pub fn sleep_after() -> Option<Duration> {
    None
}

pub fn on_ac() -> Option<bool> {
    if cfg!(target_os = "macos") {
        parse_pmset(&run("pmset", &["-g", "batt"])?)
    } else {
        parse_supplies(&supplies())
    }
}

pub fn network_up() -> Option<bool> {
    if cfg!(target_os = "macos") {
        // A default route means a way out.
        return Command::new("route")
            .args(["-n", "get", "default"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .map(|s| s.success());
    }
    let mut states = Vec::new();
    for e in std::fs::read_dir("/sys/class/net").ok()?.flatten() {
        if e.file_name() != "lo" {
            states.push(std::fs::read_to_string(e.path().join("operstate")).unwrap_or_default().trim().to_owned());
        }
    }
    Some(states.iter().any(|s| s == "up"))
}

pub fn idle_for() -> Option<Duration> {
    if cfg!(target_os = "macos") {
        parse_hid_idle(&run("ioreg", &["-c", "IOHIDSystem"])?)
    } else {
        // X11 only, and only with the tool installed.
        let ms: u64 = run("xprintidle", &[])?.trim().parse().ok()?;
        Some(Duration::from_millis(ms))
    }
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `pmset -g batt`: "Now drawing from 'AC Power'" or "'Battery Power'".
fn parse_pmset(out: &str) -> Option<bool> {
    if out.contains("'AC Power'") {
        Some(true)
    } else if out.contains("'Battery Power'") {
        Some(false)
    } else {
        None
    }
}

/// `(type, online)` of each power supply under /sys/class/power_supply.
fn supplies() -> Vec<(String, bool)> {
    let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else { return Vec::new() };
    dir.flatten()
        .map(|e| {
            let read = |f: &str| std::fs::read_to_string(e.path().join(f)).unwrap_or_default().trim().to_owned();
            (read("type"), read("online") == "1")
        })
        .collect()
}

/// Mains online: on AC. A battery and no mains online: on battery. No battery: a desktop, on AC.
fn parse_supplies(s: &[(String, bool)]) -> Option<bool> {
    if s.iter().any(|(t, online)| t == "Mains" && *online) {
        return Some(true);
    }
    Some(!s.iter().any(|(t, _)| t == "Battery"))
}

/// `ioreg -c IOHIDSystem`: `"HIDIdleTime" = 1234567` in nanoseconds.
fn parse_hid_idle(out: &str) -> Option<Duration> {
    let rest = out.split("\"HIDIdleTime\"").nth(1)?;
    let n: String = rest.trim_start_matches([' ', '=']).chars().take_while(char::is_ascii_digit).collect();
    n.parse().ok().map(Duration::from_nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmset_names_the_source() {
        assert_eq!(parse_pmset("Now drawing from 'AC Power'\n -InternalBattery-0 100%"), Some(true));
        assert_eq!(parse_pmset("Now drawing from 'Battery Power'\n"), Some(false));
        assert_eq!(parse_pmset(""), None);
    }

    #[test]
    fn supplies_decide_mains() {
        let bat = ("Battery".to_owned(), false);
        let mains = |on| ("Mains".to_owned(), on);
        assert_eq!(parse_supplies(&[bat.clone(), mains(true)]), Some(true));
        assert_eq!(parse_supplies(&[bat, mains(false)]), Some(false));
        assert_eq!(parse_supplies(&[]), Some(true));
    }

    #[test]
    fn hid_idle_is_nanoseconds() {
        assert_eq!(parse_hid_idle("  | \"HIDIdleTime\" = 2500000000\n"), Some(Duration::from_millis(2500)));
        assert_eq!(parse_hid_idle("nothing"), None);
    }
}
