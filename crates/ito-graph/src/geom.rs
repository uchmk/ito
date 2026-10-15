//! Where the rows, the dots and the edges go, in the caller's unit (egui's
//! points), with row 0's top-left corner at the origin.

use std::ops::Deref;

use crate::{Edge, Layout};

/// A point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A piece of an edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line(Pt, Pt),
    /// A cubic Bézier curve: the start, two control points, the end.
    Cubic([Pt; 4]),
}

/// How big a row and a lane are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// A row's height.
    pub row: f32,
    /// A lane's width.
    pub lane: f32,
    /// The room left of lane 0 and right of the last.
    pub pad: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self { row: 26.0, lane: 14.0, pad: 8.0 }
    }
}

impl Metrics {
    /// The middle of a lane.
    pub fn x(&self, lane: u16) -> f32 {
        self.pad + lane as f32 * self.lane + self.lane / 2.0
    }

    /// The middle of a row.
    pub fn y(&self, row: u32) -> f32 {
        row as f32 * self.row + self.row / 2.0
    }

    /// How wide a graph of `lanes` lanes is, its padding included.
    pub fn width(&self, lanes: u16) -> f32 {
        self.pad * 2.0 + lanes as f32 * self.lane
    }

    /// How big a dot is next to `Metrics::default`'s.
    pub fn scale(&self) -> f32 {
        self.lane / 14.0
    }
}

/// An edge's pieces, three at most.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Path {
    segs: [Seg; 3],
    n: usize,
}

impl Path {
    fn new() -> Self {
        let none = Seg::Line(Pt::default(), Pt::default());
        Self { segs: [none; 3], n: 0 }
    }

    fn add(&mut self, s: Seg) {
        self.segs[self.n] = s;
        self.n += 1;
    }
}

impl Deref for Path {
    type Target = [Seg];

    fn deref(&self) -> &[Seg] {
        &self.segs[..self.n]
    }
}

impl<Id> Layout<Id> {
    /// The way an edge goes: straight down its lane when it can; otherwise
    /// it bends into its lane in the first row below the child and out of it
    /// in the last row above the parent. An edge whose parent has not come
    /// runs down to the middle of the row after the last; one whose parent
    /// never will is a short stub.
    pub fn path(&self, e: &Edge, m: Metrics) -> Path {
        let mut p = Path::new();
        let child = &self.rows[e.from as usize];
        let (x, mut y, xv, r) = (m.x(child.lane), m.y(e.from), m.x(e.via), m.row);
        let bend = |x0: f32, y0: f32, x1: f32, y1: f32| {
            Seg::Cubic([Pt::new(x0, y0), Pt::new(x0, y0 + r * 0.6), Pt::new(x1, y1 - r * 0.6), Pt::new(x1, y1)])
        };
        let Some(to) = e.to else {
            if self.finished {
                p.add(Seg::Line(Pt::new(x, y), Pt::new(x, y + r * 0.9)));
                return p;
            }
            if e.via != child.lane {
                p.add(bend(x, y, xv, y + r));
                y += r;
            }
            p.add(Seg::Line(Pt::new(xv, y), Pt::new(xv, m.y(self.rows.len() as u32))));
            return p;
        };
        let parent = self.rows[to as usize].lane;
        let (xp, yp) = (m.x(parent), m.y(to));
        if to - e.from == 1 {
            p.add(if child.lane == parent { Seg::Line(Pt::new(x, y), Pt::new(xp, yp)) } else { bend(x, y, xp, yp) });
            return p;
        }
        if e.via != child.lane {
            p.add(bend(x, y, xv, y + r));
            y += r;
        }
        if e.via != parent {
            p.add(Seg::Line(Pt::new(xv, y), Pt::new(xv, yp - r)));
            p.add(bend(xv, yp - r, xp, yp));
        } else {
            p.add(Seg::Line(Pt::new(xv, y), Pt::new(xv, yp)));
        }
        p
    }
}
