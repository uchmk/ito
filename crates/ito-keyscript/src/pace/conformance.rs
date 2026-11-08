//! The table that pins [`Pace`]: scripts, a clock, what the host sees each frame and how it answers, and the trace a
//! correct implementation leaves. A host that keeps its own copy of the logic proves it agrees by implementing
//! [`Subject`] over its own state machine and running [`check`] with [`cases`].
//!
//! Times are milliseconds from the start of the run (the fake clock is `base + ms`), passes are the main window's pass
//! numbers. Scripts are parsed with one app reading, `Pane` (tag `pane`), for `<ReadHas:Pane=...>`.

use super::{Config, Ending, Entry, Frame, How, Kind, More, Next, Pace, Taken};
use crate::{parse_with, Dialect, Press};
use ito_common::writes::{Failure, Pending, Status};
use std::time::{Duration, Instant};

/// How a frame's config-write ledger looks.
#[derive(Clone, Debug, Default)]
pub struct StatusSpec {
    /// The oldest write still waiting: its number, its file and when it began.
    pub pending: Option<(u64, &'static str, u64)>,
    pub answers: u64,
    pub failed: Option<&'static str>,
}

impl StatusSpec {
    pub(crate) fn status(&self, base: Instant) -> Status {
        Status {
            pending: usize::from(self.pending.is_some()),
            oldest: self.pending.map(|(seq, file, ms)| Pending { seq, file: file.to_owned(), since: base + Duration::from_millis(ms) }),
            answers: self.answers,
            failed: self.failed.map(|why| Failure { seq: 1, file: "common.toml".to_owned(), why: why.to_owned() }),
        }
    }
}

/// What happens to a picture asked for earlier, at the start of a tick.
#[derive(Clone, Debug)]
pub enum ShotEvent {
    Saved,
    Failed(&'static str),
}

/// What happens to a state JSON asked for earlier, at the start of a tick.
#[derive(Clone, Debug)]
pub enum StateEvent {
    Saved,
    Failed(&'static str),
}

/// How the host answers a take.
#[derive(Clone, Debug)]
pub enum Answer {
    Input {
        flight: bool,
        more: Vec<More>,
    },
    /// The window changed without events going in (a resize, a theme, a language): [`Taken::Changed`].
    Changed(Vec<More>),
    Done,
    Again(&'static str),
    Shot(&'static str),
    /// The app was asked to write the state JSON of `<State:name>`: [`Taken::State`].
    State(&'static str),
    Quit,
    Failed(&'static str),
}

impl Answer {
    pub(crate) fn taken(&self) -> Taken {
        match self {
            Answer::Input { flight, more } => Taken::Input { flight: *flight, more: more.clone() },
            Answer::Changed(more) => Taken::Changed { more: more.clone() },
            Answer::Done => Taken::Done,
            Answer::Again(why) => Taken::Again { why: (*why).to_owned() },
            Answer::Shot(path) => Taken::Shot { path: (*path).to_owned() },
            Answer::State(name) => Taken::State { name: (*name).to_owned() },
            Answer::Quit => Taken::Quit,
            Answer::Failed(why) => Taken::Failed((*why).to_owned()),
        }
    }

    /// What a host that has nothing special to say answers a step of this kind.
    pub(crate) fn default_for(e: &Entry) -> Self {
        match e.kind {
            Kind::Input => Answer::Input { flight: false, more: Vec::new() },
            // A look that is not `<Tree:>` finds its target and presses it (so it acts); `<Tree:>` only writes a file.
            Kind::Look if !matches!(e.press, Press::Tree(_)) => Answer::Input { flight: false, more: Vec::new() },
            Kind::Shot => Answer::Shot("s.png"),
            Kind::Quit => Answer::Quit,
            _ => Answer::Done,
        }
    }
}

/// Whether a step that was answered with `answer` has really pressed something, so the host calls [`Pace::act`] (and takes
/// its mark) before [`Pace::taken`].
///
/// - An input acts unless it is answered `Again`: when it is answered `Failed`, the input went in first and something
///   failed after; a `Changed` window is a press too.
/// - A look acts only when it found its target and pressed it (`Input` or `Changed`). Answered `Done` (a `<Tree:>` only
///   writes a file), `Failed` or `Again`, it did not press anything.
/// - Every other kind (a continuation, a check, a poll, a record, a shot, a quit) never acts.
pub fn acts(entry: &Entry, answer: &Answer) -> bool {
    acts_taken(entry, &answer.taken())
}

/// [`acts`] for a host that has the [`Taken`] itself.
pub fn acts_taken(entry: &Entry, taken: &Taken) -> bool {
    match entry.kind {
        Kind::Input => !matches!(taken, Taken::Again { .. }),
        Kind::Look => matches!(taken, Taken::Input { .. } | Taken::Changed { .. }),
        _ => false,
    }
}

/// One frame of the main window (or of another viewport).
#[derive(Clone, Debug)]
pub struct Tick {
    pub ms: u64,
    pub pass: u64,
    pub viewport: Option<&'static str>,
    pub settled: Option<bool>,
    pub generation: u64,
    pub writes: Option<StatusSpec>,
    pub shot: Option<ShotEvent>,
    pub state: Option<StateEvent>,
    /// The answer if the frame takes a step; the default for the step's kind if `None`.
    pub answer: Option<Answer>,
}

impl Tick {
    /// A settled frame of the main window.
    pub fn t(ms: u64, pass: u64) -> Self {
        Self { ms, pass, viewport: None, settled: Some(true), generation: 0, writes: None, shot: None, state: None, answer: None }
    }
    pub fn moving(mut self) -> Self {
        self.settled = Some(false);
        self
    }
    pub fn unknown(mut self) -> Self {
        self.settled = None;
        self
    }
    pub fn vp(mut self, id: &'static str) -> Self {
        self.viewport = Some(id);
        self
    }
    pub fn gen(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }
    pub fn writes(mut self, w: StatusSpec) -> Self {
        self.writes = Some(w);
        self
    }
    pub fn shot(mut self, e: ShotEvent) -> Self {
        self.shot = Some(e);
        self
    }
    pub fn state(mut self, e: StateEvent) -> Self {
        self.state = Some(e);
        self
    }
    pub fn answer(mut self, a: Answer) -> Self {
        self.answer = Some(a);
        self
    }
}

/// What a run leaves behind. Two subjects agree when their traces are equal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trace {
    /// Each take: the step's index, the time, and whether it was a continuation.
    pub takes: Vec<(usize, u64, bool)>,
    /// Each time `act` said "first input of this step": the step's index and the time.
    pub marks: Vec<(usize, u64)>,
    /// The tail of the done file, if the run ended.
    pub tail: Option<String>,
    pub failed: bool,
    /// Steps not carried out at the end.
    pub left: usize,
    pub unsettled: Vec<usize>,
    /// The failure's reason, if it failed.
    pub why: Option<String>,
    /// When the run ended (the tick, or the time of the close).
    pub end_ms: Option<u64>,
    pub action_gen: u64,
    /// The `left` that went into the end's tail ([`super::Ended::left`]), if the run ended. Differs from `left` after a
    /// failure, which clears the queue.
    pub ended_left: Option<usize>,
}

/// One case.
#[derive(Clone, Debug)]
pub struct Case {
    pub name: &'static str,
    pub script: &'static str,
    pub config: Config,
    pub ticks: Vec<Tick>,
    /// The window closed or the app quit at this time, if the run had not ended by then.
    pub end: Option<(u64, Ending)>,
    pub expect: Trace,
}

/// Something a case asks of a subject, derived from the case itself (see [`Case::needs`]). A subject that cannot do one
/// of them lists it in [`Subject::skips`] and [`check`] leaves the case out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// Some answer has a `More::Then` continuation.
    Then,
    /// Some main-window tick has `settled: None` (the 60 ms fallback).
    UnknownSettled,
    /// `config.first_wait` is not zero.
    FirstWait,
    /// Some answer is `Answer::Changed`.
    Changed,
    /// Some answer is `Answer::State` or some tick has a state event: the subject writes a state JSON and acknowledges it.
    StateJson,
}

impl Case {
    /// What this case asks of a subject, without duplicates, in the order of [`Need`]. Derived from the case, never
    /// tagged by hand.
    pub fn needs(&self) -> Vec<Need> {
        let has_then = |more: &[More]| more.iter().any(|m| matches!(m, More::Then(_)));
        let mut then = false;
        let mut changed = false;
        let mut state_json = self.ticks.iter().any(|t| t.state.is_some());
        for a in self.ticks.iter().filter_map(|t| t.answer.as_ref()) {
            match a {
                Answer::Input { more, .. } => then |= has_then(more),
                Answer::Changed(more) => {
                    changed = true;
                    then |= has_then(more);
                }
                Answer::State(_) => state_json = true,
                _ => {}
            }
        }
        // A child-viewport tick never reads `settled`, so it does not count.
        let unknown = self.ticks.iter().any(|t| t.viewport.is_none() && t.settled.is_none());
        let first = !self.config.first_wait.is_zero();
        [
            (then, Need::Then),
            (unknown, Need::UnknownSettled),
            (first, Need::FirstWait),
            (changed, Need::Changed),
            (state_json, Need::StateJson),
        ]
        .into_iter()
        .filter_map(|(on, n)| on.then_some(n))
        .collect()
    }
}

/// An implementation of the state machine, driven by the table.
pub trait Subject {
    fn run(&mut self, case: &Case) -> Trace;

    /// The needs this subject cannot meet: [`check`] skips a case that has one of them.
    fn skips(&self) -> &[Need] {
        &[]
    }
}

fn skipped_by(subject: &impl Subject, case: &Case) -> bool {
    let skips = subject.skips();
    case.needs().iter().any(|n| skips.contains(n))
}

/// The names of the cases [`check`] leaves out for `subject`, so a host can print them.
pub fn skipped(subject: &impl Subject, cases: &[Case]) -> Vec<&'static str> {
    cases.iter().filter(|c| skipped_by(subject, c)).map(|c| c.name).collect()
}

/// Runs every case; one error text lists every difference.
pub fn check(subject: &mut impl Subject, cases: &[Case]) -> Result<(), String> {
    let mut errs = Vec::new();
    for case in cases {
        if skipped_by(subject, case) {
            continue;
        }
        let got = subject.run(case);
        let want = &case.expect;
        macro_rules! cmp {
            ($f:ident) => {
                if want.$f != got.$f {
                    errs.push(format!("{}: {}: want {:?}, got {:?}", case.name, stringify!($f), want.$f, got.$f));
                }
            };
        }
        cmp!(takes);
        cmp!(marks);
        cmp!(tail);
        cmp!(failed);
        cmp!(left);
        cmp!(unsettled);
        cmp!(why);
        cmp!(end_ms);
        cmp!(action_gen);
        cmp!(ended_left);
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("\n"))
    }
}

pub(crate) fn dialect() -> Dialect<'static> {
    Dialect { reads: &[("Pane", "pane")], ..Dialect::default() }
}

/// [`Pace`] itself as a subject.
pub struct PaceSubject;

impl Subject for PaceSubject {
    fn run(&mut self, case: &Case) -> Trace {
        let base = Instant::now();
        let at = |ms: u64| base + Duration::from_millis(ms);
        let steps = parse_with(case.script, &dialect()).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let mut pace = Pace::new(case.config.clone(), &steps, base);
        let mut tr = Trace::default();
        let mut ended = None;
        let mut end_ms = None;
        for tick in &case.ticks {
            let now = at(tick.ms);
            match &tick.shot {
                Some(ShotEvent::Saved) => pace.shot_saved(now),
                Some(ShotEvent::Failed(why)) => {
                    if let Some(e) = pace.shot_failed((*why).to_owned()) {
                        ended = Some(e);
                        end_ms = Some(tick.ms);
                        break;
                    }
                }
                None => {}
            }
            match &tick.state {
                Some(StateEvent::Saved) => pace.state_saved(now),
                Some(StateEvent::Failed(why)) => {
                    if let Some(e) = pace.state_failed(why) {
                        ended = Some(e);
                        end_ms = Some(tick.ms);
                        break;
                    }
                }
                None => {}
            }
            let status = tick.writes.as_ref().map(|w| w.status(base));
            let frame = Frame {
                pass: tick.pass,
                now,
                viewport: tick.viewport,
                settled: tick.settled,
                writes: status.as_ref(),
                generation: tick.generation,
            };
            match pace.next(&frame) {
                Next::Hold { .. } => {}
                Next::End(e) => {
                    ended = Some(e);
                    end_ms = Some(tick.ms);
                    break;
                }
                Next::Take(entry) => {
                    tr.takes.push((entry.idx, tick.ms, entry.kind == Kind::Then));
                    let answer = tick.answer.clone().unwrap_or_else(|| Answer::default_for(&entry));
                    if acts(&entry, &answer) && pace.act(now) {
                        tr.marks.push((entry.idx, tick.ms));
                    }
                    if let Some(e) = pace.taken(&frame, answer.taken()) {
                        ended = Some(e);
                        end_ms = Some(tick.ms);
                        break;
                    }
                }
            }
        }
        if let Some((ms, how)) = case.end {
            let closed = pace.end(how, at(ms), None);
            if ended.is_none() {
                if closed.is_some() {
                    end_ms = Some(ms);
                }
                ended = closed;
            } else if closed.is_some() {
                tr.why = Some("end returned Some after the run had ended".to_owned());
            }
            if pace.end(how, at(ms), None).is_some() {
                tr.why = Some("a second end returned Some".to_owned());
            }
        }
        if let Some(e) = &ended {
            tr.tail = Some(e.tail.clone());
            if let How::Failed { why, .. } = &e.how {
                tr.why = Some(why.clone());
            }
            tr.ended_left = Some(e.left);
        }
        tr.failed = pace.failed();
        tr.left = pace.left();
        tr.unsettled = pace.unsettled().to_vec();
        tr.end_ms = end_ms;
        tr.action_gen = pace.action_gen();
        tr
    }
}

fn case(name: &'static str, script: &'static str, ticks: Vec<Tick>, expect: Trace) -> Case {
    Case { name, script, config: Config::new("app"), ticks, end: None, expect }
}

fn failed_tail(step: &str, why: &str, left: usize) -> Option<String> {
    Some(crate::failed_report(step, why, left))
}

fn done_tail() -> Option<String> {
    Some("keys: done\n".to_owned())
}

/// Every case.
#[allow(clippy::vec_init_then_push)]
pub fn cases() -> Vec<Case> {
    use Answer as A;
    use Tick as T;
    let flight = A::Input { flight: true, more: Vec::new() };
    let mut out = Vec::new();

    out.push(case(
        "two_passes_after_the_last_step",
        "a",
        vec![T::t(0, 1), T::t(10, 2), T::t(20, 3), T::t(30, 4)],
        Trace { takes: vec![(0, 10, false)], marks: vec![(0, 10)], tail: done_tail(), end_ms: Some(30), ..Trace::default() },
    ));
    out.push(case(
        "an_unsettled_screen_holds_the_step",
        "a",
        vec![T::t(0, 2).moving(), T::t(100, 3).moving(), T::t(200, 4), T::t(300, 5), T::t(400, 6)],
        Trace { takes: vec![(0, 200, false)], marks: vec![(0, 200)], tail: done_tail(), end_ms: Some(400), ..Trace::default() },
    ));
    out.push(case(
        "unknown_settled_means_60_ms_since_the_last_action",
        "a",
        vec![
            T::t(0, 2).unknown(),
            T::t(59, 3).unknown(),
            T::t(60, 4).unknown(),
            T::t(70, 5).unknown(),
            T::t(80, 6).unknown(),
            T::t(119, 6).unknown(),
            T::t(120, 6).unknown(),
        ],
        Trace { takes: vec![(0, 60, false)], marks: vec![(0, 60)], tail: done_tail(), end_ms: Some(120), ..Trace::default() },
    ));
    out.push(case(
        "the_settle_wait_is_passed_strictly",
        "ab",
        vec![T::t(0, 2), T::t(1, 4).moving(), T::t(5000, 4).moving(), T::t(5001, 4).moving(), T::t(5002, 5).moving(), T::t(5003, 6)],
        Trace {
            takes: vec![(0, 0, false), (1, 5001, false)],
            marks: vec![(0, 0), (1, 5001)],
            tail: Some("unsettled: 1 of 2 steps went in after 5 s without settling: b\nkeys: unsettled\n".to_owned()),
            failed: true,
            unsettled: vec![1],
            end_ms: Some(5003),
            ..Trace::default()
        },
    ));
    out.push(case(
        "a_script_ending_on_a_moving_screen_is_unsettled",
        "a",
        vec![T::t(0, 2), T::t(6000, 4).moving()],
        Trace {
            takes: vec![(0, 0, false)],
            marks: vec![(0, 0)],
            tail: Some("unsettled: the script ended after 5 s without settling\nkeys: unsettled\n".to_owned()),
            failed: true,
            end_ms: Some(6000),
            ..Trace::default()
        },
    ));
    out.push(case(
        "now_needs_one_pass_and_not_a_settled_screen",
        "a<Now>b",
        vec![T::t(0, 2), T::t(1, 3).moving(), T::t(2, 4), T::t(3, 5)],
        Trace {
            takes: vec![(0, 0, false), (2, 1, false)],
            marks: vec![(0, 0), (2, 1)],
            tail: done_tail(),
            end_ms: Some(3),
            ..Trace::default()
        },
    ));
    out.push(case(
        "now_goes_through_a_wait",
        "a<Now><Wait:100>b",
        vec![T::t(0, 2), T::t(1, 3), T::t(50, 4), T::t(100, 5), T::t(101, 6), T::t(102, 7), T::t(103, 8)],
        Trace {
            takes: vec![(0, 0, false), (3, 101, false)],
            marks: vec![(0, 0), (3, 101)],
            tail: done_tail(),
            end_ms: Some(103),
            ..Trace::default()
        },
    ));
    let mut first = Config::new("app");
    first.first_wait = Duration::from_millis(500);
    out.push(Case {
        name: "a_leading_now_still_waits_for_the_first_wait",
        script: "<Now>a",
        config: first,
        ticks: vec![T::t(0, 2), T::t(499, 3), T::t(500, 4), T::t(501, 6)],
        end: None,
        expect: Trace { takes: vec![(1, 500, false)], marks: vec![(1, 500)], tail: done_tail(), end_ms: Some(501), ..Trace::default() },
    });
    out.push(case(
        "a_continuation_needs_a_new_pass_and_a_frame",
        "a",
        vec![
            T::t(0, 2).answer(A::Input { flight: false, more: vec![More::Then(Press::Paste)] }),
            T::t(5, 2),
            T::t(10, 3),
            T::t(20, 3),
            T::t(30, 4),
            T::t(40, 5),
        ],
        Trace { takes: vec![(0, 0, false), (0, 20, true)], marks: vec![(0, 0)], tail: done_tail(), end_ms: Some(40), ..Trace::default() },
    ));
    out.push(case(
        "a_check_looks_again_each_pass_then_the_next_step_waits_to_settle",
        "<Assert:x>b",
        vec![
            T::t(0, 2).answer(A::Again("no")),
            T::t(16, 3).answer(A::Again("no")),
            T::t(32, 4),
            T::t(40, 5),
            T::t(50, 6),
            T::t(60, 7),
            T::t(70, 8),
        ],
        Trace {
            takes: vec![(0, 0, false), (0, 16, false), (0, 32, false), (1, 50, false)],
            marks: vec![(1, 50)],
            tail: done_tail(),
            end_ms: Some(70),
            ..Trace::default()
        },
    ));
    out.push(case(
        "a_check_fails_after_the_text_wait",
        "<Assert:x>",
        vec![T::t(0, 2).answer(A::Again("no")), T::t(2999, 3).answer(A::Again("no")), T::t(3000, 4).answer(A::Again("no"))],
        Trace {
            takes: vec![(0, 0, false), (0, 2999, false), (0, 3000, false)],
            tail: failed_tail("<Assert:x>", "no", 1),
            failed: true,
            why: Some("no".to_owned()),
            end_ms: Some(3000),
            ..Trace::default()
        },
    ));
    out.push(case(
        "a_read_has_succeeds_after_a_retry",
        "<ReadHas:Pane=ok>",
        vec![T::t(0, 2).answer(A::Again("no")), T::t(100, 3), T::t(110, 5)],
        Trace { takes: vec![(0, 0, false), (0, 100, false)], tail: done_tail(), end_ms: Some(110), ..Trace::default() },
    ));
    out.push(case(
        "a_read_has_fails_after_the_read_wait",
        "<ReadHas:Pane=ok>",
        vec![T::t(0, 2).answer(A::Again("no")), T::t(14999, 3).answer(A::Again("no")), T::t(15000, 4).answer(A::Again("no"))],
        Trace {
            takes: vec![(0, 0, false), (0, 14999, false), (0, 15000, false)],
            tail: failed_tail("<ReadHas:Pane=ok>", "no", 1),
            failed: true,
            why: Some("no".to_owned()),
            end_ms: Some(15000),
            ..Trace::default()
        },
    ));
    out.push(case(
        "a_look_retries_only_on_a_settled_screen",
        "a<Click:Save>",
        vec![T::t(0, 2), T::t(1, 4).answer(A::Again("no")), T::t(2, 5).moving(), T::t(3, 6), T::t(4, 7), T::t(5, 8)],
        Trace {
            takes: vec![(0, 0, false), (1, 1, false), (1, 3, false)],
            marks: vec![(0, 0), (1, 3)],
            tail: done_tail(),
            end_ms: Some(5),
            ..Trace::default()
        },
    ));
    out.push(case(
        "a_check_retries_even_on_a_moving_screen",
        "a<Assert:x>",
        vec![T::t(0, 2), T::t(1, 4).answer(A::Again("no")), T::t(2, 5).moving(), T::t(3, 6), T::t(4, 7)],
        Trace {
            takes: vec![(0, 0, false), (1, 1, false), (1, 2, false)],
            marks: vec![(0, 0)],
            tail: done_tail(),
            end_ms: Some(4),
            ..Trace::default()
        },
    ));
    out.push(case(
        "an_input_that_met_another_viewport_fails",
        "a",
        vec![T::t(0, 2).answer(flight.clone()), T::t(1, 2).vp("ViewportId(1)")],
        Trace {
            takes: vec![(0, 0, false)],
            marks: vec![(0, 0)],
            tail: failed_tail("a", "the input went to viewport ViewportId(1) while the main window was not drawn", 1),
            failed: true,
            why: Some("the input went to viewport ViewportId(1) while the main window was not drawn".to_owned()),
            end_ms: Some(1),
            ..Trace::default()
        },
    ));
    out.push(case(
        "another_viewport_is_skipped",
        "a",
        vec![T::t(0, 2).vp("ViewportId(1)"), T::t(1, 2), T::t(2, 3), T::t(3, 4)],
        Trace { takes: vec![(0, 1, false)], marks: vec![(0, 1)], tail: done_tail(), end_ms: Some(3), ..Trace::default() },
    ));
    out.push(case(
        "a_drawn_input_moves_the_action_generation",
        "ab",
        vec![T::t(0, 2).answer(flight.clone()), T::t(10, 3).gen(7), T::t(20, 4).gen(7), T::t(30, 5).gen(7), T::t(40, 6).gen(7)],
        Trace {
            takes: vec![(0, 0, false), (1, 20, false)],
            marks: vec![(0, 0), (1, 20)],
            tail: done_tail(),
            end_ms: Some(40),
            action_gen: 7,
            ..Trace::default()
        },
    ));
    out.push(case(
        "an_input_not_drawn_fails_after_the_deliver_wait",
        "a",
        vec![T::t(0, 2).answer(flight), T::t(9999, 2), T::t(10000, 2)],
        Trace {
            takes: vec![(0, 0, false)],
            marks: vec![(0, 0)],
            tail: failed_tail("a", "the input was not drawn within 10 s (the window may be hidden, minimized or covered)", 1),
            failed: true,
            why: Some("the input was not drawn within 10 s (the window may be hidden, minimized or covered)".to_owned()),
            end_ms: Some(10000),
            ..Trace::default()
        },
    ));
    let pending = StatusSpec { pending: Some((1, "common.toml", 0)), ..StatusSpec::default() };
    out.push(case(
        "a_pending_config_write_holds_then_fails_when_old",
        "a",
        vec![T::t(0, 2).writes(pending.clone()), T::t(9999, 3).writes(pending.clone()), T::t(10000, 4).writes(pending.clone())],
        Trace {
            tail: failed_tail("<config write>", "no answer from the config writer within 10 s of write #1 (common.toml)", 1),
            failed: true,
            left: 0,
            why: Some("no answer from the config writer within 10 s of write #1 (common.toml)".to_owned()),
            end_ms: Some(10000),
            ..Trace::default()
        },
    ));
    let answered = StatusSpec { answers: 1, ..StatusSpec::default() };
    out.push(case(
        "an_answered_config_write_moves_the_action_generation",
        "a",
        vec![
            T::t(0, 2).writes(pending),
            T::t(5, 3).writes(answered.clone()).gen(4),
            T::t(6, 4).writes(answered.clone()).gen(4),
            T::t(7, 5).writes(answered).gen(4),
        ],
        Trace { takes: vec![(0, 5, false)], marks: vec![(0, 5)], tail: done_tail(), end_ms: Some(7), action_gen: 4, ..Trace::default() },
    ));
    out.push(case(
        "a_failed_config_write_fails_verbatim",
        "a",
        vec![T::t(0, 2).writes(StatusSpec { failed: Some("disk full"), ..StatusSpec::default() })],
        Trace {
            tail: failed_tail("<config write>", "disk full", 1),
            failed: true,
            why: Some("disk full".to_owned()),
            end_ms: Some(0),
            ..Trace::default()
        },
    ));
    out.push(case(
        "a_saved_shot_lets_the_next_step_go",
        "<Shot:s>b",
        vec![T::t(0, 2), T::t(100, 3), T::t(200, 4).shot(ShotEvent::Saved), T::t(210, 5), T::t(220, 6)],
        Trace {
            takes: vec![(0, 0, false), (1, 200, false)],
            marks: vec![(1, 200)],
            tail: done_tail(),
            end_ms: Some(220),
            ..Trace::default()
        },
    ));
    out.push(case(
        "a_shot_that_never_arrives_fails",
        "<Shot:s>",
        vec![T::t(0, 2), T::t(5000, 3), T::t(5001, 4)],
        Trace {
            takes: vec![(0, 0, false)],
            tail: failed_tail("<Shot:s>", "no picture arrived within 5 s; s.png was not saved", 1),
            failed: true,
            why: Some("no picture arrived within 5 s; s.png was not saved".to_owned()),
            end_ms: Some(5001),
            ..Trace::default()
        },
    ));
    out.push(case(
        "a_shot_that_could_not_be_saved_fails",
        "<Shot:s>",
        vec![T::t(0, 2), T::t(10, 3).shot(ShotEvent::Failed("disk full"))],
        Trace {
            takes: vec![(0, 0, false)],
            tail: failed_tail("<Shot:s>", "disk full", 1),
            failed: true,
            why: Some("disk full".to_owned()),
            end_ms: Some(10),
            ..Trace::default()
        },
    ));
    out.push(Case {
        name: "closing_with_a_shot_out_fails",
        script: "<Shot:s>b",
        config: Config::new("app"),
        ticks: vec![T::t(0, 2)],
        end: Some((10, Ending::Closed)),
        expect: Trace {
            takes: vec![(0, 0, false)],
            tail: failed_tail("<Shot:s>", "the window was closed before the picture of <Shot:s> was saved", 2),
            failed: true,
            why: Some("the window was closed before the picture of <Shot:s> was saved".to_owned()),
            end_ms: Some(10),
            left: 0,
            ..Trace::default()
        },
    });
    out.push(case(
        "state_json_saved_lets_the_next_step_go",
        "<State:a>b",
        vec![T::t(0, 2).answer(Answer::State("a")), T::t(100, 3), T::t(200, 4).state(StateEvent::Saved), T::t(210, 5), T::t(220, 6)],
        Trace {
            takes: vec![(0, 0, false), (1, 200, false)],
            marks: vec![(1, 200)],
            tail: done_tail(),
            end_ms: Some(220),
            ..Trace::default()
        },
    ));
    out.push(case(
        "state_json_that_could_not_be_written_fails",
        "<State:a>",
        vec![T::t(0, 2).answer(Answer::State("a")), T::t(10, 3).state(StateEvent::Failed("disk full"))],
        Trace {
            takes: vec![(0, 0, false)],
            tail: failed_tail("<State:a>", "disk full", 1),
            failed: true,
            why: Some("disk full".to_owned()),
            end_ms: Some(10),
            ..Trace::default()
        },
    ));
    out.push(case(
        "state_json_that_never_arrives_fails",
        "<State:a>",
        vec![T::t(0, 2).answer(Answer::State("a")), T::t(5000, 3), T::t(5001, 4)],
        Trace {
            takes: vec![(0, 0, false)],
            tail: failed_tail("<State:a>", "no state JSON for <State:a> within 5 s", 1),
            failed: true,
            why: Some("no state JSON for <State:a> within 5 s".to_owned()),
            end_ms: Some(5001),
            ..Trace::default()
        },
    ));
    out.push(Case {
        name: "state_json_closing_with_one_out_fails",
        script: "<State:a>b",
        config: Config::new("app"),
        ticks: vec![T::t(0, 2).answer(Answer::State("a"))],
        end: Some((10, Ending::Closed)),
        expect: Trace {
            takes: vec![(0, 0, false)],
            tail: failed_tail("<State:a>", "the window was closed before the state JSON of <State:a> was written", 2),
            failed: true,
            why: Some("the window was closed before the state JSON of <State:a> was written".to_owned()),
            end_ms: Some(10),
            left: 0,
            ..Trace::default()
        },
    });
    out.push(case(
        "state_json_out_holds_a_following_wait",
        "<State:a><Wait:100>b",
        vec![
            T::t(0, 2).answer(Answer::State("a")),
            T::t(1, 3),
            T::t(2, 4),
            T::t(3, 5),
            T::t(200, 6).state(StateEvent::Saved),
            T::t(201, 7),
            T::t(202, 8),
            T::t(203, 9),
            T::t(304, 10),
            T::t(305, 11),
        ],
        // The wait is counted from the State step (as after a shot), so it is already up when the JSON is saved at 200.
        Trace {
            takes: vec![(0, 0, false), (2, 202, false)],
            marks: vec![(2, 202)],
            tail: done_tail(),
            end_ms: Some(304),
            ..Trace::default()
        },
    ));
    out.push(Case {
        name: "closing_with_steps_left_is_not_a_failure",
        script: "ab",
        config: Config::new("app"),
        ticks: vec![T::t(0, 2)],
        end: Some((10, Ending::Closed)),
        expect: Trace {
            takes: vec![(0, 0, false)],
            marks: vec![(0, 0)],
            tail: Some("keys: closed\nleft: 1 not pressed\n".to_owned()),
            left: 1,
            end_ms: Some(10),
            ..Trace::default()
        },
    });
    out.push(Case {
        name: "closing_after_the_end_writes_nothing_more",
        script: "a",
        config: Config::new("app"),
        ticks: vec![T::t(0, 2), T::t(1, 3), T::t(2, 4)],
        end: Some((10, Ending::Closed)),
        expect: Trace { takes: vec![(0, 0, false)], marks: vec![(0, 0)], tail: done_tail(), end_ms: Some(2), ..Trace::default() },
    });
    out.push(Case {
        name: "the_app_quitting_after_quit_is_done",
        script: "<Quit>",
        config: Config::new("app"),
        ticks: vec![T::t(0, 2)],
        end: Some((10, Ending::Quit)),
        expect: Trace {
            takes: vec![(0, 0, false)],
            tail: Some("quit: yes\nkeys: done\n".to_owned()),
            end_ms: Some(10),
            ..Trace::default()
        },
    });
    let mut quiet = Config::new("app");
    quiet.report = false;
    out.push(Case {
        name: "without_a_report_an_empty_queue_just_holds",
        script: "a",
        config: quiet,
        ticks: vec![T::t(0, 2), T::t(1, 3), T::t(2, 4), T::t(3, 5)],
        end: Some((10, Ending::Closed)),
        expect: Trace { takes: vec![(0, 0, false)], marks: vec![(0, 0)], ..Trace::default() },
    });
    out.push(case(
        "a_failed_step_fails_the_run",
        "ab",
        vec![T::t(0, 2).answer(A::Failed("boom"))],
        Trace {
            tail: failed_tail("a", "boom", 2),
            failed: true,
            why: Some("boom".to_owned()),
            takes: vec![(0, 0, false)],
            marks: vec![(0, 0)],
            end_ms: Some(0),
            ..Trace::default()
        },
    ));
    // `<Tree:>` only writes a file: it is taken and answered `Done`, and leaves no mark; the key after it does.
    out.push(case(
        "a_tree_does_not_act",
        "<Tree:x>a",
        vec![T::t(0, 2), T::t(1, 4), T::t(2, 5), T::t(3, 6)],
        Trace { takes: vec![(0, 0, false), (1, 1, false)], marks: vec![(1, 1)], tail: done_tail(), end_ms: Some(3), ..Trace::default() },
    ));
    // A look that fails pressed nothing, so it takes no mark.
    out.push(case(
        "a_look_that_fails_does_not_act",
        "<Click:Save>",
        vec![T::t(0, 2).answer(A::Failed("no such button"))],
        Trace {
            takes: vec![(0, 0, false)],
            tail: failed_tail("<Click:Save>", "no such button", 1),
            failed: true,
            why: Some("no such button".to_owned()),
            end_ms: Some(0),
            ..Trace::default()
        },
    ));
    // A look that found its target and pressed it takes one mark, at the take; its continuation takes none.
    out.push(case(
        "a_found_look_acts",
        "<Click:Save>",
        vec![
            T::t(0, 2).answer(A::Input { flight: true, more: vec![More::Then(Press::Paste)] }),
            T::t(10, 3),
            T::t(20, 3),
            T::t(30, 4),
            T::t(40, 5),
        ],
        Trace { takes: vec![(0, 0, false), (0, 20, true)], marks: vec![(0, 0)], tail: done_tail(), end_ms: Some(40), ..Trace::default() },
    ));
    // A change without events (a resize, a theme) is a press: it takes a mark.
    out.push(case(
        "a_changed_answer_acts",
        "<Size:480x600>",
        vec![T::t(0, 2).answer(A::Changed(Vec::new())), T::t(1, 3), T::t(2, 4)],
        Trace { takes: vec![(0, 0, false)], marks: vec![(0, 0)], tail: done_tail(), end_ms: Some(2), ..Trace::default() },
    ));
    // A change has no events to lose: a frame of another viewport at the same pass holds it; the next main pass draws it.
    out.push(case(
        "a_changed_flight_holds_on_a_viewport_frame",
        "<Size:480x600>b",
        vec![
            T::t(0, 2).answer(A::Changed(Vec::new())),
            T::t(1, 2).vp("ViewportId(1)"),
            T::t(2, 3).gen(5),
            T::t(3, 4).gen(5),
            T::t(4, 5).gen(5),
            T::t(5, 6).gen(5),
        ],
        Trace {
            takes: vec![(0, 0, false), (1, 3, false)],
            marks: vec![(0, 0), (1, 3)],
            tail: done_tail(),
            end_ms: Some(5),
            action_gen: 5,
            ..Trace::default()
        },
    ));
    // The counterpart: an input that put events in fails when a viewport frame meets it at the same pass.
    out.push(case(
        "an_input_flight_fails_on_a_viewport_frame",
        "<Click:Save>",
        vec![T::t(0, 2).answer(A::Input { flight: true, more: Vec::new() }), T::t(1, 2).vp("ViewportId(2)")],
        Trace {
            takes: vec![(0, 0, false)],
            marks: vec![(0, 0)],
            tail: failed_tail("<Click:Save>", "the input went to viewport ViewportId(2) while the main window was not drawn", 1),
            failed: true,
            why: Some("the input went to viewport ViewportId(2) while the main window was not drawn".to_owned()),
            end_ms: Some(1),
            ..Trace::default()
        },
    ));
    // A change that is never drawn still fails at the deliver wait.
    out.push(case(
        "a_changed_flight_not_drawn_fails_at_the_deliver_wait",
        "<Size:480x600>",
        vec![T::t(0, 2).answer(A::Changed(Vec::new())), T::t(9999, 2), T::t(10000, 2)],
        Trace {
            takes: vec![(0, 0, false)],
            marks: vec![(0, 0)],
            tail: failed_tail("<Size:480x600>", "the input was not drawn within 10 s (the window may be hidden, minimized or covered)", 1),
            failed: true,
            why: Some("the input was not drawn within 10 s (the window may be hidden, minimized or covered)".to_owned()),
            end_ms: Some(10000),
            ..Trace::default()
        },
    ));
    // How a host paces `<Size:>`: the resize is answered `Changed` with a 400 ms wait after it. The step after goes in 400 ms
    // after the resize, then two passes and `GAP` (60 ms, the fallback for an unknown screen) after the wait was consumed.
    out.push(case(
        "a_resize_waits_its_settle_then_the_next_step_settles",
        "<Size:480x600>a",
        vec![
            T::t(0, 2).unknown(),
            T::t(60, 3).unknown().answer(A::Changed(vec![More::Step(Press::Wait(Duration::from_millis(400)))])),
            T::t(61, 4).unknown(),
            T::t(200, 5).unknown(),
            T::t(459, 6).unknown(),
            T::t(460, 7).unknown(),
            T::t(461, 8).unknown(),
            T::t(520, 9).unknown(),
            T::t(521, 10).unknown(),
            T::t(522, 11).unknown(),
            T::t(523, 12).unknown(),
            T::t(582, 13).unknown(),
            T::t(583, 14).unknown(),
        ],
        Trace {
            takes: vec![(0, 60, false), (1, 520, false)],
            marks: vec![(0, 60), (1, 520)],
            tail: done_tail(),
            end_ms: Some(582),
            ..Trace::default()
        },
    ));
    // The `left` in the tail survives the failure, which clears the queue (`left` of the trace is 0).
    out.push(case(
        "ended_left_is_the_tail_left",
        "abc",
        vec![T::t(0, 2), T::t(2, 4).answer(A::Failed("boom"))],
        Trace {
            takes: vec![(0, 0, false), (1, 2, false)],
            marks: vec![(0, 0), (1, 2)],
            tail: failed_tail("b", "boom", 2),
            failed: true,
            why: Some("boom".to_owned()),
            end_ms: Some(2),
            ended_left: Some(2),
            ..Trace::default()
        },
    ));
    // `ended_left` is the number in the tail's `left:` line (0 when it has none): derived, so the older rows need no tag.
    for c in &mut out {
        if c.expect.ended_left.is_none() {
            if let Some(tail) = &c.expect.tail {
                let n = tail.lines().find_map(|l| l.strip_prefix("left: ")?.strip_suffix(" not pressed")?.parse().ok());
                c.expect.ended_left = Some(n.unwrap_or(0));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare(ticks: Vec<Tick>) -> Case {
        case("t", "a", ticks, Trace::default())
    }

    #[test]
    fn a_continuation_inside_an_input_is_a_then_need() {
        let c = bare(vec![Tick::t(0, 2).answer(Answer::Input { flight: false, more: vec![More::Then(Press::Paste)] })]);
        assert_eq!(c.needs(), vec![Need::Then]);
        let step = bare(vec![Tick::t(0, 2).answer(Answer::Input { flight: false, more: vec![More::Step(Press::Paste)] })]);
        assert_eq!(step.needs(), vec![]);
    }

    #[test]
    fn a_continuation_inside_a_changed_answer_is_both_needs() {
        let c = bare(vec![Tick::t(0, 2).answer(Answer::Changed(vec![More::Then(Press::Paste)]))]);
        assert_eq!(c.needs(), vec![Need::Then, Need::Changed]);
    }

    #[test]
    fn several_needs_come_once_in_the_order_of_the_enum() {
        let mut c = bare(vec![
            Tick::t(0, 2).unknown().answer(Answer::Changed(vec![])),
            Tick::t(1, 3).unknown().answer(Answer::Input { flight: false, more: vec![More::Then(Press::Paste)] }),
            Tick::t(2, 4).unknown().answer(Answer::Changed(vec![More::Then(Press::Paste)])),
        ]);
        c.config.first_wait = Duration::from_millis(5);
        assert_eq!(c.needs(), vec![Need::Then, Need::UnknownSettled, Need::FirstWait, Need::Changed]);
    }

    #[test]
    fn an_unknown_settled_child_viewport_tick_is_not_a_need() {
        let c = bare(vec![Tick::t(0, 2).vp("ViewportId(1)").unknown(), Tick::t(1, 2)]);
        assert_eq!(c.needs(), vec![]);
        assert_eq!(bare(vec![Tick::t(0, 2).unknown()]).needs(), vec![Need::UnknownSettled]);
    }

    #[test]
    fn check_leaves_out_a_case_whose_need_the_subject_skips() {
        struct NoChange {
            ran: Vec<&'static str>,
        }
        impl Subject for NoChange {
            fn run(&mut self, case: &Case) -> Trace {
                self.ran.push(case.name);
                case.expect.clone()
            }
            fn skips(&self) -> &[Need] {
                &[Need::Changed]
            }
        }
        let cases = cases();
        let mut s = NoChange { ran: Vec::new() };
        check(&mut s, &cases).unwrap();
        assert!(!s.ran.contains(&"a_changed_answer_acts"));
        assert!(s.ran.contains(&"a_tree_does_not_act"));
        let skipped = skipped(&s, &cases);
        assert!(skipped.contains(&"a_changed_answer_acts"), "{skipped:?}");
        assert!(!skipped.contains(&"a_tree_does_not_act"));
        assert_eq!(s.ran.len() + skipped.len(), cases.len());
    }
}
