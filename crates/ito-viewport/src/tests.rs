use super::*;
use egui::{FullOutput, RawInput};

type W = Windows<&'static str>;

fn spec(kind: &str) -> Spec {
    Spec {
        kind: kind.into(),
        name: None,
        title: format!("title {kind}"),
        size: egui::vec2(600.0, 400.0),
        min_size: egui::vec2(300.0, 200.0),
        esc_closes: true,
    }
}

/// A context as eframe's desktop one: real child viewports, AccessKit on.
fn ctx() -> Context {
    let ctx = Context::default();
    ctx.set_embed_viewports(false);
    ctx.enable_accesskit();
    ctx
}

fn windows(dir: Option<PathBuf>) -> W {
    Windows::new("test", dir, |ui, key| {
        ui.label(*key);
    })
}

/// Show passes until `want` windows are declared (the places are read by threads).
fn root_until(ctx: &Context, w: &mut W, want: usize) -> (Vec<Closed<&'static str>>, FullOutput) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let (closed, out) = root(ctx, w);
        if out.viewport_output.len() == want + 1 || std::time::Instant::now() > deadline {
            return (closed, out);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !f() {
        assert!(std::time::Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn root(ctx: &Context, w: &mut W) -> (Vec<Closed<&'static str>>, FullOutput) {
    let mut closed = Vec::new();
    let mut out = ctx.run_ui(RawInput::default(), |ui| closed.extend(w.show(ui.ctx())));
    out.textures_delta.clear();
    (closed, out)
}

/// One pass of the child `id`, as eframe runs it: the stored callback.
fn child(ctx: &Context, out: &FullOutput, id: ViewportId, edit: impl FnOnce(&mut RawInput)) -> FullOutput {
    let cb = out.viewport_output[&id].viewport_ui_cb.clone().expect("a deferred viewport has its callback");
    let mut raw = RawInput { viewport_id: id, ..Default::default() };
    raw.viewports.insert(id, Default::default());
    edit(&mut raw);
    let mut out = ctx.run_ui(raw, |ui| cb(ui));
    out.textures_delta.clear();
    out
}

fn escape(raw: &mut RawInput) {
    raw.events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
}

#[test]
fn opening_twice_is_one_window_brought_to_the_front() {
    let ctx = ctx();
    let mut w = windows(None);
    assert_eq!(w.open("a", spec("log"), None), Opened::New);
    let id = w.viewport_id(&"a");
    let (_, out) = root(&ctx, &mut w);
    assert_eq!(out.viewport_output.len(), 2, "the root and one child");
    let o = &out.viewport_output[&id];
    assert!(o.class == ViewportClass::Deferred);
    assert_eq!(o.builder.title.as_deref(), Some("title log"));
    assert_eq!(o.builder.inner_size, Some(egui::vec2(600.0, 400.0)));
    assert!(!o.commands.contains(&ViewportCommand::Focus), "a new window is not told to focus");
    assert_eq!(w.open("a", spec("log"), None), Opened::Focused);
    assert_eq!(w.keys(), ["a"]);
    let (_, out) = root(&ctx, &mut w);
    assert_eq!(out.viewport_output.len(), 2);
    let o = &out.viewport_output[&id];
    assert!(o.commands.contains(&ViewportCommand::Focus) && o.commands.contains(&ViewportCommand::Minimized(false)), "{:?}", o.commands);
    // Told once.
    let (_, out) = root(&ctx, &mut w);
    assert!(!out.viewport_output[&id].commands.contains(&ViewportCommand::Focus));
}

#[test]
fn closing_takes_what_was_opened_with_it() {
    let ctx = ctx();
    let mut w = windows(None);
    w.open("run", spec("run"), None);
    w.open("job", spec("job"), Some("run"));
    w.open("log", spec("log"), Some("job"));
    w.open("other", spec("log"), None);
    assert!(w.close(&"run"));
    assert_eq!(w.keys(), ["other"]);
    assert!(!w.close(&"run"), "closing twice is a no-op");
    assert!(!w.close(&"log"));
    let (closed, out) = root(&ctx, &mut w);
    assert_eq!(
        closed,
        [Closed { key: "run", why: Why::App }, Closed { key: "job", why: Why::Parent }, Closed { key: "log", why: Why::Parent },]
    );
    assert_eq!(out.viewport_output.len(), 2, "only `other` is declared");
    assert!(root(&ctx, &mut w).0.is_empty(), "reported once");
    w.close_all();
    assert!(!w.is_open(&"other") && w.keys().is_empty());
    assert_eq!(root(&ctx, &mut w).0, [Closed { key: "other", why: Why::App }]);
}

#[test]
fn esc_closes_unless_the_body_took_it_or_the_spec_says_no() {
    let ctx = ctx();
    let mut w = windows(None);
    w.open("a", spec("log"), None);
    let id = w.viewport_id(&"a");
    let (_, out) = root(&ctx, &mut w);
    child(&ctx, &out, id, |_| {}).drop_without_applying_deltas();
    assert!(w.is_open(&"a"), "no key, no close");
    child(&ctx, &out, id, escape).drop_without_applying_deltas();
    assert!(!w.is_open(&"a"));
    assert_eq!(root(&ctx, &mut w).0, [Closed { key: "a", why: Why::Esc }]);

    // `esc_closes: false`.
    let mut w = windows(None);
    w.open("a", Spec { esc_closes: false, ..spec("log") }, None);
    let (_, out) = root(&ctx, &mut w);
    child(&ctx, &out, w.viewport_id(&"a"), escape).drop_without_applying_deltas();
    assert!(w.is_open(&"a"));

    // A body that took Esc keeps it.
    let mut w: W = Windows::new("test", None, |ui: &mut egui::Ui, _: &&'static str| {
        ui.ctx().input_mut(|i| i.consume_key(Modifiers::NONE, egui::Key::Escape));
    });
    w.open("a", spec("log"), None);
    let (_, out) = root(&ctx, &mut w);
    child(&ctx, &out, w.viewport_id(&"a"), escape).drop_without_applying_deltas();
    assert!(w.is_open(&"a"));
}

#[test]
fn the_os_closing_a_window_is_one_close() {
    let ctx = ctx();
    let mut w = windows(None);
    w.open("a", spec("log"), None);
    w.open("b", spec("log"), Some("a"));
    let (_, out) = root(&ctx, &mut w);
    child(&ctx, &out, w.viewport_id(&"a"), |raw| {
        raw.viewports.get_mut(&w.viewport_id(&"a")).unwrap().events.push(egui::ViewportEvent::Close);
    })
    .drop_without_applying_deltas();
    assert!(w.keys().is_empty());
    assert_eq!(root(&ctx, &mut w).0, [Closed { key: "a", why: Why::Os }, Closed { key: "b", why: Why::Parent }]);
    assert!(!w.close(&"a"));
}

#[test]
fn the_window_is_named_for_keys_and_the_name_may_be_chosen() {
    let ctx = ctx();
    let mut w = windows(None);
    w.open("a", spec("log"), None);
    w.open("b", Spec { name: Some("job-2".into()), ..spec("job") }, None);
    let (_, out) = root(&ctx, &mut w);
    let (a, b) = (w.viewport_id(&"a"), w.viewport_id(&"b"));
    child(&ctx, &out, a, |_| {}).drop_without_applying_deltas();
    child(&ctx, &out, b, |_| {}).drop_without_applying_deltas();
    assert_eq!(ito_a11y::take_window_name(&ctx, a).as_deref(), Some("log"));
    assert_eq!(ito_a11y::take_window_name(&ctx, b).as_deref(), Some("job-2"));
    assert_eq!(ito_a11y::take_window_name(&ctx, ViewportId::ROOT), None);
}

#[test]
fn the_body_is_drawn_in_the_window_and_may_lock_its_own_data() {
    let ctx = ctx();
    let data = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = data.clone();
    let mut w: W = Windows::new("test", None, move |ui: &mut egui::Ui, key: &&'static str| {
        seen.lock().unwrap().push(format!("{key}@{:?}", ui.ctx().viewport_id() == ViewportId::ROOT));
        ui.label(*key);
    });
    w.open("a", spec("log"), None);
    let (_, out) = root(&ctx, &mut w);
    let out = child(&ctx, &out, w.viewport_id(&"a"), |_| {});
    assert!(!out.shapes.is_empty());
    assert_eq!(*data.lock().unwrap(), ["a@false"]);
}

#[test]
fn embedded_viewports_do_not_deadlock_and_draw_the_body() {
    let ctx = Context::default();
    assert!(ctx.embed_viewports(), "egui's default");
    let drawn = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let d = drawn.clone();
    let mut w: W = Windows::new("test", None, move |ui: &mut egui::Ui, key: &&'static str| {
        // Locks its own data, and reads the registry of nobody.
        d.lock().unwrap().push(*key);
        ui.label(*key);
    });
    w.open("a", spec("log"), None);
    w.open("b", spec("log"), Some("a"));
    let (_, out) = root(&ctx, &mut w);
    assert_eq!(out.viewport_output.len(), 1, "no OS window: embedded in the root");
    assert_eq!(*drawn.lock().unwrap(), ["a", "b"]);
    // Esc closes an embedded window too, and its children.
    let mut closed = Vec::new();
    let mut raw = RawInput::default();
    escape(&mut raw);
    ctx.run_ui(raw, |ui| closed.extend(w.show(ui.ctx()))).drop_without_applying_deltas();
    assert!(w.keys().is_empty(), "{:?}", w.keys());
    assert_eq!(root(&ctx, &mut w).0.len(), 2);
}

#[test]
fn the_place_is_kept_per_kind_and_the_next_window_of_a_kind_is_moved() {
    let dir = tempfile::tempdir().unwrap();
    let file = ito_common::window::path_named(dir.path(), "log");
    std::fs::write(&file, "x = 100.0\ny = 80.0\nw = 900.0\nh = 600.0\nscale = 1.0\nzoom = 1.0\n").unwrap();
    let ctx = ctx();
    let mut w = windows(Some(dir.path().to_path_buf()));
    // One at a time: the places arrive in the order of the threads otherwise.
    for (k, kind) in [("a", "log"), ("b", "log"), ("c", "log"), ("j", "job")] {
        w.open(k, spec(kind), None);
        wait_for("the place", || lock(&w.inner).entries.iter().all(|e| e.loading.is_none()));
    }
    let (_, out) = root_until(&ctx, &mut w, 4);
    let ids: Vec<_> = ["a", "b", "c", "j"].iter().map(|k| (*k, w.viewport_id(k))).collect();
    let id = move |k: &'static str| ids.iter().find(|(n, _)| *n == k).unwrap().1;
    let at = |k: &'static str| out.viewport_output[&id(k)].builder.position;
    let size = |k: &'static str| out.viewport_output[&id(k)].builder.inner_size;
    let place = ito_common::window::Window { x: 100.0, y: 80.0, w: 900.0, h: 600.0, maximized: false, scale: 1.0, zoom: 1.0 };
    if place.reachable_now(ito_common::window::PRIMARY_GUESS, ito_common::window::GRAB) {
        assert_eq!(at("a"), Some(egui::pos2(100.0, 80.0)));
        assert_eq!(at("b"), Some(egui::pos2(124.0, 104.0)));
        assert_eq!(at("c"), Some(egui::pos2(148.0, 128.0)));
        assert_eq!(size("a"), Some(egui::vec2(900.0, 600.0)));
    }
    assert_eq!((at("j"), size("j")), (None, Some(egui::vec2(600.0, 400.0))), "no place for another kind");
    // The position does not change from pass to pass (no put-back while dragged).
    let first_b = at("b");
    let (_, again) = root(&ctx, &mut w);
    assert_eq!(again.viewport_output[&w.viewport_id(&"b")].builder.position, first_b);
    // Closing writes the window's place into its kind's file only.
    let a = w.viewport_id(&"j");
    child(&ctx, &again, a, |raw| {
        let v = raw.viewports.get_mut(&a).unwrap();
        v.outer_rect = Some(egui::Rect::from_min_size(egui::pos2(300.0, 200.0), egui::vec2(710.0, 530.0)));
        v.inner_rect = Some(egui::Rect::from_min_size(egui::pos2(305.0, 225.0), egui::vec2(700.0, 500.0)));
        v.native_pixels_per_point = Some(1.0);
    })
    .drop_without_applying_deltas();
    assert!(w.close(&"j"));
    // The place is written by a thread.
    for _ in 0..200 {
        if ito_common::window::path_named(dir.path(), "job").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(ito_common::window::path_named(dir.path(), "job").exists());
    let saved = Saved::load_named(Some(dir.path().to_path_buf()), "job").place().expect("the job place");
    assert_eq!((saved.x, saved.y, saved.w, saved.h), (300.0, 200.0, 700.0, 500.0));
    assert!(!ito_common::window::path(dir.path()).exists(), "window.toml is the main window's");
}

#[test]
fn the_title_changes_in_the_builder() {
    let ctx = ctx();
    let mut w = windows(None);
    w.open("a", spec("log"), None);
    root(&ctx, &mut w).1.drop_without_applying_deltas();
    w.set_title(&"a", "new");
    let (_, out) = root(&ctx, &mut w);
    assert_eq!(out.viewport_output[&w.viewport_id(&"a")].builder.title.as_deref(), Some("new"));
}

#[test]
fn two_sets_do_not_share_ids() {
    let a: W = Windows::new("one", None, |_, _| {});
    let b: W = Windows::new("two", None, |_, _| {});
    assert_ne!(a.viewport_id(&"k"), b.viewport_id(&"k"));
    assert_ne!(a.viewport_id(&"k"), a.viewport_id(&"l"));
    assert_eq!(a.viewport_id(&"k"), a.viewport_id(&"k"));
}

#[test]
fn esc_in_a_text_field_does_not_close_the_window() {
    let ctx = ctx();
    let text = Arc::new(Mutex::new(String::new()));
    let t = text.clone();
    let asked = Arc::new(Mutex::new(false));
    let mut w: W = Windows::new("test", None, move |ui: &mut egui::Ui, _: &&'static str| {
        let mut s = t.lock().unwrap();
        let r = ui.add(egui::TextEdit::singleline(&mut *s).id(egui::Id::new("field")));
        if !r.has_focus() && ui.ctx().memory(|m| m.focused().is_none()) && !*asked.lock().unwrap() {
            *asked.lock().unwrap() = true;
            r.request_focus();
        }
    });
    w.open("a", spec("log"), None);
    let id = w.viewport_id(&"a");
    let (_, out) = root(&ctx, &mut w);
    // Two passes: the field takes the focus.
    child(&ctx, &out, id, |_| {});
    child(&ctx, &out, id, |_| {});
    assert!(ctx.memory(|m| m.focused().is_some()), "the field has the focus");
    child(&ctx, &out, id, escape);
    assert!(w.is_open(&"a"), "Esc went to the field");
    assert!(ctx.memory(|m| m.focused().is_none()), "egui dropped the focus");
    child(&ctx, &out, id, escape);
    assert!(!w.is_open(&"a"));
    assert_eq!(root(&ctx, &mut w).0, [Closed { key: "a", why: Why::Esc }]);
}

use std::sync::atomic::Ordering::SeqCst;

fn gated(w: &W) -> Arc<Gate> {
    let gate = Arc::new(Gate::default());
    lock(&w.inner).gate = Some(gate.clone());
    gate
}

#[test]
fn without_a_state_dir_the_window_is_declared_in_the_first_show() {
    let ctx = ctx();
    let mut w = windows(None);
    w.open("a", spec("log"), None);
    let (_, out) = root(&ctx, &mut w);
    assert_eq!(out.viewport_output.len(), 2);
    assert!(lock(&w.inner).entries[0].loading.is_none());
}

#[test]
fn a_window_is_not_declared_until_its_place_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx();
    let mut w = windows(Some(dir.path().to_path_buf()));
    let gate = gated(&w);
    assert_eq!(w.open("a", spec("log"), None), Opened::New);
    assert!(w.is_open(&"a"));
    wait_for("the read", || gate.loaded.load(SeqCst) == 1);
    assert_eq!(root(&ctx, &mut w).1.viewport_output.len(), 1, "loading: not declared");
    // Focus asked for while loading is kept.
    assert_eq!(w.open("a", spec("log"), None), Opened::Focused);
    assert_eq!(root(&ctx, &mut w).1.viewport_output.len(), 1);
    gate.release();
    wait_for("the apply", || gate.finished.load(SeqCst) == 1);
    let (_, out) = root(&ctx, &mut w);
    assert_eq!(out.viewport_output.len(), 2);
    let o = &out.viewport_output[&w.viewport_id(&"a")];
    assert!(o.commands.contains(&ViewportCommand::Focus), "{:?}", o.commands);
}

#[test]
fn opening_an_open_key_does_not_touch_the_disk() {
    let ctx = ctx();
    let mut w = windows(None);
    w.open("a", spec("log"), None);
    // A state dir that cannot be read: a second open must not look at it.
    let dir = tempfile::tempdir().unwrap();
    lock(&w.inner).state_dir = Some(dir.path().join("no").join("such").join("dir"));
    let gate = gated(&w);
    assert_eq!(w.open("a", spec("log"), None), Opened::Focused);
    assert_eq!(gate.loaded.load(SeqCst), 0);
    assert_eq!(root(&ctx, &mut w).1.viewport_output.len(), 2);
    assert_eq!(gate.loaded.load(SeqCst), 0);
}

#[test]
fn a_window_closed_while_its_place_loads_never_appears() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx();
    let mut w = windows(Some(dir.path().to_path_buf()));
    let gate = gated(&w);
    w.open("a", spec("log"), None);
    wait_for("the read", || gate.loaded.load(SeqCst) == 1);
    assert!(w.close(&"a"));
    gate.release();
    wait_for("the late result", || gate.finished.load(SeqCst) == 1);
    assert!(!w.is_open(&"a"));
    let (closed, out) = root(&ctx, &mut w);
    assert_eq!(closed, [Closed { key: "a", why: Why::App }]);
    assert_eq!(out.viewport_output.len(), 1, "nothing declared");
    assert!(w.keys().is_empty());
}

#[test]
fn a_key_closed_and_opened_again_gets_the_second_places() {
    let dir = tempfile::tempdir().unwrap();
    let file = ito_common::window::path_named(dir.path(), "log");
    let write =
        |x: f32, w: f32| std::fs::write(&file, format!("x = {x}\ny = 80.0\nw = {w}\nh = 600.0\nscale = 1.0\nzoom = 1.0\n")).unwrap();
    write(100.0, 900.0);
    let ctx = ctx();
    let mut w = windows(Some(dir.path().to_path_buf()));
    let gate = gated(&w);
    w.open("a", spec("log"), None);
    wait_for("the first read", || gate.loaded.load(SeqCst) == 1);
    assert!(w.close(&"a"));
    write(300.0, 700.0);
    assert_eq!(w.open("a", Spec { title: "second".into(), ..spec("log") }, None), Opened::New);
    wait_for("the second read", || gate.loaded.load(SeqCst) == 2);
    gate.release();
    wait_for("both loads", || gate.finished.load(SeqCst) == 2);
    let (_, out) = root(&ctx, &mut w);
    assert_eq!(out.viewport_output.len(), 2);
    let o = &out.viewport_output[&w.viewport_id(&"a")];
    assert_eq!(o.builder.title.as_deref(), Some("second"));
    let place = ito_common::window::Window { x: 300.0, y: 80.0, w: 700.0, h: 600.0, maximized: false, scale: 1.0, zoom: 1.0 };
    if place.reachable_now(ito_common::window::PRIMARY_GUESS, ito_common::window::GRAB) {
        assert_eq!(o.builder.position, Some(egui::pos2(300.0, 80.0)));
        assert_eq!(o.builder.inner_size, Some(egui::vec2(700.0, 600.0)));
    }
}
