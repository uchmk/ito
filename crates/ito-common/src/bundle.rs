//! Settings to one file and back: the backup / move-to-another-PC pair. The
//! bundle is one TOML file holding the text of `common.toml`, the user's
//! themes and the app's own settings files. What the app keeps between runs
//! (the window's place, sessions, history) is not in it. Every call reads or
//! writes the disk, so run it off the UI thread.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The bundle's format number; a newer one is refused, not guessed at.
pub const FORMAT: u32 = 1;
const MAX_FILE: u64 = 1 << 20;
const MAX_TOTAL: u64 = 8 << 20;
const MAX_FILES: usize = 200;
const MAX_DEPTH: usize = 4;

/// The settings of one app on one machine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    /// The bundle's format number ([`FORMAT`]).
    pub format: u32,
    /// The app that wrote it (`kura`).
    pub app: String,
    /// The app's version then, for the human who opens the file.
    pub version: String,
    /// The files by name: `common.toml`, `themes/<name>.toml` (both in the
    /// `uchmk` folder) and `app/<path>` (in the app's own settings folder).
    pub files: BTreeMap<String, String>,
}

/// What [`restore`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Restored {
    /// The names written; each one that was there before kept its old text in `<file>.bak-<time>`.
    pub written: Vec<String>,
    /// How many were the same as what is there already.
    pub unchanged: usize,
    /// The names left alone, with why.
    pub skipped: Vec<(String, String)>,
}

fn is_settings_file(name: &str) -> bool {
    name.ends_with(".toml") && !name.contains(".bak") && !name.contains(".uchmk-new")
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut BTreeMap<String, String>, total: &mut u64) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Some(name) = e.file_name().to_str().map(str::to_owned) else { continue };
        let key = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            if depth < MAX_DEPTH && !name.starts_with('.') {
                walk(&e.path(), &key, depth + 1, out, total)?;
            }
        } else if kind.is_file() && is_settings_file(&name) {
            let size = e.metadata().map_or(0, |m| m.len());
            if size > MAX_FILE {
                continue;
            }
            // Text that is not UTF-8 is left out rather than failing the whole export.
            let Ok(text) = String::from_utf8(std::fs::read(e.path()).map_err(|er| format!("{}: {er}", e.path().display()))?) else {
                continue;
            };
            *total += size;
            if out.len() >= MAX_FILES || *total > MAX_TOTAL {
                return Err("there are too many settings files to bundle".to_owned());
            }
            out.insert(key, text);
        }
    }
    Ok(())
}

/// The settings now: `common.toml` and the themes from the `uchmk` folder
/// `base`, and every `.toml` under the app's own folder `app_dir`.
pub fn collect(base: &Path, app_dir: &Path, app: &str, version: &str) -> Result<Bundle, String> {
    let mut files = BTreeMap::new();
    let mut total = 0;
    let common = crate::common_path(base);
    if let Ok(text) = std::fs::read_to_string(&common) {
        total += text.len() as u64;
        files.insert("common.toml".to_owned(), text);
    }
    walk(&crate::themes_dir(base), "themes", 1, &mut files, &mut total)?;
    let mut own = BTreeMap::new();
    walk(app_dir, "app", 0, &mut own, &mut total)?;
    files.extend(own);
    Ok(Bundle { format: FORMAT, app: app.to_owned(), version: version.to_owned(), files })
}

/// The bundle as the text of its file.
pub fn to_text(b: &Bundle) -> Result<String, String> {
    let head = "# uchmk settings bundle. Import it from the app's Advanced page.\n";
    toml::to_string(b).map(|t| format!("{head}{t}")).map_err(|e| e.to_string())
}

/// The bundle in `text`; an error when it is not one, or from a newer format.
pub fn parse(text: &str) -> Result<Bundle, String> {
    let b: Bundle = toml::from_str(text).map_err(|e| format!("not a settings bundle: {e}"))?;
    if b.format == 0 || b.format > FORMAT {
        return Err(format!("the bundle's format is {}, and this app reads up to {FORMAT}", b.format));
    }
    Ok(b)
}

/// Where the bundle's `name` goes; an error for a name that could leave the
/// settings folders (`..`, a drive, a backslash) or is not a `.toml` file.
fn target(base: &Path, app_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let parts: Vec<&str> = name.split('/').collect();
    let clean =
        parts.iter().all(|p| !p.is_empty() && *p != "." && *p != ".." && !p.contains(['\\', ':']) && !p.chars().any(char::is_control));
    if !clean || !is_settings_file(name) {
        return Err("not a settings file name".to_owned());
    }
    let (root, rest) = match parts.as_slice() {
        ["common.toml"] => return Ok(crate::common_path(base)),
        ["themes", rest @ ..] if !rest.is_empty() => (crate::themes_dir(base), rest),
        ["app", rest @ ..] if !rest.is_empty() => (app_dir.to_path_buf(), rest),
        _ => return Err("not a place settings go".to_owned()),
    };
    Ok(rest.iter().fold(root, |p, s| p.join(s)))
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Write the bundle's files over the settings. Nothing is written when a file
/// is not valid TOML. A file that differs keeps its old text in
/// `<file>.bak-<time>`. The app's own files (`app/…`) are skipped when the
/// bundle came from another app.
pub fn restore(b: &Bundle, base: &Path, app_dir: &Path, app: &str) -> Result<Restored, String> {
    let mut plan = Vec::new();
    let mut out = Restored::default();
    for (name, text) in &b.files {
        if name.starts_with("app/") && b.app != app {
            out.skipped.push((name.clone(), format!("made by {}", b.app)));
            continue;
        }
        let path = match target(base, app_dir, name) {
            Ok(p) => p,
            Err(why) => {
                out.skipped.push((name.clone(), why));
                continue;
            }
        };
        if text.len() as u64 > MAX_FILE {
            out.skipped.push((name.clone(), "too big".to_owned()));
            continue;
        }
        toml::from_str::<toml::Table>(text).map_err(|e| format!("{name} is not valid TOML: {e}"))?;
        plan.push((name, path, text));
    }
    let stamp = now_secs();
    for (name, path, text) in plan {
        match std::fs::read(&path) {
            Ok(old) if old == text.as_bytes() => {
                out.unchanged += 1;
                continue;
            }
            Ok(old) => {
                let mut aside = path.as_os_str().to_owned();
                aside.push(format!(".bak-{stamp}"));
                crate::write_atomic(Path::new(&aside), old)?;
            }
            Err(_) => {}
        }
        crate::write_atomic(&path, text)?;
        out.written.push(name.clone());
    }
    Ok(out)
}

/// Write the settings now to `file`.
pub fn export(file: &Path, base: &Path, app_dir: &Path, app: &str, version: &str) -> Result<usize, String> {
    let b = collect(base, app_dir, app, version)?;
    crate::write_atomic(file, to_text(&b)?)?;
    Ok(b.files.len())
}

/// Read the bundle in `file` back over the settings.
pub fn import(file: &Path, base: &Path, app_dir: &Path, app: &str) -> Result<Restored, String> {
    if std::fs::metadata(file).map_err(|e| format!("{}: {e}", file.display()))?.len() > MAX_TOTAL {
        return Err("the file is too big to be a settings bundle".to_owned());
    }
    restore(&parse(&crate::read_or_empty(file)?)?, base, app_dir, app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("ito-bundle-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (base, app) = (root.join("uchmk"), root.join("uchmk").join("kura"));
        std::fs::create_dir_all(base.join("themes")).unwrap();
        std::fs::create_dir_all(&app).unwrap();
        (root, base, app)
    }

    #[test]
    fn a_bundle_round_trips_through_its_text() {
        let mut b = Bundle { format: FORMAT, app: "kura".into(), version: "1.0.0".into(), ..Bundle::default() };
        b.files.insert("common.toml".into(), "language = \"ja\"\n[clock]\nformat = 'x'\n".into());
        b.files.insert("themes/a b.toml".into(), "name = \"a\"\n".into());
        assert_eq!(parse(&to_text(&b).unwrap()).unwrap(), b);
    }

    #[test]
    fn collect_takes_settings_and_leaves_backups_and_other_apps() {
        let (root, base, app) = dirs("collect");
        std::fs::write(base.join("common.toml"), "language = \"en\"\n").unwrap();
        std::fs::write(base.join("themes").join("t.toml"), "x = 1\n").unwrap();
        std::fs::write(app.join("config.toml"), "a = 1\n").unwrap();
        std::fs::write(app.join("config.toml.bak-1"), "old").unwrap();
        std::fs::write(app.join("notes.txt"), "no").unwrap();
        std::fs::create_dir_all(base.join("tsumugi")).unwrap();
        std::fs::write(base.join("tsumugi").join("config.toml"), "b = 1\n").unwrap();
        let b = collect(&base, &app, "kura", "1").unwrap();
        let names: Vec<&str> = b.files.keys().map(String::as_str).collect();
        assert_eq!(names, ["app/config.toml", "common.toml", "themes/t.toml"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn restore_keeps_a_copy_of_what_it_replaces_and_skips_odd_names() {
        let (root, base, app) = dirs("restore");
        std::fs::write(base.join("common.toml"), "language = \"en\"\n").unwrap();
        std::fs::write(app.join("config.toml"), "a = 1\n").unwrap();
        let mut b = Bundle { format: FORMAT, app: "kura".into(), version: "1".into(), ..Bundle::default() };
        b.files.insert("common.toml".into(), "language = \"ja\"\n".into());
        b.files.insert("app/config.toml".into(), "a = 1\n".into());
        b.files.insert("themes/new.toml".into(), "x = 1\n".into());
        b.files.insert("../evil.toml".into(), "x = 1\n".into());
        b.files.insert("themes/..\\evil.toml".into(), "x = 1\n".into());
        b.files.insert("other.toml".into(), "x = 1\n".into());
        let r = restore(&b, &base, &app, "kura").unwrap();
        assert_eq!(r.written, ["common.toml", "themes/new.toml"]);
        assert_eq!(r.unchanged, 1);
        assert_eq!(r.skipped.len(), 3);
        assert_eq!(std::fs::read_to_string(base.join("common.toml")).unwrap(), "language = \"ja\"\n");
        let baks =
            std::fs::read_dir(&base).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("common.toml.bak-")).count();
        assert_eq!(baks, 1);
        assert!(!root.join("evil.toml").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn another_apps_own_files_are_skipped_and_bad_toml_writes_nothing() {
        let (root, base, app) = dirs("other");
        let mut b = Bundle { format: FORMAT, app: "yagura".into(), version: "1".into(), ..Bundle::default() };
        b.files.insert("app/config.toml".into(), "a = 1\n".into());
        b.files.insert("common.toml".into(), "language = \"ja\"\n".into());
        let r = restore(&b, &base, &app, "kura").unwrap();
        assert_eq!(r.written, ["common.toml"]);
        assert_eq!(r.skipped.len(), 1);
        assert!(!app.join("config.toml").exists());
        b.files.insert("themes/bad.toml".into(), "= nope".into());
        b.files.insert("common.toml".into(), "language = \"en\"\n".into());
        assert!(restore(&b, &base, &app, "kura").is_err());
        assert_eq!(std::fs::read_to_string(base.join("common.toml")).unwrap(), "language = \"ja\"\n");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_newer_format_or_a_stranger_file_is_refused() {
        assert!(parse("format = 99\napp = \"x\"\nversion = \"1\"\n[files]\n").is_err());
        assert!(parse("hello = 1").is_err());
    }
}
