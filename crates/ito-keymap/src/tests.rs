use super::*;

const DEFAULTS: &str = r#"
[[main.keymap]]
on = "q"
run = "quit"

[[main.keymap]]
on = "g g"
run = "top"

[[main.keymap]]
on = "gG"
run = "bottom"

[[main.keymap]]
on = "<C-,>"
run = "settings"

[[main.keymap]]
on = "m s"
run = "mark save"

[[main.keymap]]
on = "m t"
run = "mark take"

[[help.keymap]]
on = "<Esc>"
run = "close"

[[help.keymap]]
on = "j"
run = "down"
"#;

const SPEC: Spec =
    Spec { layers: &[Layer { name: "main", aliases: &["manager"] }, Layer::new("help")], ignored: &["cmp"], defaults: DEFAULTS };

/// Any command but `fly` reads as itself; `bad` is kept but said.
fn command(s: &str, w: &mut Vec<String>) -> Option<String> {
    match s {
        "fly" => {
            w.push(format!("unknown command `{s}`"));
            None
        }
        "bad" => {
            w.push(format!("unsupported command `{s}`"));
            Some(s.to_owned())
        }
        _ => Some(s.to_owned()),
    }
}

fn load_with(files: &[(&str, &str)]) -> Keymap<String> {
    load(&SPEC, files, &mut command)
}

fn keys(s: &str) -> Vec<Key> {
    tokenize(s).iter().map(|t| Key::parse(t).unwrap()).collect()
}

fn run_of<'a>(km: &'a Keymap<String>, layer: &str, on: &str) -> Option<&'a str> {
    match resolve(km.layer(layer), &keys(on)) {
        Match::Exact(b) => Some(b.raw.as_str()),
        _ => None,
    }
}

#[test]
fn the_defaults_load_without_a_word() {
    let km = load_with(&[]);
    assert!(km.warnings.is_empty(), "{:?}", km.warnings);
    assert_eq!(km.layer("main").len(), 6);
    assert_eq!(run_of(&km, "main", "q"), Some("quit"));
    assert!(km.layer("nowhere").is_empty());
}

#[test]
fn keys_are_written_with_or_without_spaces() {
    assert_eq!(tokenize("gg"), ["g", "g"]);
    assert_eq!(tokenize("g g"), ["g", "g"]);
    assert_eq!(tokenize("<C-a>b"), ["<C-a>", "b"]);
    let km = load_with(&[]);
    assert_eq!(run_of(&km, "main", "g G"), Some("bottom"));
    assert!(matches!(resolve(km.layer("main"), &keys("g")), Match::Pending(c) if c.len() == 2));
    assert!(matches!(resolve(km.layer("main"), &keys("<F9>")), Match::None));
}

/// A prepended key comes first, and wins over the default chords it starts.
#[test]
fn a_prepended_key_wins_and_buries_its_chords() {
    let user = "[[main.prepend_keymap]]\non = \"m\"\nrun = \"bookmark\"\n";
    let km = load_with(&[("/cfg/keymap.toml", user)]);
    assert_eq!(run_of(&km, "main", "m"), Some("bookmark"));
    let about_m: Vec<&String> = km.warnings.iter().filter(|w| w.contains("`m`")).collect();
    assert_eq!(about_m.len(), 1, "one line per prefix: {:?}", km.warnings);
    assert!(about_m[0].contains("the 2 key(s)") && about_m[0].contains("`ms`"), "{}", about_m[0]);
}

#[test]
fn a_short_keymap_that_replaces_the_defaults_is_named() {
    let user = "[[main.keymap]]\non = \"<F9>\"\nrun = \"reload\"\n";
    let km = load_with(&[("/cfg/keymap.toml", user)]);
    assert_eq!(km.layer("main").len(), 1, "the defaults are replaced, as in yazi");
    assert_eq!(
        km.warnings,
        ["/cfg/keymap.toml: `[[main.keymap]]` replaces all 6 keys of [main] with 1 -- did you mean `[[main.prepend_keymap]]`?"]
    );
    // The same for a layer of two: under a quarter is the line.
    let km = load_with(&[("/cfg/keymap.toml", "[[help.keymap]]\non = \"x\"\nrun = \"close\"\n")]);
    assert!(km.warnings.is_empty(), "{:?}", km.warnings);
}

#[test]
fn a_broken_file_names_its_path_first_and_is_left_out() {
    let km = load_with(&[("/cfg/a.toml", "[[main.keymap]\n"), ("/cfg/b.toml", "[[main.append_keymap]]\non = \"x\"\nrun = \"y\"\n")]);
    assert!(km.warnings[0].starts_with("/cfg/a.toml: TOML parse error"), "{:?}", km.warnings);
    assert!(!km.warnings[0].ends_with('\n'));
    assert_eq!(run_of(&km, "main", "x"), Some("y"), "the next file still counts");
    assert!(!parses("[[main.keymap]\n") && parses(DEFAULTS));
}

#[test]
fn a_backslash_in_double_quotes_gets_the_single_quote_hint() {
    let user = "[[main.prepend_keymap]]\non = \"e\"\nrun = \"shell 'C:\\dev\\nvim.exe'\"\n";
    let km = load_with(&[("keymap.toml", user)]);
    assert_eq!(km.warnings.len(), 1, "{:?}", km.warnings);
    assert!(km.warnings[0].ends_with("write a Windows path in 'single quotes')"), "{}", km.warnings[0]);
}

#[test]
fn bad_keys_and_commands_are_said() {
    let user = r#"
        [main]
        append_keymap = [
            { on = "<Nope>", run = "quit" },
            { on = "z", run = "fly" },
            { on = "b", run = "bad" },
            { on = "<S-t>", run = "quit" },
            { on = "<C-S-t>", run = "quit" },
            { on = "", run = "quit" },
        ]
    "#;
    let km = load_with(&[("k.toml", user)]);
    let w = &km.warnings;
    assert!(w.contains(&"unknown key `<Nope>`".to_owned()), "{w:?}");
    assert!(w.contains(&"unknown command `fly`".to_owned()), "{w:?}");
    assert!(w.contains(&"unsupported command `bad`".to_owned()), "{w:?}");
    assert!(w.iter().any(|w| w.contains("`<S-t>` can never be pressed") && w.contains("write `T`")), "{w:?}");
    assert!(!w.iter().any(|w| w.contains("`<C-S-t>`")), "{w:?}");
    assert!(w.contains(&"k.toml: [main] a binding with no key".to_owned()), "{w:?}");
    assert_eq!(run_of(&km, "main", "z"), None, "a command that does not read leaves the binding out");
    assert_eq!(run_of(&km, "main", "b"), Some("bad"), "one the parser keeps stays");
}

#[test]
fn unknown_sections_and_fields_are_said_aliases_and_ignored_ones_are_not() {
    let user = r#"
        [[manager.prepend_keymap]]
        on = "x"
        run = "cut"
        [[cmp.keymap]]
        on = "<Tab>"
        run = "next"
        [[mian.prepend_keymap]]
        on = "y"
        run = "yank"
        [[help.prepnd_keymap]]
        on = "y"
        run = "yank"
        [[help.append_keymap]]
        on = "k"
        rnu = "up"
    "#;
    let km = load_with(&[("k.toml", user)]);
    assert_eq!(run_of(&km, "main", "x"), Some("cut"), "[manager] is [main]");
    assert_eq!(
        km.warnings,
        [
            "k.toml: [help] has `prepnd_keymap`, which a section does not take (keymap, prepend_keymap, append_keymap)",
            "k.toml: [help] a binding has `rnu`, which a binding does not take (on, on_mac, for, run, desc)",
            "k.toml: no layer is called [mian] (there are [main], [help])",
        ]
    );
}

#[test]
fn for_and_on_mac_pick_the_machine() {
    let user = r#"
        [[main.prepend_keymap]]
        on = "<F2>"
        for = "windows"
        run = "win"
        [[main.prepend_keymap]]
        on = "<F2>"
        for = "unix"
        run = "unix"
        [[main.prepend_keymap]]
        on = "<F3>"
        on_mac = "<F4>"
        run = "three"
        [[main.prepend_keymap]]
        on = "<F5>"
        for = "plan9"
        run = "x"
    "#;
    let km = load_with(&[("k.toml", user)]);
    assert_eq!(run_of(&km, "main", "<F2>"), Some(if cfg!(windows) { "win" } else { "unix" }));
    let f3 = if cfg!(target_os = "macos") { "<F4>" } else { "<F3>" };
    assert_eq!(run_of(&km, "main", f3), Some("three"));
    assert_eq!(km.warnings, ["k.toml: [main] `for = \"plan9\"` is none of windows, macos, linux, unix"]);
}

/// A user's key over a default one is an override, not a warning; two of the
/// user's own on one key are a warning naming both files; the same command on
/// both sides says nothing.
#[test]
fn duplicates_and_overrides() {
    let km = load_with(&[("/cfg/kura/keymap.toml", "[[main.prepend_keymap]]\non = \"q\"\nrun = \"close\"\n")]);
    assert!(km.warnings.is_empty(), "{:?}", km.warnings);
    assert_eq!(km.overrides, ["[main] `q`: `close` (/cfg/kura/keymap.toml) instead of the default `quit`"]);

    let a = "[[main.prepend_keymap]]\non = \"Q\"\nrun = \"quit\"\n";
    let b = "[[main.prepend_keymap]]\non = \"Q\"\nrun = \"hide\"\n";
    let km = load_with(&[("/a.toml", a), ("/b.toml", b)]);
    assert_eq!(km.warnings, ["[main] `Q` is bound more than once; only `hide` (/b.toml) runs, not `quit` (/a.toml)"]);

    let km = load_with(&[("/a.toml", "[[main.prepend_keymap]]\non = \"q\"\nrun = \"quit\"\n")]);
    assert!(km.warnings.is_empty() && km.overrides.is_empty(), "{:?} {:?}", km.warnings, km.overrides);

    let km = load_with(&[("/a.toml", "[[main.append_keymap]]\non = \"q\"\nrun = \"close\"\n")]);
    assert_eq!(km.warnings, [format!("[main] `q` is bound more than once; only `quit` ({BUILT_IN}) runs, not `close` (/a.toml)")]);
}

#[test]
fn a_list_of_commands_runs_in_turn() {
    let km = load_with(&[("k.toml", "[[main.prepend_keymap]]\non = [\"<F6>\"]\nrun = [\"one\", \"two\"]\ndesc = \"Both\"\n")]);
    let Match::Exact(b) = resolve(km.layer("main"), &keys("<F6>")) else { panic!() };
    assert_eq!(b.run, ["one", "two"]);
    assert_eq!((b.raw.as_str(), b.desc.as_str(), &*b.from), ("one; two", "Both", "k.toml"));
    let mut km = km;
    assert_eq!(km.take("help").len(), 2);
    assert!(km.layer("help").is_empty());
}

// ------------------------------------------------- the settings screen

fn names(k: &Keymap<String>) -> Vec<(String, Vec<Binding<String>>)> {
    k.layers.iter().map(|(n, b)| (n.clone(), b.clone())).collect()
}

fn table_of(files: &[(&str, &str)]) -> Vec<Row> {
    rows(&names(&load_with(files)), &names(&load_with(&[])), "noop")
}

fn row<'a>(rows: &'a [Row], run: &str) -> &'a Row {
    rows.iter().find(|r| r.run == run).unwrap_or_else(|| panic!("no row runs `{run}`"))
}

#[test]
fn the_defaults_alone_are_a_row_per_run_in_their_order_and_none_changed() {
    let rows = table_of(&[]);
    let runs: Vec<&str> = rows.iter().map(|r| r.run.as_str()).collect();
    assert_eq!(runs, ["quit", "top", "bottom", "settings", "mark save", "mark take", "close", "down"]);
    assert!(rows.iter().all(|r| !r.changed() && r.keys.iter().all(|k| k.state == State::Live)));
    assert_eq!(row(&rows, "top").defaults, [keys("g g")]);
}

#[test]
fn a_users_key_comes_first_and_a_noop_strikes_a_default_out() {
    let file = r#"
[[main.prepend_keymap]]
on = "<C-t>"
run = "top"

[[main.prepend_keymap]]
on = "q"
run = "noop"

[[main.prepend_keymap]]
on = "x"
run = "mine"
"#;
    let rows = table_of(&[("km.toml", file)]);
    let top = row(&rows, "top");
    assert!(top.changed());
    assert_eq!(top.keys.iter().map(|k| k.on.clone()).collect::<Vec<_>>(), [keys("<C-t>"), keys("g g")]);
    assert_eq!(&*top.keys[0].from, "km.toml");
    let quit = row(&rows, "quit");
    assert!(quit.changed() && quit.keys[0].removed);
    assert!(rows.iter().all(|r| r.run != "noop"), "a noop is a mark, not a row");
    // An operation only the file binds comes after the defaults of its layer.
    let runs: Vec<&str> = rows.iter().filter(|r| r.layer == "main").map(|r| r.run.as_str()).collect();
    assert_eq!(runs.last(), Some(&"mine"));
}

#[test]
fn a_key_a_shorter_one_runs_first_is_shadowed_by_it() {
    let file = "[[main.prepend_keymap]]\non = \"g\"\nrun = \"quit\"\n";
    let rows = table_of(&[("km.toml", file)]);
    let top = row(&rows, "top");
    assert!(matches!(&top.keys[0].state, State::Shadowed { run, .. } if run == "quit"));
    assert!(top.changed(), "the file took its key");
    assert!(!top.keys[0].removed);
}

#[test]
fn clashes_name_what_the_new_keys_would_shut_out() {
    let km = load_with(&[("km.toml", "[[main.prepend_keymap]]\non = \"z\"\nrun = \"noop\"\n")]);
    let main = km.layer("main");
    assert_eq!(clashes(main, &keys("q"), "top", "noop"), [Clash::Same { run: "quit".into(), desc: String::new() }]);
    let prefix: Vec<String> = clashes(main, &keys("g"), "quit", "noop")
        .into_iter()
        .map(|c| match c {
            Clash::Prefix { run, .. } => run,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(prefix, ["top", "bottom"]);
    assert!(clashes(main, &keys("g g h"), "quit", "noop").len() == 1, "`g g` runs first");
    assert!(clashes(main, &keys("g g"), "top", "noop").is_empty(), "its own key");
    assert!(clashes(main, &keys("z"), "top", "noop").is_empty(), "a noop is nothing to lose");
    assert_eq!(common(&keys("<C-,>")), Some("the settings"));
    assert_eq!(common(&keys("<C-t>")), None);
}

fn edited(text: &str, edits: &[Edit]) -> String {
    edit(text, &SPEC, edits, "noop").unwrap()
}

fn add(on: &str, run: &str) -> Edit {
    Edit::Add { layer: "main".into(), on: keys(on), run: run.into(), desc: String::new() }
}

fn remove(on: &str, run: &str) -> Edit {
    Edit::Remove { layer: "main".into(), on: keys(on), run: run.into() }
}

fn reset(run: &str) -> Edit {
    Edit::Reset { layer: "main".into(), run: run.into() }
}

fn top_keys(text: &str) -> Vec<Vec<Key>> {
    let km = load_with(&[("km.toml", text)]);
    assert!(km.warnings.is_empty(), "{:?}", km.warnings);
    let rows = rows(&names(&km), &names(&load_with(&[])), "noop");
    row(&rows, "top").keys.iter().filter(|k| k.state == State::Live).map(|k| k.on.clone()).collect()
}

#[test]
fn nothing_changed_writes_nothing() {
    assert_eq!(edited("", &[reset("top")]), "");
    assert_eq!(edited("", &[add("g g", "top")]), "", "a default key is already there");
    let mine = "# my keys\n";
    assert_eq!(edited(mine, &[remove("<C-t>", "top")]), mine);
}

#[test]
fn a_key_added_is_prepended_and_works() {
    let out = edited("", &[Edit::Add { layer: "main".into(), on: keys("<C-S-Enter>"), run: "top".into(), desc: "Go top".into() }]);
    assert_eq!(out, "[[main.prepend_keymap]]\non = \"<C-S-Enter>\"\nrun = \"top\"\ndesc = \"Go top\"\n");
    assert_eq!(top_keys(&out), [keys("<C-S-Enter>"), keys("g g")]);
    // Keys pressed in turn are a list.
    let out = edited("", &[add("g h", "top")]);
    assert!(out.contains("on = [\"g\", \"h\"]"), "{out}");
}

#[test]
fn a_default_key_taken_away_is_a_noop_and_given_back_is_nothing() {
    let out = edited("", &[remove("g g", "top")]);
    assert_eq!(out, "[[main.prepend_keymap]]\non = \"g\"\nrun = \"noop\"\n".replace("\"g\"", "[\"g\", \"g\"]"));
    assert!(top_keys(&out).is_empty());
    let back = edited(&out, &[add("g g", "top")]);
    assert!(!back.contains("prepend_keymap"), "{back}");
    assert_eq!(top_keys(&back), [keys("g g")]);
}

#[test]
fn a_users_own_key_taken_away_leaves_no_line() {
    let out = edited("", &[add("<C-t>", "top"), add("<C-y>", "top")]);
    let out = edited(&out, &[remove("<C-t>", "top")]);
    assert!(!out.contains("<C-t>") && out.contains("<C-y>"), "{out}");
}

#[test]
fn a_key_given_to_another_run_leaves_its_old_one() {
    let out = edited("", &[add("<C-t>", "quit")]);
    let out = edited(&out, &[add("<C-t>", "top")]);
    assert_eq!(out.matches("<C-t>").count(), 1, "{out}");
    assert!(out.contains("run = \"top\""), "{out}");
}

#[test]
fn a_reset_takes_the_runs_lines_out_and_keeps_the_rest() {
    let start = "# my keys\n\n[[main.prepend_keymap]]\non = \"x\"\nrun = \"mine\" # kept\n";
    let out = edited(start, &[add("<C-t>", "top"), remove("g g", "top")]);
    assert!(top_keys(&out) == [keys("<C-t>")], "{out}");
    let back = edited(&out, &[reset("top")]);
    assert_eq!(back, start);
    // Everything reset: no list left, the comments still there.
    let none = edited(&edited("# mine\n", &[add("<C-t>", "top")]), &[reset("top")]);
    assert!(!none.contains("prepend_keymap") && none.contains("# mine"), "{none}");
}

#[test]
fn an_alias_section_is_written_where_it_is() {
    let start = "[[manager.prepend_keymap]]\non = \"x\"\nrun = \"mine\"\n";
    let out = edited(start, &[add("<C-t>", "top")]);
    assert!(!out.contains("[[main."), "{out}");
    assert_eq!(out.matches("[[manager.prepend_keymap]]").count(), 2, "{out}");
}

#[test]
fn a_file_the_screen_cannot_write_back_is_left_alone() {
    assert!(edit("[[main.keymap]]\non = \"q\"\nrun = \"quit\"\n", &SPEC, &[add("<C-t>", "top")], "noop").is_err());
    assert!(edit("[[main.prepend_keymap]\n", &SPEC, &[add("<C-t>", "top")], "noop").is_err());
    assert!(edit("", &SPEC, &[Edit::Reset { layer: "nope".into(), run: "x".into() }], "noop").is_err());
    // A line for some machines only is the person's: kept as written.
    let only = "[[main.prepend_keymap]]\non = \"<C-t>\"\nfor = \"macos\"\nrun = \"quit\"\n";
    assert_eq!(edited(only, &[add("<C-t>", "top")]).matches("for = \"macos\"").count(), 1);
}
