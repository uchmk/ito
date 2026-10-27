//! `{{app}}.com`: the console-subsystem front for `{{app}}.exe` (ito's
//! `ito_console::front`, the same as every uchmk app). `{{app}}.exe` is a
//! windowed program, so PowerShell does not wait for it; this one does, and
//! passes the exit code on. A command line that answers in text runs
//! `{{app}}.exe`, waits and relays; with none (or `--keys`) it starts the
//! window and returns once it takes input.

use std::ffi::OsString;

/// The first arguments that open the window rather than print and exit.
/// Kept in step with `main` in `main.rs`.
const OPENS_WINDOW: &[&str] = &["--keys"];

fn answers_in_text(args: &[OsString]) -> bool {
    args.first().is_some_and(|a| !OPENS_WINDOW.contains(&a.to_str().unwrap_or("")))
}

fn main() {
    ito_console::front("{{app}}", answers_in_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(args: &[&str]) -> Vec<OsString> {
        args.iter().map(Into::into).collect()
    }

    #[test]
    fn it_waits_for_all_but_the_window() {
        for waits in [&["--version"][..], &["-V"], &["--help"], &["env"], &["--env"], &["--bogus"]] {
            assert!(answers_in_text(&line(waits)), "{waits:?}");
        }
        assert!(!answers_in_text(&line(&[])));
        assert!(!answers_in_text(&line(&["--keys", "<Tab>"])));
    }
}
