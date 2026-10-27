//! `{{app}} env`: what a bug report would otherwise have to ask for (the same
//! report every uchmk app prints). Printed, not opened in a browser, and it
//! never opens a window. The release workflow runs `{{app}}.com env` and wants
//! ten lines or more.

use ito_common::report::{files_rows, finish, row, section, variables, version_rows, Rows};

/// The whole report.
pub fn text() -> String {
    let mut out = String::new();
    section(&mut out, "{{app}}", &version_rows(env!("CARGO_PKG_VERSION"), &ito_bugreport::os_line()));
    section(&mut out, "Files", &files_rows("{{app}}"));
    section(&mut out, "Last run", &last_run());
    section(&mut out, "Variables", &variables(&["UCHMK_CONFIG_DIR", "WGPU_BACKEND", "LANG"], "(not set)"));
    finish(&mut out);
    out
}

fn last_run() -> Rows {
    let Some(run) = ito_common::runinfo::load("{{app}}") else {
        return vec![row("Last run", "no record ({{app}} has not opened a window here)")];
    };
    let mut rows = vec![row("Version", run.version.clone())];
    if let Some([adapter, backend, device]) = run.rendering() {
        rows.push(row("Adapter", adapter));
        rows.push(row("Backend", backend));
        rows.push(row("Device", device));
    }
    if let Some(line) = run.window_line() {
        rows.push(row("Window", line));
    }
    rows
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_report_is_long_enough() {
        let t = super::text();
        assert!(t.lines().count() >= 10, "{t}");
        assert!(t.contains("Version") && t.ends_with('\n') && !t.ends_with("\n\n"));
    }
}
