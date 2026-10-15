//! A commit graph laid out row by row, the way `git log --graph` and the Git
//! clients draw one: a lane for each line of history still open, the trunk
//! (main's first parents) always in the leftmost lane, an edge from each row
//! down to each of its parents, and a colour for each branch.
//!
//! The rows come in topological order (children before parents), a few at
//! a time, so a long history is laid out while it is read and drawn only
//! where it is seen. What a row is (a commit, a pull request, a session) and
//! which of its parents are shown (after folding or filtering) is the
//! caller's: the layout takes ids, parents and a name for the colour, and
//! knows nothing of Git.
//!
//! The geometry (`Metrics`, `Layout::path`) has nothing to do with egui
//! either; painting it is the `egui` feature's `ui::paint`.

mod color;
mod geom;
#[cfg(feature = "egui")]
pub mod ui;

pub use color::{Colors, ALLOWED};
pub use geom::{Metrics, Path, Pt, Seg};

use std::collections::HashMap;
use std::hash::Hash;
use std::ops::Range;

/// An edge longer than this many rows is kept apart, so the edges of the
/// rows in sight are found without looking at all of them.
const LONG: u32 = 32;

/// How a row's dot is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dot {
    #[default]
    Commit,
    /// A merge (folded ones too): a ring around a small dot.
    Merge,
    /// Not committed yet: a dashed ring, its edges dashed.
    Wip,
    /// Stopped by a conflict (a merge under way): a ring in the error
    /// colour, its edges dashed.
    Conflict,
}

/// A row as the caller hands it in.
#[derive(Clone, Copy, Debug)]
pub struct Row<'a, Id> {
    pub id: Id,
    /// The parents shown, the first parent first.
    pub parents: &'a [Id],
    /// The branch the row is on, for its colour (`None`: not known).
    pub name: Option<&'a str>,
    /// On the trunk (main's first-parent line): kept in lane 0.
    pub trunk: bool,
    pub dot: Dot,
}

/// A row as laid out.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// Its lane, 0 on the left.
    pub lane: u16,
    /// The rightmost lane in use across this row: the graph is this wide here.
    pub span: u16,
    /// An index into the terminal's sixteen colours.
    pub color: u8,
    pub dot: Dot,
    /// Its edges in `Layout::edges_of`.
    edges: Range<u32>,
}

/// An edge from a row down to one of its parents.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    /// The child's row.
    pub from: u32,
    /// The parent's row; `None` while it has not come (drawn down to the
    /// last row), or, once the layout is finished, never will (a shallow
    /// clone's edge: a short stub).
    pub to: Option<u32>,
    /// The lane it runs down in between the two rows.
    pub via: u16,
    /// To the first parent: it takes the child's colour, else the parent's.
    pub first: bool,
    /// An index into the terminal's sixteen colours.
    pub color: u8,
    /// Kept in `Layout::long`: longer than `LONG`, or still open.
    long: bool,
}

/// The rows laid out so far.
#[derive(Clone, Debug)]
pub struct Layout<Id> {
    rows: Vec<Placed>,
    edges: Vec<Edge>,
    /// What each lane is waiting for: a parent still to come.
    lanes: Vec<Option<Id>>,
    /// The colour running down each lane, when it is known.
    lane_colors: Vec<Option<u8>>,
    /// The open edges into each parent still to come.
    waiting: HashMap<Id, Vec<u32>>,
    index: HashMap<Id, u32>,
    /// The edges with `long` set, and some that have lost it since.
    long: Vec<u32>,
    /// How many of `long` still have it.
    live_long: usize,
    /// 1 while lane 0 is the trunk's.
    first_free: u16,
    widest: u16,
    finished: bool,
}

impl<Id: Clone + Eq + Hash> Default for Layout<Id> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Id: Clone + Eq + Hash> Layout<Id> {
    /// A layout whose lane 0 is kept for the trunk.
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            edges: Vec::new(),
            lanes: Vec::new(),
            lane_colors: Vec::new(),
            waiting: HashMap::new(),
            index: HashMap::new(),
            long: Vec::new(),
            live_long: 0,
            first_free: 1,
            widest: 0,
            finished: false,
        }
    }

    /// A layout with no trunk: lane 0 is anyone's.
    pub fn without_trunk() -> Self {
        Self { first_free: 0, ..Self::new() }
    }

    /// Lays out the next row, below the ones before it, and gives its index.
    /// A row's parents come after it; a parent laid out already is not drawn
    /// to, and an id seen before is not laid out twice.
    pub fn push(&mut self, row: Row<'_, Id>, colors: &mut Colors) -> u32 {
        if let Some(&i) = self.index.get(&row.id) {
            return i;
        }
        let i = self.rows.len() as u32;
        // The lanes that waited for this row are free again; it takes the
        // leftmost, unless it is the trunk's.
        let mut hit = None;
        for (j, l) in self.lanes.iter_mut().enumerate() {
            if l.as_ref() == Some(&row.id) {
                *l = None;
                self.lane_colors[j] = None;
                hit.get_or_insert(j as u16);
            }
        }
        let lane = match (row.trunk, hit) {
            (true, _) => 0,
            (false, Some(j)) => j,
            (false, None) => self.free(),
        };
        let beside = |j: Option<u16>| j.and_then(|j| self.lane_colors.get(j as usize).copied().flatten());
        let avoid: Vec<u8> = [beside(lane.checked_sub(1)), beside(lane.checked_add(1))].into_iter().flatten().collect();
        let color = colors.place(row.name, &avoid);
        for k in self.waiting.remove(&row.id).unwrap_or_default() {
            let e = &mut self.edges[k as usize];
            e.to = Some(i);
            if !e.first {
                e.color = color;
            }
            if i - e.from <= LONG {
                e.long = false;
                self.live_long -= 1;
            }
        }
        let start = self.edges.len() as u32;
        let mut first = true;
        for (k, p) in row.parents.iter().enumerate() {
            if *p == row.id || row.parents[..k].contains(p) || self.index.contains_key(p) {
                continue;
            }
            let via = if first {
                lane
            } else {
                match self.lanes.iter().position(|l| l.as_ref() == Some(p)) {
                    Some(j) => j as u16,
                    None => self.free(),
                }
            };
            let slot = via as usize;
            if self.lanes.len() <= slot {
                self.lanes.resize(slot + 1, None);
                self.lane_colors.resize(slot + 1, None);
            }
            if first {
                self.lane_colors[slot] = Some(color);
            }
            self.lanes[slot] = Some(p.clone());
            let e = self.edges.len() as u32;
            self.edges.push(Edge { from: i, to: None, via, first, color, long: true });
            self.long.push(e);
            self.live_long += 1;
            self.waiting.entry(p.clone()).or_default().push(e);
            first = false;
        }
        let span = self.lanes.iter().rposition(Option::is_some).map_or(lane, |j| (j as u16).max(lane));
        self.widest = self.widest.max(span);
        self.rows.push(Placed { lane, span, color, dot: row.dot, edges: start..self.edges.len() as u32 });
        self.index.insert(row.id, i);
        if self.long.len() > 64 && self.long.len() > 2 * self.live_long {
            self.long.retain(|&k| self.edges[k as usize].long);
        }
        i
    }

    /// The leftmost lane not in use.
    fn free(&self) -> u16 {
        let from = self.first_free as usize;
        (from..).find(|&j| self.lanes.get(j).is_none_or(Option::is_none)).unwrap_or(from) as u16
    }

    /// No more rows will come: the edges still open go nowhere.
    pub fn finish(&mut self) {
        self.finished = true;
        self.waiting.clear();
    }

    /// Whether `finish` was called.
    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[Placed] {
        &self.rows
    }

    pub fn row(&self, i: u32) -> Option<&Placed> {
        self.rows.get(i as usize)
    }

    /// The row an id was laid out on.
    pub fn find(&self, id: &Id) -> Option<u32> {
        self.index.get(id).copied()
    }

    /// The edges from row `i` down to its parents.
    pub fn edges_of(&self, i: u32) -> &[Edge] {
        self.rows.get(i as usize).map_or(&[], |r| &self.edges[r.edges.start as usize..r.edges.end as usize])
    }

    /// The edges that cross any of `rows`, once each, in no set order.
    pub fn edges(&self, rows: Range<u32>) -> impl Iterator<Item = &Edge> {
        let (lo, hi) = (rows.start, rows.end);
        let a = self.edges.partition_point(|e| e.from < lo.saturating_sub(LONG));
        let b = self.edges.partition_point(|e| e.from < hi);
        let short = self.edges[a..b].iter().filter(move |e| !e.long && e.to.is_some_and(|t| t >= lo));
        let long =
            self.long.iter().map(|&k| &self.edges[k as usize]).filter(move |e| e.long && e.from < hi && e.to.is_none_or(|t| t >= lo));
        short.chain(long)
    }

    /// How many lanes wide the graph is at its widest.
    pub fn width(&self) -> u16 {
        if self.rows.is_empty() {
            0
        } else {
            self.widest + 1
        }
    }

    /// How many lanes wide the graph is across `rows`.
    pub fn width_in(&self, rows: Range<u32>) -> u16 {
        let end = (rows.end as usize).min(self.rows.len());
        let start = (rows.start as usize).min(end);
        self.rows[start..end].iter().map(|r| r.span + 1).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests;
