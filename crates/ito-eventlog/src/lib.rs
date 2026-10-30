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
//!
//! [`Subscription`] reads the other way: the new records of a channel (`Application`, `System`,
//! `Security`) that match a [`Filter`] (event ids, source, level), through `EvtQuery`. The position is
//! the record id of the last record delivered ([`Subscription::position`]); an app saves it and starts
//! again from [`From::After`], so a restart repeats nothing and misses nothing the log still holds.
//! Reading `Security` needs the right to ([`WhyNot::Denied`] otherwise). Only Windows reads; elsewhere
//! it is [`WhyNot::Unsupported`].

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
    /// The person may not read this log (`Security` needs an administrator, or membership of
    /// *Event Log Readers*).
    Denied,
    /// There is no log of that name.
    NoChannel,
}

impl fmt::Display for WhyNot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WhyNot::Unsupported => f.write_str("this system has no log to write to"),
            WhyNot::Failed(n) => write!(f, "the system log refused (error {n})"),
            WhyNot::Denied => f.write_str("not allowed to read this log (run as an administrator, or join Event Log Readers)"),
            WhyNot::NoChannel => f.write_str("there is no log with that name"),
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

/// Which records to deliver; an empty `ids` means any id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub ids: Vec<u32>,
    /// The provider name (`Microsoft-Windows-Security-Auditing`).
    pub source: Option<String>,
    /// `Error` also takes Critical; `Info` takes Information and Verbose.
    pub level: Option<Level>,
}

/// Where a [`Subscription`] begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    /// Only records written from now on.
    End,
    /// The oldest record the log still holds.
    Start,
    /// The records after this record id (from [`Subscription::position`]).
    After(u64),
}

/// One event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub record_id: u64,
    /// When it happened, as the log has it: ISO 8601, UTC (`2026-10-31T01:02:03.1234567Z`).
    pub time: String,
    pub id: u32,
    pub source: String,
    pub level: Level,
    /// The formatted text; the event's data values joined by spaces when the provider's text is not on this PC.
    pub message: String,
}

/// New records of one log. Not `Sync`; give it to one thread (a worker, not the UI).
#[derive(Debug)]
pub struct Subscription(sys::Reader);

impl Subscription {
    /// Open `channel` (`"Application"`, `"System"`, `"Security"`, or a full channel path).
    pub fn new(channel: &str, filter: &Filter, from: From) -> Result<Subscription, WhyNot> {
        sys::reader(channel, filter, from).map(Subscription)
    }

    /// The records after the last call, oldest first.
    pub fn poll(&mut self) -> Result<Vec<Record>, WhyNot> {
        self.0.poll()
    }

    /// The record id of the last record delivered (or where it began): save it, and give it back as
    /// [`From::After`].
    pub fn position(&self) -> u64 {
        self.0.position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn why_not_reads_as_a_sentence() {
        assert!(WhyNot::Unsupported.to_string().contains("no log"));
        assert!(WhyNot::Failed(5).to_string().contains('5'));
        assert!(WhyNot::Denied.to_string().contains("Event Log Readers"));
    }

    #[cfg(windows)]
    #[test]
    fn the_application_log_reads_and_a_missing_one_is_named() {
        let mut s = Subscription::new("Application", &Filter::default(), From::End).unwrap();
        assert!(s.poll().unwrap().is_empty() || s.position() > 0);
        let all = Subscription::new("Application", &Filter { level: Some(Level::Info), ..Filter::default() }, From::Start);
        let mut all = all.unwrap();
        let first = all.poll().unwrap();
        assert!(first.windows(2).all(|w| w[0].record_id < w[1].record_id));
        assert_eq!(all.position(), first.last().map_or(0, |r| r.record_id));
        assert_eq!(Subscription::new("No-Such-Channel-ito", &Filter::default(), From::End).err(), Some(WhyNot::NoChannel));
    }

    #[cfg(windows)]
    #[test]
    fn what_we_write_we_can_read_back() {
        let id = 4242 + std::process::id() % 1000;
        let mut s = Subscription::new(
            "Application",
            &Filter { ids: vec![id], source: Some("ito-eventlog-test".into()), level: Some(Level::Warning) },
            From::End,
        )
        .unwrap();
        Log::open("ito-eventlog-test").unwrap().write(Level::Warning, id, "hello from the test").unwrap();
        let mut got = Vec::new();
        for _ in 0..50 {
            got.extend(s.poll().unwrap());
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!((got[0].id & 0xFFFF, got[0].level), (id & 0xFFFF, Level::Warning));
        assert!(got[0].message.contains("hello from the test"), "{:?}", got[0]);
        assert_eq!(s.position(), got[0].record_id);
    }
}
