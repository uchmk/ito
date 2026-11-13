//! Session events (sign-in, sign-out, lock, unlock) and the signed-in person's idle time.
//!
//! A process that runs in the person's own session gets the events from a hidden message-only window
//! (`WTSRegisterSessionNotification`). A Windows service (session 0) has no such window that works: the SCM
//! hands it `SERVICE_CONTROL_SESSIONCHANGE` instead, which a service host such as koyomi's `koyomi-service` passes on to [`service_session_change`]
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

/// Whose session an event is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    /// The Windows session id (1, 2, ...). 0, the services' own, is never passed.
    pub id: u32,
    /// `DOMAIN\name` (`PC\name` for a local account) of the person signed in to it. For a sign-out, the name
    /// remembered from the session's earlier events (the system may have forgotten it by then); `None` when it was
    /// never known.
    pub user: Option<String>,
    /// True for a Remote Desktop session, false for the PC's own screen.
    pub remote: bool,
}

/// Whether two user names are the same person the way Windows reads them: `PC\taro`, `.\taro` and `taro` match
/// (a missing domain or `.` matches any domain), case-insensitively. Two different domains do not match.
pub fn same_user(a: &str, b: &str) -> bool {
    fn split(s: &str) -> (Option<String>, String) {
        match s.trim().rsplit_once('\\') {
            Some((d, n)) => ((!d.is_empty() && d != ".").then(|| d.to_lowercase()), n.to_lowercase()),
            None => (None, s.trim().to_lowercase()),
        }
    }
    let ((da, na), (db, nb)) = (split(a), split(b));
    na == nb && !na.is_empty() && (da.is_none() || db.is_none() || da == db)
}

pub(crate) type Callback = Arc<dyn Fn(SessionEvent, &SessionInfo) + Send + Sync>;

static USERS: Mutex<Vec<(u32, String)>> = Mutex::new(Vec::new());

/// Build the [`SessionInfo`] for an event from what the system says now (`looked`) and what was remembered.
fn describe(event: SessionEvent, id: u32, looked: Option<String>, remote: bool) -> SessionInfo {
    let mut known = USERS.lock().unwrap_or_else(|e| e.into_inner());
    let at = known.iter().position(|(i, _)| *i == id);
    let user = if event == SessionEvent::SignOut {
        // By now the system may have forgotten the name: the remembered one comes first, and the session is forgotten.
        let remembered = at.map(|i| known.remove(i).1);
        remembered.or(looked)
    } else {
        match (looked, at) {
            (Some(u), Some(i)) => {
                known[i].1 = u.clone();
                Some(u)
            }
            (Some(u), None) => {
                known.push((id, u.clone()));
                Some(u)
            }
            (None, Some(i)) => Some(known[i].1.clone()),
            (None, None) => None,
        }
    };
    SessionInfo { id, user, remote }
}

/// Tell `cb` about an event of session `id`.
pub(crate) fn emit(cb: &Callback, event: SessionEvent, id: u32) {
    if id == 0 {
        return;
    }
    cb(event, &describe(event, id, sys::session_user(id), sys::session_remote(id)));
}

static SERVICE: AtomicBool = AtomicBool::new(false);
static NEXT: AtomicU64 = AtomicU64::new(1);
static LISTENERS: Mutex<Vec<(u64, Callback)>> = Mutex::new(Vec::new());

fn listeners() -> std::sync::MutexGuard<'static, Vec<(u64, Callback)>> {
    LISTENERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Called by an app's service host (koyomi's `koyomi-service`) before it runs the service's main: from here on
/// [`on_session`] is fed by [`service_session_change`] and opens no window. Not for apps.
#[doc(hidden)]
pub fn set_service_mode() {
    SERVICE.store(true, Ordering::SeqCst);
}

/// Called by an app's service host (koyomi's `koyomi-service`) for `SERVICE_CONTROL_SESSIONCHANGE`: `code` is its event type, `session` the session id
/// (session 0 is the services' own and is ignored). Not for apps.
#[doc(hidden)]
pub fn service_session_change(code: u32, session: u32) {
    let Some(event) = SessionEvent::from_code(code) else { return };
    if session == 0 {
        return;
    }
    let all: Vec<Callback> = listeners().iter().map(|(_, f)| f.clone()).collect();
    for f in all {
        emit(&f, event, session);
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
/// - In a Windows service run by a service host such as koyomi's `koyomi-service`: events of every session but the services' own.
///
/// `Err(WhyNot::Unsupported)` on macOS and Linux; `Err(WhyNot::Failed(_))` when the window cannot be made.
pub fn on_session(f: impl Fn(SessionEvent) + Send + Sync + 'static) -> Result<SessionWatch, WhyNot> {
    on_session_with(move |e, _| f(e))
}

/// Like [`on_session`], and says whose session it was ([`SessionInfo`]: the id, `DOMAIN\name`, remote or not).
/// The name of a sign-out is the one remembered from the session's earlier events, since the system may already have
/// forgotten it. [`same_user`] compares names.
pub fn on_session_with(f: impl Fn(SessionEvent, &SessionInfo) + Send + Sync + 'static) -> Result<SessionWatch, WhyNot> {
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
        let w = on_session_with(move |e, info| {
            assert_eq!(e, SessionEvent::Lock);
            assert_eq!(info.id, 2);
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
    fn a_sign_out_gets_the_name_remembered_for_its_session() {
        let a = |e, looked: Option<&str>| describe(e, 71, looked.map(str::to_owned), false);
        assert_eq!(a(SessionEvent::SignIn, Some("PC\\taro")).user.as_deref(), Some("PC\\taro"));
        assert_eq!(a(SessionEvent::Lock, None).user.as_deref(), Some("PC\\taro"), "kept when the system says nothing");
        let out = a(SessionEvent::SignOut, None);
        assert_eq!((out.id, out.user.as_deref()), (71, Some("PC\\taro")));
        assert_eq!(a(SessionEvent::SignOut, None).user, None, "forgotten after the sign-out");
        let b = describe(SessionEvent::SignOut, 72, Some("PC\\hanako".into()), false);
        assert_eq!(b.user.as_deref(), Some("PC\\hanako"), "never seen: the lookup");
    }

    #[test]
    fn user_names_compare_the_way_windows_reads_them() {
        assert!(same_user("PC\\taro", ".\\taro"));
        assert!(same_user("PC\\Taro", "taro"));
        assert!(same_user("taro", "TARO"));
        assert!(!same_user("PC\\taro", "OTHER\\taro"));
        assert!(!same_user("PC\\taro", "PC\\hanako"));
        assert!(!same_user("", ""));
    }

    #[test]
    fn the_idle_time_is_sane_or_unknown() {
        if let Some(d) = user_idle_for() {
            assert!(d < std::time::Duration::from_secs(60 * 60 * 24 * 365), "{d:?}");
        }
    }
}
