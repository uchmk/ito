//! The one `--keys` execution state machine: when the next step may go in, what a retry does, how a run fails, how a
//! shot is waited for, and what the done file ends with. Window-free: it never touches an egui context, and steps go
//! through it as [`Press`] values it does not open. The host (ito's `Runner`, kura's `ScriptRun`) keeps everything that
//! touches egui (injecting events, reading the tree, taking the picture, writing the report) and asks [`Pace`] what to do
//! each frame.
//!
//! The behaviour is kura's `ScriptRun` (its `hook`, `look_again`, `fail_as` and `ended`), word for word where it speaks.
//! [`conformance`] is the table that pins it; a host proves it follows by running [`conformance::check`] with a
//! [`conformance::Subject`] of its own.
//!
//! The order [`Pace::next`] decides in:
//!
//! 1. finished, or no report and nothing left: hold;
//! 2. a frame of another viewport: hold (or fail, if an input was drawn there);
//! 3. an input in flight and a later pass: it was drawn (`action_gen` moves);
//! 4. a config write answered: `action_gen` moves;
//! 5. a config write failed: fail;
//! 6. a config write pending: hold, or fail when it is [`CONFIG_WAIT`] old;
//! 7. an input in flight for [`DELIVER_WAIT`]: fail;
//! 8. a `<Wait:N>` at the front on a settled screen: sleep out the rest;
//! 9. a shot (or a state JSON) out: hold, or fail after the deadline;
//! 10. `<Now>` at the front: spent on the next step;
//! 11. the first wait before the first step;
//! 12. ready (a continuation, a tight retry or `<Now>`, or settled two passes on), else hold;
//! 13. `late`: past the settle wait on a screen that is still moving, outside `<Now>`;
//! 14. nothing left: the run is done (or unsettled);
//! 15. a `<Wait:N>`: finished when its time is up;
//! 16. otherwise the step is taken.

use crate::{
    closed_report, failed_report, label, press, quit_report, since_action_line, Press, Progress, SnapStage, Step, DELIVER_WAIT, FRAME, GAP,
    READ_WAIT, TEXT_WAIT,
};
use ito_common::writes::Status;
use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

pub mod conformance;

/// How long a run may go on without settling before a step counts as unsettled (kura `DEFAULT_WAIT`).
pub const SETTLE_WAIT: Duration = Duration::from_secs(5);
/// How long a `<Shot:>` waits for its picture.
pub const SHOT_DEADLINE: Duration = Duration::from_secs(5);
/// How long a `<State:>` waits for its state JSON (the app's `write_state`, then the writer's acknowledgement).
pub const STATE_DEADLINE: Duration = Duration::from_secs(5);
/// How long a config write may stay unanswered.
pub const CONFIG_WAIT: Duration = Duration::from_secs(10);
/// The step name a config write failure is reported under.
pub const CONFIG_STEP: &str = "<config write>";

/// How a [`Pace`] is set up.
#[derive(Clone, Debug)]
pub struct Config {
    /// The app's name, for "{app} quit".
    pub app: String,
    /// Nothing goes in before this has passed since [`Pace::new`] (the Runner 500 ms, kura 0).
    pub first_wait: Duration,
    /// See [`SETTLE_WAIT`].
    pub settle_wait: Duration,
    /// See [`SHOT_DEADLINE`].
    pub shot_deadline: Duration,
    /// See [`STATE_DEADLINE`].
    pub state_deadline: Duration,
    /// Whether the run writes a report. Without one an empty queue just holds (kura with no done file).
    pub report: bool,
}

impl Config {
    /// The usual setup for `app`: no first wait, the default waits, a report.
    pub fn new(app: &str) -> Self {
        Self {
            app: app.to_owned(),
            first_wait: Duration::ZERO,
            settle_wait: SETTLE_WAIT,
            shot_deadline: SHOT_DEADLINE,
            state_deadline: STATE_DEADLINE,
            report: true,
        }
    }
}

/// What kind of step an entry is, which decides how it is paced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Puts input into the window.
    Input,
    /// The rest of an input the host split in several frames: needs a new pass and a frame's time, not a settled screen.
    Then,
    /// `<Wait:N>`.
    Wait(Duration),
    /// `<Now>`.
    Now,
    /// A check of the tree or the last copy that looks again each frame until [`TEXT_WAIT`].
    Check,
    /// Finds a widget and then acts on it; looks again until [`TEXT_WAIT`] (not every frame).
    Look,
    /// `<ReadHas:>`: reads again each frame until [`READ_WAIT`].
    Poll,
    /// Writes a report (`<State:>`, the app's readings).
    Record,
    /// `<Shot:>`.
    Shot,
    /// `<Quit>`.
    Quit,
}

/// The kind of a [`Press`]. Never [`Kind::Then`]: a continuation comes from the host's [`More::Then`].
pub fn kind(p: &Press) -> Kind {
    match p {
        Press::Wait(d) => Kind::Wait(*d),
        Press::Now => Kind::Now,
        Press::Shot(_) | Press::Snap { stage: SnapStage::Before, .. } => Kind::Shot,
        Press::Quit => Kind::Quit,
        Press::State(_) | Press::Read { .. } => Kind::Record,
        // The later stages of a snap are queued by Pace itself (as `Kind::Then`), never parsed.
        Press::Snap { .. } => Kind::Then,
        Press::ReadHas { .. } => Kind::Poll,
        Press::Assert { .. }
        | Press::Is { .. }
        | Press::IdCheck(_)
        | Press::Announced(_)
        | Press::Clipboard(_)
        | Press::ClipboardHas(_) => Kind::Check,
        Press::ClickText { .. } | Press::DClickText(_) | Press::ClickId(_) | Press::Tree(_) => Kind::Look,
        Press::Events(_)
        | Press::Paste
        | Press::PasteText(_)
        | Press::Click { .. }
        | Press::DClick { .. }
        | Press::Hover { .. }
        | Press::Wheel { .. }
        | Press::Drag { .. }
        | Press::Size { .. }
        | Press::Theme(_)
        | Press::Lang(_) => Kind::Input,
    }
}

/// Whether a step that ran counts as "went in", and so can be named as unsettled.
fn records(kind: Kind) -> bool {
    matches!(kind, Kind::Input | Kind::Look | Kind::Record | Kind::Shot | Kind::Quit)
}

/// A step the host is to carry out.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The step's index in the script (a continuation keeps its step's).
    pub idx: usize,
    pub press: Press,
    pub kind: Kind,
}

/// What the host sees now, for one call of [`Pace::next`].
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    /// The main window's latest pass number; never decreases.
    pub pass: u64,
    pub now: Instant,
    /// `None` for the main window, `Some(format!("{id:?}"))` for another viewport.
    pub viewport: Option<&'a str>,
    /// Whether the screen has settled; `None` falls back to "the last action was at least 60 ms ago".
    pub settled: Option<bool>,
    /// Config writes in flight, if the app keeps a ledger.
    pub writes: Option<&'a Status>,
    /// The tree generation the host sees now: what a reading must be newer than ([`fresh`]).
    pub generation: u64,
}

/// What [`Pace::next`] decided.
#[derive(Clone, Debug, PartialEq)]
pub enum Next {
    /// Nothing this frame. Ask again after `wake` (`Some(ZERO)` is the next frame); `None` needs no frame.
    Hold { wake: Option<Duration> },
    /// Carry this out, then answer with [`Pace::taken`].
    Take(Entry),
    /// The run is over (it failed or is done).
    End(Ended),
}

/// More work an input leaves, put in front of the queue under the same step.
#[derive(Clone, Debug, PartialEq)]
pub enum More {
    /// An ordinary step (a drag's later events, the wait after a resize).
    Step(Press),
    /// A continuation: goes in a frame or more after the last one, without waiting for the screen to settle.
    Then(Press),
}

/// How the host answers a [`Next::Take`].
#[derive(Clone, Debug, PartialEq)]
pub enum Taken {
    /// Input went in this frame. `flight`: it must be drawn in a later pass, or the run fails.
    Input { flight: bool, more: Vec<More> },
    /// Done, nothing in flight (a report written, a check passed, a click found nothing to wait for).
    Done,
    /// The window was changed without events going in (a resize, a theme, a language). Like `Input { flight: true, .. }`,
    /// it must be drawn in a later pass, but a frame of another viewport at the same pass does not fail it.
    Changed { more: Vec<More> },
    /// A check, look or poll did not find what it looks for yet; `why` is the failure if it never does.
    Again { why: String },
    /// A picture was asked for; it is to be saved at `path`.
    Shot { path: String },
    /// The app was asked to write the state JSON of `<State:name>`; the host reports the outcome with
    /// [`Pace::state_saved`] / [`Pace::state_failed`].
    State { name: String },
    /// The app was told to quit.
    Quit,
    /// The step failed.
    Failed(String),
}

/// How the window ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// Closed from outside.
    Closed,
    /// The app quit by itself.
    Quit,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq)]
pub enum How {
    Done,
    /// Done, but some step went in on a screen that had not settled.
    Unsettled,
    Failed {
        step: String,
        why: String,
    },
    Closed,
    Quit,
}

/// The end of a run: what the host writes, says and shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Ended {
    pub how: How,
    /// The last lines of the done file (after the host's state report), ending in a newline.
    pub tail: String,
    /// The line for standard error, if any.
    pub say: Option<String>,
    /// The run has failed (sticky: the host's process flag).
    pub failed: bool,
    /// The number of steps not carried out that went into `tail`: the `left` of a failure, `0` for a run that reached
    /// its end ([`How::Done`], [`How::Unsettled`]), the `left` of [`Pace::end`] for [`How::Closed`] and [`How::Quit`].
    /// A failure clears the queue, so [`Pace::left`] is `0` afterwards; a host that writes its own report uses this.
    pub left: usize,
}

#[derive(Debug)]
struct Flight {
    pass: u64,
    at: Instant,
    idx: usize,
    /// Whether the input put events into the window. A change without events (a resize, a theme, a language) is not
    /// lost when a frame of another viewport meets it at the same pass.
    events: bool,
}

#[derive(Debug)]
struct StateOut {
    idx: usize,
    name: String,
    since: Instant,
    /// The next stage of a `<Snap:>`, queued when this one is saved.
    then: Option<Press>,
    /// `Some((name, stage))` while the state belongs to a `<Snap:>`.
    snap: Option<(String, SnapStage)>,
}

#[derive(Debug)]
struct ShotOut {
    idx: usize,
    name: String,
    path: String,
    since: Instant,
    then: Option<Press>,
    snap: Option<(String, SnapStage)>,
}

/// How a trace line ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The step's work completed.
    Done,
    /// The step failed (or a failure belongs to it).
    Failed,
    /// The run ended while the step was running (the window closed, the app quit).
    Cut,
}

impl Outcome {
    /// `"done"`, `"failed"` or `"cut"`.
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Done => "done",
            Outcome::Failed => "failed",
            Outcome::Cut => "cut",
        }
    }
}

/// One line of `trace.jsonl`: a script step that ran.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TraceLine {
    /// 1-based step number; `None` for a failure that belongs to no step (`<run.json>`, a config write, ...).
    pub step: Option<usize>,
    pub label: String,
    pub outcome: Outcome,
    /// The `settled` value when the step was first handed out; `None` for `<Wait:>`, `<Now>` and the no-step line.
    pub settled: Option<bool>,
    /// The pass when the step started.
    pub frame: u64,
    /// Milliseconds since the `now` given to [`Pace::new`], when the step started.
    pub time_ms: u64,
    pub end_frame: u64,
    pub end_ms: u64,
    pub why: Option<String>,
}

impl TraceLine {
    /// A line that starts and ends at `frame` / `time_ms` (a host's own no-step failure, for one).
    pub fn new(step: Option<usize>, label: &str, outcome: Outcome, frame: u64, time_ms: u64, why: Option<String>) -> Self {
        Self { step, label: label.to_owned(), outcome, settled: None, frame, time_ms, end_frame: frame, end_ms: time_ms, why }
    }
}

#[derive(Debug)]
struct OpenLine {
    idx: usize,
    settled: Option<bool>,
    frame: u64,
    ms: u64,
}

/// The state machine. See the module documentation.
#[derive(Debug)]
pub struct Pace {
    config: Config,
    queue: VecDeque<Entry>,
    labels: Vec<String>,
    /// The one entry the host is carrying out.
    take: Option<Entry>,
    /// Whether that entry went in on an unsettled screen.
    late_take: bool,
    /// The pass and time of the last step that went in.
    at: (u64, Instant),
    start: Instant,
    begun: bool,
    now_flag: bool,
    flight: Option<Flight>,
    shot: Option<ShotOut>,
    state: Option<StateOut>,
    check_since: Option<Instant>,
    read_has_since: Option<Instant>,
    seen_answers: u64,
    action_gen: u64,
    acted: Option<Instant>,
    acted_idx: Option<usize>,
    progress_at: Instant,
    unsettled: Vec<usize>,
    unsettled_end: bool,
    failed: bool,
    finished: bool,
    /// The `settled` of the take that is out.
    take_settled: Option<bool>,
    open: Option<OpenLine>,
    trace: Vec<TraceLine>,
    /// The last pass and time Pace was told about.
    last: (u64, Instant),
}

fn hold(wake: Option<Duration>) -> Next {
    Next::Hold { wake }
}

impl Pace {
    /// A run of `steps`, started at `now`. Steps no keyboard can type are skipped; the rest keep their index.
    pub fn new(config: Config, steps: &[Step], now: Instant) -> Self {
        let queue = steps.iter().enumerate().filter_map(|(idx, s)| press(s).map(|p| Entry { idx, kind: kind(&p), press: p })).collect();
        Self {
            config,
            queue,
            labels: steps.iter().map(label).collect(),
            take: None,
            late_take: false,
            at: (0, now),
            start: now,
            begun: false,
            now_flag: false,
            flight: None,
            shot: None,
            state: None,
            check_since: None,
            read_has_since: None,
            seen_answers: 0,
            action_gen: 0,
            acted: None,
            acted_idx: None,
            progress_at: now,
            unsettled: Vec::new(),
            unsettled_end: false,
            failed: false,
            finished: false,
            take_settled: None,
            open: None,
            trace: Vec::new(),
            last: (0, now),
        }
    }

    fn ms(&self, now: Instant) -> u64 {
        u64::try_from(now.saturating_duration_since(self.start).as_millis()).unwrap_or(u64::MAX)
    }

    fn trace_open(&mut self, idx: usize, settled: Option<bool>, frame: u64, now: Instant) {
        if self.open.as_ref().is_some_and(|o| o.idx == idx) {
            return;
        }
        self.trace_close(Outcome::Done, None, frame, now);
        self.open = Some(OpenLine { idx, settled, frame, ms: self.ms(now) });
    }

    fn trace_close(&mut self, outcome: Outcome, why: Option<String>, frame: u64, now: Instant) {
        let Some(o) = self.open.take() else { return };
        let line = TraceLine {
            step: Some(o.idx + 1),
            label: self.label(o.idx),
            outcome,
            settled: o.settled,
            frame: o.frame,
            time_ms: o.ms,
            end_frame: frame,
            end_ms: self.ms(now),
            why,
        };
        self.trace.push(line);
    }

    /// Whether step `idx` still has work to do: a slot, a queued piece, a flight, or a take.
    fn outstanding(&self, idx: usize) -> bool {
        self.shot.as_ref().is_some_and(|s| s.idx == idx)
            || self.state.as_ref().is_some_and(|s| s.idx == idx)
            || self.queue.front().is_some_and(|e| e.idx == idx)
            || self.flight.as_ref().is_some_and(|f| f.idx == idx)
            || self.take.as_ref().is_some_and(|e| e.idx == idx)
    }

    /// Closes the open line `Done` when its step has nothing left to do.
    fn trace_settle(&mut self, frame: u64, now: Instant) {
        if let Some(idx) = self.open.as_ref().map(|o| o.idx) {
            if !self.outstanding(idx) {
                self.trace_close(Outcome::Done, None, frame, now);
            }
        }
    }

    fn trace_cut(&mut self) {
        let (frame, now) = self.last;
        self.trace_close(Outcome::Cut, None, frame, now);
    }

    fn trace_fail(&mut self, idx: Option<usize>, label: &str, why: &str) {
        let (frame, now) = self.last;
        match idx {
            Some(i) if self.open.as_ref().is_some_and(|o| o.idx == i) => {
                self.trace_close(Outcome::Failed, Some(why.to_owned()), frame, now)
            }
            _ => {
                self.trace_close(Outcome::Cut, None, frame, now);
                let ms = self.ms(now);
                self.trace.push(TraceLine::new(idx.map(|i| i + 1), label, Outcome::Failed, frame, ms, Some(why.to_owned())));
            }
        }
    }

    /// The trace lines closed since the last call, in order.
    pub fn take_trace(&mut self) -> Vec<TraceLine> {
        std::mem::take(&mut self.trace)
    }

    /// A `Failed` line that belongs to no step, stamped with the last pass and time Pace saw (a host's own failure
    /// after the end, which [`Pace::host_failed`] cannot report any more).
    pub fn no_step_line(&self, label: &str, why: &str) -> TraceLine {
        TraceLine::new(None, label, Outcome::Failed, self.last.0, self.ms(self.last.1), Some(why.to_owned()))
    }

    /// While a [`Next::Take`] is out: the `settled` [`Pace::next`] saw when it handed the step out.
    pub fn take_settled(&self) -> Option<bool> {
        self.take.as_ref().and(self.take_settled)
    }

    fn note_frame(&mut self, pass: u64, now: Instant) {
        self.last = (pass.max(self.last.0), now);
    }

    fn label(&self, idx: usize) -> String {
        self.labels.get(idx).cloned().unwrap_or_else(|| format!("step {}", idx + 1))
    }

    /// Distinct steps not carried out: the queue, the one being carried out, the one whose picture is awaited, `extra`.
    fn left_with(&self, extra: Option<usize>) -> usize {
        let steps: BTreeSet<usize> = self
            .queue
            .iter()
            .map(|e| e.idx)
            .chain(self.take.as_ref().map(|e| e.idx))
            .chain(self.shot.as_ref().map(|s| s.idx))
            .chain(self.state.as_ref().map(|s| s.idx))
            .chain(extra)
            .collect();
        steps.len()
    }

    /// Steps not carried out yet (distinct step indices, the one being carried out included).
    pub fn left(&self) -> usize {
        self.left_with(None)
    }

    fn fail_at(&mut self, idx: usize, why: String) -> Ended {
        let label = self.label(idx);
        let left = self.left_with(Some(idx));
        self.fail_core(Some(idx), &label, left, why)
    }

    fn fail_as(&mut self, label: &str, left: usize, why: String) -> Ended {
        self.fail_core(None, label, left, why)
    }

    fn fail_core(&mut self, idx: Option<usize>, label: &str, left: usize, why: String) -> Ended {
        self.trace_fail(idx, label, &why);
        self.failed = true;
        self.queue.clear();
        self.shot = None;
        self.state = None;
        self.take = None;
        self.flight = None;
        // Nothing waits any more, so `due` stops asking for frames.
        self.check_since = None;
        self.read_has_since = None;
        self.finished = true;
        Ended {
            how: How::Failed { step: label.to_owned(), why: why.clone() },
            tail: failed_report(label, &why, left),
            say: Some(format!("failed: step {label}: {why}")),
            failed: true,
            left,
        }
    }

    fn record_unsettled(&mut self, idx: usize) {
        self.failed = true;
        if !self.unsettled.contains(&idx) {
            self.unsettled.push(idx);
        }
    }

    fn unsettled_text(&self) -> Option<String> {
        let labels: Vec<String> = (0..self.labels.len()).map(|i| self.label(i)).collect();
        unsettled_lines(&self.unsettled, &labels, self.unsettled_end, self.config.settle_wait)
    }

    /// What to do in this frame. While a [`Next::Take`] is out, answer it with [`Pace::taken`] before asking again.
    pub fn next(&mut self, f: &Frame) -> Next {
        if self.take.is_some() {
            debug_assert!(false, "Pace::next called while a take is outstanding");
            return hold(None);
        }
        self.note_frame(f.pass, f.now);
        if self.finished || (!self.config.report && self.queue.is_empty() && self.shot.is_none() && self.state.is_none()) {
            return hold(None);
        }
        // Another window's frame is not the main window's: nothing goes in on it. An input waiting to be drawn that
        // meets one at the same pass went to a window that was not drawn.
        if let Some(id) = f.viewport {
            if let Some(fl) = self.flight.as_ref().filter(|fl| fl.events && fl.pass == f.pass) {
                let idx = fl.idx;
                let why = format!("the input went to viewport {id} while the main window was not drawn");
                return Next::End(self.fail_at(idx, why));
            }
            return hold(None);
        }
        if self.flight.as_ref().is_some_and(|fl| f.pass > fl.pass) {
            self.flight = None;
            self.action_gen = f.generation;
            self.progress_at = f.now;
            self.trace_settle(f.pass, f.now);
        }
        // A config write that failed, or has not been answered, holds the script: what it reads next would be the old
        // config.
        if let Some(w) = f.writes {
            if w.answers != self.seen_answers {
                self.seen_answers = w.answers;
                // A reading after `<Theme:>` / `<Lang:>` needs a tree drawn after the new config was applied.
                self.action_gen = f.generation;
                self.progress_at = f.now;
            }
            if let Some(failure) = &w.failed {
                let left = self.left_with(None);
                return Next::End(self.fail_as(CONFIG_STEP, left, failure.why.clone()));
            }
            if w.pending > 0 {
                if let Some(o) = &w.oldest {
                    if o.since.checked_add(CONFIG_WAIT).is_some_and(|t| t <= f.now) {
                        let why =
                            format!("no answer from the config writer within {} s of write #{} ({})", CONFIG_WAIT.as_secs(), o.seq, o.file);
                        let left = self.left_with(None);
                        return Next::End(self.fail_as(CONFIG_STEP, left, why));
                    }
                }
                return hold(Some(Duration::ZERO));
            }
        }
        if let Some(fl) = &self.flight {
            if f.now.saturating_duration_since(fl.at) >= DELIVER_WAIT {
                let idx = fl.idx;
                let why =
                    format!("the input was not drawn within {} s (the window may be hidden, minimized or covered)", DELIVER_WAIT.as_secs());
                return Next::End(self.fail_at(idx, why));
            }
        }
        let since = f.now.saturating_duration_since(self.at.1);
        let settled = f.settled.unwrap_or(since >= GAP);
        // Inside a `<Wait:N>` once the step before it has been drawn and has settled: sleep out the rest instead of
        // drawing frames that change nothing.
        if let Some(Kind::Wait(d)) = self.queue.front().map(|e| e.kind) {
            let left = d.saturating_sub(since);
            if self.shot.is_none() && self.state.is_none() {
                if let Some(idx) = self.queue.front().map(|e| e.idx) {
                    self.trace_open(idx, None, f.pass, f.now);
                }
            }
            if !left.is_zero() && self.shot.is_none() && self.state.is_none() && f.pass >= self.at.0.saturating_add(2) && settled {
                return hold(Some(left));
            }
        }
        // A picture being taken holds the next step until it is saved. One that never comes is a missing file.
        if let Some(shot) = &self.shot {
            if f.now.saturating_duration_since(shot.since) <= self.config.shot_deadline {
                return hold(Some(Duration::ZERO));
            }
            let idx = shot.idx;
            let secs = self.config.shot_deadline.as_secs();
            let why = match &shot.snap {
                Some((a, _)) => format!("shot: no picture of <Snap:{a}> arrived within {secs} s; {} was not saved", shot.path),
                None => format!("no picture arrived within {secs} s; {} was not saved", shot.path),
            };
            return Next::End(self.fail_at(idx, why));
        }
        // The state JSON being written holds the next step the same way. One that never comes is a harness failure.
        if let Some(state) = &self.state {
            if f.now.saturating_duration_since(state.since) <= self.config.state_deadline {
                return hold(Some(Duration::ZERO));
            }
            let idx = state.idx;
            let secs = self.config.state_deadline.as_secs();
            let why = match &state.snap {
                Some((a, stage)) => format!("{}: no state JSON for <Snap:{a}> within {secs} s", stage.name()),
                None => format!("no state JSON for <State:{}> within {secs} s", state.name),
            };
            return Next::End(self.fail_at(idx, why));
        }
        // `<Now>` is for the step after it only. Several in a row are spent on the same step.
        while self.queue.front().is_some_and(|e| e.kind == Kind::Now) {
            if let Some(e) = self.queue.pop_front() {
                self.trace_open(e.idx, None, f.pass, f.now);
                self.trace_close(Outcome::Done, None, f.pass, f.now);
            }
            self.now_flag = true;
        }
        if !self.begun {
            if let Some(until) = self.start.checked_add(self.config.first_wait) {
                if f.now < until {
                    return hold(Some(until.saturating_duration_since(f.now)));
                }
            }
        }
        let front = self.queue.front().map(|e| e.kind);
        let ready = if front == Some(Kind::Then) {
            // The rest of an input: a new pass and a frame's time, not a settled screen.
            let new_pass = f.pass > self.at.0;
            if !(new_pass && since >= FRAME) {
                return hold(Some(if new_pass { FRAME.saturating_sub(since) } else { Duration::ZERO }));
            }
            true
        } else if self.now_flag || self.read_has_since.is_some() || (self.check_since.is_some() && front == Some(Kind::Check)) {
            // `<Now>`: one pass after the last step is enough. A `<ReadHas:>` that has started looking, and a check that
            // has, read again each pass, settled or not.
            f.pass > self.at.0
        } else {
            f.pass >= self.at.0.saturating_add(2) && (settled || since > self.config.settle_wait)
        };
        if !ready {
            return hold(Some(Duration::ZERO));
        }
        // Past the settle wait on a screen that is still moving, outside `<Now>`.
        let late = !self.now_flag && !settled;
        let Some(entry) = self.queue.pop_front() else {
            return Next::End(self.finish(late));
        };
        if let Kind::Wait(d) = entry.kind {
            // Counted from the step before it: the next step goes in at least this long after that one.
            if d.saturating_sub(since).is_zero() {
                self.at = (f.pass, f.now);
                self.progress_at = f.now;
                self.begun = true;
                self.trace_open(entry.idx, None, f.pass, f.now);
                self.trace_settle(f.pass, f.now);
            } else {
                self.trace_open(entry.idx, None, f.pass, f.now);
                self.queue.push_front(entry);
            }
            return hold(Some(Duration::ZERO));
        }
        self.late_take = late && entry.kind != Kind::Then;
        self.trace_open(entry.idx, Some(settled), f.pass, f.now);
        self.take_settled = Some(settled);
        self.take = Some(entry.clone());
        Next::Take(entry)
    }

    fn finish(&mut self, late: bool) -> Ended {
        self.trace_cut();
        self.finished = true;
        self.unsettled_end = late;
        let lines = self.unsettled_text();
        if lines.is_some() {
            self.failed = true;
        }
        Ended {
            how: if lines.is_some() { How::Unsettled } else { How::Done },
            tail: lines.clone().unwrap_or_else(|| "keys: done\n".to_owned()),
            say: lines.map(|l| l.lines().collect::<Vec<_>>().join("; ")),
            failed: self.failed,
            left: 0,
        }
    }

    /// An action's first input goes in now (call it right before the side effects of an input step, and of a look once
    /// it found its target). `true` the first time for a step: the host takes its mark then (what the app copied and
    /// announced from here on is what `<Announced:>` looks at).
    pub fn act(&mut self, now: Instant) -> bool {
        let Some(entry) = &self.take else {
            debug_assert!(false, "Pace::act called with no take outstanding");
            return false;
        };
        if self.acted_idx == Some(entry.idx) {
            return false;
        }
        self.acted = Some(now);
        self.acted_idx = Some(entry.idx);
        true
    }

    /// The host's answer to the [`Next::Take`]. `Some` when the run failed on it.
    pub fn taken(&mut self, f: &Frame, t: Taken) -> Option<Ended> {
        let Some(entry) = self.take.take() else {
            debug_assert!(false, "Pace::taken called with no take outstanding");
            return None;
        };
        let idx = entry.idx;
        self.note_frame(f.pass, f.now);
        self.take_settled = None;
        if let Press::Snap { name, stage } = &entry.press {
            self.check_since = None;
            self.read_has_since = None;
            if let Err(why) = self.snap_taken(f, idx, name, *stage, t) {
                return Some(self.fail_at(idx, why));
            }
            // The first stage is the step that waited for the screen; it records a screen that never settled.
            if *stage == SnapStage::Before && self.late_take {
                self.record_unsettled(idx);
            }
            self.now_flag = false;
            self.progress_at = f.now;
            self.begun = true;
            self.trace_settle(f.pass, f.now);
            return None;
        }
        if let Taken::Again { why } = t {
            // Nothing was pressed: no `at`, no progress, not unsettled.
            let (clock, limit) = match entry.kind {
                Kind::Poll => (&mut self.read_has_since, READ_WAIT),
                Kind::Check | Kind::Look => (&mut self.check_since, TEXT_WAIT),
                _ => {
                    debug_assert!(false, "Taken::Again for a step that cannot look again");
                    return Some(self.fail_at(idx, why));
                }
            };
            let since = *clock.get_or_insert(f.now);
            if f.now.saturating_duration_since(since) < limit {
                self.queue.push_front(entry);
                return None;
            }
            return Some(self.fail_at(idx, why));
        }
        self.check_since = None;
        self.read_has_since = None;
        match t {
            Taken::Input { flight, more } => self.put_in(f, idx, flight, true, more),
            Taken::Changed { more } => self.put_in(f, idx, true, false, more),
            Taken::Done | Taken::Quit => self.at = (f.pass, f.now),
            Taken::Shot { path } => {
                let name = match &entry.press {
                    Press::Shot(name) => name.clone(),
                    _ => String::new(),
                };
                self.shot = Some(ShotOut { idx, name, path, since: f.now, then: None, snap: None });
                self.at = (f.pass, f.now);
            }
            Taken::State { name } => {
                self.state = Some(StateOut { idx, name, since: f.now, then: None, snap: None });
                self.at = (f.pass, f.now);
            }
            Taken::Failed(why) => return Some(self.fail_at(idx, why)),
            Taken::Again { .. } => unreachable!("handled above"),
        }
        if records(entry.kind) && self.late_take {
            self.record_unsettled(idx);
        }
        self.now_flag = false;
        self.progress_at = f.now;
        self.begun = true;
        self.trace_settle(f.pass, f.now);
        None
    }

    /// The host's answer to one stage of a `<Snap:>`. `Err` is the failure's reason.
    fn snap_taken(&mut self, f: &Frame, idx: usize, a: &str, stage: SnapStage, t: Taken) -> Result<(), String> {
        let what = match &t {
            Taken::Failed(why) => return Err(format!("{}: <Snap:{a}>: {why}", stage.name())),
            Taken::Again { why } => {
                return Err(format!("{}: <Snap:{a}>: the host asked to look again, which a snap cannot do ({why})", stage.name()))
            }
            Taken::State { name } => format!("State {name:?}"),
            Taken::Input { .. } => "Input".to_owned(),
            Taken::Done => "Done".to_owned(),
            Taken::Changed { .. } => "Changed".to_owned(),
            Taken::Shot { .. } => "Shot".to_owned(),
            Taken::Quit => "Quit".to_owned(),
        };
        let snap = Some((a.to_owned(), stage));
        match (stage, t) {
            (SnapStage::Before, Taken::State { name }) if name == format!("{a}-before") => {
                let then = Some(Press::Snap { name: a.to_owned(), stage: SnapStage::Shot });
                self.state = Some(StateOut { idx, name, since: f.now, then, snap });
            }
            (SnapStage::Shot, Taken::Shot { path }) => {
                let then = Some(Press::Snap { name: a.to_owned(), stage: SnapStage::After });
                self.shot = Some(ShotOut { idx, name: a.to_owned(), path, since: f.now, then, snap });
            }
            (SnapStage::After, Taken::State { name }) if name == format!("{a}-after") => {
                self.state = Some(StateOut { idx, name, since: f.now, then: None, snap });
            }
            _ => return Err(format!("{}: the host answered {what} for <Snap:{a}>", stage.name())),
        }
        self.at = (f.pass, f.now);
        Ok(())
    }

    fn put_in(&mut self, f: &Frame, idx: usize, flight: bool, events: bool, more: Vec<More>) {
        self.at = (f.pass, f.now);
        if flight {
            self.flight = Some(Flight { pass: f.pass, at: f.now, idx, events });
        }
        for m in more.into_iter().rev() {
            let e = match m {
                More::Step(p) => Entry { idx, kind: kind(&p), press: p },
                More::Then(p) => Entry { idx, kind: Kind::Then, press: p },
            };
            self.queue.push_front(e);
        }
    }

    /// The picture was saved.
    pub fn shot_saved(&mut self, now: Instant) {
        let Some(out) = self.shot.take() else { return };
        self.progress_at = now;
        self.last.1 = now;
        if let Some(press) = out.then {
            self.queue.push_front(Entry { idx: out.idx, kind: Kind::Then, press });
        }
        self.trace_settle(self.last.0, now);
    }

    /// The picture could not be saved. `None` when no picture is awaited (a late callback).
    pub fn shot_failed(&mut self, why: String) -> Option<Ended> {
        let out = self.shot.as_ref()?;
        let idx = out.idx;
        let why = match &out.snap {
            Some((a, stage)) => format!("{}: <Snap:{a}>: {why}", stage.name()),
            None => why,
        };
        Some(self.fail_at(idx, why))
    }

    /// The state JSON was written and acknowledged.
    pub fn state_saved(&mut self, now: Instant) {
        let Some(out) = self.state.take() else { return };
        self.progress_at = now;
        self.last.1 = now;
        if let Some(press) = out.then {
            self.queue.push_front(Entry { idx: out.idx, kind: Kind::Then, press });
        }
        self.trace_settle(self.last.0, now);
    }

    /// The state JSON could not be written. `None` when none is awaited (a late acknowledgement).
    pub fn state_failed(&mut self, why: &str) -> Option<Ended> {
        let out = self.state.as_ref()?;
        let idx = out.idx;
        let why = match &out.snap {
            Some((a, stage)) => format!("{}: <Snap:{a}>: {why}", stage.name()),
            None => why.to_owned(),
        };
        Some(self.fail_at(idx, why))
    }

    /// The host failed outside the steps (it could not write its evidence): the run fails at `label`. `None` once the run has ended.
    pub fn host_failed(&mut self, label: &str, why: String) -> Option<Ended> {
        if self.finished {
            return None;
        }
        let left = self.left_with(None);
        Some(self.fail_as(label, left, why))
    }

    /// The window closed or the app quit. `None` when there is nothing to write (no report, or the run has ended).
    pub fn end(&mut self, how: Ending, now: Instant, writes: Option<&Status>) -> Option<Ended> {
        if self.finished {
            return None;
        }
        self.last.1 = now;
        let what = match how {
            Ending::Closed => "the window was closed".to_owned(),
            Ending::Quit => format!("{} quit", self.config.app),
        };
        if let Some(shot) = &self.shot {
            let why = match &shot.snap {
                Some((a, _)) => format!("shot: {what} before the picture of <Snap:{a}> was saved"),
                None => format!("{what} before the picture of <Shot:{}> was saved", shot.name),
            };
            let idx = shot.idx;
            return Some(self.fail_at(idx, why));
        }
        if let Some(state) = &self.state {
            let why = match &state.snap {
                Some((a, stage)) => format!("{}: {what} before the state JSON of <Snap:{a}> was written", stage.name()),
                None => format!("{what} before the state JSON of <State:{}> was written", state.name),
            };
            let idx = state.idx;
            return Some(self.fail_at(idx, why));
        }
        // A stage was saved and the next one is queued, not taken yet.
        if self.take.is_none() {
            if let Some(Entry { idx, press: Press::Snap { name, stage }, .. }) = self.queue.front() {
                let (idx, why) = (*idx, format!("{}: {what} before this stage of <Snap:{name}> ran", stage.name()));
                return Some(self.fail_at(idx, why));
            }
        }
        if !self.config.report {
            self.trace_cut();
            self.finished = true;
            self.queue.clear();
            self.take = None;
            self.flight = None;
            self.check_since = None;
            self.read_has_since = None;
            return None;
        }
        if let Some(failure) = writes.and_then(|w| w.failed.as_ref()) {
            let left = self.left_with(None);
            return Some(self.fail_as(CONFIG_STEP, left, failure.why.clone()));
        }
        if let Some(w) = writes.filter(|w| w.pending > 0) {
            let file = w.oldest.as_ref().map_or("config", |o| o.file.as_str());
            let why = format!("{what} before the config writer answered ({file})");
            let left = self.left_with(None);
            return Some(self.fail_as(CONFIG_STEP, left, why));
        }
        let left = self.left_with(None);
        let lines = self.unsettled_text();
        let base = match how {
            Ending::Closed => closed_report(left),
            Ending::Quit => quit_report(left),
        };
        // A `keys: done` ending gives way to the unsettled lines. A `keys: closed` / `keys: quit` ending (with the `left:`
        // line after it) stays at the end, since it says what was left; the unsettled lines go before it.
        let tail = match (&lines, base.strip_suffix("keys: done\n")) {
            (Some(lines), Some(head)) => format!("{head}{lines}"),
            (Some(lines), None) => format!("{}{base}", lines.strip_suffix("keys: unsettled\n").unwrap_or(lines)),
            (None, _) => base,
        };
        if lines.is_some() {
            self.failed = true;
        }
        self.trace_cut();
        self.finished = true;
        Some(Ended {
            how: match how {
                Ending::Closed => How::Closed,
                Ending::Quit => How::Quit,
            },
            tail,
            say: lines.map(|l| l.lines().collect::<Vec<_>>().join("; ")),
            failed: self.failed,
            left,
        })
    }

    /// The host could not write the report: the run failed.
    pub fn write_failed(&mut self) {
        self.failed = true;
    }

    /// The run failed (sticky): the host's process flag.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// The run has ended.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// The tree generation a reading must be newer than: it moves when an input was drawn or a config write answered.
    pub fn action_gen(&self) -> u64 {
        self.action_gen
    }

    /// An input or a change is out and has not been drawn yet.
    pub fn in_flight(&self) -> bool {
        self.flight.is_some()
    }

    /// How long ago the last action began, for the `<State:>` line.
    pub fn since_action(&self, now: Instant) -> Option<Duration> {
        self.acted.map(|t| now.saturating_duration_since(t))
    }

    /// The step the last action belongs to.
    pub fn acted_idx(&self) -> Option<usize> {
        self.acted_idx
    }

    /// The steps that went in on a screen that had not settled, in order.
    pub fn unsettled(&self) -> &[usize] {
        &self.unsettled
    }

    /// When the next frame is wanted for a retry that waits: a repaint asked for while drawing can be forgotten when
    /// the pass begins, so a quiet window could stop drawing while a check waits.
    pub fn due(&self) -> Option<Duration> {
        (self.check_since.is_some() || self.read_has_since.is_some()).then_some(Duration::from_millis(16))
    }

    /// How far the run has got, for the watchdog.
    pub fn progress(&self) -> Progress {
        Progress {
            left: self.left_with(None),
            at: self.progress_at,
            due: match self.queue.front().map(|e| e.kind) {
                Some(Kind::Wait(d)) => d,
                _ => Duration::ZERO,
            },
            finished: self.finished,
        }
    }
}

/// The window as a reading step may judge it: seen after the last input was drawn, in the main window, and not
/// minimized. `obs_generation` is the tree's generation, `action_gen` is [`Pace::action_gen`].
pub fn fresh(obs_generation: u64, action_gen: u64, root: bool, minimized: Option<bool>) -> Result<(), String> {
    if obs_generation > action_gen && root {
        if minimized == Some(true) {
            Err("the window is minimized".into())
        } else {
            Ok(())
        }
    } else {
        Err("the window's tree was not updated after the last action (the window may be hidden, minimized or covered)".into())
    }
}

/// Why `<ReadHas:>` failed: what `got` (the reading, if there was one) held in place of `text`.
pub fn read_has_why(tag: &str, text: &str, got: Option<&str>) -> String {
    let got = got.unwrap_or("");
    let shown: String = got.chars().take(200).collect();
    let more = if got.chars().count() > 200 { "..." } else { "" };
    format!("{tag} did not contain {text:?} within {} s; it read {shown:?}{more}", READ_WAIT.as_secs())
}

/// The lines `<State:>` adds to the host's own: `settled`, `config writes pending`, `config write failed` and
/// `since last action`, each ending in a newline. A `settled` of `None` reads `unknown`.
pub fn state_lines(settled: Option<bool>, writes: Option<&Status>, since_action: Option<Duration>) -> String {
    let settled = match settled {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    let pending = if writes.is_some_and(|w| w.pending > 0) { "yes" } else { "no" };
    let failed = writes.and_then(|w| w.failed.as_ref()).map_or("none", |f| f.why.as_str());
    format!("settled: {settled}\nconfig writes pending: {pending}\nconfig write failed: {failed}\n") + &since_action_line(since_action)
}

/// The last lines of the report when anything went in unsettled, in place of `keys: done`. `unsettled` are step
/// indices into `labels`, `end` is a script that ended on a screen still moving. `None` when there is nothing to say.
pub fn unsettled_lines(unsettled: &[usize], labels: &[String], end: bool, settle_wait: Duration) -> Option<String> {
    if unsettled.is_empty() && !end {
        return None;
    }
    let secs = settle_wait.as_secs();
    let mut out = String::new();
    if !unsettled.is_empty() {
        let named: Vec<String> = unsettled.iter().map(|i| labels.get(*i).cloned().unwrap_or_else(|| format!("step {}", i + 1))).collect();
        out +=
            &format!("unsettled: {} of {} steps went in after {secs} s without settling: {}\n", named.len(), labels.len(), named.join(" "));
    }
    if end {
        out += &format!("unsettled: the script ended after {secs} s without settling\n");
    }
    out += "keys: unsettled\n";
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, parse_with, Dialect};
    use ito_common::writes::{Failure, Pending};

    #[test]
    fn the_conformance_table_holds() {
        if let Err(e) = conformance::check(&mut conformance::PaceSubject, &conformance::cases()) {
            panic!("{e}");
        }
    }

    #[test]
    fn there_are_enough_cases_with_distinct_names() {
        let cases = conformance::cases();
        assert!(cases.len() >= 20, "{} cases", cases.len());
        let names: BTreeSet<&str> = cases.iter().map(|c| c.name).collect();
        assert_eq!(names.len(), cases.len(), "a name is used twice");
    }

    #[test]
    fn a_check_that_disagrees_is_reported() {
        struct Wrong;
        impl conformance::Subject for Wrong {
            fn run(&mut self, case: &conformance::Case) -> conformance::Trace {
                let mut t = case.expect.clone();
                t.left += 1;
                t
            }
        }
        let err = conformance::check(&mut Wrong, &conformance::cases()).unwrap_err();
        assert!(err.contains("left"), "{err}");
    }

    #[test]
    fn kinds_follow_kura() {
        let steps = parse_with(
            "a<Paste><Click:Save><DClick:Save><Assert:x><Clipboard:x><Tree:x><Wait:5><Now><Shot:s><State:s><Quit><ReadHas:Pane=x><Size:480x600><Theme:dark><Lang:en>",
            &Dialect { reads: &[("Pane", "pane")], ..Dialect::default() },
        )
        .unwrap();
        let kinds: Vec<Kind> = steps.iter().map(|s| kind(&press(s).unwrap())).collect();
        use Kind::*;
        let want = [
            Input,
            Input,
            Look,
            Look,
            Check,
            Check,
            Look,
            Wait(Duration::from_millis(5)),
            Now,
            Shot,
            Record,
            Quit,
            Poll,
            Input,
            Input,
            Input,
        ];
        assert_eq!(kinds, want);
        assert_ne!(kind(&Press::Events(Vec::new())), Kind::Then);
    }

    #[test]
    fn fresh_wants_a_newer_tree_in_the_main_window() {
        assert_eq!(fresh(5, 4, true, None), Ok(()));
        assert_eq!(fresh(5, 4, true, Some(false)), Ok(()));
        let stale = "the window's tree was not updated after the last action (the window may be hidden, minimized or covered)";
        assert_eq!(fresh(4, 4, true, None), Err(stale.to_owned()), "strictly newer");
        assert_eq!(fresh(9, 4, false, None), Err(stale.to_owned()), "another viewport");
        assert_eq!(fresh(5, 4, true, Some(true)), Err("the window is minimized".to_owned()));
        // As kura does it: a stale tree is the first thing said, even for a minimized window.
        assert_eq!(fresh(4, 4, true, Some(true)), Err(stale.to_owned()));
    }

    #[test]
    fn read_has_says_what_it_read() {
        assert_eq!(read_has_why("Pane", "ok", Some("abc")), "Pane did not contain \"ok\" within 15 s; it read \"abc\"");
        assert_eq!(read_has_why("Pane", "ok", None), "Pane did not contain \"ok\" within 15 s; it read \"\"");
        let long = "x".repeat(250);
        let why = read_has_why("Pane", "ok", Some(&long));
        assert!(why.ends_with(&format!("it read \"{}\"...", "x".repeat(200))), "{why}");
        let exact = "y".repeat(200);
        assert!(!read_has_why("Pane", "ok", Some(&exact)).ends_with("..."));
    }

    #[test]
    fn state_lines_say_the_three_facts_and_the_age() {
        let none = state_lines(Some(true), None, None);
        assert_eq!(none, "settled: yes\nconfig writes pending: no\nconfig write failed: none\nsince last action: none\n");
        let base = Instant::now();
        let status = Status {
            pending: 1,
            oldest: Some(Pending { seq: 1, file: "common.toml".into(), since: base }),
            answers: 0,
            failed: Some(Failure { seq: 1, file: "common.toml".into(), why: "disk full".into() }),
        };
        let some = state_lines(Some(false), Some(&status), Some(Duration::from_millis(153)));
        assert_eq!(some, "settled: no\nconfig writes pending: yes\nconfig write failed: disk full\nsince last action: 153 ms\n");
        assert!(state_lines(None, None, None).starts_with("settled: unknown\n"));
    }

    #[test]
    fn unsettled_lines_name_the_steps_and_the_end() {
        let labels: Vec<String> = ["a", "b", "<Shot:s>"].iter().map(|s| s.to_string()).collect();
        let five = Duration::from_secs(5);
        assert_eq!(unsettled_lines(&[], &labels, false, five), None);
        assert_eq!(
            unsettled_lines(&[0, 2], &labels, false, five).unwrap(),
            "unsettled: 2 of 3 steps went in after 5 s without settling: a <Shot:s>\nkeys: unsettled\n"
        );
        assert_eq!(
            unsettled_lines(&[], &labels, true, five).unwrap(),
            "unsettled: the script ended after 5 s without settling\nkeys: unsettled\n"
        );
        let both = unsettled_lines(&[1], &labels, true, Duration::from_secs(2)).unwrap();
        assert_eq!(
            both,
            "unsettled: 1 of 3 steps went in after 2 s without settling: b\nunsettled: the script ended after 2 s without settling\nkeys: unsettled\n"
        );
        assert!(unsettled_lines(&[7], &labels, false, five).unwrap().contains("step 8"), "a label that is missing");
    }

    fn frame(pass: u64, now: Instant) -> Frame<'static> {
        Frame { pass, now, viewport: None, settled: Some(true), writes: None, generation: 0 }
    }

    #[test]
    fn time_never_panics() {
        let base = Instant::now();
        let steps = parse("a<Wait:10>b").unwrap();
        let mut cfg = Config::new("app");
        cfg.first_wait = Duration::MAX;
        let mut pace = Pace::new(cfg, &steps, base);
        // A frame before the start, a first wait that overflows, a huge pass: all quiet.
        let early = base.checked_sub(Duration::from_secs(1)).unwrap_or(base);
        assert!(matches!(pace.next(&frame(u64::MAX, early)), Next::Take(_) | Next::Hold { .. }));
    }

    #[test]
    fn calling_taken_without_a_take_is_quiet_in_release() {
        let base = Instant::now();
        let steps = parse("a").unwrap();
        let mut pace = Pace::new(Config::new("app"), &steps, base);
        if !cfg!(debug_assertions) {
            assert_eq!(pace.taken(&frame(2, base), Taken::Done), None);
        }
        assert!(!pace.failed() && !pace.finished());
        assert_eq!(pace.due(), None);
        let p = pace.progress();
        assert_eq!((p.left, p.finished, p.due), (1, false, Duration::ZERO));
    }

    #[test]
    fn a_check_that_looks_again_does_not_move_the_watch() {
        let base = Instant::now();
        let at = |ms: u64| base + Duration::from_millis(ms);
        let steps = parse("<Assert:ok>").unwrap();
        let mut pace = Pace::new(Config::new("app"), &steps, base);
        for (pass, ms) in [(2, 100), (3, 200)] {
            let f = frame(pass, at(ms));
            assert!(matches!(pace.next(&f), Next::Take(_)), "pass {pass}");
            assert_eq!(pace.taken(&f, Taken::Again { why: "not yet".into() }), None);
        }
        assert_eq!(pace.progress().at, base, "a look that finds nothing is not progress");
        assert!(pace.due().is_some(), "a check that waits asks for frames");
        let f = frame(4, at(300));
        assert!(matches!(pace.next(&f), Next::Take(_)));
        assert_eq!(pace.taken(&f, Taken::Done), None);
        assert_eq!(pace.progress().at, at(300));
        assert_eq!(pace.due(), None);
    }

    #[test]
    fn the_watch_sees_a_wait_as_due() {
        let base = Instant::now();
        let steps = parse("a<Wait:5000>b").unwrap();
        let mut pace = Pace::new(Config::new("app"), &steps, base);
        let f = frame(2, base);
        assert!(matches!(pace.next(&f), Next::Take(_)));
        assert_eq!(pace.taken(&f, Taken::Input { flight: true, more: Vec::new() }), None);
        // Drawn and settled: the rest of the wait is slept out, and the watch allows for it.
        let f = frame(4, base + Duration::from_millis(100));
        assert!(matches!(pace.next(&f), Next::Hold { wake: Some(d) } if d == Duration::from_millis(4900)));
        assert_eq!(pace.progress().due, Duration::from_secs(5));
    }

    #[test]
    fn act_is_true_once_per_step() {
        let base = Instant::now();
        let steps = parse("ab").unwrap();
        let mut pace = Pace::new(Config::new("app"), &steps, base);
        let f = frame(2, base);
        assert!(matches!(pace.next(&f), Next::Take(_)));
        assert!(pace.act(base));
        assert!(!pace.act(base));
        assert_eq!(pace.acted_idx(), Some(0));
        assert_eq!(pace.since_action(base + Duration::from_millis(5)), Some(Duration::from_millis(5)));
        assert_eq!(pace.taken(&f, Taken::Input { flight: false, more: vec![] }), None);
    }

    #[test]
    fn the_repaint_asked_for_a_retry_is_16_ms() {
        let base = Instant::now();
        let steps = parse("<Assert:x>").unwrap();
        let mut pace = Pace::new(Config::new("app"), &steps, base);
        let f = frame(2, base);
        assert!(matches!(pace.next(&f), Next::Take(_)));
        assert_eq!(pace.taken(&f, Taken::Again { why: "no".into() }), None);
        assert_eq!(pace.due(), Some(Duration::from_millis(16)));
    }

    /// Run `script` on a Pace the way a host would: 25 ms per pass, every answer the plain one, every shot and state saved
    /// one pass later. Stops at the end of the run or after `max` passes. Returns the Pace, the start and the end.
    fn drive(script: &str, cfg: Config, max: u64, answer: &dyn Fn(&Entry) -> Taken) -> (Pace, Instant, Option<Ended>) {
        let base = Instant::now();
        let steps = parse_with(script, &conformance::dialect()).unwrap();
        let mut pace = Pace::new(cfg, &steps, base);
        for pass in 1..=max {
            let now = base + Duration::from_millis(25 * pass);
            if pace.shot.is_some() {
                pace.shot_saved(now);
            }
            if pace.state.is_some() {
                pace.state_saved(now);
            }
            let f = frame(pass, now);
            match pace.next(&f) {
                Next::Hold { .. } => {}
                Next::End(e) => return (pace, base, Some(e)),
                Next::Take(entry) => {
                    let taken = answer(&entry);
                    if conformance::acts_taken(&entry, &taken) {
                        pace.act(now);
                    }
                    if let Some(e) = pace.taken(&f, taken) {
                        return (pace, base, Some(e));
                    }
                }
            }
        }
        (pace, base, None)
    }

    fn plain(e: &Entry) -> Taken {
        conformance::Answer::default_for(e).taken(e)
    }

    fn outcomes(lines: &[TraceLine]) -> Vec<(Option<usize>, &'static str)> {
        lines.iter().map(|l| (l.step, l.outcome.name())).collect()
    }

    #[test]
    fn the_trace_has_a_done_line_per_step() {
        let (mut pace, _, ended) = drive("a<Wait:30><Now>b", Config::new("app"), 200, &plain);
        assert!(ended.is_some());
        let lines = pace.take_trace();
        assert_eq!(outcomes(&lines), vec![(Some(1), "done"), (Some(2), "done"), (Some(3), "done"), (Some(4), "done")]);
        assert_eq!(lines[0].settled, Some(true));
        assert_eq!((lines[1].settled, lines[2].settled), (None, None), "<Wait:> and <Now> have no settled");
        assert!(lines.iter().all(|l| l.end_frame >= l.frame && l.end_ms >= l.time_ms && l.why.is_none()));
        assert!(pace.take_trace().is_empty(), "taken once");
    }

    #[test]
    fn a_shot_line_closes_when_the_picture_is_saved() {
        let (mut pace, _, _) = drive("a<Shot:s>b", Config::new("app"), 200, &plain);
        let lines = pace.take_trace();
        assert_eq!(outcomes(&lines), vec![(Some(1), "done"), (Some(2), "done"), (Some(3), "done")]);
        assert!(lines[1].end_ms > lines[1].time_ms, "the shot line is open until shot_saved: {lines:?}");
    }

    #[test]
    fn a_snap_is_one_trace_line() {
        let (mut pace, _, ended) = drive("a<Snap:x>b", Config::new("app"), 300, &plain);
        assert!(ended.is_some());
        let lines = pace.take_trace();
        assert_eq!(outcomes(&lines), vec![(Some(1), "done"), (Some(2), "done"), (Some(3), "done")]);
        assert!(lines[1].label.contains("Snap"), "{}", lines[1].label);
        assert!(lines[1].end_ms > lines[1].time_ms);
    }

    #[test]
    fn a_flight_holds_the_line_open() {
        let flying = |e: &Entry| match e.press {
            Press::Events(_) => Taken::Input { flight: true, more: Vec::new() },
            _ => plain(e),
        };
        let (mut pace, base, _) = drive("ab", Config::new("app"), 300, &flying);
        let lines = pace.take_trace();
        assert_eq!(outcomes(&lines), vec![(Some(1), "done"), (Some(2), "done")]);
        assert!(lines[0].end_frame > lines[0].frame, "closed when the flight landed, not when the input went in");
        assert!(lines[0].time_ms <= lines[0].end_ms && base <= base);
    }

    #[test]
    fn a_failure_closes_its_line_with_the_reason() {
        let fail = |e: &Entry| match e.press {
            Press::Events(_) if e.idx == 2 => Taken::Failed("it broke".into()),
            _ => plain(e),
        };
        let (mut pace, _, ended) = drive("a<Now>bc", Config::new("app"), 300, &fail);
        assert!(ended.is_some());
        let lines = pace.take_trace();
        let last = lines.last().unwrap();
        assert_eq!((last.step, last.outcome), (Some(3), Outcome::Failed), "{lines:?}");
        assert!(last.why.as_deref().is_some_and(|w| w.contains("it broke")), "{last:?}");
        assert!(lines[..lines.len() - 1].iter().all(|l| l.outcome == Outcome::Done));
    }

    #[test]
    fn closing_the_window_in_a_wait_cuts_the_line() {
        let (mut pace, base, ended) = drive("a<Wait:5000>b", Config::new("app"), 20, &plain);
        assert!(ended.is_none());
        let done = pace.end(Ending::Closed, base + Duration::from_millis(900), None);
        assert!(done.is_some());
        let lines = pace.take_trace();
        assert_eq!(outcomes(&lines), vec![(Some(1), "done"), (Some(2), "cut")]);
        // A host failure after the end has no step: it is a line of its own.
        let line = pace.no_step_line("<trace.jsonl>", "cannot write");
        assert_eq!((line.step, line.outcome), (None, Outcome::Failed));
        assert_eq!(line.why.as_deref(), Some("cannot write"));
    }

    #[test]
    fn an_answered_quit_stays_done_and_end_repeats_nothing() {
        let (mut pace, base, ended) = drive("a<Quit>b", Config::new("app"), 200, &plain);
        assert!(ended.is_some());
        assert!(pace.end(Ending::Quit, base + Duration::from_secs(9), None).is_none(), "the run has ended");
        assert!(pace.end(Ending::Quit, base + Duration::from_secs(9), None).is_none());
        let lines = pace.take_trace();
        assert!(lines.iter().all(|l| l.outcome == Outcome::Done), "{lines:?}");
        assert_eq!(lines[1].label, "<Quit>");
    }

    #[test]
    fn ending_between_the_stages_of_a_snap_fails_it_once() {
        // Stop after the first stage was saved: the shot stage is queued but not taken.
        let (mut pace, base, ended) = drive("<Snap:x>", Config::new("app"), 3, &plain);
        assert!(ended.is_none());
        let now = base + Duration::from_millis(500);
        let e = pace.end(Ending::Closed, now, None);
        assert!(e.as_ref().is_some_and(|e| e.failed), "{e:?}");
        assert!(pace.end(Ending::Closed, now, None).is_none(), "ending twice reports once");
        let lines = pace.take_trace();
        assert_eq!(lines.iter().filter(|l| l.outcome == Outcome::Failed).count(), 1, "{lines:?}");
        assert!(lines.iter().all(|l| l.step == Some(1)));
    }

    #[test]
    fn trace_lines_are_kept_without_a_report() {
        let mut cfg = Config::new("app");
        cfg.report = false;
        let (mut pace, base, _) = drive("a<Wait:5000>", cfg, 20, &plain);
        assert!(pace.end(Ending::Closed, base + Duration::from_millis(900), None).is_none());
        let lines = pace.take_trace();
        assert_eq!(outcomes(&lines), vec![(Some(1), "done"), (Some(2), "cut")]);
    }
}
