//! Windows: `RegisterEventSourceW` / `ReportEventW` to the Application log.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::EventLog::{
    DeregisterEventSource, RegisterEventSourceW, ReportEventW, EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE, EVENTLOG_WARNING_TYPE,
};

use crate::{Level, WhyNot};

#[derive(Debug)]
pub struct Source(HANDLE);

// SAFETY: an event source handle is a kernel object handle, usable from any thread.
unsafe impl Send for Source {}
unsafe impl Sync for Source {}

impl Drop for Source {
    fn drop(&mut self) {
        // SAFETY: the handle came from RegisterEventSourceW and is closed once.
        unsafe {
            let _ = DeregisterEventSource(self.0);
        }
    }
}

fn failed(e: windows::core::Error) -> WhyNot {
    WhyNot::Failed(e.code().0 as u32)
}

pub fn open(source: &str) -> Result<Source, WhyNot> {
    let name = HSTRING::from(source);
    // SAFETY: `name` outlives the call; the handle is closed in Drop.
    unsafe { RegisterEventSourceW(PCWSTR::null(), &name) }.map(Source).map_err(failed)
}

impl Source {
    pub fn write(&self, level: Level, id: u32, text: &str) -> Result<(), WhyNot> {
        let kind = match level {
            Level::Info => EVENTLOG_INFORMATION_TYPE,
            Level::Warning => EVENTLOG_WARNING_TYPE,
            Level::Error => EVENTLOG_ERROR_TYPE,
        };
        let text = HSTRING::from(text);
        let strings = [PCWSTR(text.as_ptr())];
        // SAFETY: `strings` points at `text`, which outlives the call.
        unsafe { ReportEventW(self.0, kind, 0, id, None, 0, Some(&strings), None) }.map_err(failed)
    }
}
