//! The checklists the apps' test lanes tick, generated so they cannot drift
//! from what they list. An app keeps a small example per checklist that builds
//! a `Config` and calls `run`; everything else is here.
//!
//! - [`testcheck`]: `TESTING.md`'s tables as one tickable line per check, a
//!   file per lane (Windows, Linux), with the rows `cargo test` already covers
//!   left out and a gloss per row from an annotations file.
//! - [`keycheck`]: one tickable line per binding of a yazi-style
//!   `keymap.toml`.
//!
//! Both carry the ticks already in the file over to the new one, so
//! regenerating costs nothing already done, and both take `--check` (write
//! nothing, exit 1 when the file is out of date: what CI runs) and `--stats`
//! (the counts, which are kept out of the file so that two pull requests
//! ticking boxes do not conflict over a total).

pub mod keycheck;
pub mod testcheck;

/// What one run printed and how it ends, so a test can read it without a
/// process: `run` prints `stdout` then `stderr` and exits with `code`.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl Report {
    fn exit(self) {
        print!("{}", self.stdout);
        eprint!("{}", self.stderr);
        if self.code != 0 {
            std::process::exit(self.code);
        }
    }
}
