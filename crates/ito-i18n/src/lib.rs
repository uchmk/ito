//! The words on screen, looked up by key, and the language setting uchmk's
//! apps share. An app ships its tables (`en.toml`, `ja.toml` … as
//! `include_str!`), English first; [`Table::new`] lays the chosen language
//! over English, so a key a translation lacks still reads.
//!
//! Which language: the app's own setting, else `language` in the
//! `common.toml` every uchmk app reads ([`base_dir`]), else the OS's
//! ([`os_language`]); `auto` passes the choice on ([`resolve`]). mimamori,
//! tsumugi and filer read the same file, so a person sets it once.
//!
//! ```text
//! <config dir>/uchmk/common.toml   language = "auto" | "en" | "ja"
//! ```
//!
//! [`check`] has what an app's tests run over its own tables and source.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The values `language` takes in common.toml, with their names as a
/// settings screen lists them (each in its own language).
pub const LANGUAGES: &[(&str, &str)] = &[("auto", "Auto (system)"), ("en", "English"), ("ja", "日本語")];

/// The `uchmk` folder: `UCHMK_CONFIG_DIR` when set; else `%APPDATA%\uchmk`
/// on Windows, `~/Library/Application Support/uchmk` on macOS,
/// `$XDG_CONFIG_HOME/uchmk` (or `~/.config/uchmk`) elsewhere.
pub fn base_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = var("UCHMK_CONFIG_DIR") {
        return Some(d);
    }
    let dir = if cfg!(windows) {
        var("APPDATA")?
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library").join("Application Support")
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))?
    };
    Some(dir.join("uchmk"))
}

/// `common.toml` in the `uchmk` folder `base`.
pub fn common_path(base: &Path) -> PathBuf {
    base.join("common.toml")
}

/// `language` from common.toml's text; `Err` says what is wrong with the
/// file (it does not parse, or the value is not a string).
pub fn common_language(text: &str) -> Result<Option<String>, String> {
    let t = text.parse::<toml::Table>().map_err(|e| format!("common.toml: {e}"))?;
    match t.get("language") {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err("common.toml: `language` must be a string".to_owned()),
    }
}

/// Read `language` from common.toml in `base`: `Ok(None)` when there is no
/// file or no key.
pub fn read_common_language(base: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(common_path(base)) {
        Ok(text) => common_language(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("common.toml: {e}")),
    }
}

/// common.toml's text with `language` set to `code`, the rest of the file
/// (other keys, comments) as it was. A file that does not parse is an error,
/// never written over.
#[cfg(feature = "edit")]
pub fn set_common_language(text: &str, code: &str) -> Result<String, String> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| format!("common.toml does not read, so it is left as it is: {e}"))?;
    doc["language"] = toml_edit::value(code);
    Ok(doc.to_string())
}

/// The OS's language as it names it (`ja-JP`, `en_US.UTF-8` …).
pub fn os_language() -> Option<String> {
    sys_locale::get_locale()
}

/// The language to use: the first of the app's and the common setting that
/// is not `auto`, else the OS's (`ja-JP` → `ja`); one not in `available` is
/// English.
pub fn resolve(app: Option<&str>, common: Option<&str>, os: Option<&str>, available: &[&str]) -> String {
    let chosen = [app, common].into_iter().flatten().map(str::trim).find(|l| !l.is_empty() && !l.eq_ignore_ascii_case("auto"));
    let code = match chosen {
        Some(l) => l.to_ascii_lowercase(),
        None => os.and_then(|l| l.split(['-', '_']).next()).unwrap_or("en").to_ascii_lowercase(),
    };
    if available.contains(&code.as_str()) {
        code
    } else {
        "en".into()
    }
}

/// One language's words, English underneath.
#[derive(Debug, Default)]
pub struct Table {
    words: HashMap<String, String>,
}

impl Table {
    /// The table for `code` from an app's `tables` (code and TOML text,
    /// English among them). A table that does not parse is left out and said
    /// in the second value; the app shows or prints that.
    pub fn new(tables: &[(&str, &str)], code: &str) -> (Self, Vec<String>) {
        let (mut words, mut errors) = (HashMap::new(), Vec::new());
        for (lang, src) in tables.iter().filter(|(l, _)| *l == "en").chain(tables.iter().filter(|(l, _)| *l == code && code != "en")) {
            match src.parse::<toml::Table>() {
                Ok(t) => flatten("", &t, &mut words),
                Err(e) => errors.push(format!("lang/{lang}.toml: {e}")),
            }
        }
        (Self { words }, errors)
    }

    /// The text for `key`; the key itself when no table has it, so a missing
    /// entry shows up on screen instead of a blank.
    pub fn tr<'a>(&'a self, key: &'a str) -> &'a str {
        self.words.get(key).map_or(key, String::as_str)
    }

    /// [`Table::tr`] with `{0}`, `{1}` … replaced by `args`.
    pub fn trf(&self, key: &str, args: &[&str]) -> String {
        let mut s = self.tr(key).to_owned();
        for (i, a) in args.iter().enumerate() {
            s = s.replace(&format!("{{{i}}}"), a);
        }
        s
    }

    /// A key's description in a help screen: the `[keydesc]` entry for the
    /// keymap's English text, else that text (a person's own description
    /// reads as written).
    pub fn tr_desc<'a>(&'a self, desc: &'a str) -> &'a str {
        self.words.get(&format!("keydesc.{desc}")).map_or(desc, String::as_str)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.words.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.words.keys().map(String::as_str)
    }
}

/// `[status] paused = "…"` becomes `status.paused`.
fn flatten(prefix: &str, t: &toml::Table, out: &mut HashMap<String, String>) {
    for (k, v) in t {
        let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
        match v {
            toml::Value::String(s) => {
                out.insert(key, s.clone());
            }
            toml::Value::Table(sub) => flatten(&key, sub, out),
            _ => {}
        }
    }
}

/// What an app's tests check its tables and source with. Each returns the
/// problems found, empty when there are none.
pub mod check {
    use super::*;

    /// One table on its own, without English underneath.
    fn own(src: &str) -> Result<HashMap<String, String>, String> {
        let mut out = HashMap::new();
        flatten("", &src.parse::<toml::Table>().map_err(|e| e.to_string())?, &mut out);
        Ok(out)
    }

    /// The `{0}` … a string takes.
    fn holes(s: &str) -> Vec<usize> {
        (0..10).filter(|i| s.contains(&format!("{{{i}}}"))).collect()
    }

    /// Every translation has each English key, no key English lacks (a typo
    /// would never be read; `keydesc.` aside, which translates keymap text),
    /// and the same `{0}` … to fill in.
    pub fn translations(tables: &[(&str, &str)]) -> Vec<String> {
        let mut bad = Vec::new();
        let Some(en) = tables.iter().find(|(l, _)| *l == "en") else { return vec!["no English table".to_owned()] };
        let en = match own(en.1) {
            Ok(t) => t,
            Err(e) => return vec![format!("en.toml: {e}")],
        };
        for (code, src) in tables.iter().filter(|(l, _)| *l != "en") {
            let t = match own(src) {
                Ok(t) => t,
                Err(e) => {
                    bad.push(format!("{code}.toml: {e}"));
                    continue;
                }
            };
            bad.extend(en.keys().filter(|k| !t.contains_key(*k)).map(|k| format!("{code}.toml: missing {k}")));
            for (k, v) in t.iter().filter(|(k, _)| !k.starts_with("keydesc.")) {
                match en.get(k) {
                    None => bad.push(format!("{code}.toml: not in English: {k}")),
                    Some(e) if holes(e) != holes(v) => bad.push(format!("{code}.toml: {k}: {:?} vs {:?}", holes(e), holes(v))),
                    _ => {}
                }
            }
        }
        bad.sort();
        bad
    }

    /// Each of `descs` (a keymap's descriptions) has a `[keydesc]` entry in
    /// every translation.
    pub fn key_descriptions(tables: &[(&str, &str)], descs: &[&str]) -> Vec<String> {
        let mut bad = Vec::new();
        for (code, src) in tables.iter().filter(|(l, _)| *l != "en") {
            let Ok(t) = own(src) else { continue };
            bad.extend(descs.iter().filter(|d| !t.contains_key(&format!("keydesc.{d}"))).map(|d| format!("{code}.toml [keydesc] lacks {d:?}")));
        }
        bad
    }

    /// Every `tr("…")` / `trf("…"` in the `.rs` files under `src` has an
    /// entry in `english`, so no key ever shows on screen as itself. Files
    /// named in `skip` (the app's own i18n module) are not read.
    pub fn source_keys(src: &Path, english: &Table, skip: &[&str]) -> Vec<String> {
        let mut missing = Vec::new();
        let mut stack = vec![src.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if p.extension().is_none_or(|x| x != "rs") || skip.contains(&name) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                missing.extend(keys_in(&text).into_iter().filter(|k| !english.contains(k)).map(|k| format!("{}: {k}", p.display())));
            }
        }
        missing.sort();
        missing
    }

    /// The literal keys `tr("…")` and `trf("…"` take in `text`.
    pub fn keys_in(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        for pat in ["tr(\"", "trf(\""] {
            for (i, _) in text.match_indices(pat) {
                // `xtr(` or `.tr(` would be someone else's function.
                if text[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                let rest = &text[i + pat.len()..];
                if let Some(end) = rest.find('"') {
                    out.push(&rest[..end]);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "[status]\npaused = \"paused\"\nleft = \"{0} left\"\nonly = \"English only\"\n";
    const JA: &str = "[status]\npaused = \"一時停止\"\nleft = \"残り {0}\"\n\n[keydesc]\n\"Quit\" = \"終了\"\n";
    const TABLES: &[(&str, &str)] = &[("en", EN), ("ja", JA)];

    #[test]
    fn a_translation_lies_over_english() {
        let (t, errors) = Table::new(TABLES, "ja");
        assert!(errors.is_empty());
        assert_eq!(t.tr("status.paused"), "一時停止");
        assert_eq!(t.tr("status.only"), "English only", "a key the translation lacks reads in English");
        assert_eq!(t.tr("no.such"), "no.such");
        assert_eq!(t.trf("status.left", &["3 s"]), "残り 3 s");
        assert_eq!(t.tr_desc("Quit"), "終了");
        assert_eq!(t.tr_desc("My own"), "My own");
        let (en, _) = Table::new(TABLES, "en");
        assert_eq!(en.tr("status.paused"), "paused");
        assert_eq!(en.tr_desc("Quit"), "Quit");
    }

    #[test]
    fn a_table_that_does_not_parse_is_said() {
        let (t, errors) = Table::new(&[("en", EN), ("ja", "[status")], "ja");
        assert_eq!(errors.len(), 1);
        assert_eq!(t.tr("status.paused"), "paused");
    }

    #[test]
    fn the_app_setting_beats_the_common_one_and_auto_follows_the_os() {
        let av = ["en", "ja"];
        assert_eq!(resolve(Some("en"), Some("ja"), Some("ja-JP"), &av), "en");
        assert_eq!(resolve(Some("auto"), Some("ja"), Some("en-US"), &av), "ja");
        assert_eq!(resolve(Some("auto"), Some("en"), Some("ja-JP"), &av), "en");
        assert_eq!(resolve(None, None, Some("ja-JP"), &av), "ja");
        assert_eq!(resolve(None, None, Some("ja_JP.UTF-8"), &av), "ja");
        assert_eq!(resolve(Some("JA"), None, None, &av), "ja");
        assert_eq!(resolve(None, None, Some("en-US"), &av), "en");
        assert_eq!(resolve(None, Some("auto"), None, &av), "en");
        assert_eq!(resolve(Some("xx"), None, None, &av), "en");
        assert_eq!(resolve(None, Some("ja"), None, &["en"]), "en", "an app without the table stays in English");
    }

    #[test]
    fn common_toml_is_read_and_its_mistakes_said() {
        assert_eq!(common_language("language = \"ja\"\n"), Ok(Some("ja".into())));
        assert_eq!(common_language("# nothing\n"), Ok(None));
        assert!(common_language("language = 1\n").is_err());
        assert!(common_language("language = \n").is_err());
        let dir = std::env::temp_dir().join(format!("tsumugi-i18n-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read_common_language(&dir), Ok(None), "no folder, no file");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(common_path(&dir), "language = \"en\"\n").unwrap();
        assert_eq!(read_common_language(&dir), Ok(Some("en".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "edit")]
    #[test]
    fn setting_the_language_keeps_the_rest_of_the_file() {
        let text = "# shared by uchmk's apps\nlanguage = \"en\" # mine\ntheme = \"dark\"\n";
        let next = set_common_language(text, "ja").unwrap();
        assert!(next.contains("# shared by uchmk's apps"));
        assert!(next.contains("theme = \"dark\""));
        assert_eq!(common_language(&next), Ok(Some("ja".into())));
        assert_eq!(common_language(&set_common_language("", "auto").unwrap()), Ok(Some("auto".into())));
        assert!(set_common_language("language = ", "ja").is_err(), "a broken file is not written over");
    }

    #[test]
    fn the_checks_find_what_is_wrong() {
        assert_eq!(check::translations(TABLES), vec!["ja.toml: missing status.only".to_owned()]);
        let bad = [("en", EN), ("ja", "[status]\npaused = \"a\"\nleft = \"b\"\nonly = \"c\"\ntypo = \"d\"\n")];
        assert_eq!(check::translations(&bad), vec!["ja.toml: not in English: status.typo".to_owned(), "ja.toml: status.left: [0] vs []".to_owned()]);
        assert_eq!(check::key_descriptions(TABLES, &["Quit", "Help"]), vec!["ja.toml [keydesc] lacks \"Help\"".to_owned()]);
        let src = "tr(\"a.b\"); trf(\"c\", &[]); x.tr(\"no\"); attr(\"no\");";
        assert_eq!(check::keys_in(src), vec!["a.b", "c"]);
    }

    #[test]
    fn every_language_a_screen_lists_resolves_to_itself() {
        let av: Vec<&str> = LANGUAGES.iter().map(|(c, _)| *c).filter(|c| *c != "auto").collect();
        for code in &av {
            assert_eq!(&resolve(Some(code), None, None, &av), code);
        }
    }
}
