use crate::{note_copy, Error};
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Default)]
struct State {
    contents: Option<String>,
    last_set: Option<String>,
    writes: Vec<String>,
    noting: bool,
    fail: Option<Error>,
}

/// A clipboard in memory for tests. Clones share one clipboard, so a test can give it to workers.
#[derive(Clone, Default)]
pub struct Fake(Arc<Mutex<State>>);

thread_local! {
    static IMPLICIT: Fake = Fake::new();
    static ENTERED: RefCell<Vec<(u64, Fake)>> = const { RefCell::new(Vec::new()) };
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// Guard from [`Fake::enter`]. Not `Send`: it belongs to the thread that entered.
pub struct Entered {
    id: u64,
    _not_send: PhantomData<*const ()>,
}

impl Drop for Entered {
    fn drop(&mut self) {
        let id = self.id;
        let _ = ENTERED.try_with(|e| e.borrow_mut().retain(|(i, _)| *i != id));
    }
}

pub(crate) fn entered_top() -> Option<Fake> {
    ENTERED.with(|e| e.borrow().last().map(|(_, f)| f.clone()))
}

pub(crate) fn implicit() -> Fake {
    IMPLICIT.with(|f| f.clone())
}

impl Fake {
    pub fn new() -> Fake {
        Fake::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Successful writes also go to [`note_copy`] (off by default). Only for a test that checks `<Clipboard:>` /
    /// `<ClipboardHas:>`: the record is process-wide, so such tests must take turns, and other tests must not note.
    pub fn noting(self) -> Fake {
        self.lock().noting = true;
        self
    }

    /// While the guard lives, `clipboard!()` on this thread picks this fake, in any build (integration tests, a library
    /// crate's tests, code compiled without `cfg(test)`). Nested `enter`s stack; dropping a guard pops its own entry.
    pub fn enter(&self) -> Entered {
        let id = NEXT.with(|n| {
            n.set(n.get() + 1);
            n.get()
        });
        ENTERED.with(|e| e.borrow_mut().push((id, self.clone())));
        Entered { id, _not_send: PhantomData }
    }

    /// What this thread's `clipboard!()` would pick if it were a fake: the entered fake, else this thread's implicit one.
    pub fn this_thread() -> Fake {
        entered_top().unwrap_or_else(implicit)
    }

    /// Stage what `get_text` returns, as if another program had copied it (`None`: empty). Not an app write:
    /// `last_set` and `writes` do not change and nothing is noted.
    pub fn put(&self, text: Option<&str>) {
        self.lock().contents = text.map(str::to_owned);
    }

    /// What the fake holds now: the last `put` or successful write, whichever came later (`get_text` reads `""` as `None`).
    pub fn contents(&self) -> Option<String> {
        self.lock().contents.clone()
    }

    /// The last successful write, as given.
    pub fn last_set(&self) -> Option<String> {
        self.lock().last_set.clone()
    }

    /// Every successful write, in order, as given.
    pub fn writes(&self) -> Vec<String> {
        self.lock().writes.clone()
    }

    /// The next `set_text` (only the next) fails with `e` and changes nothing; for testing an app's error path.
    pub fn fail_next_write(&self, e: Error) {
        self.lock().fail = Some(e);
    }

    pub(crate) fn write(&self, text: &str) -> Result<(), Error> {
        let mut s = self.lock();
        if let Some(e) = s.fail.take() {
            return Err(e);
        }
        s.contents = Some(text.to_owned());
        s.last_set = Some(text.to_owned());
        s.writes.push(text.to_owned());
        if s.noting {
            // Lock order: this fake, then the record's.
            note_copy(text);
        }
        Ok(())
    }
}
