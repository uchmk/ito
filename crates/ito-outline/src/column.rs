//! The outline as a column on the right of a text pane (feature `egui`; the sums are plain functions and need no
//! window). The entry being read is marked and kept in view, a click on an entry says where to jump to.

use unicode_width::UnicodeWidthStr;

use crate::Heading;

/// One row of the column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub level: u8,
    /// Indented by its depth already; what does not fit is elided when drawn.
    pub label: String,
    /// What the column compares with the top of the pane and hands back on a click: a line, or a byte; the app says which.
    pub target: usize,
}

/// The rows for `headings`, indented two cells per level below the outermost one, each jumping to `target(heading)`.
pub fn items(headings: &[Heading], target: impl Fn(&Heading) -> usize) -> Vec<Item> {
    let top = headings.iter().map(|h| h.level).min().unwrap_or(1);
    headings
        .iter()
        .map(|h| Item { level: h.level, label: format!("{}{}", "  ".repeat((h.level - top) as usize), h.title), target: target(h) })
        .collect()
}

/// Terminal cells a text takes: a full-width letter is two.
pub fn cells(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Width of the column, in cells, whose widest label takes `widest`, in a pane `cols` wide.
pub fn column_cols(widest: usize, cols: u16) -> u16 {
    let max = 32.min(cols / 3);
    (widest.min(100) as u16 + 1).clamp(16.min(max), max)
}

/// Width of the column of `items` in a pane `cols` wide.
pub fn cols_for(items: &[Item], cols: u16) -> u16 {
    column_cols(items.iter().map(|i| cells(&i.label)).max().unwrap_or(0), cols)
}

/// The entry being read: the last one whose target is at or before `top`.
pub fn current(items: &[Item], top: usize) -> Option<usize> {
    items.iter().rposition(|i| i.target <= top)
}

/// The first row to draw so `current` is in the middle of `slots` rows (and the list does not end short).
pub fn first_row(current: Option<usize>, len: usize, slots: usize) -> usize {
    current.map_or(0, |c| c.saturating_sub(slots / 2)).min(len.saturating_sub(slots))
}

#[cfg(feature = "egui")]
pub use draw::{sep_x, show, Look};

#[cfg(feature = "egui")]
mod draw {
    use egui::text::LayoutJob;
    use egui::{pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Sense, Stroke, StrokeKind, TextFormat, Ui};

    use super::{current, first_row, Item};

    /// How the pane lays its text out: the column is drawn on the same grid.
    pub struct Look {
        /// Width of one cell and height of one row, in points.
        pub cell: f32,
        pub row_h: f32,
        pub font: FontId,
        /// The marked entry's font, when the app has a bold one.
        pub bold: Option<FontId>,
        /// The heading above the rows ("Contents"), in the app's language.
        pub title: String,
    }

    /// Where the rule left of a column `cols` wide goes, in a pane `rect`.
    pub fn sep_x(rect: Rect, cols: u16, look: &Look) -> f32 {
        rect.right() - 8.0 - cols as f32 * look.cell - 1.5 * look.cell
    }

    fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
        let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
    }

    /// Draw the column at the right of `rect`, `cols` cells wide. `cursor` is the entry the keys are on (the column
    /// then has the focus colors); otherwise the entry being read, from `top` (the first line on screen, in
    /// `Item::target`'s unit), is marked. `overlay` covers the text beneath it. Returns the index of the entry clicked.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        ui: &Ui,
        rect: Rect,
        items: &[Item],
        cols: u16,
        top: usize,
        cursor: Option<usize>,
        overlay: bool,
        look: &Look,
    ) -> Option<usize> {
        let c = ito_theme::colors();
        let painter = ui.painter();
        let glyph_h = painter.layout_no_wrap("M".into(), look.font.clone(), c.fg).size().y;
        let lift = ((look.row_h - glyph_h) / 2.0).max(0.0);
        let width = cols as f32 * look.cell;
        let sep = sep_x(rect, cols, look);
        let left = sep + 1.5 * look.cell;
        let focused = cursor.is_some();
        if overlay {
            painter.rect_filled(Rect::from_x_y_ranges(sep..=rect.right(), rect.y_range()), CornerRadius::ZERO, c.bg);
        }
        let rule = if focused { c.blue } else { c.border };
        painter.vline(sep, rect.top() + 4.0..=rect.bottom() - 4.0, Stroke::new(1.0, rule));
        let (font, color) = match (&look.bold, focused) {
            (Some(b), true) => (b.clone(), c.blue),
            (None, true) => (look.font.clone(), c.blue),
            (_, false) => (look.font.clone(), c.dim),
        };
        painter.text(pos2(left, rect.top() + lift), Align2::LEFT_TOP, &look.title, font, color);

        let top_level = items.iter().map(|i| i.level).min().unwrap_or(1);
        let cur = match cursor {
            Some(k) => Some(k.min(items.len().saturating_sub(1))),
            None => current(items, top),
        };
        let slots = ((rect.height() / look.row_h) as usize).saturating_sub(1).max(1);
        let first = first_row(cur, items.len(), slots);
        let clip = painter.with_clip_rect(Rect::from_x_y_ranges(sep + 1.0..=rect.right(), rect.y_range()));
        let hover = mix(c.border, c.dim, 0.4);

        let mut jump = None;
        for (k, item) in items.iter().enumerate().skip(first).take(slots) {
            let y = rect.top() + (k - first + 1) as f32 * look.row_h;
            let row = Rect::from_min_size(pos2(left - 4.0, y), vec2(width + 8.0, look.row_h));
            let resp = ui.interact(row, ui.id().with(("ito-outline", k)), Sense::click());
            let here = cur == Some(k);
            // Bright only where the keys are, so the entry being read stays quiet.
            if here {
                clip.rect_filled(row, CornerRadius::same(3), mix(c.panel, c.fg, if focused { 0.18 } else { 0.08 }));
            } else if resp.hovered() {
                clip.rect_stroke(row, CornerRadius::same(3), Stroke::new(1.0, hover), StrokeKind::Inside);
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                jump = Some(k);
            }
            let color = if here || item.level == top_level { c.fg } else { c.dim };
            let font_id = match (&look.bold, here) {
                (Some(b), true) => b.clone(),
                _ => look.font.clone(),
            };
            let mut job = LayoutJob::single_section(item.label.clone(), TextFormat { font_id, color, ..Default::default() });
            job.wrap.max_width = width;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            job.wrap.overflow_character = Some('…');
            clip.galley(pos2(left, y + lift), clip.layout_job(job), color);
        }
        jump
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads() -> Vec<Heading> {
        crate::headings("# One\n\n## Two\n\n### Three\n\n# 四\n", 6)
    }

    #[test]
    fn rows_are_indented_by_depth_and_jump_where_asked() {
        let it = items(&heads(), |h| h.line);
        let l: Vec<_> = it.iter().map(|i| (i.label.as_str(), i.target)).collect();
        assert_eq!(l, vec![("One", 0), ("  Two", 2), ("    Three", 4), ("四", 6)]);
        assert!(items(&[], |h| h.line).is_empty());
    }

    #[test]
    fn full_width_letters_take_two_cells() {
        assert_eq!(cells("abc"), 3);
        assert_eq!(cells("設定"), 4);
    }

    #[test]
    fn the_column_is_between_16_and_32_cells_and_a_third_of_the_pane() {
        assert_eq!(column_cols(3, 120), 16);
        assert_eq!(column_cols(20, 120), 21);
        assert_eq!(column_cols(99, 120), 32);
        assert_eq!(column_cols(20, 60), 20);
        assert_eq!(column_cols(40, 30), 10);
    }

    #[test]
    fn the_entry_being_read_is_the_last_one_at_or_above_the_top() {
        let it = items(&heads(), |h| h.line);
        assert_eq!(current(&it, 0), Some(0));
        assert_eq!(current(&it, 3), Some(1));
        assert_eq!(current(&it, 99), Some(3));
        assert_eq!(current(&[], 5), None);
    }

    #[test]
    fn the_window_of_rows_keeps_the_current_one_in_the_middle() {
        assert_eq!(first_row(Some(10), 30, 5), 8);
        assert_eq!(first_row(Some(1), 30, 5), 0);
        assert_eq!(first_row(Some(29), 30, 5), 25);
        assert_eq!(first_row(None, 30, 5), 0);
        assert_eq!(first_row(Some(2), 3, 5), 0);
    }

    #[cfg(feature = "egui")]
    #[test]
    fn the_column_draws_in_a_headless_frame() {
        let it = items(&heads(), |h| h.line);
        let look = Look { cell: 8.0, row_h: 16.0, font: egui::FontId::monospace(13.0), bold: None, title: "Contents".into() };
        let ctx = egui::Context::default();
        let mut jump = Some(99);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(600.0, 300.0));
            jump = show(ui, rect, &it, cols_for(&it, 75), 3, None, false, &look);
        })
        .drop_without_applying_deltas();
        assert_eq!(jump, None);
    }
}
