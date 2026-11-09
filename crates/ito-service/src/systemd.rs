//! `state` on Linux: ask systemd with `systemctl show`.

use crate::{Error, State, Status};

/// Where the unit `<name>.service` is; `None` when systemd has no such unit (or is not what booted this machine).
#[cfg(target_os = "linux")]
pub fn state(name: &str) -> Result<Option<Status>, Error> {
    use std::io::ErrorKind;
    use std::process::{Command, Stdio};

    // These are not unit-name characters, and `systemctl show` would read them as a pattern for some other unit.
    if name.contains(['*', '?', '[']) {
        return Err(Error::BadName);
    }
    // The test `sd_booted()` makes. `systemctl` can be installed where systemd is not running (a container, WSL), and that is
    // "no such service here", not a failed query.
    match std::fs::metadata("/run/systemd/system") {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Failed(e.raw_os_error().unwrap_or(0) as u32)),
    }
    let out = match Command::new("systemctl")
        .args(["show", "--property=LoadState,ActiveState,MainPID", "--"])
        .arg(format!("{name}.service"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(out) => out,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::Failed(e.raw_os_error().unwrap_or(0) as u32)),
    };
    if !out.status.success() {
        return Err(Error::Failed(out.status.code().unwrap_or(0) as u32));
    }
    parse_show(&String::from_utf8_lossy(&out.stdout))
}

/// Read the `KEY=VALUE` lines of `systemctl show` (in any order) into a status.
pub(crate) fn parse_show(text: &str) -> Result<Option<Status>, Error> {
    let (mut load, mut active, mut pid) = (None, None, 0);
    for line in text.lines() {
        match line.split_once('=') {
            Some(("LoadState", v)) => load = Some(v.trim()),
            Some(("ActiveState", v)) => active = Some(v.trim()),
            Some(("MainPID", v)) => pid = v.trim().parse::<u32>().unwrap_or(0),
            _ => {}
        }
    }
    match load {
        None => return Err(Error::Failed(0)),
        Some("not-found") => return Ok(None),
        Some(_) => {}
    }
    let state = match active {
        Some("active" | "reloading" | "refreshing") => State::Running,
        Some("inactive" | "failed" | "maintenance") => State::Stopped,
        Some("activating") => State::Starting,
        Some("deactivating") => State::Stopping,
        _ => return Err(Error::Failed(0)),
    };
    Ok(Some(Status { state, pid }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(load: &str, active: &str, pid: &str) -> String {
        format!("MainPID={pid}\nLoadState={load}\nActiveState={active}\n")
    }

    #[test]
    fn every_active_state() {
        for (active, want) in [
            ("active", State::Running),
            ("reloading", State::Running),
            ("refreshing", State::Running),
            ("inactive", State::Stopped),
            ("failed", State::Stopped),
            ("maintenance", State::Stopped),
            ("activating", State::Starting),
            ("deactivating", State::Stopping),
        ] {
            assert_eq!(parse_show(&show("loaded", active, "1234")), Ok(Some(Status { state: want, pid: 1234 })), "{active}");
        }
    }

    #[test]
    fn units_and_order() {
        assert_eq!(parse_show(&show("not-found", "inactive", "0")), Ok(None));
        assert_eq!(parse_show(&show("masked", "inactive", "0")), Ok(Some(Status { state: State::Stopped, pid: 0 })));
        assert_eq!(parse_show("ActiveState=active\nMainPID=7\nLoadState=loaded"), Ok(Some(Status { state: State::Running, pid: 7 })));
        assert_eq!(parse_show("LoadState=loaded\nActiveState=active\n"), Ok(Some(Status { state: State::Running, pid: 0 })), "no MainPID");
        assert_eq!(parse_show(&show("loaded", "active", "x")), Ok(Some(Status { state: State::Running, pid: 0 })));
    }

    #[test]
    fn what_is_not_known_is_an_error() {
        assert_eq!(parse_show("ActiveState=active\nMainPID=1\n"), Err(Error::Failed(0)));
        assert_eq!(parse_show(""), Err(Error::Failed(0)));
        assert_eq!(parse_show(&show("loaded", "bogus", "1")), Err(Error::Failed(0)));
        assert_eq!(parse_show("LoadState=loaded\n"), Err(Error::Failed(0)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_state() {
        assert_eq!(state("a*b"), Err(Error::BadName));
        // The machine may have no systemd or may not reach it; only "unsupported" and "found" are wrong.
        match state("ito-service-test-no-such-unit") {
            Ok(None) | Err(Error::Failed(_)) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}
