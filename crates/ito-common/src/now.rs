//! The time now, in one place, so a test build can set it.
//!
//! [`now`] and [`now_local`] are `Utc::now()` and `Local::now()` in a normal build. With the cargo feature
//! `fake-clock` they read `UCHMK_FAKE_CLOCK=<file>` instead: a TOML file with `now = "2026-12-31T23:59:50+09:00"`
//! (RFC 3339). The clock then runs on from that time at the real speed; the file is read again at most once a
//! second, and when its `now` changes the clock jumps to it (forward or back). That runs the rows that need
//! "an hour back" or "past midnight" without moving the OS clock.
//!
//! A build without the feature never looks at the variable, so a shipped app cannot be made to run on a wrong
//! time. `Instant` is not touched: intervals stay real.

use chrono::{DateTime, Local, Utc};

/// The name of the environment variable that points at the fake clock file (`fake-clock` builds only).
pub const ENV: &str = "UCHMK_FAKE_CLOCK";

/// The time now (UTC).
pub fn now() -> DateTime<Utc> {
    #[cfg(feature = "fake-clock")]
    if let Some(t) = fake::now() {
        return t;
    }
    Utc::now()
}

/// The time now in the local zone.
pub fn now_local() -> DateTime<Local> {
    now().with_timezone(&Local)
}

/// `Some(text)` when the clock is fake: a line for the top of a log ("fake clock: …"), so a log is not mistaken
/// for a real run. Always `None` in a build without `fake-clock`.
pub fn fake_notice() -> Option<String> {
    #[cfg(feature = "fake-clock")]
    if let Some(t) = fake::now() {
        return Some(format!("fake clock: {} (from {})", t.to_rfc3339(), std::env::var(ENV).unwrap_or_default()));
    }
    None
}

#[cfg(feature = "fake-clock")]
mod fake {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// A clock that follows a file.
    pub(super) struct Follow {
        path: std::path::PathBuf,
        /// The `now` last read from the file, and the real instant it was taken at.
        set: Option<(DateTime<Utc>, DateTime<Utc>, Instant)>,
        read_at: Option<Instant>,
    }

    impl Follow {
        pub(super) fn new(path: std::path::PathBuf) -> Follow {
            Follow { path, set: None, read_at: None }
        }

        fn read(&self) -> Option<DateTime<Utc>> {
            let text = std::fs::read_to_string(&self.path).ok()?;
            let v: toml::Table = text.parse().ok()?;
            let s = v.get("now")?.as_str()?;
            DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
        }

        /// The fake time at the real instant `at`, re-reading the file when a second has passed.
        pub(super) fn now_at(&mut self, at: Instant) -> Option<DateTime<Utc>> {
            let stale = self.read_at.is_none_or(|r| at.duration_since(r) >= Duration::from_secs(1));
            if stale {
                self.read_at = Some(at);
                if let Some(t) = self.read() {
                    // A new value (or the first): the clock starts again from it.
                    if self.set.map(|(v, _, _)| v) != Some(t) {
                        self.set = Some((t, t, at));
                    }
                }
            }
            let (_, base, since) = self.set?;
            Some(base + chrono::Duration::from_std(at.duration_since(since)).ok()?)
        }
    }

    static STATE: Mutex<Option<Follow>> = Mutex::new(None);

    pub(super) fn now() -> Option<DateTime<Utc>> {
        let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            *g = Some(Follow::new(std::env::var_os(ENV)?.into()));
        }
        g.as_mut()?.now_at(Instant::now())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn at(s: &str) -> DateTime<Utc> {
            DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
        }

        #[test]
        fn it_runs_on_from_the_file_and_jumps_when_the_file_changes() {
            let dir = tempfile::tempdir().unwrap();
            let f = dir.path().join("clock.toml");
            std::fs::write(&f, "now = \"2026-12-31T23:59:50+09:00\"\n").unwrap();
            let mut c = Follow::new(f.clone());
            let t0 = Instant::now();
            assert_eq!(c.now_at(t0), Some(at("2026-12-31T14:59:50Z")));
            // Ten real seconds later the fake time is ten seconds on: past midnight in Japan.
            std::fs::write(&f, "now = \"1999-01-01T00:00:00Z\"\n").unwrap();
            assert_eq!(c.now_at(t0 + Duration::from_millis(500)), Some(at("2026-12-31T14:59:50.5Z")));
            // After a second the file is read again: the new value is taken, back in time.
            assert_eq!(c.now_at(t0 + Duration::from_secs(10)), Some(at("1999-01-01T00:00:00Z")));
            assert_eq!(c.now_at(t0 + Duration::from_secs(10) + Duration::from_millis(300)), Some(at("1999-01-01T00:00:00.3Z")));
        }

        #[test]
        fn a_missing_or_broken_file_is_no_fake_time() {
            let dir = tempfile::tempdir().unwrap();
            let mut c = Follow::new(dir.path().join("none.toml"));
            assert_eq!(c.now_at(Instant::now()), None);
            std::fs::write(dir.path().join("bad.toml"), "now = 5\n").unwrap();
            assert_eq!(Follow::new(dir.path().join("bad.toml")).now_at(Instant::now()), None);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn without_the_variable_it_is_the_real_time() {
        let a = chrono::Utc::now();
        let b = super::now();
        assert!((b - a).num_seconds().abs() < 5);
    }
}
