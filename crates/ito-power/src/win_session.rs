//! Windows: a hidden message-only window on its own thread for `WM_WTSSESSION_CHANGE`, and the console
//! session's last input time from `WTSQuerySessionInformationW(WTSSessionInfoEx)`.

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSFreeMemory, WTSGetActiveConsoleSessionId, WTSQuerySessionInformationW, WTSRegisterSessionNotification,
    WTSUnRegisterSessionNotification, WTSINFOEXW, WTS_INFO_CLASS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowLongPtrW, PostMessageW, PostQuitMessage,
    RegisterClassW, SetWindowLongPtrW, TranslateMessage, GWLP_USERDATA, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE,
    WM_DESTROY, WNDCLASSW,
};

use crate::session::{Callback, SessionEvent};
use crate::WhyNot;

const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const NOTIFY_FOR_THIS_SESSION: u32 = 0;
/// `WTSSessionInfoEx`.
const SESSION_INFO_EX: WTS_INFO_CLASS = WTS_INFO_CLASS(25);
/// `WTSActive`.
const WTS_ACTIVE: i32 = 0;

/// The window thread. Dropping it closes the window and waits for the thread.
pub struct Window {
    hwnd: isize,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: the window belongs to the thread, which handles WM_CLOSE by destroying it and quitting.
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_WTSSESSION_CHANGE => {
            // SAFETY: the pointer was set by `run` to a leaked Box<Callback> that lives until WM_DESTROY.
            let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Callback;
            if let (false, Some(event)) = (p.is_null(), SessionEvent::from_code(wparam.0 as u32)) {
                unsafe { (*p)(event) };
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // SAFETY: closing our own window.
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: undo the registration, free the callback box set in `run`, and end the loop.
            unsafe {
                let _ = WTSUnRegisterSessionNotification(hwnd);
                let p = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut Callback;
                if !p.is_null() {
                    drop(Box::from_raw(p));
                }
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        // SAFETY: the default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn run(cb: Callback, ready: mpsc::Sender<Result<isize, WhyNot>>) {
    let fail = |e: windows::core::Error| WhyNot::Failed(e.code().0 as u32);
    // SAFETY: plain Win32 window calls on this thread; the callback box is freed in WM_DESTROY.
    unsafe {
        let module = match GetModuleHandleW(None) {
            Ok(m) => m,
            Err(e) => {
                let _ = ready.send(Err(fail(e)));
                return;
            }
        };
        let class =
            WNDCLASSW { lpfnWndProc: Some(proc), hInstance: module.into(), lpszClassName: w!("ItoPowerSession"), ..Default::default() };
        // A second watch registers the class again and is told it exists: fine.
        RegisterClassW(&class);
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("ItoPowerSession"),
            w!("ito-power session"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module.into()),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                let _ = ready.send(Err(fail(e)));
                return;
            }
        };
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(cb)) as isize);
        if let Err(e) = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) {
            let _ = DestroyWindow(hwnd); // WM_DESTROY frees the callback
            let mut m = MSG::default();
            while GetMessageW(&mut m, None, 0, 0).as_bool() {}
            let _ = ready.send(Err(fail(e)));
            return;
        }
        let _ = ready.send(Ok(hwnd.0 as isize));
        let mut m = MSG::default();
        while GetMessageW(&mut m, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

pub fn watch_session(cb: Callback) -> Result<Window, WhyNot> {
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::Builder::new().name("ito-power-session".into()).spawn(move || run(cb, tx)).map_err(|_| WhyNot::Failed(0))?;
    match rx.recv() {
        Ok(Ok(hwnd)) => Ok(Window { hwnd, thread: Some(thread) }),
        Ok(Err(why)) => {
            let _ = thread.join();
            Err(why)
        }
        Err(_) => Err(WhyNot::Failed(0)),
    }
}

pub fn user_idle_for() -> Option<Duration> {
    // SAFETY: the buffer WTS returns is read within `bytes` and freed with WTSFreeMemory.
    unsafe {
        let session = WTSGetActiveConsoleSessionId();
        if session == 0xFFFF_FFFF {
            return None;
        }
        let mut buf = PWSTR::null();
        let mut bytes = 0u32;
        WTSQuerySessionInformationW(None, session, SESSION_INFO_EX, &mut buf, &mut bytes).ok()?;
        let idle = if buf.is_null() || (bytes as usize) < size_of::<WTSINFOEXW>() {
            None
        } else {
            let info = &*(buf.0 as *const WTSINFOEXW);
            let l = &info.Data.WTSInfoExLevel1;
            // Both are 100 ns FILETIME-style ticks. Zero means the session never saw input (or the system does not track it).
            (info.Level == 1 && l.SessionState.0 == WTS_ACTIVE && l.LastInputTime > 0 && l.CurrentTime >= l.LastInputTime)
                .then(|| Duration::from_nanos((l.CurrentTime - l.LastInputTime) as u64 * 100))
        };
        WTSFreeMemory(buf.0.cast());
        idle
    }
}
