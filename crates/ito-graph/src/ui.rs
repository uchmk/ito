//! The graph painted in egui (the `egui` feature): the edges first, then the
//! dots over them. The rows' text, the selection and anything a row carries
//! beside its dot are the caller's.

use std::hash::Hash;
use std::ops::Range;

use egui::epaint::{CircleShape, CubicBezierShape};
use egui::{Color32, Painter, Pos2, Shape, Stroke};

use crate::{Dot, Layout, Metrics, Pt, Seg};

/// The colours and the line.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// The terminal's sixteen colours (`ito_theme::Colors::ansi`): a row's
    /// colour is an index into them.
    pub ansi: [Color32; 16],
    /// The background: inside the rings and around the dots.
    pub bg: Color32,
    /// The ring of a row stopped by a conflict.
    pub conflict: Color32,
    /// The edges' width.
    pub width: f32,
}

/// How far an open edge fades: it goes on past what is laid out.
const OPEN: f32 = 0.45;
/// How far a stub fades: its parent is not in the repository.
const STUB: f32 = 0.4;

/// Paints `rows` of `layout`, row 0's top-left corner at `origin` (above
/// the clip rectangle once scrolled), and the edges that cross them.
pub fn paint<Id: Clone + Eq + Hash>(painter: &Painter, origin: Pos2, layout: &Layout<Id>, rows: Range<u32>, m: Metrics, look: &Look) {
    let at = |p: Pt| Pos2::new(origin.x + p.x, origin.y + p.y);
    let end = rows.end.min(layout.len() as u32);
    let rows = rows.start.min(end)..end;
    let mut shapes = Vec::new();
    for e in layout.edges(rows.clone()) {
        let mut color = look.ansi[e.color as usize & 15];
        let dashed = matches!(layout.rows()[e.from as usize].dot, Dot::Wip | Dot::Conflict);
        let (dash, fade) = match e.to {
            Some(_) => (dashed.then_some((3.0, 3.0)), 0.0),
            None if layout.finished() => (Some((2.0, 3.0)), STUB),
            None => (dashed.then_some((3.0, 3.0)), OPEN),
        };
        if fade > 0.0 {
            color = color.gamma_multiply(fade);
        }
        let stroke = Stroke::new(look.width, color);
        for seg in layout.path(e, m).iter() {
            match (seg, dash) {
                (Seg::Line(a, b), None) => shapes.push(Shape::line_segment([at(*a), at(*b)], stroke)),
                (Seg::Cubic(c), None) => {
                    shapes.push(CubicBezierShape::from_points_stroke(c.map(at), false, Color32::TRANSPARENT, stroke).into());
                }
                (Seg::Line(a, b), Some((d, g))) => shapes.extend(Shape::dashed_line(&[at(*a), at(*b)], stroke, d, g)),
                (Seg::Cubic(c), Some((d, g))) => {
                    let points = CubicBezierShape::from_points_stroke(c.map(at), false, Color32::TRANSPARENT, stroke).flatten(Some(0.2));
                    shapes.extend(Shape::dashed_line(&points, stroke, d, g));
                }
            }
        }
    }
    let k = m.scale();
    for i in rows {
        let r = &layout.rows()[i as usize];
        let c = at(Pt::new(m.x(r.lane), m.y(i)));
        let color = look.ansi[r.color as usize & 15];
        match r.dot {
            Dot::Commit => shapes.push(circle(c, 4.0 * k, color, Stroke::new(1.5 * k, look.bg))),
            Dot::Merge => {
                shapes.push(circle(c, 5.0 * k, look.bg, Stroke::new(2.0 * k, color)));
                shapes.push(Shape::circle_filled(c, 2.0 * k, color));
            }
            Dot::Wip => {
                shapes.push(Shape::circle_filled(c, 4.5 * k, look.bg));
                shapes.extend(ring(c, 4.5 * k, Stroke::new(1.6 * k, color), 2.2 * k, 1.8 * k));
            }
            Dot::Conflict => {
                shapes.push(circle(c, 5.5 * k, look.bg, Stroke::new(2.0 * k, look.conflict)));
                shapes.push(Shape::circle_filled(c, 2.0 * k, look.conflict));
            }
        }
    }
    painter.extend(shapes);
}

/// A filled circle with a line around it.
fn circle(center: Pos2, radius: f32, fill: Color32, stroke: Stroke) -> Shape {
    CircleShape { center, radius, fill, stroke }.into()
}

/// A dashed circle.
fn ring(c: Pos2, r: f32, stroke: Stroke, dash: f32, gap: f32) -> Vec<Shape> {
    let n = 48;
    let points: Vec<Pos2> = (0..=n)
        .map(|k| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    Shape::dashed_line(&points, stroke, dash, gap)
}
