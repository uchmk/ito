//! The writing box: a multi-line field (or the Markdown preview) in a rectangle the caller gives, with Ctrl+B / I / K /
//! U on the selection, Ctrl+P for the preview, Ctrl+Enter to send, Esc to leave. The toolbar, the buttons and the
//! panel around it are the app's; it feeds a mark from its toolbar with [`Writing::apply`].

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{pos2, Align2, FontId, Id, Key, Modifiers, Rect, Ui, UiBuilder};

use crate::{layout, tool, Tool};

/// What the box asks of the app after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    Stay,
    /// Ctrl+Enter sends; Ctrl+Shift+Enter sends and closes (`close`).
    Send {
        close: bool,
    },
    /// Esc with the preview off.
    Leave,
}

/// The text of one box, whether it shows the preview, and a mark waiting for the next frame.
pub struct Writing {
    pub text: String,
    pub preview: bool,
    /// Take the focus back whenever it is lost, and consume the shortcuts whatever widget has focus (the default).
    /// Off: the box only reacts to the keys while it has focus (or shows the preview), and leaves them to others otherwise.
    pub grab: bool,
    id: Id,
    pending: Option<Tool>,
}

impl Writing {
    /// `id`: any value that is unique to this box (it keys the field's cursor and the preview's scroll).
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug) -> Writing {
        Writing { text: String::new(), preview: false, grab: true, id: Id::new(("ito-markdown-writing", id)), pending: None }
    }

    /// Ask for the focus at the next [`show`](Writing::show), once (for `grab` off).
    pub fn focus(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(self.id));
    }

    /// The field's id, to ask for or test focus.
    pub fn id(&self) -> Id {
        self.id
    }

    /// A mark on the selection at the next [`show`](Writing::show), from the app's toolbar.
    pub fn apply(&mut self, t: Tool) {
        self.pending = Some(t);
    }

    /// Draw the box in `field`. `empty`: what the preview says with nothing typed. `size`: the font size.
    /// The shortcuts above are consumed first, so the field never sees them.
    pub fn show(&mut self, ui: &mut Ui, field: Rect, empty: &str, size: f32) -> Out {
        let ctx = ui.ctx().clone();
        let listen = self.grab || self.preview || ctx.memory(|m| m.has_focus(self.id));
        let (send_close, send, preview, bold, italic, link, under, esc) = ctx.input_mut(|i| {
            if !listen {
                return (false, false, false, false, false, false, false, false);
            }
            (
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::Enter),
                i.consume_key(Modifiers::COMMAND, Key::P),
                i.consume_key(Modifiers::COMMAND, Key::B),
                i.consume_key(Modifiers::COMMAND, Key::I),
                i.consume_key(Modifiers::COMMAND, Key::K),
                i.consume_key(Modifiers::COMMAND, Key::U),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let mut out = Out::Stay;
        if send_close || send {
            out = Out::Send { close: send_close };
        } else if esc {
            if self.preview {
                self.preview = false;
            } else {
                out = Out::Leave;
            }
        }
        if preview {
            self.preview = !self.preview;
        }
        let key_tool = [(bold, Tool::Bold), (italic, Tool::Italic), (link, Tool::Link), (under, Tool::Underline)]
            .into_iter()
            .find_map(|(on, t)| on.then_some(t));
        if let Some(t) = key_tool.or(self.pending.take()) {
            let mut st = TextEditState::load(&ctx, self.id).unwrap_or_default();
            let end = self.text.chars().count();
            let (a, b) = st.cursor.char_range().map_or((end, end), |r| {
                let (x, y): (usize, usize) = (r.primary.index.into(), r.secondary.index.into());
                (x.min(y), x.max(y))
            });
            let (text, a, b) = tool(&self.text, a, b, t);
            self.text = text;
            st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
            st.store(&ctx, self.id);
        }
        if self.preview {
            self.draw_preview(ui, field, empty, size);
        } else {
            let mut child = ui.new_child(UiBuilder::new().max_rect(field));
            child.set_clip_rect(field);
            let rows = ((field.height() / (size * 1.4)) as usize).max(2);
            let edit = egui::TextEdit::multiline(&mut self.text)
                .id(self.id)
                .frame(egui::Frame::NONE)
                .desired_width(field.width())
                .desired_rows(rows)
                .lock_focus(true)
                .font(FontId::proportional(size));
            let hidden = egui::scroll_area::ScrollBarVisibility::AlwaysHidden;
            egui::ScrollArea::vertical()
                .id_salt(self.id.with("scroll"))
                .max_height(field.height())
                .min_scrolled_height(field.height())
                .auto_shrink([false, false])
                .scroll_bar_visibility(hidden)
                .show(&mut child, |ui| ui.add(edit));
            // Only when it has lost it: asking again ends an IME composition.
            ctx.memory_mut(|m| {
                if self.grab && !m.has_focus(self.id) {
                    m.request_focus(self.id)
                }
            });
        }
        out
    }

    /// The preview from its top; the wheel over it scrolls it (3 lines a notch).
    fn draw_preview(&self, ui: &Ui, field: Rect, empty: &str, size: f32) {
        let ctx = ui.ctx();
        let c = ito_theme::colors();
        let p = ui.painter().with_clip_rect(field);
        if self.text.trim().is_empty() {
            p.text(pos2(field.left(), field.top() + 10.0), Align2::LEFT_CENTER, empty, FontId::proportional(size - 1.0), c.dim);
            return;
        }
        let md = layout(&p, &self.text, field.width(), size);
        // Back at the top when the preview was not there the pass before.
        let pass = ctx.cumulative_pass_nr();
        let key = self.id.with("preview-top");
        let (top, last) = ctx.data(|d| d.get_temp::<(f32, u64)>(key)).unwrap_or_default();
        let mut top = if last + 1 >= pass { top } else { 0.0 };
        if ctx.rect_contains_pointer(ui.layer_id(), field) {
            let dy = ctx.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y));
            top -= dy;
        }
        let top = top.clamp(0.0, (md.height - field.height()).max(0.0));
        ctx.data_mut(|d| d.insert_temp(key, (top, pass)));
        md.paint(&p, field.left(), field.top() - top);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: &mut Writing, events: Vec<egui::Event>) -> Out {
        let ctx = egui::Context::default();
        let mut out = Out::Stay;
        let input =
            egui::RawInput { events, screen_rect: Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 300.0))), ..Default::default() };
        ctx.run_ui(input, |ui| {
            out = w.show(ui, Rect::from_min_max(pos2(10.0, 10.0), pos2(390.0, 200.0)), "empty", 13.5);
        })
        .drop_without_applying_deltas();
        out
    }

    fn key(k: Key, m: Modifiers) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }
    }

    #[test]
    fn keys_send_toggle_the_preview_and_leave() {
        let mut w = Writing::new("t");
        w.text = "# hi\n\n- a\n- b".into();
        assert_eq!(frame(&mut w, vec![key(Key::Enter, Modifiers::COMMAND)]), Out::Send { close: false });
        assert_eq!(frame(&mut w, vec![key(Key::Enter, Modifiers::COMMAND | Modifiers::SHIFT)]), Out::Send { close: true });
        frame(&mut w, vec![key(Key::P, Modifiers::COMMAND)]);
        assert!(w.preview);
        assert_eq!(frame(&mut w, vec![key(Key::Escape, Modifiers::NONE)]), Out::Stay);
        assert!(!w.preview);
        assert_eq!(frame(&mut w, vec![key(Key::Escape, Modifiers::NONE)]), Out::Leave);
    }

    #[test]
    fn without_grab_the_keys_are_left_to_others_until_it_has_focus() {
        let mut w = Writing::new("t3");
        w.grab = false;
        assert_eq!(frame(&mut w, vec![key(Key::Enter, Modifiers::COMMAND)]), Out::Stay);
        // The preview still listens, so Esc can leave it.
        w.preview = true;
        frame(&mut w, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!w.preview);
    }

    #[test]
    fn a_toolbar_mark_applies_on_the_next_frame() {
        let mut w = Writing::new("t2");
        w.apply(Tool::Bold);
        frame(&mut w, vec![]);
        assert_eq!(w.text, "****");
    }
}
