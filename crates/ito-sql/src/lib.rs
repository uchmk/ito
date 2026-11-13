//! Run a SQL script on SQL Server, PostgreSQL or SQLite with a plain blocking API: call it from a worker thread.
//!
//! - [`run`]: connect, run the script batch by batch, count the changed rows, and write the last result set as a CSV
//!   (through `ito-table`: UTF-8 with a BOM, CRLF, text that starts with `=` `+` `@` is made safe for Excel). A stop flag
//!   and a timeout end it; the connection is dropped, so the server cancels what was running.
//! - [`split_batches`]: how a script is cut. SQL Server: at a line that is only `GO`. PostgreSQL and SQLite: at `;`
//!   outside quotes, comments, `$tag$ … $tag$` (PostgreSQL) and `CREATE TRIGGER … END` (SQLite). Each batch commits by
//!   itself unless the script has its own `BEGIN` … `COMMIT`.
//! - [`check`]: "test the connection": connect and read one thing, for a settings page's button.
//! - Features `sqlserver`, `postgres`, `sqlite` (all on by default) keep a program that needs one from carrying the others.
//!
//! A connection that fails is [`Error::Connect`] with a [`ConnectKind`] (unreachable, TLS, sign-in refused, not found,
//! not a database, other), so a caller can say why and [`Error::retryable`] can tell what is worth another try.
//! PostgreSQL with [`Tls::Require`] or [`Tls::Verify`] never falls back to plain text.
//!
//! The password is given as text in [`Target`]; the caller reads it from `ito-secret` first. `Debug` hides it.
//! The drivers are `tiberius` (TDS, rustls), `tokio-postgres` (rustls) and `rusqlite` (SQLite built in). The two network
//! drivers run on a private single-thread runtime inside `run`; nothing async leaks out.
//!
//! Windows authentication for SQL Server (the signed-in account, no password) is for Windows only; elsewhere it is
//! [`Error::Unsupported`].

mod split;

#[cfg(feature = "sqlite")]
mod lite;
#[cfg(feature = "sqlserver")]
mod mssql;
#[cfg(feature = "postgres")]
mod pg;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub use split::split_batches;

/// Which SQL dialect a script is written in (it decides how [`split_batches`] cuts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    SqlServer,
    Postgres,
    Sqlite,
}

/// How to sign in to SQL Server.
#[derive(Clone, PartialEq, Eq)]
pub enum SqlAuth {
    /// The Windows account that runs this program (SSPI). Windows only.
    Windows,
    /// A SQL Server login.
    Login { user: String, password: String },
}

impl fmt::Debug for SqlAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SqlAuth::Windows => f.write_str("Windows"),
            SqlAuth::Login { user, .. } => f.debug_struct("Login").field("user", user).field("password", &"***").finish(),
        }
    }
}

/// The encryption of a PostgreSQL connection.
///
/// [`Tls::Require`] and [`Tls::Verify`] fail ([`ConnectKind::Tls`]) when the server does not encrypt; they never fall
/// back to plain text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tls {
    /// Plain text on the wire.
    Disable,
    /// Encrypted, the certificate is not checked (libpq's `sslmode=require`: for a server with its own certificate).
    Require,
    /// Encrypted, and the certificate must chain to a public root and match the host (`verify-full`).
    Verify,
}

/// How a SQLite target is opened. A path that starts with `file:` is a SQLite URI (the bundled SQLite reads it so), but
/// its `mode=` cannot ask for more than the mode here gives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SqliteOpen {
    /// Read and write; the file is made when it is not there (as before 0.295.0).
    #[default]
    Create,
    /// Read and write an existing file; a missing one is `Error::Connect { kind: Missing, .. }` and is not made.
    Existing,
    /// Read only: never made, and the database file itself is not changed. Reading still opens a read transaction, so a
    /// WAL-mode database may get its `-wal` / `-shm` side files made or touched next to it; a database left with a hot
    /// rollback journal (a crashed writer) cannot be read this way and fails (SQLite's `SQLITE_READONLY_ROLLBACK`).
    ReadOnly,
}

/// Where the script runs.
#[derive(Clone, PartialEq, Eq)]
pub enum Target {
    /// `trust_cert` accepts any server certificate (a SQL Server that made its own); otherwise the public roots decide.
    SqlServer {
        host: String,
        port: u16,
        database: String,
        auth: SqlAuth,
        trust_cert: bool,
    },
    Postgres {
        host: String,
        port: u16,
        database: String,
        user: String,
        password: String,
        tls: Tls,
    },
    /// A file on this PC; `open` says whether it is made when it is not there.
    Sqlite {
        path: PathBuf,
        open: SqliteOpen,
    },
}

impl Target {
    pub fn dialect(&self) -> Dialect {
        match self {
            Target::SqlServer { .. } => Dialect::SqlServer,
            Target::Postgres { .. } => Dialect::Postgres,
            Target::Sqlite { .. } => Dialect::Sqlite,
        }
    }
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::SqlServer { host, port, database, auth, trust_cert } => f
                .debug_struct("SqlServer")
                .field("host", host)
                .field("port", port)
                .field("database", database)
                .field("auth", auth)
                .field("trust_cert", trust_cert)
                .finish(),
            Target::Postgres { host, port, database, user, tls, .. } => f
                .debug_struct("Postgres")
                .field("host", host)
                .field("port", port)
                .field("database", database)
                .field("user", user)
                .field("password", &"***")
                .field("tls", tls)
                .finish(),
            Target::Sqlite { path, open } => f.debug_struct("Sqlite").field("path", path).field("open", open).finish(),
        }
    }
}

/// What a run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Rows the INSERT / UPDATE / DELETE statements changed, as the server counted them. On SQL Server, when an output
    /// file is asked for, a batch that has `SELECT` or `EXEC` in it is read as a stream and its count is not added.
    pub rows_affected: u64,
    /// How many batches ran.
    pub batches: usize,
    /// The rows written to the CSV, or `None` when no file was asked for or no statement gave a result set (no file is
    /// written then, so "nothing came back" differs from an empty table: `Some(0)` is a file with only the header).
    pub result_rows: Option<u64>,
}

/// Why a connection could not be made. More kinds may come; match with a `_` arm.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConnectKind {
    /// The server could not be reached: the name did not resolve, the connection was refused or reset, or it closed early.
    Unreachable,
    /// The TLS check failed: the server would not encrypt, its certificate was refused, or the handshake broke.
    Tls,
    /// The server refused the sign-in: a wrong user or password, a locked or expired login.
    SignIn,
    /// What was named is not there: the SQLite file (or its folder), or the database on the server
    /// (PostgreSQL `3D000`; SQL Server `4060`, which SQL Server also gives when the login may not open that database).
    Missing,
    /// The SQLite file is there but is not a database (or is encrypted or damaged at its header).
    NotDatabase,
    /// Anything else (e.g. too many connections, the server starting up).
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A connection could not be made: `kind` says why (unreachable, TLS, sign-in refused, not found, not a database,
    /// other) and `text` is the driver's or the server's own words.
    Connect { kind: ConnectKind, text: String },
    /// The database refused a statement. The text is the database's own.
    Sql(String),
    /// The stop flag was raised.
    Cancelled,
    /// The time ran out.
    Timeout,
    /// This target cannot be reached from this build or OS (the feature is off, or Windows authentication off Windows).
    Unsupported(String),
    /// The output file could not be written.
    Io(String),
}

impl Error {
    pub(crate) fn connect(kind: ConnectKind, text: impl Into<String>) -> Error {
        Error::Connect { kind, text: text.into() }
    }

    /// Whether the same run is worth another try later: a server that could not be reached (or another connect failure
    /// of no known kind), or the time running out. It does not mean it will succeed: `Other` also holds lasting
    /// failures (a protocol or configuration error), so a caller keeps its own cap on the tries. A refused TLS or
    /// sign-in, a missing target and a file that is not a database are never retryable (none fixes itself, and a retried
    /// wrong password can lock a login).
    pub fn retryable(&self) -> bool {
        matches!(self, Error::Connect { kind: ConnectKind::Unreachable | ConnectKind::Other, .. } | Error::Timeout)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connect { kind, text } => match kind {
                ConnectKind::Unreachable => write!(f, "cannot reach the server: {text}"),
                ConnectKind::Tls => write!(f, "TLS failed: {text}"),
                ConnectKind::SignIn => write!(f, "sign-in refused: {text}"),
                ConnectKind::Missing => write!(f, "not found: {text}"),
                ConnectKind::NotDatabase => write!(f, "not a database: {text}"),
                ConnectKind::Other => write!(f, "cannot connect: {text}"),
            },
            Error::Sql(s) => f.write_str(s),
            Error::Cancelled => f.write_str("cancelled"),
            Error::Timeout => f.write_str("timed out"),
            Error::Unsupported(s) => write!(f, "not supported: {s}"),
            Error::Io(s) => write!(f, "cannot write the result: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// Connect and read one thing: a settings page's "test the connection". Blocks; call it from a worker thread.
/// PostgreSQL and SQL Server: sign in and run `SELECT 1`. SQLite: open the file read-only whatever the target's `open` says
/// (a missing file is `Missing`, never made) and read its header. It shows the target can be reached and read, not that the
/// target's own `open` mode (writing, making the file) will work; for a WAL database it may touch the `-wal` / `-shm` files.
pub fn check(target: &Target, cancel: &AtomicBool, timeout: Duration) -> Result<(), Error> {
    let target = match target {
        Target::Sqlite { path, .. } => Target::Sqlite { path: path.clone(), open: SqliteOpen::ReadOnly },
        other => other.clone(),
    };
    run(&target, "SELECT 1", None, cancel, timeout).map(|_| ())
}

/// Run `script` on `target`. Blocks until it ends, so call it from a worker thread.
///
/// `out` is where the last result set (from the last statement that returned columns) is written as a CSV; it is
/// written whole, by name-then-rename, only when the script ends well. The result set is kept in memory until then.
/// `cancel` is looked at about ten times a second, and `timeout` covers the connect and every statement together.
/// A script with nothing in it (only blanks and comments) is an `Ok` with zero batches.
pub fn run(target: &Target, script: &str, out: Option<&Path>, cancel: &AtomicBool, timeout: Duration) -> Result<Outcome, Error> {
    let batches = split_batches(script, target.dialect());
    let clock = Clock { cancel, deadline: Instant::now() + timeout };
    clock.check()?;
    let mut last = Last::default();
    let mut outcome = Outcome::default();
    if !batches.is_empty() {
        match target {
            #[cfg(feature = "sqlserver")]
            Target::SqlServer { .. } => mssql::run(target, &batches, out.is_some(), &clock, &mut last, &mut outcome)?,
            #[cfg(not(feature = "sqlserver"))]
            Target::SqlServer { .. } => return Err(Error::Unsupported("SQL Server (feature `sqlserver` is off)".into())),
            #[cfg(feature = "postgres")]
            Target::Postgres { .. } => pg::run(target, &batches, &clock, &mut last, &mut outcome)?,
            #[cfg(not(feature = "postgres"))]
            Target::Postgres { .. } => return Err(Error::Unsupported("PostgreSQL (feature `postgres` is off)".into())),
            #[cfg(feature = "sqlite")]
            Target::Sqlite { .. } => lite::run(target, &batches, &clock, &mut last, &mut outcome)?,
            #[cfg(not(feature = "sqlite"))]
            Target::Sqlite { .. } => return Err(Error::Unsupported("SQLite (feature `sqlite` is off)".into())),
        }
    }
    outcome.batches = batches.len();
    if let (Some(path), Some(table)) = (out, last.table.take()) {
        outcome.result_rows = Some(table.len() as u64);
        table.csv(path).map_err(|e| Error::Io(e.to_string()))?;
    }
    Ok(outcome)
}

/// The stop flag and the end of the time, looked at together.
pub(crate) struct Clock<'a> {
    pub cancel: &'a AtomicBool,
    pub deadline: Instant,
}

impl Clock<'_> {
    pub fn check(&self) -> Result<(), Error> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(Error::Timeout)
        } else {
            Ok(())
        }
    }

    /// Wait until the flag is raised or the time ends; never returns otherwise.
    #[cfg(any(feature = "sqlserver", feature = "postgres"))]
    pub async fn stopped(&self) -> Error {
        loop {
            if let Err(e) = self.check() {
                return e;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Run `work` on a private single-thread runtime until it ends or the flag / time stops it (the work is dropped,
    /// which closes its connection).
    #[cfg(any(feature = "sqlserver", feature = "postgres"))]
    pub fn block_on<T>(&self, work: impl std::future::Future<Output = Result<T, Error>>) -> Result<T, Error> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::connect(ConnectKind::Other, format!("no runtime: {e}")))?;
        rt.block_on(async {
            tokio::select! {
                r = work => r,
                e = self.stopped() => Err(e),
            }
        })
    }
}

/// The last result set so far.
#[derive(Default)]
pub(crate) struct Last {
    table: Option<ito_table::Table>,
}

impl Last {
    /// A new result set starts; the one before it is dropped.
    pub fn begin(&mut self, columns: &[String]) {
        let header: Vec<&str> = columns.iter().map(String::as_str).collect();
        self.table = Some(ito_table::Table::new(&header));
    }

    /// A row of the result set. `None` is a NULL (an empty field).
    pub fn row(&mut self, values: impl IntoIterator<Item = Option<String>>) {
        if let Some(t) = self.table.as_mut() {
            t.row(values.into_iter().map(|v| cell(v.unwrap_or_default())));
        }
    }
}

/// A number stays a number (so `-5` is not made into text for Excel); anything else is text.
fn cell(text: String) -> ito_table::Cell {
    match text.parse::<f64>() {
        Ok(n) if n.is_finite() && n.to_string() == text => ito_table::Cell::Number(n),
        _ => ito_table::Cell::Text(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_passwords() {
        let t = Target::Postgres {
            host: "h".into(),
            port: 5432,
            database: "d".into(),
            user: "u".into(),
            password: "hunter2".into(),
            tls: Tls::Require,
        };
        assert!(!format!("{t:?}").contains("hunter2"));
        let a = SqlAuth::Login { user: "sa".into(), password: "hunter2".into() };
        assert!(!format!("{a:?}").contains("hunter2"));
    }

    #[test]
    fn retryable_by_kind() {
        let c = |kind| Error::connect(kind, "x");
        assert!(c(ConnectKind::Unreachable).retryable());
        assert!(c(ConnectKind::Other).retryable());
        assert!(!c(ConnectKind::Tls).retryable());
        assert!(!c(ConnectKind::SignIn).retryable());
        assert!(!c(ConnectKind::Missing).retryable());
        assert!(!c(ConnectKind::NotDatabase).retryable());
        assert!(Error::Timeout.retryable());
        assert!(!Error::Cancelled.retryable());
        assert!(!Error::Sql("x".into()).retryable());
    }

    #[test]
    fn display_by_kind() {
        let show = |kind| Error::connect(kind, "why").to_string();
        assert_eq!(show(ConnectKind::Unreachable), "cannot reach the server: why");
        assert_eq!(show(ConnectKind::Tls), "TLS failed: why");
        assert_eq!(show(ConnectKind::SignIn), "sign-in refused: why");
        assert_eq!(show(ConnectKind::Missing), "not found: why");
        assert_eq!(show(ConnectKind::NotDatabase), "not a database: why");
        assert_eq!(show(ConnectKind::Other), "cannot connect: why");
    }

    #[test]
    fn debug_of_a_sqlite_target_shows_open() {
        let t = Target::Sqlite { path: "a.db".into(), open: SqliteOpen::ReadOnly };
        assert!(format!("{t:?}").contains("open: ReadOnly"), "{t:?}");
        assert_eq!(SqliteOpen::default(), SqliteOpen::Create);
    }

    #[test]
    fn numbers_stay_numbers_and_the_rest_is_text() {
        assert_eq!(cell("-5".into()), ito_table::Cell::Number(-5.0));
        assert_eq!(cell("3.25".into()), ito_table::Cell::Number(3.25));
        assert_eq!(cell("007".into()), ito_table::Cell::Text("007".into()));
        assert_eq!(cell("1.50".into()), ito_table::Cell::Text("1.50".into()));
        assert_eq!(cell("abc".into()), ito_table::Cell::Text("abc".into()));
        assert_eq!(cell("NaN".into()), ito_table::Cell::Text("NaN".into()));
    }
}
