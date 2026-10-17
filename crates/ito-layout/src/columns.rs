//! Column widths a header's grips resize: the arithmetic only, so it is tested
//! without egui. `ui::grips` (the `egui` feature) says which edge moved by how
//! much; these turn that into widths.

/// Which side of its column the grip sits on, so which way widens it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// Dragging right widens the column (a table's columns).
    Right,
    /// Dragging left widens the column (a column laid out from the right).
    Left,
}

/// Moves one column's edge by `delta` (the pointer's movement, in points); the
/// width stays within `min..=max`. The others keep theirs, so the total moves.
pub fn resize(widths: &mut [f32], index: usize, delta: f32, min: f32, max: f32, edge: Edge) {
    let Some(w) = widths.get_mut(index) else { return };
    let grown = match edge {
        Edge::Right => *w + delta,
        Edge::Left => *w - delta,
    };
    *w = grown.clamp(min, max.max(min));
}

/// Moves the edge between column `index` and the next by `delta`: one widens
/// and the other narrows, so the total stays (a flexible column that fills
/// what the fixed ones leave). Neither goes under `min`.
pub fn trade(widths: &mut [f32], index: usize, delta: f32, min: f32) {
    let Some(&[a, b]) = widths.get(index..index + 2).map(|s| <&[f32; 2]>::try_from(s).unwrap()) else { return };
    let total = a + b;
    let a = (a + delta).clamp(min, (total - min).max(min));
    widths[index] = a;
    widths[index + 1] = total - a;
}

/// The x of each column's right edge, the first column starting at `start`.
pub fn right_edges(start: f32, widths: &[f32]) -> Vec<f32> {
    widths
        .iter()
        .scan(start, |x, w| {
            *x += w;
            Some(*x)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_right_edge_widens_to_the_right() {
        let mut w = [100.0, 80.0];
        resize(&mut w, 0, 20.0, 40.0, 300.0, Edge::Right);
        assert_eq!(w, [120.0, 80.0]);
    }

    #[test]
    fn the_left_edge_widens_to_the_left() {
        let mut w = [100.0, 80.0];
        resize(&mut w, 1, -30.0, 40.0, 300.0, Edge::Left);
        assert_eq!(w, [100.0, 110.0]);
    }

    #[test]
    fn a_width_stays_within_its_limits() {
        let mut w = [100.0];
        resize(&mut w, 0, -500.0, 40.0, 300.0, Edge::Right);
        assert_eq!(w, [40.0]);
        resize(&mut w, 0, 900.0, 40.0, 300.0, Edge::Right);
        assert_eq!(w, [300.0]);
        // A max under the min cannot undo the min.
        resize(&mut w, 0, -900.0, 40.0, 10.0, Edge::Right);
        assert_eq!(w, [40.0]);
    }

    #[test]
    fn an_index_out_of_range_changes_nothing() {
        let mut w = [100.0];
        resize(&mut w, 3, 10.0, 40.0, 300.0, Edge::Right);
        trade(&mut w, 0, 10.0, 40.0);
        assert_eq!(w, [100.0]);
    }

    #[test]
    fn a_trade_keeps_the_total() {
        let mut w = [100.0, 200.0, 50.0];
        trade(&mut w, 0, 60.0, 40.0);
        assert_eq!(w, [160.0, 140.0, 50.0]);
        trade(&mut w, 0, 900.0, 40.0);
        assert_eq!(w, [260.0, 40.0, 50.0]);
        trade(&mut w, 0, -900.0, 40.0);
        assert_eq!(w, [40.0, 260.0, 50.0]);
    }

    #[test]
    fn the_edges_add_up() {
        assert_eq!(right_edges(10.0, &[100.0, 50.0]), vec![110.0, 160.0]);
    }
}
