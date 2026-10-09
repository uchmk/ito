//! The pane's tests, as they were in filer's `src/terminal.rs`. Each module
//! sees the whole crate, private helpers included, as it did there.

#[allow(unused_imports)]
pub(crate) use std::io::{self, Write};
#[allow(unused_imports)]
pub(crate) use std::path::{Path, PathBuf};
#[allow(unused_imports)]
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
#[allow(unused_imports)]
pub(crate) use std::sync::{Arc, Mutex};
#[allow(unused_imports)]
pub(crate) use std::time::{Duration, Instant};

#[allow(unused_imports)]
pub(crate) use alacritty_terminal::grid::{Dimensions, Scroll};
#[allow(unused_imports)]
pub(crate) use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
#[allow(unused_imports)]
pub(crate) use alacritty_terminal::term::{Config, Term};
#[allow(unused_imports)]
pub(crate) use crate::{grid::*, keys::*, log::*, osc::*, shell::*, sys::*, terminal::*, testing};

mod pane {
    use super::pane_env;

    /// OSC 133's prompt marks are seen whole or cut across two reads, and
    /// nothing else counts as one.
    #[test]
    fn a_prompt_mark_is_seen_across_reads() {
        let mut tail = Vec::new();
        assert!(!super::scan_prompt_mark(&mut tail, b"PowerShell 7.6.6\r\n"));
        assert!(super::scan_prompt_mark(&mut tail, b"\x1b]133;A\x07PS C:\\> "));
        let mut tail = Vec::new();
        assert!(!super::scan_prompt_mark(&mut tail, b"banner\x1b]13"));
        assert!(super::scan_prompt_mark(&mut tail, b"3;B\x07"), "cut in the middle");
        let mut tail = Vec::new();
        assert!(!super::scan_prompt_mark(&mut tail, b"\x1b]133;D;0\x07\x1b]7;file://h/tmp\x07"), "the end of a command is not a prompt");
    }

    /// Off Windows the shell is told it is in an xterm, whatever filer was
    /// started with; on Windows nothing is added.
    #[test]
    fn the_pane_names_its_terminal_type() {
        let env = pane_env();
        match cfg!(windows) {
            true => assert!(env.is_empty(), "{env:?}"),
            false => {
                assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
                assert_eq!(env.get("COLORTERM").map(String::as_str), Some("truecolor"));
            }
        }
    }

    use super::testing::{feed, term};
    use super::*;

    /// Turning round mid-search moves at once: one `<C-S-b>` undoes one
    /// `<C-S-n>` (#93, 1.9g). Before, the first press after a change of
    /// direction found the match it was already on.
    #[test]
    fn a_search_turns_round_in_one_press() {
        let mut t = term(40, 5);
        for i in 0..30 {
            feed(&mut t, &format!("line {i:02} hit\r\n"));
        }
        let mut found = None;
        let line = |f: &Option<(Point, Point)>| f.expect("a match").0.line.0;
        // `back` walks toward the older lines, as `<C-S-f>` and `<C-S-n>` do.
        assert_eq!(search_in(&mut t, &mut found, "hit", true), Some(false));
        let first = line(&found);
        assert_eq!(search_in(&mut t, &mut found, "hit", true), Some(false));
        let second = line(&found);
        assert!(second < first, "a second press goes further back: {first} then {second}");

        assert_eq!(search_in(&mut t, &mut found, "hit", false), Some(false));
        assert_eq!(line(&found), first, "one press the other way comes straight back");
        assert_eq!(search_in(&mut t, &mut found, "hit", true), Some(false));
        assert_eq!(line(&found), second, "and one more returns to where it was");
    }

    /// Typed text is found as it reads, its regex characters plain, and
    /// in either case while it is all lower case.
    #[test]
    fn plain_text_is_found_as_typed() {
        let mut t = term(40, 5);
        feed(&mut t, "cost (a+b) = $5\r\nOther Line\r\n");
        let mut found = None;
        assert_eq!(plain_pattern("(a+b)"), "\\(a\\+b\\)");
        assert_eq!(search_in(&mut t, &mut found, &plain_pattern("(a+b) = $5"), true), Some(false));
        assert_eq!(search_in(&mut t, &mut None, &plain_pattern("other"), true), Some(false));
        assert_eq!(search_in(&mut t, &mut None, &plain_pattern("OTHER"), true), None);
    }

    /// The whole buffer comes out as the program wrote it: the scrollback
    /// first, a wrapped line whole, the blank screen below cut.
    #[test]
    fn the_whole_buffer_is_read_as_written() {
        let mut t = term(10, 4);
        for i in 0..6 {
            feed(&mut t, &format!("row {i}\r\n"));
        }
        feed(&mut t, "a line longer than ten\r\n");
        assert_eq!(all_text(&t), "row 0\nrow 1\nrow 2\nrow 3\nrow 4\nrow 5\na line longer than ten\n");
    }

    /// A search across sessions reads the whole buffer: the newest line
    /// first, the scrollback too, case aside; and showing one puts it on
    /// screen with its match selected.
    #[test]
    fn lines_are_found_in_the_scrollback_and_shown() {
        let mut t = term(40, 5);
        for i in 0..30 {
            feed(&mut t, &format!("line {i:02}{}\r\n", if i % 10 == 3 { " Needle here" } else { "" }));
        }
        let found = find_lines(&t, "needle", 10);
        assert_eq!(found.iter().map(|f| f.2.as_str()).collect::<Vec<_>>(), ["line 23 Needle here", "line 13 Needle here", "line 03 Needle here"]);
        assert_eq!(found[0].1, 8, "the cell the match starts at");
        assert!(found[2].0 < 0, "the oldest is in the scrollback");
        assert_eq!(find_lines(&t, "needle", 1).len(), 1, "at most as many as asked");
        assert!(find_lines(&t, "", 10).is_empty());
        // A character that lowers to two does not move the column.
        let mut u = term(40, 3);
        feed(&mut u, "İİ hit\r\n");
        assert_eq!(find_lines(&u, "hit", 1)[0].1, 3, "after İİ and a space");
        let (line, col, _) = found[2].clone();
        reveal(&mut t, line, col, 6);
        assert_eq!(t.grid().display_offset() as i32, -line, "scrolled to it");
        assert_eq!(t.selection_to_string().as_deref(), Some("Needle"));
    }

    /// TESTING.md 1.9i — past the last match the search starts again from the
    /// other end, and says that it did (#98).
    #[test]
    fn a_search_says_when_it_wraps() {
        let mut t = term(40, 5);
        for i in 0..30 {
            feed(&mut t, &format!("line {i:02}{}\r\n", if i % 10 == 0 { " hit" } else { "" }));
        }
        let mut found = None;
        for n in 0..3 {
            assert_eq!(search_in(&mut t, &mut found, "hit", true), Some(false), "match {n} going back");
        }
        assert_eq!(search_in(&mut t, &mut found, "hit", true), Some(true), "the fourth wraps to the newest");
        assert_eq!(search_in(&mut t, &mut found, "hit", true), Some(false), "and walks on from there");
        assert_eq!(search_in(&mut t, &mut found, "nowhere", true), None);
    }

    /// #131: `filer env` names the shell the pane really starts.
    #[test]
    fn the_default_program_is_the_login_shell_off_windows() {
        assert_eq!(default_program_from(None, false, Some("/bin/bash".into())), "/bin/bash");
        assert_eq!(default_program_from(None, false, None), "sh");
        assert_eq!(default_program_from(None, true, None), "powershell");
        assert_eq!(default_program_from(Some("pwsh".into()), true, None), "pwsh");
    }

    /// The pane's first toast names the shell, the default included.
    #[test]
    fn the_shell_is_named() {
        assert_eq!(name_shell(Some("pwsh"), true, None), "pwsh");
        // 29.8: written in the config, 5.1 still says which it is.
        for p in ["powershell", "PowerShell.exe", r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"] {
            assert_eq!(name_shell(Some(p), true, None), "powershell (Windows PowerShell 5.1)", "{p}");
        }
        assert_eq!(name_shell(Some("powershell"), false, None), "powershell", "not 5.1 off Windows");
        assert_eq!(name_shell(None, true, None), "powershell (Windows PowerShell 5.1)");
        assert_eq!(name_shell(None, false, Some("/usr/bin/zsh".into())), "zsh");
        assert_eq!(name_shell(None, false, None), "sh");
        assert_eq!(name_shell(Some("/bin/bash"), false, None), "bash");
    }

    /// Ending the pane ends the shell before `drop` returns, not whenever the
    /// reader thread gets round to it: a test that returned first left a shell
    /// and its OpenConsole behind on Windows, one pair per run (#180).
    #[test]
    fn dropping_the_pane_ends_the_shell() {
        let dir = crate::util::test_dir("term-drop");
        let t = match Terminal::spawn(&dir, Size::new(80, 24), (8, 16), None, None, || {}) {
            Ok(t) => t,
            Err(e) if cfg!(any(windows, target_os = "linux")) => panic!("the terminal did not start: {e}"),
            Err(_) => return,
        };
        let pid = t.shell_pid.expect("the PTY says which process the shell is");
        assert_eq!(t.scrollback().0, 0, "a fresh pane is at the bottom of its history");
        assert!(children(std::process::id()).contains(&pid), "the shell is running first");
        drop(t);
        assert!(!children(std::process::id()).contains(&pid), "the shell {pid} outlived its pane");
    }

    /// #265: a program that asks for the background (OSC 11) gets the pane's
    /// color, where it used to get nothing and wait.
    #[cfg(unix)]
    #[test]
    fn the_background_color_is_answered() {
        let dir = crate::util::test_dir("term-osc11");
        let out = dir.join("reply");
        let script = format!("stty raw -echo; printf '\\033]11;?\\033\\\\'; dd bs=1 count=12 2>/dev/null > '{}'", out.display());
        let shell = Some(("sh".to_owned(), vec!["-c".to_owned(), script]));
        let mut t = Terminal::spawn(&dir, Size::new(80, 24), (8, 16), shell, None, || {}).expect("the terminal starts");
        t.set_colors([0xee, 0xee, 0xee], [0x10, 0x20, 0x30]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::fs::metadata(&out).map_or(true, |m| m.len() < 12) && Instant::now() < deadline {
            t.drain();
            std::thread::sleep(Duration::from_millis(20));
        }
        let got = std::fs::read(&out).unwrap_or_default();
        assert_eq!(String::from_utf8_lossy(&got), "\x1b]11;rgb:101", "{got:?}");
    }

    /// A recording holds what the shell wrote after it began, as `.cast`
    /// lines, and a resize.
    #[cfg(unix)]
    #[test]
    fn a_pane_records_what_its_shell_writes() {
        let dir = crate::util::test_dir("term-cast");
        let cast = dir.join("a.cast");
        let shell = Some(("sh".to_owned(), vec!["-c".to_owned(), "sleep 0.3; printf 'cast-me\\n'; sleep 5".to_owned()]));
        let mut t = Terminal::spawn(&dir, Size::new(80, 24), (8, 16), shell, None, || {}).expect("the terminal starts");
        t.record(&cast).expect("the file is made");
        assert!(t.recording());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !std::fs::read_to_string(&cast).unwrap_or_default().contains("cast-me") && Instant::now() < deadline {
            t.drain();
            std::thread::sleep(Duration::from_millis(20));
        }
        t.resize(Size::new(100, 30), (8, 16));
        t.stop_recording();
        assert!(!t.recording());
        let text = std::fs::read_to_string(&cast).unwrap_or_default();
        assert!(text.starts_with("{\"version\": 2, \"width\": 80, \"height\": 24"), "{text}");
        assert!(text.contains("cast-me\\r\\n"), "{text}");
        assert!(text.trim_end().ends_with(", \"r\", \"100x30\"]"), "{text}");
    }

    /// `children` finds a process this one started: the question `<C-S-t>`
    /// asks of the shell before ending it (Q21). Asserted on the child's own
    /// pid rather than on "none before, one after", because tests running
    /// beside this one start processes of their own (git, mostly).
    #[test]
    fn a_started_process_is_found_among_the_children() {
        let mut child = if cfg!(windows) {
            std::process::Command::new("cmd").args(["/c", "ping -n 30 127.0.0.1 >nul"]).spawn()
        } else {
            std::process::Command::new("sleep").arg("30").spawn()
        }
        .expect("a child process to look for");
        let found = children(std::process::id());
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.contains(&child.id()), "{} among {found:?}", child.id());
    }

    /// The scrollback keys moved the view and the screen did not follow.
    ///
    /// `snapshot` copied `Line(0)..Line(screen_lines)`, which is the active
    /// area whatever `display_offset` says, so `<S-PageUp>` and the wheel both
    /// changed a number nothing was reading. The badge made it worse by being
    /// right: it said "16 lines back" over a screen that had not moved.
    #[test]
    fn scrolling_back_shows_the_older_lines() {
        let mut t = term(20, 4);
        for i in 0..20 {
            feed(&mut t, &format!("line{i}\r\n"));
        }
        let row = |t: &Term<Proxy>, n: usize| -> String {
            snapshot(t)[n].iter().map(|c| c.c).collect::<String>().trim_end().to_string()
        };

        // The cursor sits on a fresh line below `line19`, so the top of a
        // four-line screen is `line17`, not `line16`.
        let bottom = row(&t, 0);
        assert_eq!(bottom, "line17");

        t.scroll_display(Scroll::Delta(3));
        assert_eq!(t.grid().display_offset(), 3, "the offset is the easy half");

        let scrolled = row(&t, 0);
        assert_ne!(scrolled, bottom, "the screen has to follow the offset");
        assert_eq!(scrolled, "line14", "three older than the row that was on top");

        t.scroll_display(Scroll::Bottom);
        assert_eq!(row(&t, 0), bottom, "and come back");
    }

    /// The cursor belongs to a row, so it travels with it.
    #[test]
    fn the_cursor_leaves_with_its_row() {
        let mut t = term(20, 4);
        for i in 0..20 {
            feed(&mut t, &format!("line{i}\r\n"));
        }
        let (_, before) = cursor_cell(&t);
        assert!(before < 4, "on screen to start with: {before}");

        t.scroll_display(Scroll::Delta(3));
        let (_, after) = cursor_cell(&t);
        assert_eq!(after, before + 3, "it moves down as older lines come in above");
        assert!(after >= 4, "and is off the screen, so the pane draws nothing");
    }

    /// A drag copied the text and showed nothing, so there was no way to see
    /// what was about to be copied. The cells carry the flag now.
    #[test]
    fn a_selection_marks_the_cells_it_covers() {
        use alacritty_terminal::index::Side;
        use alacritty_terminal::selection::{Selection, SelectionType};

        let mut t = term(20, 4);
        feed(&mut t, "hello world\r\n");
        let row = 0;
        let mut sel = Selection::new(
            SelectionType::Simple,
            Point::new(Line(row), Column(0)),
            Side::Left,
        );
        sel.update(Point::new(Line(row), Column(4)), Side::Right);
        t.selection = Some(sel);

        let rows = snapshot(&t);
        let marked: String = rows[row as usize]
            .iter()
            .filter(|c| c.selected)
            .map(|c| c.c)
            .collect();
        assert_eq!(marked, "hello", "exactly the dragged cells");
        assert!(rows[1].iter().all(|c| !c.selected), "and nothing on other rows");
    }

    /// The underline styles, their colour and the strike reach the cells,
    /// so the view has something to draw them from.
    #[test]
    fn underlines_and_their_colour_reach_the_cells() {
        use alacritty_terminal::term::cell::Flags;
        use alacritty_terminal::vte::ansi::{Color, Rgb};

        let mut t = term(20, 4);
        feed(&mut t, "\x1b[4ma\x1b[4:3;58;2;1;2;3mb\x1b[0;9mc\x1b[4:4md\x1b[4:5me\x1b[4:2mf\x1b[0;8mg\x1b[0mh");
        let row = &snapshot(&t)[0];
        assert!(row[0].flags.contains(Flags::UNDERLINE) && row[0].ul.is_none(), "a plain underline in the text's colour");
        assert!(row[1].flags.contains(Flags::UNDERCURL), "4:3 is the curl");
        assert_eq!(row[1].ul, Some(Color::Spec(Rgb { r: 1, g: 2, b: 3 })), "58 gives its colour");
        assert!(row[2].flags.contains(Flags::STRIKEOUT));
        assert!(row[3].flags.contains(Flags::DOTTED_UNDERLINE));
        assert!(row[4].flags.contains(Flags::DASHED_UNDERLINE));
        assert!(row[5].flags.contains(Flags::DOUBLE_UNDERLINE));
        assert!(row[6].flags.contains(Flags::HIDDEN));
        assert!(row[7].flags.is_empty() && row[7].ul.is_none(), "SGR 0 ends them all");
    }

    /// DECSET 1004 on and off, as vim sends it on the way in and out.
    #[test]
    fn focus_reports_are_asked_for_and_given_up() {
        let mut t = term(20, 4);
        assert!(!crate::focus_report(&t), "nobody asked yet");
        feed(&mut t, "\x1b[?1004h");
        assert!(crate::focus_report(&t));
        feed(&mut t, "\x1b[?1004l");
        assert!(!crate::focus_report(&t));
        assert_eq!(crate::focus_bytes(true), b"\x1b[I");
        assert_eq!(crate::focus_bytes(false), b"\x1b[O");
    }

    /// Which match a search lands on first.
    ///
    /// Alacritty wraps, so a backwards search finds *something* from anywhere;
    /// the origin decides what. From the top line of the view it steps over
    /// every row underneath and goes into the history, landing far from where
    /// the reader is looking even when the word is on screen. From the bottom
    /// right it finds the nearest one going up, which is what a terminal's
    /// find does.
    #[test]
    fn a_backwards_search_starts_at_the_bottom_of_the_view() {
        use alacritty_terminal::term::search::RegexSearch;

        let mut t = term(20, 4);
        feed(&mut t, "target early\r\n");
        for i in 0..20 {
            feed(&mut t, &format!("filler{i}\r\n"));
        }
        feed(&mut t, "target late\r\n");

        let origin = search_origin(&t, true);
        assert_eq!(origin.line, Line(3), "the bottom row of the view, not the top");

        let mut re = RegexSearch::new("target").unwrap();
        let near = t
            .search_next(&mut re, origin, Direction::Left, Side::Left, None)
            .expect("there are two of them");
        assert_eq!(near.start().line, Line(2), "the one on screen, just above the origin");

        let from_top = Point::new(Line(0), Column(0));
        let far = t
            .search_next(&mut re, from_top, Direction::Left, Side::Left, None)
            .expect("wrapping means this finds one too");
        assert!(far.start().line < Line(0), "but the old one, up in the history");
    }

    /// Forwards still starts at the top of the view.
    #[test]
    fn a_forwards_search_starts_at_the_top_of_the_view() {
        let mut t = term(20, 4);
        for i in 0..20 {
            feed(&mut t, &format!("line{i}\r\n"));
        }
        assert_eq!(search_origin(&t, false).line, Line(0));
        t.scroll_display(Scroll::Delta(2));
        assert_eq!(t.grid().display_offset(), 2, "there is history to move into");
        assert_eq!(search_origin(&t, false).line, Line(-2), "which moves with the view");
    }

    /// Dragging the other way must select the same text.
    ///
    /// The sides used to be fixed -- `Left` to start, `Right` to extend --
    /// which is right only while the drag runs left to right. Alacritty
    /// settles a range by ordering the two anchors and then dropping the first
    /// cell if that anchor reads `Right` and the last if it reads `Left`, so a
    /// backwards drag swapped them into exactly the two cases that drop, and
    /// lost a character at each end. Reported as the first character of the
    /// line refusing to be copied.
    #[test]
    fn a_drag_selects_the_same_text_in_either_direction() {
        let selected = |t: &Term<Proxy>| -> String {
            snapshot(t)[0].iter().filter(|c| c.selected).map(|c| c.c).collect()
        };
        // The pointer is on the outer half of each end: the left half of the
        // cell the drag starts from and the right half of the one it ends on,
        // whichever way round those are.
        let mut t = term(20, 4);
        feed(&mut t, "hello world");

        select_at(&mut t, (0, 0), false, true);
        select_at(&mut t, (4, 0), true, false);
        assert_eq!(selected(&t), "hello", "left to right");

        let mut t = term(20, 4);
        feed(&mut t, "hello world");
        select_at(&mut t, (4, 0), true, true);
        select_at(&mut t, (0, 0), false, false);
        assert_eq!(selected(&t), "hello", "right to left, and the h is not dropped");
    }

    /// The half the pointer is in is what makes that work, so it has to count.
    #[test]
    fn the_half_of_the_cell_decides_what_is_included() {
        let selected = |t: &Term<Proxy>| -> String {
            snapshot(t)[0].iter().filter(|c| c.selected).map(|c| c.c).collect()
        };
        let mut t = term(20, 4);
        feed(&mut t, "hello world");

        // Starting on the *right* half of `h` leaves it out, which is what a
        // terminal does and what makes a backwards drag come out right.
        select_at(&mut t, (0, 0), true, true);
        select_at(&mut t, (4, 0), true, false);
        assert_eq!(selected(&t), "ello", "the cell the drag started past is not in it");
    }

    fn s(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).unwrap().replace('\x1b', "<ESC>")
    }

    #[test]
    fn the_plain_keys_send_what_a_terminal_expects() {
        let n = Mods::default();
        // Enter is a carriage return, not a newline, and Backspace is DEL.
        assert_eq!(encode(Special::Enter, n, false), b"\r");
        assert_eq!(encode(Special::Backspace, n, false), b"\x7f");
        assert_eq!(encode(Special::Tab, n, false), b"\t");
        assert_eq!(s(encode(Special::Escape, n, false)), "<ESC>");
        assert_eq!(s(encode(Special::Up, n, false)), "<ESC>[A");
        assert_eq!(s(encode(Special::Delete, n, false)), "<ESC>[3~");
        assert_eq!(s(encode(Special::F(1), n, false)), "<ESC>OP");
        assert_eq!(s(encode(Special::F(5), n, false)), "<ESC>[15~");
        assert_eq!(s(encode(Special::F(12), n, false)), "<ESC>[24~");
    }

    /// vim and readline put the terminal in application-cursor mode and then
    /// expect SS3 arrows; sending CSI there types letters into the buffer.
    #[test]
    fn the_arrows_follow_the_terminals_cursor_mode() {
        let n = Mods::default();
        assert_eq!(s(encode(Special::Up, n, true)), "<ESC>OA");
        assert_eq!(s(encode(Special::Down, n, true)), "<ESC>OB");
        assert_eq!(s(encode(Special::Right, n, true)), "<ESC>OC");
        assert_eq!(s(encode(Special::Left, n, true)), "<ESC>OD");
        // Only the arrows change; the rest is the same either way.
        assert_eq!(encode(Special::Enter, n, true), encode(Special::Enter, n, false));
    }

    #[test]
    fn a_modifier_turns_a_key_into_its_parameterised_form() {
        let ctrl = Mods { ctrl: true, ..Default::default() };
        let shift = Mods { shift: true, ..Default::default() };
        let both = Mods { ctrl: true, shift: true, ..Default::default() };
        // 1 + shift(1) + alt(2) + ctrl(4), which is xterm's numbering.
        assert_eq!(s(encode(Special::Right, ctrl, false)), "<ESC>[1;5C");
        assert_eq!(s(encode(Special::Right, shift, false)), "<ESC>[1;2C");
        assert_eq!(s(encode(Special::Right, both, false)), "<ESC>[1;6C");
        // Application-cursor mode gives way to the modifier form.
        assert_eq!(s(encode(Special::Up, ctrl, true)), "<ESC>[1;5A");
        // Alt on a key with no parameterised form is the escape prefix.
        let alt = Mods { alt: true, ..Default::default() };
        assert_eq!(s(encode(Special::Enter, alt, false)), "<ESC>\r");
    }

    #[test]
    fn ctrl_with_a_letter_is_the_control_code_it_stands_for() {
        assert_eq!(control_code('c', false), Some(vec![0x03]));
        assert_eq!(control_code('C', false), Some(vec![0x03]));
        assert_eq!(control_code('d', false), Some(vec![0x04]));
        assert_eq!(control_code('[', false), Some(vec![0x1b]));
        assert_eq!(control_code(' ', false), Some(vec![0x00]));
        // Alt as well: the escape prefix in front of the control code.
        assert_eq!(control_code('c', true), Some(vec![0x1b, 0x03]));
        // Nothing sensible to send, so nothing is sent.
        assert_eq!(control_code('é', false), None);
    }

    fn osc7(path: &str) -> Vec<u8> {
        format!("\x1b]7;file://host{path}\x07").into_bytes()
    }

    #[test]
    fn a_shell_saying_where_it_is_is_understood() {
        let mut partial = Vec::new();
        assert_eq!(
            scan_osc7(&mut partial, &osc7("/home/user/src")),
            vec![PathBuf::from("/home/user/src")]
        );
        // Terminated by ST rather than BEL, which is equally correct.
        let st = b"\x1b]7;file:///tmp\x1b\\";
        assert_eq!(scan_osc7(&mut partial, st), vec![PathBuf::from("/tmp")]);
        // Percent-escapes, which is how a space arrives.
        let esc = b"\x1b]7;file://h/a%20b/c\x07";
        assert_eq!(scan_osc7(&mut partial, esc), vec![PathBuf::from("/a b/c")]);
        // Ordinary output carries none, and must not be mistaken for one.
        assert!(scan_osc7(&mut partial, b"just some output\r\n").is_empty());
        // The tail of every read is kept in case a start marker was split
        // across it, but only ever those few bytes: output does not pile up.
        for _ in 0..50 {
            scan_osc7(&mut partial, &vec![b'x'; 4096]);
        }
        assert!(partial.len() < 4, "the carry stays small: {}", partial.len());
    }

    /// A read stops wherever the pipe happened to fill, which can be in the
    /// middle of the escape sequence.
    #[test]
    fn a_sequence_split_across_two_reads_is_still_one() {
        let whole = osc7("/var/log");
        for at in 1..whole.len() {
            let mut partial = Vec::new();
            let first = scan_osc7(&mut partial, &whole[..at]);
            let second = scan_osc7(&mut partial, &whole[at..]);
            let got: Vec<PathBuf> = first.into_iter().chain(second).collect();
            assert_eq!(got, vec![PathBuf::from("/var/log")], "split at {at}");
        }
    }

    #[test]
    fn two_in_one_read_are_both_seen() {
        let mut partial = Vec::new();
        let mut chunk = osc7("/one");
        chunk.extend_from_slice(b"some output\n");
        chunk.extend_from_slice(&osc7("/two"));
        assert_eq!(
            scan_osc7(&mut partial, &chunk),
            vec![PathBuf::from("/one"), PathBuf::from("/two")]
        );
    }

    /// A sequence that never terminates must not grow the buffer for ever.
    #[test]
    fn an_unterminated_sequence_is_given_up_on() {
        let mut partial = Vec::new();
        assert!(scan_osc7(&mut partial, b"\x1b]7;file://h/start").is_empty());
        assert!(!partial.is_empty(), "it is still waiting for the end");
        for _ in 0..10 {
            scan_osc7(&mut partial, &vec![b'x'; 1024]);
        }
        assert!(partial.len() <= MAX_OSC, "got {}", partial.len());
    }

    #[test]
    fn a_windows_file_url_names_a_windows_path() {
        // `file:///C:/dev/filer`: the slash before the drive letter goes.
        let got = from_file_url(b"file:///C:/dev/filer").unwrap();
        assert_eq!(got, crate::util::normalize(Path::new("C:/dev/filer")));
        // A bare root stays one.
        assert_eq!(from_file_url(b"file:///"), Some(PathBuf::from("/")));
        // Anything that is not a file URL is not a directory.
        assert_eq!(from_file_url(b"http://example.com/"), None);
        assert_eq!(from_file_url(b"nonsense"), None);
    }

    /// 29.4 / #101: a share comes back as a share, however many slashes the
    /// shell put in front of the host. Windows only: elsewhere `//x` is an
    /// ordinary path and `normalize` folds it, so both sides would agree
    /// whether the slashes were kept or not.
    #[cfg(windows)]
    #[test]
    fn a_share_keeps_its_two_leading_slashes() {
        let want = crate::util::normalize(Path::new("//localhost/C$/dev"));
        assert_eq!(from_file_url(b"file://///localhost/C$/dev"), Some(want.clone()), "PowerShell's five");
        assert_eq!(from_file_url(b"file:////localhost/C$/dev"), Some(want), "and four");
    }

    /// A paste is bracketed only when the program on the other end asked, and
    /// the markers go outside the text rather than into it.
    #[test]
    fn a_paste_is_bracketed_only_when_it_was_asked_for() {
        assert_eq!(bracket("ls -l", false), b"ls -l".to_vec());
        assert_eq!(bracket("ls -l", true), b"\x1b[200~ls -l\x1b[201~".to_vec());
        // The case the brackets exist for: without them these are two commands
        // the shell runs, with them two lines it holds.
        assert_eq!(
            bracket("one\rtwo", true),
            b"\x1b[200~one\rtwo\x1b[201~".to_vec()
        );
        // Nothing to paste stays nothing, not a pair of bare markers going to a
        // shell that would print them.
        assert_eq!(bracket("", false), Vec::<u8>::new());
        assert_eq!(bracket("", true), Vec::<u8>::new());
        // A paste that tries to end the brackets itself and run the rest:
        // its Esc goes, so the fake end is only text (the source review,
        // 2026-10-07). Tab and Enter stay; other controls and C1 go.
        let evil = crate::terminal::pasteable("echo hi\x1b[201~rm -rf ~\r\tx\u{7}\u{9b}y");
        assert_eq!(evil, "echo hi[201~rm -rf ~\r\txy");
        assert_eq!(bracket(&evil, true), [b"\x1b[200~".as_slice(), evil.as_bytes(), b"\x1b[201~"].concat(), "one end marker only");
    }

    /// `Esc` as a win32-input-mode record: the fields tcell reads it by, and
    /// **no release record after it**.
    ///
    /// The release is what lost the key. A program in VT input mode gets the
    /// press as a lone `0x1b` and waits 50ms; a release arriving inside that
    /// wait is turned by tcell into a second ESC, which makes the first one an
    /// Alt prefix, and the key never comes out. So the assertion that matters
    /// is that exactly one record is sent, and that it is a press.
    #[test]
    fn escape_goes_as_one_key_press_and_no_release() {
        let s = |b: Vec<u8>| String::from_utf8(b).unwrap();
        let esc = s(win32_key(0x1b, 1, 0x1b, Mods::default()));
        assert_eq!(esc, "\x1b[27;1;27;1;0;1_", "VK_ESCAPE, scan code 1, pressed");
        assert_eq!(esc.matches('_').count(), 1, "one record, no release after it");

        let shift = Mods { shift: true, ..Default::default() };
        let ctrl_alt = Mods { ctrl: true, alt: true, ..Default::default() };
        assert_eq!(s(win32_key(0x1b, 1, 0x1b, shift)), "\x1b[27;1;27;1;16;1_");
        assert_eq!(s(win32_key(0x1b, 1, 0x1b, ctrl_alt)), "\x1b[27;1;27;1;10;1_");
    }

    /// The PTY log spells control characters out, and cannot be misread.
    ///
    /// `\e[?9001;0$y` has to read as the reply it is, and a literal backslash
    /// followed by `e` in the shell's output must not look like an ESC.
    #[test]
    fn the_pty_log_spells_the_bytes_out() {
        assert_eq!(escape_bytes(b"\x1b[?9001;0$y"), "\\e[?9001;0$y");
        assert_eq!(escape_bytes(b"a\r\n\tb\x07\x7f"), "a\\r\\n\\tb\\x07\\x7F");
        assert_eq!(escape_bytes(br"C:\e"), r"C:\\e", "a real backslash is doubled");
        assert_eq!(escape_bytes("日本".as_bytes()), "日本", "text stays text");
    }

    /// Each chunk is one line: time, direction, bytes -- appended, so a second
    /// pane in the same run adds to the file rather than replacing it.
    #[test]
    fn the_pty_log_writes_one_line_per_chunk() {
        let dir = crate::util::test_dir("pty-log");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pty.log");
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
        let log: PtyLog = Some(Arc::new(Mutex::new(PtyLogFile { file, start: Instant::now() })));
        log_pty(&log, "out", b"\x1b[?9001h");
        log_pty(&log, "in reply", b"\x1b[?6c");
        log_pty(&None, "in key", b"ignored when the log is off");
        drop(log);

        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "one line per chunk, nothing when off: {text:?}");
        assert!(lines[0].ends_with("out       \\e[?9001h"), "{:?}", lines[0]);
        assert!(lines[1].ends_with("in reply  \\e[?6c"), "{:?}", lines[1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// OSC 9, 777 and 99 are notifications, whole or cut across reads;
    /// ConEmu's progress (`9;4;`) and other OSCs are not.
    #[test]
    fn notifications_are_read_from_the_output() {
        let mut carry = Vec::new();
        assert_eq!(scan_notices(&mut carry, b"a\x1b]9;Claude needs you\x07b"), vec!["Claude needs you"]);
        assert_eq!(scan_notices(&mut carry, b"\x1b]777;notify;Claude;Waiting for input\x1b\\"), vec!["Claude: Waiting for input"]);
        assert_eq!(scan_notices(&mut carry, b"\x1b]99;i=1:d=0;Done\x07"), vec!["Done"]);
        assert!(scan_notices(&mut carry, b"\x1b]9;4;1;50\x07").is_empty(), "a progress bar is no notification");
        assert!(scan_notices(&mut carry, b"\x1b]0;title\x07\x1b]7;file:///tmp\x07").is_empty());
        assert!(scan_notices(&mut carry, b"\x1b]9;spl").is_empty(), "not finished yet");
        assert_eq!(scan_notices(&mut carry, b"it\x07"), vec!["split"], "finished in the next read");
    }

    /// Only a console host that was not there before is a stray, whatever
    /// case Windows spells its name in.
    #[test]
    fn only_a_new_console_host_is_a_stray() {
        let now = [(1, "OpenConsole.exe".to_owned()), (2, "conhost.exe".to_owned()), (3, "git.exe".to_owned()), (4, "CONHOST.EXE".to_owned())];
        assert_eq!(strays(&[2], &now), vec![1, 4]);
        assert_eq!(strays(&[1, 2, 4], &now), Vec::<u32>::new());
    }

    /// A shell that does not exist leaves no console host behind (filer's TODO:
    /// three failed `<C-t>` left three `OpenConsole.exe`). On Windows only,
    /// where the pseudoconsole is a process of its own.
    #[cfg(windows)]
    #[test]
    fn a_shell_that_fails_leaves_no_console_behind() {
        let dir = crate::util::test_dir("term-no-shell");
        // Holding the lock keeps the other tests' panes from opening consoles
        // in between, which would read as left behind.
        let _one_at_a_time = SPAWNING.lock().unwrap_or_else(|e| e.into_inner());
        let before = crate::sys::consoles();
        let options = alacritty_terminal::tty::Options {
            shell: Some(alacritty_terminal::tty::Shell::new("no-such-shell-tsumugi.exe".to_owned(), Vec::new())),
            working_directory: Some(dir),
            drain_on_exit: false,
            env: Default::default(),
            escape_args: true,
        };
        let window = alacritty_terminal::event::WindowSize { num_lines: 24, num_cols: 80, cell_width: 8, cell_height: 16 };
        assert!(new_pty(&options, window).is_err(), "the shell does not exist");
        let after = crate::sys::consoles();
        let left: Vec<u32> = after.into_iter().filter(|p| !before.contains(p)).collect();
        assert!(left.is_empty(), "console hosts left behind: {left:?}");
    }

    /// filer #243: a key sent as a win32-input-mode record is named after it in
    /// the log, so `\e[66;48;98;1;2;1_` reads as `Alt+b` without a table.
    #[test]
    fn the_pty_log_names_the_keys_it_sends() {
        let named = |b: &[u8]| name_records(b).join(" ");
        assert_eq!(named(b"\x1b[66;48;98;1;2;1_"), "Alt+b");
        assert_eq!(named(&char_record('b', Mods { alt: true, ..Default::default() }).unwrap()), "Alt+b", "what filer sends");
        assert_eq!(named(&special_record(Special::Up, Mods::default())), "Up");
        assert_eq!(named(&special_record(Special::Tab, Mods { shift: true, ..Default::default() })), "Shift+Tab");
        assert_eq!(named(b"\x1b[67;46;3;1;8;1_"), "Ctrl+c", "the letter, not the control character");
        assert_eq!(named(b"\x1b[66;48;66;1;16;1_"), "B", "a shifted character says itself");
        assert_eq!(named(b"\x1b[27;1;27;1;0;1_\x1b[27;1;27;0;0;1_"), "Esc Esc up", "two records, a release");
        assert_eq!(named(b"\x1b[112;59;0;1;0;1_"), "F1");
        assert_eq!(named(b"ls\r"), "", "plain text names nothing");
        assert_eq!(named(b"\x1b[A"), "", "nor does a VT sequence");

        let dir = crate::util::test_dir("pty-log-keys");
        let path = dir.join("pty.log");
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
        let log: PtyLog = Some(Arc::new(Mutex::new(PtyLogFile { file, start: Instant::now() })));
        log_pty(&log, "in key", b"\x1b[66;48;98;1;2;1_");
        drop(log);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.trim_end().ends_with("\\e[66;48;98;1;2;1_  (Alt+b)"), "{text:?}");
    }

    /// A path reaches the shell as one word -- in the form *that* shell reads.
    ///
    /// The single quote is the whole point. This test used to assert the POSIX
    /// escape for every shell, which is how the bug survived: PowerShell reads
    /// `'it'\''s'` as the string `it`, a stray backslash, and an unterminated
    /// string, and sits at `>>` waiting for the rest.
    #[test]
    fn a_path_reaches_the_shell_as_one_word() {
        use Quoting::*;
        // Nothing awkward in it, so nothing is added, whatever the shell.
        for how in [PowerShell, Posix, Cmd] {
            assert_eq!(quote("/home/user/src", how), "/home/user/src", "{how:?}");
        }

        // A Windows path is bare only where `\` is just a separator. This line
        // asserted the bare form for `Posix` too, which is how Git Bash came to
        // receive `cd R:\Temp\x` and read it as `R:Tempx`: a backslash escapes
        // the character after it, so the separators ate the directory names and
        // no ordinary path could be walked into at all.
        assert_eq!(quote(r"C:\dev\filer", PowerShell), r"C:\dev\filer");
        assert_eq!(quote(r"C:\dev\filer", Cmd), r"C:\dev\filer");
        assert_eq!(quote(r"C:\dev\filer", Posix), r"'C:\dev\filer'", "quoted, so `\\` survives");
        // Forward slashes are a separator in every shell, so those stay bare --
        // the quoting is about the backslash, not about being a Windows path.
        assert_eq!(quote("C:/dev/filer", Posix), "C:/dev/filer");

        // A space would split it in two without the quotes.
        assert_eq!(quote("/a b/c", PowerShell), "'/a b/c'");
        assert_eq!(quote("/a b/c", Posix), "'/a b/c'");
        assert_eq!(quote("/a b/c", Cmd), "\"/a b/c\"");

        // The one they disagree about.
        assert_eq!(quote("it's", PowerShell), "'it''s'", "doubled, not escaped");
        assert_eq!(quote("it's", Posix), r"'it'\''s'");
        // cmd does not treat `'` as anything, so it needs no help at all.
        assert_eq!(quote("it's", Cmd), "\"it's\"");

        assert_eq!(quote("", PowerShell), "''");
        assert_eq!(quote("", Posix), "''");
        assert_eq!(quote("", Cmd), "\"\"");
    }
    /// Q29: PowerShell 7 is preferred on Windows when it is there, and
    /// nothing changes anywhere else.
    #[test]
    fn the_default_shell_is_pwsh_on_windows_when_it_is_installed() {
        assert_eq!(pick_default_shell(true, true), Some("pwsh".to_owned()));
        assert_eq!(pick_default_shell(true, false), None, "5.1, the platform default");
        assert_eq!(pick_default_shell(false, true), None, "a pwsh on Linux is not the login shell");
    }


    /// What `[term] shell` names decides the convention, and the fallback is
    /// the platform's rather than one convention for everybody.
    #[test]
    fn the_shells_name_picks_the_quoting() {
        let native = if cfg!(windows) { Quoting::PowerShell } else { Quoting::Posix };
        // `None` is what `tty::Options` starts by default.
        assert_eq!(Quoting::for_shell(None), native, "the platform default shell");

        assert_eq!(Quoting::for_shell(Some("powershell")), Quoting::PowerShell);
        assert_eq!(Quoting::for_shell(Some("pwsh")), Quoting::PowerShell);
        assert_eq!(Quoting::for_shell(Some("cmd")), Quoting::Cmd);
        assert_eq!(Quoting::for_shell(Some("bash")), Quoting::Posix);
        assert_eq!(Quoting::for_shell(Some("zsh")), Quoting::Posix);

        // A full path, and the case Windows writes it in.
        assert_eq!(Quoting::for_shell(Some(r"C:\WINDOWS\System32\cmd.exe")), Quoting::Cmd);
        assert_eq!(Quoting::for_shell(Some(r"C:\Program Files\PowerShell\7\pwsh.exe")), Quoting::PowerShell);
        assert_eq!(Quoting::for_shell(Some("/usr/bin/bash")), Quoting::Posix);

        // Something nobody listed: guess by platform rather than by habit.
        assert_eq!(Quoting::for_shell(Some("nushell")), native);
    }
}

mod readme_snippet {
    use super::scan_osc7;
    use std::path::{Path, PathBuf};

    /// The path the parser should arrive at, written the way the shell writes
    /// it — with forward slashes and no drive-letter slash.
    ///
    /// Not a Windows literal such as `r"C:\dev\filer"`: on Linux a backslash is
    /// an ordinary character, so that literal is one long component and the
    /// comparison fails on the machine the tests are usually run on. Putting
    /// the expected value through the same `normalize` the parser ends with
    /// keeps these tests about what they are about — the OSC framing and the
    /// escaping — and leaves separator spelling to `util::normalize`'s own
    /// tests.
    fn as_the_shell_names_it(path: &str) -> PathBuf {
        crate::util::normalize(Path::new(path))
    }

    /// Exactly what the PowerShell hook in the README emits, byte for byte.
    ///
    /// `[Console]::Write("$([char]27)]7;file:///$p$([char]27)\")` with `$p` a
    /// Windows path whose backslashes have been turned into forward ones. If
    /// this stops parsing, the README is telling people to paste something that
    /// does not work — and they will conclude the feature is broken rather than
    /// the instructions.
    fn as_powershell_writes_it(drive_path: &str) -> Vec<u8> {
        format!("\x1b]7;file:///{drive_path}\x1b\\").into_bytes()
    }

    #[test]
    fn the_readme_hook_is_understood() {
        let mut carry = Vec::new();
        assert_eq!(
            scan_osc7(&mut carry, &as_powershell_writes_it("C:/dev/filer")),
            vec![as_the_shell_names_it("C:/dev/filer")],
        );
    }

    /// The README says spaces and non-ASCII need no escaping. That is a promise
    /// about this parser, so it is checked here.
    #[test]
    fn spaces_and_japanese_need_no_escaping() {
        let mut carry = Vec::new();
        assert_eq!(
            scan_osc7(&mut carry, &as_powershell_writes_it("C:/my docs/報告書")),
            vec![as_the_shell_names_it("C:/my docs/報告書")],
        );
    }

    /// And escaped anyway, since the README calls that optional rather than
    /// wrong.
    #[test]
    fn percent_escaped_is_accepted_too() {
        let mut carry = Vec::new();
        assert_eq!(
            scan_osc7(&mut carry, &as_powershell_writes_it("C:/my%20docs")),
            vec![as_the_shell_names_it("C:/my docs")],
        );
    }
}

/// Q27: keys as win32-input-mode records once the other end asks for them.
mod win32_input_tests {
    use super::*;

    fn s(b: Vec<u8>) -> String {
        String::from_utf8(b).unwrap()
    }

    /// The request is seen however the reads fall, and the last one wins.
    #[test]
    fn the_request_is_seen_even_when_a_read_cuts_it() {
        let mut tail = Vec::new();
        assert_eq!(scan_win32_mode(&mut tail, b"hello \x1b[?90"), None);
        assert_eq!(scan_win32_mode(&mut tail, b"01h and more"), Some(true), "across two reads");
        assert_eq!(scan_win32_mode(&mut tail, b"\x1b[?9001l"), Some(false));
        assert_eq!(scan_win32_mode(&mut tail, b"\x1b[?9001h\x1b[?9001l"), Some(false), "the last one");
        assert_eq!(scan_win32_mode(&mut tail, b"\x1b[?1049h"), None, "another mode is not this one");
        assert!(tail.len() < 10, "and the carry stays short: {}", tail.len());
    }

    /// `\e[?9001$p` was answered "not recognised"; now it is answered as it is.
    #[test]
    fn the_query_is_answered_with_the_state() {
        assert_eq!(answer_win32_query("\x1b[?9001;0$y".into(), true), "\x1b[?9001;1$y");
        assert_eq!(answer_win32_query("\x1b[?9001;0$y".into(), false), "\x1b[?9001;2$y");
        assert_eq!(answer_win32_query("\x1b[?1049;1$y".into(), true), "\x1b[?1049;1$y", "others untouched");
    }

    /// `<S-End>` -- the key that took lazygit's `<Esc>` with it -- is one
    /// record with no ESC in front of its own, and so is every special key.
    #[test]
    fn special_keys_are_records() {
        let shift = Mods { shift: true, ..Default::default() };
        assert_eq!(s(special_record(Special::End, shift)), "\x1b[35;79;0;1;272;1_");
        assert_eq!(s(special_record(Special::Escape, Mods::default())), "\x1b[27;1;27;1;0;1_");
        assert_eq!(s(special_record(Special::Enter, Mods::default())), "\x1b[13;28;13;1;0;1_");
        assert_eq!(s(special_record(Special::BackTab, Mods::default())), "\x1b[9;15;9;1;16;1_");
        assert_eq!(s(special_record(Special::F(5), Mods::default())), "\x1b[116;63;0;1;0;1_");
        assert_eq!(s(special_record(Special::F(12), Mods::default())), "\x1b[123;88;0;1;0;1_");
    }

    /// Ctrl carries the control code, Alt the character, Alt+Shift the capital.
    #[test]
    fn chords_are_records() {
        let ctrl = Mods { ctrl: true, ..Default::default() };
        let alt = Mods { alt: true, ..Default::default() };
        let alt_shift = Mods { alt: true, shift: true, ..Default::default() };
        assert_eq!(s(char_record('c', ctrl).unwrap()), "\x1b[67;46;3;1;8;1_");
        assert_eq!(s(char_record('b', alt).unwrap()), "\x1b[66;48;98;1;2;1_");
        assert_eq!(s(char_record('b', alt_shift).unwrap()), "\x1b[66;48;66;1;18;1_");
        assert_eq!(s(char_record('1', alt).unwrap()), "\x1b[49;2;49;1;2;1_");
        assert_eq!(char_record('[', ctrl), None, "no record form: the caller keeps the old bytes");
        assert_eq!(char_record('a', Mods::default()), None, "a plain letter is text");
    }
}

/// The alternate screen, and the meta prefix — the two things the pane needs
/// in order to know when a key is its own and when it belongs to a program.
mod alt_screen_tests {
    use super::testing::{feed, term};
    use super::*;

    /// `nvim` and `less` switch with DECSET 1049 and back with DECRST.
    #[test]
    fn the_flag_follows_the_escape_sequence() {
        let mut t = term(20, 5);
        assert!(!alt_screen(&t), "a fresh terminal is on the primary screen");

        feed(&mut t, "\x1b[?1049h");
        assert!(alt_screen(&t), "a full-screen program has taken the screen");

        feed(&mut t, "\x1b[?1049l");
        assert!(!alt_screen(&t), "and given it back on the way out");
    }

    /// Q28: what nvim sends at startup turns the wheel into mouse reports, in
    /// the SGR form it asked for; turning the mouse off again turns them off.
    #[test]
    fn a_program_that_asks_for_the_mouse_is_told_about_the_wheel() {
        let mut t = term(80, 24);
        assert_eq!(mouse_report(&t), MouseReport::Off);

        feed(&mut t, "\x1b[?1049h\x1b[?1002h\x1b[?1006h");
        assert_eq!(mouse_report(&t), MouseReport::Sgr);
        assert_eq!(wheel_report(true, 9, 4, MouseReport::Sgr), b"\x1b[<64;10;5M");
        assert_eq!(wheel_report(false, 0, 0, MouseReport::Sgr), b"\x1b[<65;1;1M");

        feed(&mut t, "\x1b[?1006l");
        assert_eq!(mouse_report(&t), MouseReport::Plain, "still reporting, the old way");
        assert_eq!(wheel_report(true, 9, 4, MouseReport::Plain), [0x1b, b'[', b'M', 96, 42, 37]);
        assert_eq!(wheel_report(false, 500, 0, MouseReport::Plain)[4], 255, "a far column is capped");

        feed(&mut t, "\x1b[?1002l");
        assert_eq!(mouse_report(&t), MouseReport::Off, "and `less` never asks");
        assert!(wheel_report(true, 0, 0, MouseReport::Off).is_empty());
    }

    /// The reason the flag is the right question: there is nothing to scroll
    /// to up there, so a scrolling key spent on the alternate screen is spent
    /// on nothing.
    #[test]
    fn the_alternate_screen_has_no_scrollback() {
        let mut t = term(20, 3);
        for i in 0..40 {
            feed(&mut t, &format!("line {i}\r\n"));
        }
        t.scroll_display(alacritty_terminal::grid::Scroll::Top);
        assert!(t.grid().display_offset() > 0, "the primary screen scrolls back");

        feed(&mut t, "\x1b[?1049h");
        t.scroll_display(alacritty_terminal::grid::Scroll::Top);
        assert_eq!(t.grid().display_offset(), 0, "the alternate screen does not");
    }

    /// `Alt`+letter is `ESC` then the letter, the same prefix `control_code`
    /// puts in front of a control chord.
    #[test]
    fn meta_is_escape_then_the_character() {
        assert_eq!(meta_char('b'), vec![0x1b, b'b']);
        assert_eq!(meta_char('f'), vec![0x1b, b'f']);
        assert_eq!(meta_char('J'), vec![0x1b, b'J']);
        // Whatever the character costs in UTF-8, the prefix is one byte.
        assert_eq!(meta_char('あ'), [vec![0x1b], "あ".as_bytes().to_vec()].concat());
        // The same shape `control_code` produces for Alt with Ctrl.
        assert_eq!(control_code('b', true).unwrap()[0], 0x1b);
    }
}

/// What runs under a pane, and what it listens on.
mod processes {
    use crate::sys::{descendants, listening_ports, stem, Proc};

    fn p(pid: u32, ppid: u32, name: &str) -> Proc {
        Proc { pid, ppid, name: name.into(), args: Vec::new() }
    }

    #[test]
    fn the_tree_under_a_shell_is_found() {
        let table = vec![p(1, 0, "init"), p(10, 1, "bash"), p(11, 10, "node"), p(12, 11, "codex-x86_64"), p(13, 1, "other"), p(14, 12, "git")];
        let mut under: Vec<u32> = descendants(&table, 10).iter().map(|p| p.pid).collect();
        under.sort_unstable();
        assert_eq!(under, [11, 12, 14]);
        assert!(descendants(&table, 99).is_empty());
        assert_eq!((stem("/usr/local/bin/claude"), stem(r"C:\Tools\Codex.EXE"), stem("gemini")), ("claude".into(), "codex".into(), "gemini".into()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn listening_lines_are_read_by_their_sockets() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4242 1\n   1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 99 1\n   2: 0100007F:0050 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 4243 1\n";
        let inodes: std::collections::HashSet<u64> = [4242, 4243].into_iter().collect();
        assert_eq!(crate::sys::listening_in(table, &inodes), [3000], "a listener of ours; not another's, not a connection");
    }

    /// This very process listening is found, on the systems CI runs.
    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn a_port_listened_on_is_found() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(listening_ports(&[std::process::id()]).contains(&port), "port {port}");
        assert!(listening_ports(&[]).is_empty());
    }

    /// A shell named in Japanese is no panic (four bytes from the end fell
    /// inside a character; the source review, 2026-10-07).
    #[test]
    fn a_shell_named_in_japanese_is_read() {
        assert!(!crate::shell::is_powershell_51("C:\\tools\\シェル"));
        assert!(crate::shell::is_powershell_51("C:\\Windows\\powershell.exe"));
    }
}

mod prompt_marks {
    use super::*;

    /// What a shell writes through the links, fed as `reads` cuts it.
    fn through(reads: &[&[u8]]) -> Vec<u8> {
        let mut marks = PromptLinks::default();
        reads.iter().flat_map(|r| marks.feed(r).unwrap_or_else(|| r.to_vec())).collect()
    }

    /// A link opens after `A` and closes at `B`, or at the first newline
    /// when no `B` comes; other output passes as it was.
    #[test]
    fn a_prompt_is_linked_up_to_its_input() {
        let mut marks = PromptLinks::default();
        assert_eq!(marks.feed(b"plain output\r\n\x1b]7;file://h/tmp\x07"), None);
        let out = through(&[b"\x1b]133;A\x07PS> \x1b]133;B\x07ls\r\n"]);
        let want = [b"\x1b]133;A\x07".as_slice(), b"\x1b]8;id=tsumugi-mark;tsumugi:prompt\x1b\\", b"PS> \x1b]133;B\x07", b"\x1b]8;;\x1b\\", b"ls\r\n"].concat();
        assert_eq!(out, want);
        let out = through(&[b"\x1b]133;A\x1b\\$ \r", b"\nnext"]);
        let want = [b"\x1b]133;A\x1b\\".as_slice(), b"\x1b]8;id=tsumugi-mark;tsumugi:prompt\x1b\\", b"$ \r", b"\x1b]8;;\x1b\\", b"\nnext"].concat();
        assert_eq!(out, want);
    }

    /// Cut anywhere, the same bytes come out.
    #[test]
    fn a_mark_cut_across_reads_is_still_linked() {
        let whole: &[u8] = b"out\r\n\x1b]133;A\x07user@host$ \x1b]133;B\x07";
        let once = through(&[whole]);
        for at in 1..whole.len() {
            assert_eq!(through(&[&whole[..at], &whole[at..]]), once, "cut at {at}");
        }
    }

    /// `D;<code>` goes on the next prompt's link, and `C` opens a link on
    /// the first row of the output.
    #[test]
    fn the_exit_code_and_the_output_are_marked() {
        let out = through(&[b"\x1b]133;C\x07hi\r\n\x1b]133;D;2\x07\x1b]133;A\x07$ "]);
        let want = [
            b"\x1b]133;C\x07".as_slice(),
            b"\x1b]8;id=tsumugi-mark;tsumugi:output\x1b\\",
            b"hi\r",
            b"\x1b]8;;\x1b\\",
            b"\n\x1b]133;D;2\x07\x1b]133;A\x07",
            b"\x1b]8;id=tsumugi-mark;tsumugi:prompt?exit=2\x1b\\",
            b"$ ",
        ]
        .concat();
        assert_eq!(out, want);
        // A `D` with no code, or one with more after it.
        assert!(through(&[b"\x1b]133;D\x07\x1b]133;A\x07"]).ends_with(b"tsumugi:prompt\x1b\\"));
        assert!(through(&[b"\x1b]133;D;0;aid=1\x07\x1b]133;A\x07"]).ends_with(b"tsumugi:prompt?exit=0\x1b\\"));
        assert_eq!(crate::osc::exit_of("tsumugi:prompt?exit=-1"), Some(-1));
        assert_eq!(crate::osc::exit_of("tsumugi:prompt"), None);
    }

    /// Each command between two prompts is a block with its exit code; the
    /// one still running is not; the last output is copied whole.
    #[test]
    fn the_commands_are_blocks_and_the_last_output_is_copied() {
        let mut t = testing::term(20, 12);
        let mut marks = PromptLinks::default();
        let mut text = Vec::new();
        for (n, code) in [(0, 0), (1, 1)] {
            text.extend_from_slice(format!("\x1b]133;A\x07${n} \x1b]133;B\x07cmd\r\n\x1b]133;C\x07").as_bytes());
            for k in 0..2 {
                text.extend_from_slice(format!("out {n}.{k}\r\n").as_bytes());
            }
            text.extend_from_slice(format!("\x1b]133;D;{code}\x07").as_bytes());
        }
        text.extend_from_slice(b"\x1b]133;A\x07$2 \x1b]133;B\x07sleep\r\n\x1b]133;C\x07");
        let fed = marks.feed(&text).unwrap();
        testing::feed(&mut t, std::str::from_utf8(&fed).unwrap());
        assert_eq!(blocks(&t), [Block { lines: 0..3, exit: 0 }, Block { lines: 3..6, exit: 1 }]);
        // Nothing yet from the running one: the output of the one before.
        assert_eq!(last_output(&t).as_deref(), Some("out 1.0\nout 1.1"));
        testing::feed(&mut t, "zzz\r\n");
        assert_eq!(last_output(&t).as_deref(), Some("zzz"));
        let fed = marks.feed(b"\x1b]133;D;0\x07\x1b]133;A\x07$3 ").unwrap();
        testing::feed(&mut t, std::str::from_utf8(&fed).unwrap());
        assert_eq!(blocks(&t).last(), Some(&Block { lines: 6..8, exit: 0 }));
        // The marks are no program's links.
        assert!(hyperlinks(&t).is_empty());
        assert!(all_text(&t).contains("out 1.1"));
    }

    /// A shell that marks no output (pwsh): the rows after the prompt, and
    /// an empty Enter's prompt is passed over.
    #[test]
    fn without_c_the_output_is_the_rows_after_the_prompt() {
        let mut t = testing::term(20, 8);
        let mut marks = PromptLinks::default();
        let text = b"\x1b]133;D;0\x07\x1b]133;A\x07PS> dir\r\na\r\nb\r\n\x1b]133;D;1\x07\x1b]133;A\x07PS> \r\n\x1b]133;D;1\x07\x1b]133;A\x07PS> ";
        let fed = marks.feed(text).unwrap();
        testing::feed(&mut t, std::str::from_utf8(&fed).unwrap());
        assert_eq!(last_output(&t).as_deref(), Some("a\nb"));
        assert_eq!(blocks(&t), [Block { lines: 0..3, exit: 1 }, Block { lines: 3..4, exit: 1 }]);
    }

    /// The prompts are found in the grid, and the keys go from one to the
    /// one before and back down to the bottom.
    #[test]
    fn the_view_jumps_from_prompt_to_prompt() {
        let mut t = testing::term(20, 4);
        let mut marks = PromptLinks::default();
        let mut text = Vec::new();
        for n in 0..3 {
            text.extend_from_slice(format!("\x1b]133;A\x07${n} \x1b]133;B\x07cmd\r\n").as_bytes());
            for k in 0..5 {
                text.extend_from_slice(format!("out {n}.{k}\r\n").as_bytes());
            }
        }
        text.extend_from_slice(b"\x1b]133;A\x07$3 ");
        let fed = marks.feed(&text).unwrap();
        testing::feed(&mut t, std::str::from_utf8(&fed).unwrap());
        let lines = prompt_lines(&t);
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(lines.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>(), [6, 6, 6]);
        let top = |t: &Term<Proxy>| -(t.grid().display_offset() as i32);
        assert!(jump_prompt(&mut t, true));
        assert_eq!(top(&t), lines[2]);
        assert!(jump_prompt(&mut t, true));
        assert_eq!(top(&t), lines[1]);
        assert!(jump_prompt(&mut t, false));
        assert_eq!(top(&t), lines[2]);
        assert!(jump_prompt(&mut t, false), "the last prompt is on the screen: to the bottom");
        assert_eq!(t.grid().display_offset(), 0);
        assert!(!jump_prompt(&mut t, false));
        // The row reads as it was written: the link draws nothing.
        assert!(all_text(&t).contains("$0 cmd"));
    }

    /// A program's OSC 8 links are found where they are on the view, piece
    /// by piece; the prompt's mark is not one of them.
    #[test]
    fn the_links_a_program_wrote_are_found_on_the_view() {
        let mut t = testing::term(20, 4);
        let mut marks = PromptLinks::default();
        let text = b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\nsee \x1b]8;;https://a.example/\x1b\\here\x1b]8;;\x1b\\ ok\r\n\x1b]8;id=f;file:///tmp/x\x07x\x1b]8;;\x07";
        let fed = marks.feed(text).unwrap();
        testing::feed(&mut t, std::str::from_utf8(&fed).unwrap());
        let found = hyperlinks(&t);
        assert_eq!(found, [Hyperlink { line: 1, cells: 4..8, uri: "https://a.example/".into() }, Hyperlink { line: 2, cells: 0..1, uri: "file:///tmp/x".into() }]);
        // Scrolled back a line, the same links a line lower.
        testing::feed(&mut t, "\r\n\r\n");
        let up = hyperlinks(&t);
        assert_eq!(up.len(), 2);
        t.scroll_display(alacritty_terminal::grid::Scroll::Delta(1));
        let back = hyperlinks(&t);
        assert_eq!(back.iter().map(|h| h.line).collect::<Vec<_>>(), up.iter().map(|h| h.line + 1).filter(|l| *l < 4).collect::<Vec<_>>());
    }
}

mod pictures {
    use super::*;
    use crate::image::{base64, sixel, Catcher, Pictures};

    fn catcher(cols: usize) -> (Catcher, Pictures) {
        let store: Pictures = Default::default();
        {
            let mut s = store.lock().unwrap();
            s.cell = (10, 20);
            s.cols = cols;
        }
        (Catcher::new(store.clone()), store)
    }

    fn b64(bytes: &[u8]) -> String {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in bytes.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..=c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            }
            for _ in c.len()..3 {
                out.push('=');
            }
        }
        out
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 128, 255, 255]));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    /// The key the cells carry, from what the catcher put in.
    fn key_of(out: &[u8]) -> u64 {
        let s = String::from_utf8_lossy(out);
        let at = s.find(crate::IMAGE_LINK).expect("a picture's cell") + crate::IMAGE_LINK.len();
        s[at..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap()
    }

    #[test]
    fn base64_with_or_without_padding_and_breaks() {
        assert_eq!(base64(b"aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64(b"aGVs\nbG8").unwrap(), b"hello");
        assert_eq!(base64(b"").unwrap(), b"");
        assert!(base64(b"a*b").is_none());
        assert_eq!(base64(b64(&[0, 255, 7, 9]).as_bytes()).unwrap(), [0, 255, 7, 9]);
    }

    /// Colors by RGB percent and by DEC's HLS, repeats, and the next band.
    #[test]
    fn a_sixel_is_painted() {
        let p = sixel(b"0;1;0\"1;1;3;12#1;2;100;0;0#1!3~-#2;1;120;50;100!2~").unwrap();
        assert_eq!((p.width, p.height), (3, 12));
        let px = |x: u32, y: u32| p.rgba[((y * p.width + x) * 4) as usize..][..4].to_vec();
        assert_eq!(px(2, 5), [255, 0, 0, 255]);
        // HLS 120 is red, in DEC's.
        assert_eq!(px(1, 6), [255, 0, 0, 255]);
        // Not set, so clear.
        assert_eq!(px(2, 6), [0, 0, 0, 0]);
        assert!(sixel(b"#1;2;0;0;0").is_none());
    }

    /// Sixel: the sequence is taken out, and in its place one cell with the
    /// picture's link and the cursor below it, the text around untouched.
    #[test]
    fn a_sixel_becomes_a_cell_with_a_link() {
        let (mut c, store) = catcher(80);
        let out = c.feed(b"a\x1bPq#0;2;0;100;0!20~-!20~\x1b\\b").unwrap();
        let key = key_of(&out);
        let shown = store.lock().unwrap().get(key).unwrap();
        // 20 x 12 pixels in cells of 10 x 20.
        assert_eq!((shown.cols, shown.rows), (2, 1));
        let s = String::from_utf8_lossy(&out);
        assert!(s.starts_with("a\x1b]8;"), "{s:?}");
        assert!(s.ends_with("\x08\nb"), "{s:?}");
        assert!(!s.contains("\x1bP"));
    }

    /// What is not a picture goes on as it was, even cut between reads.
    #[test]
    fn other_sequences_pass_through_split_anywhere() {
        let text: &[u8] = b"\x1b[31mred\x1bP$qm\x1b\\\x1b]0;title\x07\x1b]1337;SetUserVar=a=b\x07\x1b_Xother\x1b\\end\x1b\x1b[0m";
        for cut in 0..text.len() {
            let (mut c, _) = catcher(80);
            let mut out = Vec::new();
            for part in [&text[..cut], &text[cut..]] {
                match c.feed(part) {
                    Some(o) => out.extend(o),
                    None => out.extend_from_slice(part),
                }
            }
            assert_eq!(out, text, "cut at {cut}");
        }
    }

    /// Kitty: a picture sent and placed at once, answered unless quiet; one
    /// sent in chunks; one sent, then placed by its id; and deleted.
    #[test]
    fn kitty_pictures_are_answered_and_placed() {
        let (mut c, store) = catcher(80);
        let rgb = b64(&[255, 0, 0, 0, 255, 0]);
        let out = c.feed(format!("\x1b_Ga=T,f=24,s=2,v=1,i=5;{rgb}\x1b\\").as_bytes()).unwrap();
        assert_eq!(c.replies, [b"\x1b_Gi=5;OK\x1b\\".to_vec()]);
        let shown = store.lock().unwrap().get(key_of(&out)).unwrap();
        assert_eq!((shown.picture.width, shown.picture.rgba[4..8].to_vec()), (2, vec![0, 255, 0, 255]));
        c.replies.clear();
        // Quiet, in two chunks, of a PNG, in a size asked for.
        let data = b64(&png(40, 40));
        let (a, b) = data.split_at(data.len() / 2);
        assert!(c.feed(format!("\x1b_Ga=T,f=100,q=1,c=4,r=2,m=1;{a}\x1b\\").as_bytes()).unwrap().is_empty());
        let out = c.feed(format!("\x1b_Gm=0;{b}\x1b\\").as_bytes()).unwrap();
        assert!(c.replies.is_empty());
        let shown = store.lock().unwrap().get(key_of(&out)).unwrap();
        assert_eq!((shown.cols, shown.rows), (4, 2));
        // Beside: down a row and on past it.
        assert!(String::from_utf8_lossy(&out).ends_with("\n\x1b[3C"));
        // Sent, then placed; a place of nothing is an error.
        assert!(c.feed(format!("\x1b_Gf=24,s=2,v=1,i=9;{rgb}\x1b\\").as_bytes()).unwrap().is_empty());
        let out = c.feed(b"\x1b_Ga=p,i=9,p=2,C=1\x1b\\").unwrap();
        assert!(String::from_utf8_lossy(&out).ends_with("\x1b]8;;\x1b\\\x08"));
        // The answer says which placement.
        assert_eq!(c.replies.last().unwrap(), b"\x1b_Gi=9,p=2;OK\x1b\\".as_slice());
        c.feed(b"\x1b_Ga=p,i=77\x1b\\");
        assert!(String::from_utf8_lossy(c.replies.last().unwrap()).contains("ENOENT"));
        // Deleting image 9's placements leaves image 5's.
        let nine = key_of(&out);
        c.feed(b"\x1b_Ga=d,d=i,i=9\x1b\\");
        assert!(store.lock().unwrap().get(nine).is_none());
        assert!(store.lock().unwrap().tallest() > 0);
        c.feed(b"\x1b_Ga=d\x1b\\");
        assert_eq!(store.lock().unwrap().tallest(), 0);
    }

    /// iTerm2: a file shown inline, sized in cells; one not inline is not
    /// shown, and the multipart form.
    #[test]
    fn iterm_files_are_shown_inline() {
        let (mut c, store) = catcher(80);
        let data = b64(&png(30, 30));
        let out = c.feed(format!("\x1b]1337;File=name=eA==;inline=1;width=6;height=3:{data}\x07").as_bytes()).unwrap();
        let shown = store.lock().unwrap().get(key_of(&out)).unwrap();
        // Shape kept: 6 x 3 cells is 60 x 60 pixels.
        assert_eq!((shown.cols, shown.rows), (6, 3));
        let out = c.feed(format!("\x1b]1337;File=inline=0:{data}\x07").as_bytes()).unwrap();
        assert!(out.is_empty());
        assert!(c.feed(b"\x1b]1337;MultipartFile=inline=1\x07").unwrap().is_empty());
        let (a, b) = data.split_at(8);
        c.feed(format!("\x1b]1337;FilePart={a}\x07").as_bytes());
        c.feed(format!("\x1b]1337;FilePart={b}\x07").as_bytes());
        let out = c.feed(b"\x1b]1337;FileEnd\x1b\\").unwrap();
        let shown = store.lock().unwrap().get(key_of(&out)).unwrap();
        // 30 x 30 pixels in cells of 10 x 20.
        assert_eq!((shown.cols, shown.rows), (3, 2));
    }

    /// A picture wider than the pane is made to fit it.
    #[test]
    fn a_wide_picture_fits_the_pane() {
        let (mut c, store) = catcher(10);
        let out = c.feed(format!("\x1b_Ga=T,f=100;{}\x1b\\", b64(&png(400, 100))).as_bytes()).unwrap();
        let shown = store.lock().unwrap().get(key_of(&out)).unwrap();
        assert_eq!((shown.cols, shown.rows), (10, 2));
        assert!(shown.picture.width <= 200);
    }

    /// Fed a byte at a time, the same picture comes out.
    #[test]
    fn a_picture_cut_into_bytes_comes_out_whole() {
        let (mut c, store) = catcher(80);
        let text = format!("x\x1b_Ga=T,f=24,s=1,v=1;{}\x1b\\y", b64(&[1, 2, 3]));
        let mut out = Vec::new();
        for b in text.as_bytes() {
            match c.feed(std::slice::from_ref(b)) {
                Some(o) => out.extend(o),
                None => out.push(*b),
            }
        }
        let shown = store.lock().unwrap().get(key_of(&out)).unwrap();
        assert_eq!(shown.picture.rgba, [1, 2, 3, 255]);
        assert!(out.starts_with(b"x\x1b]8;") && out.ends_with(b"y"));
    }

    /// In the grid, the cell is found where the picture went, and scrolls.
    #[test]
    fn the_cell_is_found_in_the_grid() {
        let (mut c, _) = catcher(20);
        let mut t = testing::term(20, 4);
        let out = c.feed(format!("ab\x1b_Ga=T,f=24,s=1,v=1,c=2,r=2;{}\x1b\\z", b64(&[1, 2, 3])).as_bytes()).unwrap();
        let key = key_of(&out);
        testing::feed(&mut t, std::str::from_utf8(&out).unwrap());
        assert_eq!(placements(&t, 0), [(key, 0, 2)]);
        // The picture's link is no program's.
        assert!(hyperlinks(&t).is_empty());
        // Up off the top: found only when looked for above.
        testing::feed(&mut t, "\r\n\r\n\r\n\r\n");
        assert!(placements(&t, 0).is_empty());
        assert_eq!(placements(&t, 3), [(key, -2, 2)]);
    }
}

/// What the pane says when a program asks what terminal it is talking to:
/// the questions neovim, tmux, notcurses and yazi send before they decide
/// what to draw.
mod identity_tests {
    use std::sync::Arc;

    use alacritty_terminal::event::Event;
    use alacritty_terminal::term::{Config, Term};
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    use crate::osc::{answer_query, scan_xtversion, xtversion};
    use crate::{Proxy, Size};

    /// The replies the parser wrote back, as the pane would send them, for
    /// `text` fed in one go (so a synchronized update can stay open).
    fn replies(text: &str) -> (Term<Proxy>, Vec<String>) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut t = Term::new(Config { kitty_keyboard: true, ..Default::default() }, &Size::new(20, 4), Proxy { tx, wake: Arc::new(|| {}) });
        let mut parser = Processor::<StdSyncHandler>::default();
        parser.advance(&mut t, text.as_bytes());
        let said = rx.try_iter().filter_map(|e| if let Event::PtyWrite(s) = e { Some(answer_query(s, false)) } else { None }).collect();
        (t, said)
    }

    #[test]
    fn device_attributes_say_vt220_with_sixel() {
        assert_eq!(replies("\x1b[c").1, ["\x1b[?62;4;22c"]);
        let (_, da2) = replies("\x1b[>c");
        assert!(da2[0].starts_with("\x1b[>0;") && da2[0].ends_with(";1c"), "{da2:?}");
    }

    /// kitty's keyboard flags: pushed, asked after, and popped again (the
    /// pane turns the protocol on; `kitty.rs` spells the keys).
    #[test]
    fn kitty_keyboard_flags_are_kept_and_answered() {
        let (t, said) = replies("\x1b[>1u\x1b[?u");
        assert_eq!(crate::kitty_flags(&t), crate::kitty::DISAMBIGUATE);
        assert_eq!(said, ["\x1b[?1u"]);
        let (t, _) = replies("\x1b[>1u\x1b[>31u");
        assert_eq!(crate::kitty_flags(&t), 31);
        let (t, said) = replies("\x1b[>1u\x1b[>31u\x1b[<u\x1b[?u");
        assert_eq!(crate::kitty_flags(&t), 1, "popped back to the first");
        assert_eq!(said, ["\x1b[?1u"]);
        let (t, _) = replies("\x1b[>1u\x1b[<u");
        assert_eq!(crate::kitty_flags(&t), 0);
    }

    #[test]
    fn xtversion_is_found_and_answered() {
        let mut tail = Vec::new();
        assert_eq!(scan_xtversion(&mut tail, b"\x1b[>q"), 1);
        assert_eq!(scan_xtversion(&mut tail, b"a\x1b[>"), 0);
        assert_eq!(scan_xtversion(&mut tail, b"0q\x1b[c"), 1, "across two reads");
        assert_eq!(scan_xtversion(&mut tail, b"\x1b[>4;2m\x1b[>1q"), 0, "other CSI > are not this");
        assert!(tail.len() < 5);
        let reply = String::from_utf8(xtversion()).unwrap();
        assert_eq!(reply, format!("\x1bP>|tsumugi {}\x1b\\", env!("CARGO_PKG_VERSION")));
    }

    /// DECRQM: set (1), reset (2), and 0 only for what is not known at all.
    #[test]
    fn modes_are_reported_as_they_are() {
        let (_, said) = replies("\x1b[?1004h\x1b[?1004$p\x1b[?2004$p\x1b[?2026$p\x1b[?1049$p\x1b[?7777$p");
        assert_eq!(said, ["\x1b[?1004;1$y", "\x1b[?2004;2$y", "\x1b[?2026;2$y", "\x1b[?1049;2$y", "\x1b[?7777;0$y"]);
    }

    /// DECSET 2026: what comes between the start and the end of an update is
    /// held by the parser and lands at once, so a program redrawing the whole
    /// screen is never seen half done.
    #[test]
    fn a_synchronized_update_lands_whole() {
        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut t = Term::new(Config::default(), &Size::new(20, 4), Proxy { tx, wake: Arc::new(|| {}) });
        let mut parser = Processor::<StdSyncHandler>::default();
        let first = |t: &Term<Proxy>| crate::snapshot(t)[0].iter().map(|c| c.c).collect::<String>().trim_end().to_owned();
        parser.advance(&mut t, b"\x1b[?2026hhalf");
        assert_eq!(first(&t), "", "held while the update is open");
        parser.advance(&mut t, b" done\x1b[?2026l");
        assert_eq!(first(&t), "half done");
    }
}
