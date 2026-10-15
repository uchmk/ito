use super::*;

/// Lays out rows given as `(id, parents, trunk, name)`, top to bottom.
fn lay(rows: &[(&'static str, &[&'static str], bool, &'static str)]) -> (Layout<&'static str>, Colors) {
    let mut colors = Colors::default();
    colors.pin("main", 6);
    let mut l = Layout::new();
    for &(id, parents, trunk, name) in rows {
        let dot = if parents.len() > 1 { Dot::Merge } else { Dot::Commit };
        l.push(Row { id, parents, name: (!name.is_empty()).then_some(name), trunk, dot }, &mut colors);
    }
    (l, colors)
}

fn lanes(l: &Layout<&str>) -> Vec<u16> {
    l.rows().iter().map(|r| r.lane).collect()
}

#[test]
fn a_straight_line_stays_in_lane_0() {
    let (l, _) = lay(&[("c", &["b"], true, "main"), ("b", &["a"], true, "main"), ("a", &[], true, "main")]);
    assert_eq!(lanes(&l), [0, 0, 0]);
    assert_eq!(l.width(), 1);
    assert!(l.rows().iter().all(|r| r.color == 6));
    assert_eq!(l.edges_of(0), [Edge { from: 0, to: Some(1), via: 0, first: true, color: 6, long: false }]);
    assert!(l.edges_of(2).is_empty());
}

#[test]
fn a_merged_branch_takes_lane_1_and_gives_it_back() {
    // m merges x2 (x1 below it) into main; then y forks off and is merged too.
    let (l, colors) = lay(&[
        ("n", &["k", "y1"], true, "main"),
        ("y1", &["k"], false, "y"),
        ("k", &["m"], true, "main"),
        ("m", &["a", "x2"], true, "main"),
        ("x2", &["x1"], false, "x"),
        ("x1", &["a"], false, "x"),
        ("a", &[], true, "main"),
    ]);
    assert_eq!(lanes(&l), [0, 1, 0, 0, 1, 1, 0]);
    // The second parent runs in lane 1 and takes the branch's colour.
    let into_y = &l.edges_of(0)[1];
    assert_eq!((into_y.to, into_y.via, into_y.first, into_y.color), (Some(1), 1, false, colors.of("y")));
    // y's lane is free once y1 joins main at k, and x takes it again.
    assert_eq!(l.rows()[2].span, 0);
    assert_eq!(l.rows()[4].lane, 1);
    // x1's first parent runs down lane 1 and bends into main at a.
    assert_eq!(l.edges_of(5), [Edge { from: 5, to: Some(6), via: 1, first: true, color: colors.of("x"), long: false }]);
    assert_eq!(l.width(), 2);
}

#[test]
fn two_children_of_one_parent_meet_in_the_leftmost_lane() {
    let (l, _) = lay(&[
        ("m", &["p"], true, "main"),
        ("b", &["q"], false, "b"),
        ("c", &["q"], false, "c"),
        ("q", &["p"], false, "b"),
        ("p", &[], true, "main"),
    ]);
    assert_eq!(lanes(&l), [0, 1, 2, 1, 0]);
    assert_eq!(l.rows()[2].span, 2);
    // c's edge keeps its own lane down to q, and its own colour.
    let e = &l.edges_of(2)[0];
    assert_eq!((e.to, e.via), (Some(3), 2));
    assert_eq!(l.rows()[3].span, 1);
}

#[test]
fn rows_above_the_trunk_leave_lane_0_to_it() {
    // A worktree's uncommitted row sits above everything else.
    let (l, _) = lay(&[("wip", &["f"], false, "f"), ("main", &["f"], true, "main"), ("f", &[], true, "main")]);
    assert_eq!(lanes(&l), [1, 0, 0]);
    let mut l2 = Layout::without_trunk();
    l2.push(Row { id: "a", parents: &["b"], name: None, trunk: false, dot: Dot::Commit }, &mut Colors::default());
    assert_eq!(l2.rows()[0].lane, 0);
}

#[test]
fn parents_shown_twice_or_already_laid_out_are_skipped() {
    let (l, _) = lay(&[("a", &["b", "b", "a"], true, "main"), ("b", &["a"], true, "main")]);
    assert_eq!(l.edges_of(0).len(), 1);
    assert!(l.edges_of(1).is_empty());
    // The same id twice is laid out once.
    let (mut l, mut colors) = lay(&[("a", &[], true, "main")]);
    assert_eq!(l.push(Row { id: "a", parents: &[], name: None, trunk: true, dot: Dot::Commit }, &mut colors), 0);
    assert_eq!(l.len(), 1);
}

#[test]
fn an_edge_to_a_parent_still_to_come_runs_to_the_bottom_then_to_nothing() {
    let (mut l, mut colors) = lay(&[("m", &["a", "x"], true, "main"), ("a", &["root"], true, "main")]);
    let m = Metrics::default();
    let open = &l.edges_of(0)[1];
    assert_eq!(open.to, None);
    let path = l.path(open, m);
    // Into lane 1, then down to the middle of the row after the last.
    assert_eq!(path.len(), 2);
    assert_eq!(path[1], Seg::Line(Pt::new(m.x(1), m.y(1)), Pt::new(m.x(1), m.y(2))));
    // Open edges cross every row below their child.
    assert_eq!(l.edges(1..2).count(), 3);
    assert_eq!(l.edges(2..3).count(), 2);
    l.push(Row { id: "x", parents: &[], name: Some("x"), trunk: false, dot: Dot::Commit }, &mut colors);
    assert_eq!(l.edges_of(0)[1].to, Some(2));
    l.finish();
    let stub = l.path(&l.edges_of(1)[0], m);
    assert_eq!(&*stub, [Seg::Line(Pt::new(m.x(0), m.y(1)), Pt::new(m.x(0), m.y(1) + m.row * 0.9))]);
}

#[test]
fn the_paths_bend_where_the_lanes_change() {
    let (l, _) =
        lay(&[("m", &["a", "x2"], true, "main"), ("x2", &["x1"], false, "x"), ("x1", &["a"], false, "x"), ("a", &[], true, "main")]);
    let m = Metrics::default();
    // Next row, same lane: a line. Next row, another lane: one curve.
    assert!(matches!(&*l.path(&l.edges_of(1)[0], m), [Seg::Line(..)]));
    assert!(matches!(&*l.path(&l.edges_of(2)[0], m), [Seg::Cubic(..)]));
    // m to x2 is the next row, in lane 1: a curve from lane 0.
    let p = l.path(&l.edges_of(0)[1], m);
    assert_eq!(
        &*p,
        [Seg::Cubic([Pt::new(m.x(0), m.y(0)), Pt::new(m.x(0), m.y(0) + 15.6), Pt::new(m.x(1), m.y(1) - 15.6), Pt::new(m.x(1), m.y(1))])]
    );
    // m to a, down lane 0 all the way: one line.
    assert_eq!(&*l.path(&l.edges_of(0)[0], m), [Seg::Line(Pt::new(m.x(0), m.y(0)), Pt::new(m.x(0), m.y(3)))]);
}

#[test]
fn a_long_edge_leaves_its_lane_and_comes_back() {
    // b's parent is far down; the trunk runs beside it.
    let mut rows: Vec<(&'static str, Vec<&'static str>, bool, &'static str)> = vec![("b", vec!["base"], false, "b")];
    let ids: Vec<&'static str> = (0..5).map(|k| &*Box::leak(format!("t{k}").into_boxed_str())).collect();
    for (k, id) in ids.iter().enumerate() {
        rows.push((id, vec![ids.get(k + 1).copied().unwrap_or("base")], true, "main"));
    }
    rows.push(("base", vec![], true, "main"));
    let rows: Vec<_> = rows.iter().map(|(a, p, t, n)| (*a, p.as_slice(), *t, *n)).collect();
    let (l, _) = lay(&rows);
    let m = Metrics::default();
    let e = &l.edges_of(0)[0];
    assert_eq!((e.via, e.to), (1, Some(6)));
    // Down lane 1 from b, then into lane 0 in the last row.
    let p = l.path(e, m);
    assert_eq!(p.len(), 2);
    assert_eq!(p[0], Seg::Line(Pt::new(m.x(1), m.y(0)), Pt::new(m.x(1), m.y(5))));
    assert!(matches!(p[1], Seg::Cubic([_, _, _, end]) if end == Pt::new(m.x(0), m.y(6))));
}

/// A history of `n` commits with many branches, merges and long edges, from
/// a fixed seed, laid out as far as row `upto`; and which rows are the trunk.
fn tangle(n: u32, upto: u32) -> (Layout<u32>, Vec<bool>) {
    let mut seed = 0x2545_f491_u32;
    let mut next = move |k: u32| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed % k
    };
    let parents: Vec<Vec<u32>> = (0..n)
        .map(|i| {
            let mut ps = vec![i + 1 + next(3)];
            if next(4) == 0 {
                ps.push(i + 2 + next(80));
            }
            ps.retain(|&p| p < n);
            ps
        })
        .collect();
    let mut trunk = vec![false; n as usize];
    let mut at = Some(0);
    while let Some(i) = at {
        trunk[i as usize] = true;
        at = parents[i as usize].first().copied();
    }
    let mut colors = Colors::default();
    let mut l = Layout::new();
    for i in 0..upto {
        let name = format!("b{}", next(6));
        l.push(Row { id: i, parents: &parents[i as usize], name: Some(&name), trunk: trunk[i as usize], dot: Dot::Commit }, &mut colors);
    }
    (l, trunk)
}

#[test]
fn the_edges_in_sight_are_all_the_edges_that_cross_them() {
    let (l, _) = tangle(600, 600);
    check_edges_in_sight(&l, &[(0, 10), (37, 80), (300, 301), (590, 700), (0, 600)]);
    // Half read: the edges into the rest are open, and cross every row below.
    let (l, _) = tangle(600, 300);
    assert!(l.edges_of(299).iter().any(|e| e.to.is_none()));
    check_edges_in_sight(&l, &[(0, 10), (250, 300), (299, 300), (300, 400)]);
}

fn check_edges_in_sight(l: &Layout<u32>, ranges: &[(u32, u32)]) {
    let all: Vec<&Edge> = (0..l.len() as u32).flat_map(|i| l.edges_of(i)).collect();
    assert!(all.iter().any(|e| e.to.is_some_and(|t| t - e.from > LONG)), "the test needs long edges");
    for &(a, b) in ranges {
        let mut got: Vec<_> = l.edges(a..b).map(|e| (e.from, e.to)).collect();
        let mut want: Vec<_> = all.iter().filter(|e| e.from < b && e.to.is_none_or(|t| t >= a)).map(|e| (e.from, e.to)).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "rows {a}..{b}");
    }
}

#[test]
fn the_width_is_the_widest_row() {
    let (l, trunk) = tangle(300, 300);
    assert_eq!(l.width(), l.rows().iter().map(|r| r.span + 1).max().unwrap());
    assert!(l.width_in(0..5) <= l.width());
    assert_eq!(l.width_in(400..500), 0);
    // The trunk has lane 0 to itself.
    assert!(trunk.iter().filter(|&&t| t).count() > 20);
    for (i, r) in l.rows().iter().enumerate() {
        if trunk[i] {
            assert_eq!(r.lane, 0);
        } else {
            assert!(r.lane >= 1);
        }
    }
}

#[test]
fn colours_are_pinned_reserved_hashed_and_kept() {
    let mut c = Colors::default();
    c.pin("main", 6);
    c.reserve(["fix/142", "feat/focus", "main"]);
    assert_eq!((c.of("main"), c.of("fix/142"), c.of("feat/focus")), (6, 3, 5));
    // The same name hashes the same way every time.
    let h = c.of("feat/other");
    assert_eq!(c.of("feat/other"), h);
    assert!([4, 12, 13, 10, 11].contains(&h));
    // Beside a lane of the same colour, the newcomer moves on, and keeps it.
    let moved = c.place(Some("feat/other"), &[h]);
    assert_ne!(moved, h);
    assert_eq!(c.place(Some("feat/other"), &[]), moved);
    assert_eq!(c.of("feat/other"), moved);
    // A pinned or reserved colour never moves.
    assert_eq!(c.place(Some("main"), &[6]), 6);
    // No name: the first of the pool not beside it.
    assert_eq!(c.place(None, &[4]), 12);
    assert!(ALLOWED.contains(&c.place(None, &[])));
}

#[test]
fn reserving_more_names_than_colours_hashes_the_rest() {
    let mut c = Colors::new(&[4, 12], &[3, 5]);
    c.reserve(["a", "b", "c"]);
    assert_eq!((c.of("a"), c.of("b")), (3, 5));
    assert!([4, 12].contains(&c.of("c")));
}

#[test]
fn a_branch_beside_one_of_its_colour_moves_on() {
    let mut colors = Colors::new(&[4, 12], &[]);
    let mut l = Layout::new();
    // Find two names that hash to the same colour.
    let names: Vec<String> = (0..20).map(|k| format!("n{k}")).collect();
    let a = &names[0];
    let b = names.iter().skip(1).find(|n| colors.of(n) == colors.of(a)).unwrap();
    l.push(Row { id: 0, parents: &[10, 11], name: Some("main"), trunk: true, dot: Dot::Merge }, &mut colors);
    l.push(Row { id: 11, parents: &[12], name: Some(a), trunk: false, dot: Dot::Commit }, &mut colors);
    l.push(Row { id: 13, parents: &[12], name: Some(b), trunk: false, dot: Dot::Commit }, &mut colors);
    assert_eq!(l.rows()[2].lane, 2);
    assert_ne!(l.rows()[2].color, l.rows()[1].color);
}

#[cfg(feature = "egui")]
#[test]
fn every_kind_of_dot_and_edge_paints() {
    let mut colors = Colors::default();
    let mut l = Layout::new();
    for (id, parents, trunk, dot) in [
        (0, &[3][..], false, Dot::Wip),
        (1, &[2, 4], true, Dot::Conflict),
        (2, &[3, 5], true, Dot::Merge),
        (3, &[9], true, Dot::Commit),
        (4, &[3], false, Dot::Commit),
    ] {
        l.push(Row { id, parents, name: Some("x"), trunk, dot }, &mut colors);
    }
    let look = ui::Look { ansi: [egui::Color32::GRAY; 16], bg: egui::Color32::BLACK, conflict: egui::Color32::RED, width: 1.5 };
    let ctx = egui::Context::default();
    for finished in [false, true] {
        if finished {
            l.finish();
        }
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui::paint(ui.painter(), ui.min_rect().min, &l, 0..10, Metrics::default(), &look);
            ui::paint(ui.painter(), ui.min_rect().min, &l, 7..l.len() as u32 + 3, Metrics::default(), &look);
        });
        out.textures_delta.clear();
        assert!(!out.shapes.is_empty());
    }
}
