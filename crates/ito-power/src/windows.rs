//! Windows: a power request (`PowerCreateRequest`) to hold, a waitable timer with resume to wake, and the
//! active power plan for the idle timeout and the wake-timer setting.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
use windows::Win32::Networking::NetworkListManager::{INetworkListManager, NetworkListManager};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, PowerClearRequest, PowerCreateRequest, PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex,
    PowerRequestSystemRequired, PowerSetRequest, SYSTEM_POWER_STATUS,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    CreateWaitableTimerW, SetWaitableTimer, POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

use crate::WhyNot;

/// Power plan: Sleep subgroup, "Sleep after".
const SUB_SLEEP: GUID = GUID::from_u128(0x238c9fa8_0aad_41ed_83f4_97be242c8f20);
const STANDBYIDLE: GUID = GUID::from_u128(0x29f6c1db_86da_48c5_9fdb_f2b67b1f44da);
/// Power plan: Sleep subgroup, "Allow wake timers" (0 off, 1 on, 2 important only).
const RTCWAKE: GUID = GUID::from_u128(0xbd3b718a_0680_4d9d_8ab2_e1d2b4ac806d);

#[derive(Debug)]
pub struct Hold(Option<HANDLE>);

// SAFETY: a power request handle is a kernel object handle, usable from any thread.
unsafe impl Send for Hold {}
unsafe impl Sync for Hold {}

impl Hold {
    pub fn active(&self) -> bool {
        self.0.is_some()
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            // SAFETY: `h` came from PowerCreateRequest and is closed once.
            unsafe {
                let _ = PowerClearRequest(h, PowerRequestSystemRequired);
                let _ = CloseHandle(h);
            }
        }
    }
}

pub fn hold(reason: &str) -> Hold {
    let mut text: Vec<u16> = reason.encode_utf16().chain(Some(0)).collect();
    let ctx = REASON_CONTEXT {
        Version: 0, // POWER_REQUEST_CONTEXT_VERSION
        Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
        Reason: REASON_CONTEXT_0 { SimpleReasonString: PWSTR(text.as_mut_ptr()) },
    };
    // SAFETY: `ctx` and `text` outlive the call; the handle is closed in Drop.
    unsafe {
        let Ok(h) = PowerCreateRequest(&ctx) else { return Hold(None) };
        if PowerSetRequest(h, PowerRequestSystemRequired).is_err() {
            let _ = CloseHandle(h);
            return Hold(None);
        }
        Hold(Some(h))
    }
}

#[derive(Debug)]
pub struct Timer {
    pub when: SystemTime,
    handle: HANDLE,
}

// SAFETY: a waitable timer handle is a kernel object handle, usable from any thread.
unsafe impl Send for Timer {}
unsafe impl Sync for Timer {}

impl Drop for Timer {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateWaitableTimerW and is closed once; closing cancels the timer.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

fn on_battery() -> bool {
    let mut st = SYSTEM_POWER_STATUS::default();
    // SAFETY: `st` is a valid out parameter.
    unsafe { GetSystemPowerStatus(&mut st).is_ok() && st.ACLineStatus == 0 }
}

/// One setting of the active plan on the current power source.
fn plan_value(setting: &GUID, battery: bool) -> Option<u32> {
    // SAFETY: the scheme GUID is freed with LocalFree; the out value is a valid u32.
    unsafe {
        let mut scheme: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(None, &mut scheme).0 != 0 || scheme.is_null() {
            return None;
        }
        let mut v = 0u32;
        let r = if battery {
            PowerReadDCValueIndex(None, Some(scheme), Some(&SUB_SLEEP), Some(setting), &mut v) == 0
        } else {
            PowerReadACValueIndex(None, Some(scheme), Some(&SUB_SLEEP), Some(setting), &mut v).0 == 0
        };
        let _ = LocalFree(Some(HLOCAL(scheme.cast())));
        r.then_some(v)
    }
}

pub fn wake_at(when: SystemTime) -> Result<Timer, WhyNot> {
    let battery = on_battery();
    if plan_value(&RTCWAKE, battery) == Some(0) {
        return Err(if battery { WhyNot::OnBattery } else { WhyNot::TimersDisabled });
    }
    // Absolute time: 100 ns steps since 1601, positive.
    let since_unix = when.duration_since(UNIX_EPOCH).map_err(|_| WhyNot::InThePast)?;
    let due = (since_unix.as_nanos() / 100) as i64 + 116_444_736_000_000_000;
    // SAFETY: the handle is owned by the Timer, which closes it.
    unsafe {
        let h = CreateWaitableTimerW(None, true, None).map_err(|e| WhyNot::Failed(e.code().0 as u32))?;
        if let Err(e) = SetWaitableTimer(h, &due, 0, None, None, true) {
            let _ = CloseHandle(h);
            return Err(WhyNot::Failed(e.code().0 as u32));
        }
        Ok(Timer { when, handle: h })
    }
}

pub fn sleep_after() -> Option<Duration> {
    match plan_value(&STANDBYIDLE, on_battery()) {
        Some(0) | None => None,
        Some(s) => Some(Duration::from_secs(s as u64)),
    }
}

pub fn on_ac() -> Option<bool> {
    let mut st = SYSTEM_POWER_STATUS::default();
    // SAFETY: `st` is a valid out parameter.
    unsafe { GetSystemPowerStatus(&mut st).ok()? };
    match st.ACLineStatus {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

pub fn network_up() -> Option<bool> {
    // The Network List Manager: any local network or internet connectivity, IPv4 or IPv6.
    const LOCAL_OR_INTERNET: i32 = 0x20 | 0x40 | 0x200 | 0x400;
    // SAFETY: COM calls on this thread; an apartment already set up in another mode is fine to use.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let nlm: INetworkListManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()?;
        let flags = nlm.GetConnectivity().ok()?.0;
        Some(flags & LOCAL_OR_INTERNET != 0)
    }
}

pub fn idle_for() -> Option<Duration> {
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: `info` is a valid out parameter with its size set.
    unsafe {
        if !GetLastInputInfo(&mut info).as_bool() {
            return None;
        }
        // Both are 32-bit millisecond ticks that wrap together (about every 49 days).
        Some(Duration::from_millis(GetTickCount().wrapping_sub(info.dwTime) as u64))
    }
}
