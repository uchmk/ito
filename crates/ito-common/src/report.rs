//! The text of an app's `env` command (`kura env`, `yagura env`): sections of
//! `key : value` rows for a bug report to paste, laid out the same way in
//! every app. What each app has to say stays its own; this is how it is set
//! down.

/// A section's rows, key and value.
pub type Rows = Vec<(String, String)>;

/// A key longer than this goes on a line of its own with the value under it,
/// so one long row (a config directory, as long as the path) does not push
/// every other row past a console's width.
const MAX_KEY: usize = 24;

/// A row.
pub fn row(k: &str, v: impl Into<String>) -> (String, String) {
    (k.to_string(), v.into())
}

/// Add a titled section: the keys lined up, a value of several lines indented
/// under its own key, and a blank line after.
pub fn section(out: &mut String, title: &str, rows: &[(String, String)]) {
    out.push_str(title);
    out.push('\n');
    let width = rows.iter().map(|(k, _)| k.chars().count()).filter(|&n| n <= MAX_KEY).max().unwrap_or(0);
    for (k, v) in rows {
        let mut lines = v.split('\n');
        let first = lines.next().unwrap_or_default();
        if k.chars().count() > MAX_KEY {
            out.push_str(&format!("    {k}\n    {:width$}   {first}\n", ""));
        } else {
            out.push_str(&format!("    {k:width$} : {first}\n"));
        }
        for rest in lines {
            out.push_str(&format!("    {:width$}   {rest}\n", ""));
        }
    }
    out.push('\n');
}

/// End the report with one newline instead of the blank line every section
/// leaves: its line count then does not depend on how it was counted.
pub fn finish(out: &mut String) {
    out.truncate(out.trim_end_matches('\n').len());
    out.push('\n');
}

/// One row per environment variable in `names`, `unset` (the app's word) for
/// those that are not set.
pub fn variables(names: &[&str], unset: &str) -> Rows {
    names.iter().map(|k| row(k, std::env::var(k).unwrap_or_else(|_| unset.into()))).collect()
}

/// The first section of every app's `env`: `Version`, `Executable` (with two
/// builds side by side nothing else says which one answered), the OS rows from
/// `os_line` (`ito_bugreport::os_line`, one `Key: value` per line, passed in so
/// this crate need not know how a platform is named) and `Debug`.
pub fn version_rows(version: &str, os_line: &str) -> Rows {
    let mut rows = vec![row("Version", version)];
    rows.push(row("Executable", std::env::current_exe().map_or_else(|e| format!("(unknown: {e})"), |p| p.display().to_string())));
    for line in os_line.split('\n') {
        match line.split_once(": ") {
            Some((k, v)) => rows.push(row(k, v)),
            None => rows.push(row(line, "")),
        }
    }
    rows.push(row("Debug", cfg!(debug_assertions).to_string()));
    rows
}

/// `found`, `found (N bytes)` or `not found`: the words every app uses for a
/// file it looked for.
pub fn presence(p: &std::path::Path) -> String {
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => "found".into(),
        Ok(m) => format!("found ({} bytes)", m.len()),
        Err(_) => "not found".into(),
    }
}

/// The folders and shared files every uchmk app has, for a `Files` section:
/// the `uchmk` folder, the app's own, `common.toml`, `themes`, the state folder
/// and the last-run record. The app adds its own files after these.
pub fn files_rows(app: &str) -> Rows {
    let shown = |p: &Option<std::path::PathBuf>| {
        p.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "none (this OS has no such folder)".into())
    };
    let base = crate::base_dir();
    let mut rows = vec![row("uchmk folder", shown(&base)), row(&format!("{app} folder"), shown(&crate::app_dir(app)))];
    if let Some(base) = &base {
        rows.push(row("common.toml", presence(&crate::common_path(base))));
        rows.push(row("themes", presence(&crate::themes_dir(base))));
    }
    let state = crate::state_dir(app);
    rows.push(row("state folder", shown(&state)));
    if let Some(p) = crate::runinfo::path(app) {
        rows.push(row(crate::runinfo::FILE, presence(&p)));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_key_gets_a_line_of_its_own() {
        let mut out = String::new();
        let long = "k".repeat(30);
        section(&mut out, "T", &[row("a", "1"), row(&long, "2\n3")]);
        assert_eq!(out, format!("T\n    a : 1\n    {long}\n        2\n        3\n\n"));
    }

    #[test]
    fn the_report_ends_in_one_newline() {
        let mut out = String::new();
        section(&mut out, "T", &[row("a", "1")]);
        finish(&mut out);
        assert_eq!(out, "T\n    a : 1\n");
    }

    #[test]
    fn variables_say_unset_in_the_apps_word() {
        let rows = variables(&["ITO_REPORT_NO_SUCH_VARIABLE"], "(unset)");
        assert_eq!(rows, [row("ITO_REPORT_NO_SUCH_VARIABLE", "(unset)")]);
    }

    #[test]
    fn the_version_section_splits_the_os_lines_into_rows() {
        let rows = version_rows(
            "1.2.3",
            "OS: Windows 11
Process arch: x86_64",
        );
        let keys: Vec<_> = rows.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["Version", "Executable", "OS", "Process arch", "Debug"]);
        assert_eq!(rows[2].1, "Windows 11");
    }

    #[test]
    fn the_files_section_names_the_shared_files() {
        let rows = files_rows("demo");
        let keys: Vec<_> = rows.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.starts_with(&["uchmk folder", "demo folder"]), "{keys:?}");
        assert!(keys.contains(&"state folder"));
        assert_eq!(presence(std::path::Path::new("/no/such/ito/file")), "not found");
    }
}
