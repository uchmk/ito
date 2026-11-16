use super::*;

fn rect(c: &Cap) -> (f32, f32, f32, f32) {
    (c.x, c.y, c.x + c.w, c.y + c.h)
}

fn overlap(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    a.0 < b.2 - 1e-4 && b.0 < a.2 - 1e-4 && a.1 < b.3 - 1e-4 && b.1 < a.3 - 1e-4
}

#[test]
fn the_main_rows_end_at_15_and_the_mac_bottom_rows_before_it() {
    for b in Board::ALL {
        let mut rows: Vec<(f32, f32)> = Vec::new();
        let main: Vec<&Cap> = caps(b).iter().filter(|c| c.block == Block::Main).collect();
        for c in main.iter().filter(|c| c.h <= 1.0) {
            let right = c.x + c.w;
            match rows.iter_mut().find(|(y, _)| (*y - c.y).abs() < 1e-4) {
                Some((_, r)) => *r = r.max(right),
                None => rows.push((c.y, right)),
            }
        }
        // A tall key (the JIS Enter) also ends the rows it spans.
        for c in main.iter().filter(|c| c.h > 1.0) {
            for (y, r) in rows.iter_mut() {
                if *y >= c.y - 1e-4 && *y < c.y + c.h - 1e-4 {
                    *r = r.max(c.x + c.w);
                }
            }
        }
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(rows.len(), 5, "{b:?}");
        for (i, (y, right)) in rows.iter().enumerate() {
            let bottom = i == 4;
            if bottom && b.is_mac() {
                assert!(*right <= 15.0 + 1e-4, "{b:?} bottom ends at {right}");
            } else {
                assert!((*right - 15.0).abs() < 1e-4, "{b:?} row at {y} ends at {right}");
            }
        }
    }
}

#[test]
fn the_nav_block_is_three_wide_and_the_whole_board_18_and_a_half() {
    for b in Board::ALL {
        let nav: Vec<&Cap> = caps(b).iter().filter(|c| matches!(c.block, Block::Nav | Block::Arrows)).collect();
        let left = nav.iter().map(|c| c.x).fold(f32::MAX, f32::min);
        let right = nav.iter().map(|c| c.x + c.w).fold(0.0, f32::max);
        assert_eq!((left, right - left), (15.5, 3.0), "{b:?}");
        let all = caps(b).iter().map(|c| c.x + c.w).fold(0.0, f32::max);
        assert_eq!(all, 18.5, "{b:?}");
    }
}

#[test]
fn no_two_keys_overlap_and_each_position_and_id_is_used_once() {
    for b in Board::ALL {
        let all = caps(b);
        for (i, a) in all.iter().enumerate() {
            let mut shapes = vec![rect(a)];
            if let Some((x, y, w, h)) = a.notch_rect() {
                shapes.push((x, y, x + w, y + h));
            }
            for o in &all[i + 1..] {
                let mut others = vec![rect(o)];
                if let Some((x, y, w, h)) = o.notch_rect() {
                    others.push((x, y, x + w, y + h));
                }
                for s in &shapes {
                    for t in &others {
                        assert!(!overlap(*s, *t), "{b:?}: {:?} and {:?}", a.phys, o.phys);
                    }
                }
            }
            assert_eq!(all.iter().filter(|c| c.phys == a.phys).count(), 1, "{b:?} {:?}", a.phys);
            assert_eq!(all.iter().filter(|c| c.phys.id() == a.phys.id()).count(), 1);
        }
    }
    let mut ids: Vec<&str> = Phys::ALL.iter().map(|p| p.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), Phys::ALL.len());
    assert_eq!(Phys::KeyG.code(), "KeyG");
    assert_eq!(Phys::IntlYen.id(), "intlyen");
}

#[test]
fn only_the_jis_enter_has_a_notch() {
    for b in Board::ALL {
        let notched: Vec<Phys> = caps(b).iter().filter(|c| c.notch > 0.0).map(|c| c.phys).collect();
        assert_eq!(notched, if b.family() == Family::Jis { vec![Phys::Enter] } else { vec![] }, "{b:?}");
    }
}

#[test]
fn the_boards_have_the_keys_of_their_family_and_os() {
    let has = |b: Board, p: Phys| caps(b).iter().any(|c| c.phys == p);
    assert!(has(Board::PcJis, Phys::IntlRo) && has(Board::PcJis, Phys::Backquote));
    assert!(!has(Board::PcUs, Phys::IntlRo) && !has(Board::MacJis, Phys::Backquote));
    assert!(has(Board::PcUs, Phys::Insert) && !has(Board::MacUs, Phys::Insert) && has(Board::MacUs, Phys::Fn));
    assert!(has(Board::MacJis, Phys::Lang1) && !has(Board::PcJis, Phys::Lang1));
    assert_eq!(Board::of(Family::Jis, true), Board::MacJis);
    assert_eq!(Board::of(Family::Us, false).family(), Family::Us);
    assert_eq!(modifier(Board::MacUs, Phys::MetaLeft), Some(Modifier::Cmd));
    assert_eq!(modifier(Board::PcUs, Phys::MetaLeft), None);
}

#[test]
fn legends_are_printed_the_way_the_keys_are() {
    assert_eq!(legend(Board::PcUs, Phys::KeyG), ("G", None));
    assert_eq!(legend(Board::PcUs, Phys::Digit2), ("2", Some("@")));
    assert_eq!(legend(Board::PcJis, Phys::Digit2), ("2", Some("\"")));
    assert_eq!(legend(Board::PcJis, Phys::IntlYen), ("\u{a5}", Some("|")));
    assert_eq!(legend(Board::PcUs, Phys::Enter).0, "Enter");
    assert_eq!(legend(Board::MacUs, Phys::Enter).0, "return");
    assert_eq!(legend(Board::MacUs, Phys::MetaLeft).0, "command \u{2318}");
    assert_eq!(legend(Board::PcJis, Phys::Quote), (":", Some("*")));
}
