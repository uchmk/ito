//! A masked field for a secret (a token, a password, an OAuth client secret) that the app hands to `ito-secret`.
//!
//! Unlike [`crate::field`] it never shows what is kept: the box starts empty, the hint says whether a secret is kept, and the
//! typed text is dropped from the [`Drafts`](crate::Drafts) the moment it is taken.

use crate::{field_id, focus_ring, take_keys, z, Draft, Drafts, Look};
use egui::FontId;

/// The words of [`secret_field`]'s eye button.
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

/// A one-line masked field plus a small show / hide button. Returns the typed secret on Enter or when the field is left
/// (Esc drops it; an empty box returns nothing). The box is empty again afterwards and `drafts` keeps nothing, so the secret
/// lives only in the value returned once. `hint` is the app's: "kept" or "not kept".
pub fn secret_field(ui: &mut egui::Ui, l: Look, drafts: &mut Drafts, id: &str, hint: &str, w: &SecretWords, width: f32) -> Option<String> {
    if !ui.is_visible() {
        ui.allocate_space(egui::vec2(width, z(ui, 32.0)));
        return None;
    }
    let eid = field_id(id);
    let reveal_id = egui::Id::new(("prefs-secret-reveal", id));
    let eye_id = egui::Id::new(("prefs-secret-eye", id));
    let reveal = ui.data(|d| d.get_temp::<bool>(reveal_id)).unwrap_or(false);
    let mut text = drafts.get(id).map_or_else(String::new, |d| d.text.clone());
    let mut taken_text = None;
    ui.horizontal(|ui| {
        let eye_w = z(ui, 64.0);
        let edit = egui::TextEdit::singleline(&mut text)
            .id(eid)
            .password(!reveal)
            .hint_text(hint)
            .desired_width((width - eye_w - ui.spacing().item_spacing.x).max(z(ui, 80.0)))
            .font(FontId::monospace(z(ui, 12.5)))
            .margin(egui::vec2(10.0, 6.0));
        let shown = edit.show(ui);
        let resp = &shown.response.response;
        take_keys(ui, resp);
        focus_ring(ui, resp);
        if resp.has_focus() || resp.changed() {
            drafts.insert(id.to_owned(), Draft { text: text.clone(), sent: None });
        }
        // Pressing the eye takes the focus off the field; that is not "leaving it".
        let on_eye = ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| ui.data(|d| d.get_temp::<egui::Rect>(eye_id)).is_some_and(|r| r.contains(p)))
        });
        if resp.lost_focus() && !on_eye {
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            taken_text = taken(&text, escape);
            drafts.remove(id);
        }
        let label = if reveal { w.hide } else { w.show };
        let eye = crate::button_with(ui, l, label, crate::Tone::Plain);
        ui.data_mut(|d| d.insert_temp(eye_id, eye.rect));
        if eye.clicked() {
            ui.data_mut(|d| d.insert_temp(reveal_id, !reveal));
        }
    });
    if taken_text.is_some() {
        ui.data_mut(|d| d.remove_temp::<bool>(reveal_id));
    }
    taken_text
}

#[cfg(test)]
mod tests {
    use super::taken;

    #[test]
    fn typed_text_is_taken_once() {
        assert_eq!(taken("hunter2", false).as_deref(), Some("hunter2"));
    }

    #[test]
    fn escape_or_an_empty_box_takes_nothing() {
        assert_eq!(taken("hunter2", true), None);
        assert_eq!(taken("", false), None);
    }
}
