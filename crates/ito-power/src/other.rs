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
