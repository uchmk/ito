//! Power and sleep for an app that runs jobs on an ordinary PC.
//!
//! - [`Awake::hold`]: keep the PC from sleeping while a job runs (a guard; drop it to let the PC sleep).
//! - [`wake_at`]: wake the PC for a time, or say [`WhyNot`] it cannot (a handle; drop it to cancel).
//! - [`sleep_after`]: how long the PC idles before it sleeps, on the current power source.
//! - [`on_ac`], [`network_up`], [`idle_for`]: probes for "run only when ..." conditions; `None` when the system cannot say.
//!
//! Windows uses power requests, a resume timer and the active power plan. macOS holds with `caffeinate`
//! and Linux with `systemd-inhibit` (both a child process killed on drop); neither can wake the PC, so
//! [`wake_at`] says `Unsupported` there and [`sleep_after`] is `None`. Nothing here blocks for long, but
//! call it from a worker if the app is strict about the UI thread.

use std::fmt;
use std::time::{Duration, SystemTime};

#[cfg(windows)]
#[path = "windows.rs"]
mod sys;
#[cfg(not(windows))]
#[path = "other.rs"]
mod sys;

/// A hold on the PC: while it lives, the system does not go to sleep by idling. Drop it to let go.
#[derive(Debug)]
pub struct Awake(sys::Hold);

impl Awake {
    /// Keep the PC awake. `reason` is what the system shows in `powercfg /requests` (Windows).
    /// When the system cannot hold, the guard is inert: see [`Awake::is_active`].
    pub fn hold(reason: &str) -> Awake {
        Awake(sys::hold(reason))
    }

    /// Whether the hold really took effect.
    pub fn is_active(&self) -> bool {
        self.0.active()
    }
}

/// A timer that wakes the PC. Drop it to cancel.
#[derive(Debug)]
pub struct WakeHandle(sys::Timer);

impl WakeHandle {
    /// The time it was set for.
    pub fn when(&self) -> SystemTime {
        self.0.when
    }
}

/// Why the PC cannot be woken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhyNot {
    /// The power plan has wake timers off.
    TimersDisabled,
    /// On battery, and the plan allows no wake timers there.
    OnBattery,
    /// The system has no way to do it (macOS, Linux, or no permission).
    Unsupported,
    /// The time has passed already.
    InThePast,
    /// The system refused for another reason (the OS error number).
    Failed(u32),
}

impl fmt::Display for WhyNot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WhyNot::TimersDisabled => f.write_str("wake timers are off in the power plan"),
            WhyNot::OnBattery => f.write_str("on battery, and the power plan allows no wake timers there"),
            WhyNot::Unsupported => f.write_str("this system cannot wake the PC from here"),
            WhyNot::InThePast => f.write_str("that time has passed"),
            WhyNot::Failed(n) => write!(f, "the system refused (error {n})"),
        }
    }
}

impl std::error::Error for WhyNot {}

/// Wake the PC at `when` (it only wakes; it does not stay awake: [`Awake::hold`] after).
pub fn wake_at(when: SystemTime) -> Result<WakeHandle, WhyNot> {
    if when <= SystemTime::now() {
        return Err(WhyNot::InThePast);
    }
    sys::wake_at(when).map(WakeHandle)
}

/// How long the PC idles before it sleeps, on the power source it runs on now. `None` when it never
/// sleeps, or when this is not known.
pub fn sleep_after() -> Option<Duration> {
    sys::sleep_after()
}

/// Whether the PC runs on mains power (`Some(false)`: on battery). A PC without a battery is on mains.
/// `None` when this is not known.
pub fn on_ac() -> Option<bool> {
    sys::on_ac()
}

/// Whether a network connection is up (a local network or the internet; loopback does not count).
/// `None` when this is not known.
pub fn network_up() -> Option<bool> {
    sys::network_up()
}

/// How long the user has not touched the keyboard or mouse. `None` when this is not known (Linux
/// needs `xprintidle`).
pub fn idle_for() -> Option<Duration> {
    sys::idle_for()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probes_answer_or_say_unknown_without_panicking() {
        let _ = on_ac();
        let _ = network_up();
        if let Some(d) = idle_for() {
            assert!(d < Duration::from_secs(60 * 60 * 24 * 365), "{d:?}");
        }
    }

    #[test]
    fn a_time_in_the_past_cannot_be_woken_for() {
        assert_eq!(wake_at(SystemTime::now() - Duration::from_secs(5)).unwrap_err(), WhyNot::InThePast);
    }

    #[test]
    fn holding_and_letting_go_works_twice() {
        let a = Awake::hold("ito-power test");
        let b = Awake::hold("ito-power test 2");
        #[cfg(windows)]
        assert!(a.is_active() && b.is_active());
        drop(a);
        drop(b);
    }

    #[test]
    fn why_not_reads_as_a_sentence() {
        assert!(WhyNot::OnBattery.to_string().contains("battery"));
        assert!(WhyNot::Failed(5).to_string().contains('5'));
    }

    #[test]
    fn the_sleep_timeout_is_sane() {
        if let Some(d) = sleep_after() {
            assert!(d <= Duration::from_secs(60 * 60 * 24 * 7), "{d:?}");
        }
    }

    #[test]
    fn a_wake_timer_is_set_or_explained() {
        match wake_at(SystemTime::now() + Duration::from_secs(3600)) {
            Ok(h) => assert!(h.when() > SystemTime::now()),
            Err(why) => assert!(!why.to_string().is_empty()),
        }
    }
}
