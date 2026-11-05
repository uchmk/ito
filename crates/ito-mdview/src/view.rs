//! The egui half: a [`Doc`] drawn on a monospace cell grid in a scroll area.
//!
//! The painter takes a finished `Doc` and never parses; lay it out off the UI thread with [`crate::render`] at the
//! `cols` [`show`] reports. Only the visible lines are drawn.

use std::ops::Range;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use ito_theme::scale::{self, Text};

use crate::{cells, Doc, DocLine, LineKind};

/// Scroll and width of one pane. Keep it with the pane; it is cheap.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// Pixels scrolled from the top.
    pub offset: f32,
    /// Columns that fit the pane's width at the last frame: render the `Doc` at this many.
    pub cols: u16,
    jump: Option<usize>,
}

impl State {
    /// Scroll so the line showing original `byte` is at the top, at the next [`show`]. The jump is not reported as
    /// the person's scrolling ([`Output::scrolled`]), so two panes following each other do not feed back.
    pub fn scroll_to_byte(&mut self, byte: usize) {
        self.jump = Some(byte);
    }

    /// The original byte of the line at the top of the pane (`line_height` is [`Output::line_height`]).
    pub fn top_byte(&self, doc: &Doc, line_height: f32) -> usize {
        doc.byte_at_line(top_line(self.offset, line_height))
    }
}

/// How to draw.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options<'a> {
    /// Letter size; `None` is `editor.text` at the text zoom.
    pub size: Option<f32>,
    /// Draw these matches (`Doc::marks`) behind the text.
    pub matcher: Option<&'a ito_match::Matcher>,
    /// The caret's place in the original text: its lines get a faint band, and the first match there is the current one.
    pub caret_byte: Option<usize>,
}

/// What happened this frame.
#[derive(Clone, Debug, Default)]
pub struct Output {
    /// A click: the original byte under the pointer (close, not exact -- see [`Doc::byte_at`]; snap it).
    pub clicked_byte: Option<usize>,
    /// The original byte of the top line.
    pub top_byte: usize,
    /// Columns that fit; differs from `doc.body_cols` when the pane was resized and wants a new layout.
    pub cols: u16,
    /// The person scrolled (wheel, bar, keys), as opposed to [`State::scroll_to_byte`] or no change.
    pub scrolled: bool,
    /// Pixel height of one line.
    pub line_height: f32,
}

fn top_line(offset: f32, lh: f32) -> usize {
    (offset.max(0.0) / lh.max(1.0)).floor() as usize
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn line_text(l: &DocLine) -> String {
    l.spans.iter().map(|s| s.text.as_str()).collect()
}

/// Draw `doc` filling the space `ui` has.
pub fn show(ui: &mut egui::Ui, doc: &Doc, state: &mut State, opt: &Options) -> Output {
    let c = ito_theme::colors();
    let size = opt.size.unwrap_or_else(|| Text::EditorText.at(ui.ctx()));
    let font = FontId::monospace(size);
    let cell = ui.painter().layout_no_wrap("M".into(), font.clone(), Color32::WHITE).size().x.max(1.0);
    let lh = (size * 1.5).round();
    let pad = scale::space::MD;
    let width = ui.available_width();
    let cols = ((width - 2.0 * pad - 14.0) / cell).floor().max(1.0) as u16;
    state.cols = cols;
    let total = doc.lines.len() as f32 * lh;

    let mut jumped = false;
    let mut area = egui::ScrollArea::vertical().auto_shrink(false);
    if let Some(byte) = state.jump.take() {
        area = area.vertical_scroll_offset(doc.line_at_byte(byte) as f32 * lh);
        jumped = true;
    }
    let mut clicked = None;
    let caret_lines: Range<usize> = match opt.caret_byte {
        Some(b) => {
            let hit: Vec<usize> =
                doc.lines.iter().enumerate().filter(|(_, l)| l.bytes.start <= b && b < l.bytes.end).map(|(i, _)| i).collect();
            match (hit.first(), hit.last()) {
                (Some(&a), Some(&z)) => a..z + 1,
                _ => {
                    let i = doc.line_at_byte(b);
                    i..i + 1
                }
            }
        }
        None => 0..0,
    };

    let out = area.show_viewport(ui, |ui, view| {
        ui.set_min_height(total + pad);
        ui.set_min_width(width - 14.0);
        let origin = ui.min_rect().min;
        let first = top_line(view.min.y, lh).min(doc.lines.len());
        let last = (((view.max.y / lh).ceil() as usize) + 1).min(doc.lines.len());
        let marks = opt.matcher.map(|m| doc.marks(m, first, last));
        let mut current_done = false;
        let painter = ui.painter().clone();
        for i in first..last {
            let l = &doc.lines[i];
            let top = origin.y + i as f32 * lh;
            let x0 = origin.x + pad;
            let row = Rect::from_min_size(Pos2::new(origin.x, top), Vec2::new(width, lh));
            if caret_lines.contains(&i) {
                painter.rect_filled(row, 0.0, c.current_line());
            }
            let band =
                Rect::from_min_max(Pos2::new(x0 + f32::from(l.indent) * cell, top), Pos2::new(x0 + f32::from(cols) * cell, top + lh));
            match l.kind {
                LineKind::Code => {
                    painter.rect_filled(band, 0.0, c.raised());
                }
                LineKind::Placeholder => {
                    painter.rect_filled(band, f32::from(scale::radius::MARK), c.hover());
                }
                LineKind::Rule => {
                    let y = top + lh / 2.0;
                    painter.line_segment([Pos2::new(band.min.x, y), Pos2::new(band.max.x, y)], Stroke::new(1.0, c.border));
                }
                LineKind::Heading(n) if n <= 2 => {
                    painter
                        .line_segment([Pos2::new(x0, top + lh - 1.0), Pos2::new(band.max.x, top + lh - 1.0)], Stroke::new(1.0, c.border));
                }
                _ => {}
            }
            // Matches behind the text.
            if let Some(ms) = marks.as_ref().and_then(|m| m.get(i - first)) {
                let whole = line_text(l);
                for r in ms {
                    let (Some(a), Some(b)) = (whole.get(..r.start), whole.get(..r.end)) else { continue };
                    let rect = Rect::from_min_max(
                        Pos2::new(x0 + cells(a) as f32 * cell, top + 1.0),
                        Pos2::new(x0 + cells(b) as f32 * cell, top + lh - 1.0),
                    );
                    let is_current = !current_done && opt.caret_byte.is_some() && caret_lines.contains(&i);
                    current_done |= is_current;
                    painter.rect_filled(rect, 2.0, if is_current { c.match_current_bg() } else { c.match_bg() });
                }
            }
            // Spans on the grid.
            let mut col = 0usize;
            for s in &l.spans {
                let w = cells(&s.text);
                let x = x0 + col as f32 * cell;
                let heading = matches!(l.kind, LineKind::Heading(_));
                let color = s.color.map(rgb).unwrap_or(if heading { c.strong() } else { c.fg });
                if s.code {
                    painter.rect_filled(Rect::from_min_size(Pos2::new(x, top + 2.0), Vec2::new(w as f32 * cell, lh - 4.0)), 3.0, c.hover());
                }
                let at = Pos2::new(x, top + (lh - size) / 2.0 - 1.0);
                painter.text(at, Align2::LEFT_TOP, &s.text, font.clone(), color);
                if s.bold || heading {
                    painter.text(at + Vec2::new(0.5, 0.0), Align2::LEFT_TOP, &s.text, font.clone(), color);
                }
                let right = x + w as f32 * cell;
                if s.strike {
                    painter.line_segment([Pos2::new(x, top + lh / 2.0), Pos2::new(right, top + lh / 2.0)], Stroke::new(1.0, color));
                }
                if s.underline {
                    painter.line_segment([Pos2::new(x, top + lh - 3.0), Pos2::new(right, top + lh - 3.0)], Stroke::new(1.0, color));
                }
                col += w;
            }
        }
        let area = Rect::from_min_size(origin, Vec2::new(width, total.max(view.height())));
        let resp = ui.interact(area, ui.id().with("mdview-click"), Sense::click());
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let line = ((p.y - origin.y) / lh).floor().max(0.0) as usize;
                let col = ((p.x - origin.x - pad) / cell).floor().max(0.0) as usize;
                clicked = Some(doc.byte_at(line.min(doc.lines.len().saturating_sub(1)), col));
            }
        }
    });

    let scrolled = !jumped && (out.state.offset.y - state.offset).abs() > 0.5;
    state.offset = out.state.offset.y;
    let clicked = if doc.lines.is_empty() { None } else { clicked };
    Output { clicked_byte: clicked, top_byte: state.top_byte(doc, lh), cols, scrolled, line_height: lh }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{render, Palette};

    fn run(doc: &Doc, state: &mut State, opt: &Options) -> Output {
        let ctx = egui::Context::default();
        let mut out = Output::default();
        ctx.run_ui(egui::RawInput::default(), |ui| out = show(ui, doc, state, opt)).drop_without_applying_deltas();
        out
    }

    const SRC: &str = "# Title\n\nA paragraph with `code` and **bold** text.\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n\n---\n";

    #[test]
    fn draws_every_kind_and_reports_the_width() {
        let doc = render(SRC, 60, &Palette::from_theme()).0;
        let m = ito_match::Matcher::new("para").unwrap();
        let mut st = State::default();
        let out = run(&doc, &mut st, &Options { matcher: Some(&m), caret_byte: Some(12), ..Default::default() });
        assert!(out.cols > 10 && out.line_height > 10.0);
        assert!(!out.scrolled && out.clicked_byte.is_none());
        assert_eq!(out.top_byte, doc.byte_at_line(0));
    }

    #[test]
    fn a_jump_is_not_the_persons_scrolling() {
        let long = (0..200).map(|i| format!("line {i}\n\n")).collect::<String>();
        let doc = render(&long, 60, &Palette::from_theme()).0;
        let mut st = State::default();
        run(&doc, &mut st, &Options::default());
        let want = long.find("line 100").unwrap();
        st.scroll_to_byte(want);
        let first = run(&doc, &mut st, &Options::default());
        assert!(!first.scrolled, "following the other pane is not scrolling");
        let out = run(&doc, &mut st, &Options::default());
        assert!(!out.scrolled);
    }

    #[test]
    fn an_empty_doc_draws() {
        let out = run(&Doc::default(), &mut State::default(), &Options::default());
        assert_eq!(out.top_byte, 0);
    }
}
