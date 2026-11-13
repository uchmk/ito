//! A masked field for a secret (a token, a password, an OAuth client secret) that the app hands to `ito-secret`.
//!
//! Unlike [`crate::field`] it never shows what is kept: the box starts empty, the hint says whether a secret is kept.
//! Two modes: take-once ([`SecretField::take`], what [`secret_field`] does: the typed text is handed out once and dropped
//! from the [`Drafts`](crate::Drafts)) and bound ([`SecretField::show`], for a form with a save button: the box edits the
//! app's [`Hidden`] directly).
//!
//! What this does and does not promise: [`Hidden`]'s `Debug` never prints the text; the box keeps no undo copies in egui's
//! memory; the AccessKit tree carries only the masked form while the box is masked. There is no zeroing on drop and no
//! promise of erasure: egui clones the text for a frame and caches galleys, and a revealed copy goes to the clipboard on
//! Ctrl+C. Nothing here touches the disk.

use crate::{field_id, focus_ring, take_keys, z, Draft, Drafts, Look};
use egui::{FontId, Id};
use ito_theme::mix;

/// A secret the app holds for a form. `Debug` never prints it (`Hidden(..)`) and there is no `Display`, no `Deref` to
/// `str` and no conversion into `String`: the text leaves only through [`expose`](Self::expose) or [`take`](Self::take).
/// `Clone` is there because forms are cloned.
///
/// An empty `Hidden` means "nothing typed". A form that must tell "keep the stored secret" from "delete it" keeps its own
/// state for that; `Hidden` does not express delete. The text is not zeroed on drop (this crate has no `unsafe`).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Hidden(String);

impl Hidden {
    /// A box holding `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Hidden(text.into())
    }

    /// The text, to hand to the store. The name says it leaves the box.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Takes the text out, leaving this one empty.
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.0)
    }

    /// Empties the text.
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Whether nothing is typed.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Hidden {
    /// Neither the text nor its length.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "Hidden(empty)" } else { "Hidden(..)" })
    }
}

/// The words of the secret field's eye button.
pub struct SecretWords<'a> {
    /// Shown on the button while the text is masked.
    pub show: &'a str,
    /// Shown on the button while the text is plain.
    pub hide: &'a str,
}

/// What a lost focus means: the text to take, or nothing (Esc, or nothing typed).
pub fn taken(text: &str, escape: bool) -> Option<String> {
    if escape || text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

/// What a secret field answers.
pub struct SecretResponse {
    /// The text edit's response (focus, `lost_focus`, `changed`), so the app can add more with `ito_a11y`.
    pub response: egui::Response,
    /// Take mode: the text, once. Always `None` in bound mode.
    pub taken: Option<String>,
    /// The text changed this frame (bound mode: the app's [`Hidden`] changed).
    pub changed: bool,
}

/// A one-line masked field plus a small show / hide button.
///
/// Test IDs (each only when `ito_a11y::is_test_id` accepts it): `secret.{id}` for the box and `secret.{id}.eye` for the
/// button. The box is a password input to a screen reader, named by [`name`](Self::name).
///
/// While the text is masked, nothing in AccessKit carries it. After the eye is pressed the box shows it, and a screen
/// reader may read it, as a browser's "show password" does.
pub struct SecretField<'a> {
    id: &'a str,
    words: &'a SecretWords<'a>,
    name: &'a str,
    hint: &'a str,
    width: f32,
    error: bool,
}

impl<'a> SecretField<'a> {
    /// `id` tells this field from the others (it is the drafts key in take mode).
    pub fn new(id: &'a str, words: &'a SecretWords<'a>) -> Self {
        SecretField { id, words, name: "", hint: "", width: 280.0, error: false }
    }

    /// The name a screen reader hears for the box (the form's visible label); the hint, then the id, until it is set.
    /// The eye is heard as "{name} {show|hide}".
    pub fn name(mut self, name: &'a str) -> Self {
        self.name = name;
        self
    }

    /// The grey words of the empty box ("kept" / "not kept": the app's words).
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    /// The width of the box and the eye together.
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// The border in the error colour, and AccessKit `invalid`, like `InputField::error`.
    pub fn error(mut self, on: bool) -> Self {
        self.error = on;
        self
    }

    /// Take-once mode. The typed text is handed out once, on Enter or when the field is left; the box is empty again and
    /// `drafts` keeps nothing. Esc drops it. Tab from the box to the eye keeps the draft (the eye can reveal it); it is
    /// taken when the keys leave the eye.
    pub fn take(self, ui: &mut egui::Ui, l: Look, drafts: &mut Drafts) -> SecretResponse {
        if !ui.is_visible() {
            return self.skipped(ui);
        }
        let id = self.id;
        let mut text = drafts.get(id).map_or_else(String::new, |d| d.text.clone());
        let parts = self.draw(ui, l, &mut text, false);
        let (edit, eye) = (&parts.edit, &parts.eye);
        if edit.has_focus() || edit.changed() {
            drafts.insert(id.to_owned(), Draft { text: text.clone(), sent: None });
        }
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        // The eye keeps the keys it took from the box; the draft waits for them to leave the eye.
        let left_box = edit.lost_focus() && !eye.has_focus();
        let left_eye = eye.lost_focus() && !edit.has_focus() && !parts.eye_pressed;
        let mut taken_text = None;
        if (left_box || left_eye) && !parts.on_eye {
            taken_text = taken(&text, escape);
            drafts.remove(id);
            ui.data_mut(|d| d.remove_temp::<bool>(reveal_id(id)));
        }
        let changed = edit.changed();
        SecretResponse { response: parts.edit, taken: taken_text, changed }
    }

    /// Bound mode, for a form with a save button: the box edits `text` directly, so its value is current on the frame the
    /// save button is clicked. Nothing is handed out; the app reads [`Hidden::expose`] when it saves and clears it after.
    /// The eye masks again when the field was not drawn on the previous frame, and when the app clears `text`.
    pub fn show(self, ui: &mut egui::Ui, l: Look, text: &mut Hidden) -> SecretResponse {
        if !ui.is_visible() {
            return self.skipped(ui);
        }
        let parts = self.draw(ui, l, &mut text.0, true);
        let changed = parts.edit.changed();
        SecretResponse { response: parts.edit, taken: None, changed }
    }

    fn skipped(&self, ui: &mut egui::Ui) -> SecretResponse {
        let response = ui.allocate_response(egui::vec2(self.width, z(ui, 32.0)), egui::Sense::hover());
        SecretResponse { response, taken: None, changed: false }
    }

    /// The box and the eye, for both modes.
    fn draw(&self, ui: &mut egui::Ui, l: Look, text: &mut String, reset_on_clear: bool) -> Parts {
        let c = l.c;
        let ctx = ui.ctx().clone();
        let id = self.id;
        let eid = field_id(id);
        let (reveal_id, eye_id, seen_id) = (reveal_id(id), Id::new(("prefs-secret-eye", id)), Id::new(("prefs-secret-seen", id)));
        // Before drawing: mask again when the field was not drawn on the last frame (a form opened again starts masked),
        // or when the app emptied the text since the last frame.
        let frame = ctx.cumulative_frame_nr();
        let seen = ui.data(|d| d.get_temp::<(u64, bool)>(seen_id));
        let fresh = seen.is_some_and(|(at, _)| at + 1 >= frame);
        let cleared = reset_on_clear && text.is_empty() && seen.is_some_and(|(_, empty)| !empty);
        if !fresh || cleared {
            ui.data_mut(|d| d.remove_temp::<bool>(reveal_id));
        }
        let reveal = ui.data(|d| d.get_temp::<bool>(reveal_id)).unwrap_or(false);
        let mut out = None;
        ui.horizontal(|ui| {
            let eye_w = z(ui, 64.0);
            let edit = egui::TextEdit::singleline(text)
                .id(eid)
                .password(!reveal)
                .hint_text(self.hint)
                .desired_width((self.width - eye_w - ui.spacing().item_spacing.x).max(z(ui, 80.0)))
                .font(FontId::monospace(z(ui, 12.5)))
                .margin(egui::vec2(10.0, 6.0))
                // As tall as the eye button next to it (`button_with`).
                .min_size(egui::vec2(0.0, z(ui, 30.0)));
            let shown = edit.show(ui);
            let resp = shown.response.response;
            take_keys(ui, &resp);
            focus_ring(ui, &resp);
            if self.error {
                let rect = resp.rect;
                ui.painter().rect_stroke(rect, ito_theme::scale::radius::CONTROL, egui::Stroke::new(1.0, c.err), egui::StrokeKind::Inside);
                if !resp.has_focus() {
                    let glow = egui::Stroke::new(2.0, mix(c.bg, c.err, 0.3));
                    ui.painter().rect_stroke(rect.expand(1.5), 7.5, glow, egui::StrokeKind::Outside);
                }
            }
            self.describe_box(&ctx, &resp, eid, reveal);
            // egui keeps copies of the text for undo; a password box needs none.
            if let Some(mut state) = egui::TextEdit::load_state(&ctx, eid) {
                state.clear_undoer();
                state.store(&ctx, eid);
            }
            // Pressing the eye takes the focus off the box; that is not "leaving it".
            // (Read the rect before `input`: `data` inside the `input` closure waits on a lock `input` holds.)
            let eye_rect = ui.data(|d| d.get_temp::<egui::Rect>(eye_id));
            let pressed_now =
                ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| eye_rect.is_some_and(|r| r.contains(p))));
            // egui reports the box's lost focus on the frame after the press, so the press counts for two frames.
            let down_id = Id::new(("prefs-secret-down", id));
            let pressed_before = ui.data(|d| d.get_temp::<u64>(down_id)).is_some_and(|at| at + 1 >= frame);
            if pressed_now {
                ui.data_mut(|d| d.insert_temp(down_id, frame));
            }
            let on_eye = pressed_now || pressed_before;
            let label = if reveal { self.words.hide } else { self.words.show };
            let eye = crate::button_with(ui, l, label, crate::Tone::Plain);
            ui.data_mut(|d| d.insert_temp(eye_id, eye.rect));
            let eye_name = if self.name.is_empty() { label.to_owned() } else { format!("{} {label}", self.name) };
            ito_a11y::say(&eye, ito_a11y::Role::Button, &eye_name);
            self.test_id(&eye, ".eye");
            let eye_pressed = eye.clicked();
            if eye_pressed {
                ui.data_mut(|d| d.insert_temp(reveal_id, !reveal));
            }
            out = Some(Parts { edit: resp, eye, on_eye, eye_pressed });
        });
        let parts = out.expect("the field is drawn");
        ui.data_mut(|d| d.insert_temp(seen_id, (frame, text.is_empty())));
        parts
    }

    /// The test ID `secret.{id}{suffix}`, when it is a valid one.
    fn test_id(&self, resp: &egui::Response, suffix: &str) {
        let name = format!("secret.{}{suffix}", self.id);
        if ito_a11y::is_test_id(&name) {
            ito_a11y::test_id(resp, &name);
        }
    }

    /// The box's AccessKit node: a password input (a plain one while revealed), named, invalid on error.
    /// (`WidgetInfo::labeled` would reset the role to a plain text input, so the node is set directly.)
    fn describe_box(&self, ctx: &egui::Context, resp: &egui::Response, eid: Id, reveal: bool) {
        use egui::accesskit::{Invalid, Role};
        let name = [self.name, self.hint, self.id].into_iter().find(|s| !s.is_empty()).unwrap_or("");
        let (error, role) = (self.error, if reveal { Role::TextInput } else { Role::PasswordInput });
        ctx.accesskit_node_builder(eid, |n| {
            n.set_label(name);
            n.set_role(role);
            if error {
                n.set_invalid(Invalid::True);
            }
        });
        self.test_id(resp, "");
    }
}

struct Parts {
    edit: egui::Response,
    eye: egui::Response,
    /// The pointer went down on the eye this frame or the last.
    on_eye: bool,
    /// The eye was activated (click, or Space / Enter on it) this frame.
    eye_pressed: bool,
}

fn reveal_id(id: &str) -> Id {
    Id::new(("prefs-secret-reveal", id))
}

/// A one-line masked field plus a small show / hide button. Returns the typed secret on Enter or when the field is left
/// (Esc drops it; an empty box returns nothing). The box is empty again afterwards and `drafts` keeps nothing, so the secret
/// lives only in the value returned once. `hint` is the app's: "kept" or "not kept".
///
/// The plain form of `SecretField::new(id, w).hint(hint).width(width).take(ui, l, drafts)`.
pub fn secret_field(ui: &mut egui::Ui, l: Look, drafts: &mut Drafts, id: &str, hint: &str, w: &SecretWords, width: f32) -> Option<String> {
    SecretField::new(id, w).hint(hint).width(width).take(ui, l, drafts).taken
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_while_the_field_is_drawn_does_not_deadlock() {
        let ctx = egui::Context::default();
        let mut drafts = Drafts::default();
        let words = SecretWords { show: "Show", hide: "Hide" };
        let look = Look { c: ito_theme::colors(), q: "" };
        let at = egui::pos2(20.0, 20.0);
        let press =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        for events in [vec![egui::Event::PointerMoved(at)], vec![press(true)], vec![press(false)]] {
            let raw = egui::RawInput { events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                secret_field(ui, look, &mut drafts, "k", "hint", &words, 300.0);
            });
            out.textures_delta.clear();
        }
    }

    #[test]
    fn typed_text_is_taken_once() {
        assert_eq!(taken("hunter2", false).as_deref(), Some("hunter2"));
    }

    #[test]
    fn escape_or_an_empty_box_takes_nothing() {
        assert_eq!(taken("hunter2", true), None);
        assert_eq!(taken("", false), None);
    }

    use egui::{accesskit, Event, Key, Modifiers, PointerButton};

    const WORDS: SecretWords<'static> = SecretWords { show: "Show", hide: "Hide" };

    fn look() -> Look<'static> {
        Look { c: ito_theme::colors(), q: "" }
    }

    /// One frame; the whole output, for the AccessKit tree and the events.
    fn run(ctx: &egui::Context, events: Vec<Event>, mut f: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| f(ui));
        });
        out.textures_delta.clear();
        out
    }

    fn key(k: Key, modifiers: Modifiers) -> Event {
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn typed(s: &str) -> Event {
        Event::Text(s.to_owned())
    }

    fn focus(ctx: &egui::Context, id: &str) {
        ctx.memory_mut(|m| m.request_focus(field_id(id)));
    }

    fn button(pos: egui::Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// Where a test ID was drawn on the last frame.
    fn rect_of(ctx: &egui::Context, name: &str) -> egui::Rect {
        let found = ito_a11y::take_test_ids(ctx, egui::ViewportId::ROOT);
        let names: Vec<&String> = found.iter().map(|e| &e.name).collect();
        found.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} in {names:?}")).rect
    }

    fn node_of<'a>(out: &'a egui::FullOutput, author: &str) -> &'a accesskit::Node {
        let nodes = &out.platform_output.accesskit_update.as_ref().expect("AccessKit is on").nodes;
        nodes.iter().find(|(_, n)| n.author_id() == Some(author)).map(|(_, n)| n).unwrap_or_else(|| panic!("a node {author}"))
    }

    fn bound_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        ctx
    }

    fn bound_frame(ctx: &egui::Context, events: Vec<Event>, text: &mut Hidden) -> (egui::FullOutput, SecretResponse) {
        let mut got = None;
        let out = run(ctx, events, |ui| {
            got = Some(SecretField::new("k", &WORDS).name("Password").hint("not kept").width(300.0).show(ui, look(), text));
        });
        (out, got.expect("drawn"))
    }

    /// A click on the eye: move, press, release.
    fn click_eye(ctx: &egui::Context, mut frame: impl FnMut(Vec<Event>)) {
        let at = rect_of(ctx, "secret.k.eye").center();
        for e in [vec![Event::PointerMoved(at)], vec![button(at, true)], vec![button(at, false)]] {
            frame(e);
        }
    }

    #[test]
    fn bound_mode_edits_the_hidden_and_reports_the_change_the_same_frame() {
        let ctx = bound_ctx();
        let mut text = Hidden::default();
        bound_frame(&ctx, vec![], &mut text);
        focus(&ctx, "k");
        bound_frame(&ctx, vec![], &mut text);
        let (_, r) = bound_frame(&ctx, vec![typed("abc")], &mut text);
        assert!(r.changed && r.response.changed());
        assert_eq!(text.expose(), "abc");
        assert!(r.taken.is_none());
        let (_, r) = bound_frame(&ctx, vec![], &mut text);
        assert!(!r.changed);
    }

    #[test]
    fn a_button_in_the_same_frame_as_the_last_key_sees_the_text() {
        let ctx = bound_ctx();
        let mut text = Hidden::default();
        bound_frame(&ctx, vec![], &mut text);
        focus(&ctx, "k");
        bound_frame(&ctx, vec![], &mut text);
        let mut seen = None;
        run(&ctx, vec![typed("s3cret")], |ui| {
            SecretField::new("k", &WORDS).show(ui, look(), &mut text);
            // The app's save button runs after the field, in the same frame.
            let _ = crate::button(ui, look(), "Save");
            seen = Some(text.expose().to_owned());
        });
        assert_eq!(seen.as_deref(), Some("s3cret"));
    }

    #[test]
    fn enter_does_not_empty_a_bound_box() {
        let ctx = bound_ctx();
        let mut text = Hidden::new("kept");
        bound_frame(&ctx, vec![], &mut text);
        focus(&ctx, "k");
        bound_frame(&ctx, vec![], &mut text);
        let (_, r) = bound_frame(&ctx, vec![key(Key::Enter, Modifiers::NONE)], &mut text);
        assert_eq!(text.expose(), "kept");
        assert!(r.taken.is_none());
    }

    fn take_frame(ctx: &egui::Context, events: Vec<Event>, drafts: &mut Drafts) -> Option<String> {
        let mut taken = None;
        run(ctx, events, |ui| {
            taken = SecretField::new("t", &WORDS).hint("h").take(ui, look(), drafts).taken;
        });
        taken
    }

    #[test]
    fn take_mode_through_the_builder_hands_the_text_out_once() {
        let ctx = egui::Context::default();
        let mut drafts = Drafts::default();
        take_frame(&ctx, vec![], &mut drafts);
        focus(&ctx, "t");
        take_frame(&ctx, vec![], &mut drafts);
        take_frame(&ctx, vec![typed("hunter2")], &mut drafts);
        assert!(drafts.contains_key("t"));
        let taken = take_frame(&ctx, vec![key(Key::Enter, Modifiers::NONE)], &mut drafts);
        assert_eq!(taken.as_deref(), Some("hunter2"));
        assert!(drafts.is_empty());
        // Esc drops it.
        focus(&ctx, "t");
        take_frame(&ctx, vec![], &mut drafts);
        take_frame(&ctx, vec![typed("nope")], &mut drafts);
        let taken = take_frame(&ctx, vec![key(Key::Escape, Modifiers::NONE)], &mut drafts);
        assert_eq!(taken, None);
        assert!(drafts.is_empty());
    }

    #[test]
    fn the_plain_function_still_takes_on_enter() {
        let ctx = egui::Context::default();
        let mut drafts = Drafts::default();
        let mut got = Vec::new();
        for events in [vec![], vec![], vec![typed("tok")], vec![key(Key::Enter, Modifiers::NONE)]] {
            focus(&ctx, "g");
            run(&ctx, events, |ui| {
                got.push(secret_field(ui, look(), &mut drafts, "g", "kept", &WORDS, 280.0));
            });
        }
        assert_eq!(got, [None, None, None, Some("tok".to_owned())]);
    }

    #[test]
    fn the_box_and_the_eye_have_test_ids_and_spoken_names() {
        let ctx = bound_ctx();
        let mut text = Hidden::default();
        let (out, _) = bound_frame(&ctx, vec![], &mut text);
        let found = ito_a11y::take_test_ids(&ctx, egui::ViewportId::ROOT);
        let names: Vec<&str> = found.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"secret.k") && names.contains(&"secret.k.eye"), "{names:?}");
        let edit = node_of(&out, "secret.k");
        assert_eq!(edit.role(), accesskit::Role::PasswordInput);
        assert_eq!(edit.label(), Some("Password"));
        let eye = node_of(&out, "secret.k.eye");
        assert_eq!(eye.role(), accesskit::Role::Button);
        assert_eq!(eye.label(), Some("Password Show"));
    }

    #[test]
    fn without_a_name_the_hint_names_the_box_and_the_words_name_the_eye() {
        let ctx = bound_ctx();
        let out = run(&ctx, vec![], |ui| {
            SecretField::new("k", &WORDS).hint("kept").show(ui, look(), &mut Hidden::default());
        });
        assert_eq!(node_of(&out, "secret.k").label(), Some("kept"));
        assert_eq!(node_of(&out, "secret.k.eye").label(), Some("Show"));
    }

    #[test]
    fn an_id_that_is_not_a_test_id_gets_none_but_still_works() {
        let ctx = bound_ctx();
        run(&ctx, vec![], |ui| {
            SecretField::new("Two Words", &WORDS).show(ui, look(), &mut Hidden::default());
        });
        assert!(ito_a11y::take_test_ids(&ctx, egui::ViewportId::ROOT).is_empty());
    }

    #[test]
    fn an_error_is_invalid_to_a_screen_reader() {
        for (error, want) in [(true, Some(accesskit::Invalid::True)), (false, None)] {
            let ctx = bound_ctx();
            let out = run(&ctx, vec![], |ui| {
                SecretField::new("k", &WORDS).error(error).show(ui, look(), &mut Hidden::default());
            });
            assert_eq!(node_of(&out, "secret.k").invalid(), want, "error {error}");
        }
    }

    #[test]
    fn nothing_in_accesskit_or_the_events_carries_a_masked_secret() {
        // 300 chars cross egui's 255-character run split; no label has this letter.
        let secret = "ж".repeat(300);
        let ctx = bound_ctx();
        let mut text = Hidden::default();
        bound_frame(&ctx, vec![], &mut text);
        focus(&ctx, "k");
        let mut outs = vec![bound_frame(&ctx, vec![], &mut text).0];
        outs.push(bound_frame(&ctx, vec![typed(&secret)], &mut text).0);
        assert_eq!(text.expose(), secret);
        outs.push(bound_frame(&ctx, vec![], &mut text).0);
        outs.push(bound_frame(&ctx, vec![key(Key::ArrowLeft, Modifiers::NONE)], &mut text).0);
        let mut nodes = 0;
        for out in &outs {
            for (_, n) in &out.platform_output.accesskit_update.as_ref().expect("update").nodes {
                nodes += 1;
                // The whole node, every string in it.
                assert!(!format!("{n:?}").contains('ж'), "a node carries the secret");
                for s in [n.label(), n.value(), n.description(), n.placeholder()].into_iter().flatten() {
                    assert!(!s.contains('ж'));
                }
            }
            for e in &out.platform_output.events {
                assert!(!format!("{e:?}").contains('ж'), "an event carries the secret: {e:?}");
            }
        }
        assert!(nodes > 0);
        assert_eq!(node_of(&outs[1], "secret.k").role(), accesskit::Role::PasswordInput);
    }

    fn box_role(ctx: &egui::Context, text: &mut Hidden) -> accesskit::Role {
        let (out, _) = bound_frame(ctx, vec![], text);
        node_of(&out, "secret.k").role()
    }

    #[test]
    fn a_revealed_box_is_a_plain_text_input_that_shows_the_text() {
        let ctx = bound_ctx();
        let mut text = Hidden::new("abc");
        bound_frame(&ctx, vec![], &mut text);
        click_eye(&ctx, |e| {
            bound_frame(&ctx, e, &mut text);
        });
        let (out, _) = bound_frame(&ctx, vec![], &mut text);
        let edit = node_of(&out, "secret.k");
        assert_eq!(edit.role(), accesskit::Role::TextInput);
        assert_eq!(edit.label(), Some("Password"));
        assert_eq!(node_of(&out, "secret.k.eye").label(), Some("Password Hide"));
    }

    #[test]
    fn egui_keeps_no_undo_copies_of_the_secret() {
        let ctx = bound_ctx();
        let mut text = Hidden::default();
        bound_frame(&ctx, vec![], &mut text);
        focus(&ctx, "k");
        for s in ["a", "b", "c"] {
            bound_frame(&ctx, vec![typed(s)], &mut text);
        }
        assert_eq!(text.expose(), "abc");
        let state = egui::TextEdit::load_state(&ctx, field_id("k")).expect("state");
        let other = (egui::text::CCursorRange::default(), "zzz".to_owned());
        assert!(!state.undoer().has_undo(&other));
        assert!(!state.undoer().is_in_flux());
    }

    #[test]
    fn hidden_never_prints_its_text() {
        let shown = format!("{:?}", Hidden::new("hunter2"));
        assert!(!shown.contains("hunter2") && !shown.contains('7'), "{shown}");
        assert_eq!(format!("{:?}", Hidden::default()), "Hidden(empty)");
        let mut h = Hidden::new("x");
        assert_eq!(h.take(), "x");
        assert!(h.is_empty());
    }

    #[test]
    fn the_reveal_resets_after_a_frame_without_the_field() {
        let ctx = bound_ctx();
        let mut text = Hidden::new("abc");
        bound_frame(&ctx, vec![], &mut text);
        click_eye(&ctx, |e| {
            bound_frame(&ctx, e, &mut text);
        });
        assert_eq!(box_role(&ctx, &mut text), accesskit::Role::TextInput);
        run(&ctx, vec![], |_| {});
        assert_eq!(box_role(&ctx, &mut text), accesskit::Role::PasswordInput);
    }

    #[test]
    fn the_reveal_resets_after_the_app_clears_the_text() {
        let ctx = bound_ctx();
        let mut text = Hidden::new("abc");
        bound_frame(&ctx, vec![], &mut text);
        click_eye(&ctx, |e| {
            bound_frame(&ctx, e, &mut text);
        });
        assert_eq!(box_role(&ctx, &mut text), accesskit::Role::TextInput);
        text.clear();
        assert_eq!(box_role(&ctx, &mut text), accesskit::Role::PasswordInput);
    }

    #[test]
    fn two_secret_fields_share_neither_text_nor_reveal() {
        let ctx = bound_ctx();
        let (mut a, mut b) = (Hidden::new("aaa"), Hidden::default());
        let draw = |ctx: &egui::Context, events: Vec<Event>, a: &mut Hidden, b: &mut Hidden| {
            run(ctx, events, |ui| {
                SecretField::new("one", &WORDS).show(ui, look(), a);
                SecretField::new("two", &WORDS).show(ui, look(), b);
            })
        };
        draw(&ctx, vec![], &mut a, &mut b);
        let at = rect_of(&ctx, "secret.one.eye").center();
        for e in [vec![Event::PointerMoved(at)], vec![button(at, true)], vec![button(at, false)]] {
            draw(&ctx, e, &mut a, &mut b);
        }
        let out = draw(&ctx, vec![], &mut a, &mut b);
        assert_eq!(node_of(&out, "secret.one").role(), accesskit::Role::TextInput);
        assert_eq!(node_of(&out, "secret.two").role(), accesskit::Role::PasswordInput);
        assert_eq!((a.expose(), b.expose()), ("aaa", ""));
    }

    /// Take mode with a button after the field, to give the keys somewhere to go.
    fn keys_frame(ctx: &egui::Context, events: Vec<Event>, drafts: &mut Drafts) -> (Option<String>, egui::FullOutput) {
        let mut taken = None;
        let out = run(ctx, events, |ui| {
            taken = SecretField::new("k", &WORDS).name("Password").take(ui, look(), drafts).taken;
            let _ = crate::button(ui, look(), "Next");
        });
        (taken, out)
    }

    fn tab() -> Event {
        key(Key::Tab, Modifiers::NONE)
    }

    #[test]
    fn tab_to_the_eye_keeps_the_draft_and_space_reveals_it() {
        let ctx = bound_ctx();
        let mut drafts = Drafts::default();
        keys_frame(&ctx, vec![], &mut drafts);
        focus(&ctx, "k");
        keys_frame(&ctx, vec![], &mut drafts);
        keys_frame(&ctx, vec![typed("abc")], &mut drafts);
        let (taken, _) = keys_frame(&ctx, vec![tab()], &mut drafts);
        assert_eq!(taken, None, "the box was left for the eye");
        assert_eq!(drafts.get("k").map(|d| d.text.as_str()), Some("abc"));
        let (taken, _) = keys_frame(&ctx, vec![], &mut drafts);
        assert_eq!(taken, None);
        // Space on the focused eye reveals.
        let (taken, _) = keys_frame(&ctx, vec![key(Key::Space, Modifiers::NONE)], &mut drafts);
        assert_eq!(taken, None);
        let (_, out) = keys_frame(&ctx, vec![], &mut drafts);
        assert_eq!(node_of(&out, "secret.k").role(), accesskit::Role::TextInput);
        // The keys leave the eye for the next control: now it is taken.
        let (taken, _) = keys_frame(&ctx, vec![tab()], &mut drafts);
        assert_eq!(taken.as_deref(), Some("abc"));
        assert!(drafts.is_empty());
        let (_, out) = keys_frame(&ctx, vec![], &mut drafts);
        assert_eq!(node_of(&out, "secret.k").role(), accesskit::Role::PasswordInput, "masked again after the take");
    }

    #[test]
    fn tab_past_the_eye_with_nothing_typed_takes_nothing() {
        let ctx = bound_ctx();
        let mut drafts = Drafts::default();
        keys_frame(&ctx, vec![], &mut drafts);
        focus(&ctx, "k");
        keys_frame(&ctx, vec![], &mut drafts);
        for _ in 0..3 {
            assert_eq!(keys_frame(&ctx, vec![tab()], &mut drafts).0, None);
        }
        assert!(drafts.is_empty());
    }

    #[test]
    fn a_press_on_the_eye_does_not_take_and_esc_after_a_reveal_drops_and_masks() {
        let ctx = bound_ctx();
        let mut drafts = Drafts::default();
        keys_frame(&ctx, vec![], &mut drafts);
        focus(&ctx, "k");
        keys_frame(&ctx, vec![], &mut drafts);
        keys_frame(&ctx, vec![typed("abc")], &mut drafts);
        let at = rect_of(&ctx, "secret.k.eye").center();
        for (i, e) in [vec![Event::PointerMoved(at)], vec![button(at, true)], vec![button(at, false)]].into_iter().enumerate() {
            assert_eq!(keys_frame(&ctx, e, &mut drafts).0, None, "frame {i}");
        }
        assert_eq!(drafts.get("k").map(|d| d.text.as_str()), Some("abc"));
        let (_, out) = keys_frame(&ctx, vec![], &mut drafts);
        assert_eq!(node_of(&out, "secret.k").role(), accesskit::Role::TextInput);
        // Back into the box, then Esc.
        focus(&ctx, "k");
        keys_frame(&ctx, vec![], &mut drafts);
        let (taken, _) = keys_frame(&ctx, vec![key(Key::Escape, Modifiers::NONE)], &mut drafts);
        assert_eq!(taken, None);
        assert!(drafts.is_empty());
        let (_, out) = keys_frame(&ctx, vec![], &mut drafts);
        assert_eq!(node_of(&out, "secret.k").role(), accesskit::Role::PasswordInput);
    }

    #[test]
    fn a_field_in_error_draws_an_error_border() {
        let ctx = egui::Context::default();
        let c = ito_theme::colors();
        let out = run(&ctx, vec![], |ui| {
            SecretField::new("k", &WORDS).error(true).take(ui, look(), &mut Drafts::default());
        });
        let has_err = out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.stroke.color == c.err));
        assert!(has_err, "a rect stroked in the error colour");
    }
}
