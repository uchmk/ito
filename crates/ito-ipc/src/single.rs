//! One window per user: the first process to [`acquire`] owns the door, and a
//! later one hands its request (the files it was asked to open) to the owner
//! and goes away.
//!
//! ```text
//! weft +3:5 a.md ──[forward]── socket / pipe ──[listen thread]── channel ── UI thread
//! ```
//!
//! The door is its own address (`<app>-open`, see [`Address::per_user`]), not
//! the one `<app> mcp` uses, so the two never answer each other. Only this
//! user can connect (see the crate docs). The owner's thread never touches the
//! UI: it queues each message on a channel and calls `wake`, and the UI drains
//! the channel with [`Owner::try_iter`].
//!
//! * Two processes started at once: the pipe is created with
//!   `FILE_FLAG_FIRST_PIPE_INSTANCE`, and on Unix the bind runs under a lock
//!   file, so exactly one binds and the other forwards.
//! * A dead owner: a socket file nobody answers on is removed and bound over;
//!   a pipe with no server is simply not there.
//! * A hung owner: [`forward`] gives up after [`FORWARD_WAIT`] with
//!   `TimedOut` and does not take over (the owner is alive, only busy).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryIter};
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{frame, Address, Listener};

/// The longest message the door accepts. A list of files is far below this;
/// a larger length is a stranger on the pipe, not a request.
pub const MAX_MESSAGE: u32 = 1 << 20;

/// How long [`forward`] waits for the owner to connect and acknowledge.
pub const FORWARD_WAIT: Duration = Duration::from_secs(5);

/// Connections served at once; more are dropped (a flood, not a user).
const MAX_CONNS: usize = 8;

/// Whose turn it is after [`acquire`].
pub enum Outcome<M> {
    /// This process holds the door. Keep the [`Owner`] for as long as the
    /// app runs and read the others' requests from it.
    Owner(Owner<M>),
    /// The running owner has the message; exit.
    Forwarded,
}

/// The process that holds the door.
pub struct Owner<M> {
    rx: Receiver<M>,
}

impl<M> Owner<M> {
    /// The messages that arrived since the last call, without waiting. Call
    /// it from the UI thread each frame (or when `wake` ran).
    pub fn try_iter(&self) -> TryIter<'_, M> {
        self.rx.try_iter()
    }

    /// The next message, waiting up to `wait`.
    pub fn recv_timeout(&self, wait: Duration) -> Option<M> {
        self.rx.recv_timeout(wait).ok()
    }
}

/// The door of `app`, with `env` as the override of its address.
fn address(app: &str, env: &str) -> Address {
    Address::per_user(&format!("{app}-open"), env)
}

/// Become the owner, or hand `msg` to the one that is. `wake` runs on the
/// door's thread after each message is queued, to repaint an idle window.
///
/// `proto` is bumped when `M` changes shape; a request of another version is
/// refused with a message the forwarding side gets back as an error.
pub fn acquire<M>(app: &str, env: &str, proto: u32, msg: &M, wake: impl Fn() + Send + Sync + 'static) -> io::Result<Outcome<M>>
where
    M: Serialize + DeserializeOwned + Send + 'static,
{
    let at = address(app, env);
    // A few rounds: the owner may exit between our connect and our bind, or
    // a rival may win the bind in between.
    let mut last = io::Error::other("no door");
    let mut wake = Some(wake);
    for _ in 0..5 {
        match forward_to(&at, proto, msg, FORWARD_WAIT) {
            Ok(()) => return Ok(Outcome::Forwarded),
            Err(e) if matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused) => {}
            Err(e) => return Err(e),
        }
        match bind(&at) {
            Ok(listener) => {
                let wake = wake.take().expect("returns after the first bind");
                return Ok(Outcome::Owner(serve(app, listener, proto, wake)));
            }
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => last = e,
            Err(e) => return Err(e),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(last)
}

/// Hand `msg` to the owner without ever becoming one (for a tool that only
/// asks a running window). `NotFound` or `ConnectionRefused` when none runs.
pub fn forward<M: Serialize>(app: &str, env: &str, proto: u32, msg: &M) -> io::Result<()> {
    forward_to(&address(app, env), proto, msg, FORWARD_WAIT)
}

fn forward_to<M: Serialize>(at: &Address, proto: u32, msg: &M, wait: Duration) -> io::Result<()> {
    // Encode here so the worker owns plain bytes and `msg` need not be 'static.
    let bytes = frame::frame_max(&(proto, msg), MAX_MESSAGE)?;
    let at = at.clone();
    let (tx, rx) = channel();
    // Connecting and reading have no timeout of their own, so a worker does
    // them and we stop waiting; a stuck worker ends when its peer does.
    std::thread::Builder::new().name("ito-single-forward".into()).spawn(move || {
        let _ = tx.send(send_frame(&at, &bytes));
    })?;
    match rx.recv_timeout(wait) {
        Ok(r) => r,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "the running window did not answer")),
    }
}

fn send_frame(at: &Address, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let conn = crate::connect(at)?;
    let (mut r, mut w) = conn.split()?;
    w.write_all(bytes)?;
    w.flush()?;
    let reply: Result<(), String> = frame::decode(&frame::read_body_max(&mut r, 4096)?)?;
    reply.map_err(io::Error::other)
}

fn serve<M>(app: &str, listener: Listener, proto: u32, wake: impl Fn() + Send + Sync + 'static) -> Owner<M>
where
    M: DeserializeOwned + Send + 'static,
{
    let (tx, rx) = channel::<M>();
    let wake = Arc::new(wake);
    let live = Arc::new(AtomicUsize::new(0));
    let name = format!("{app}-open");
    let _ = std::thread::Builder::new().name(name.clone()).spawn(move || {
        while let Ok(conn) = listener.accept() {
            if live.fetch_add(1, Ordering::SeqCst) >= MAX_CONNS {
                live.fetch_sub(1, Ordering::SeqCst);
                continue; // dropped: the client sees the pipe close
            }
            let (tx, wake, live2) = (tx.clone(), wake.clone(), live.clone());
            let spawned = std::thread::Builder::new().name(format!("{name}-conn")).spawn(move || {
                let _ = serve_one::<M>(conn, proto, &tx, &*wake);
                live2.fetch_sub(1, Ordering::SeqCst);
            });
            if spawned.is_err() {
                // The closure (and its decrement) never ran.
                live.fetch_sub(1, Ordering::SeqCst);
            }
        }
    });
    Owner { rx }
}

fn serve_one<M: DeserializeOwned>(conn: crate::Conn, proto: u32, tx: &Sender<M>, wake: &dyn Fn()) -> io::Result<()> {
    let (mut r, mut w) = conn.split()?;
    let body = frame::read_body_max(&mut r, MAX_MESSAGE)?;
    let reply: Result<(), String> = match frame::decode::<(u32, M)>(&body) {
        Ok((p, m)) if p == proto => {
            if tx.send(m).is_ok() {
                wake();
                Ok(())
            } else {
                Err("the window is closing".into())
            }
        }
        Ok((p, _)) => Err(format!("the running window speaks version {proto} of the door and the new one {p}")),
        Err(e) => Err(format!("unreadable request: {e}")),
    };
    frame::write(&mut w, &reply)
}

#[cfg(windows)]
fn bind(at: &Address) -> io::Result<Listener> {
    // The first-instance flag makes the creation itself the lock.
    Listener::bind(at)
}

#[cfg(unix)]
fn bind(at: &Address) -> io::Result<Listener> {
    // Two processes that both find a stale socket would both remove it and
    // the second would bind over the first's. Serialise the cleanup + bind.
    // The lock is released on drop; the bound socket is the lasting claim.
    let lock_path = at.0.with_file_name("lock");
    if let Some(dir) = lock_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock_path)?;
    let mut tries = 0;
    while lock.try_lock().is_err() {
        tries += 1;
        if tries > 100 {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, "another instance is starting"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Listener::bind(at)
}

/// Where the door is, for diagnostics and tests.
pub fn door_path(app: &str, env: &str) -> PathBuf {
    address(app, env).0
}

// ---------------------------------------------------------------------------
// The usual message: "open these files".

/// One file to open, with where to put the caret (1-based, as editors say it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileArg {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub col: Option<u32>,
}

/// What a second `app FILE...` asks the running window: the files, and the
/// directory it was run from (the owner's own is somewhere else).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    pub files: Vec<FileArg>,
    pub cwd: Option<PathBuf>,
}

impl OpenRequest {
    /// Read `+LINE[:COL] FILE` argv (`weft +3:5 a.md b.md`): a `+N` or `+N:M`
    /// applies to the next file only. Relative paths are made absolute
    /// against `cwd` here, so the owner needs no guess about where they came
    /// from. Other words starting with `-` are ignored; `--` ends the
    /// options, and then everything is a file. A `+` word not in that form
    /// is a file name.
    pub fn parse<I, S>(args: I, cwd: &Path) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString>,
    {
        let mut files = Vec::new();
        let (mut pos, mut options) = (None::<(u32, Option<u32>)>, true);
        for a in args {
            let a: std::ffi::OsString = a.into();
            if let Some(s) = a.to_str().filter(|_| options) {
                if s == "--" {
                    options = false;
                    continue;
                }
                if let Some(p) = parse_pos(s) {
                    pos = Some(p);
                    continue;
                }
                if s.starts_with('-') && s.len() > 1 {
                    continue;
                }
            }
            let path = cwd.join(PathBuf::from(a)); // an absolute path replaces cwd
            let (line, col) = match pos.take() {
                Some((l, c)) => (Some(l), c),
                None => (None, None),
            };
            files.push(FileArg { path, line, col });
        }
        Self { files, cwd: Some(cwd.to_path_buf()) }
    }
}

/// `+3` or `+3:5`, both 1-based and non-zero.
fn parse_pos(s: &str) -> Option<(u32, Option<u32>)> {
    let rest = s.strip_prefix('+')?;
    let (l, c) = match rest.split_once(':') {
        Some((l, c)) => (l, Some(c)),
        None => (rest, None),
    };
    let line: u32 = l.parse().ok().filter(|&n| n > 0)?;
    let col = match c {
        Some(c) => Some(c.parse::<u32>().ok().filter(|&n| n > 0)?),
        None => None,
    };
    Some((line, col))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn unique(tag: &str) -> (String, String) {
        let n = format!("itosingle{tag}{}", std::process::id());
        let env = format!("{}_ADDR", n.to_uppercase());
        (n, env)
    }

    fn point_env(env: &str, tag: &str) {
        let id = std::process::id();
        #[cfg(windows)]
        let at = PathBuf::from(format!(r"\\.\pipe\ito-single-{tag}-{id}"));
        #[cfg(not(windows))]
        let at = {
            let dir = std::env::temp_dir().join(format!("ito-single-{tag}-{id}"));
            std::fs::create_dir_all(&dir).unwrap();
            dir.join("sock")
        };
        std::env::set_var(env, at);
    }

    #[test]
    fn first_owns_second_forwards() {
        let (app, env) = unique("a");
        point_env(&env, "a");
        let woke = Arc::new(AtomicBool::new(false));
        let w = woke.clone();
        let req = OpenRequest::parse(["+2:3", "a.md"], Path::new("/work"));
        let Outcome::Owner(owner) = acquire(&app, &env, 1, &req, move || w.store(true, Ordering::SeqCst)).unwrap() else {
            panic!("first must own");
        };
        let second = OpenRequest::parse(["b.md"], Path::new("/other"));
        assert!(matches!(acquire(&app, &env, 1, &second, || {}).unwrap(), Outcome::Forwarded));
        assert_eq!(owner.recv_timeout(Duration::from_secs(5)), Some(second.clone()));
        assert!(woke.load(Ordering::SeqCst));
        // A different version is refused with the reason.
        let e = forward(&app, &env, 2, &second).unwrap_err();
        assert!(e.to_string().contains("version 1"), "{e}");
        assert!(owner.try_iter().next().is_none());
        std::env::remove_var(&env);
    }

    #[test]
    fn forward_without_owner_says_not_found() {
        let (app, env) = unique("b");
        point_env(&env, "b");
        let e = forward(&app, &env, 1, &OpenRequest::default()).unwrap_err();
        assert!(matches!(e.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused), "{e}");
        std::env::remove_var(&env);
    }

    #[cfg(unix)]
    #[test]
    fn a_dead_owners_socket_is_replaced() {
        let (app, env) = unique("c");
        point_env(&env, "c");
        let path = door_path(&app, &env);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // A socket file with nobody behind it.
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        assert!(path.exists());
        let r = OpenRequest::default();
        assert!(matches!(acquire(&app, &env, 1, &r, || {}).unwrap(), Outcome::Owner(_)));
        std::env::remove_var(&env);
    }

    #[test]
    fn simultaneous_starts_make_one_owner() {
        let (app, env) = unique("d");
        point_env(&env, "d");
        let owners = Arc::new(AtomicUsize::new(0));
        let keep = Arc::new(std::sync::Mutex::new(Vec::new()));
        let hs: Vec<_> = (0..6)
            .map(|i| {
                let (app, env, owners, keep) = (app.clone(), env.clone(), owners.clone(), keep.clone());
                std::thread::spawn(move || {
                    let r = OpenRequest { files: vec![], cwd: Some(PathBuf::from(format!("/{i}"))) };
                    match acquire(&app, &env, 1, &r, || {}).unwrap() {
                        Outcome::Owner(o) => {
                            owners.fetch_add(1, Ordering::SeqCst);
                            keep.lock().unwrap().push(o);
                        }
                        Outcome::Forwarded => {}
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert_eq!(owners.load(Ordering::SeqCst), 1);
        std::env::remove_var(&env);
    }

    #[test]
    fn oversize_messages_are_refused_before_sending() {
        let (app, env) = unique("e");
        point_env(&env, "e");
        let big = OpenRequest { files: vec![], cwd: Some(PathBuf::from("x".repeat(2 << 20))) };
        let e = forward(&app, &env, 1, &big).unwrap_err();
        assert!(e.to_string().contains("too large"), "{e}");
        std::env::remove_var(&env);
    }

    #[test]
    fn parses_line_and_column_for_the_next_file() {
        let cwd = Path::new("/w");
        let r = OpenRequest::parse(["+3:5", "a.md", "+7", "b.md", "c.md", "-x", "+0", "+a:b"], cwd);
        let f = |p: &str, l, c| FileArg { path: cwd.join(p), line: l, col: c };
        assert_eq!(
            r.files,
            vec![f("a.md", Some(3), Some(5)), f("b.md", Some(7), None), f("c.md", None, None), f("+0", None, None), f("+a:b", None, None)]
        );
        assert_eq!(r.cwd.as_deref(), Some(cwd));
        let r = OpenRequest::parse(["--", "-odd", "+1"], cwd);
        assert_eq!(r.files, vec![f("-odd", None, None), f("+1", None, None)]);
    }
}
