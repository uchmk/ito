//! Model tests (no egui).

use super::*;

fn ticket_of(w: Option<Want>) -> (Ticket, String) {
    match w {
        Some(Want::List { ticket, path }) => (ticket, path),
        None => panic!("no want"),
        #[allow(unreachable_patterns)]
        _ => panic!("other want"),
    }
}

fn posix(mode: Mode) -> Picker {
    Picker::new("t", mode, Style::Posix).start("/home/u")
}

/// Takes the want and answers it.
fn answer(p: &mut Picker, entries: Vec<Entry>) -> (Ticket, String) {
    let (t, path) = ticket_of(p.take().want);
    assert!(p.set_listing(t, Ok(Listing::new(entries))));
    (t, path)
}

fn names(p: &Picker) -> Vec<String> {
    p.rows().iter().map(|&i| p.entry(i).unwrap().name.clone()).collect()
}

fn picked(p: &mut Picker) -> Vec<String> {
    match p.take().done {
        Some(Done::Picked(v)) => v,
        d => panic!("not picked: {d:?}"),
    }
}

fn sample() -> Vec<Entry> {
    vec![
        Entry::file("b.txt").size(10),
        Entry::dir("sub"),
        Entry::file("a.TXT").size(5),
        Entry::file("file10.sh"),
        Entry::file("file2.sh"),
        Entry::file(".bashrc"),
        Entry::file("Makefile"),
        Entry::other("sock"),
        Entry::unknown("broken"),
    ]
}

// ---- Path ----

#[test]
fn posix_paths() {
    let s = Style::Posix;
    assert_eq!(s.join("/", "a"), "/a");
    assert_eq!(s.join("/a", "b"), "/a/b");
    assert_eq!(s.join("/a/", "b\\c"), "/a/b\\c");
    assert_eq!(s.parent("/a/b").as_deref(), Some("/a"));
    assert_eq!(s.parent("/a").as_deref(), Some("/"));
    assert_eq!(s.parent("/"), None);
    assert_eq!(s.resolve("../x", "/a/b").unwrap(), "/a/x");
    assert_eq!(s.resolve("/../..", "/a").unwrap(), "/");
    assert_eq!(s.resolve("a\\b", "/").unwrap(), "/a\\b");
    assert!(s.is_under("/a/b", "/a"));
    assert!(!s.is_under("/ab", "/a"));
    assert!(!s.same("/A", "/a"));
    let c: Vec<_> = s.crumbs("/a/b").into_iter().map(|(_, p)| p).collect();
    assert_eq!(c, ["/", "/a", "/a/b"]);
}

#[test]
fn windows_paths() {
    let s = Style::Windows;
    assert_eq!(s.resolve("C:", "D:\\x").unwrap(), "C:\\");
    assert_eq!(s.resolve("c:/a/b", "D:\\").unwrap(), "C:\\a\\b");
    assert_eq!(s.resolve("..\\..\\..", "C:\\a").unwrap(), "C:\\");
    assert_eq!(s.resolve("\\x", "D:\\a\\b").unwrap(), "D:\\x");
    assert_eq!(s.resolve("\\x", "\\\\srv\\share\\a").unwrap(), "\\\\srv\\share\\x");
    assert_eq!(s.resolve("\\\\srv\\share", "C:\\").unwrap(), "\\\\srv\\share\\");
    assert_eq!(s.resolve("a/b", "C:\\").unwrap(), "C:\\a\\b");
    assert_eq!(s.resolve("C:\\a.. ", "C:\\").unwrap(), "C:\\a.. ");
    assert_eq!(s.resolve("C:foo", "C:\\"), Err(PathError::DriveRelative));
    assert_eq!(s.resolve("\\\\?\\C:\\", "C:\\"), Err(PathError::Verbatim));
    assert_eq!(s.resolve("\\\\.\\x", "C:\\"), Err(PathError::Verbatim));
    assert_eq!(s.resolve("\\\\server", "C:\\"), Err(PathError::NoShare));
    assert_eq!(s.parent("C:\\a").as_deref(), Some("C:\\"));
    assert_eq!(s.parent("C:\\"), None);
    assert_eq!(s.parent("\\\\srv\\share\\"), None);
    assert!(s.same("C:\\Foo", "c:\\foo\\"));
    assert!(s.is_under("C:\\Foo\\bar", "c:\\foo"));
    assert_eq!(s.join("C:\\", "a"), "C:\\a");
    let c: Vec<_> = s.crumbs("C:\\a\\b").into_iter().map(|(_, p)| p).collect();
    assert_eq!(c, ["C:\\", "C:\\a", "C:\\a\\b"]);
}

// ---- Listing ----

#[test]
fn bad_names_are_left_out_and_counted() {
    let mut p = posix(Mode::OpenFile);
    let (t, _) = ticket_of(p.take().want);
    let l = Listing::new(vec![Entry::file("ok"), Entry::file("a/b"), Entry::file(".."), Entry::file(""), Entry::file("n\0")]).left_out(2);
    assert!(p.set_listing(t, Ok(l)));
    assert_eq!(names(&p), ["ok"]);
    assert_eq!(p.left_out(), 6);
    assert!(p.incomplete());
}

#[test]
fn natural_sort_and_folders_first() {
    let mut p = posix(Mode::OpenFile).show_hidden(true);
    answer(&mut p, sample());
    assert_eq!(names(&p), ["sub", ".bashrc", "a.TXT", "b.txt", "file2.sh", "file10.sh", "Makefile", "broken", "sock"]);
    for by in [Sort::Modified, Sort::Kind, Sort::Size, Sort::Name] {
        p.set_sort(by);
        assert_eq!(p.row_entry(0).unwrap().name, "sub", "{by:?}");
        p.set_sort(by);
        assert_eq!(p.row_entry(0).unwrap().name, "sub", "{by:?} descending");
        let n = names(&p);
        let tail: Vec<&str> = n[n.len() - 2..].iter().map(String::as_str).collect();
        assert!(tail.contains(&"sock") && tail.contains(&"broken"), "{by:?}: Other and Unknown last");
    }
    assert_eq!(natural_cmp("a2", "a10"), std::cmp::Ordering::Less);
    assert_eq!(natural_cmp("A", "a"), "A".cmp("a"));
}

#[test]
fn kind_sort_is_extension_without_words() {
    let mut p = posix(Mode::OpenFile).show_hidden(true).sort(Sort::Kind, false);
    answer(&mut p, sample());
    // Folders, then no extension (.bashrc, Makefile), then sh, then txt; Other and Unknown last.
    assert_eq!(names(&p), ["sub", ".bashrc", "Makefile", "file2.sh", "file10.sh", "a.TXT", "b.txt", "broken", "sock"]);
}

#[test]
fn filters_hidden_rules() {
    let mut p = posix(Mode::OpenFile).filters(vec![Filter::new("Text", &["txt"]), Filter::new("Shell", &["*.sh"])]);
    answer(&mut p, sample());
    // Case-insensitive; dot files hidden; Makefile and .bashrc have no extension.
    assert_eq!(names(&p), ["sub", "a.TXT", "b.txt", "broken", "sock"]);
    p.set_filter(1);
    assert_eq!(names(&p), ["sub", "file2.sh", "file10.sh", "broken", "sock"]);
    p.set_filter(2);
    assert!(names(&p).contains(&"Makefile".to_owned()));
    assert!(!names(&p).contains(&".bashrc".to_owned()));
    p.set_show_hidden(true);
    assert!(names(&p).contains(&".bashrc".to_owned()));
    assert_eq!(p.filter_chosen(), None);
    assert!(Filter::new("none", &[]).matches("x"));
    assert_eq!(extension(".bashrc"), None);
    assert_eq!(extension("a."), None);
    assert_eq!(extension("a.B").as_deref(), Some("b"));
}

#[test]
fn windows_hidden_is_the_flag_only() {
    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows).start("C:\\");
    answer(&mut p, vec![Entry::file(".git"), Entry::file("secret").hidden(true), Entry::dir("d").hidden(true)]);
    assert_eq!(names(&p), [".git"]);
    p.set_show_hidden(true);
    assert_eq!(names(&p), ["d", ".git", "secret"]);
}

#[test]
fn folder_mode_shows_only_folders() {
    let mut p = posix(Mode::OpenFolder);
    answer(&mut p, sample());
    assert_eq!(names(&p), ["sub", "broken"]);
}

// ---- Tickets and history ----

#[test]
fn only_the_newest_ticket_lives() {
    let mut p = posix(Mode::OpenFile);
    let (a, _) = ticket_of(p.take().want);
    p.go("/b");
    let (b, path) = ticket_of(p.take().want);
    assert_eq!(path, "/b");
    p.go("/a");
    let (c, _) = ticket_of(p.take().want);
    assert!(!p.set_listing(a, Ok(Listing::new(vec![Entry::file("late")]))));
    assert!(!p.set_listing(b, Ok(Listing::new(vec![]))));
    assert!(p.set_listing(c, Ok(Listing::new(vec![Entry::file("x")]))));
    assert!(!p.set_listing(c, Ok(Listing::new(vec![]))));
    assert_eq!(names(&p), ["x"]);
    assert_eq!(a.to_string(), "#1");
}

#[test]
fn an_untaken_want_is_replaced() {
    let mut p = posix(Mode::OpenFile);
    p.go("/a");
    p.go("/b");
    p.refresh();
    p.refresh();
    let s = p.take();
    let (_, path) = ticket_of(s.want);
    assert_eq!(path, "/b");
    assert!(p.take().want.is_none());
}

#[test]
fn nothing_after_done() {
    let mut p = posix(Mode::OpenFile);
    let (t, _) = answer(&mut p, vec![Entry::file("f")]);
    p.cancel();
    assert_eq!(p.take().done, Some(Done::Cancelled));
    assert_eq!(p.take().done, None);
    assert!(p.is_done());
    p.go("/x");
    assert!(p.take().want.is_none());
    assert!(!p.set_listing(t, Ok(Listing::default())));
}

#[test]
fn a_late_answer_after_done_is_dropped() {
    let mut p = posix(Mode::OpenFile);
    let (t, _) = ticket_of(p.take().want);
    p.cancel();
    assert!(!p.set_listing(t, Ok(Listing::default())));
}

#[test]
fn current_is_never_a_failed_folder() {
    let mut p = posix(Mode::OpenFile);
    assert_eq!(p.current(), None);
    answer(&mut p, vec![]);
    assert_eq!(p.current(), Some("/home/u"));
    p.go("/nope");
    let (t, _) = ticket_of(p.take().want);
    assert_eq!(p.current(), Some("/home/u"));
    p.set_listing(t, Err("permission denied".into()));
    assert_eq!(p.status(), Status::Failed("permission denied"));
    assert_eq!(p.current(), Some("/home/u"));
    assert_eq!(p.target(), "/nope");
    p.retry();
    let (_, path) = ticket_of(p.take().want);
    assert_eq!(path, "/nope");
}

#[test]
fn history_and_up() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::dir("docs")]);
    p.go("/home/u/docs");
    answer(&mut p, vec![]);
    assert!(p.can_back() && !p.can_forward());
    p.up();
    let (t, path) = ticket_of(p.take().want);
    assert_eq!(path, "/home/u");
    p.set_listing(t, Ok(Listing::new(vec![Entry::dir("a"), Entry::dir("docs"), Entry::dir("z")])));
    assert_eq!(p.row_entry(p.cursor_row().unwrap()).unwrap().name, "docs");
    p.back();
    let (_, path) = ticket_of(p.take().want);
    assert_eq!(path, "/home/u/docs");
    assert!(p.can_forward());
    p.forward();
    let (_, path) = ticket_of(p.take().want);
    assert_eq!(path, "/home/u");
    // A new navigation drops the forward half.
    p.back();
    p.take();
    p.go("/tmp");
    assert!(!p.can_forward());
}

#[test]
fn refresh_keeps_selection_by_name() {
    let mut p = posix(Mode::OpenFiles);
    answer(&mut p, vec![Entry::file("a"), Entry::file("b"), Entry::file("c")]);
    p.click(0, false, false);
    p.click(2, true, false);
    p.refresh();
    let (t, _) = ticket_of(p.take().want);
    p.set_listing(t, Ok(Listing::new(vec![Entry::file("c"), Entry::file("a"), Entry::file("new")])));
    assert_eq!(p.selected_count(), 2);
    assert_eq!(picks(&mut p), ["/home/u/a", "/home/u/c"]);
}

fn picks(p: &mut Picker) -> Vec<String> {
    p.accept();
    picked(p)
}

// ---- Typed grammar ----

#[test]
fn typed_plain_names() {
    let mut p = posix(Mode::OpenFile).show_hidden(false).filters(vec![Filter::new("Text", &["txt"])]);
    answer(&mut p, sample());
    // A hidden and a filtered name are still accepted.
    p.set_name(".bashrc");
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/.bashrc"]);
}

#[test]
fn typed_folder_goes_in_and_file_in_folder_mode_is_refused() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, sample());
    p.set_name("sub");
    p.accept();
    assert_eq!(ticket_of(p.take().want).1, "/home/u/sub");

    let mut p = posix(Mode::OpenFolder);
    answer(&mut p, sample());
    p.set_name("b.txt");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::ChooseFolder));
    p.set_name("sock");
    assert_eq!(p.error(), None);
    p.accept();
    assert_eq!(p.error(), Some(&InputError::CannotOpen));
    p.set_name("nothere");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::NotFound { name: "nothere".into() }));
}

#[test]
fn other_and_unknown_cannot_be_picked() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, sample());
    p.set_name("sock");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::CannotOpen));
    p.set_name("broken");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::CannotOpen));
    p.set_cursor(p.rows().iter().position(|&i| p.entry(i).unwrap().name == "sock").unwrap());
    assert_eq!(p.selected_count(), 0);
    assert!(!p.is_done());
}

#[test]
fn windows_lookup_is_case_insensitive_and_returns_the_listing_spelling() {
    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows).start("C:\\d");
    answer(&mut p, vec![Entry::file("ReadMe.TXT")]);
    p.set_name("  readme.txt ");
    p.accept();
    assert_eq!(picked(&mut p), ["C:\\d\\ReadMe.TXT"]);

    // Posix is exact.
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::file("ReadMe")]);
    p.set_name("readme");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::NotFound { name: "readme".into() }));

    // Two names that differ only by case: exact wins, otherwise ambiguous is not found.
    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows).start("C:\\d");
    answer(&mut p, vec![Entry::file("A"), Entry::file("a")]);
    p.set_name("a");
    p.accept();
    assert_eq!(picked(&mut p), ["C:\\d\\a"]);
}

#[test]
fn incomplete_listing_never_proves_a_name_missing() {
    for (truncated, left_out) in [(true, 0), (false, 1)] {
        let mut p = posix(Mode::OpenFile);
        let (t, _) = ticket_of(p.take().want);
        p.set_listing(t, Ok(Listing::new(vec![Entry::file("a"), Entry::file("b")]).truncated(truncated).left_out(left_out)));
        p.set_name("zzz");
        p.accept();
        assert_eq!(p.error(), Some(&InputError::NotAmongShown { name: "zzz".into(), shown: 2 }), "{truncated} {left_out}");
    }
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::file("a")]);
    p.set_name("zzz");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::NotFound { name: "zzz".into() }));
}

#[test]
fn typed_paths_and_pending_accept() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![]);
    p.set_name("../x/y.txt");
    p.accept();
    let (t, path) = ticket_of(p.take().want);
    assert_eq!(path, "/home/x");
    assert!(!p.is_done());
    p.set_listing(t, Ok(Listing::new(vec![Entry::file("y.txt")])));
    assert_eq!(picked(&mut p), ["/home/x/y.txt"]);

    // An edit drops the pending accept.
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![]);
    p.set_name("/a/b.txt");
    p.accept();
    let (t, _) = ticket_of(p.take().want);
    p.set_name("/a/c.txt");
    p.set_listing(t, Ok(Listing::new(vec![Entry::file("b.txt")])));
    assert!(!p.is_done());

    // A new navigation drops it too.
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![]);
    p.set_name("/a/b.txt");
    p.accept();
    p.take();
    p.go("/z");
    let (t, _) = ticket_of(p.take().want);
    p.set_listing(t, Ok(Listing::new(vec![Entry::file("b.txt")])));
    assert!(!p.is_done());

    // A failure drops it.
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![]);
    p.set_name("/a/b.txt");
    p.accept();
    let (t, _) = ticket_of(p.take().want);
    p.set_listing(t, Err("no".into()));
    assert!(!p.is_done());
    assert_eq!(p.status(), Status::Failed("no"));
}

#[test]
fn pending_accept_in_a_truncated_listing_is_not_found_never() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![]);
    p.set_name("/a/b.txt");
    p.accept();
    let (t, _) = ticket_of(p.take().want);
    p.set_listing(t, Ok(Listing::new(vec![Entry::file("c")]).truncated(true)));
    assert!(matches!(p.error(), Some(InputError::NotAmongShown { .. })));
}

#[test]
fn typed_folder_and_special_forms() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![]);
    p.set_name("/etc/");
    p.accept();
    assert_eq!(ticket_of(p.take().want).1, "/etc");
    p.set_name("..");
    p.accept();
    assert_eq!(ticket_of(p.take().want).1, "/");
    // "." in OpenFile does nothing.
    p.set_name(".");
    p.accept();
    assert!(p.take().want.is_none());

    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows).start("C:\\");
    answer(&mut p, vec![]);
    p.set_name("\"a\" \"b\"");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::OneName));
    p.set_name("C:foo");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::Path(PathError::DriveRelative)));
}

#[test]
fn no_default_extension_is_added() {
    let mut p = posix(Mode::OpenFile).filters(vec![Filter::new("Text", &["txt"])]);
    answer(&mut p, vec![Entry::file("note"), Entry::file("note.txt")]);
    p.set_name("note");
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/note"]);
}

// ---- Selection and OpenFolder ----

#[test]
fn not_edited_box_picks_the_real_names() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::file("we\"ird"), Entry::file("b")]);
    p.click(1, false, false);
    assert_eq!(p.name(), "we\"ird");
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/we\"ird"]);
}

#[test]
fn open_files_selection_in_shown_order() {
    let mut p = posix(Mode::OpenFiles);
    answer(&mut p, vec![Entry::dir("d"), Entry::file("a"), Entry::file("b"), Entry::file("c")]);
    p.click(3, false, false);
    p.click(1, true, false);
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/a", "/home/u/c"]);

    let mut p = posix(Mode::OpenFiles);
    answer(&mut p, vec![Entry::dir("d"), Entry::file("a"), Entry::file("b"), Entry::file("c")]);
    p.click(1, false, false);
    p.click(3, false, true);
    assert_eq!(p.selected_count(), 3);
    p.select_all();
    assert_eq!(p.selected_count(), 3);
    p.accept();
    assert_eq!(picked(&mut p).len(), 3);
}

#[test]
fn one_file_mode_ignores_modifiers() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::file("a"), Entry::file("b")]);
    p.click(0, true, false);
    p.click(1, true, false);
    assert_eq!(p.selected_count(), 1);
}

#[test]
fn enter_on_a_selected_folder_goes_in() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::dir("d"), Entry::file("a")]);
    p.set_cursor(0);
    p.accept();
    assert_eq!(ticket_of(p.take().want).1, "/home/u/d");
}

#[test]
fn open_folder_accept() {
    let mut p = posix(Mode::OpenFolder);
    answer(&mut p, vec![Entry::dir("d")]);
    // An empty box picks the target.
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u"]);

    let mut p = posix(Mode::OpenFolder);
    answer(&mut p, vec![Entry::dir("d")]);
    p.click(0, false, false);
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/d"]);

    // Still loading: nothing to pick.
    let mut p = posix(Mode::OpenFolder);
    p.take();
    assert!(!p.can_accept());
    p.accept();
    assert!(!p.is_done());
}

#[test]
fn picks_are_deduplicated() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::file("a")]);
    p.set_name("/home/u/a");
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/a"]);
}

#[test]
fn double_click_and_open_row() {
    let mut p = posix(Mode::OpenFiles);
    answer(&mut p, vec![Entry::dir("d"), Entry::file("a")]);
    p.open_row(1);
    assert_eq!(picked(&mut p), ["/home/u/a"]);
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::dir("d"), Entry::file("a")]);
    p.open_row(0);
    assert_eq!(ticket_of(p.take().want).1, "/home/u/d");
}

#[test]
fn start_and_first_name() {
    let mut p = Picker::new("t", Mode::OpenFile, Style::Posix).places(vec![Place::new("Home", "/h", PlaceKind::Home)]);
    assert_eq!(ticket_of(p.take().want).1, "/h");
    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows);
    assert_eq!(ticket_of(p.take().want).1, "C:\\");
    let mut p = posix(Mode::OpenFile).file_name("b");
    answer(&mut p, vec![Entry::file("a"), Entry::file("b")]);
    assert_eq!(p.selected_count(), 1);
    assert_eq!(p.row_entry(p.cursor_row().unwrap()).unwrap().name, "b");
}

// ---- Type-ahead ----

#[test]
fn type_ahead() {
    use std::time::{Duration, Instant};
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::file("apple"), Entry::file("avocado"), Entry::file("banana"), Entry::file("blueberry")]);
    let t0 = Instant::now();
    p.type_ahead("b", t0);
    assert_eq!(p.row_entry(p.cursor_row().unwrap()).unwrap().name, "banana");
    p.type_ahead("b", t0 + Duration::from_millis(100));
    assert_eq!(p.row_entry(p.cursor_row().unwrap()).unwrap().name, "blueberry");
    p.type_ahead("l", t0 + Duration::from_millis(200));
    assert_eq!(p.row_entry(p.cursor_row().unwrap()).unwrap().name, "blueberry");
    // After a second the prefix starts over.
    p.type_ahead("a", t0 + Duration::from_secs(3));
    assert_eq!(p.row_entry(p.cursor_row().unwrap()).unwrap().name, "apple");
    p.type_ahead("a", t0 + Duration::from_secs(3) + Duration::from_millis(50));
    assert_eq!(p.row_entry(p.cursor_row().unwrap()).unwrap().name, "avocado");
    p.type_ahead("\u{1b}", t0 + Duration::from_secs(9));
    assert_eq!(p.row_entry(p.cursor_row().unwrap()).unwrap().name, "avocado");
}

#[test]
fn size_text() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(1023), "1023 B");
    assert_eq!(format_size(1536), "1.5 KB");
    assert_eq!(format_size(10 * 1024 * 1024), "10 MB");
}

#[test]
fn words_cover_errors() {
    let w = Words::for_lang("ja");
    assert!(w.error(&InputError::NotAmongShown { name: "x".into(), shown: 3 }).contains('3'));
    assert!(Words::en().error(&InputError::NotFound { name: "x".into() }).contains("\"x\""));
    assert_eq!(Words::en().kind_word(Kind::File, Some("sh")), "SH file");
    assert_eq!(w.title(Mode::OpenFolder), w.choose_folder);
}

#[test]
fn ten_thousand_entries() {
    let mut p = posix(Mode::OpenFiles);
    let entries: Vec<_> = (0..100_000).map(|i| Entry::file(&format!("f{i}.txt"))).collect();
    answer(&mut p, entries);
    assert_eq!(p.rows().len(), 100_000);
    p.move_cursor(isize::MAX / 2, false);
    assert_eq!(p.cursor_row(), Some(99_999));
}

#[test]
fn error_texts_read_the_name_last() {
    let w = Words::en();
    let text = w.error(&InputError::NotAmongShown { name: "{n}".into(), shown: 7 });
    assert_eq!(text, "\"{n}\" is not among the 7 items shown. Type a folder's path to open it.");
    let ja = Words::ja().error(&InputError::NotAmongShown { name: "{n}".into(), shown: 7 });
    assert!(ja.starts_with("「{n}」は表示している 7 件の中にありません。"), "{ja}");
    assert_eq!(Words::ja().error(&InputError::NotFound { name: "a".into() }), "「a」が見つかりません。");
}

#[test]
fn the_words_the_spec_names() {
    assert_eq!(Words::en().loading, "Loading\u{2026}");
    assert_eq!(Words::en().failed, "Cannot open this folder");
    assert_eq!(Words::en().no_match, "No items match the filter");
    assert_eq!(Words::ja().loading, "読み込み中…");
    assert_eq!(Words::ja().failed, "このフォルダーを開けません");
    assert_eq!(Words::ja().no_match, "フィルターに合うものがありません");
}

#[test]
fn a_bad_start_goes_to_the_first_place_with_the_error() {
    let places = vec![Place::new("Home", "D:\\home", PlaceKind::Home)];
    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows).places(places).start("C:foo");
    assert_eq!(ticket_of(p.take().want).1, "D:\\home");
    assert_eq!(p.target(), "D:\\home");
    assert_eq!(p.path_error(), Some(&PathError::DriveRelative));
}

#[test]
fn a_bad_start_and_no_place_goes_to_the_root() {
    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows).start("C:foo");
    assert_eq!(ticket_of(p.take().want).1, "C:\\");
    assert_eq!(p.path_error(), Some(&PathError::DriveRelative));
    // A first place that does not read falls back to the root as well.
    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows).places(vec![Place::new("Bad", "\\\\nas", PlaceKind::Folder)]);
    assert_eq!(ticket_of(p.take().want).1, "C:\\");
    assert_eq!(p.path_error(), Some(&PathError::NoShare));
    // When the start and the first place both fail, the start's fault is the one told.
    let bad = vec![Place::new("Bad", "\\\\nas", PlaceKind::Folder)];
    let mut p = Picker::new("t", Mode::OpenFile, Style::Windows).places(bad).start("C:foo");
    assert_eq!(ticket_of(p.take().want).1, "C:\\");
    assert_eq!(p.path_error(), Some(&PathError::DriveRelative));
}

#[test]
fn a_unc_server_without_a_share_is_refused() {
    let w = Style::Windows;
    assert_eq!(w.resolve("\\\\nas", "\\\\nas\\share"), Err(PathError::NoShare));
    assert_eq!(w.resolve("\\\\nas\\", "\\\\nas\\share"), Err(PathError::NoShare));
    assert_eq!(w.resolve("//nas", "C:\\"), Err(PathError::NoShare));
    // One backslash is a path on the base's root, which is a different thing.
    assert_eq!(w.resolve("\\nas", "\\\\nas\\share").unwrap(), "\\\\nas\\share\\nas");
}

#[test]
fn entry_out_of_range_is_none() {
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::file("a")]);
    assert_eq!(p.entry(0).map(|e| e.name.as_str()), Some("a"));
    assert!(p.entry(1).is_none());
    assert!(p.entry(usize::MAX).is_none());
}

#[test]
fn a_filter_that_hides_the_selection_clears_the_name() {
    let mut p = posix(Mode::OpenFiles).filters(vec![Filter::new("Markdown", &["md"])]).filter_index(1);
    answer(&mut p, vec![Entry::file("a.txt"), Entry::file("b.md")]);
    p.click(0, false, false);
    assert_eq!(p.name(), "a.txt");
    p.set_filter(0);
    assert_eq!(p.selected_count(), 0);
    assert_eq!(p.name(), "");
}

#[test]
fn the_lit_place_is_the_deepest_one_that_holds_the_target() {
    let places = vec![
        Place::new("Root", "/", PlaceKind::Root),
        Place::new("Home", "/home/koyomi", PlaceKind::Home),
        Place::new("Jobs", "/home/koyomi/jobs2", PlaceKind::Folder),
    ];
    let mut p = Picker::new("t", Mode::OpenFile, Style::Posix).places(places).start("/home/koyomi/jobs");
    p.begin();
    assert_eq!(p.lit_place(), Some(1));
    p.go("/home/koyomi");
    assert_eq!(p.lit_place(), Some(1));
    p.go("/etc");
    assert_eq!(p.lit_place(), Some(0));
    let mut none =
        Picker::new("t", Mode::OpenFile, Style::Posix).places(vec![Place::new("Home", "/home/koyomi", PlaceKind::Home)]).start("/etc");
    none.begin();
    assert_eq!(none.lit_place(), None);
}

#[test]
fn shift_home_and_end_extend_in_many_file_mode_only() {
    let files = vec![Entry::file("a"), Entry::file("b"), Entry::file("c"), Entry::file("d")];
    let mut many = posix(Mode::OpenFiles);
    answer(&mut many, files.clone());
    many.click(1, false, false);
    many.move_cursor(isize::MAX / 2, true);
    assert_eq!(many.selected_count(), 3);
    many.move_cursor(isize::MIN / 2, true);
    assert_eq!(many.selected_count(), 2);
    let mut one = posix(Mode::OpenFile);
    answer(&mut one, files);
    one.click(1, false, false);
    one.move_cursor(isize::MAX / 2, true);
    assert_eq!(one.selected_count(), 1);
    assert_eq!(one.name(), "d");
}

// ---- Save as ----

fn save_files() -> Vec<Entry> {
    vec![Entry::file("report.csv"), Entry::file("notes.md"), Entry::dir("sub"), Entry::dir("data.csv"), Entry::other("sock")]
}

fn saver() -> Picker {
    let mut p = Picker::new("t", Mode::SaveFile, Style::Posix)
        .start("/home/u")
        .file_name("report")
        .filters(vec![Filter::new("CSV", &["csv"]), Filter::new("Text", &["txt"])]);
    answer(&mut p, save_files());
    p
}

fn win_saver() -> Picker {
    let mut p = Picker::new("t", Mode::SaveFile, Style::Windows).start("C:\\u").filters(vec![Filter::new("CSV", &["csv"])]);
    answer(&mut p, save_files());
    p
}

fn row_of(p: &Picker, n: &str) -> usize {
    p.rows().iter().position(|&i| p.entry(i).unwrap().name == n).unwrap()
}

#[test]
fn save_new_name_gets_the_filter_extension() {
    let mut p = saver();
    assert!(p.can_accept());
    p.set_name("fresh");
    p.accept();
    assert_eq!(p.replaces(), Some(false));
    assert_eq!(picked(&mut p), ["/home/u/fresh.csv"]);
    assert_eq!(p.replaces(), Some(false), "readable after take");
}

#[test]
fn save_keeps_a_typed_extension_even_outside_the_filter() {
    let mut p = saver();
    p.set_name("fresh.md");
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/fresh.md"]);
    let mut p = saver();
    p.set_name(".env");
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/.env"]);
}

#[test]
fn save_existing_file_asks_then_replaces() {
    let mut p = saver();
    p.accept();
    assert_eq!(p.replace_question(), Some("report.csv"));
    assert!(!p.is_done());
    assert_eq!(p.replaces(), None);
    p.answer_replace(true);
    assert_eq!(p.replaces(), Some(true));
    assert_eq!(picked(&mut p), ["/home/u/report.csv"]);
}

#[test]
fn save_question_no_keeps_the_name() {
    let mut p = saver();
    p.accept();
    p.answer_replace(false);
    assert_eq!(p.replace_question(), None);
    assert!(!p.is_done());
    assert_eq!(p.name(), "report.csv");
    assert_eq!(p.replaces(), None);
}

#[test]
fn save_without_confirmation_picks_at_once() {
    let mut p = Picker::new("t", Mode::SaveFile, Style::Posix).start("/home/u").file_name("report.csv").confirm_replace(false);
    answer(&mut p, save_files());
    p.accept();
    assert_eq!(p.replaces(), Some(true));
    assert_eq!(picked(&mut p), ["/home/u/report.csv"]);
}

#[test]
fn save_question_locks_the_model() {
    let mut p = saver();
    p.accept();
    p.set_name("other");
    p.set_filter(1);
    p.go("/x");
    p.up();
    p.accept();
    assert_eq!(p.name(), "report.csv");
    assert_eq!(p.replace_question(), Some("report.csv"));
    assert!(!p.can_accept());
    assert!(p.take().want.is_none());
    p.cancel();
    assert!(p.is_done());
    assert_eq!(p.replaces(), None);
    assert!(matches!(p.take().done, Some(Done::Cancelled)));
}

#[test]
fn save_late_listing_is_dropped_while_asking() {
    let mut p = saver();
    p.set_name("sub/new.csv");
    p.accept();
    let (t, _) = ticket_of(p.take().want);
    assert!(p.set_listing(t, Ok(Listing::new(vec![Entry::file("new.csv")]))));
    assert_eq!(p.replace_question(), Some("new.csv"));
    // Nothing can be asked for while the question is open, and a stray answer changes nothing.
    p.refresh();
    assert!(p.take().want.is_none());
    assert!(!p.set_listing(t, Ok(Listing::new(vec![]))));
    assert_eq!(p.replace_question(), Some("new.csv"));
    assert_eq!(p.entry_count(), 1);
}

#[test]
fn save_double_click_on_a_file_asks() {
    let mut p = saver();
    assert!(!p.rows().iter().any(|&i| p.entry(i).unwrap().name == "notes.md"), "CSV hides notes.md");
    p.set_filter(2);
    let r = row_of(&p, "notes.md");
    p.open_row(r);
    assert_eq!(p.replace_question(), Some("notes.md"));
}

#[test]
fn save_selecting_a_file_writes_its_name_and_a_folder_keeps_it() {
    let mut p = saver();
    p.set_filter(2);
    let r = row_of(&p, "notes.md");
    p.click(r, false, false);
    assert_eq!(p.name(), "notes.md");
    let d = row_of(&p, "sub");
    p.click(d, false, false);
    assert_eq!(p.name(), "notes.md");
    assert!(p.opens_folder());
    p.accept();
    assert_eq!(p.target(), "/home/u/sub");
    assert!(!p.is_done());
}

#[test]
fn save_typing_after_a_folder_click_saves_instead_of_opening() {
    let mut p = saver();
    let d = row_of(&p, "sub");
    p.click(d, false, false);
    assert!(p.opens_folder());
    p.set_name("x");
    assert!(!p.opens_folder());
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/x.csv"]);
}

#[test]
fn save_name_that_is_a_folder_navigates_or_refuses() {
    let mut p = saver();
    p.set_name("sub");
    p.accept();
    assert_eq!(p.target(), "/home/u/sub");
    let mut p = saver();
    p.set_name("data");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::CannotReplace { name: "data.csv".to_owned() }));
    assert!(!p.is_done());
    let mut p = saver();
    p.set_name("sock");
    p.set_filter(2);
    p.accept();
    assert_eq!(p.error(), Some(&InputError::CannotReplace { name: "sock".to_owned() }));
}

#[test]
fn save_navigation_text_only_navigates_and_keeps_the_name() {
    for t in [".", "..", "sub/", "sub/..", "sub/."] {
        let mut p = saver();
        p.set_name(t);
        p.accept();
        assert!(!p.is_done(), "{t}");
        assert_eq!(p.replace_question(), None, "{t}");
        assert_eq!(p.name(), "report", "{t}: the name box goes back");
    }
    let mut p = saver();
    p.set_name("..");
    p.accept();
    let (_, path) = ticket_of(p.take().want);
    assert_eq!(path, "/home");
}

#[test]
fn save_typed_path_saves_in_that_folder() {
    let mut p = saver();
    p.set_name("sub/new.csv");
    p.accept();
    let (t, path) = ticket_of(p.take().want);
    assert_eq!(path, "/home/u/sub");
    assert!(!p.is_done());
    assert!(p.set_listing(t, Ok(Listing::new(vec![Entry::file("new.csv")]))));
    assert_eq!(p.replace_question(), Some("new.csv"));
    p.answer_replace(true);
    assert_eq!(picked(&mut p), ["/home/u/sub/new.csv"]);
}

#[test]
fn save_typed_path_to_a_new_name_picks_after_the_listing() {
    let mut p = saver();
    p.set_name("/tmp/out");
    p.accept();
    let (t, path) = ticket_of(p.take().want);
    assert_eq!(path, "/tmp");
    assert!(p.set_listing(t, Ok(Listing::new(vec![]))));
    assert_eq!(p.replaces(), Some(false));
    assert_eq!(picked(&mut p), ["/tmp/out.csv"]);
}

#[test]
fn save_pending_is_dropped_by_a_filter_change() {
    let mut p = saver();
    p.set_name("sub/new.csv");
    p.accept();
    let (t, _) = ticket_of(p.take().want);
    p.set_filter(1);
    p.set_listing(t, Ok(Listing::new(vec![Entry::file("new.csv")])));
    assert!(!p.is_done());
    assert_eq!(p.replace_question(), None);
}

#[test]
fn save_windows_is_case_insensitive_and_posix_is_not() {
    let mut p = win_saver();
    p.set_name("REPORT.CSV");
    p.accept();
    assert_eq!(p.replace_question(), Some("report.csv"));
    assert_eq!(p.name(), "report.csv");
    let mut p = saver();
    p.set_name("report.CSV");
    p.accept();
    assert_eq!(p.replaces(), Some(false));
    assert_eq!(picked(&mut p), ["/home/u/report.CSV"]);
}

#[test]
fn save_windows_ambiguous_match_is_an_error() {
    let mut p = Picker::new("t", Mode::SaveFile, Style::Windows).start("C:\\u");
    answer(&mut p, vec![Entry::file("Ab.txt"), Entry::file("aB.txt")]);
    p.set_name("AB.txt");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::Ambiguous { name: "AB.txt".to_owned() }));
    assert!(!p.is_done());
    p.set_name("Ab.txt");
    p.accept();
    assert_eq!(p.replace_question(), Some("Ab.txt"));
}

#[test]
fn save_windows_refuses_bad_names_and_posix_accepts_them() {
    for (n, why) in [
        ("a*b", NameFault::Char),
        ("a\"b", NameFault::Char),
        ("CON", NameFault::Reserved),
        ("nul.txt", NameFault::Reserved),
        ("COM1", NameFault::Reserved),
        ("x.", NameFault::Trailing),
    ] {
        let mut p = win_saver();
        p.set_name(n);
        p.accept();
        assert!(matches!(p.error(), Some(InputError::BadName { why: w, .. }) if *w == why), "{n}: {:?}", p.error());
        assert!(!p.is_done(), "{n}");
    }
    // "a:b" reads as a drive-relative path, so it is refused as a path rather than as a name.
    let mut p = win_saver();
    p.set_name("a:b");
    p.accept();
    assert_eq!(p.error(), Some(&InputError::Path(PathError::DriveRelative)));
    assert!(!p.is_done());
    let mut p = saver();
    p.set_name("a:b");
    p.accept();
    assert_eq!(picked(&mut p), ["/home/u/a:b.csv"]);
}

#[test]
fn save_extension_is_checked_after_it_is_added() {
    let mut p = Picker::new("t", Mode::SaveFile, Style::Windows).start("C:\\u").filters(vec![Filter::new("X", &["txt"])]);
    answer(&mut p, vec![]);
    p.set_name("nul");
    p.accept();
    assert!(matches!(p.error(), Some(InputError::BadName { why: NameFault::Reserved, .. })), "{:?}", p.error());
}

#[test]
fn save_filter_change_swaps_the_extension() {
    let mut p = saver();
    p.set_name("a.csv");
    p.set_filter(1);
    assert_eq!(p.name(), "a.txt");
    p.set_filter(1);
    assert_eq!(p.name(), "a.txt", "same filter: nothing");
    p.set_filter(2);
    assert_eq!(p.name(), "a.txt", "all files keeps it");
    p.set_filter(1);
    p.set_name("a.md");
    p.set_filter(0);
    assert_eq!(p.name(), "a.md", "a foreign extension is kept");
}

#[test]
fn save_filter_change_cuts_at_the_dot_when_lowercase_changes_the_length() {
    // The Kelvin sign (3 bytes) lowercases to `k` (1 byte).
    let mut p = Picker::new("t", Mode::SaveFile, Style::Posix)
        .start("/home/u")
        .filters(vec![Filter::new("K", &["k"]), Filter::new("Text", &["txt"])]);
    answer(&mut p, vec![]);
    p.set_name("ab.\u{212A}");
    p.set_filter(1);
    assert_eq!(p.name(), "ab.txt");
}

#[test]
fn save_cancel_closes_the_question() {
    let mut p = saver();
    p.set_name("report.csv");
    p.accept();
    assert_eq!(p.replace_question(), Some("report.csv"));
    p.cancel();
    assert_eq!(p.replace_question(), None);
    assert!(matches!(p.take().done, Some(Done::Cancelled)));
}

#[test]
fn save_can_accept_cases() {
    let mut p = saver();
    assert!(p.can_accept());
    p.set_name("");
    assert!(!p.can_accept());
    p.set_name("x");
    assert!(p.can_accept());
    let mut p = Picker::new("t", Mode::SaveFile, Style::Posix).start("/home/u").file_name("x");
    assert!(!p.can_accept(), "still loading");
    p.set_name("x/y");
    assert!(p.can_accept(), "a path can go on loading");
    p.cancel();
    assert!(!p.can_accept());
}

#[test]
fn save_replaces_is_none_for_an_incomplete_listing_and_other_modes() {
    let mut p = Picker::new("t", Mode::SaveFile, Style::Posix).start("/home/u");
    let (t, _) = ticket_of(p.take().want);
    assert!(p.set_listing(t, Ok(Listing::new(vec![Entry::file("a")]).truncated(true))));
    p.set_name("zzz");
    p.accept();
    assert_eq!(p.replaces(), None);
    assert_eq!(picked(&mut p), ["/home/u/zzz"]);
    let mut p = posix(Mode::OpenFile);
    answer(&mut p, vec![Entry::file("a")]);
    p.set_name("a");
    p.accept();
    assert_eq!(p.replaces(), None, "not a save");
}

#[test]
fn save_existing_file_in_an_incomplete_listing_replaces() {
    let mut p = Picker::new("t", Mode::SaveFile, Style::Posix).start("/home/u").confirm_replace(false);
    let (t, _) = ticket_of(p.take().want);
    assert!(p.set_listing(t, Ok(Listing::new(vec![Entry::file("a")]).left_out(2))));
    p.set_name("a");
    p.accept();
    assert_eq!(p.replaces(), Some(true));
    assert_eq!(picked(&mut p), ["/home/u/a"]);
}

#[test]
fn save_open_row_on_a_special_item_refuses() {
    let mut p = saver();
    p.set_filter(2);
    let r = row_of(&p, "sock");
    p.open_row(r);
    assert_eq!(p.error(), Some(&InputError::CannotReplace { name: "sock".to_owned() }));
    assert!(!p.is_done());
}

#[test]
fn save_going_elsewhere_keeps_the_typed_name() {
    let mut p = saver();
    p.set_name("summary");
    p.go("/tmp");
    answer(&mut p, vec![]);
    assert_eq!(p.name(), "summary");
    p.up();
    answer(&mut p, vec![]);
    assert_eq!(p.name(), "summary");
    p.accept();
    assert_eq!(picked(&mut p), ["/summary.csv"]);
}
