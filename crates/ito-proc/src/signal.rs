//! The child's side of a soft stop: a flag that turns true when the parent (or the user's Ctrl+C)
//! asks the process to stop.

use std::sync::atomic::AtomicBool;

static ASKED: AtomicBool = AtomicBool::new(false);

/// A flag that becomes `true` when this process is asked to stop: `CTRL_C`, `CTRL_BREAK` (what
/// [`crate::run_lines_soft`] sends), console close, logoff or shutdown on Windows; `SIGINT` or `SIGTERM`
/// elsewhere. The first call installs the handler; the same flag comes back every time.
///
/// The default ending (the process dies on the spot) is replaced, so the program must watch the flag and
/// return. One that never does can only be ended by a kill: [`crate::run_lines_soft`] does that once its grace runs out.
pub fn stop_requested() -> &'static AtomicBool {
    imp::install();
    &ASKED
}

#[cfg(windows)]
mod imp {
    use super::ASKED;
    use std::sync::atomic::Ordering;
    use std::sync::Once;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
    }

    unsafe extern "system" fn handler(_kind: u32) -> i32 {
        // CTRL_C, CTRL_BREAK, CTRL_CLOSE, CTRL_LOGOFF, CTRL_SHUTDOWN: all mean "stop"; handled here.
        ASKED.store(true, Ordering::Relaxed);
        1
    }

    fn set() {
        ASKED.store(true, Ordering::Relaxed);
    }

    pub fn install() {
        static ONCE: Once = Once::new();
        // SAFETY: `handler` is a plain `extern "system"` function that only stores into a static atomic.
        ONCE.call_once(|| unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
            // The parent's named event reaches a child that has no console to send CTRL_BREAK to.
            if let Ok(name) = std::env::var(crate::event::ENV) {
                crate::event::watch_named(&name, set);
            }
        });
    }
}

#[cfg(unix)]
mod imp {
    use super::ASKED;
    use std::sync::atomic::Ordering;
    use std::sync::Once;

    extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    // The same numbers on Linux and macOS.
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;

    extern "C" fn handler(_sig: i32) {
        ASKED.store(true, Ordering::Relaxed);
    }

    pub fn install() {
        static ONCE: Once = Once::new();
        // SAFETY: `handler` only stores into a static atomic, which is async-signal-safe.
        ONCE.call_once(|| unsafe {
            signal(SIGINT, handler as *const () as usize);
            signal(SIGTERM, handler as *const () as usize);
        });
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    pub fn install() {}
}
