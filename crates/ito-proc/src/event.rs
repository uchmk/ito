//! A named event that reaches a child whatever console it has (Windows). A child of a windows-subsystem
//! program has no console, so `CTRL_BREAK_EVENT` cannot reach it; the event can.

/// The environment variable that carries the event's name from [`crate::run_lines_soft`] to the child.
pub(crate) const ENV: &str = "ITO_PROC_STOP_EVENT";

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicU32, Ordering};

    type H = *mut std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateEventW(attrs: *const u8, manual_reset: i32, initial: i32, name: *const u16) -> H;
        fn OpenEventW(access: u32, inherit: i32, name: *const u16) -> H;
        fn SetEvent(h: H) -> i32;
        fn CloseHandle(h: H) -> i32;
        fn WaitForSingleObject(h: H, ms: u32) -> u32;
    }

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    /// The parent's end: created before the child starts, set to ask, closed when dropped.
    pub struct StopEvent {
        handle: H,
        name: String,
    }

    impl StopEvent {
        pub fn create() -> Option<StopEvent> {
            static N: AtomicU32 = AtomicU32::new(0);
            let name = format!("Local\\ito-proc-stop-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
            let w = wide(&name);
            // SAFETY: `w` is a NUL-terminated wide string that lives through the call; default security.
            let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, w.as_ptr()) };
            (!handle.is_null()).then_some(StopEvent { handle, name })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub fn set(&self) -> bool {
            // SAFETY: `handle` is a live event handle owned by `self`.
            unsafe { SetEvent(self.handle) != 0 }
        }
    }

    impl Drop for StopEvent {
        fn drop(&mut self) {
            // SAFETY: closing the handle this value owns, once.
            unsafe { CloseHandle(self.handle) };
        }
    }

    /// The child's end: wait on the named event in a thread and run `on_set` when it is set.
    pub fn watch_named(name: &str, on_set: fn()) {
        const SYNCHRONIZE: u32 = 0x0010_0000;
        let w = wide(name);
        // SAFETY: a NUL-terminated wide string that lives through the call.
        let handle = unsafe { OpenEventW(SYNCHRONIZE, 0, w.as_ptr()) };
        if handle.is_null() {
            return;
        }
        let raw = handle as usize;
        let _ = std::thread::Builder::new().name("ito-proc-stop".into()).spawn(move || {
            // SAFETY: the handle is ours and stays open for the life of the thread.
            if unsafe { WaitForSingleObject(raw as H, u32::MAX) } == 0 {
                on_set();
            }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    pub struct StopEvent;

    impl StopEvent {
        pub fn create() -> Option<StopEvent> {
            None
        }

        pub fn name(&self) -> &str {
            ""
        }

        pub fn set(&self) -> bool {
            false
        }
    }
}

#[cfg(windows)]
pub(crate) use imp::watch_named;
pub(crate) use imp::StopEvent;
