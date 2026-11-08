//! Accessibility the apps share.
//!
//! - **Not by colour alone.** A state that is shown only by a colour (a
//!   selected row, the focused pane, a conflicting file) is lost on a
//!   colour-blind eye or a grey screen. [`State`] names the states every app
//!   has and gives each a [`mark`](State::mark) (a glyph put beside the text)
//!   and a [`name`](State::name) (what a screen reader says, English and
//!   Japanese). The marks are always safe to draw; with `high_contrast = true`
//!   in common.toml (`ito_common::Common::high_contrast_or_off`) the app draws
//!   them on every row, otherwise only where colour is not enough.
//! - **Colours** of the high-contrast variant: `ito_theme::Colors::high_contrast`.
//! - **A screen reader reads what egui paints** only when the painted text is
//!   given a label. With the `egui` feature, [`say`] gives a response that
//!   label and a role, and [`announce`] tells the reader about a change that
//!   has no widget (a toast, a finished job). The message is a live region, so
//!   the reader speaks it without the user moving focus. [`announce_toast`]
//!   speaks a toast once per raise ([`Told`]), with its level in words
//!   ([`toast_line`]).

/// A state an app shows on a row, a pane, a tab or a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Selected,
    Focused,
    Conflict,
    Error,
    Warning,
    Done,
    Running,
    Disabled,
    /// Will start when its time, file or other work comes.
    Waiting,
    /// Stopped by a person until released.
    Held,
    /// Passed on purpose (overlap, "skip next", a person pressed skip).
    Skipped,
    /// The planned time went by while nothing could run (the PC was off or asleep).
    Missed,
    /// The flow did not reach it (after an upstream error, say). Not [`State::Disabled`], which is "unavailable".
    NotRun,
    /// Not run because something it needs is not approved yet.
    Blocked,
}

impl State {
    /// The glyph for the state, drawn before the text. Single-width, in the
    /// bundled mono font and in common system fonts.
    pub fn mark(self) -> &'static str {
        match self {
            State::Selected => "●",
            State::Focused => "▶",
            State::Conflict => "≠",
            State::Error => "✖",
            State::Warning => "▲",
            State::Done => "✔",
            State::Running => "…",
            State::Disabled => "–",
            State::Waiting => "○",
            State::Held => "‖",
            State::Skipped => "»",
            State::Missed => "↷",
            State::NotRun => "·",
            State::Blocked => "⊘",
        }
    }

    /// What a screen reader says (`ja`, else English).
    pub fn name(self, lang: &str) -> &'static str {
        let ja = lang == "ja";
        match (self, ja) {
            (State::Selected, false) => "selected",
            (State::Selected, true) => "選択中",
            (State::Focused, false) => "focused",
            (State::Focused, true) => "フォーカス中",
            (State::Conflict, false) => "conflict",
            (State::Conflict, true) => "競合",
            (State::Error, false) => "error",
            (State::Error, true) => "エラー",
            (State::Warning, false) => "warning",
            (State::Warning, true) => "注意",
            (State::Done, false) => "done",
            (State::Done, true) => "完了",
            (State::Running, false) => "running",
            (State::Running, true) => "実行中",
            (State::Disabled, false) => "unavailable",
            (State::Disabled, true) => "使えません",
            (State::Waiting, false) => "waiting",
            (State::Waiting, true) => "実行待ち",
            (State::Held, false) => "held",
            (State::Held, true) => "保留",
            (State::Skipped, false) => "skipped",
            (State::Skipped, true) => "スキップ",
            (State::Missed, false) => "missed",
            (State::Missed, true) => "時刻を過ぎた",
            (State::NotRun, false) => "not run",
            (State::NotRun, true) => "未実行",
            (State::Blocked, false) => "blocked",
            (State::Blocked, true) => "未確認で止めた",
        }
    }

    /// `text` with the mark before it (`● main.rs`).
    pub fn marked(self, text: &str) -> String {
        format!("{} {text}", self.mark())
    }

    /// `text` as a screen reader says it: the text, then the states
    /// (`main.rs, selected, conflict`).
    pub fn spoken(text: &str, states: &[State], lang: &str) -> String {
        let mut s = text.to_owned();
        for st in states {
            s.push_str(", ");
            s.push_str(st.name(lang));
        }
        s
    }
}

/// What a painted thing is, for the screen reader.
#[cfg(feature = "egui")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Label,
    Button,
    /// A row of a list the user moves through.
    Row,
    Heading,
}

/// Give `resp` (the response of an area the app painted or interacted with)
/// a screen-reader `label` and `role`. Call it for text drawn by a painter,
/// which egui cannot name by itself.
#[cfg(feature = "egui")]
pub fn say(resp: &egui::Response, role: Role, label: &str) {
    use egui::{WidgetInfo, WidgetType};
    let kind = match role {
        Role::Label | Role::Heading => WidgetType::Label,
        Role::Button => WidgetType::Button,
        Role::Row => WidgetType::SelectableLabel,
    };
    let enabled = resp.enabled();
    resp.widget_info(|| WidgetInfo::labeled(kind, enabled, label));
}

static RECORD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The kept messages with their serial numbers, and the serial of the last one.
struct Heard {
    count: u64,
    kept: Vec<(u64, String)>,
}

static HEARD: std::sync::Mutex<Heard> = std::sync::Mutex::new(Heard { count: 0, kept: Vec::new() });

/// Keep the messages given to `announce` (a `--keys` run turns this on, for `<Announced:text>`). Off by default, so a
/// shipped app keeps nothing.
pub fn record_announcements(on: bool) {
    RECORD.store(on, std::sync::atomic::Ordering::SeqCst);
}

/// The messages `announce`d since [`record_announcements`] was turned on, oldest first (the last 200).
pub fn announced() -> Vec<String> {
    HEARD.lock().unwrap_or_else(|e| e.into_inner()).kept.iter().map(|(_, m)| m.clone()).collect()
}

/// The serial number of the last recorded message: 1 for the first one in the process, counting up, never reset (0
/// before any). Only messages given while [`record_announcements`] is on are counted.
pub fn announced_count() -> u64 {
    HEARD.lock().unwrap_or_else(|e| e.into_inner()).count
}

/// The kept messages recorded after serial `count` (as [`announced_count`] returned it), oldest first. With more than
/// 200 messages after `count`, only the last 200 are kept.
pub fn announced_after(count: u64) -> Vec<String> {
    HEARD.lock().unwrap_or_else(|e| e.into_inner()).kept.iter().filter(|(n, _)| *n > count).map(|(_, m)| m.clone()).collect()
}

#[cfg(feature = "egui")]
fn heard(message: &str) {
    if RECORD.load(std::sync::atomic::Ordering::SeqCst) {
        let mut h = HEARD.lock().unwrap_or_else(|e| e.into_inner());
        if h.kept.len() >= 200 {
            h.kept.remove(0);
        }
        h.count += 1;
        let n = h.count;
        h.kept.push((n, message.to_owned()));
    }
}

/// How soon the reader should speak an announcement.
#[cfg(feature = "egui")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Urgency {
    /// Wait for the reader to finish what it is saying.
    #[default]
    Polite,
    /// Interrupt the reader (an error the user must not miss).
    Assertive,
}

/// Tell the screen reader about a change that has no widget (a toast, a job
/// finished), politely: it waits for the reader to finish what it is saying.
/// Same as [`announce_with`] with [`Urgency::Polite`].
///
/// The message is a hidden label marked as a live region, and it exists only
/// for the frame this is called in. The reader speaks when such a node is
/// added or its text changes, so calling it again in the very next frame with
/// the same text at the same place is not a new message. Call it once per
/// message ([`announce_toast`] does that for toasts).
#[cfg(feature = "egui")]
pub fn announce(ui: &mut egui::Ui, message: &str) {
    announce_with(ui, message, Urgency::Polite);
}

/// Like [`announce`], with the urgency chosen by the caller.
#[cfg(feature = "egui")]
pub fn announce_with(ui: &mut egui::Ui, message: &str, urgency: Urgency) {
    use egui::accesskit::Live;
    heard(message);
    let resp = ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
    say(&resp, Role::Label, message);
    let live = match urgency {
        Urgency::Polite => Live::Polite,
        Urgency::Assertive => Live::Assertive,
    };
    ui.ctx().accesskit_node_builder(resp.id, |n| n.set_live(live));
}

/// The line a screen reader gets for a toast: the text, with the level in
/// words in front when there is one (`注意: 保存できません`, `error: disk full`).
pub fn toast_line(level: Option<State>, text: &str, lang: &str) -> String {
    match level {
        None => text.to_owned(),
        Some(s) => format!("{}: {text}", s.name(lang)),
    }
}

/// The shortest time between two spoken toasts. A burst is not read out in a batch.
const REPEAT_GAP: std::time::Duration = std::time::Duration::from_secs(1);

/// What the screen reader was told about one toast. The app keeps it next to the toast (not `Copy`: a copy would be
/// updated instead of the kept one, and the toast spoken again).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Told {
    raises: u32,
    at: Option<std::time::Instant>,
    line: u64,
}

impl Told {
    /// Whether to speak now. `raises` counts how many times the app has raised this toast so far (1 for a new toast).
    /// A raise already counted is not new. A new raise is spoken at once the first time, and later only when the last
    /// spoken one is at least a second old; raises skipped inside that second are marked told and never spoken later.
    pub fn due(&mut self, raises: u32, now: std::time::Instant) -> bool {
        if raises <= self.raises {
            return false;
        }
        let speak = match self.at {
            None => true,
            Some(at) => now.checked_duration_since(at).is_some_and(|d| d >= REPEAT_GAP),
        };
        self.raises = raises;
        if speak {
            self.at = Some(now);
        }
        speak
    }

    /// Forget everything if `line` is not the line told last: another text or level is another toast.
    #[cfg_attr(not(feature = "egui"), allow(dead_code))]
    fn follow(&mut self, line: &str) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        line.hash(&mut h);
        let hash = h.finish();
        if self.line != hash {
            *self = Self { raises: 0, at: None, line: hash };
        }
    }
}

/// Tell the screen reader about a toast, once per raise: [`toast_line`] as a
/// live message, assertive for [`State::Error`] and polite otherwise. `text`
/// has no `x2` count. `raises` is how many times the app has raised this toast
/// (1 for a new one); `told` is kept with the toast. A different line starts
/// over, so it is spoken at once. Returns whether it spoke.
#[cfg(feature = "egui")]
pub fn announce_toast(
    ui: &mut egui::Ui,
    told: &mut Told,
    raises: u32,
    level: Option<State>,
    text: &str,
    lang: &str,
    now: std::time::Instant,
) -> bool {
    let line = toast_line(level, text, lang);
    told.follow(&line);
    if !told.due(raises, now) {
        return false;
    }
    let urgency = if level == Some(State::Error) { Urgency::Assertive } else { Urgency::Polite };
    announce_with(ui, &line, urgency);
    true
}

/// A test ID: lowercase ASCII letters, digits, `_` and `-`, in one or more parts joined by `.`
/// (`settings.shell.env.add`). `^[a-z0-9_-]+(\.[a-z0-9_-]+)*$`.
pub fn is_test_id(name: &str) -> bool {
    !name.is_empty() && name.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-')))
}

/// Where a test ID was drawn, as [`test_id`] noted it.
#[cfg(feature = "egui")]
#[derive(Clone, Debug, PartialEq)]
pub struct TestIdEntry {
    pub name: String,
    pub egui_id: egui::Id,
    /// `resp.rect`, in the layer's own coordinates.
    pub rect: egui::Rect,
    /// `resp.interact_rect`, in the layer's own coordinates.
    pub interact_rect: egui::Rect,
    pub layer: egui::LayerId,
    /// `resp.enabled()`.
    pub enabled: bool,
    /// `ctx.viewport_id()` at the call.
    pub viewport: egui::ViewportId,
    /// `ctx.cumulative_pass_nr()` at the call.
    pub pass: u64,
    /// `ui.clip_rect()` at the call, in the layer's own coordinates; `None` from [`test_id`].
    pub clip: Option<egui::Rect>,
    /// `Galley::elided` of the text drawn; `None` when [`test_id`] was used or no galley was given.
    pub elided: Option<bool>,
}

#[cfg(feature = "egui")]
#[derive(Clone, Default)]
struct TestIds(std::collections::HashMap<egui::ViewportId, (u64, Vec<TestIdEntry>)>);

#[cfg(feature = "egui")]
fn test_ids_key() -> egui::Id {
    egui::Id::new("ito-a11y.test-ids")
}

/// Give the drawn `resp` a test ID that `--keys` can find (`<ClickId:name>`, `<Id:name>`...): AccessKit's `author_id` on its node,
/// and a note of where it was drawn. Call it after the widget is drawn, every frame it is shown. A name that is not
/// [`is_test_id`] is not attached (and trips a `debug_assert!`, AccessKit on or off). Otherwise it does nothing while
/// AccessKit is off (a shipped app pays nothing).
#[cfg(feature = "egui")]
pub fn test_id(resp: &egui::Response, name: &str) {
    register_test_id(resp, name, None, None);
}

/// [`test_id`], plus what only the `Ui` knows: its clip rectangle (`clip`), and whether the text `galley` was cut
/// off (`elided`). `--keys` writes both into the state JSON (`clip_px`, `visible`, `elided`). `Response::interact_rect`
/// is not a clip (menus widen it, `union` merges it), which is why the clip comes from the `Ui`. Pass the galley of the
/// widget's text, or `None` when it has none.
#[cfg(feature = "egui")]
pub fn test_id_in(ui: &egui::Ui, resp: &egui::Response, name: &str, galley: Option<&egui::Galley>) {
    register_test_id(resp, name, Some(ui.clip_rect()), galley.map(|g| g.elided));
}

#[cfg(feature = "egui")]
fn register_test_id(resp: &egui::Response, name: &str, clip: Option<egui::Rect>, elided: Option<bool>) {
    if !is_test_id(name) {
        debug_assert!(false, "`{name}` is not a test ID: lowercase letters, digits, - and _, in parts joined by `.`");
        return;
    }
    let on = resp.ctx.accesskit_node_builder(resp.id, |n| n.set_author_id(name)).is_some();
    if !on {
        return;
    }
    let entry = TestIdEntry {
        name: name.to_owned(),
        egui_id: resp.id,
        rect: resp.rect,
        interact_rect: resp.interact_rect,
        layer: resp.layer_id,
        enabled: resp.enabled(),
        viewport: resp.ctx.viewport_id(),
        pass: resp.ctx.cumulative_pass_nr(),
        clip,
        elided,
    };
    resp.ctx.data_mut(|d| {
        let store = d.get_temp_mut_or_default::<TestIds>(test_ids_key());
        let slot = store.0.entry(entry.viewport).or_default();
        if slot.0 != entry.pass {
            slot.1.clear();
            slot.0 = entry.pass;
        }
        slot.1.push(entry);
    });
}

/// The test IDs recorded during the latest pass of `viewport`, taken out (the store keeps nothing for it afterwards).
#[cfg(feature = "egui")]
pub fn take_test_ids(ctx: &egui::Context, viewport: egui::ViewportId) -> Vec<TestIdEntry> {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<TestIds>(test_ids_key()).0.remove(&viewport).map(|s| s.1).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_names() {
        for ok in ["a", "settings.shell.env.row.3.name", "a-b_c"] {
            assert!(is_test_id(ok), "{ok}");
        }
        for bad in ["", ".a", "a.", "a..b", "A", "a b", "ä", "a/b"] {
            assert!(!is_test_id(bad), "{bad}");
        }
    }

    #[cfg(feature = "egui")]
    #[test]
    fn announcements_have_serial_numbers() {
        record_announcements(true);
        let start = announced_count();
        // Other tests may announce too, so only look at our own texts.
        for i in 0..3 {
            heard(&format!("serial-a-{i}"));
        }
        let mid = announced_count();
        assert!(mid >= start + 3);
        heard("serial-b-0");
        let after_mid = announced_after(mid);
        assert!(after_mid.contains(&"serial-b-0".to_owned()));
        assert!(!after_mid.iter().any(|m| m.starts_with("serial-a-")));
        assert_eq!(announced_after(start).iter().filter(|m| m.starts_with("serial-a-")).count(), 3);
        // The cap keeps the serials.
        let base = announced_count();
        for i in 0..205 {
            heard(&format!("serial-cap-{i}"));
        }
        let kept = announced_after(base);
        assert_eq!(kept.len(), 200);
        assert!(announced_count() >= base + 205);
        // Another test may announce in between and push one more of ours out, so ours are an unbroken run from the 6th
        // message or later up to the last.
        let ours: Vec<usize> = kept.iter().filter_map(|m| m.strip_prefix("serial-cap-")?.parse().ok()).collect();
        assert!(ours.first().is_some_and(|&i| i >= 5), "{ours:?}");
        assert_eq!(ours.last(), Some(&204));
        assert!(ours.windows(2).all(|w| w[1] == w[0] + 1), "{ours:?}");
    }

    #[cfg(feature = "egui")]
    fn pass(ctx: &egui::Context, names: &[&str]) -> Vec<egui::accesskit::TreeUpdate> {
        let mut upd = Vec::new();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            for n in names {
                let r = ui.button(*n);
                test_id(&r, n);
            }
        });
        upd.extend(out.platform_output.accesskit_update.clone());
        out.drop_without_applying_deltas();
        upd
    }

    #[cfg(feature = "egui")]
    #[test]
    fn a_test_id_is_in_the_tree_and_noted_once() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut id = None;
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let r = ui.button("go");
            test_id(&r, "x.y");
            id = Some(r.id);
        });
        let upd = out.platform_output.accesskit_update.clone().expect("an update");
        out.drop_without_applying_deltas();
        let id = id.unwrap();
        let node = upd.nodes.iter().find(|(i, _)| *i == id.accesskit_id()).map(|(_, n)| n).expect("the node");
        assert_eq!(node.author_id(), Some("x.y"));
        let got = take_test_ids(&ctx, egui::ViewportId::ROOT);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].name.as_str(), got[0].egui_id), ("x.y", id));
        assert!(take_test_ids(&ctx, egui::ViewportId::ROOT).is_empty());
    }

    #[cfg(feature = "egui")]
    #[test]
    fn nothing_is_noted_while_accesskit_is_off() {
        let ctx = egui::Context::default();
        pass(&ctx, &["a"]);
        assert!(take_test_ids(&ctx, egui::ViewportId::ROOT).is_empty());
    }

    #[cfg(feature = "egui")]
    #[test]
    fn only_the_latest_pass_is_kept() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        pass(&ctx, &["a", "b"]);
        pass(&ctx, &["c"]);
        let got = take_test_ids(&ctx, egui::ViewportId::ROOT);
        assert_eq!(got.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["c"]);
    }

    #[cfg(feature = "egui")]
    #[test]
    fn test_id_in_notes_the_clip_and_whether_the_text_was_cut() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let plain = ui.button("plain");
            test_id(&plain, "plain");
            let wide = ui.label("short");
            let galley = egui::WidgetText::from("short").into_galley(ui, Some(egui::TextWrapMode::Truncate), 1000.0, egui::TextStyle::Body);
            test_id_in(ui, &wide, "short", Some(&galley));
            let narrow = ui.label("short");
            let cut = egui::WidgetText::from("a very long line of text that cannot fit").into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                20.0,
                egui::TextStyle::Body,
            );
            test_id_in(ui, &narrow, "cut", Some(&cut));
            let none = ui.label("none");
            test_id_in(ui, &none, "none", None);
        });
        out.drop_without_applying_deltas();
        let got = take_test_ids(&ctx, egui::ViewportId::ROOT);
        let by = |n: &str| got.iter().find(|e| e.name == n).unwrap_or_else(|| panic!("{n}"));
        assert_eq!((by("plain").clip, by("plain").elided), (None, None));
        assert!(by("short").clip.is_some());
        assert_eq!(by("short").elided, Some(false));
        assert_eq!(by("cut").elided, Some(true));
        assert!(by("none").clip.is_some());
        assert_eq!(by("none").elided, None);
    }

    #[cfg(feature = "egui")]
    /// One pass that announces `text` with `urgency` (nothing for `None`); the nodes of that pass whose text is `text`.
    fn live_pass(ctx: &egui::Context, text: &str, urgency: Option<Urgency>) -> Vec<(egui::accesskit::Role, Option<egui::accesskit::Live>)> {
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(u) = urgency {
                announce_with(ui, text, u);
            }
        });
        let upd = out.platform_output.accesskit_update.clone().expect("an update");
        out.drop_without_applying_deltas();
        upd.nodes.iter().filter(|(_, n)| n.value() == Some(text)).map(|(_, n)| (n.role(), n.live())).collect()
    }

    #[cfg(feature = "egui")]
    #[test]
    fn announce_is_a_live_node_for_one_frame() {
        use egui::accesskit::{Live, Role};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let found = live_pass(&ctx, "saved-live", Some(Urgency::Polite));
        assert_eq!(found, [(Role::Label, Some(Live::Polite))]);
        // Not called: the node is gone, so the same text can be heard again when it comes back.
        assert!(live_pass(&ctx, "saved-live", None).is_empty());
        let again = live_pass(&ctx, "saved-live", Some(Urgency::Polite));
        assert_eq!(again, [(Role::Label, Some(Live::Polite))]);
    }

    #[cfg(feature = "egui")]
    #[test]
    fn announce_plain_is_polite_and_assertive_is_assertive() {
        use egui::accesskit::Live;
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| announce(ui, "plain-polite"));
        let upd = out.platform_output.accesskit_update.clone().expect("an update");
        out.drop_without_applying_deltas();
        assert!(upd.nodes.iter().any(|(_, n)| n.value() == Some("plain-polite") && n.live() == Some(Live::Polite)));
        let found = live_pass(&ctx, "loud-live", Some(Urgency::Assertive));
        assert_eq!(found.iter().map(|f| f.1).collect::<Vec<_>>(), [Some(Live::Assertive)]);
    }

    #[test]
    fn told_speaks_once_per_raise_and_not_in_a_burst() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut t = Told::default();
        assert!(t.due(1, t0));
        assert!(!t.due(1, t0));
        assert!(!t.due(2, ms(500)));
        assert!(!t.due(3, ms(900)));
        // The skipped raises are not spoken later.
        assert!(!t.due(3, ms(1200)));
        assert!(t.due(4, ms(1200)));
        // Raises that jump (the app raised it three times between two frames) are spoken once.
        assert!(t.due(7, ms(2300)));
        assert!(!t.due(7, ms(3500)));
        // The clock went back: not spoken, no panic.
        assert!(!t.due(8, t0));
    }

    #[test]
    fn toast_lines() {
        assert_eq!(toast_line(None, "x", "en"), "x");
        assert_eq!(toast_line(Some(State::Warning), "x", "ja"), "注意: x");
        assert_eq!(toast_line(Some(State::Error), "x", "en"), "error: x");
    }

    #[cfg(feature = "egui")]
    #[test]
    fn announce_toast_resets_on_a_changed_line() {
        use egui::accesskit::Live;
        // Read from the pass's tree, not the global record: another test's burst could push these out of its cap.
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let now = std::time::Instant::now();
        let mut told = Told::default();
        let mut spoke = Vec::new();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            spoke.push(announce_toast(ui, &mut told, 1, None, "toast-one-207", "en", now));
            spoke.push(announce_toast(ui, &mut told, 1, None, "toast-one-207", "en", now));
            spoke.push(announce_toast(ui, &mut told, 1, Some(State::Error), "toast-two-207", "en", now));
            spoke.push(announce_toast(ui, &mut told, 2, Some(State::Error), "toast-two-207", "en", now));
        });
        let upd = out.platform_output.accesskit_update.clone().expect("an update");
        out.drop_without_applying_deltas();
        assert_eq!(spoke, [true, false, true, false]);
        let mut ours: Vec<(String, Option<Live>)> =
            upd.nodes.iter().filter_map(|(_, n)| n.value().filter(|v| v.contains("-207")).map(|v| (v.to_owned(), n.live()))).collect();
        ours.sort_by(|a, b| a.0.cmp(&b.0));
        // An error interrupts; a toast without a level does not.
        assert_eq!(ours, [("error: toast-two-207".to_owned(), Some(Live::Assertive)), ("toast-one-207".to_owned(), Some(Live::Polite))]);
    }

    const ALL: [State; 14] = [
        State::Selected,
        State::Focused,
        State::Conflict,
        State::Error,
        State::Warning,
        State::Done,
        State::Running,
        State::Disabled,
        State::Waiting,
        State::Held,
        State::Skipped,
        State::Missed,
        State::NotRun,
        State::Blocked,
    ];

    #[test]
    fn every_state_has_its_own_mark_and_its_names() {
        let mut marks: Vec<_> = ALL.iter().map(|s| s.mark()).collect();
        marks.sort_unstable();
        marks.dedup();
        assert_eq!(marks.len(), ALL.len(), "two states share a mark");
        for s in ALL {
            assert!(!s.name("en").is_empty() && !s.name("ja").is_empty());
            assert_ne!(s.name("en"), s.name("ja"));
            assert_eq!(s.name("fr"), s.name("en"));
        }
    }

    #[test]
    fn text_is_marked_and_spoken() {
        assert_eq!(State::Selected.marked("main.rs"), "● main.rs");
        assert_eq!(State::spoken("main.rs", &[State::Selected, State::Conflict], "en"), "main.rs, selected, conflict");
        assert_eq!(State::spoken("main.rs", &[State::Error], "ja"), "main.rs, エラー");
        assert_eq!(State::spoken("x", &[], "en"), "x");
        assert_eq!(State::spoken("月次", &[State::Running, State::Error], "ja"), "月次, 実行中, エラー");
        assert_eq!(State::spoken("月次", &[State::Missed], "ja"), "月次, 時刻を過ぎた");
    }

    #[cfg(feature = "egui")]
    #[test]
    fn say_and_announce_run_in_a_frame() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let r = ui.label("row");
            say(&r, Role::Row, "row, selected");
            say(&r, Role::Button, "ok");
            announce(ui, "saved");
        })
        .drop_without_applying_deltas();
    }
}
