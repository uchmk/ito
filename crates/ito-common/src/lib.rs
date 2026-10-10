//! What every uchmk app (tsumugi, mimamori, filer and those to come) shares
//! (docs/common-spec.md): where the settings live, `common.toml` -- the
//! language, the theme and the clock, so changing that one file changes
//! every app at once -- and editing a TOML file one key at a time with what
//! a person wrote around it kept.
//!
//! ```text
//! <config dir>/uchmk/common.toml        language, theme, dark_theme, light_theme, [clock]
//! <config dir>/uchmk/<app>/config.toml  each app's own
//! <config dir>/uchmk/themes/*.toml      themes of one's own
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

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

/// The themes of one's own every app lists, in the `uchmk` folder `base`.
pub fn themes_dir(base: &Path) -> PathBuf {
    base.join("themes")
}

/// The OS's language as it names it (`ja-JP`, `en_US.UTF-8` …).
pub fn os_language() -> Option<String> {
    sys_locale::get_locale()
}

/// The date formats the clock knows.
pub const DATE_FORMATS: [&str; 4] = ["YYYY/MM/DD", "YYYY-MM-DD", "MM/DD/YYYY", "DD/MM/YYYY"];

/// The clock at the status bar's right end (`[clock]` in common.toml).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Clock {
    pub show: bool,
    pub hour24: bool,
    pub date: bool,
    pub date_format: String,
    pub weekday: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Self { show: true, hour24: true, date: true, date_format: "YYYY/MM/DD".into(), weekday: true }
    }
}

impl Clock {
    /// A `chrono` format string: `2026/10/05 (Mon) 14:32`.
    pub fn format(&self) -> String {
        let mut out = Vec::new();
        if self.date {
            out.push(
                match self.date_format.as_str() {
                    "YYYY-MM-DD" => "%Y-%m-%d",
                    "MM/DD/YYYY" => "%m/%d/%Y",
                    "DD/MM/YYYY" => "%d/%m/%Y",
                    _ => "%Y/%m/%d",
                }
                .to_owned(),
            );
            if self.weekday {
                out.push("(%a)".into());
            }
        }
        out.push(if self.hour24 { "%H:%M".into() } else { "%-I:%M %p".into() });
        out.join(" ")
    }
}

#[cfg(feature = "clock")]
mod clock {
    use chrono::{Datelike, NaiveDateTime, Timelike};

    use super::Clock;

    const JA_WEEKDAYS: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

    fn ja_weekday(now: &NaiveDateTime) -> &'static str {
        JA_WEEKDAYS[now.weekday().num_days_from_monday() as usize]
    }

    impl Clock {
        /// The clock's words for `now` in the language `lang` (`ja`: the
        /// weekday as 「(月)」).
        pub fn text(&self, now: &NaiveDateTime, lang: &str) -> String {
            let mut format = self.format();
            if lang == "ja" {
                format = format.replace("(%a)", &format!("({})", ja_weekday(now)));
            }
            now.format(&format).to_string()
        }

        /// The whole date, for the pointer over the clock.
        pub fn full(now: &NaiveDateTime, lang: &str) -> String {
            match lang {
                "ja" => now.format(&format!("%Y年%-m月%-d日 {}曜日", ja_weekday(now))).to_string(),
                _ => now.format("%A, %-d %B %Y").to_string(),
            }
        }

        /// How long until the minute changes: when to draw the clock again.
        pub fn next_minute(now: &NaiveDateTime) -> std::time::Duration {
            let into = std::time::Duration::from_secs(u64::from(now.second()))
                + std::time::Duration::from_nanos(u64::from(now.nanosecond() % 1_000_000_000));
            std::time::Duration::from_secs(60).saturating_sub(into).max(std::time::Duration::from_millis(1))
        }
    }
}

/// What `common.toml` holds; `None` for a key it does not have, so an app
/// can fall back on its own older setting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Common {
    /// `auto`, `en`, `ja` …
    pub language: Option<String>,
    /// A theme's name, or `dark`, `light` or `system` (follow the OS).
    pub theme: Option<String>,
    /// The themes `system` uses in a dark and a light OS.
    pub dark_theme: Option<String>,
    pub light_theme: Option<String>,
    pub clock: Option<Clock>,
}

/// The keys `common.toml` has.
const KEYS: [&str; 5] = ["language", "theme", "dark_theme", "light_theme", "clock"];

impl Common {
    /// The settings in common.toml's `text`, and what in it is not known
    /// (a key no app reads, a date format not in [`DATE_FORMATS`]) to show
    /// as a warning. `Err` when the file does not read as TOML or a value is
    /// of the wrong kind.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        let t = text.parse::<toml::Table>().map_err(|e| format!("common.toml: {e}"))?;
        let mut warnings = Vec::new();
        for k in t.keys().filter(|k| !KEYS.contains(&k.as_str())) {
            warnings.push(format!("common.toml: `{k}` is not a setting uchmk's apps read"));
        }
        let text_of = |key: &str| -> Result<Option<String>, String> {
            match t.get(key) {
                None => Ok(None),
                Some(toml::Value::String(s)) => Ok(Some(s.clone())),
                Some(_) => Err(format!("common.toml: `{key}` must be a string")),
            }
        };
        let clock = match t.get("clock") {
            None => None,
            Some(toml::Value::Table(c)) => {
                let known = ["show", "hour24", "date", "date_format", "weekday"];
                let mut kept = toml::Table::new();
                for (k, v) in c {
                    if known.contains(&k.as_str()) {
                        kept.insert(k.clone(), v.clone());
                    } else {
                        warnings.push(format!("common.toml: `clock.{k}` is not a setting uchmk's apps read"));
                    }
                }
                let mut clock =
                    Clock::deserialize(toml::Value::Table(kept)).map_err(|e| format!("common.toml: [clock]: {}", e.message()))?;
                if !DATE_FORMATS.contains(&clock.date_format.as_str()) {
                    warnings.push(format!(
                        "common.toml: clock.date_format `{}` is not one of {}",
                        clock.date_format,
                        DATE_FORMATS.join(", ")
                    ));
                    clock.date_format = Clock::default().date_format;
                }
                Some(clock)
            }
            Some(_) => return Err("common.toml: `clock` must be a table ([clock])".to_owned()),
        };
        let common = Self {
            language: text_of("language")?,
            theme: text_of("theme")?,
            dark_theme: text_of("dark_theme")?,
            light_theme: text_of("light_theme")?,
            clock,
        };
        Ok((common, warnings))
    }

    /// common.toml in `base`: the empty settings when there is no file.
    pub fn read(base: &Path) -> Result<(Self, Vec<String>), String> {
        let path = common_path(base);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Self::default(), Vec::new())),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// The theme chosen -- `theme`, `dark_theme`, `light_theme` -- each from
    /// common.toml, else from `fallback` (an app's own older setting), else
    /// `dark`, `tsumugi Dark` and `tsumugi Light`.
    pub fn theme_choice(&self, fallback: Option<(&str, &str, &str)>) -> (String, String, String) {
        let (theme, dark, light) = fallback.unwrap_or(("dark", "tsumugi Dark", "tsumugi Light"));
        let pick = |own: &Option<String>, old: &str| own.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| old.to_owned());
        (pick(&self.theme, theme), pick(&self.dark_theme, dark), pick(&self.light_theme, light))
    }

    /// The clock: common.toml's, else `fallback`, else the default.
    pub fn clock_or(&self, fallback: Option<&Clock>) -> Clock {
        self.clock.clone().or_else(|| fallback.cloned()).unwrap_or_default()
    }
}

/// A change to common.toml: one key in a table (`None`: the top), its value
/// already written as TOML. A settings screen hands these back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonChange {
    pub table: Option<&'static str>,
    pub key: &'static str,
    pub value: String,
}

impl CommonChange {
    /// common.toml's `text` with the change made.
    pub fn apply(&self, text: &str) -> Result<String, String> {
        set_key(text, self.table, self.key, &self.value)
    }
}

/// Make a change to common.toml in `base` -- its text in, the new text out
/// -- and write it at once. One change at a time across the app, each on
/// what the last wrote. Call it off the UI thread.
pub fn edit_common(base: &Path, change: impl FnOnce(&str) -> Result<String, String>) -> Result<(), String> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let path = common_path(base);
    read_or_empty(&path).and_then(|text| change(&text)).and_then(|next| write_atomic(&path, next))
}

/// The table at the dotted `path` (`None`: the top), made when it is not
/// there; an error when the path is something else (an array of tables).
fn table_at<'a>(doc: &'a mut toml_edit::DocumentMut, path: Option<&str>) -> Result<&'a mut toml_edit::Table, String> {
    let mut t = doc.as_table_mut();
    let Some(path) = path else { return Ok(t) };
    for part in path.split('.') {
        let item = t.entry(part).or_insert_with(|| {
            let mut new = toml_edit::Table::new();
            new.set_implicit(true);
            toml_edit::Item::Table(new)
        });
        t = item.as_table_mut().ok_or_else(|| format!("[{path}] is not a table in the file"))?;
    }
    Ok(t)
}

fn document(text: &str) -> Result<toml_edit::DocumentMut, String> {
    text.parse::<toml_edit::DocumentMut>().map_err(|e| format!("the file does not read as TOML, so it is left as it is: {e}"))
}

/// A key as the settings write it (`A`, `"my tag"`), as the name it stands for.
fn key_name(key: &str) -> Result<String, String> {
    let parts = toml_edit::Key::parse(key).map_err(|e| format!("`{key}` is not a key: {e}"))?;
    match parts.as_slice() {
        [one] => Ok(one.get().to_owned()),
        _ => Err(format!("`{key}` is not a single key")),
    }
}

/// `text` with `key` in `table` (`None`: the top, before any table) set to
/// `value`, already written as TOML. Only that key changes, so what a person
/// wrote around it -- comments, order, blank lines -- stays. A key not there
/// is added at the end of its table, and a table not there at the end of the
/// file. An error, and nothing to write, when the file does not read as TOML
/// or the value is not a TOML value.
pub fn set_key(text: &str, table: Option<&str>, key: &str, value: &str) -> Result<String, String> {
    let mut doc = document(text)?;
    let mut new: toml_edit::Value = value.parse().map_err(|e| format!("`{value}` is not a TOML value: {e}"))?;
    let name = key_name(key)?;
    let t = table_at(&mut doc, table)?;
    // A new table that only held tables before is shown now it has a key.
    t.set_implicit(false);
    match t.get_mut(&name).and_then(toml_edit::Item::as_value_mut) {
        Some(old) => {
            // The comment after the old value stays after the new one.
            let suffix = old.decor().suffix().and_then(|s| s.as_str()).map(str::to_owned);
            new.decor_mut().set_prefix(" ");
            new.decor_mut().set_suffix(suffix.as_deref().filter(|s| s.contains('#')).unwrap_or(""));
            *old = new;
        }
        None => {
            t.insert(&name, toml_edit::Item::Value(new));
        }
    }
    Ok(doc.to_string())
}

/// `text` without `key` in `table`; the same text when it is not there.
pub fn remove_key(text: &str, table: Option<&str>, key: &str) -> Result<String, String> {
    let mut doc = document(text)?;
    let name = key_name(key)?;
    let mut t = doc.as_table_mut();
    if let Some(path) = table {
        for part in path.split('.') {
            match t.get_mut(part).and_then(toml_edit::Item::as_table_mut) {
                Some(next) => t = next,
                None => return Ok(text.to_owned()),
            }
        }
    }
    if t.remove(&name).is_none() {
        return Ok(text.to_owned());
    }
    Ok(doc.to_string())
}

/// `text` with every `[[name]]` block taken out and `blocks` written at the
/// end in their place, each a list of keys and their values as TOML.
/// Comments inside the old blocks go with them; the rest stays.
pub fn set_tables(text: &str, name: &str, blocks: &[Vec<(&str, String)>]) -> Result<String, String> {
    let mut doc = document(text)?;
    let (parent, last) = match name.rsplit_once('.') {
        Some((p, l)) => (Some(p), l),
        None => (None, name),
    };
    let mut list = toml_edit::ArrayOfTables::new();
    for b in blocks {
        let mut t = toml_edit::Table::new();
        for (k, v) in b {
            let v: toml_edit::Value = v.parse().map_err(|e| format!("`{v}` is not a TOML value: {e}"))?;
            t.insert(&key_name(k)?, toml_edit::Item::Value(v));
        }
        list.push(t);
    }
    let t = table_at(&mut doc, parent)?;
    t.remove(last);
    if !list.is_empty() {
        t.insert(last, toml_edit::Item::ArrayOfTables(list));
    }
    Ok(doc.to_string())
}

/// A string as TOML writes it, quoted and escaped.
pub fn quote(s: &str) -> String {
    toml::Value::String(s.to_owned()).to_string()
}

/// A list of strings as TOML writes it.
pub fn quote_list(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", "))
}

/// When the file was last changed, to read it again only then; `None` when
/// there is none.
pub fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The file's text; empty when there is no such file. Any other failure --
/// no permission, another program holding it, not UTF-8 (a PowerShell 5.1
/// profile saved as UTF-16) -- is an error, so nothing gets written over it.
pub fn read_or_empty(path: &Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8 text (saved as UTF-16?), so it is left as it is", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Write `bytes` to `path` all at once: beside it first, to the disk, then
/// over it. A crash or a full disk leaves the old file whole.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut aside = path.as_os_str().to_owned();
    aside.push(".uchmk-new");
    let aside = PathBuf::from(aside);
    let written = (|| {
        let mut f = std::fs::File::create(&aside)?;
        f.write_all(bytes.as_ref())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&aside, path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&aside);
        return Err(format!("{}: {e}", path.display()));
    }
    Ok(())
}

/// Look at when `paths` last changed every `every`, on a thread of its own,
/// and call `on_change` with those that did; it returns `false` to stop (the
/// app has gone). Only the times are read here; reading the files again is
/// `on_change`'s.
pub fn watch(paths: Vec<PathBuf>, every: Duration, mut on_change: impl FnMut(&[PathBuf]) -> bool + Send + 'static) {
    let _ = std::thread::Builder::new().name("uchmk-watch".into()).spawn(move || {
        let mut seen: Vec<Option<SystemTime>> = paths.iter().map(|p| stamp(p)).collect();
        loop {
            std::thread::sleep(every);
            let mut changed = Vec::new();
            for (p, was) in paths.iter().zip(seen.iter_mut()) {
                let now = stamp(p);
                if now != *was {
                    *was = now;
                    changed.push(p.clone());
                }
            }
            if !changed.is_empty() && !on_change(&changed) {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-common-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn common_toml_is_read_and_what_it_does_not_know_is_said() {
        let (c, warnings) = Common::parse("language = \"ja\"\ntheme = \"Nord\"\n[clock]\nhour24 = false\n").unwrap();
        assert_eq!(c.language.as_deref(), Some("ja"));
        assert_eq!(c.theme.as_deref(), Some("Nord"));
        assert_eq!(c.dark_theme, None);
        assert_eq!(c.clock, Some(Clock { hour24: false, ..Clock::default() }));
        assert!(warnings.is_empty());
        let (c, warnings) = Common::parse("colour = 1\n[clock]\nseconds = true\ndate_format = \"DD.MM.YYYY\"\n").unwrap();
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert_eq!(c.clock.unwrap().date_format, "YYYY/MM/DD", "an unknown format falls back");
        assert!(Common::parse("theme = 3\n").is_err());
        assert!(Common::parse("[clock]\nshow = \"yes\"\n").is_err());
        assert!(Common::parse("[broken\n").is_err());
        assert_eq!(Common::read(&dir("none")), Ok((Common::default(), Vec::new())), "no file: nothing set");
    }

    #[test]
    fn common_toml_beats_an_apps_older_setting() {
        let (c, _) = Common::parse("dark_theme = \"Nord\"\n").unwrap();
        assert_eq!(
            c.theme_choice(Some(("light", "Dracula", "Solarized Light"))),
            ("light".into(), "Nord".into(), "Solarized Light".into())
        );
        assert_eq!(Common::default().theme_choice(None), ("dark".into(), "tsumugi Dark".into(), "tsumugi Light".into()));
        let old = Clock { weekday: false, ..Clock::default() };
        assert_eq!(Common::default().clock_or(Some(&old)), old, "moved over from the app's settings");
        let (c, _) = Common::parse("[clock]\ndate = false\n").unwrap();
        assert!(!c.clock_or(Some(&old)).date && c.clock_or(Some(&old)).weekday, "common.toml's own, whole");
    }

    #[test]
    fn a_change_keeps_the_rest_of_the_file() {
        let text = "# mine\nlanguage = \"ja\" # here\n\n[clock]\nshow = true\n";
        let change = CommonChange { table: None, key: "language", value: quote("en") };
        assert_eq!(change.apply(text).unwrap(), "# mine\nlanguage = \"en\" # here\n\n[clock]\nshow = true\n");
        let t = set_key(text, Some("clock"), "hour24", "false").unwrap();
        assert!(t.ends_with("[clock]\nshow = true\nhour24 = false\n"), "{t}");
        assert_eq!(set_key("", Some("clock"), "date", "true").unwrap(), "[clock]\ndate = true\n");
        assert_eq!(remove_key(&t, Some("clock"), "hour24").unwrap(), text);
        assert!(set_key("[broken\n", None, "theme", &quote("x")).is_err(), "a file that does not read is not rewritten");
        let blocks = vec![vec![("name", quote("a")), ("addr", quote("127.0.0.1:1"))]];
        let t = set_tables("x = 1\n", "host", &blocks).unwrap();
        assert_eq!(t, "x = 1\n\n[[host]]\nname = \"a\"\naddr = \"127.0.0.1:1\"\n");
        assert_eq!(set_tables(&t, "host", &[]).unwrap(), "x = 1\n");
    }

    #[test]
    fn common_toml_is_edited_whole_and_watched() {
        let base = dir("edit");
        edit_common(&base, |t| set_key(t, None, "theme", &quote("Nord"))).unwrap();
        assert_eq!(Common::read(&base).unwrap().0.theme.as_deref(), Some("Nord"));
        assert!(!base.join("common.toml.uchmk-new").exists(), "nothing left beside it");
        std::fs::write(common_path(&base), [0xFF, 0xFE, b'a', 0]).unwrap();
        assert!(edit_common(&base, |t| Ok(t.to_owned())).is_err(), "not text: refused, not taken as empty");
        let (tx, rx) = std::sync::mpsc::channel();
        watch(vec![common_path(&base)], Duration::from_millis(20), move |changed| tx.send(changed.to_vec()).is_ok());
        std::thread::sleep(Duration::from_millis(60));
        std::fs::remove_file(common_path(&base)).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), vec![common_path(&base)]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(feature = "clock")]
    #[test]
    fn the_clock_says_the_weekday_in_the_language() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap().and_hms_opt(14, 32, 10).unwrap();
        let c = Clock::default();
        assert_eq!(c.text(&now, "en"), "2026/10/05 (Mon) 14:32");
        assert_eq!(c.text(&now, "ja"), "2026/10/05 (月) 14:32");
        let c = Clock { hour24: false, date_format: "DD/MM/YYYY".into(), ..Clock::default() };
        assert_eq!(c.text(&now, "en"), "05/10/2026 (Mon) 2:32 PM");
        assert_eq!(Clock { date: false, ..Clock::default() }.text(&now, "ja"), "14:32");
        assert_eq!(Clock::full(&now, "en"), "Monday, 5 October 2026");
        assert_eq!(Clock::full(&now, "ja"), "2026年10月5日 月曜日");
        assert_eq!(Clock::next_minute(&now), Duration::from_secs(50));
    }
}
