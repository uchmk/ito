//! Write `TESTING-CHECKS.md`: the manual half of TESTING.md, with a box to
//! tick per check. The generator is ito's `ito-testcheck` (kura, tsumugi and
//! the rest use it); this is the app's configuration of it.
//!
//!     cargo run --example make-testcheck [-- --check | --stats]
//!
//! `--check` writes nothing and exits 1 when the file is out of date (what CI
//! runs). Ticks survive regeneration, and rows a test names are left out.

use ito_testcheck::testcheck::{run, Config, Counts, Lane};

fn main() {
    run(&Config {
        source: "TESTING.md",
        notes: "scripts/testcheck-ja.toml",
        src_dirs: &["crates"],
        dep_crates: &[],
        release_list: None,
        command: "cargo run --example make-testcheck",
        lanes: &[Lane { name: "windows", out: "TESTING-CHECKS.md", marks: &['~'], setup: false, check_release: false, preamble }],
    });
}

fn preamble(c: &Counts) -> String {
    format!("# Checklist\n\n{} checks in TESTING.md, {} left for a person.\n", c.all, c.manual)
}
