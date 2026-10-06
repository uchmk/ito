//! Splitting a rectangle into panes, the way tmux and WezTerm do: a tree whose
//! leaves are the panes and whose inner nodes split their area in two, side
//! by side or one above the other, at a ratio a divider can be dragged to.
//!
//! The tree knows nothing of what a leaf holds (`T` is a session id in
//! tsumugi) and nothing of how it is drawn: it hands out rectangles, finds the
//! pane beside another, and answers where the dividers are. filer can take it
//! as it is (docs/v1-scope.md, 3).

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A rectangle in whatever unit the caller draws in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// Which way a split divides its area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Dir {
    /// Side by side: the second pane to the right of the first.
    Right,
    /// One above the other: the second pane below the first.
    Down,
}

/// A direction to move the focus in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
    Up,
    Down,
}

/// Where a split's share stops: no pane is squeezed to nothing by a drag.
pub const MIN_RATIO: f32 = 0.05;

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Node<T> {
    Leaf(T),
    Split {
        dir: Dir,
        /// The first pane's share of the area, between `MIN_RATIO` and `1 - MIN_RATIO`.
        ratio: f32,
        first: Box<Node<T>>,
        second: Box<Node<T>>,
    },
}

/// A divider between the two halves of a split, for drawing it and dragging.
#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    /// The split it belongs to: from the root, `false` for the first child
    /// and `true` for the second (see [`Node::set_ratio`]).
    pub path: Vec<bool>,
    pub dir: Dir,
    /// The gap between the two panes.
    pub gap: Rect,
    /// The area the split divides, which a drag's position is measured in.
    pub area: Rect,
}

impl<T> Node<T> {
    /// The same shape with each pane's `T` turned into a `U` (a session
    /// restored after a restart gets a new id).
    pub fn map<U>(self, f: &mut impl FnMut(T) -> U) -> Node<U> {
        match self {
            Node::Leaf(t) => Node::Leaf(f(t)),
            Node::Split { dir, ratio, first, second } => {
                let first = Box::new(first.map(f));
                Node::Split { dir, ratio, first, second: Box::new(second.map(f)) }
            }
        }
    }
}

impl<T: Clone + PartialEq> Node<T> {
    /// The panes, first to last (left to right, top to bottom).
    pub fn leaves(&self) -> Vec<T> {
        let mut out = Vec::new();
        self.each_leaf(&mut |t| out.push(t.clone()));
        out
    }

    fn each_leaf(&self, f: &mut impl FnMut(&T)) {
        match self {
            Node::Leaf(t) => f(t),
            Node::Split { first, second, .. } => {
                first.each_leaf(f);
                second.each_leaf(f);
            }
        }
    }

    pub fn contains(&self, t: &T) -> bool {
        match self {
            Node::Leaf(x) => x == t,
            Node::Split { first, second, .. } => first.contains(t) || second.contains(t),
        }
    }

    /// Split the pane `at` in two, `new` going right of it or below it, half
    /// each. False when there is no such pane.
    pub fn split(&mut self, at: &T, dir: Dir, new: T) -> bool {
        match self {
            Node::Leaf(x) if x == at => {
                let old = Node::Leaf(x.clone());
                *self = Node::Split { dir, ratio: 0.5, first: Box::new(old), second: Box::new(Node::Leaf(new)) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => first.split(at, dir, new.clone()) || second.split(at, dir, new),
        }
    }

    /// Split the pane `at` in two with `new` on the side `toward` of it,
    /// half each. False when there is no such pane.
    pub fn split_toward(&mut self, at: &T, toward: Toward, new: T) -> bool {
        match self {
            Node::Leaf(x) if x == at => {
                let (old, new) = (Box::new(Node::Leaf(x.clone())), Box::new(Node::Leaf(new)));
                let (dir, first, second) = match toward {
                    Toward::Left => (Dir::Right, new, old),
                    Toward::Right => (Dir::Right, old, new),
                    Toward::Up => (Dir::Down, new, old),
                    Toward::Down => (Dir::Down, old, new),
                };
                *self = Node::Split { dir, ratio: 0.5, first, second };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => first.split_toward(at, toward, new.clone()) || second.split_toward(at, toward, new),
        }
    }

    /// Panes `a` and `b` trade places; the shape stays.
    pub fn swap(&mut self, a: &T, b: &T) {
        match self {
            Node::Leaf(x) if x == a => *x = b.clone(),
            Node::Leaf(x) if x == b => *x = a.clone(),
            Node::Leaf(_) => {}
            Node::Split { first, second, .. } => {
                first.swap(a, b);
                second.swap(a, b);
            }
        }
    }

    /// The tree with pane `moving` taken out and put on the side `toward`
    /// of pane `target`, the two sharing what was `target`'s room. The tree
    /// as it was when either is missing or they are the same.
    pub fn moved(self, moving: &T, target: &T, toward: Toward) -> Node<T> {
        if moving == target || !self.contains(moving) || !self.contains(target) {
            return self;
        }
        let Some(mut rest) = self.clone().remove(moving) else { return self };
        rest.split_toward(target, toward, moving.clone());
        rest
    }

    /// The tree with only the panes `keep` says yes to; a split left with one
    /// side gives way to that side. `None` when no pane is left.
    pub fn retain(self, keep: &impl Fn(&T) -> bool) -> Option<Node<T>> {
        match self {
            Node::Leaf(t) => keep(&t).then_some(Node::Leaf(t)),
            Node::Split { dir, ratio, first, second } => match (first.retain(keep), second.retain(keep)) {
                (Some(a), Some(b)) => Some(Node::Split { dir, ratio, first: Box::new(a), second: Box::new(b) }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }

    /// The tree without pane `t`.
    pub fn remove(self, t: &T) -> Option<Node<T>> {
        self.retain(&|x| x != t)
    }

    /// Each pane's rectangle in `area`, with `gap` left between neighbours
    /// for the divider.
    pub fn layout(&self, area: Rect, gap: f32) -> Vec<(T, Rect)> {
        let mut out = Vec::new();
        self.walk(area, gap, &mut Vec::new(), &mut |n, r, _| {
            if let Node::Leaf(t) = n {
                out.push((t.clone(), r));
            }
        });
        out
    }

    /// The dividers in `area`, outermost first.
    pub fn dividers(&self, area: Rect, gap: f32) -> Vec<Divider> {
        let mut out = Vec::new();
        self.walk(area, gap, &mut Vec::new(), &mut |n, r, path| {
            if let Node::Split { dir, ratio, .. } = n {
                let (a, _) = halves(r, *dir, *ratio, gap);
                let g = match dir {
                    Dir::Right => Rect::new(a.right(), r.y, gap, r.h),
                    Dir::Down => Rect::new(r.x, a.bottom(), r.w, gap),
                };
                out.push(Divider { path: path.to_vec(), dir: *dir, gap: g, area: r });
            }
        });
        out
    }

    fn walk(&self, area: Rect, gap: f32, path: &mut Vec<bool>, f: &mut impl FnMut(&Node<T>, Rect, &[bool])) {
        f(self, area, path);
        if let Node::Split { dir, ratio, first, second } = self {
            let (a, b) = halves(area, *dir, *ratio, gap);
            path.push(false);
            first.walk(a, gap, path, f);
            path.pop();
            path.push(true);
            second.walk(b, gap, path, f);
            path.pop();
        }
    }

    /// Move the divider of the split at `path` (see [`Divider::path`]).
    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) {
        match (self, path.split_first()) {
            (Node::Split { ratio: r, .. }, None) => *r = ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO),
            (Node::Split { first, second, .. }, Some((side, rest))) => {
                if *side { second } else { first }.set_ratio(rest, ratio)
            }
            (Node::Leaf(_), _) => {}
        }
    }

    /// The pane next to `from` in direction `toward`: of the panes on that
    /// side whose edge it shares a stretch of, the nearest, and among those
    /// the one most in line with it.
    pub fn neighbor(&self, from: &T, toward: Toward, area: Rect) -> Option<T> {
        let rects = self.layout(area, 0.0);
        let here = rects.iter().find(|(t, _)| t == from)?.1;
        let eps = 0.5;
        rects
            .iter()
            .filter(|(t, _)| t != from)
            .filter_map(|(t, r)| {
                let (beyond, overlap, distance, offset) = match toward {
                    Toward::Left => (r.right() <= here.x + eps, span(here.y, here.bottom(), r.y, r.bottom()), here.x - r.right(), (r.y - here.y).abs()),
                    Toward::Right => (r.x >= here.right() - eps, span(here.y, here.bottom(), r.y, r.bottom()), r.x - here.right(), (r.y - here.y).abs()),
                    Toward::Up => (r.bottom() <= here.y + eps, span(here.x, here.right(), r.x, r.right()), here.y - r.bottom(), (r.x - here.x).abs()),
                    Toward::Down => (r.y >= here.bottom() - eps, span(here.x, here.right(), r.x, r.right()), r.y - here.bottom(), (r.x - here.x).abs()),
                };
                (beyond && overlap > 0.0).then_some((t, distance, offset))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
            .map(|(t, _, _)| t.clone())
    }
}

/// How much `[a0, a1)` and `[b0, b1)` overlap.
fn span(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// The two halves of `area` for a split, `gap` apart.
fn halves(area: Rect, dir: Dir, ratio: f32, gap: f32) -> (Rect, Rect) {
    match dir {
        Dir::Right => {
            let room = (area.w - gap).max(0.0);
            let a = (room * ratio).round();
            (Rect::new(area.x, area.y, a, area.h), Rect::new(area.x + a + gap, area.y, room - a, area.h))
        }
        Dir::Down => {
            let room = (area.h - gap).max(0.0);
            let a = (room * ratio).round();
            (Rect::new(area.x, area.y, area.w, a), Rect::new(area.x, area.y + a + gap, area.w, room - a))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect { x: 0.0, y: 0.0, w: 1000.0, h: 600.0 };

    /// 1 | 2 over 3: one pane on the left, two stacked on the right.
    fn three() -> Node<u32> {
        let mut n = Node::Leaf(1);
        assert!(n.split(&1, Dir::Right, 2));
        assert!(n.split(&2, Dir::Down, 3));
        n
    }

    #[test]
    fn panes_share_the_area_with_a_gap_between() {
        let rects = three().layout(AREA, 4.0);
        assert_eq!(rects[0], (1, Rect::new(0.0, 0.0, 498.0, 600.0)));
        assert_eq!(rects[1], (2, Rect::new(502.0, 0.0, 498.0, 298.0)));
        assert_eq!(rects[2], (3, Rect::new(502.0, 302.0, 498.0, 298.0)));
    }

    #[test]
    fn dividers_sit_in_the_gaps() {
        let d = three().dividers(AREA, 4.0);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].path.clone(), d[0].gap), (vec![], Rect::new(498.0, 0.0, 4.0, 600.0)));
        assert_eq!((d[1].path.clone(), d[1].gap), (vec![true], Rect::new(502.0, 298.0, 498.0, 4.0)));
    }

    #[test]
    fn a_divider_moves_and_stops_short_of_nothing() {
        let mut n = three();
        n.set_ratio(&[true], 0.25);
        assert_eq!(n.layout(AREA, 0.0)[1].1.h, 150.0);
        n.set_ratio(&[], 2.0);
        assert_eq!(n.layout(AREA, 0.0)[0].1.w, 950.0, "clamped to 1 - MIN_RATIO");
    }

    #[test]
    fn a_pane_moves_beside_another_or_trades_places() {
        // 1 | 2 over 3; 1 goes below 3: 2 over (3 over 1).
        let n = three().moved(&1, &3, Toward::Down);
        assert_eq!(n.leaves(), vec![2, 3, 1]);
        let r = n.layout(AREA, 0.0);
        assert_eq!(r[0].1, Rect::new(0.0, 0.0, 1000.0, 300.0), "2 takes the room 1 left");
        assert_eq!(r[2].1, Rect::new(0.0, 450.0, 1000.0, 150.0), "1 has the lower half of 3's");
        // 3 goes left of 2.
        assert_eq!(three().moved(&3, &2, Toward::Left).leaves(), vec![1, 3, 2]);
        assert_eq!(three().moved(&3, &3, Toward::Left), three(), "onto itself, nothing");
        let mut n = three();
        n.swap(&1, &3);
        assert_eq!(n.leaves(), vec![3, 2, 1]);
        assert_eq!(n.layout(AREA, 4.0)[0].1, three().layout(AREA, 4.0)[0].1, "the shape stays");
    }

    #[test]
    fn removing_a_pane_gives_its_room_to_its_sibling() {
        let n = three().remove(&2).unwrap();
        assert_eq!(n.leaves(), vec![1, 3]);
        assert_eq!(n.layout(AREA, 0.0)[1].1, Rect::new(500.0, 0.0, 500.0, 600.0));
        assert_eq!(Node::Leaf(1).remove(&1), None, "the last pane leaves nothing");
    }

    #[test]
    fn neighbours_are_found_by_where_they_are() {
        let n = three();
        assert_eq!(n.neighbor(&1, Toward::Right, AREA), Some(2), "the upper one, more in line");
        assert_eq!(n.neighbor(&3, Toward::Left, AREA), Some(1));
        assert_eq!(n.neighbor(&2, Toward::Down, AREA), Some(3));
        assert_eq!(n.neighbor(&3, Toward::Up, AREA), Some(2));
        assert_eq!(n.neighbor(&1, Toward::Left, AREA), None, "nothing past the edge");
        assert_eq!(n.neighbor(&2, Toward::Up, AREA), None);
    }

    #[test]
    fn the_shape_survives_a_map() {
        let n = three().map(&mut |t| t * 10);
        assert_eq!(n.leaves(), vec![10, 20, 30]);
        assert_eq!(n.layout(AREA, 4.0).iter().map(|(_, r)| *r).collect::<Vec<_>>(), three().layout(AREA, 4.0).iter().map(|(_, r)| *r).collect::<Vec<_>>());
    }

    #[test]
    fn splitting_a_pane_that_is_not_there_does_nothing() {
        let mut n = three();
        assert!(!n.split(&9, Dir::Right, 4));
        assert_eq!(n, three());
    }
}
