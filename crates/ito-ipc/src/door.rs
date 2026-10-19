//! The door an app's window opens for its own `<app> mcp` bridge (and anything
//! else that asks the running window a question): one listener thread, one
//! request and one reply per connection, the request handed to the UI thread
//! through a channel and answered there from the state already in memory.
//!
//! ```text
//! client ──stdio── <app> mcp ──[ask]── socket / pipe ──[listen]── window (UI thread)
//! ```
//!
//! The app keeps only its `Request` type, its answers, and its tool
//! declarations (`ito-mcp`). The first window to bind holds the door and the
//! others retry every half minute, so closing that one hands it on. Nothing
//! here knows about a GUI: `wake` is whatever repaints an idle window.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::{frame, Address, Conn, Listener};

/// What the window says: the tool's text, or why it could not.
pub type Reply = Result<String, String>;

/// How long the door waits for the UI thread to take a request, and by
/// default for its reply. A window busy for longer (a modal dialog on
/// Windows) is reported, not waited for.
pub const ANSWER_WAIT: Duration = Duration::from_secs(5);

/// How often a window that lost the race for the door tries again.
const RETRY: Duration = Duration::from_secs(30);

/// One request handed to the UI thread, with where its answer goes.
pub struct Ask<Rq> {
    pub request: Rq,
    /// Send the answer here. Dropping it unanswered tells the client the
    /// window closed the request.
    pub reply: SyncSender<Reply>,
    /// Set when the client stopped waiting: a box for it that is pressed
    /// after that should do nothing.
    pub expired: Arc<AtomicBool>,
}

impl<Rq> Ask<Rq> {
    pub fn answer(self, reply: Reply) {
        let _ = self.reply.send(reply);
    }
}

/// How an app's door is set up.
pub struct Door<Rq> {
    /// The app's name, for the thread names and the messages.
    pub app: &'static str,
    /// The environment variable that names another address (see [`Address::per_user`]).
    pub env: &'static str,
    /// Bumped when `Rq` or [`Reply`] change shape, so a window and a bridge
    /// of different versions say so instead of misreading.
    pub proto: u32,
    /// Run on the connection's thread before the UI thread is asked: reject
    /// a request that is wrong on its face, or touches the disk (a path on a
    /// share that stopped answering would hold the window).
    pub check: fn(&Rq) -> Result<(), String>,
    /// How long to wait for the reply to this request (a box the user has to
    /// answer takes longer than a read from memory).
    pub wait: fn(&Rq) -> Duration,
}

impl<Rq> Door<Rq> {
    /// A door whose requests are all read from memory.
    pub const fn new(app: &'static str, env: &'static str, proto: u32) -> Self {
        Self { app, env, proto, check: |_| Ok(()), wait: |_| ANSWER_WAIT }
    }

    pub fn address(&self) -> Address {
        Address::per_user(self.app, self.env)
    }
}

impl<Rq: Serialize + DeserializeOwned + Send + 'static> Door<Rq> {
    /// Open the door on a thread of its own and hand each request to the UI
    /// thread through the returned channel; `wake` is called after each
    /// request is queued (and when one expires), to repaint an idle window.
    pub fn listen(self, wake: impl Fn() + Send + Sync + 'static) -> Receiver<Ask<Rq>> {
        let (tx, rx) = sync_channel::<Ask<Rq>>(16);
        let at = self.address();
        let door = Arc::new(self);
        let wake = Arc::new(wake);
        let name = format!("{}-mcp", door.app);
        let _ = std::thread::Builder::new().name(name.clone()).spawn(move || {
            let listener = loop {
                match Listener::bind(&at) {
                    Ok(l) => break l,
                    // Another window has it. Try again later, so the door
                    // moves on when that window closes.
                    Err(e) if e.kind() == io::ErrorKind::AddrInUse => std::thread::sleep(RETRY),
                    // The door is a convenience; the window works without it.
                    Err(_) => return,
                }
            };
            while let Ok(conn) = listener.accept() {
                let (tx, door, wake) = (tx.clone(), door.clone(), wake.clone());
                let _ = std::thread::Builder::new().name(format!("{name}-conn")).spawn(move || {
                    let _ = door.serve_one(conn, &tx, &*wake);
                });
            }
        });
        rx
    }

    /// One connection: one request, one reply. A probe that connects and goes
    /// away (another window checking whether the door is taken) ends at the read.
    fn serve_one(&self, conn: Conn, tx: &SyncSender<Ask<Rq>>, wake: &dyn Fn()) -> io::Result<()> {
        let (mut r, mut w) = conn.split()?;
        let (proto, request): (u32, Rq) = frame::read(&mut r)?;
        let reply = if proto != self.proto {
            let (app, mine) = (self.app, self.proto);
            Err(format!("this {app} window speaks version {mine} of the door and {app} mcp spoke {proto}; run the same {app} for both"))
        } else {
            self.answer(request, tx, wake)
        };
        frame::write(&mut w, &reply)
    }

    fn answer(&self, request: Rq, tx: &SyncSender<Ask<Rq>>, wake: &dyn Fn()) -> Reply {
        (self.check)(&request)?;
        let wait = (self.wait)(&request);
        let app = self.app;
        let (reply_tx, reply_rx) = sync_channel(1);
        let expired = Arc::new(AtomicBool::new(false));
        if tx.try_send(Ask { request, reply: reply_tx, expired: expired.clone() }).is_err() {
            return Err(format!("the {app} window is busy"));
        }
        wake();
        match reply_rx.recv_timeout(wait) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Disconnected) => Err(format!("the {app} window closed the request: not done")),
            Err(RecvTimeoutError::Timeout) => {
                expired.store(true, Ordering::Relaxed);
                // An idle window draws nothing, so wake it to close its box.
                wake();
                Err(format!("the {app} window did not answer in time"))
            }
        }
    }
}

/// Ask the running window, for `<app> mcp`. The error is text for the model:
/// nothing listening is "not running", which is a tool error and not a crash.
pub fn ask<Rq: Serialize>(door: &Door<Rq>, request: &Rq) -> Reply {
    let app = door.app;
    let conn = crate::connect(&door.address())
        .map_err(|_| format!("{app} is not running (or its [mcp] enable is false); start {app} and ask again"))?;
    let (mut r, mut w) = conn.split().map_err(|e| e.to_string())?;
    frame::write(&mut w, &(door.proto, request)).map_err(|e| format!("could not reach the {app} window: {e}"))?;
    frame::read::<_, Reply>(&mut r).map_err(|e| format!("the {app} window did not answer: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize)]
    enum Rq {
        Echo(String),
        Bad,
    }

    fn door(proto: u32) -> Door<Rq> {
        let mut d = Door::new("itodoor", "ITODOOR_ADDRESS", proto);
        d.check = |r| if matches!(r, Rq::Bad) { Err("bad".into()) } else { Ok(()) };
        d
    }

    #[test]
    fn asks_a_window_and_says_so_when_there_is_none() {
        let id = std::process::id();
        #[cfg(windows)]
        let at = std::path::PathBuf::from(format!(r"\\.\pipe\ito-door-{id}"));
        #[cfg(not(windows))]
        let at = {
            let dir = std::env::temp_dir().join(format!("ito-door-{id}"));
            std::fs::create_dir_all(&dir).unwrap();
            dir.join("sock")
        };
        // The only test that sets the variable: the tests share a process.
        std::env::set_var("ITODOOR_ADDRESS", at);
        let none = ask(&door(1), &Rq::Echo("x".into())).unwrap_err();
        assert!(none.contains("itodoor is not running"), "{none}");

        let rx = door(1).listen(|| {});
        std::thread::spawn(move || {
            while let Ok(a) = rx.recv() {
                if let Rq::Echo(s) = &a.request {
                    let s = format!("echo {s}");
                    a.answer(Ok(s));
                }
            }
        });
        // The listener binds on its own thread; wait for it.
        let mut got = Err(String::new());
        for _ in 0..100 {
            got = ask(&door(1), &Rq::Echo("hi".into()));
            if got.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(got.unwrap(), "echo hi");
        assert_eq!(ask(&door(1), &Rq::Bad).unwrap_err(), "bad");
        let old = ask(&door(2), &Rq::Echo("x".into())).unwrap_err();
        assert!(old.contains("version 1"), "{old}");
        std::env::remove_var("ITODOOR_ADDRESS");
    }
}
