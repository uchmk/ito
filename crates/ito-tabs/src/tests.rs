use super::*;

fn tabs(ids: &[&str], limit: usize) -> Tabs<String> {
    let mut t = Tabs::new(limit);
    for i in ids {
        t.open((*i).to_owned());
    }
    t
}

fn s(x: &str) -> String {
    x.to_owned()
}

#[test]
fn open_brings_an_existing_tab_forward_and_a_new_one_opens_beside_the_front() {
    let mut t = tabs(&["a", "b", "c"], 12);
    assert_eq!(t.front(), Some(&s("c")));
    assert_eq!(t.open(s("a")), 0);
    assert_eq!(t.items().len(), 3);
    assert_eq!(t.front(), Some(&s("a")));
    assert_eq!(t.open(s("d")), 1);
    assert_eq!(t.items(), [s("a"), s("d"), s("b"), s("c")]);
}

#[test]
fn next_and_prev_wrap_at_both_ends() {
    let mut t = tabs(&["a", "b", "c"], 12);
    t.apply(Op::Next);
    assert_eq!(t.front_index(), 0);
    t.apply(Op::Prev);
    assert_eq!(t.front_index(), 2);
    let mut empty: Tabs<String> = Tabs::new(3);
    assert_eq!(empty.apply(Op::Next), None);
    assert_eq!(empty.front(), None);
}

#[test]
fn closing_the_front_tab_brings_its_right_neighbour_or_the_last() {
    let mut t = tabs(&["a", "b", "c", "d"], 12);
    t.apply(Op::Goto(1));
    assert_eq!(t.apply(Op::Close), Some(s("b")));
    assert_eq!(t.front(), Some(&s("c")));
    t.apply(Op::Goto(2));
    assert_eq!(t.apply(Op::Close), Some(s("d")));
    assert_eq!(t.front(), Some(&s("c")));
    assert_eq!(t.apply(Op::Close), Some(s("c")));
    assert_eq!(t.apply(Op::Close), Some(s("a")));
    assert!(t.is_empty());
    assert_eq!(t.apply(Op::Close), None);
}

#[test]
fn closing_another_tab_keeps_the_front_tab_in_front() {
    let mut t = tabs(&["a", "b", "c"], 12);
    assert_eq!(t.apply(Op::CloseAt(0)), Some(s("a")));
    assert_eq!(t.front(), Some(&s("c")));
    assert_eq!(t.apply(Op::CloseAt(9)), None);
    assert_eq!(t.len(), 2);
}

#[test]
fn goto_ignores_an_index_out_of_range() {
    let mut t = tabs(&["a", "b"], 12);
    t.apply(Op::Goto(0));
    t.apply(Op::Goto(5));
    assert_eq!(t.front_index(), 0);
}

#[test]
fn move_keeps_the_same_tab_in_front() {
    let mut t = tabs(&["a", "b", "c", "d"], 12);
    t.apply(Op::Goto(1)); // b
    t.apply(Op::Move(1, 3));
    assert_eq!(t.items(), [s("a"), s("c"), s("d"), s("b")]);
    assert_eq!(t.front(), Some(&s("b")));
    t.apply(Op::Move(0, 2)); // a moves past the front
    assert_eq!(t.front(), Some(&s("b")));
    t.apply(Op::Move(3, 0));
    assert_eq!(t.front(), Some(&s("b")));
    assert_eq!(t.apply(Op::Move(0, 9)), None);
}

#[test]
fn the_limit_drops_the_leftmost_tab_that_is_not_the_new_one() {
    let mut t = tabs(&["a", "b", "c"], 3);
    let (i, dropped) = t.open_evicting(s("d"));
    assert_eq!(dropped, Some(s("a")));
    assert_eq!(t.items(), [s("b"), s("c"), s("d")]);
    assert_eq!((i, t.front_index()), (2, 2));
    let mut t = tabs(&["a", "b"], 2);
    t.apply(Op::Goto(0));
    t.open(s("z")); // inserted at 1; drops "a"
    assert_eq!(t.items(), [s("z"), s("b")]);
    assert_eq!(t.front(), Some(&s("z")));
    assert_eq!(Tabs::<String>::new(0).limit(), 1);
}

#[test]
fn labels_add_the_parent_folder_only_where_names_clash() {
    // A `\` separates components only where it is the platform's separator.
    #[cfg(windows)]
    {
        let l = labels(&["C:\\dev\\docs\\README.md", "C:\\dev\\api\\README.md", "C:\\dev\\notes.md"]);
        assert_eq!(l, ["docs/README.md", "api/README.md", "notes.md"]);
    }
    let l = labels(&["/a/x/README.md", "/b/x/README.md", "/b/x/other.md"]);
    assert_eq!(l, ["a/x/README.md", "b/x/README.md", "other.md"]);
    assert_eq!(labels(&["/same/README.md"]), ["README.md"]);
    let none: [&str; 0] = [];
    assert!(labels(&none).is_empty());
}

#[test]
fn toml_round_trip_keeps_order_and_front() {
    let mut t = tabs(&["a", "b", "c"], 12);
    t.apply(Op::Goto(1));
    let text = t.to_toml().unwrap();
    let back = Tabs::<String>::from_toml(&text, 12).unwrap();
    assert_eq!(back, t);
    assert!(Tabs::<String>::from_toml("not toml [", 12).is_none());
}

#[test]
fn restoring_cuts_repeats_extras_and_a_wild_front() {
    let t = Tabs::<String>::from_toml("front = 9\ntabs = [\"a\", \"a\", \"b\", \"c\"]\n", 2).unwrap();
    assert_eq!(t.items(), [s("a"), s("b")]);
    assert_eq!(t.front_index(), 0);
}

#[test]
fn saved_through_the_writer_and_loaded_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tabs.toml");
    let w = ito_common::kept::Writer::new(&path, true);
    let mut t = tabs(&["x.md", "y.md"], 12);
    t.apply(Op::Goto(0));
    w.save_now(t.to_toml().unwrap()).unwrap();
    assert_eq!(Tabs::<String>::load(&path, 12), t);
    assert!(Tabs::<String>::load(&dir.path().join("none.toml"), 5).is_empty());
    t.save(&ito_common::kept::Writer::new(dir.path().join("off.toml"), false));
}

#[test]
fn paths_work_as_ids() {
    let mut t: Tabs<PathBuf> = Tabs::new(4);
    t.open(PathBuf::from("a.md"));
    let back = Tabs::<PathBuf>::from_toml(&t.to_toml().unwrap(), 4).unwrap();
    assert_eq!(back, t);
}

#[test]
fn commands_map_to_ops() {
    assert_eq!(command("tab.next"), Some(Op::Next));
    assert_eq!(command("tab.prev"), Some(Op::Prev));
    assert_eq!(command("tab.close"), Some(Op::Close));
    assert_eq!(command("tab.goto_1"), Some(Op::Goto(0)));
    assert_eq!(command("tab.goto_9"), Some(Op::Goto(8)));
    assert_eq!(command("tab.goto_10"), None);
    assert_eq!(command("file.save"), None);
}

#[test]
fn settings_read_and_write_as_toml() {
    let s: Settings = toml::from_str("show_bar = \"always\"\nopen_from_outside = \"new_window\"\nreopen = \"only_given\"\n").unwrap();
    assert_eq!(
        (s.show_bar, s.open_from_outside, s.reopen, s.limit),
        (ShowBar::Always, OpenFromOutside::NewWindow, Reopen::OnlyGiven, DEFAULT_LIMIT)
    );
    let d: Settings = toml::from_str("").unwrap();
    assert_eq!(d, Settings::default());
    assert_eq!(toml::from_str::<Settings>(&toml::to_string(&s).unwrap()).unwrap(), s);
    assert!(toml::from_str::<Settings>("show_bar = \"sometimes\"").is_err());
}

#[test]
fn the_bar_shows_by_setting() {
    assert!(ShowBar::Always.visible(0));
    assert!(!ShowBar::TwoOrMore.visible(1));
    assert!(ShowBar::TwoOrMore.visible(2));
    assert!(!ShowBar::Never.visible(5));
}
