//! SQLite through rusqlite.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::{Clock, ConnectKind, Error, Last, Outcome, SqliteOpen, Target};

pub(crate) fn run(target: &Target, batches: &[String], clock: &Clock, last: &mut Last, outcome: &mut Outcome) -> Result<(), Error> {
    let Target::Sqlite { path, open } = target else { unreachable!() };
    let conn = match open {
        SqliteOpen::Create => Connection::open(path),
        SqliteOpen::Existing => Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX),
        SqliteOpen::ReadOnly => Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX),
    }
    .map_err(|e| {
        let kind = match std::fs::metadata(path) {
            Err(m) if m.kind() == std::io::ErrorKind::NotFound => ConnectKind::Missing,
            _ => ConnectKind::Other,
        };
        Error::connect(kind, format!("{e}: {}", path.display()))
    })?;
    // `interrupt()` does not break SQLite's busy wait (measured: a locked file held a 1 s run for over 7 s), so the wait is
    // capped by what is left of the run's time. A stop (cancel) during a busy wait can still take up to that long.
    let left = clock.deadline.saturating_duration_since(std::time::Instant::now());
    let wait = left.min(Duration::from_secs(5));
    // When the wait is the time left, a file still busy at its end means the run's time is up. SQLite counts the wait in
    // whole milliseconds, so it can end a moment before the deadline; `clock.check()` alone would miss that.
    let busy = if left < Duration::from_secs(5) { Busy::Timeout } else { Busy::Error };
    conn.busy_timeout(wait).map_err(|e| Error::connect(ConnectKind::Other, e.to_string()))?;
    let handle = conn.get_interrupt_handle();
    let done = AtomicBool::new(false);
    std::thread::scope(|s| {
        // Looks at the stop flag and the time while SQLite is busy inside a statement.
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if clock.check().is_err() {
                    handle.interrupt();
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let r = probe(&conn, clock, busy, path).and_then(|()| go(&conn, batches, clock, busy, last, outcome));
        done.store(true, Ordering::Relaxed);
        r
    })
}

/// Reads the header (page 1; writes nothing; an empty file is an empty database) so that a file that is not a database
/// is told at the connect, in every mode, and not at the first statement.
fn probe(conn: &Connection, clock: &Clock, busy: Busy, path: &std::path::Path) -> Result<(), Error> {
    conn.pragma_query_value(None, "schema_version", |r| r.get::<_, i64>(0)).map(|_| ()).map_err(|e| {
        if let Err(stop) = clock.check() {
            return stop;
        }
        let code = match &e {
            rusqlite::Error::SqliteFailure(f, _) => Some(f.code),
            _ => None,
        };
        match code {
            Some(ErrorCode::OperationInterrupted) => Error::Cancelled,
            Some(ErrorCode::DatabaseBusy) if busy == Busy::Timeout => Error::Timeout,
            Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => {
                Error::connect(ConnectKind::NotDatabase, format!("{e}: {}", path.display()))
            }
            _ => Error::connect(ConnectKind::Other, format!("{e}: {}", path.display())),
        }
    })
}

/// What a busy file at the end of SQLite's wait means.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    /// The wait was the run's time left: the time is up.
    Timeout,
    /// The wait was the 5 s cap: an error of the statement.
    Error,
}

fn go(conn: &Connection, batches: &[String], clock: &Clock, busy: Busy, last: &mut Last, outcome: &mut Outcome) -> Result<(), Error> {
    for batch in batches {
        clock.check()?;
        step(conn, batch, last, outcome).map_err(|e| match clock.check() {
            Err(stop) => stop,
            Ok(()) => sql_error(&e, busy),
        })?;
    }
    Ok(())
}

fn step(conn: &Connection, batch: &str, last: &mut Last, outcome: &mut Outcome) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(batch)?;
    if stmt.column_count() == 0 {
        outcome.rows_affected += stmt.execute([])? as u64;
        return Ok(());
    }
    let names: Vec<String> = stmt.column_names().map(|n| n.to_string()).collect();
    last.begin(&names);
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let mut values = Vec::with_capacity(names.len());
        for i in 0..names.len() {
            values.push(text(row.get_ref(i)?));
        }
        last.row(values);
    }
    Ok(())
}

fn text(v: ValueRef) -> Option<String> {
    match v {
        ValueRef::Null => None,
        ValueRef::Integer(n) => Some(n.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Some(format!("0x{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())),
    }
}

fn sql_error(e: &rusqlite::Error, busy: Busy) -> Error {
    if let rusqlite::Error::SqliteFailure(f, _) = e {
        if f.code == ErrorCode::OperationInterrupted {
            return Error::Cancelled;
        }
        if f.code == ErrorCode::DatabaseBusy && busy == Busy::Timeout {
            return Error::Timeout;
        }
    }
    Error::Sql(e.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{check, run, ConnectKind, Error, SqliteOpen, Target};
    use std::sync::atomic::AtomicBool;

    fn db() -> (std::path::PathBuf, Target) {
        let dir = std::env::temp_dir().join(format!("ito-sql-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let _ = std::fs::remove_file(&path);
        (dir, Target::Sqlite { path, open: SqliteOpen::Create })
    }

    #[test]
    fn counts_rows_and_writes_the_last_result_set() {
        let (dir, t) = db();
        let csv = dir.join("out.csv");
        let script = "create table t(a integer, b text);\n\
                      insert into t values (1, 'x;y'), (-2, '=SUM(1)'), (3, null);\n\
                      update t set a = a + 10 where a > 0;\n\
                      select a, b from t order by a;";
        let o = run(&t, script, Some(&csv), &AtomicBool::new(false), Duration::from_secs(10)).unwrap();
        assert_eq!((o.rows_affected, o.batches, o.result_rows), (5, 4, Some(3)));
        let text = std::fs::read_to_string(&csv).unwrap();
        assert!(text.starts_with('\u{feff}'));
        assert_eq!(text.lines().count(), 4);
        assert!(text.contains("-2,\"'=SUM(1)\"") || text.contains("-2,'=SUM(1)"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn no_result_set_writes_no_file() {
        let (dir, t) = db();
        let csv = dir.join("none.csv");
        let o = run(&t, "create table t(a);", Some(&csv), &AtomicBool::new(false), Duration::from_secs(10)).unwrap();
        assert_eq!(o.result_rows, None);
        assert!(!csv.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_sql_error_is_reported_with_its_text() {
        let (dir, t) = db();
        let e = run(&t, "select * from nope", None, &AtomicBool::new(false), Duration::from_secs(10)).unwrap_err();
        assert!(matches!(&e, Error::Sql(s) if s.contains("nope")), "{e:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_stop_flag_interrupts_a_long_statement() {
        let (dir, t) = db();
        let stop = AtomicBool::new(false);
        let long = "with recursive c(x) as (select 1 union all select x + 1 from c) select count(*) from c";
        let r = std::thread::scope(|s| {
            let h = s.spawn(|| run(&t, long, None, &stop, Duration::from_secs(60)));
            std::thread::sleep(Duration::from_millis(300));
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            h.join().unwrap()
        });
        assert_eq!(r.unwrap_err(), Error::Cancelled);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_timeout_ends_a_long_statement() {
        let (dir, t) = db();
        let long = "with recursive c(x) as (select 1 union all select x + 1 from c) select count(*) from c";
        let r = run(&t, long, None, &AtomicBool::new(false), Duration::from_millis(300));
        assert_eq!(r.unwrap_err(), Error::Timeout);
        let _ = std::fs::remove_dir_all(dir);
    }

    // ---- open modes, the header probe, `check` ----

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ito-sql-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn target(path: &std::path::Path, open: SqliteOpen) -> Target {
        Target::Sqlite { path: path.to_path_buf(), open }
    }

    fn go(t: &Target, script: &str) -> Result<crate::Outcome, Error> {
        run(t, script, None, &AtomicBool::new(false), Duration::from_secs(10))
    }

    fn check_it(t: &Target) -> Result<(), Error> {
        check(t, &AtomicBool::new(false), Duration::from_secs(10))
    }

    fn is_connect(e: &Error, want: ConnectKind) -> bool {
        matches!(e, Error::Connect { kind, .. } if *kind == want) && !e.retryable()
    }

    fn made(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("t.db");
        go(&target(&path, SqliteOpen::Create), "create table t(a); insert into t values (1);").unwrap();
        path
    }

    fn stamp(path: &std::path::Path) -> (Vec<u8>, std::time::SystemTime) {
        (std::fs::read(path).unwrap(), std::fs::metadata(path).unwrap().modified().unwrap())
    }

    fn unchanged(path: &std::path::Path, before: &(Vec<u8>, std::time::SystemTime)) {
        let now = stamp(path);
        assert_eq!(now.0, before.0, "the bytes changed");
        assert!(now.1 <= before.1, "the mtime moved");
    }

    #[test]
    fn a_missing_file_is_missing_and_is_not_made() {
        let dir = folder("missing");
        let path = dir.join("nope.db");
        for open in [SqliteOpen::Existing, SqliteOpen::ReadOnly] {
            let e = go(&target(&path, open), "select 1").unwrap_err();
            assert!(is_connect(&e, ConnectKind::Missing), "{open:?}: {e:?}");
            assert!(!path.exists());
        }
        let e = check_it(&target(&path, SqliteOpen::Create)).unwrap_err();
        assert!(is_connect(&e, ConnectKind::Missing), "{e:?}");
        assert!(!path.exists());
        let deep = dir.join("no").join("such").join("t.db");
        let e = go(&target(&deep, SqliteOpen::Create), "select 1").unwrap_err();
        assert!(is_connect(&e, ConnectKind::Missing), "{e:?}");
        assert!(e.to_string().contains("t.db"), "{e}");
        assert!(!dir.join("no").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn read_only_reads_and_changes_nothing() {
        let dir = folder("readonly");
        let path = made(&dir);
        let before = stamp(&path);
        let csv = dir.join("out.csv");
        let o = run(
            &target(&path, SqliteOpen::ReadOnly),
            "select count(*) from t",
            Some(&csv),
            &AtomicBool::new(false),
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(o.result_rows, Some(1));
        assert!(std::fs::read_to_string(&csv).unwrap().contains('1'));
        check_it(&target(&path, SqliteOpen::Create)).unwrap();
        unchanged(&path, &before);
        let e = go(&target(&path, SqliteOpen::ReadOnly), "insert into t values (2)").unwrap_err();
        assert!(matches!(&e, Error::Sql(_)), "{e:?}");
        unchanged(&path, &before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn existing_writes() {
        let dir = folder("existing");
        let path = made(&dir);
        let o = go(&target(&path, SqliteOpen::Existing), "insert into t values (2)").unwrap();
        assert_eq!(o.rows_affected, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_that_is_not_a_database_is_told_at_the_connect() {
        let dir = folder("notdb");
        let path = dir.join("text.db");
        std::fs::write(&path, "this is not a database. ".repeat(43)).unwrap();
        let before = stamp(&path);
        for open in [SqliteOpen::ReadOnly, SqliteOpen::Existing, SqliteOpen::Create] {
            let e = go(&target(&path, open), "select 1").unwrap_err();
            assert!(is_connect(&e, ConnectKind::NotDatabase), "{open:?}: {e:?}");
            let e = check_it(&target(&path, open)).unwrap_err();
            assert!(is_connect(&e, ConnectKind::NotDatabase), "check {open:?}: {e:?}");
        }
        unchanged(&path, &before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_empty_file_is_an_empty_database() {
        let dir = folder("empty");
        let path = dir.join("empty.db");
        std::fs::write(&path, "").unwrap();
        let t = target(&path, SqliteOpen::ReadOnly);
        let o = go(&t, "select count(*) from sqlite_master").unwrap();
        assert_eq!(o.batches, 1);
        check_it(&t).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_wal_database_reads_read_only() {
        let dir = folder("wal");
        let path = dir.join("w.db");
        {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.query_row("pragma journal_mode=wal", [], |_| Ok(())).unwrap();
            c.execute_batch("create table t(a); insert into t values (1);").unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let csv = dir.join("out.csv");
        let o = run(&target(&path, SqliteOpen::ReadOnly), "select a from t", Some(&csv), &AtomicBool::new(false), Duration::from_secs(10))
            .unwrap();
        assert_eq!(o.result_rows, Some(1));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_timeout_is_kept_while_the_file_is_locked() {
        let dir = folder("busy");
        let path = made(&dir);
        let holder = rusqlite::Connection::open(&path).unwrap();
        holder.execute_batch("begin exclusive; insert into t values (9);").unwrap();
        let started = std::time::Instant::now();
        let r = run(&target(&path, SqliteOpen::Existing), "select count(*) from t", None, &AtomicBool::new(false), Duration::from_secs(1));
        let took = started.elapsed();
        assert_eq!(r.unwrap_err(), Error::Timeout);
        assert!(took < Duration::from_millis(2500), "took {took:?}");
        drop(holder);
        let _ = std::fs::remove_dir_all(dir);
    }
}
