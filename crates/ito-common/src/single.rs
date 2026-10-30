//! One running copy of an app (or of one part of it): a lock the OS drops when
//! the process dies, however it dies.
//!
//! [`acquire`] takes an exclusive lock on `single-<scope>.lock` in the app's
//! state folder ([`crate::state_dir`]) -- `LockFileEx` on Windows, `flock`
//! elsewhere, through `File::try_lock` -- and keeps it while the [`Guard`] lives.
//! A crash or a kill releases it, so there is no stale lock to clean up. The
//! holder's pid goes in the sibling `single-<scope>.pid` (a locked file cannot
//! be read on Windows), so the copy that lost can say who has it.
//!
//! When the state folder cannot be made or written, the guard is returned
//! anyway: an app that cannot take the lock still starts.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;

/// Another copy holds the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    /// Its pid, when it has written it yet.
    pub pid: Option<u32>,
}

impl std::fmt::Display for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pid {
            Some(pid) => write!(f, "already running (pid {pid})"),
            None => write!(f, "already running"),
        }
    }
}

impl std::error::Error for Held {}

/// The lock, held until this is dropped (or the process ends).
#[derive(Debug)]
pub struct Guard {
    // Held for its lock; closing it releases the lock.
    _lock: Option<File>,
    pid_file: Option<PathBuf>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(p) = &self.pid_file {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Take the lock `scope` of `app`. `Err(Held)` when another copy has it.
pub fn acquire(app: &str, scope: &str) -> Result<Guard, Held> {
    let Some(dir) = crate::state_dir(app) else { return Ok(Guard { _lock: None, pid_file: None }) };
    acquire_in(&dir, scope)
}

fn acquire_in(dir: &std::path::Path, scope: &str) -> Result<Guard, Held> {
    let open = || -> std::io::Result<(File, PathBuf)> {
        std::fs::create_dir_all(dir)?;
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!("single-{scope}.lock")))?;
        Ok((lock, dir.join(format!("single-{scope}.pid"))))
    };
    let Ok((lock, pid_file)) = open() else { return Ok(Guard { _lock: None, pid_file: None }) };
    match lock.try_lock() {
        Ok(()) => {
            let _ = std::fs::write(&pid_file, std::process::id().to_string());
            Ok(Guard { _lock: Some(lock), pid_file: Some(pid_file) })
        }
        Err(TryLockError::WouldBlock) => {
            let pid = std::fs::read_to_string(&pid_file).ok().and_then(|s| s.trim().parse().ok());
            Err(Held { pid })
        }
        // A file system that cannot lock: start anyway.
        Err(TryLockError::Error(_)) => Ok(Guard { _lock: None, pid_file: None }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-single-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_second_copy_is_refused_with_the_holders_pid_until_the_first_lets_go() {
        let d = temp("held");
        let first = acquire_in(&d, "app").expect("free");
        let held = acquire_in(&d, "app").expect_err("held");
        assert_eq!(held.pid, Some(std::process::id()));
        // Another scope is another lock.
        let other = acquire_in(&d, "server").expect("other scope");
        drop(first);
        let again = acquire_in(&d, "app").expect("released on drop");
        drop((other, again));
        let _ = std::fs::remove_dir_all(&d);
    }
}
