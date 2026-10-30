//! A line in the system log, for the monitoring tool (SCOM, Zabbix, Event Viewer) that watches a PC.
//!
//! [`Log::open`] names the source; [`Log::write`] reports one event with a [`Level`], an event id (one
//! per state the monitor tells apart: ended OK, failed, late) and a text.
//!
//! Windows writes to the Application log through `ReportEventW`. The source needs no registration: an
//! unregistered one still records the event, and Event Viewer shows the text under the note that the
//! description cannot be found. To show the text without that note, register the source once (an
//! administrator, `EventMessageFile` under `HKLM\SYSTEM\CurrentControlSet\Services\EventLog\Application`);
//! this crate does not. Elsewhere the line goes to syslog with `logger`, tagged with the source; the id
//! is part of the text.

use std::fmt;

#[cfg(windows)]
#[path = "windows.rs"]
mod sys;
#[cfg(not(windows))]
#[path = "other.rs"]
mod sys;

/// How serious the event is (Windows: Information, Warning, Error).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// Why a line could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WhyNot {
    /// The system has no log to write to (no `logger` on this PC).
    Unsupported,
    /// The system refused (the OS error number).
    Failed(u32),
}

impl fmt::Display for WhyNot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WhyNot::Unsupported => f.write_str("this system has no log to write to"),
            WhyNot::Failed(n) => write!(f, "the system log refused (error {n})"),
        }
    }
}

impl std::error::Error for WhyNot {}

/// An open source. Drop it to close.
#[derive(Debug)]
pub struct Log(sys::Source);

impl Log {
    /// Open `source`, the name the monitor filters on (`koyomi`).
    pub fn open(source: &str) -> Result<Log, WhyNot> {
        sys::open(source).map(Log)
    }

    /// Report one event. `id` is the event id (Windows) and is kept in the text elsewhere.
    pub fn write(&self, level: Level, id: u32, text: &str) -> Result<(), WhyNot> {
        self.0.write(level, id, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn why_not_reads_as_a_sentence() {
        assert!(WhyNot::Unsupported.to_string().contains("no log"));
        assert!(WhyNot::Failed(5).to_string().contains('5'));
    }
}
