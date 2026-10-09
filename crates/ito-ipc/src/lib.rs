//! A local, per-user channel into a running app: a Unix domain socket, or a
//! named pipe on Windows. Either way the app listens at one [`Address`], each
//! [`Conn`] is a byte stream both ways, and it splits into a reader and a
//! writer that two threads use at once. [`frame`] puts serde messages on it.
//!
//! Taken out of tsumugi-mux (the tsumugi server's) so that filer and the
//! apps after it open the same kind of door, for `filer mcp` and the like
//! (docs/llm-integration.md in filer). Only this user can connect: the socket
//! sits in a 0700 folder, the pipe refuses remote clients and anyone but the
//! owner and SYSTEM.

use std::io::{self, Read, Write};
use std::path::PathBuf;

pub mod frame;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// Where the app listens: a socket file, or `\\.\pipe\<name>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address(pub PathBuf);

impl Address {
    /// This user's door into `app`: `$XDG_RUNTIME_DIR/<app>/sock` (else
    /// `/tmp/<app>-<user>/sock`), or `\\.\pipe\<app>-<user>` on Windows. The
    /// environment variable `env`, when set, overrides it (a second instance
    /// for trying something, or the tests).
    pub fn per_user(app: &str, env: &str) -> Self {
        if let Some(a) = std::env::var_os(env).filter(|a| !a.is_empty()) {
            return Self(PathBuf::from(a));
        }
        Self(imp::default_address(app))
    }
}

pub struct Listener(imp::Listener);

impl Listener {
    /// Listen at `at`. Fails with `AddrInUse` while another listener answers
    /// there, so a second instance started by accident goes away.
    pub fn bind(at: &Address) -> io::Result<Self> {
        imp::Listener::bind(at).map(Self)
    }

    /// The next client, waiting for one.
    pub fn accept(&self) -> io::Result<Conn> {
        self.0.accept().map(Conn)
    }
}

pub struct Conn(imp::Conn);

/// Connect to whatever listens at `at`; `NotFound` or `ConnectionRefused`
/// when nothing is.
pub fn connect(at: &Address) -> io::Result<Conn> {
    imp::connect(at).map(Conn)
}

impl Conn {
    /// A reader and a writer over the one connection, for two threads.
    pub fn split(self) -> io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>)> {
        self.0.split()
    }
}
