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
}
