//! A local SQLite store for what an app keeps in quantity: a cache of threads or items, searched as the
//! person types. (Small state stays in `ito-common::kept`'s TOML files.)
//!
//! - [`Store::open`]: a file in the app's state folder, WAL, a busy timeout, and the [migrations](Store::open_at)
//!   applied in order (`PRAGMA user_version`).
//! - [`Store::write`]: runs in **one write thread**, in a transaction (data and the sync position commit together;
//!   an error rolls both back). The UI thread gets a [`Pending`] to look at later and never blocks.
//! - [`Store::read`]: on its own read connection (WAL: reads do not wait for the writer). Quick lookups only; call
//!   from a worker if the query is big.
//! - [`meta_get`] / [`meta_set`]: a key-value table inside the same database, for the sync position.
//! - [`fts_sql`] and [`parse_query`]: a full-text table kept in step with a content table, and a query built from
//!   what the person typed. The `trigram` tokenizer is what makes Japanese searchable, so a word needs 3 or more
//!   characters to match; shorter ones come back as [`Query::short`] for the caller to filter with `LIKE`.
//!
//! SQLite is compiled in (`bundled`), so it builds the same on every target and needs a C compiler.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread::JoinHandle;

pub use rusqlite;
use rusqlite::{Connection, OpenFlags, Transaction};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// An open database. Dropping it finishes the queued writes first.
pub struct Store {
    path: PathBuf,
    read: Mutex<Connection>,
    jobs: Option<Sender<Job>>,
    writer: Option<JoinHandle<()>>,
}

/// A write that may not be done yet.
pub struct Pending<T>(Receiver<Result<T, String>>);

impl<T> Pending<T> {
    /// The answer if it is there; `None` while the write thread is still at it.
    pub fn try_get(&self) -> Option<Result<T, String>> {
        match self.0.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("the store closed before the write ran".to_owned())),
        }
    }

    /// Wait for the answer (not on the UI thread).
    pub fn wait(self) -> Result<T, String> {
        self.0.recv().unwrap_or_else(|_| Err("the store closed before the write ran".to_owned()))
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn configure(c: &Connection) -> rusqlite::Result<()> {
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

impl Store {
    /// `<state folder of app>/<name>.sqlite3`, opened and migrated. See [`Store::open_at`].
    pub fn open(app: &str, name: &str, migrations: &[&str]) -> Result<Store, String> {
        let dir = ito_common::state_dir(app).ok_or("no folder to keep the database in")?;
        Store::open_at(dir.join(format!("{name}.sqlite3")), migrations)
    }

    /// Open (creating) the database at `path`. `migrations[i]` is the SQL that takes the schema from version `i` to
    /// `i + 1`; those not applied yet run in order, each in a transaction. Never remove or change one that has
    /// shipped: add a new one. A database newer than `migrations` (a later build wrote it) is refused.
    pub fn open_at(path: impl Into<PathBuf>, migrations: &[&str]) -> Result<Store, String> {
        let path = path.into();
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let ctx = |e: rusqlite::Error| format!("{}: {e}", path.display());
        let mut w = Connection::open(&path).map_err(ctx)?;
        configure(&w).map_err(ctx)?;
        // WAL is a property of the file; a read-only or network file system may say no, and then the default stays.
        let _ = w.pragma_update(None, "journal_mode", "WAL");
        w.pragma_update(None, "synchronous", "NORMAL").map_err(ctx)?;
        migrate(&mut w, migrations).map_err(ctx)?;

        let r = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(ctx)?;
        configure(&r).map_err(ctx)?;

        let (tx, rx) = mpsc::channel::<Job>();
        let writer = std::thread::Builder::new()
            .name("ito-store".into())
            .spawn(move || {
                let mut w = w;
                while let Ok(job) = rx.recv() {
                    job(&mut w);
                }
            })
            .map_err(err)?;
        Ok(Store { path, read: Mutex::new(r), jobs: Some(tx), writer: Some(writer) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run `f` in a transaction on the write thread: committed when it returns `Ok`, rolled back when it fails.
    pub fn write<T: Send + 'static>(&self, f: impl FnOnce(&Transaction) -> rusqlite::Result<T> + Send + 'static) -> Pending<T> {
        let (tx, rx) = mpsc::channel();
        let job: Job = Box::new(move |c| {
            let r = c.transaction().and_then(|t| {
                let v = f(&t)?;
                t.commit()?;
                Ok(v)
            });
            let _ = tx.send(r.map_err(err));
        });
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(job);
        }
        Pending(rx)
    }

    /// Read with `f` on the read connection. Sees what the writer has committed.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
        let c = self.read.lock().unwrap_or_else(|e| e.into_inner());
        f(&c).map_err(err)
    }

    /// One line for `<app> env` and a bug report's list of files: where the database is, how big, which schema.
    pub fn describe(&self) -> String {
        let size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        let v = self.read(|c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))).unwrap_or(-1);
        format!("{} ({} KB, schema v{v})", self.path.display(), size.div_ceil(1024))
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

fn migrate(c: &mut Connection, migrations: &[&str]) -> rusqlite::Result<()> {
    let have: usize = c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?.max(0) as usize;
    if have > migrations.len() {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_SCHEMA),
            Some(format!("the database is schema v{have}, newer than this build knows (v{})", migrations.len())),
        ));
    }
    for (i, sql) in migrations.iter().enumerate().skip(have) {
        let t = c.transaction()?;
        t.execute_batch(sql)?;
        t.pragma_update(None, "user_version", (i + 1) as i64)?;
        t.commit()?;
    }
    c.execute_batch("CREATE TABLE IF NOT EXISTS ito_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID")
}

/// A value kept with the data (the sync position, a last-run time).
pub fn meta_get(c: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    match c.query_row("SELECT value FROM ito_meta WHERE key = ?1", [key], |r| r.get(0)) {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Keep `value` under `key`. In [`Store::write`], it commits with the rows written there.
pub fn meta_set(c: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    c.execute("INSERT INTO ito_meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])?;
    Ok(())
}

fn ident(s: &str) -> Result<&str, String> {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(s)
    } else {
        Err(format!("not a plain name: {s:?}"))
    }
}

/// SQL (for a migration) that makes the full-text table `<content>_fts` over `columns` of `content`, with the
/// `trigram` tokenizer, and triggers that keep it in step with inserts, updates and deletes. `rowid` is the content
/// table's integer key. Rows already in `content` are indexed too.
pub fn fts_sql(content: &str, rowid: &str, columns: &[&str]) -> Result<String, String> {
    let (t, k) = (ident(content)?, ident(rowid)?);
    if columns.is_empty() {
        return Err("no columns to index".to_owned());
    }
    let cols = columns.iter().map(|c| ident(c)).collect::<Result<Vec<_>, _>>()?;
    let list = cols.join(", ");
    let new = cols.iter().map(|c| format!("new.{c}")).collect::<Vec<_>>().join(", ");
    let old = cols.iter().map(|c| format!("old.{c}")).collect::<Vec<_>>().join(", ");
    Ok(format!(
        "CREATE VIRTUAL TABLE {t}_fts USING fts5({list}, content='{t}', content_rowid='{k}', tokenize='trigram');\n\
         CREATE TRIGGER {t}_fts_ai AFTER INSERT ON {t} BEGIN INSERT INTO {t}_fts(rowid, {list}) VALUES (new.{k}, {new}); END;\n\
         CREATE TRIGGER {t}_fts_ad AFTER DELETE ON {t} BEGIN INSERT INTO {t}_fts({t}_fts, rowid, {list}) VALUES ('delete', old.{k}, {old}); END;\n\
         CREATE TRIGGER {t}_fts_au AFTER UPDATE ON {t} BEGIN INSERT INTO {t}_fts({t}_fts, rowid, {list}) VALUES ('delete', old.{k}, {old}); INSERT INTO {t}_fts(rowid, {list}) VALUES (new.{k}, {new}); END;\n\
         INSERT INTO {t}_fts({t}_fts) VALUES ('rebuild');"
    ))
}

/// What the person typed, made safe for FTS5.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    /// The `MATCH` text for words of 3 or more characters (each quoted, all must match); `None` when there are none.
    pub fts: Option<String>,
    /// Words shorter than the trigram can hold: filter with `LIKE '%word%'`.
    pub short: Vec<String>,
}

/// Split `input` into words. Quotes and FTS5's operators (`AND`, `NEAR`, `*`, `-`, `:`) in it are plain text.
pub fn parse_query(input: &str) -> Query {
    let mut words = Vec::new();
    let mut short = Vec::new();
    for w in input.split_whitespace() {
        if w.chars().count() >= 3 {
            words.push(format!("\"{}\"", w.replace('"', "\"\"")));
        } else {
            short.push(w.to_owned());
        }
    }
    Query { fts: (!words.is_empty()).then(|| words.join(" ")), short }
}

/// The `content`'s row ids that match `q`'s long words, best first, at most `limit`. Empty when `q` has none
/// (then the caller uses [`Query::short`]).
pub fn search_ids(c: &Connection, content: &str, q: &Query, limit: usize) -> rusqlite::Result<Vec<i64>> {
    let (Some(m), Ok(t)) = (&q.fts, ident(content)) else { return Ok(Vec::new()) };
    let mut st = c.prepare(&format!("SELECT rowid FROM {t}_fts WHERE {t}_fts MATCH ?1 ORDER BY rank LIMIT ?2"))?;
    let rows = st.query_map(rusqlite::params![m, limit as i64], |r| r.get(0))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            static N: AtomicU32 = AtomicU32::new(0);
            let d = std::env::temp_dir().join(format!("ito-store-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&d);
            Dir(d)
        }

        fn db(&self) -> PathBuf {
            self.0.join("mail.sqlite3")
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn schema() -> Vec<String> {
        vec![
            "CREATE TABLE msg (id INTEGER PRIMARY KEY, subject TEXT NOT NULL, body TEXT NOT NULL)".to_owned(),
            fts_sql("msg", "id", &["subject", "body"]).unwrap(),
        ]
    }

    fn open(d: &Dir) -> Store {
        let s = schema();
        let refs: Vec<&str> = s.iter().map(String::as_str).collect();
        Store::open_at(d.db(), &refs).unwrap()
    }

    fn add(s: &Store, id: i64, subject: &'static str, body: &'static str) {
        s.write(move |t| t.execute("INSERT INTO msg VALUES (?1, ?2, ?3)", rusqlite::params![id, subject, body]).map(|_| ()))
            .wait()
            .unwrap();
    }

    fn find(s: &Store, typed: &str) -> Vec<i64> {
        let q = parse_query(typed);
        s.read(|c| search_ids(c, "msg", &q, 10)).unwrap()
    }

    fn count(s: &Store) -> i64 {
        s.read(|c| c.query_row("SELECT count(*) FROM msg", [], |r| r.get::<_, i64>(0))).unwrap()
    }

    #[test]
    fn migrations_run_once_and_a_newer_database_is_refused() {
        let d = Dir::new();
        drop(open(&d));
        let s = open(&d);
        assert!(s.describe().contains("schema v2"), "{}", s.describe());
        drop(s);
        let err = Store::open_at(d.db(), &["CREATE TABLE x (a)"]).err().unwrap();
        assert!(err.contains("newer"), "{err}");
    }

    #[test]
    fn data_and_the_sync_position_commit_together_or_not_at_all() {
        let d = Dir::new();
        let s = open(&d);
        add(&s, 1, "hello", "world");
        let bad = s.write(|t| {
            t.execute("INSERT INTO msg VALUES (2, 'second', 'x')", [])?;
            meta_set(t, "history_id", "99")?;
            t.execute("INSERT INTO msg VALUES (2, 'dup', 'x')", []).map(|_| ())
        });
        assert!(bad.wait().is_err());
        assert_eq!(s.read(|c| meta_get(c, "history_id")).unwrap(), None);
        assert_eq!(count(&s), 1);
        s.write(|t| {
            t.execute("INSERT INTO msg VALUES (2, 'second', 'x')", [])?;
            meta_set(t, "history_id", "100")
        })
        .wait()
        .unwrap();
        assert_eq!(s.read(|c| meta_get(c, "history_id")).unwrap().as_deref(), Some("100"));
    }

    #[test]
    fn a_pending_write_can_be_looked_at_without_waiting() {
        let d = Dir::new();
        let s = open(&d);
        let p = s.write(|_| Ok(7));
        let mut got = None;
        for _ in 0..500 {
            if let Some(r) = p.try_get() {
                got = Some(r);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(got, Some(Ok(7)));
    }

    #[test]
    fn search_finds_japanese_and_stays_in_step_with_the_rows() {
        let d = Dir::new();
        let s = open(&d);
        add(&s, 1, "請求書のご確認", "来週の会議について");
        add(&s, 2, "Invoice attached", "Please see the Quarterly report");
        assert_eq!(find(&s, "請求書"), vec![1]);
        assert_eq!(find(&s, "会議について"), vec![1]);
        assert_eq!(find(&s, "quarterly REPORT"), vec![2]);
        s.write(|t| t.execute("UPDATE msg SET subject = '領収書' WHERE id = 1", []).map(|_| ())).wait().unwrap();
        assert!(find(&s, "請求書").is_empty());
        assert_eq!(find(&s, "領収書"), vec![1]);
        s.write(|t| t.execute("DELETE FROM msg WHERE id = 2", []).map(|_| ())).wait().unwrap();
        assert!(find(&s, "invoice").is_empty());
    }

    #[test]
    fn what_is_typed_is_never_fts_syntax() {
        let q = parse_query(r#"re: "a" OR NEAR( 会議 -x*"#);
        assert_eq!(q.fts.as_deref(), Some(r#""re:" """a""" "NEAR(" "-x*""#));
        assert_eq!(q.short, vec!["OR".to_owned(), "会議".to_owned()]);
        assert_eq!(parse_query("  "), Query::default());
        let d = Dir::new();
        let s = open(&d);
        add(&s, 1, "say \"hi\"", "x");
        assert_eq!(find(&s, "\"hi\""), vec![1]);
        assert!(find(&s, "AND OR NOT \" ( ) * :").is_empty());
    }

    #[test]
    fn fts_sql_refuses_names_that_are_not_plain() {
        assert!(fts_sql("msg; DROP TABLE x", "id", &["a"]).is_err());
        assert!(fts_sql("msg", "id", &[]).is_err());
        assert!(fts_sql("msg", "id", &["a b"]).is_err());
    }

    #[test]
    fn dropping_the_store_finishes_the_queued_writes() {
        let d = Dir::new();
        let s = open(&d);
        for i in 0..50 {
            let _ = s.write(move |t| t.execute("INSERT INTO msg VALUES (?1, 's', 'b')", [i]).map(|_| ()));
        }
        drop(s);
        let s = open(&d);
        assert_eq!(count(&s), 50);
    }
}
