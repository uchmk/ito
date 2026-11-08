//! What a `--keys` run records about itself: `run.json`, written by the app's runner ([`Runner`](crate::Runner), or an app's
//! own runner such as kura's) into the folder of the done file. It says what ran (the runner, the ito version and rev, the
//! app, the run ID, the OS), how many steps the script had, when it started and ended, and how it ended. A run that never
//! reaches its end (a crash, a kill, a watchdog) leaves `"status": "running"`; the lane classifies that.
//!
//! The JSON is written by hand (no serde), keys in a fixed order, like [`review::Meta`](crate::review::Meta).

use std::io::Write;
use std::path::Path;

use crate::pace::{Ended, How};
use crate::review::esc;

/// The file name, in the done file's folder.
pub const FILE: &str = "run.json";
/// The version of the JSON's shape.
pub const SCHEMA: u32 = 1;
/// The label a run fails at when its `run.json` cannot be written.
pub const STEP: &str = "<run.json>";
/// The `runner` of ito's own [`Runner`](crate::Runner).
pub const RUNNER: &str = "ito_keyscript::Runner";

/// How far the run has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Done,
    Unsettled,
    Failed,
    Closed,
    Quit,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Unsettled => "unsettled",
            Self::Failed => "failed",
            Self::Closed => "closed",
            Self::Quit => "quit",
        }
    }
}

/// The ito this crate was built from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ito {
    pub version: &'static str,
    /// The commit, when the build knew it (see `build.rs`).
    pub rev: Option<&'static str>,
    /// Where `rev` came from: `env` (`ITO_BUILD_REV`), `checkout-path` (cargo's git checkout) or `unknown`.
    pub rev_source: &'static str,
}

/// The ito of this build.
pub fn ito() -> Ito {
    let rev = env!("ITO_REV");
    Ito { version: env!("CARGO_PKG_VERSION"), rev: (!rev.is_empty()).then_some(rev), rev_source: env!("ITO_REV_SOURCE") }
}

/// Unix milliseconds now.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The record of one run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    /// [`RUNNER`], or the app's own runner's name.
    pub runner: String,
    /// Empty when unknown.
    pub app: String,
    /// Empty when unknown.
    pub app_version: String,
    pub run_id: Option<String>,
    /// The number of steps in the script, all of them as parsed.
    pub steps: usize,
    /// Unix milliseconds.
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub status: Status,
    pub failed: bool,
    /// The label of the step a failed run failed at.
    pub step: Option<String>,
    pub why: Option<String>,
    /// Steps not carried out; `None` while the run goes on.
    pub left: Option<usize>,
}

impl Run {
    /// A run that starts now, with the run ID decided so far ([`current_run_id`](crate::current_run_id)).
    pub fn new(runner: &str, steps: usize) -> Self {
        Self {
            runner: runner.to_owned(),
            app: String::new(),
            app_version: String::new(),
            run_id: crate::current_run_id().map(str::to_owned),
            steps,
            started_ms: now_ms(),
            ended_ms: None,
            status: Status::Running,
            failed: false,
            step: None,
            why: None,
            left: None,
        }
    }

    /// The run ended as `e` says, at `ended_ms`.
    pub fn end(&mut self, e: &Ended, ended_ms: u64) {
        self.ended_ms = Some(ended_ms);
        self.failed = e.failed;
        self.left = Some(e.left);
        self.step = None;
        self.why = None;
        self.status = match &e.how {
            How::Done => Status::Done,
            How::Unsettled => Status::Unsettled,
            How::Failed { step, why } => {
                self.step = Some(step.clone());
                self.why = Some(why.clone());
                Status::Failed
            }
            How::Closed => Status::Closed,
            How::Quit => Status::Quit,
        };
        if e.failed && self.why.is_none() {
            let tail = e.tail.trim();
            self.why = e.say.clone().or_else(|| (!tail.is_empty()).then(|| tail.to_owned()));
        }
    }

    /// One JSON object, keys in a fixed order.
    pub fn to_json(&self) -> String {
        let text = |v: &Option<String>| v.as_deref().map_or_else(|| "null".to_owned(), esc);
        let ito = ito();
        format!(
            "{{\n  \"schema\": {},\n  \"runner\": {},\n  \"ito\": {{ \"version\": {}, \"rev\": {}, \"rev_source\": {} }},\n  \"app\": {{ \"name\": {}, \"version\": {} }},\n  \"run_id\": {},\n  \"os\": {},\n  \"arch\": {},\n  \"steps\": {},\n  \"started_ms\": {},\n  \"ended_ms\": {},\n  \"status\": {},\n  \"failed\": {},\n  \"step\": {},\n  \"why\": {},\n  \"left\": {}\n}}\n",
            SCHEMA,
            esc(&self.runner),
            esc(ito.version),
            ito.rev.map_or_else(|| "null".to_owned(), esc),
            esc(ito.rev_source),
            esc(&self.app),
            esc(&self.app_version),
            text(&self.run_id),
            esc(std::env::consts::OS),
            esc(std::env::consts::ARCH),
            self.steps,
            self.started_ms,
            self.ended_ms.map_or_else(|| "null".to_owned(), |v| v.to_string()),
            esc(self.status.as_str()),
            self.failed,
            text(&self.step),
            text(&self.why),
            self.left.map_or_else(|| "null".to_owned(), |v| v.to_string()),
        )
    }

    /// `<dir>/run.json.part`, flushed, then renamed over `<dir>/run.json`.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        let part = dir.join(format!("{FILE}.part"));
        let result = (|| {
            let mut f = std::fs::File::create(&part)?;
            f.write_all(self.to_json().as_bytes())?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&part, dir.join(FILE))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result
    }
}

/// `bytes` at `path`, durably: `<path>.part` written and flushed, then renamed over `path`. The `.part` is removed on error.
/// Public so an app that writes its own `<Snap:>` side JSON saves it the same way.
pub fn write_durable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let result = (|| {
        let mut f = std::fs::File::create(&part)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&part, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// The trace's file name, in the done file's folder.
pub const TRACE: &str = "trace.jsonl";
/// The label a run fails at when its `trace.jsonl` cannot be written.
pub const TRACE_STEP: &str = "<trace.jsonl>";

/// `trace.jsonl`: one JSON object per line, one line per script step, appended as the step closes.
#[derive(Debug)]
pub struct TraceFile {
    file: std::fs::File,
    run_id: Option<String>,
}

impl TraceFile {
    /// Creates (truncates) `<dir>/trace.jsonl`.
    pub fn create(dir: &Path, run_id: Option<&str>) -> std::io::Result<Self> {
        Ok(Self { file: std::fs::File::create(dir.join(TRACE))?, run_id: run_id.map(str::to_owned) })
    }

    /// One line as JSON (without the newline), keys in a fixed order.
    pub fn line_json(&self, l: &crate::pace::TraceLine) -> String {
        let num = |v: Option<u64>| v.map_or_else(|| "null".to_owned(), |v| v.to_string());
        format!(
            "{{\"schema_version\":{},\"run_id\":{},\"step\":{},\"label\":{},\"outcome\":{},\"settled\":{},\"frame\":{},\"time_ms\":{},\"end_frame\":{},\"end_ms\":{},\"why\":{}}}",
            SCHEMA,
            self.run_id.as_deref().map_or_else(|| "null".to_owned(), esc),
            num(l.step.map(|s| s as u64)),
            esc(&l.label),
            esc(l.outcome.name()),
            l.settled.map_or_else(|| "null".to_owned(), |b| b.to_string()),
            l.frame,
            l.time_ms,
            l.end_frame,
            l.end_ms,
            l.why.as_deref().map_or_else(|| "null".to_owned(), esc),
        )
    }

    /// Appends one line and flushes.
    pub fn write(&mut self, l: &crate::pace::TraceLine) -> std::io::Result<()> {
        let mut s = self.line_json(l);
        s.push('\n');
        self.file.write_all(s.as_bytes())?;
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_file_appends_lines() {
        use crate::pace::{Outcome, TraceLine};
        let dir = std::env::temp_dir().join(format!("ito-trace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut t = TraceFile::create(&dir, Some("r1")).unwrap();
        t.write(&TraceLine::new(Some(1), "<Wait:1>", Outcome::Done, 3, 40, None)).unwrap();
        t.write(&TraceLine::new(None, "<run.json>", Outcome::Failed, 4, 50, Some("a\"b".into()))).unwrap();
        let text = std::fs::read_to_string(dir.join(TRACE)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "{\"schema_version\":1,\"run_id\":\"r1\",\"step\":1,\"label\":\"<Wait:1>\",\"outcome\":\"done\",\"settled\":null,\"frame\":3,\"time_ms\":40,\"end_frame\":3,\"end_ms\":40,\"why\":null}"
        );
        assert!(lines[1].contains("\"step\":null") && lines[1].contains("\"outcome\":\"failed\"") && lines[1].contains("a\\\"b"));
        assert!(TraceFile::create(&dir.join("nope"), None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn run() -> Run {
        Run {
            runner: RUNNER.into(),
            app: "fude".into(),
            app_version: "0.9.0".into(),
            run_id: Some("1731050000000-1234".into()),
            steps: 12,
            started_ms: 1731050000000,
            ended_ms: None,
            status: Status::Running,
            failed: false,
            step: None,
            why: None,
            left: None,
        }
    }

    /// The ito line of the record varies with the build; the tests compare the rest.
    fn json(r: &Run) -> String {
        let j = r.to_json();
        let a = j.find("  \"ito\":").unwrap();
        let b = j[a..].find('\n').unwrap() + a + 1;
        format!("{}{}", &j[..a], &j[b..])
    }

    fn ended(how: How, failed: bool, say: Option<&str>, tail: &str, left: usize) -> Ended {
        Ended { how, tail: tail.into(), say: say.map(str::to_owned), failed, left }
    }

    fn head() -> String {
        format!(
            "{{\n  \"schema\": 1,\n  \"runner\": \"ito_keyscript::Runner\",\n  \"app\": {{ \"name\": \"fude\", \"version\": \"0.9.0\" }},\n  \"run_id\": \"1731050000000-1234\",\n  \"os\": \"{}\",\n  \"arch\": \"{}\",\n  \"steps\": 12,\n  \"started_ms\": 1731050000000,\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }

    #[test]
    fn a_running_record() {
        let want = head()
            + "  \"ended_ms\": null,\n  \"status\": \"running\",\n  \"failed\": false,\n  \"step\": null,\n  \"why\": null,\n  \"left\": null\n}\n";
        assert_eq!(json(&run()), want);
        let j = run().to_json();
        assert!(j.contains(&format!("\"ito\": {{ \"version\": \"{}\"", env!("CARGO_PKG_VERSION"))), "{j}");
        let mut none = run();
        none.run_id = None;
        assert!(none.to_json().contains("\"run_id\": null"));
    }

    #[test]
    fn each_ending() {
        let tail = |status: &str, failed: &str, step: &str, why: &str, left: usize| {
            head()
                + &format!(
                    "  \"ended_ms\": 1731050009000,\n  \"status\": \"{status}\",\n  \"failed\": {failed},\n  \"step\": {step},\n  \"why\": {why},\n  \"left\": {left}\n}}\n"
                )
        };
        let end = |e: Ended| {
            let mut r = run();
            r.end(&e, 1731050009000);
            json(&r)
        };
        assert_eq!(end(ended(How::Done, false, None, "keys: done\n", 0)), tail("done", "false", "null", "null", 0));
        assert_eq!(end(ended(How::Unsettled, false, None, "keys: done\n", 0)), tail("unsettled", "false", "null", "null", 0));
        let failed = How::Failed { step: "<Assert:x>".into(), why: "no x".into() };
        assert_eq!(
            end(ended(failed, true, Some("failed: step <Assert:x>: no x"), "", 2)),
            tail("failed", "true", "\"<Assert:x>\"", "\"no x\"", 2)
        );
        assert_eq!(
            end(ended(How::Closed, true, Some("closed before the picture"), "keys: closed\n", 3)),
            tail("closed", "true", "null", "\"closed before the picture\"", 3)
        );
        assert_eq!(
            end(ended(How::Quit, true, None, "keys: quit\nleft: 1 not pressed\n", 1)),
            tail("quit", "true", "null", "\"keys: quit\\nleft: 1 not pressed\"", 1)
        );
        assert_eq!(end(ended(How::Quit, false, None, "keys: done\n", 0)), tail("quit", "false", "null", "null", 0));
    }

    #[test]
    fn write_leaves_no_part_and_replaces() {
        let dir = std::env::temp_dir().join(format!("ito-evidence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "old").unwrap();
        let r = run();
        r.write(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), r.to_json());
        assert!(!dir.join("run.json.part").exists());
        assert!(r.write(&dir.join("nope")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The files in `docs/schemas/examples/` have the shape (keys, order, types) of what the writers write.
    #[test]
    fn the_run_and_trace_have_the_shape_of_the_examples() {
        use crate::pace::{Outcome, TraceLine};
        let dir = std::env::temp_dir().join(format!("ito-evidence-shape-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut t = TraceFile::create(&dir, Some("1731050000000-1234")).unwrap();
        let mut line = TraceLine::new(Some(2), "<Click:ok>", Outcome::Done, 10, 200, None);
        line.settled = Some(true);
        line.end_frame = 12;
        line.end_ms = 230;
        crate::shape::check("trace-done", &t.line_json(&line), crate::shape::example!("trace-done"));
        let nostep = TraceLine::new(None, "<run.json>", Outcome::Failed, 4, 50, Some("cannot write".into()));
        crate::shape::check("trace-nostep", &t.line_json(&nostep), crate::shape::example!("trace-nostep"));
        let _ = t.write(&line);
        let _ = std::fs::remove_dir_all(&dir);

        crate::shape::check("run-running", &run().to_json(), crate::shape::example!("run-running"));
        let mut r = run();
        r.end(&ended(How::Failed { step: "<Assert:x>".into(), why: "no x".into() }, true, Some("failed"), "", 2), 1731050009000);
        crate::shape::check("run-done", &r.to_json(), crate::shape::example!("run-done"));
    }
}
