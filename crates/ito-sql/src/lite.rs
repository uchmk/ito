//! SQLite through rusqlite.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode};

use crate::{Clock, Error, Last, Outcome, Target};

pub(crate) fn run(target: &Target, batches: &[String], clock: &Clock, last: &mut Last, outcome: &mut Outcome) -> Result<(), Error> {
    let Target::Sqlite { path } = target else { unreachable!() };
    let conn = Connection::open(path).map_err(|e| Error::Connect(e.to_string()))?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(|e| Error::Connect(e.to_string()))?;
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
        let r = go(&conn, batches, clock, last, outcome);
        done.store(true, Ordering::Relaxed);
        r
    })
}

fn go(conn: &Connection, batches: &[String], clock: &Clock, last: &mut Last, outcome: &mut Outcome) -> Result<(), Error> {
    for batch in batches {
        clock.check()?;
        step(conn, batch, last, outcome).map_err(|e| match clock.check() {
            Err(stop) => stop,
            Ok(()) => sql_error(&e),
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

fn sql_error(e: &rusqlite::Error) -> Error {
    if let rusqlite::Error::SqliteFailure(f, _) = e {
        if f.code == ErrorCode::OperationInterrupted {
            return Error::Cancelled;
        }
    }
    Error::Sql(e.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{run, Error, Target};
    use std::sync::atomic::AtomicBool;

    fn db() -> (std::path::PathBuf, Target) {
        let dir = std::env::temp_dir().join(format!("ito-sql-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let _ = std::fs::remove_file(&path);
        (dir, Target::Sqlite { path })
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
}
