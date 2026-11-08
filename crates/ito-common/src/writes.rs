//! Config writes in flight, so a `--keys` run can wait for them.
//!
//! The app takes a [`Ticket`] when it starts a write and answers it once the UI has applied the result (or the write
//! failed). A runner reads [`Ledger::status`].

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

#[derive(Debug)]
struct Inner {
    watching: bool,
    next: u64,
    pending: Vec<Pending>,
    answers: u64,
    failed: Option<Failure>,
}

type Shared = Arc<Mutex<Inner>>;

fn lock(shared: &Shared) -> std::sync::MutexGuard<'_, Inner> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// A count of config writes in flight. Clones share one count.
#[derive(Clone, Debug)]
pub struct Ledger {
    shared: Shared,
}

/// The app's ledger. It records nothing until [`Ledger::watch`] is called (for a `--keys` run), so an app run the
/// usual way keeps nothing.
pub fn ledger() -> Ledger {
    static LEDGER: OnceLock<Ledger> = OnceLock::new();
    LEDGER.get_or_init(Ledger::off).clone()
}

impl Ledger {
    fn with(watching: bool) -> Self {
        Self { shared: Arc::new(Mutex::new(Inner { watching, next: 1, pending: Vec::new(), answers: 0, failed: None })) }
    }

    /// A ledger that records nothing until `watch`.
    fn off() -> Self {
        Self::with(false)
    }

    /// A ledger of its own that records from the start (for tests, and for code that wants its own count).
    pub fn new() -> Self {
        Self::with(true)
    }

    /// Start recording. Writes started before this are not counted.
    pub fn watch(&self) {
        lock(&self.shared).watching = true;
    }

    /// Whether [`Ledger::sent`] records.
    pub fn watching(&self) -> bool {
        lock(&self.shared).watching
    }

    /// A write to `file` (a name to show, such as "common.toml" or "keymap.toml") has started. Answer the ticket
    /// once the app has applied the result. Not watching: an inert ticket that records nothing.
    pub fn sent(&self, file: &str) -> Ticket {
        let mut inner = lock(&self.shared);
        if !inner.watching {
            return Ticket::inert();
        }
        let seq = inner.next;
        inner.next += 1;
        inner.pending.push(Pending { seq, file: file.to_owned(), since: Instant::now() });
        Ticket { ledger: Some(self.shared.clone()), seq, file: file.to_owned(), answered: false }
    }

    /// What is in flight now, and what has failed.
    pub fn status(&self) -> Status {
        let inner = lock(&self.shared);
        Status {
            pending: inner.pending.len(),
            oldest: inner.pending.iter().min_by_key(|p| p.seq).cloned(),
            answers: inner.answers,
            failed: inner.failed.clone(),
        }
    }
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

/// One write in flight. Answer it with [`Ticket::applied`] or [`Ticket::failed`]; dropping it unanswered counts as a
/// failure. Send it to the UI with the write's result, and answer it there after applying.
#[must_use = "answer the ticket with applied() or failed() once the app has applied the write"]
#[derive(Debug)]
pub struct Ticket {
    ledger: Option<Shared>,
    seq: u64,
    file: String,
    answered: bool,
}

impl Ticket {
    /// A ticket that records nothing (for tests, and for paths that build a result without writing).
    pub fn inert() -> Self {
        Self { ledger: None, seq: 0, file: String::new(), answered: true }
    }

    /// The write's number in its ledger (from 1); `None` for an inert ticket.
    pub fn seq(&self) -> Option<u64> {
        self.ledger.as_ref().map(|_| self.seq)
    }

    /// The UI has applied the write's result.
    pub fn applied(mut self) {
        self.answer(None);
    }

    /// The write failed, or its result could not be applied.
    pub fn failed(mut self, why: impl Into<String>) {
        self.answer(Some(why.into()));
    }

    fn answer(&mut self, failure: Option<String>) {
        if self.answered {
            return;
        }
        self.answered = true;
        let Some(shared) = &self.ledger else { return };
        let mut inner = lock(shared);
        inner.pending.retain(|p| p.seq != self.seq);
        inner.answers += 1;
        if let Some(why) = failure {
            if inner.failed.is_none() {
                inner.failed = Some(Failure { seq: self.seq, file: self.file.clone(), why });
            }
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        if !self.answered {
            let why = format!("{}: write #{} was dropped without an answer", self.file, self.seq);
            self.answer(Some(why));
        }
    }
}

/// What is in flight now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// Writes sent and not answered.
    pub pending: usize,
    /// The oldest of them (smallest number).
    pub oldest: Option<Pending>,
    /// How many answers there have been (applied or failed, dropped included). Grows by 1 on each answer.
    pub answers: u64,
    /// The first write that failed since recording began. It stays.
    pub failed: Option<Failure>,
}

/// A write sent and not yet answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    /// Its number in the ledger (from 1).
    pub seq: u64,
    /// The file's name as given to [`Ledger::sent`].
    pub file: String,
    /// When it was sent.
    pub since: Instant,
}

/// A write that failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// Its number in the ledger (from 1).
    pub seq: u64,
    /// The file's name as given to [`Ledger::sent`].
    pub file: String,
    /// Why, as the app said, or that the ticket was dropped.
    pub why: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applied_clears_the_pending_write() {
        let l = Ledger::new();
        let t = l.sent("common.toml");
        let s = l.status();
        assert_eq!(s.pending, 1);
        assert_eq!(s.answers, 0);
        let o = s.oldest.unwrap();
        assert_eq!((o.seq, o.file.as_str()), (1, "common.toml"));
        t.applied();
        let s = l.status();
        assert_eq!((s.pending, s.answers), (0, 1));
        assert!(s.oldest.is_none() && s.failed.is_none());
    }

    #[test]
    fn the_first_failure_stays() {
        let l = Ledger::new();
        l.sent("common.toml").failed("disk full");
        let s = l.status();
        assert_eq!((s.pending, s.answers), (0, 1));
        assert_eq!(s.failed, Some(Failure { seq: 1, file: "common.toml".into(), why: "disk full".into() }));
        l.sent("keymap.toml").failed("later");
        let s = l.status();
        assert_eq!(s.answers, 2);
        assert_eq!(s.failed.unwrap().why, "disk full");
    }

    #[test]
    fn a_dropped_ticket_is_a_failure() {
        let l = Ledger::new();
        drop(l.sent("common.toml"));
        let s = l.status();
        assert_eq!(s.failed.unwrap().why, "common.toml: write #1 was dropped without an answer");
        assert_eq!((s.pending, s.answers), (0, 1));
    }

    #[test]
    fn answers_may_come_out_of_order() {
        let l = Ledger::new();
        let (a, b, c) = (l.sent("a"), l.sent("b"), l.sent("c"));
        b.applied();
        let s = l.status();
        assert_eq!((s.pending, s.oldest.unwrap().seq), (2, 1));
        a.applied();
        assert_eq!(l.status().oldest.unwrap().seq, 3);
        c.applied();
        assert!(l.status().oldest.is_none());
    }

    #[test]
    fn a_ledger_that_is_off_records_nothing() {
        let l = Ledger::off();
        assert!(!l.watching());
        let t = l.sent("common.toml");
        assert_eq!(t.seq(), None);
        assert_eq!(l.status().pending, 0);
        t.applied();
        drop(l.sent("common.toml"));
        let s = l.status();
        assert_eq!(s.answers, 0);
        assert!(s.failed.is_none());
        l.watch();
        assert!(l.watching());
        let t = l.sent("common.toml");
        assert_eq!(t.seq(), Some(1));
        t.applied();
    }

    #[test]
    fn an_inert_ticket_does_nothing() {
        let l = Ledger::new();
        let t = Ticket::inert();
        assert_eq!(t.seq(), None);
        drop(t);
        Ticket::inert().failed("x");
        let s = l.status();
        assert_eq!((s.answers, s.pending), (0, 0));
        assert!(s.failed.is_none());
    }

    #[test]
    fn clones_share_one_count() {
        let l = Ledger::new();
        let c = l.clone();
        let t = c.sent("common.toml");
        assert_eq!(l.status().pending, 1);
        t.applied();
        assert_eq!(l.status().answers, 1);
    }

    #[test]
    fn a_ticket_is_answered_from_another_thread() {
        let l = Ledger::new();
        let t = l.sent("common.toml");
        std::thread::spawn(move || t.applied()).join().unwrap();
        let s = l.status();
        assert_eq!((s.pending, s.answers), (0, 1));
    }

    #[test]
    fn the_types_are_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<Ledger>();
        check::<Ticket>();
        check::<Status>();
    }
}
