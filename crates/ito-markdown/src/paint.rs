//! Markdown on the painter: the blocks [`crate::blocks`] reads, each with its inline marks, laid out to a width first
//! so the caller can stack what comes next under it and the preview can show its end. Colours come from `ito_theme`.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect, Stroke, StrokeKind};

use crate::{blocks, spans, Block, Span};

/// The space between blocks, and between the items of a list.
const GAP: f32 = 8.0;
const ITEM_GAP: f32 = 3.0;
/// Where an item's text starts, past its bullet or number.
const GUTTER: f32 = 22.0;

/// Text laid out and ready to paint at any place.
pub struct Md {
    parts: Vec<Part>,
    pub height: f32,
}

struct Part {
    x: f32,
    y: f32,
    text: Option<Text>,
    deco: Deco,
}

/// A galley and, for the bold runs, a second one painted a hair to the right.
struct Text {
    galley: Arc<Galley>,
    bold: Option<Arc<Galley>>,
}

enum Deco {
    None,
    /// The rule under a large heading, `w` wide.
    Underline(f32),
    Bullet,
    Number(String),
    Task(bool),
    /// The bar to the left of a quote, `h` high.
    Quote(f32),
    /// The ground of a code block, `w` by `h`.
    Code(f32, f32),
    /// A table's cells (each at its offset) and its size.
    Table(Vec<(f32, f32, Text)>, f32, f32),
    Rule(f32),
    /// A `<summary>`'s open triangle and its ground, `w` by `h`.
    Summary(f32, f32),
}

fn accent() -> Color32 {
    ito_theme::colors().blue
}

fn ground() -> Color32 {
    ito_theme::colors().side
}

/// `text` as Markdown laid out to `w`; `size` is the body's font size.
pub fn layout(p: &Painter, text: &str, w: f32, size: f32) -> Md {
    let c = ito_theme::colors();
    let mut parts = Vec::new();
    let mut y = 0.0;
    let mut last_item = false;
    for b in blocks(text) {
        let item = matches!(b, Block::Bullet(_) | Block::Numbered(..) | Block::Task(..));
        if !parts.is_empty() {
            y += if item && last_item { ITEM_GAP } else { GAP };
        }
        last_item = item;
        let body = |s: &str, w: f32| inline(p, s, w, size, c.fg, false);
        match b {
            Block::Heading(n, s) => {
                let big = [size + 7.0, size + 4.0, size + 2.0, size + 1.0][usize::from(n.clamp(1, 4)) - 1];
                let t = inline(p, &s, w, big, c.fg, true);
                let h = t.galley.size().y;
                let deco = if n <= 2 { Deco::Underline(w) } else { Deco::None };
                parts.push(Part { x: 0.0, y, text: Some(t), deco });
                y += h + if n <= 2 { 6.0 } else { 0.0 };
            }
            Block::Bullet(s) => y += item_part(&mut parts, body(&s, w - GUTTER), y, Deco::Bullet),
            Block::Numbered(n, s) => y += item_part(&mut parts, body(&s, w - GUTTER), y, Deco::Number(n)),
            Block::Task(on, s) => y += item_part(&mut parts, body(&s, w - GUTTER), y, Deco::Task(on)),
            Block::Quote(s) => {
                let t = inline(p, &s, w - 14.0, size, c.dim, false);
                let h = t.galley.size().y;
                parts.push(Part { x: 14.0, y, text: Some(t), deco: Deco::Quote(h) });
                y += h;
            }
            Block::Code(s) => {
                let g = p.layout(s, FontId::monospace(size - 1.0), c.fg, w - 20.0);
                let h = g.size().y + 16.0;
                // The ground goes under the text: push it first.
                parts.push(Part { x: 0.0, y, text: None, deco: Deco::Code(w, h) });
                parts.push(Part { x: 10.0, y: y + 8.0, text: Some(Text { galley: g, bold: None }), deco: Deco::None });
                y += h;
            }
            Block::Table(rows) => {
                let (cells, tw, th) = table(p, &rows, w, size);
                parts.push(Part { x: 0.0, y, text: None, deco: Deco::Table(cells, tw, th) });
                y += th;
            }
            Block::Rule => {
                parts.push(Part { x: 0.0, y: y + 4.0, text: None, deco: Deco::Rule(w) });
                y += 9.0;
            }
            Block::Summary(s) => {
                let s = if s.is_empty() { "Details".to_string() } else { s };
                let t = inline(p, &s, w - GUTTER - 8.0, size, c.fg, true);
                let h = t.galley.size().y + 8.0;
                parts.push(Part { x: 0.0, y, text: None, deco: Deco::Summary(w, h) });
                parts.push(Part { x: GUTTER, y: y + 4.0, text: Some(t), deco: Deco::None });
                y += h;
            }
            Block::Para(s) => {
                let t = body(&s, w);
                let h = t.galley.size().y;
                parts.push(Part { x: 0.0, y, text: Some(t), deco: Deco::None });
                y += h;
            }
        }
    }
    Md { parts, height: y }
}

fn item_part(parts: &mut Vec<Part>, t: Text, y: f32, deco: Deco) -> f32 {
    let h = t.galley.size().y;
    parts.push(Part { x: GUTTER, y, text: Some(t), deco });
    h
}

/// The cells of a table, each column as wide as its widest cell (shrunk alike when they do not fit); the head row in
/// bold.
fn table(p: &Painter, rows: &[Vec<String>], w: f32, size: f32) -> (Vec<(f32, f32, Text)>, f32, f32) {
    let c = ito_theme::colors();
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let pad = 8.0;
    let mut widths = vec![0.0f32; cols];
    for row in rows {
        for (k, cell) in row.iter().enumerate() {
            let g = p.layout_no_wrap(cell.clone(), FontId::proportional(size - 0.5), c.fg);
            widths[k] = widths[k].max(g.size().x + pad * 2.0);
        }
    }
    let total: f32 = widths.iter().sum();
    if total > w {
        widths.iter_mut().for_each(|x| *x *= w / total);
    }
    let mut cells = Vec::new();
    let mut y = 0.0;
    for (r, row) in rows.iter().enumerate() {
        let laid: Vec<Text> =
            row.iter().enumerate().map(|(k, s)| inline(p, s, (widths[k] - pad * 2.0).max(10.0), size - 0.5, c.fg, r == 0)).collect();
        let h = laid.iter().map(|t| t.galley.size().y).fold(0.0, f32::max) + 8.0;
        let mut x = 0.0;
        for (k, t) in laid.into_iter().enumerate() {
            cells.push((x + pad, y + 4.0, t));
            x += widths[k];
        }
        y += h;
    }
    (cells, widths.iter().sum(), y)
}

/// One block's inline marks as a galley (and its bold twin) to `w`.
fn inline(p: &Painter, s: &str, w: f32, size: f32, color: Color32, all_bold: bool) -> Text {
    let runs = spans(s);
    let job = |bold_only: bool| {
        let mut job = LayoutJob::default();
        job.wrap.max_width = w.max(10.0);
        for r in &runs {
            let mut f = format(r, size, color);
            if bold_only && !(r.bold || all_bold) {
                f.color = Color32::TRANSPARENT;
                f.background = Color32::TRANSPARENT;
                f.underline = Stroke::NONE;
                f.strikethrough = Stroke::NONE;
            }
            job.append(&r.text, 0.0, f);
        }
        job
    };
    let any_bold = all_bold || runs.iter().any(|r| r.bold);
    Text { galley: p.layout_job(job(false)), bold: any_bold.then(|| p.layout_job(job(true))) }
}

fn format(r: &Span, size: f32, color: Color32) -> TextFormat {
    let c = ito_theme::colors();
    let ink = if r.link.is_some() { accent() } else { color };
    let underline = if r.link.is_some() {
        Stroke::new(1.0, accent().gamma_multiply(0.6))
    } else if r.underline {
        Stroke::new(1.0, ink)
    } else {
        Stroke::NONE
    };
    TextFormat {
        font_id: if r.code { FontId::monospace(size - 1.0) } else { FontId::proportional(size) },
        color: ink,
        italics: r.italic,
        background: if r.code { ground() } else { Color32::TRANSPARENT },
        underline,
        strikethrough: if r.strike { Stroke::new(1.0, c.dim) } else { Stroke::NONE },
        ..Default::default()
    }
}

impl Md {
    /// Paint it with its top left at `(x, y)`.
    pub fn paint(&self, p: &Painter, x: f32, y: f32) {
        let c = ito_theme::colors();
        for part in &self.parts {
            let at = pos2(x + part.x, y + part.y);
            let line_mid = |t: &Option<Text>| t.as_ref().map_or(8.0, |t| t.galley.rows.first().map_or(8.0, |r| r.rect().center().y));
            match &part.deco {
                Deco::None => {}
                Deco::Underline(w) => {
                    let h = part.text.as_ref().map_or(0.0, |t| t.galley.size().y);
                    p.line_segment([pos2(at.x, at.y + h + 3.0), pos2(at.x + w, at.y + h + 3.0)], Stroke::new(1.0, c.border));
                }
                Deco::Bullet => {
                    p.circle_filled(pos2(at.x - GUTTER + 8.0, at.y + line_mid(&part.text)), 2.2, c.dim);
                }
                Deco::Number(n) => {
                    p.text(
                        pos2(at.x - 5.0, at.y + line_mid(&part.text)),
                        Align2::RIGHT_CENTER,
                        format!("{n}."),
                        FontId::proportional(13.5),
                        c.dim,
                    );
                }
                Deco::Task(on) => {
                    let b = Rect::from_center_size(pos2(at.x - GUTTER + 8.0, at.y + line_mid(&part.text)), vec2(11.0, 11.0));
                    if *on {
                        p.rect_filled(b, CornerRadius::same(3), accent());
                        p.line_segment([b.left_center() + vec2(2.5, 0.0), b.center_bottom() + vec2(-0.5, -2.5)], Stroke::new(1.5, c.bg));
                        p.line_segment([b.center_bottom() + vec2(-0.5, -2.5), b.right_top() + vec2(-2.0, 2.5)], Stroke::new(1.5, c.bg));
                    } else {
                        p.rect_stroke(b, CornerRadius::same(3), Stroke::new(1.0, c.border_strong()), StrokeKind::Inside);
                    }
                }
                Deco::Quote(h) => {
                    p.rect_filled(Rect::from_min_size(pos2(at.x - 14.0, at.y), vec2(3.0, *h)), CornerRadius::same(1), c.border_strong());
                }
                Deco::Code(w, h) => {
                    p.rect_filled(Rect::from_min_size(at, vec2(*w, *h)), CornerRadius::same(ito_theme::scale::radius::CONTROL), ground());
                }
                Deco::Table(cells, w, h) => {
                    let r = Rect::from_min_size(at, vec2(*w, *h));
                    p.rect_stroke(r, CornerRadius::same(ito_theme::scale::radius::MARK), Stroke::new(1.0, c.border), StrokeKind::Inside);
                    let mut last_y = None;
                    for (cx, cy, t) in cells {
                        if last_y.is_some_and(|ly| ly != *cy) {
                            p.line_segment([pos2(r.left(), at.y + cy - 4.0), pos2(r.right(), at.y + cy - 4.0)], Stroke::new(1.0, c.border));
                        }
                        last_y = Some(*cy);
                        paint_text(p, pos2(at.x + cx, at.y + cy), t);
                    }
                }
                Deco::Rule(w) => {
                    p.line_segment([at, pos2(at.x + w, at.y)], Stroke::new(1.0, c.border));
                }
                Deco::Summary(w, h) => {
                    p.rect_filled(Rect::from_min_size(at, vec2(*w, *h)), CornerRadius::same(ito_theme::scale::radius::MARK), ground());
                    let m = pos2(at.x + 11.0, at.y + h / 2.0);
                    let tri = vec![m + vec2(-4.0, -2.5), m + vec2(4.0, -2.5), m + vec2(0.0, 3.0)];
                    p.add(egui::Shape::convex_polygon(tri, c.dim, Stroke::NONE));
                }
            }
            if let Some(t) = &part.text {
                paint_text(p, at, t);
            }
        }
    }
}

fn paint_text(p: &Painter, at: Pos2, t: &Text) {
    p.galley(at, t.galley.clone(), Color32::PLACEHOLDER);
    if let Some(b) = &t.bold {
        p.galley(at + vec2(0.45, 0.0), b.clone(), Color32::PLACEHOLDER);
    }
}
