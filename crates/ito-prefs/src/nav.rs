//! A selectable row for a navigation list (mail folders, branches, places, calendars): an optional mark, the
//! text, and a count at the right edge. The app owns the keys and the focus; this draws `current` and reports clicks.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use ito_theme::mix;

use crate::{focus_ring, mark, take_keys, z, Look, Mark};

/// What a navigation row says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Row<'a> {
    /// A short glyph before the text (one character or symbol); empty for none. `mark` and `dot` win over it.
    pub icon: &'a str,
    pub text: &'a str,
    /// Shown dimmed at the right edge; `None` for no count. On the current row it is a pill in the accent colour.
    pub count: Option<usize>,
    /// Unread: the text is stronger and the count is in the accent colour.
    pub strong: bool,
    /// The row the list is on now.
    pub current: bool,
    /// A drawn mark in the icon column (it follows the theme). The names of rows with and without one line up.
    pub mark: Option<Mark>,
    /// A colour dot in the icon column, for a label.
    pub dot: Option<Color32>,
}

impl<'a> Row<'a> {
    pub fn new(text: &'a str) -> Self {
        Row { text, ..Row::default() }
    }
}

/// The row's height, before the text zoom.
const HEIGHT: f32 = 28.0;
/// The icon column's width, before the text zoom.
const COLUMN: f32 = 16.0;

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
        let icon_ink = if row.current { c.run } else { c.dim };
        if row.mark.is_some() || row.dot.is_some() {
            // A fixed column, so every name starts at the same x.
            let col = z(ui, COLUMN);
            let centre = Pos2::new(x + col / 2.0, rect.center().y);
            if let Some(m) = row.mark {
                mark(p, Rect::from_center_size(centre, Vec2::splat(z(ui, 14.0))), m, icon_ink);
            } else if let Some(d) = row.dot {
                p.circle_filled(centre, z(ui, 4.5), d);
            }
            x += col + z(ui, 8.0);
        } else if !row.icon.is_empty() {
            let g = p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, row.icon, font.clone(), icon_ink);
            x = g.max.x + z(ui, 8.0);
        }
        let mut right = rect.max.x - pad;
        if let Some(n) = row.count {
            let small = FontId::proportional(z(ui, 11.5));
            if row.current {
                // The accent pill that says where the list is.
                let galley = p.layout_no_wrap(n.to_string(), small, c.on_accent());
                let size = Vec2::new((galley.size().x + z(ui, 12.0)).max(z(ui, 20.0)), z(ui, 18.0));
                let pill = Rect::from_min_size(Pos2::new(right - size.x, rect.center().y - size.y / 2.0), size);
                p.rect(pill, size.y / 2.0, c.run, Stroke::NONE, egui::StrokeKind::Inside);
                p.galley(pill.center() - galley.size() / 2.0, galley, c.on_accent());
                right = pill.min.x - z(ui, 8.0);
            } else {
                let color = if row.strong { c.run } else { c.dim };
                let g = p.text(Pos2::new(right, rect.center().y), Align2::RIGHT_CENTER, n.to_string(), small, color);
                right = g.min.x - z(ui, 8.0);
            }
        }
        // The text is cut with "…" where the count begins.
        let width_of = |s: &str| p.layout_no_wrap(s.to_owned(), font.clone(), ink).size().x;
        let shown = crate::tag::fit(row.text, (right - x).max(0.0), width_of);
        p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, shown, font, ink);
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
            let a = nav_row(ui, l, Row { icon: "▸", text: "Inbox", count: Some(12), strong: true, current: true, ..Row::default() });
            let b = nav_row(ui, l, Row::new("A folder with a very very very very very very very long name that must be cut"));
            let d = nav_row(ui, l, Row { count: Some(0), ..Row::new("Sent") });
            let m = nav_row(ui, l, Row { mark: Some(Mark::Inbox), count: Some(3), current: true, ..Row::new("Inbox") });
            let o = nav_row(ui, l, Row { dot: Some(Color32::RED), ..Row::new("Work") });
            for x in [&b, &d, &m, &o] {
                assert_eq!(a.rect.height(), x.rect.height());
                assert_eq!(a.rect.width(), x.rect.width());
            }
            assert!(!a.clicked());
        });
        out.textures_delta.clear();
    }
}
