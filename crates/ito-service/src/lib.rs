//! Run as a Windows service that logs on as a person's own account, and manage it.
//!
//! The account is the person's, not SYSTEM, so the credential store, the state folders and the
//! `ito-ipc` door stay exactly as they are when the app runs at sign-in. [`install`] does what
//! `services.msc` does and `CreateService` alone does not: it grants the account `SeServiceLogonRight`,
//! and it sets the service's DACL so that account may start, stop and query the service without being an
//! administrator. The password is only an argument (the SCM keeps it; this crate never stores it).
//!
//! [`run`] is the service side: when the SCM started the process it reports Running, calls the app's
//! `main` with a [`Stop`] that is set on STOP, SHUTDOWN and PRESHUTDOWN, then reports Stopped with the
//! exit code. Started any other way it returns [`Error::NotService`] so the app can say so.
//!
//! Only Windows has a Service Control Manager; elsewhere every function is [`Error::Unsupported`]
//! (after the name check, so a bad name is [`Error::BadName`] everywhere).

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[cfg(windows)]
#[path = "windows.rs"]
mod sys;
#[cfg(not(windows))]
#[path = "other.rs"]
mod sys;

/// Why a service call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not Windows.
    Unsupported,
    /// The process was not started by the Service Control Manager (run from a console).
    NotService,
    /// The call needs an administrator (run elevated).
    NeedsAdmin,
    /// There is no service of that name.
    NotInstalled,
    /// A service of that name (or display name) already exists.
    AlreadyExists,
    /// The name is empty, too long, or has characters a service name cannot (`/`, `\`, controls).
    BadName,
    /// The account does not exist, or has no password (a PIN-only account cannot log on as a service).
    BadAccount,
    /// The account or password is wrong (`ERROR_SERVICE_LOGON_FAILED`, seen on the first start).
    LogonFailed,
    /// The service did not reach the state in time.
    Timeout,
    /// The system refused (the OS error number).
    Failed(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => f.write_str("Windows services exist only on Windows"),
            Error::NotService => f.write_str("not started by the Service Control Manager"),
            Error::NeedsAdmin => f.write_str("this needs an administrator (run elevated)"),
            Error::NotInstalled => f.write_str("there is no service with that name"),
            Error::AlreadyExists => f.write_str("a service with that name already exists"),
            Error::BadName => f.write_str("the service name is not valid"),
            Error::BadAccount => f.write_str("the account does not exist or has no password"),
            Error::LogonFailed => f.write_str("the account or password is wrong (the service could not log on)"),
            Error::Timeout => f.write_str("the service did not change state in time"),
            Error::Failed(n) => write!(f, "the system refused (error {n})"),
        }
    }
}

impl std::error::Error for Error {}

/// When the service starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// At boot.
    Auto,
    /// Shortly after boot, once the boot services are up.
    AutoDelayed,
    /// Only when asked.
    Manual,
}

/// What [`install`] creates.
#[derive(Clone, Debug)]
pub struct Install {
    /// The service name (`sc query <name>`).
    pub name: String,
    /// The name shown in `services.msc`.
    pub display: String,
    pub description: String,
    /// The program to run, and its arguments (quoted for you).
    pub exe: PathBuf,
    pub args: Vec<String>,
    /// `DOMAIN\user`, `.\user` or `user`.
    pub account: String,
    pub password: String,
    pub start: Start,
}

/// Where a service is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Stopped,
    Starting,
    Stopping,
}

/// A service's state and the process id (0 when stopped).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub pid: u32,
}

/// Set when the service is asked to end (STOP, SHUTDOWN, PRESHUTDOWN). Cheap to clone; an app in the
/// foreground can make its own and [`set`](Stop::set) it on Ctrl+C.
#[derive(Clone, Default)]
pub struct Stop(Arc<(Mutex<bool>, Condvar)>);

impl Stop {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self) {
        *self.0 .0.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.0 .1.notify_all();
    }

    pub fn is_set(&self) -> bool {
        *self.0 .0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sleep up to `timeout`, waking as soon as the stop is set. True when it is set.
    pub fn wait(&self, timeout: Duration) -> bool {
        let g = self.0 .0.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = self.0 .1.wait_timeout_while(g, timeout, |set| !*set).unwrap_or_else(|e| e.into_inner());
        *g
    }
}

/// True for a name `CreateService` accepts and `sc` can type: 1 to 256 characters, no `/`, `\` or controls.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().count() <= 256 && !name.chars().any(|c| c == '/' || c == '\\' || c.is_control())
}

/// The command line the SCM runs: the program and its arguments, quoted by the `CommandLineToArgvW` rules.
pub fn command_line(exe: &std::path::Path, args: &[String]) -> String {
    let mut s = quote(&exe.to_string_lossy());
    for a in args {
        s.push(' ');
        s.push_str(&quote(a));
    }
    s
}

fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// The service's DACL as SDDL: SYSTEM and administrators everything, any signed-in person read, and the
/// account (by SID string) query status, start, stop and interrogate.
pub fn access_sddl(account_sid: &str) -> String {
    format!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;CCLCSWLOCRRC;;;AU)(A;;LCRPWPLORC;;;{account_sid})")
}

/// The service side. If the SCM started this process, run `main` as the service and return when it has
/// ended; otherwise [`Error::NotService`]. A non-zero return from `main` is reported as the service's exit code.
pub fn run(name: &str, main: impl FnOnce(Stop) -> i32 + Send + 'static) -> Result<(), Error> {
    check(name)?;
    sys::run(name, Box::new(main))
}

/// Create the service. Needs an administrator. Does not start it.
pub fn install(i: &Install) -> Result<(), Error> {
    check(&i.name)?;
    if i.account.is_empty() || i.password.is_empty() {
        return Err(Error::BadAccount);
    }
    sys::install(i)
}

/// Delete the service (it goes once stopped). Needs an administrator.
pub fn uninstall(name: &str) -> Result<(), Error> {
    check(name)?;
    sys::uninstall(name)
}

/// Ask the SCM to start the service; already running is fine. Does not wait. The account the service
/// logs on as may do this without being an administrator.
pub fn start(name: &str) -> Result<(), Error> {
    check(name)?;
    sys::start(name)
}

/// Ask the service to stop and wait up to `wait` for it; already stopped is fine.
pub fn stop(name: &str, wait: Duration) -> Result<(), Error> {
    check(name)?;
    sys::stop(name, wait)
}

/// Where the service is; `None` when it is not installed.
pub fn state(name: &str) -> Result<Option<Status>, Error> {
    check(name)?;
    sys::state(name)
}

/// Read a password from the console without echoing it (piped input is read as is). The prompt goes to stderr.
pub fn read_password(prompt: &str) -> Result<String, Error> {
    sys::read_password(prompt)
}

fn check(name: &str) -> Result<(), Error> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(Error::BadName)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn names() {
        assert!(valid_name("koyomi"));
        assert!(valid_name("my service.1"));
        assert!(!valid_name(""));
        assert!(!valid_name("a\\b"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("a\nb"));
        assert!(!valid_name(&"x".repeat(257)));
        assert_eq!(state("a\\b"), Err(Error::BadName));
    }

    #[test]
    fn quoting() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("a b"), "\"a b\"");
        assert_eq!(quote("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote("C:\\Program Files\\x\\"), "\"C:\\Program Files\\x\\\\\"");
        assert_eq!(quote("a\\\"b"), "\"a\\\\\\\"b\"");
        let line =
            command_line(Path::new("C:\\Program Files\\koyomi\\koyomi.exe"), &["engine".into(), "--dir".into(), "D:\\my data".into()]);
        assert_eq!(line, "\"C:\\Program Files\\koyomi\\koyomi.exe\" engine --dir \"D:\\my data\"");
    }

    #[test]
    fn sddl_names_the_account() {
        let s = access_sddl("S-1-5-21-1-2-3-1001");
        assert!(s.starts_with("D:"));
        assert!(s.ends_with("(A;;LCRPWPLORC;;;S-1-5-21-1-2-3-1001)"));
    }

    #[test]
    fn stop_wakes_a_waiter() {
        let s = Stop::new();
        assert!(!s.is_set());
        assert!(!s.wait(Duration::from_millis(10)));
        let t = s.clone();
        let h = std::thread::spawn(move || t.wait(Duration::from_secs(10)));
        std::thread::sleep(Duration::from_millis(30));
        s.set();
        assert!(h.join().unwrap());
        assert!(s.is_set());
    }

    #[test]
    fn install_needs_an_account_and_password() {
        let mut i = Install {
            name: "x".into(),
            display: "X".into(),
            description: String::new(),
            exe: "x.exe".into(),
            args: vec![],
            account: String::new(),
            password: "p".into(),
            start: Start::AutoDelayed,
        };
        assert_eq!(install(&i), Err(Error::BadAccount));
        i.account = ".\\u".into();
        i.password.clear();
        assert_eq!(install(&i), Err(Error::BadAccount));
    }
}
