//! Second OS windows for an app, as egui deferred viewports: a log, a job's
//! output, a run. [`Windows`] opens and closes them by the app's own key
//! (`job:<run>:<job>`; opening one that is open brings it to the front),
//! gives each the window's own title bar (`ito-titlebar`: the same look as
//! the main window), keeps its place per kind of window
//! (`window-<kind>.toml` in the state folder, read and written off the UI
//! thread, see `ito_common::window`), and names it for `--keys` (`<Window:name>`, see
//! `ito_a11y::window_name`). Theme, fonts and zoom are the main window's:
//! they belong to the egui `Context`, which the windows share.
//!
//! # What the app does
//!
//! Call [`Windows::show`] once in every pass of the main window (from
//! `App::ui`). It returns what closed since the last call, so the app can
//! drop what it keeps for those windows.
//!
//! The body is `Fn(&mut egui::Ui, &K) + Send + Sync + 'static`: a deferred
//! viewport is drawn when the child needs a frame, not when the main window
//! draws, so the body may not borrow the app. Share what it reads with
//! `Arc<Mutex<..>>` (or `RwLock`, or atomics), and when that data changes
//! ask for a frame of the window: [`Windows::repaint`] (or
//! `ctx.request_repaint_of(windows.viewport_id(&key))` from a worker thread:
//! `egui::Context` is `Send + Sync`). A child window that only shows data
//! the app already holds in an `Arc` is what this is for.
//!
//! # The lock
//!
//! The windows share one lock with their callbacks, and the crate never holds
//! it while it draws, writes a place or calls the app. That matters because
//! egui's `embed_viewports` is on by default (eframe turns it off on the
//! desktop): then [`Windows::show`] draws the bodies inline, inside the main
//! window's pass, in egui's own `Window`. A body may lock its own data, and
//! it may call [`Windows::close`] only from a clone of the app's own state,
//! never through the `Windows` it is drawn by (it has no access to it).
//!
//! # Limits
//!
//! * OS screen readers cannot read child windows: in eframe 0.36 an
//!   AccessKit adapter exists only for the root window. egui still builds
//!   the accessibility tree of every window, so `ito-keyscript` reads child
//!   windows; the screen reader does not.
//! * A child window is not put on its pixels on a screen of another scale
//!   (`ito_common::window::Window::put` needs the native window, which eframe
//!   does not hand out for children).
//! * A new window appears on the first [`Windows::show`] after its place is
//!   read (by a worker thread, then a frame of the main window is asked for),
//!   not in the pass of [`Windows::open`]. [`Windows::is_open`] is true at once.
//! * A place is written by a thread when its window closes, so a place that
//!   was only written at close may be lost if the process ends at that
//!   moment. The tracker has already written any place that settled.
//! * The main window closing ends the app, and every child goes with it.

use egui::{Context, Modifiers, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId};
use ito_common::window::{Saved, Seen, Tracker};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// How far a window opened over another of its kind is moved, in points,
/// right and down.
const CASCADE: f32 = 24.0;
/// Two corners this near are the same place.
const NEAR: f32 = 2.0;

/// What a window of one kind is like.
#[derive(Clone, Debug)]
pub struct Spec {
    /// The kind: the place of every window of it is kept in
    /// `window-<kind>.toml` (`ito_common::window::is_window_name`).
    pub kind: String,
    /// The name `--keys` finds the window by (`<Window:name>`); the kind when
    /// `None`. A test ID (`ito_a11y::is_test_id`), never `main`.
    pub name: Option<String>,
    /// The OS title (taskbar, Alt+Tab) and the text on the window's bar.
    pub title: String,
    /// The size when no place is saved, in points.
    pub size: egui::Vec2,
    pub min_size: egui::Vec2,
    /// Esc closes the window, unless the body took the key first, or a widget
    /// had the keyboard focus (a text field, or a control reached with Tab) or
    /// a popup was open when the key was pressed (egui does not consume Esc
    /// there: it clears the focus and closes the popup itself, so that Esc
    /// must not close the window as well).
    pub esc_closes: bool,
}

/// The words of the caption buttons (their hover text and names), in the
/// app's language. Set them with [`Windows::set_words`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words {
    pub minimize: String,
    pub maximize: String,
    pub restore: String,
    pub close: String,
}

impl Default for Words {
    fn default() -> Self {
        Words { minimize: "Minimize".into(), maximize: "Maximize".into(), restore: "Restore".into(), close: "Close".into() }
    }
}

/// What [`Windows::open`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    /// A new window.
    New,
    /// The window was open: it is brought to the front (and un-minimized).
    Focused,
}

/// How a window was closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The close button on the window's bar.
    Button,
    /// Esc (`Spec::esc_closes`).
    Esc,
    /// The OS: Alt+F4, the taskbar, the system menu.
    Os,
    /// The window it was opened with as `parent` closed.
    Parent,
    /// The app: [`Windows::close`] or [`Windows::close_all`].
    App,
}

/// A window that closed, as [`Windows::show`] reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed<K> {
    pub key: K,
    pub why: Why,
}

/// What the app's key of a window must be: `Clone + Eq + Hash + Send + Sync + 'static`
/// (a `String`, or an enum of ids). Every such type is one.
pub trait WindowKey: Clone + Eq + Hash + Send + Sync + 'static {}
impl<T: Clone + Eq + Hash + Send + Sync + 'static> WindowKey for T {}

type Body<K> = Arc<dyn Fn(&mut egui::Ui, &K) + Send + Sync>;
type Shared<K> = Arc<Mutex<Inner<K>>>;

struct Entry<K> {
    key: K,
    id: ViewportId,
    parent: Option<K>,
    kind: String,
    name: Option<String>,
    title: String,
    esc_closes: bool,
    /// A widget had the focus or a popup was open at the end of the last pass.
    busy: bool,
    /// What the builder is except for the title; fixed once the place is read
    /// (a placeholder while `loading`).
    base: ViewportBuilder,
    /// Where it was opened (after the cascade).
    at: Option<egui::Pos2>,
    /// Which open of the key this is: a place that arrives for another one is dropped.
    ticket: u64,
    /// The place is being read: the window is not declared yet.
    loading: Option<Loading>,
    /// Bring it to the front in the next `show`.
    focus: bool,
    tracker: Option<Arc<Mutex<Tracker>>>,
    /// The last look sampled in the window's own pass.
    seen: Option<Seen>,
}

/// What the builder needs of the spec once the place is read.
struct Loading {
    size: egui::Vec2,
    min_size: egui::Vec2,
}

/// A hold on the place loader, for the tests.
#[cfg(test)]
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    cv: std::sync::Condvar,
    /// Loads that have read the file and wait for the gate.
    loaded: std::sync::atomic::AtomicUsize,
    /// Loads that are over (applied or dropped).
    finished: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl Gate {
    fn wait(&self) {
        let mut open = lock(&self.open);
        while !*open {
            open = self.cv.wait(open).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn release(&self) {
        *lock(&self.open) = true;
        self.cv.notify_all();
    }
}

#[cfg(test)]
type TestGate = Option<Arc<Gate>>;
#[cfg(not(test))]
type TestGate = ();

#[cfg(test)]
fn test_gate<K>(g: &Inner<K>) -> TestGate {
    g.gate.clone()
}
#[cfg(not(test))]
fn test_gate<K>(_: &Inner<K>) -> TestGate {}

#[cfg(test)]
fn gate_wait(gate: &TestGate) {
    if let Some(g) = gate {
        g.loaded.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        g.wait();
    }
}
#[cfg(not(test))]
fn gate_wait(_: &TestGate) {}

#[cfg(test)]
fn gate_done(gate: &TestGate) {
    if let Some(g) = gate {
        g.finished.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
#[cfg(not(test))]
fn gate_done(_: &TestGate) {}

struct Inner<K> {
    next_ticket: u64,
    #[cfg(test)]
    gate: Option<Arc<Gate>>,
    state_dir: Option<PathBuf>,
    icon: Option<Arc<egui::IconData>>,
    words: Words,
    body: Body<K>,
    /// Known from the first `show`; for the repaints a close asks for.
    ctx: Option<Context>,
    entries: Vec<Entry<K>>,
    closed: Vec<Closed<K>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The child windows of an app, by the app's key `K`. See the crate's docs.
pub struct Windows<K> {
    salt: String,
    inner: Shared<K>,
}

impl<K: WindowKey> Windows<K> {
    /// A set of windows. `id_salt` tells two sets of one app apart (it is part
    /// of the windows' ids). The places are kept in `state_dir`
    /// (`ito_common::state_dir`); `None` keeps none. `body` draws a window's
    /// content: it runs on the window's own pass, which is why it is
    /// `Send + Sync + 'static`.
    pub fn new(id_salt: &str, state_dir: Option<PathBuf>, body: impl Fn(&mut egui::Ui, &K) + Send + Sync + 'static) -> Self {
        let inner = Inner {
            next_ticket: 0,
            #[cfg(test)]
            gate: None,
            state_dir,
            icon: None,
            words: Words::default(),
            body: Arc::new(body),
            ctx: None,
            entries: Vec::new(),
            closed: Vec::new(),
        };
        Windows { salt: id_salt.to_owned(), inner: Arc::new(Mutex::new(inner)) }
    }

    /// The main window's icon, for every child (the taskbar).
    pub fn icon(self, icon: Arc<egui::IconData>) -> Self {
        lock(&self.inner).icon = Some(icon);
        self
    }

    /// The words of the caption buttons.
    pub fn set_words(&mut self, words: Words) {
        lock(&self.inner).words = words;
    }

    /// The id of the window of `key` (`ctx.request_repaint_of(id)` wakes it).
    /// Pure: it does not say whether the window is open.
    pub fn viewport_id(&self, key: &K) -> ViewportId {
        id_of(&self.salt, key)
    }

    /// Open the window of `key`, or, when it is open, bring it to the front.
    /// `parent`: the window it closes with (the parent's `close` closes it too);
    /// the OS window itself is not owned by the parent.
    ///
    /// The window is open (`is_open`, `close`) at once, but its place is read
    /// by a worker thread and the window is declared in the first
    /// [`Windows::show`] after that. A window opened where another of its kind
    /// sits is moved 24 points right and down, once, when its place arrives.
    pub fn open(&mut self, key: K, spec: Spec, parent: Option<K>) -> Opened {
        let id = self.viewport_id(&key);
        debug_assert!(ito_common::window::is_window_name(&spec.kind), "bad window kind {:?}", spec.kind);
        let name = spec.name.clone().unwrap_or_else(|| spec.kind.clone());
        let name = if ito_a11y::is_test_id(&name) && name != ito_a11y::MAIN_WINDOW {
            Some(name)
        } else {
            debug_assert!(false, "`{name}` is not a window name");
            None
        };
        let mut load = None;
        let (opened, ctx) = {
            let mut g = lock(&self.inner);
            if let Some(e) = g.entries.iter_mut().find(|e| e.key == key) {
                e.focus = true;
                (Opened::Focused, g.ctx.clone())
            } else {
                g.next_ticket += 1;
                let ticket = g.next_ticket;
                g.entries.push(Entry {
                    key: key.clone(),
                    id,
                    parent,
                    kind: spec.kind.clone(),
                    name,
                    title: spec.title,
                    esc_closes: spec.esc_closes,
                    busy: false,
                    base: ViewportBuilder::default(),
                    at: None,
                    ticket,
                    loading: Some(Loading { size: spec.size, min_size: spec.min_size }),
                    focus: false,
                    tracker: None,
                    seen: None,
                });
                match g.state_dir.clone() {
                    // No disk: nothing to wait for.
                    None => {
                        apply_locked(&mut g, &key, ticket, &Saved::load_named(None, &spec.kind));
                    }
                    Some(dir) => load = Some((ticket, dir, test_gate(&g))),
                }
                (Opened::New, g.ctx.clone())
            }
        };
        if let Some((ticket, dir, gate)) = load {
            // A small file, but not read on the UI thread.
            let (shared, k, kind) = (self.inner.clone(), key.clone(), spec.kind.clone());
            let spawned = std::thread::Builder::new().name("window-place".into()).spawn(move || {
                let saved = Saved::load_named(Some(dir), &kind);
                gate_wait(&gate);
                let ctx = apply_place(&shared, &k, ticket, &saved);
                gate_done(&gate);
                if let Some(ctx) = ctx {
                    ctx.request_repaint_of(ViewportId::ROOT);
                }
            });
            if spawned.is_err() {
                // No thread: open with no saved place, and do not read here.
                apply_place(&self.inner, &key, ticket, &Saved::load_named(None, &spec.kind));
            }
        }
        if let Some(ctx) = ctx {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        opened
    }

    /// Close the window of `key` and, transitively, the windows opened with it
    /// as `parent`. `false` when it was not open (closing twice is a no-op).
    pub fn close(&mut self, key: &K) -> bool {
        close_key(&self.inner, key, Why::App)
    }

    /// Close every window.
    pub fn close_all(&mut self) {
        let keys = self.keys();
        for k in keys {
            close_key(&self.inner, &k, Why::App);
        }
    }

    /// Whether the window of `key` is open.
    pub fn is_open(&self, key: &K) -> bool {
        lock(&self.inner).entries.iter().any(|e| &e.key == key)
    }

    /// The keys of the open windows, oldest first.
    pub fn keys(&self) -> Vec<K> {
        lock(&self.inner).entries.iter().map(|e| e.key.clone()).collect()
    }

    /// Change the title (the OS title and the bar's text) of an open window.
    pub fn set_title(&mut self, key: &K, title: impl Into<String>) {
        let ctx = {
            let mut g = lock(&self.inner);
            let Some(e) = g.entries.iter_mut().find(|e| &e.key == key) else { return };
            e.title = title.into();
            g.ctx.clone()
        };
        if let Some(ctx) = ctx {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }

    /// Ask for a frame of the window of `key`: its data changed.
    pub fn repaint(&self, ctx: &Context, key: &K) {
        ctx.request_repaint_of(self.viewport_id(key));
    }

    /// Ask for a frame of every open window.
    pub fn repaint_all(&self, ctx: &Context) {
        for id in lock(&self.inner).entries.iter().map(|e| e.id).collect::<Vec<_>>() {
            ctx.request_repaint_of(id);
        }
    }

    /// Show every open window. Call it once per pass of the main window; the
    /// windows are closed in the pass they are not shown in. Returns what
    /// closed since the last call.
    pub fn show(&mut self, ctx: &Context) -> Vec<Closed<K>> {
        let (closed, declared) = {
            let mut g = lock(&self.inner);
            g.ctx = Some(ctx.clone());
            let closed = std::mem::take(&mut g.closed);
            let declared: Vec<_> = g
                .entries
                .iter_mut()
                .filter(|e| e.loading.is_none())
                .map(|e| (e.key.clone(), e.id, e.base.clone().with_title(e.title.clone()), std::mem::take(&mut e.focus)))
                .collect();
            (closed, declared)
        };
        // The lock is free here: with `embed_viewports` the body runs inside
        // this call.
        for (key, id, builder, focus) in declared {
            let shared = self.inner.clone();
            ctx.show_viewport_deferred(id, builder, move |ui, class| pass(&shared, &key, ui, class));
            if focus {
                ctx.send_viewport_cmd_to(id, ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
            }
        }
        closed
    }
}

fn id_of<K: Hash>(salt: &str, key: &K) -> ViewportId {
    // `DefaultHasher::new()` has fixed keys: the same key is the same id in every run.
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    ViewportId::from_hash_of(("ito-viewport", salt, h.finish()))
}

/// Move the new window off the ones of its kind that sit where it would open.
fn cascade<'a, K: 'a>(base: ViewportBuilder, same_kind: impl Iterator<Item = &'a Entry<K>>) -> ViewportBuilder {
    let sit: Vec<egui::Pos2> = same_kind.filter_map(|e| e.seen.map(|s| egui::pos2(s.corner[0], s.corner[1])).or(e.at)).collect();
    let Some(first) = sit.first().copied() else { return base };
    let mut at = base.position.unwrap_or(first);
    let near = |a: egui::Pos2, b: egui::Pos2| (a.x - b.x).abs() < NEAR && (a.y - b.y).abs() < NEAR;
    for _ in 0..=sit.len() {
        if !sit.iter().any(|s| near(*s, at)) {
            break;
        }
        at += egui::vec2(CASCADE, CASCADE);
    }
    base.with_position(at)
}

/// Build the window's builder from the place read, and make it ready to be
/// declared. `false` when it is gone or is another open of the key.
fn apply_locked<K: WindowKey>(g: &mut Inner<K>, key: &K, ticket: u64, saved: &Saved) -> bool {
    let Some(i) = g.entries.iter().position(|e| &e.key == key && e.ticket == ticket) else { return false };
    let Some(l) = g.entries[i].loading.take() else { return false };
    let mut base = ViewportBuilder::default().with_title(g.entries[i].title.clone()).with_min_inner_size(l.min_size);
    if let Some(icon) = &g.icon {
        base = base.with_icon(icon.clone());
    }
    let base = saved.child_viewport(ito_titlebar::frameless(base.with_inner_size(l.size)));
    let kind = g.entries[i].kind.clone();
    let base = cascade(base, g.entries.iter().filter(|e| e.kind == kind));
    let e = &mut g.entries[i];
    e.at = base.position;
    e.base = base;
    e.tracker = saved.tracker().map(|t| Arc::new(Mutex::new(t)));
    true
}

/// [`apply_locked`] for a worker; the context to ask a frame of when it applied.
fn apply_place<K: WindowKey>(shared: &Shared<K>, key: &K, ticket: u64, saved: &Saved) -> Option<Context> {
    let mut g = lock(shared);
    if apply_locked(&mut g, key, ticket, saved) {
        g.ctx.clone()
    } else {
        None
    }
}

/// The one way a window closes: out of the registry (with what was opened
/// with it), its place written, a `Closed` for `show` to return, and a frame
/// of the main window so the next pass stops declaring it (and eframe closes
/// the OS window). Idempotent.
fn close_key<K: WindowKey>(shared: &Shared<K>, key: &K, why: Why) -> bool {
    let (taken, ctx) = {
        let mut g = lock(shared);
        let mut gone = vec![key.clone()];
        let mut i = 0;
        // The key and its descendants, parents first.
        while i < gone.len() {
            let parent = gone[i].clone();
            for e in &g.entries {
                if e.parent.as_ref() == Some(&parent) && !gone.contains(&e.key) {
                    gone.push(e.key.clone());
                }
            }
            i += 1;
        }
        let mut taken = Vec::new();
        for k in gone {
            if let Some(at) = g.entries.iter().position(|e| e.key == k) {
                let e = g.entries.remove(at);
                g.closed.push(Closed { key: e.key.clone(), why: if &e.key == key { why } else { Why::Parent } });
                taken.push((e.tracker, e.seen));
            }
        }
        (taken, g.ctx.clone())
    };
    if taken.is_empty() {
        return false;
    }
    // Closing a tracker writes a file: not on the UI thread.
    let trackers: Vec<_> = taken.into_iter().filter_map(|(t, seen)| t.map(|t| (t, seen))).collect();
    if !trackers.is_empty() {
        let work = Arc::new(Mutex::new(Some(trackers)));
        let job = work.clone();
        let spawned = std::thread::Builder::new().name("window-place".into()).spawn(move || close_trackers(&job));
        if spawned.is_err() {
            // No thread: the place of this close is lost, not written here
            // (this is the UI thread, as `Tracker::see` loses one too).
            drop(work);
        }
    }
    if let Some(ctx) = ctx {
        ctx.request_repaint_of(ViewportId::ROOT);
    }
    true
}

type Pending = Mutex<Option<Vec<(Arc<Mutex<Tracker>>, Option<Seen>)>>>;

/// Close the trackers once (a second call finds the list taken).
fn close_trackers(work: &Pending) {
    let list = lock(work).take().unwrap_or_default();
    for (t, seen) in list {
        lock(&t).close(seen);
    }
}

/// What a pass needs of its window, copied out so the lock is not held while
/// anything is drawn.
struct Snap<K> {
    title: String,
    name: Option<String>,
    esc_closes: bool,
    busy: bool,
    words: Words,
    body: Body<K>,
    tracker: Option<Arc<Mutex<Tracker>>>,
}

fn snapshot<K: WindowKey>(shared: &Shared<K>, key: &K) -> Option<Snap<K>> {
    let g = lock(shared);
    let e = g.entries.iter().find(|e| &e.key == key)?;
    Some(Snap {
        title: e.title.clone(),
        name: e.name.clone(),
        esc_closes: e.esc_closes,
        busy: e.busy,
        words: g.words.clone(),
        body: g.body.clone(),
        tracker: e.tracker.clone(),
    })
}

/// Esc after the body had its chance. egui does not consume Esc in a text
/// field or a popup: it clears the focus (before the body runs) and closes the
/// popup itself. So the window ignores an Esc that arrives while the last pass
/// ended with a focused widget or an open popup (`busy`: that is what was open
/// when the user pressed the key), and records `busy` for the next pass.
fn esc_and_busy<K: WindowKey>(shared: &Shared<K>, key: &K, ctx: &Context, snap: &Snap<K>) {
    if snap.esc_closes && !snap.busy && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, egui::Key::Escape)) {
        close_key(shared, key, Why::Esc);
        return;
    }
    let busy = ctx.memory(|m| m.focused().is_some()) || egui::Popup::is_any_open(ctx);
    if let Some(e) = lock(shared).entries.iter_mut().find(|e| &e.key == key) {
        e.busy = busy;
    }
}

/// One pass of one child window.
fn pass<K: WindowKey>(shared: &Shared<K>, key: &K, ui: &mut egui::Ui, class: ViewportClass) {
    let ctx = ui.ctx().clone();
    let Some(snap) = snapshot(shared, key) else { return };
    if class == ViewportClass::EmbeddedWindow {
        // Inside egui's own `Window` in the main window: the body only.
        (snap.body)(ui, key);
        esc_and_busy(shared, key, &ctx, &snap);
        return;
    }
    // The look of the window in its own pass, for its place now and when it
    // closes from outside.
    let id = ctx.viewport_id();
    let seen = Seen::of(&ctx);
    if let Some(e) = lock(shared).entries.iter_mut().find(|e| &e.key == key) {
        e.seen = seen;
    }
    if let Some(tracker) = &snap.tracker {
        let wait = lock(tracker).see(seen);
        if let Some(d) = wait {
            ctx.request_repaint_after_for(d, id);
        }
    }
    if ctx.input(|i| i.viewport().close_requested()) {
        close_key(shared, key, Why::Os);
    }
    let mac = cfg!(target_os = "macos");
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let bg = ito_theme::colors().bg;
    let mut op = None;
    egui::Panel::top("ito-viewport.bar").exact_size(ito_titlebar::HEIGHT).frame(egui::Frame::NONE.fill(bg)).show(ui, |ui| {
        let w = &snap.words;
        let spec = ito_titlebar::Spec {
            own: !mac,
            left: if mac { 76.0 } else { 0.0 },
            maximized,
            unread: 0,
            any_error: false,
            centre: false,
            right_min: 0.0,
            words: ito_titlebar::Words {
                search: "",
                search_key: "",
                bell: "",
                minimize: &w.minimize,
                maximize: &w.maximize,
                restore: &w.restore,
                close: &w.close,
            },
        };
        let title = snap.title.clone();
        let out = ito_titlebar::show(
            ui,
            &spec,
            |ui, rect| {
                let x = rect.left() + ito_theme::scale::space::MD;
                let font = egui::FontId::proportional(ito_theme::scale::Text::ChromeBody.size());
                let mut job = egui::text::LayoutJob::simple_singleline(title, font, ito_theme::colors().fg);
                job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - x - ito_theme::scale::space::SM).max(0.0));
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                let right = x + galley.size().x;
                ui.painter().galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, egui::Color32::WHITE);
                right
            },
            |_, _| {},
        );
        op = out.window;
    });
    match op {
        Some(ito_titlebar::WindowOp::Close) => {
            close_key(shared, key, Why::Button);
        }
        Some(op) => ito_titlebar::apply(&ctx, op, maximized),
        None => {}
    }
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(bg)).show(ui, |ui| (snap.body)(ui, key));
    esc_and_busy(shared, key, &ctx, &snap);
    if !mac && !maximized {
        ito_titlebar::resize_edges(&ctx);
    }
    if let Some(name) = &snap.name {
        ito_a11y::window_name(&ctx, name);
    }
}

#[cfg(test)]
mod tests;
