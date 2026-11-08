//! State JSON v1: `<dir>/state/<name>.json`, what an app writes when a script says `<State:name>`.
//!
//! Who fills what: ito fills the common part (the run, the theme, the language, the window, and `ui[]`: the physical-pixel
//! rectangle of every widget with a test ID). The app fills only what it knows ([`App`]). What the app does not say is
//! `null` in the file and its name is left out of `capabilities`; nothing is ever guessed.
//!
//! The same-pass merge: [`submit`] only queues the app's part. The file is built at the end of the main-window pass
//! that follows the submission (inside the observation plugin's `output_hook`), from that pass's accessibility tree, so
//! the rectangles and the app's part describe the same frame. A pass egui is going to throw away (a requested discard
//! with passes left) keeps the submission for the next pass; the file is built from the pass that is kept.
//!
//! The file name is `<name>.json` (written as `<name>.json.part` and renamed). A writer thread does the I/O and then
//! acknowledges with [`take_ack`]; `pace` holds the `<State:>` step until the ack, and a missing or failed ack is a
//! harness failure (`keys: failed`), not a test result.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sha2::{Digest, Sha256};

pub use serde_json::Value;

/// What the app knows; everything else is filled by ito. `None` is "this app does not say": null in the JSON and the
/// capability is left out. Never fill a guess.
#[derive(Clone, Debug, Default)]
pub struct App {
    pub screen: Option<String>,
    /// `Some(None)`: known, no overlay is open. `None`: not known.
    pub overlay: Option<Option<String>>,
    /// The in-app toasts on screen now (not OS notifications). `None`: not known (never send `Some(vec![])` unless the
    /// app knows there are none).
    pub toasts: Option<Vec<Toast>>,
    pub app_state: Option<Value>,
}

/// One in-app toast.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Toast {
    pub text: String,
    pub kind: String,
}

/// What the runner knows. Build it with `Header { dir, name, ..Default::default() }` so fields can be added later.
#[derive(Clone, Debug, Default)]
pub struct Header {
    /// The run folder; the file goes to `<dir>/state/<name>.json`.
    pub dir: PathBuf,
    /// The `<State:name>` name.
    pub name: String,
    pub run_id: Option<String>,
    /// 0-based index of the step this state follows (the State step's index minus 1).
    pub after_step: Option<usize>,
    /// That step's label.
    pub after_label: Option<String>,
    pub settled: Option<bool>,
    pub app_name: Option<String>,
    pub app_version: Option<String>,
    pub theme_requested: Option<String>,
    pub lang_requested: Option<String>,
    /// `Some` for the state before / after a `<Snap:>` picture; then `capture_id` has to be set too.
    pub phase: Option<Phase>,
    /// The id the before state, the after state and the side JSON of one capture share (`"{step}-{name}"`).
    pub capture_id: Option<String>,
}

/// Which side of a `<Snap:>` picture a state is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Before,
    After,
}

impl Phase {
    fn text(self) -> &'static str {
        match self {
            Phase::Before => "before",
            Phase::After => "after",
        }
    }
}

/// The answer to a [`submit`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Ack {
    Written { name: String, path: PathBuf, sha256: String, stable: Stable, frame: u64, snapshot_seq: u64 },
    Failed { name: String, why: String },
}

/// The hashes of the `stable` block: what two states have in common when the drawn UI / the app's part did not change.
/// Equal hashes do not prove that nothing moved in between.
#[derive(Clone, Debug, PartialEq)]
pub struct Stable {
    pub projection: u32,
    pub ui_sha256: String,
    pub app_sha256: String,
}

/// Queue the app's part of the state JSON for `header.name`. It is merged at the end of the next main-window pass.
///
/// `Err` at once (nothing queued) when [`crate::observe`] was not called, the name is bad, or a submission with a
/// different name is still waiting. A submission with the same name replaces the waiting one.
pub fn submit(ctx: &egui::Context, header: Header, app: App) -> Result<(), String> {
    if !crate::good_name(&header.name) {
        return Err(format!("bad state name `{}` (use ASCII letters, digits, `-` and `_`)", header.name));
    }
    if header.phase.is_some() && header.capture_id.as_deref().is_none_or(str::is_empty) {
        return Err(format!("state `{}` has a phase but no capture_id", header.name));
    }
    let pass = ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT);
    ctx.with_plugin::<crate::Tap, _>(|t| t.state.submit(header, app, pass))
        .unwrap_or_else(|| Err("no observation: call ito_keyscript::observe(ctx) first".to_owned()))
}

/// The oldest acknowledgement not taken yet.
pub fn take_ack(ctx: &egui::Context) -> Option<Ack> {
    ctx.with_plugin::<crate::Tap, _>(|t| t.state.take_ack()).flatten()
}

struct Pending {
    header: Header,
    app: App,
    pass: u64,
}

struct Job {
    ctx: egui::Context,
    bytes: Vec<u8>,
    dir: PathBuf,
    name: String,
    frame: u64,
    stable: Stable,
    snapshot_seq: u64,
}

/// The pending submission, the snapshot counter, the writer and its acknowledgements.
#[derive(Default)]
pub(crate) struct Hub {
    pending: Option<Pending>,
    seq: u64,
    /// The last `Before` state that no `After` has claimed: `(dir, run_id, capture_id, snapshot_seq)`.
    open_pair: Option<(PathBuf, Option<String>, String, u64)>,
    writer: Option<mpsc::Sender<Job>>,
    acks: Arc<Mutex<VecDeque<Ack>>>,
}

/// What the observation plugin knows at the end of a main-window pass.
pub(crate) struct Pass<'a> {
    pub frame: u64,
    /// egui throws this pass away and runs another.
    pub discard: bool,
    pub ppp: f32,
    /// The test IDs of this pass, or `None` when the pass brought no accessibility tree.
    pub tree: Option<TreeView<'a>>,
}

pub(crate) struct TreeView<'a> {
    pub ids: &'a BTreeMap<String, Result<crate::IdNode, String>>,
    pub viewport_rect: egui::Rect,
}

impl Hub {
    fn submit(&mut self, header: Header, app: App, pass: u64) -> Result<(), String> {
        if let Some(p) = &self.pending {
            if p.header.name != header.name {
                return Err(format!("state `{}` is still waiting to be written; cannot start `{}`", p.header.name, header.name));
            }
        }
        self.pending = Some(Pending { header, app, pass });
        Ok(())
    }

    fn take_ack(&mut self) -> Option<Ack> {
        self.acks.lock().ok()?.pop_front()
    }

    fn push(&self, ctx: &egui::Context, ack: Ack) {
        if let Ok(mut q) = self.acks.lock() {
            q.push_back(ack);
        }
        ctx.request_repaint();
    }

    pub(crate) fn merge(&mut self, ctx: &egui::Context, pass: Pass<'_>) {
        let Some(p) = &self.pending else { return };
        // A submission made during pass N is merged by the pass that ends with a number above N.
        if pass.frame <= p.pass || pass.discard {
            return;
        }
        let Some(p) = self.pending.take() else { return };
        let name = p.header.name.clone();
        let Some(tree) = pass.tree else {
            let why = format!("no accessibility tree in the pass (frame {}) that ends the state `{name}`", pass.frame);
            self.push(ctx, Ack::Failed { name, why });
            return;
        };
        let capture = p.header.capture_id.clone().unwrap_or_default();
        let key = (p.header.dir.clone(), p.header.run_id.clone());
        // An `After` takes the number of its `Before`; everything else takes the next one.
        let seq = match p.header.phase {
            Some(Phase::After) => match &self.open_pair {
                Some((d, r, c, n)) if *d == key.0 && *r == key.1 && *c == capture => *n,
                _ => {
                    let why = format!("state-after {name} has no matching state-before (capture_id {capture})");
                    self.push(ctx, Ack::Failed { name, why });
                    return;
                }
            },
            _ => self.seq + 1,
        };
        let built = build(&p.header, &p.app, pass.frame, seq, pass.ppp, &tree);
        let (bytes, stable) = match built {
            Ok(b) => b,
            Err(why) => {
                self.push(ctx, Ack::Failed { name, why });
                return;
            }
        };
        match p.header.phase {
            Some(Phase::After) => self.open_pair = None,
            Some(Phase::Before) => {
                self.seq = seq;
                self.open_pair = Some((key.0, key.1, capture, seq));
            }
            None => self.seq = seq,
        }
        let job =
            Job { ctx: ctx.clone(), bytes, dir: p.header.dir.clone(), name: name.clone(), frame: pass.frame, stable, snapshot_seq: seq };
        if let Err(why) = self.send(job) {
            self.push(ctx, Ack::Failed { name, why });
        }
    }

    fn send(&mut self, job: Job) -> Result<(), String> {
        if self.writer.is_none() {
            let (tx, rx) = mpsc::channel::<Job>();
            let acks = self.acks.clone();
            std::thread::Builder::new()
                .name("ito-keyscript-state".into())
                .spawn(move || {
                    for job in rx {
                        let ack = write(&job);
                        if let Ok(mut q) = acks.lock() {
                            q.push_back(ack);
                        }
                        job.ctx.request_repaint();
                    }
                })
                .map_err(|e| format!("cannot start the state writer: {e}"))?;
            self.writer = Some(tx);
        }
        let sent = self.writer.as_ref().is_some_and(|w| w.send(job).is_ok());
        if !sent {
            self.writer = None;
            return Err("the state writer has stopped".to_owned());
        }
        Ok(())
    }
}

fn write(job: &Job) -> Ack {
    let dir = job.dir.join("state");
    let path = dir.join(format!("{}.json", job.name));
    let part = dir.join(format!("{}.json.part", job.name));
    let done = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&dir)?;
        let mut f = std::fs::File::create(&part)?;
        std::io::Write::write_all(&mut f, &job.bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&part, &path)
    })();
    match done {
        Ok(()) => Ack::Written {
            name: job.name.clone(),
            path,
            sha256: sha256_hex(&job.bytes),
            stable: job.stable.clone(),
            frame: job.frame,
            snapshot_seq: job.snapshot_seq,
        },
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Ack::Failed { name: job.name.clone(), why: format!("cannot write {}: {e}", path.display()) }
        }
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Canonical JSON, for hashes only: keys sorted, no whitespace. `Value`'s own map order depends on a Cargo feature
/// (`preserve_order`) that other crates in the build may turn on, so it is not used.
pub(crate) fn canonical(v: &Value) -> String {
    fn go(v: &Value, out: &mut String) {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap_or_default());
                    out.push(':');
                    go(&m[k], out);
                }
                out.push('}');
            }
            Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    go(x, out);
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other).unwrap_or_default()),
        }
    }
    let mut s = String::new();
    go(v, &mut s);
    s
}

type Num = serde_json::Number;

/// An `f32` as the shortest number that reads back the same; non-finite values are an error, never `null`.
fn num(v: f32, what: &str) -> Result<Num, String> {
    if !v.is_finite() {
        return Err(format!("{what} is not a finite number ({v})"));
    }
    let wide: f64 = format!("{v}").parse().map_err(|_| format!("{what} cannot be written as a number ({v})"))?;
    Num::from_f64(wide).ok_or_else(|| format!("{what} cannot be written as a number ({v})"))
}

/// `[l, t, r, b]` in physical pixels, with the window's top-left as the origin. Not rounded.
fn px(r: egui::Rect, origin: egui::Pos2, ppp: f32, what: &str) -> Result<[Num; 4], String> {
    Ok([
        num((r.min.x - origin.x) * ppp, what)?,
        num((r.min.y - origin.y) * ppp, what)?,
        num((r.max.x - origin.x) * ppp, what)?,
        num((r.max.y - origin.y) * ppp, what)?,
    ])
}

#[derive(Serialize)]
struct UiOk {
    test_id: String,
    role: String,
    text: Option<String>,
    label: Option<String>,
    value: Option<String>,
    rect_px: [Num; 4],
    clip_px: Option<[Num; 4]>,
    visible: Option<bool>,
    elided: Option<bool>,
    enabled: bool,
    focused: bool,
    hidden: bool,
    layer: String,
}

#[derive(Serialize)]
struct UiErr {
    test_id: String,
    error: String,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Ui {
    Ok(Box<UiOk>),
    Err(UiErr),
}

#[derive(Serialize)]
struct AppInfo<'a> {
    name: &'a Option<String>,
    version: &'a Option<String>,
}

#[derive(Serialize)]
struct ThemeInfo<'a> {
    requested: &'a Option<String>,
    name: Option<String>,
    dark: Option<bool>,
    tokens: Option<BTreeMap<String, String>>,
    tokens_sha256: Option<String>,
}

#[derive(Serialize)]
struct LangInfo<'a> {
    requested: &'a Option<String>,
    in_force: Option<String>,
}

#[derive(Serialize)]
struct WindowInfo {
    inner_px: [Num; 4],
    pixels_per_point: Num,
    monitor_work_area_px: Option<[Num; 4]>,
}

#[derive(Serialize)]
struct StableOut<'a> {
    projection: u32,
    ui_sha256: &'a str,
    app_sha256: &'a str,
}

#[derive(Serialize)]
struct Doc<'a> {
    schema_version: u32,
    name: &'a str,
    run_id: &'a Option<String>,
    snapshot_seq: u64,
    phase: Option<String>,
    capture_id: Option<String>,
    after_step: Option<usize>,
    after_label: &'a Option<String>,
    frame: u64,
    settled: Option<bool>,
    app: AppInfo<'a>,
    theme: ThemeInfo<'a>,
    lang: LangInfo<'a>,
    window: WindowInfo,
    screen: &'a Option<String>,
    overlay: Option<&'a String>,
    toasts: &'a Option<Vec<Toast>>,
    app_state: &'a Option<Value>,
    ui: Vec<Ui>,
    capabilities: Vec<&'static str>,
    stable: StableOut<'a>,
}

/// `app_sha256`: the app's part plus the capabilities, so "unknown" and "known none" hash differently.
fn app_hash(app: &App, caps: &[&str]) -> String {
    let overlay = app.overlay.as_ref().and_then(|o| o.as_ref());
    let value = serde_json::json!({
        "screen": app.screen,
        "overlay": overlay,
        "toasts": app.toasts,
        "app_state": app.app_state,
        "capabilities": caps,
    });
    sha256_hex(canonical(&value).as_bytes())
}

fn build(h: &Header, app: &App, frame: u64, seq: u64, ppp: f32, tree: &TreeView<'_>) -> Result<(Vec<u8>, Stable), String> {
    let vp = tree.viewport_rect;
    let mut ui = Vec::new();
    for (name, node) in tree.ids {
        match node {
            Err(why) => ui.push(Ui::Err(UiErr { test_id: name.clone(), error: why.clone() })),
            Ok(n) => {
                let rect_px = px(n.rect, vp.min, ppp, &format!("rect of `{name}`"))?;
                let clip_px = n.clip.map(|c| px(c, vp.min, ppp, &format!("clip of `{name}`"))).transpose()?;
                // Other layers on top of the widget are not considered.
                let visible = n.clip.map(|c| {
                    let seen = n.rect.intersect(c).intersect(vp);
                    seen.width() > 0.0 && seen.height() > 0.0
                });
                ui.push(Ui::Ok(Box::new(UiOk {
                    test_id: name.clone(),
                    role: n.role.clone(),
                    text: n.label.clone().or_else(|| n.value.clone()),
                    label: n.label.clone(),
                    value: n.value.clone(),
                    rect_px,
                    clip_px,
                    visible,
                    elided: n.elided,
                    enabled: n.enabled,
                    focused: n.focused,
                    hidden: n.hidden,
                    layer: format!("{:?}", n.layer.order),
                })));
            }
        }
    }
    let ui_value = serde_json::to_value(&ui).map_err(|e| format!("cannot serialize ui: {e}"))?;
    let ui_sha256 = sha256_hex(canonical(&ui_value).as_bytes());

    let theme = match ito_theme::name() {
        Some(name) => {
            let c = ito_theme::colors();
            let tokens: BTreeMap<String, String> = c.tokens().iter().map(|(k, v)| ((*k).to_owned(), ito_theme::rgba_hex(*v))).collect();
            let value = serde_json::to_value(&tokens).map_err(|e| format!("cannot serialize the theme tokens: {e}"))?;
            let hash = sha256_hex(canonical(&value).as_bytes());
            ThemeInfo {
                requested: &h.theme_requested,
                name: Some(name),
                dark: Some(!c.light),
                tokens: Some(tokens),
                tokens_sha256: Some(hash),
            }
        }
        None => ThemeInfo { requested: &h.theme_requested, name: None, dark: None, tokens: None, tokens_sha256: None },
    };
    let lang = LangInfo { requested: &h.lang_requested, in_force: ito_i18n::in_force() };

    let mut caps = vec!["ui", "window"];
    if theme.name.is_some() {
        caps.push("theme");
    }
    if lang.in_force.is_some() {
        caps.push("lang");
    }
    if app.screen.is_some() {
        caps.push("screen");
    }
    if app.overlay.is_some() {
        caps.push("overlay");
    }
    if app.toasts.is_some() {
        caps.push("toasts");
    }
    if app.app_state.is_some() {
        caps.push("app_state");
    }
    caps.sort_unstable();

    let overlay = app.overlay.as_ref().and_then(|o| o.as_ref());
    let app_sha256 = app_hash(app, &caps);

    let window = WindowInfo {
        inner_px: [
            num(0.0, "window")?,
            num(0.0, "window")?,
            num(vp.width() * ppp, "window width")?,
            num(vp.height() * ppp, "window height")?,
        ],
        pixels_per_point: num(ppp, "pixels_per_point")?,
        monitor_work_area_px: None,
    };
    let doc = Doc {
        schema_version: 1,
        name: &h.name,
        run_id: &h.run_id,
        snapshot_seq: seq,
        phase: h.phase.map(|p| p.text().to_owned()),
        capture_id: h.capture_id.clone(),
        after_step: h.after_step,
        after_label: &h.after_label,
        frame,
        settled: h.settled,
        app: AppInfo { name: &h.app_name, version: &h.app_version },
        theme,
        lang,
        window,
        screen: &app.screen,
        overlay,
        toasts: &app.toasts,
        app_state: &app.app_state,
        ui,
        capabilities: caps,
        stable: StableOut { projection: 1, ui_sha256: &ui_sha256, app_sha256: &app_sha256 },
    };
    let mut bytes = serde_json::to_vec_pretty(&doc).map_err(|e| format!("cannot serialize the state: {e}"))?;
    bytes.push(b'\n');
    Ok((bytes, Stable { projection: 1, ui_sha256, app_sha256 }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn window() -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() }
    }

    fn pass(ctx: &egui::Context, f: &mut dyn FnMut(&mut egui::Ui)) {
        ctx.run_ui(window(), |ui| f(ui)).drop_without_applying_deltas();
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-state-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn header(dir: &std::path::Path, name: &str) -> Header {
        Header { dir: dir.to_path_buf(), name: name.into(), run_id: Some("r1".into()), ..Default::default() }
    }

    fn wait_ack(ctx: &egui::Context) -> Ack {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(a) = take_ack(ctx) {
                return a;
            }
            assert!(Instant::now() < end, "no ack within 5 s");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn read_json(dir: &std::path::Path, name: &str) -> (Vec<u8>, Value) {
        let bytes = std::fs::read(dir.join("state").join(format!("{name}.json"))).unwrap();
        let v = serde_json::from_slice(&bytes).unwrap();
        (bytes, v)
    }

    fn ui_of<'a>(v: &'a Value, name: &str) -> &'a Value {
        v["ui"].as_array().unwrap().iter().find(|e| e["test_id"] == name).unwrap_or_else(|| panic!("no ui entry {name}"))
    }

    #[test]
    fn canonical_json_ignores_insertion_order() {
        let a: Value = serde_json::from_str(r#"{"b":1,"a":{"y":[1,2],"x":null}}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":{"x":null,"y":[1,2]},"b":1}"#).unwrap();
        assert_eq!(canonical(&a), canonical(&b));
        assert_eq!(canonical(&a), r#"{"a":{"x":null,"y":[1,2]},"b":1}"#);
        let c: Value = serde_json::from_str(r#"{"b":2,"a":{"y":[1,2],"x":null}}"#).unwrap();
        assert_ne!(canonical(&a), canonical(&c));
    }

    #[test]
    fn submit_refuses_without_a_tap_a_bad_name_and_a_second_name() {
        let ctx = egui::Context::default();
        let e = submit(&ctx, header(&temp("none"), "a"), App::default()).unwrap_err();
        assert!(e.contains("no observation"), "{e}");
        crate::observe(&ctx);
        for bad in ["", "A b", "a/b", "a.b"] {
            assert!(submit(&ctx, header(&temp("bad"), bad), App::default()).is_err(), "{bad}");
        }
        submit(&ctx, header(&temp("two"), "a"), App::default()).unwrap();
        submit(&ctx, header(&temp("two"), "a"), App::default()).unwrap();
        let e = submit(&ctx, header(&temp("two"), "b"), App::default()).unwrap_err();
        assert!(e.contains("still waiting"), "{e}");
    }

    #[test]
    fn a_state_is_merged_with_the_tree_of_the_pass_and_written() {
        let dir = temp("write");
        let ctx = egui::Context::default();
        crate::observe(&ctx);
        ctx.set_pixels_per_point(2.0);
        let mut draw = |ui: &mut egui::Ui| {
            let r = ui.button("plain");
            ito_a11y::test_id(&r, "plain");
            let r = ui.button("half");
            let clip = egui::Rect::from_min_max(r.rect.min, egui::pos2(r.rect.center().x, r.rect.max.y));
            ui.scope(|ui| {
                ui.set_clip_rect(clip);
                ito_a11y::test_id_in(ui, &r, "half", None);
            });
            let r = ui.button("gone");
            ui.scope(|ui| {
                ui.set_clip_rect(egui::Rect::from_min_size(egui::pos2(300.0, 250.0), egui::vec2(10.0, 10.0)));
                ito_a11y::test_id_in(ui, &r, "gone", None);
            });
            let r = ui.label("cut");
            let mut job = egui::text::LayoutJob::single_section("a long label that has to be cut".into(), egui::TextFormat::default());
            job.wrap.max_width = 20.0;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            let cut = ui.fonts_mut(|f| f.layout_job(job));
            ito_a11y::test_id_in(ui, &r, "cut", Some(&cut));
        };
        pass(&ctx, &mut draw);
        let app = App { screen: Some("main".into()), overlay: Some(None), toasts: None, app_state: Some(serde_json::json!({"n": 1})) };
        submit(&ctx, header(&dir, "one"), app.clone()).unwrap();
        pass(&ctx, &mut draw);
        let Ack::Written { name, path, sha256, stable, .. } = wait_ack(&ctx) else { panic!("not written") };
        assert_eq!(name, "one");
        assert!(!dir.join("state").join("one.json.part").exists());
        let (bytes, v) = read_json(&dir, "one");
        assert_eq!(path, dir.join("state").join("one.json"));
        assert_eq!(sha256, sha256_hex(&bytes));
        assert_eq!(*bytes.last().unwrap(), b'\n');
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["snapshot_seq"], 1);
        assert_eq!(v["run_id"], "r1");
        assert_eq!(v["window"]["pixels_per_point"], 2.0);
        assert_eq!(v["window"]["inner_px"], serde_json::json!([0.0, 0.0, 800.0, 600.0]));
        assert_eq!(v["stable"]["ui_sha256"], stable.ui_sha256.as_str());
        assert_eq!(v["stable"]["app_sha256"], stable.app_sha256.as_str());

        // Rectangles are the widget's points times 2, from the window's top-left.
        let obs = crate::observation(&ctx).unwrap();
        let plain = obs.id("plain").unwrap().unwrap();
        let rp = ui_of(&v, "plain")["rect_px"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect::<Vec<_>>();
        assert_eq!(rp, vec![plain.rect.min.x * 2.0, plain.rect.min.y * 2.0, plain.rect.max.x * 2.0, plain.rect.max.y * 2.0]);
        assert!(ui_of(&v, "plain")["visible"].is_null());
        assert!(ui_of(&v, "plain")["clip_px"].is_null());
        assert_eq!(ui_of(&v, "half")["visible"], true);
        assert!(ui_of(&v, "half")["clip_px"].is_array());
        assert_eq!(ui_of(&v, "gone")["visible"], false);
        assert_eq!(ui_of(&v, "cut")["elided"], true);
        assert_eq!(ui_of(&v, "plain")["label"], "plain");
        assert_eq!(ui_of(&v, "plain")["text"], "plain");

        // Capabilities: what the app gave, what ito knows.
        // `theme` and `lang` follow process-wide state that other tests change, so they are not compared.
        let caps: Vec<&str> =
            v["capabilities"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).filter(|c| !matches!(*c, "theme" | "lang")).collect();
        assert_eq!(caps, vec!["app_state", "overlay", "screen", "ui", "window"]);
        assert_eq!(v["screen"], "main");
        assert!(v["overlay"].is_null());
        assert!(v["toasts"].is_null());

        submit(&ctx, header(&dir, "two"), App::default()).unwrap();
        pass(&ctx, &mut draw);
        assert!(matches!(wait_ack(&ctx), Ack::Written { .. }));
        assert_eq!(read_json(&dir, "two").1["snapshot_seq"], 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn staged(dir: &std::path::Path, name: &str, run: &str, phase: Option<Phase>, capture: Option<&str>) -> Header {
        Header { run_id: Some(run.into()), phase, capture_id: capture.map(str::to_owned), ..header(dir, name) }
    }

    /// Submit `h`, run the pass that merges it and return what the writer answered.
    fn write_one(ctx: &egui::Context, draw: &mut dyn FnMut(&mut egui::Ui), h: Header) -> Ack {
        submit(ctx, h, App::default()).unwrap();
        pass(ctx, draw);
        wait_ack(ctx)
    }

    fn seq_of(ack: &Ack) -> u64 {
        match ack {
            Ack::Written { snapshot_seq, .. } => *snapshot_seq,
            Ack::Failed { name, why } => panic!("{name} failed: {why}"),
        }
    }

    #[test]
    fn snapshot_numbers_plain_states_pairs_and_failures() {
        let dir = temp("seq");
        let ctx = egui::Context::default();
        crate::observe(&ctx);
        let mut draw = |ui: &mut egui::Ui| {
            let r = ui.button("b");
            ito_a11y::test_id(&r, "b");
        };
        pass(&ctx, &mut draw);
        // Plain states count 1, 2.
        assert_eq!(seq_of(&write_one(&ctx, &mut draw, staged(&dir, "p1", "r1", None, None))), 1);
        assert_eq!(seq_of(&write_one(&ctx, &mut draw, staged(&dir, "p2", "r1", None, None))), 2);
        // A pair shares one number, and the next plain state goes on from it.
        let b = write_one(&ctx, &mut draw, staged(&dir, "a-before", "r1", Some(Phase::Before), Some("1-a")));
        let a = write_one(&ctx, &mut draw, staged(&dir, "a-after", "r1", Some(Phase::After), Some("1-a")));
        assert_eq!((seq_of(&b), seq_of(&a)), (3, 3));
        let (_, vb) = read_json(&dir, "a-before");
        let (_, va) = read_json(&dir, "a-after");
        assert_eq!((vb["phase"].as_str(), va["phase"].as_str()), (Some("before"), Some("after")));
        assert_eq!(vb["capture_id"], "1-a");
        assert_eq!(va["capture_id"], "1-a");
        assert_eq!(seq_of(&write_one(&ctx, &mut draw, staged(&dir, "p3", "r1", None, None))), 4);
        // An unknown capture_id, and the right one from another run, fail and take no number.
        write_one(&ctx, &mut draw, staged(&dir, "b-before", "r1", Some(Phase::Before), Some("2-b")));
        for (run, cap) in [("r1", "9-x"), ("r2", "2-b")] {
            let ack = write_one(&ctx, &mut draw, staged(&dir, "b-after", run, Some(Phase::After), Some(cap)));
            assert!(matches!(&ack, Ack::Failed { why, .. } if why.contains("no matching state-before")), "{run} {cap}");
        }
        // The pair is still open for its own after state.
        assert_eq!(seq_of(&write_one(&ctx, &mut draw, staged(&dir, "b-after", "r1", Some(Phase::After), Some("2-b")))), 5);
        // A second before replaces an abandoned pair.
        write_one(&ctx, &mut draw, staged(&dir, "c-before", "r1", Some(Phase::Before), Some("3-c")));
        let n = seq_of(&write_one(&ctx, &mut draw, staged(&dir, "d-before", "r1", Some(Phase::Before), Some("4-d"))));
        assert_eq!(n, 7);
        let ack = write_one(&ctx, &mut draw, staged(&dir, "c-after", "r1", Some(Phase::After), Some("3-c")));
        assert!(matches!(ack, Ack::Failed { .. }));
        assert_eq!(seq_of(&write_one(&ctx, &mut draw, staged(&dir, "d-after", "r1", Some(Phase::After), Some("4-d")))), 7);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn submit_refuses_a_phase_without_a_capture_id() {
        let ctx = egui::Context::default();
        crate::observe(&ctx);
        for capture in [None, Some("")] {
            let e = submit(&ctx, staged(&temp("nocap"), "a", "r1", Some(Phase::Before), capture), App::default()).unwrap_err();
            assert!(e.contains("capture_id"), "{e}");
        }
    }

    #[test]
    fn app_hash_follows_the_app_part_not_the_key_order() {
        let with = |state: &str| App { app_state: Some(serde_json::from_str(state).unwrap()), ..Default::default() };
        let caps = ["app_state", "ui", "window"];
        let a = app_hash(&with(r#"{"a":1,"b":2}"#), &caps);
        assert_eq!(a, app_hash(&with(r#"{"b":2,"a":1}"#), &caps));
        assert_ne!(a, app_hash(&with(r#"{"a":1,"b":3}"#), &caps));
        // Unknown and known-none differ through the capabilities.
        let none = App { overlay: Some(None), ..Default::default() };
        assert_ne!(app_hash(&App::default(), &["ui", "window"]), app_hash(&none, &["overlay", "ui", "window"]));
    }

    #[test]
    fn a_discarded_pass_hands_the_state_to_the_next_one() {
        let dir = temp("discard");
        let ctx = egui::Context::default();
        crate::observe(&ctx);
        ctx.options_mut(|o| o.max_passes = 2.try_into().unwrap());
        pass(&ctx, &mut |_| {});
        submit(&ctx, header(&dir, "d"), App::default()).unwrap();
        let mut n = 0;
        ctx.run_ui(window(), |ui| {
            n += 1;
            if ui.ctx().cumulative_pass_nr() == ui.ctx().cumulative_pass_nr_for(egui::ViewportId::ROOT) && n == 1 {
                ui.ctx().request_discard("test");
                let r = ui.button("first");
                ito_a11y::test_id(&r, "first");
            } else {
                let r = ui.button("second");
                ito_a11y::test_id(&r, "second");
            }
        })
        .drop_without_applying_deltas();
        assert_eq!(n, 2, "the first pass was discarded");
        assert!(matches!(wait_ack(&ctx), Ack::Written { .. }));
        let (_, v) = read_json(&dir, "d");
        let ids: Vec<&str> = v["ui"].as_array().unwrap().iter().map(|e| e["test_id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["second"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rect_that_is_not_finite_fails_and_writes_nothing() {
        let dir = temp("nan");
        let ctx = egui::Context::default();
        crate::observe(&ctx);
        let mut draw = |ui: &mut egui::Ui| {
            let mut r = ui.button("bad");
            r.rect = egui::Rect::from_min_max(egui::pos2(f32::NAN, 0.0), egui::pos2(10.0, 10.0));
            ito_a11y::test_id(&r, "bad");
        };
        pass(&ctx, &mut draw);
        submit(&ctx, header(&dir, "n"), App::default()).unwrap();
        pass(&ctx, &mut draw);
        match wait_ack(&ctx) {
            Ack::Failed { name, why } => {
                assert_eq!(name, "n");
                assert!(why.contains("bad"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        assert!(!dir.join("state").exists());
    }

    #[test]
    fn a_pass_without_a_tree_fails() {
        let dir = temp("notree");
        let ctx = egui::Context::default();
        // The plugin without `enable_accesskit`: egui brings no tree.
        ctx.add_plugin(crate::Tap::default());
        pass(&ctx, &mut |_| {});
        submit(&ctx, header(&dir, "t"), App::default()).unwrap();
        pass(&ctx, &mut |_| {});
        match wait_ack(&ctx) {
            Ack::Failed { why, .. } => assert!(why.contains("no accessibility tree"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    /// Writes one state through the real writer and returns the file's text.
    fn written(dir: &std::path::Path, h: Header, app: App, draw: &mut dyn FnMut(&mut egui::Ui)) -> String {
        let ctx = egui::Context::default();
        crate::observe(&ctx);
        ctx.set_pixels_per_point(2.0);
        pass(&ctx, draw);
        let name = h.name.clone();
        submit(&ctx, h, app).unwrap();
        pass(&ctx, draw);
        assert!(matches!(wait_ack(&ctx), Ack::Written { .. }));
        std::fs::read_to_string(dir.join("state").join(format!("{name}.json"))).unwrap()
    }

    /// The files in `docs/schemas/examples/` have the shape (keys, order, types) of what the writer writes.
    #[test]
    fn the_written_state_has_the_shape_of_the_examples() {
        let _g = crate::shape::GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        ito_theme::set_theme(&ito_theme::builtin()[0]);
        ito_i18n::set_in_force("ja");
        let dir = temp("shape");
        let mut draw = |ui: &mut egui::Ui| {
            let r = ui.label("cut");
            let mut job = egui::text::LayoutJob::single_section("a long label that has to be cut".into(), egui::TextFormat::default());
            job.wrap.max_width = 20.0;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            let cut = ui.fonts_mut(|f| f.layout_job(job));
            let clip = egui::Rect::from_min_max(r.rect.min, egui::pos2(r.rect.center().x, r.rect.max.y));
            ui.scope(|ui| {
                ui.set_clip_rect(clip);
                ito_a11y::test_id_in(ui, &r, "a_cut", Some(&cut));
            });
            let r = ui.button("plain");
            ito_a11y::test_id(&r, "b_plain");
        };
        let app = App {
            screen: Some("main".into()),
            overlay: Some(Some("palette".into())),
            toasts: Some(vec![Toast { text: "saved".into(), kind: "info".into() }]),
            app_state: Some(serde_json::json!({"tab": 1})),
        };
        let plain = written(&dir, header(&dir, "plain"), app.clone(), &mut draw);
        crate::shape::check("state", &plain, crate::shape::example!("state"));

        let h = Header {
            after_step: Some(3),
            after_label: Some("<Click:x>".into()),
            settled: Some(true),
            app_name: Some("demo".into()),
            app_version: Some("1.2.3".into()),
            theme_requested: Some("ito-dark".into()),
            lang_requested: Some("ja".into()),
            phase: Some(Phase::Before),
            capture_id: Some("1-a".into()),
            ..header(&dir, "a-before")
        };
        let before = written(&dir, h, app, &mut draw);
        crate::shape::check("state-before", &before, crate::shape::example!("state-before"));

        // Two widgets with one test id: the entry is an error, not a rectangle.
        let mut dup = |ui: &mut egui::Ui| {
            let r = ui.button("one");
            ito_a11y::test_id(&r, "dup");
            let r = ui.button("two");
            ito_a11y::test_id(&r, "dup");
        };
        let err = written(&dir, header(&dir, "err"), App::default(), &mut dup);
        crate::shape::check("state-error-entry", &err, crate::shape::example!("state-error-entry"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
