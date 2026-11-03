//! Fake servers for the tests of an app, in the app's own `cargo test`: no Docker, no network, no setup.
//!
//! - [`smtp::SmtpServer`]: an SMTP server in three modes (plaintext, STARTTLS like port 587, implicit TLS like
//!   port 465), `AUTH PLAIN` and `AUTH LOGIN`, every accepted message kept in memory and as an `.eml` file, and
//!   switches for the failures an app must survive (a recipient refused with 550, the connection cut mid-DATA).
//! - [`http::HttpServer`]: a scripted HTTP server. A route is a method and a path; the answer is a status, headers, a
//!   body and a delay; anything else is 404. Every request is recorded.
//!
//! Every server listens on `127.0.0.1` only, on a port the system picks (`port()`), records what it received, and
//! stops (and removes its temp folder) when it is dropped. This crate is for `[dev-dependencies]` only.

pub mod http;
pub mod smtp;
pub mod tls;

use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// The accept loop of a server. Dropping it stops the loop; connections already open notice within ~100 ms.
pub(crate) struct Acceptor {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub addr: SocketAddr,
}

impl Acceptor {
    /// Listen on `127.0.0.1:0` and call `handle(stream, stop)` on a thread of its own for each connection.
    pub fn start(handle: impl Fn(TcpStream, Arc<AtomicBool>) + Send + Sync + 'static) -> io::Result<Acceptor> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let handle = Arc::new(handle);
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                        let (handle, flag) = (handle.clone(), flag.clone());
                        std::thread::spawn(move || handle(stream, flag));
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
                    Err(_) => break,
                }
            }
        });
        Ok(Acceptor { stop, thread: Some(thread), addr })
    }
}

impl Drop for Acceptor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A read that timed out (the sockets have a short timeout so a stop is noticed).
pub(crate) fn is_timeout(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

/// A short random name part.
pub(crate) fn unique() -> String {
    let mut b = [0u8; 6];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}
