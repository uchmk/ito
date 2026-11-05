//! The strip, drawn (feature `egui`). Deliberately not text: at two pixels a line a glyph is a smudge, and laying
//! out ten thousand of them would cost the frame. Each band is one rectangle spanning the widest line in it.

use egui::text::LayoutJob;
use egui::{pos2, vec2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, TextFormat, Ui};

use crate::{
    at_line, band_pixels, hover_card, hover_window, indent_chars, ink, line_at_y, map_height, per_band, runs, scroll_to, Area, Row,
};
use crate::{HOVER_CARD_SHARE, HOVER_MAX_CHARS};

/// How the pane lays its text out: the hover card is drawn on the same grid.
pub struct Look {
    /// How the bands are drawn.
    pub style: Style,
    /// Height of one row of the pane's text, in points.
    pub row_h: f32,
    pub font: FontId,
}

/// How a line is drawn in the strip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// One rectangle per band, spanning the widest line in it. Cheap for any file; needs no line text.
    #[default]
    Bands,
    /// Each word its own coloured block, gaps blank (like VS Code's minimap). Needs `line(i)` for every line that fits
    /// one band or more; a file with more lines than the strip has bands falls back to [`Style::Bands`].
    Runs,
    /// Like `Runs`, but one cell per character with its alpha following the ink (letters dense, punctuation light).
    Glyphs,
}

/// What the strip did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// The top line the pane should scroll to, when the strip was clicked or dragged.
    pub scroll_to: Option<usize>,
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn area(r: Rect) -> Area {
    Area::new(r.left(), r.top(), r.right(), r.bottom())
}

/// Draw the strip in `rect`. `offset` is the first line on screen and `on_screen` how many are. `body` is the pane's
/// text area (the hover card stays inside it); `line(i)` gives the `(text, color)` pieces of line `i` for the card.
/// Returns the top line to scroll to when the strip was pointed at.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &Ui,
    rect: Rect,
    body: Rect,
    rows: &[Row],
    offset: usize,
    on_screen: usize,
    look: &Look,
    line: &dyn Fn(usize) -> Vec<(String, Option<[u8; 3]>)>,
) -> Shown {
    let c = ito_theme::colors();
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), c.bg);

    let len = rows.len();
    // A short file keeps its natural height at the top; the rest of the strip stays empty.
    let rect = Rect::from_min_size(rect.min, vec2(rect.width(), map_height(len, rect.height())));
    let per = per_band(len, rect.height());
    let at = |l: usize| rect.top() + at_line(l, len) * rect.height();
    let unit = rect.width() / 80.0;
    let ppp = ui.ctx().pixels_per_point();
    let band_h = (at_line(per, len) * rect.height()).max(1.0);
    let detailed = look.style != Style::Bands && per == 1;
    for (b, band) in rows.chunks(per).enumerate() {
        if detailed {
            let y = at(b);
            if y + band_h > rect.bottom() + 0.5 {
                break;
            }
            let (top, bottom) = band_pixels(y, at(b + 1).min(rect.bottom()), ppp);
            let pieces = line(b);
            let max_cols = 80usize;
            if look.style == Style::Glyphs {
                let mut col = 0usize;
                for (text, color) in &pieces {
                    let base = color.map_or(c.fg, |[r, g, b]| Color32::from_rgb(r, g, b));
                    for ch in text.chars() {
                        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                        if col >= max_cols {
                            break;
                        }
                        let a = ink(ch);
                        if a > 0.0 && w > 0 {
                            let x0 = rect.left() + col as f32 * unit;
                            let x1 = (x0 + w as f32 * unit).min(rect.right());
                            painter.rect_filled(
                                Rect::from_min_max(pos2(x0, top), pos2(x1, bottom)),
                                CornerRadius::ZERO,
                                base.gamma_multiply(0.75 * a),
                            );
                        }
                        col += w;
                    }
                }
            } else {
                for r in runs(pieces.iter().map(|(t, k)| (t.as_str(), *k)), max_cols) {
                    let x0 = (rect.left() + r.start as f32 * unit).min(rect.right());
                    let x1 = (x0 + r.len as f32 * unit).min(rect.right());
                    let base = r.color.map_or(c.fg, |[r, g, b]| Color32::from_rgb(r, g, b));
                    painter.rect_filled(
                        Rect::from_min_max(pos2(x0, top), pos2(x1.max(x0 + 1.0), bottom)),
                        CornerRadius::ZERO,
                        base.gamma_multiply(0.7),
                    );
                }
            }
            continue;
        }
        let Some(lead) = band.iter().filter(|r| r.len > 0).max_by_key(|r| r.len) else { continue };
        let indent = band.iter().filter(|r| r.len > 0).map(|r| r.indent).min().unwrap_or(0);
        let x0 = (rect.left() + indent as f32 * unit).min(rect.right());
        let x1 = (x0 + lead.len as f32 * unit).min(rect.right());
        let y = at(b * per);
        if y + band_h > rect.bottom() + 0.5 {
            break;
        }
        let (top, bottom) = band_pixels(y, at((b + 1) * per).min(rect.bottom()), ppp);
        let filled = band.iter().filter(|r| r.len > 0).count() as f32 / band.len() as f32;
        let color = lead.color.map_or(c.fg, |[r, g, b]| Color32::from_rgb(r, g, b));
        painter.rect_filled(
            Rect::from_min_max(pos2(x0, top), pos2(x1.max(x0 + 1.0), bottom)),
            CornerRadius::ZERO,
            color.gamma_multiply(0.7 * filled.max(0.35)),
        );
    }

    let view = Rect::from_x_y_ranges(rect.x_range(), at(offset)..=at(offset + on_screen));
    painter.rect_filled(view, CornerRadius::same(2), mix(c.panel, c.fg, 0.12).gamma_multiply(0.4));
    painter.rect_stroke(view, CornerRadius::same(2), Stroke::new(1.0, c.border), StrokeKind::Inside);

    let id = ui.id().with("ito-minimap");
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    hover(ui, rect, body, len, look, line, resp.dragged());
    let mut shown = Shown::default();
    if resp.clicked() || resp.dragged() {
        if let Some(p) = resp.interact_pointer_pos() {
            shown.scroll_to = Some(scroll_to(line_at_y(p.y, rect.top(), rect.height(), len), on_screen));
        }
    }
    shown
}

/// The lines under the pointer, painted beside the strip after the tooltip delay (also while dragging).
fn hover(
    ui: &Ui,
    strip: Rect,
    body: Rect,
    len: usize,
    look: &Look,
    line: &dyn Fn(usize) -> Vec<(String, Option<[u8; 3]>)>,
    dragging: bool,
) {
    let id = ui.id().with("ito-minimap-hover");
    let over = ui.rect_contains_pointer(strip);
    if !over && !dragging {
        ui.data_mut(|d| d.remove_temp::<f64>(id));
        return;
    }
    let Some(p) = ui.ctx().pointer_latest_pos() else { return };
    let now = ui.input(|i| i.time);
    let since = ui.data_mut(|d| *d.get_temp_mut_or(id, now));
    let delay = f64::from(ui.style().interaction.tooltip_delay);
    if now - since < delay {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(delay - (now - since)));
        return;
    }
    let k = line_at_y(p.y, strip.top(), strip.height(), len);
    if k >= len {
        return;
    }
    let c = ito_theme::colors();
    let painter = ui.painter();
    let row_h = look.row_h;
    let room = |space: f32| ((space - 2.0 - row_h / 2.0) / row_h).floor().max(0.0) as usize;
    let fit = ((body.height() - 4.0) / row_h).floor() as usize;
    let window = hover_window(k, len, fit, room(p.y - body.top()), room(body.bottom() - p.y));
    let lines: Vec<_> = window.clone().map(|i| (i, line(i))).collect();
    let cut = lines.iter().filter_map(|(_, l)| indent_chars(l.iter().map(|(t, _)| t.as_str()))).min().unwrap_or(0);
    let digits = len.to_string().len().max(2);
    let max_width = (body.width() * HOVER_CARD_SHARE - 12.0).max(40.0);
    let rows: Vec<_> = lines
        .iter()
        .map(|(i, l)| painter.layout_job(card_line(&format!("{:>digits$} ", i + 1), l, cut, *i == k, max_width, look)))
        .collect();
    let w = rows.iter().map(|g| g.size().x).fold(0.0, f32::max) + 12.0;
    let pointed = (k - window.start) as f32 * row_h + 2.0;
    let card = hover_card(p.y, area(body), area(strip), w, rows.len() as f32 * row_h + 4.0, pointed + row_h / 2.0);
    let card = Rect::from_min_max(pos2(card.left, card.top), pos2(card.right, card.bottom));
    painter.rect_filled(card, CornerRadius::same(3), mix(c.bg, c.fg, 0.06));
    let mark = Rect::from_min_size(card.left_top() + vec2(1.0, pointed), vec2(card.width() - 2.0, row_h));
    painter.rect_filled(mark, CornerRadius::ZERO, mix(c.panel, c.fg, 0.12));
    painter.rect_stroke(card, CornerRadius::same(3), Stroke::new(1.0, c.border), StrokeKind::Inside);
    for (i, g) in rows.into_iter().enumerate() {
        painter.galley(card.left_top() + vec2(6.0, 2.0 + i as f32 * row_h), g, c.fg);
    }
}

/// One row of the card: the number, then the line with `cut` characters of indent taken off (the band already shows
/// the indent by where it starts). The line pointed at is underlined.
fn card_line(number: &str, pieces: &[(String, Option<[u8; 3]>)], cut: usize, pointed: bool, max_width: f32, look: &Look) -> LayoutJob {
    let c = ito_theme::colors();
    let fmt = |color: Color32, underline: bool| TextFormat {
        font_id: look.font.clone(),
        color,
        underline: if underline { Stroke::new(1.0, color) } else { Stroke::NONE },
        ..Default::default()
    };
    let mut job = LayoutJob::default();
    job.append(number, 0.0, fmt(if pointed { c.fg } else { c.dim }, false));
    let mut budget = HOVER_MAX_CHARS;
    let mut skip = cut;
    let mut leading = true;
    if indent_chars(pieces.iter().map(|(t, _)| t.as_str())).is_some() {
        for (text, color) in pieces {
            if budget == 0 {
                break;
            }
            let mut text = text.as_str();
            if skip > 0 {
                let at = text.char_indices().nth(skip).map_or(text.len(), |(i, _)| i);
                skip -= text[..at].chars().count();
                text = &text[at..];
            }
            if leading {
                let rest = text.trim_start();
                let pad: String = text[..text.len() - rest.len()].chars().take(budget).collect();
                budget -= pad.chars().count();
                job.append(&pad, 0.0, fmt(c.fg, false));
                if rest.is_empty() {
                    continue;
                }
                leading = false;
                text = rest;
            }
            let text: String = text.chars().take(budget).collect();
            budget -= text.chars().count();
            let col = color.map_or(c.fg, |[r, g, b]| Color32::from_rgb(r, g, b));
            job.append(&text, 0.0, fmt(col, pointed));
        }
    }
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_draws_in_a_headless_frame() {
        let rows: Vec<Row> = (0..400).map(|i| Row { indent: (i % 8) as u16, len: 40, color: None }).collect();
        let look = Look { style: Style::Bands, row_h: 16.0, font: FontId::monospace(13.0) };
        let ctx = egui::Context::default();
        let mut out = Shown { scroll_to: Some(1) };
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let body = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 300.0));
            let rect = Rect::from_min_size(pos2(606.0, 0.0), vec2(56.0, 300.0));
            out = show(ui, rect, body, &rows, 10, 20, &look, &|i| vec![(format!("  line {i}"), None)]);
        })
        .drop_without_applying_deltas();
        assert_eq!(out, Shown { scroll_to: None });
    }

    #[test]
    fn runs_and_glyphs_draw_in_a_headless_frame() {
        let rows: Vec<Row> = (0..40).map(|_| Row { indent: 2, len: 20, color: None }).collect();
        for style in [Style::Runs, Style::Glyphs] {
            let look = Look { style, row_h: 16.0, font: FontId::monospace(13.0) };
            let ctx = egui::Context::default();
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let body = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 300.0));
                let rect = Rect::from_min_size(pos2(606.0, 0.0), vec2(56.0, 300.0));
                show(ui, rect, body, &rows, 0, 20, &look, &|i| {
                    vec![("  let".into(), None), (format!(" x{i} = 日本;"), Some([200, 80, 80]))]
                });
            })
            .drop_without_applying_deltas();
        }
    }
}
