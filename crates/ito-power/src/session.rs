//! Session events (sign-in, sign-out, lock, unlock) and the signed-in person's idle time.
//!
//! A process that runs in the person's own session gets the events from a hidden message-only window
//! (`WTSRegisterSessionNotification`). A Windows service (session 0) has no such window that works: the SCM
//! hands it `SERVICE_CONTROL_SESSIONCHANGE` instead, which `ito-service` passes on to [`service_session_change`]
//! once [`set_service_mode`] has been called. [`on_session`] picks the right way by itself.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::{sys, WhyNot};

/// Something that happened to a session of the PC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// Someone signed in.
    SignIn,
    /// Someone signed out.
    SignOut,
    /// The screen was locked.
    Lock,
    /// The screen was unlocked.
    Unlock,
}

impl SessionEvent {
    /// From a `WTS_SESSION_*` code (`wParam` of `WM_WTSSESSION_CHANGE`, or the event type of the service control).
    pub(crate) fn from_code(code: u32) -> Option<SessionEvent> {
        match code {
            5 => Some(SessionEvent::SignIn),
            6 => Some(SessionEvent::SignOut),
            7 => Some(SessionEvent::Lock),
            8 => Some(SessionEvent::Unlock),
            _ => None,
        }
    }
}

pub(crate) type Callback = Arc<dyn Fn(SessionEvent) + Send + Sync>;

static SERVICE: AtomicBool = AtomicBool::new(false);
static NEXT: AtomicU64 = AtomicU64::new(1);
static LISTENERS: Mutex<Vec<(u64, Callback)>> = Mutex::new(Vec::new());

fn listeners() -> std::sync::MutexGuard<'static, Vec<(u64, Callback)>> {
    LISTENERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Called by `ito-service` before it runs the service's main: from here on [`on_session`] is fed by
/// [`service_session_change`] and opens no window. Not for apps.
#[doc(hidden)]
pub fn set_service_mode() {
    SERVICE.store(true, Ordering::SeqCst);
}

/// Called by `ito-service` for `SERVICE_CONTROL_SESSIONCHANGE`: `code` is its event type, `session` the session id
/// (session 0 is the services' own and is ignored). Not for apps.
#[doc(hidden)]
pub fn service_session_change(code: u32, session: u32) {
    let Some(event) = SessionEvent::from_code(code) else { return };
    if session == 0 {
        return;
    }
    let all: Vec<Callback> = listeners().iter().map(|(_, f)| f.clone()).collect();
    for f in all {
        f(event);
    }
}

/// A subscription to session events. Drop it to stop.
pub struct SessionWatch(Inner);

enum Inner {
    Service(u64),
    #[allow(dead_code)] // never built where there are no windows
    Window(sys::Window),
}

impl std::fmt::Debug for SessionWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionWatch")
    }
}

impl Drop for SessionWatch {
    fn drop(&mut self) {
        if let Inner::Service(id) = self.0 {
            listeners().retain(|(i, _)| *i != id);
        }
    }
}

/// Call `f` for each session event until the returned watch is dropped. `f` runs on another thread and should
/// return quickly.
///
/// - In a process in the person's session: events of that session (sign-in does not happen there, a process
///   that lives through a sign-out is rare, so mostly [`SessionEvent::Lock`] and [`SessionEvent::Unlock`]).
/// - In a Windows service run by `ito-service`: events of every session but the services' own.
///
/// `Err(WhyNot::Unsupported)` on macOS and Linux; `Err(WhyNot::Failed(_))` when the window cannot be made.
pub fn on_session(f: impl Fn(SessionEvent) + Send + Sync + 'static) -> Result<SessionWatch, WhyNot> {
    let cb: Callback = Arc::new(f);
    if SERVICE.load(Ordering::SeqCst) {
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        listeners().push((id, cb));
        return Ok(SessionWatch(Inner::Service(id)));
    }
    sys::watch_session(cb).map(|w| SessionWatch(Inner::Window(w)))
}

/// How long the person at the PC's own screen (the console session) has not touched the keyboard or mouse.
/// Unlike [`crate::idle_for`], a service can read it. `None` when nobody is signed in there, the session is not
/// active, or the system does not say.
///
/// Windows keeps this time only for some sessions (Remote Desktop always; a console session on many PCs reports
/// zero), and then the answer is `None` rather than a guess. Check on the PC that matters; [`crate::idle_for`] in a
/// process of the person's own session is exact.
pub fn user_idle_for() -> Option<std::time::Duration> {
    sys::user_idle_for()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn codes_map_to_events() {
        assert_eq!(SessionEvent::from_code(5), Some(SessionEvent::SignIn));
        assert_eq!(SessionEvent::from_code(6), Some(SessionEvent::SignOut));
        assert_eq!(SessionEvent::from_code(7), Some(SessionEvent::Lock));
        assert_eq!(SessionEvent::from_code(8), Some(SessionEvent::Unlock));
        assert_eq!(SessionEvent::from_code(1), None);
    }

    #[test]
    fn a_service_watch_hears_events_until_dropped_and_ignores_session_0() {
        // Service mode is process-wide, so this is the only test that sets it.
        set_service_mode();
        let seen = Arc::new(AtomicUsize::new(0));
        let s = seen.clone();
        let w = on_session(move |e| {
            assert_eq!(e, SessionEvent::Lock);
            s.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        service_session_change(7, 2);
        service_session_change(7, 0);
        service_session_change(99, 2);
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        drop(w);
        service_session_change(7, 2);
        assert_eq!(seen.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_idle_time_is_sane_or_unknown() {
        if let Some(d) = user_idle_for() {
            assert!(d < std::time::Duration::from_secs(60 * 60 * 24 * 365), "{d:?}");
        }
    }
}
