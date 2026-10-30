//! A selectable row for a navigation list (mail folders, branches, places, calendars): an optional glyph, the
//! text, and a count at the right edge. The app owns the keys and the focus; this draws `current` and reports clicks.

use egui::{Align2, FontId, Sense, Vec2};
use ito_theme::mix;

use crate::{focus_ring, take_keys, z, Look};

/// What a navigation row says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row<'a> {
    /// A short glyph before the text (one character or symbol); empty for none.
    pub icon: &'a str,
    pub text: &'a str,
    /// Shown dimmed at the right edge; `None` for no count.
    pub count: Option<usize>,
    /// Unread: the text is stronger and the count is in the accent colour.
    pub strong: bool,
    /// The row the list is on now.
    pub current: bool,
}

impl<'a> Row<'a> {
    pub fn new(text: &'a str) -> Self {
        Row { text, ..Row::default() }
    }
}

/// The row's height, before the text zoom.
const HEIGHT: f32 = 28.0;

/// A row, the full width of the column. The same height for every row.
pub fn nav_row(ui: &mut egui::Ui, l: Look, row: Row) -> egui::Response {
    let c = l.c;
    let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), z(ui, HEIGHT)), Sense::click());
    take_keys(ui, &r);
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fill = if row.current {
            Some(mix(c.side, c.run, 0.22))
        } else if r.hovered() {
            Some(mix(c.side, c.fg, 0.08))
        } else {
            None
        };
        if let Some(f) = fill {
            p.rect_filled(rect, 6.0, f);
        }
        let ink = if row.current || row.strong { c.strong() } else { c.fg };
        let font = FontId::proportional(z(ui, 12.5));
        let pad = z(ui, 10.0);
        let mut x = rect.min.x + pad;
        if !row.icon.is_empty() {
            let g = p.text(
                egui::pos2(x, rect.center().y),
                Align2::LEFT_CENTER,
                row.icon,
                font.clone(),
                if row.current { c.run } else { c.dim },
            );
            x = g.max.x + z(ui, 8.0);
        }
        let mut right = rect.max.x - pad;
        if let Some(n) = row.count {
            let color = if row.strong { c.run } else { c.dim };
            let g =
                p.text(egui::pos2(right, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), FontId::proportional(z(ui, 11.5)), color);
            right = g.min.x - z(ui, 8.0);
        }
        // The text is cut with "…" where the count begins.
        let width_of = |s: &str| p.layout_no_wrap(s.to_owned(), font.clone(), ink).size().x;
        let shown = crate::tag::fit(row.text, (right - x).max(0.0), width_of);
        p.text(egui::pos2(x, rect.center().y), Align2::LEFT_CENTER, shown, font, ink);
    }
    focus_ring(ui, &r);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_is_the_full_width_and_one_height() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let l = Look { c: ito_theme::colors(), q: "" };
            let a = nav_row(ui, l, Row { icon: "▸", text: "Inbox", count: Some(12), strong: true, current: true });
            let b = nav_row(ui, l, Row::new("A folder with a very very very very very very very long name that must be cut"));
            let d = nav_row(ui, l, Row { count: Some(0), ..Row::new("Sent") });
            assert_eq!(a.rect.height(), b.rect.height());
            assert_eq!(b.rect.height(), d.rect.height());
            assert_eq!(a.rect.width(), b.rect.width());
            assert!(!a.clicked());
        });
        out.textures_delta.clear();
    }
}
