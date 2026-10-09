//! A Unix domain socket in a folder only this user can enter.

use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use super::Address;

/// `$XDG_RUNTIME_DIR/<app>/sock`, the folder systemd makes for each user;
/// without one, `/tmp/<app>-<user>/sock`.
pub fn default_address(app: &str) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join(app).join("sock"),
        None => {
            let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
            std::env::temp_dir().join(format!("{app}-{user}")).join("sock")
        }
    }
}

pub struct Listener(UnixListener);

impl Listener {
    pub fn bind(at: &Address) -> io::Result<Self> {
        let path = &at.0;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        if path.exists() {
            // A server that is still there answers; a file left by one that
            // died does not, and is in the way.
            if UnixStream::connect(path).is_ok() {
                return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("another program is already listening at {}", path.display())));
            }
            std::fs::remove_file(path)?;
        }
        UnixListener::bind(path).map(Self)
    }

    pub fn accept(&self) -> io::Result<Conn> {
        self.0.accept().map(|(s, _)| Conn(s))
    }
}

pub struct Conn(UnixStream);

pub fn connect(at: &Address) -> io::Result<Conn> {
    UnixStream::connect(&at.0).map(Conn)
}

impl Conn {
    pub fn split(self) -> io::Result<(Box<dyn Read + Send>, Box<dyn Write + Send>)> {
        let w = self.0.try_clone()?;
        Ok((Box::new(self.0), Box::new(w)))
    }
}
