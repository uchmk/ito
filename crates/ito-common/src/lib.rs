//! What every uchmk app (tsumugi, yagura, kura and those to come) shares
//! (docs/common-spec.md): where the settings live, `common.toml` -- the
//! language, the theme and the clock, so changing that one file changes
//! every app at once -- and editing a TOML file one key at a time with what
//! a person wrote around it kept.
//!
//! ```text
//! <config dir>/uchmk/common.toml        language, theme, dark_theme, light_theme, scale, [clock]
//! <config dir>/uchmk/<app>/config.toml  each app's own (`<APP>_CONFIG_HOME` moves the folder)
//! <config dir>/uchmk/themes/*.toml      themes of one's own
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

pub mod fonts;
mod os_theme;

pub use os_theme::OsLight;

/// The `uchmk` folder: `UCHMK_CONFIG_DIR` when set; else `%APPDATA%\uchmk`
/// on Windows, `~/Library/Application Support/uchmk` on macOS,
/// `$XDG_CONFIG_HOME/uchmk` (or `~/.config/uchmk`) elsewhere.
pub fn base_dir() -> Option<PathBuf> {
    let var = env_dir;
    if let Some(d) = var("UCHMK_CONFIG_DIR") {
        return Some(d);
    }
    let dir = if cfg!(windows) {
        var("APPDATA")?
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library").join("Application Support")
    } else {
        xdg("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))?
    };
    Some(dir.join("uchmk"))
}

/// The folder an environment variable names; `None` when it is not set or
/// is empty (an exported but blank variable means unset, not `/`).
pub fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// An XDG folder variable; a relative one is ignored, as the XDG spec says.
fn xdg(var: &str) -> Option<PathBuf> {
    env_dir(var).filter(|d| d.is_absolute())
}

/// The folder of `app`'s own settings (`config.toml`, `keymap.toml` …):
/// `<APP>_CONFIG_HOME` when set, else `<uchmk>/<app>`.
pub fn app_dir(app: &str) -> Option<PathBuf> {
    env_dir(&format!("{}_CONFIG_HOME", app.to_ascii_uppercase())).or_else(|| Some(base_dir()?.join(app)))
}

/// The folder of what `app` keeps between runs (sessions, history, the
/// window's place), apart from its settings: `<APP>_STATE_HOME` when set;
/// else `%LOCALAPPDATA%\uchmk\<app>` on Windows (not roaming),
/// `~/Library/Application Support/uchmk/<app>/state` on macOS (where the
/// settings folder is too), `$XDG_STATE_HOME/uchmk/<app>` (or
/// `~/.local/state/uchmk/<app>`) elsewhere.
pub fn state_dir(app: &str) -> Option<PathBuf> {
    if let Some(d) = env_dir(&format!("{}_STATE_HOME", app.to_ascii_uppercase())) {
        return Some(d);
    }
    let var = env_dir;
    Some(if cfg!(windows) {
        var("LOCALAPPDATA")?.join("uchmk").join(app)
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library").join("Application Support").join("uchmk").join(app).join("state")
    } else {
        xdg("XDG_STATE_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("state")))?.join("uchmk").join(app)
    })
}

/// Move what an older build kept at `old` (a file or a folder) to `new`, so
/// an upgrade keeps its settings: only when `old` is there and `new` is not,
/// so nothing is merged or written over. `Ok(true)` when it moved. Run it
/// once at the start, before anything reads the settings.
pub fn move_old(old: &Path, new: &Path) -> Result<bool, String> {
    if !old.exists() || new.exists() {
        return Ok(false);
    }
    if let Some(dir) = new.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::rename(old, new).map_err(|e| format!("could not move {} to {}: {e}", old.display(), new.display()))?;
    Ok(true)
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

/// The smallest and the largest `scale`, the size the whole window is drawn
/// at (1.0: the app's own), and how far one key press moves it.
pub const SCALE_MIN: f32 = 0.2;
pub const SCALE_MAX: f32 = 5.0;
pub const SCALE_STEP: f32 = 0.1;

/// Which way a key moves the scale: `<C-=>`, `<C-->`, `<C-0>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleTo {
    In,
    Out,
    Reset,
}

/// A scale kept in range and to one decimal place, so ten steps up and ten
/// down land where they began.
pub fn tidy_scale(s: f32) -> f32 {
    if !s.is_finite() {
        return 1.0;
    }
    (s.clamp(SCALE_MIN, SCALE_MAX) * 10.0).round() / 10.0
}

/// `now` one step bigger or smaller, or back at 1.0.
pub fn scale_step(now: f32, to: ScaleTo) -> f32 {
    tidy_scale(match to {
        ScaleTo::In => now + SCALE_STEP,
        ScaleTo::Out => now - SCALE_STEP,
        ScaleTo::Reset => 1.0,
    })
}

/// A scale written as text -- `1.2` or `120%` -- as a number in range;
/// `None` for anything else. For an environment variable such as
/// `KURA_SCALE` that sets the scale for one run.
pub fn parse_scale(text: &str) -> Option<f32> {
    let text = text.trim();
    let (num, per) = match text.strip_suffix('%') {
        Some(n) => (n.trim(), 100.0),
        None => (text, 1.0),
    };
    let n = num.parse::<f32>().ok().map(|n| n / per).filter(|n| n.is_finite() && (SCALE_MIN..=SCALE_MAX).contains(n))?;
    Some(tidy_scale(n))
}

/// What a key that moved the scale says: `Scale 120%`, with `(maximum)` or
/// `(minimum)` at the ends; in Japanese 「倍率 120%（最大）」.
pub fn scale_text(s: f32, lang: &str) -> String {
    let percent = (s * 100.0).round() as i32;
    let end = |max: &'static str, min: &'static str| {
        if s >= SCALE_MAX {
            max
        } else if s <= SCALE_MIN {
            min
        } else {
            ""
        }
    };
    match lang {
        "ja" => format!("倍率 {percent}%{}", end("（最大）", "（最小）")),
        _ => format!("Scale {percent}%{}", end(" (maximum)", " (minimum)")),
    }
}

/// The change that writes `s` as common.toml's `scale`.
pub fn scale_change(s: f32) -> CommonChange {
    CommonChange { table: None, key: "scale", value: format!("{:.1}", tidy_scale(s)) }
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
    /// The size the whole window is drawn at, from [`SCALE_MIN`] to
    /// [`SCALE_MAX`]; the keys `<C-=>` `<C-->` `<C-0>` write it.
    pub scale: Option<f32>,
    pub clock: Option<Clock>,
}

/// The keys `common.toml` has.
const KEYS: [&str; 6] = ["language", "theme", "dark_theme", "light_theme", "scale", "clock"];

impl Common {
    /// The settings in common.toml's `text`, and what in it is not known
    /// (a key no app reads, a date format not in [`DATE_FORMATS`]) to show
    /// as a warning. A value of the wrong kind is a warning too, and that key
    /// is taken as not set. `Err` only when the file does not read as TOML.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        let t = text.parse::<toml::Table>().map_err(|e| format!("common.toml: {e}"))?;
        let mut warnings = Vec::new();
        for k in t.keys().filter(|k| !KEYS.contains(&k.as_str())) {
            warnings.push(format!("common.toml: `{k}` is not a setting uchmk's apps read"));
        }
        let text_of = |key: &str, warnings: &mut Vec<String>| match t.get(key) {
            None => None,
            Some(toml::Value::String(s)) => Some(s.clone()),
            Some(_) => {
                warnings.push(format!("common.toml: `{key}` must be a string, so it is not used"));
                None
            }
        };
        let language = text_of("language", &mut warnings);
        let theme = text_of("theme", &mut warnings);
        let dark_theme = text_of("dark_theme", &mut warnings);
        let light_theme = text_of("light_theme", &mut warnings);
        let scale = match t.get("scale") {
            None => None,
            Some(v) => 'n: {
                let n = match v {
                    toml::Value::Integer(i) => *i as f32,
                    toml::Value::Float(f) => *f as f32,
                    _ => {
                        warnings.push("common.toml: `scale` must be a number (1.0 is the app's own size), so it is not used".to_owned());
                        break 'n None;
                    }
                };
                if !(SCALE_MIN..=SCALE_MAX).contains(&n) {
                    warnings.push(format!("common.toml: scale {n} is not from {SCALE_MIN} to {SCALE_MAX}, so {} is used", tidy_scale(n)));
                }
                Some(tidy_scale(n))
            }
        };
        let clock = match t.get("clock") {
            None => None,
            Some(toml::Value::Table(c)) => {
                let known = ["show", "hour24", "date", "date_format", "weekday"];
                let mut kept = toml::Table::new();
                for (k, v) in c {
                    if !known.contains(&k.as_str()) {
                        warnings.push(format!("common.toml: `clock.{k}` is not a setting uchmk's apps read"));
                        continue;
                    }
                    // Each key on its own, so one of the wrong kind leaves the rest.
                    let one = toml::Table::from_iter([(k.clone(), v.clone())]);
                    match Clock::deserialize(toml::Value::Table(one)) {
                        Ok(_) => {
                            kept.insert(k.clone(), v.clone());
                        }
                        Err(e) => warnings.push(format!("common.toml: clock.{k}: {}, so it is not used", e.message())),
                    }
                }
                let mut clock = Clock::deserialize(toml::Value::Table(kept)).unwrap_or_default();
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
            Some(_) => {
                warnings.push("common.toml: `clock` must be a table ([clock]), so it is not used".to_owned());
                None
            }
        };
        let common = Self { language, theme, dark_theme, light_theme, scale, clock };
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

    /// The scale: common.toml's, else 1.0.
    pub fn scale_or_one(&self) -> f32 {
        self.scale.unwrap_or(1.0)
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

/// `text` with the `[[name]]` blocks made `blocks`, each a list of keys and
/// their values as TOML. A block keeps the old table it matches -- the same
/// `name`, else the same `addr`, else the next one left over -- with its
/// place and comments: only the values that changed are written (a comment
/// after one stays), keys no longer there are taken out. A table nothing
/// matches is taken out, and a new one goes after the last of them.
pub fn set_tables(text: &str, name: &str, blocks: &[Vec<(&str, String)>]) -> Result<String, String> {
    let mut doc = document(text)?;
    let (parent, last) = match name.rsplit_once('.') {
        Some((p, l)) => (Some(p), l),
        None => (None, name),
    };
    let mut want = Vec::new();
    for b in blocks {
        let mut keys = Vec::new();
        for (k, v) in b {
            let v: toml_edit::Value = v.parse().map_err(|e| format!("`{v}` is not a TOML value: {e}"))?;
            keys.push((key_name(k)?, v));
        }
        want.push(keys);
    }
    let t = table_at(&mut doc, parent)?;
    let old: Vec<toml_edit::Table> = match t.remove(last) {
        Some(toml_edit::Item::ArrayOfTables(a)) => a.into_iter().collect(),
        _ => Vec::new(),
    };
    let pick = match_tables(&old, &want);
    // The places the kept tables had, handed out again in the new order, so
    // the file shows them in the order of `blocks`.
    let mut places: Vec<isize> = pick.iter().flatten().filter_map(|&j| old[j].position()).collect();
    places.sort_unstable();
    let mut places = places.into_iter();
    let mut at = old.iter().filter_map(toml_edit::Table::position).max();
    let mut list = toml_edit::ArrayOfTables::new();
    for (keys, p) in want.into_iter().zip(pick.iter().copied()) {
        let table = match p {
            Some(j) => {
                let mut kept = old[j].clone();
                if let Some(place) = places.next() {
                    kept.set_position(place);
                    at = Some(place);
                }
                kept.retain(|k, _| keys.iter().any(|(name, _)| name == k));
                for (k, mut v) in keys {
                    match kept.get_mut(&k).and_then(toml_edit::Item::as_value_mut) {
                        Some(was) if same_value(was, &v) => {}
                        Some(was) => {
                            let suffix = was.decor().suffix().and_then(|s| s.as_str()).filter(|s| s.contains('#')).map(str::to_owned);
                            v.decor_mut().set_prefix(" ");
                            v.decor_mut().set_suffix(suffix.unwrap_or_default());
                            *was = v;
                        }
                        None => {
                            kept.insert(&k, toml_edit::Item::Value(v));
                        }
                    }
                }
                kept
            }
            None => {
                let mut new = toml_edit::Table::new();
                if let Some(place) = at {
                    new.set_position(place);
                }
                for (k, v) in keys {
                    new.insert(&k, toml_edit::Item::Value(v));
                }
                new
            }
        };
        list.push(table);
    }
    if !list.is_empty() {
        t.insert(last, toml_edit::Item::ArrayOfTables(list));
    }
    // Comments above a table taken out that stand apart from it (a blank
    // line between) are about the file, so they go to what comes next.
    let mut taken: Vec<(isize, String)> = (0..old.len())
        .filter(|j| !pick.contains(&Some(*j)))
        .filter_map(|j| Some((old[j].position()?, apart(old[j].decor().prefix()?.as_str()?)?)))
        .collect();
    taken.sort_by_key(|&(p, _)| std::cmp::Reverse(p));
    for (place, text) in taken {
        let mut next: Option<&mut toml_edit::Table> = None;
        after(doc.as_table_mut(), place, &mut next);
        match next {
            Some(n) => {
                let was = n.decor().prefix().and_then(|s| s.as_str()).unwrap_or("").trim_start_matches('\n').to_owned();
                n.decor_mut().set_prefix(format!("{text}\n{was}"));
            }
            None => {
                let end = doc.trailing().as_str().unwrap_or("").to_owned();
                doc.set_trailing(format!("{end}\n{text}"));
            }
        }
    }
    Ok(doc.to_string())
}

/// The lines of a table's `prefix` up to its last blank line, when they hold
/// a comment.
fn apart(prefix: &str) -> Option<String> {
    let lines: Vec<&str> = prefix.split_inclusive('\n').collect();
    let blank = lines.iter().rposition(|l| l.trim().is_empty() && l.ends_with('\n'))?;
    let text: String = lines[..=blank].concat();
    let text = text.trim_start_matches('\n');
    text.contains('#').then(|| format!("{}\n", text.trim_end_matches('\n')))
}

/// The table in `t` that is shown first after the place `place`.
fn after<'a>(t: &'a mut toml_edit::Table, place: isize, best: &mut Option<&'a mut toml_edit::Table>) {
    for (_, item) in t.iter_mut() {
        match item {
            toml_edit::Item::Table(sub) => {
                consider(sub, place, best);
            }
            toml_edit::Item::ArrayOfTables(a) => {
                for sub in a.iter_mut() {
                    consider(sub, place, best);
                }
            }
            _ => {}
        }
    }
}

fn consider<'a>(sub: &'a mut toml_edit::Table, place: isize, best: &mut Option<&'a mut toml_edit::Table>) {
    let p = sub.position().filter(|&p| p > place && !sub.is_implicit());
    if p.is_some_and(|p| best.as_ref().and_then(|b| b.position()).is_none_or(|b| p < b)) {
        *best = Some(sub);
    } else {
        after(sub, place, best);
    }
}

/// Which of the `old` tables each wanted block keeps: the same `name`, else
/// the same `addr`, else the next one no block has taken.
fn match_tables(old: &[toml_edit::Table], want: &[Vec<(String, toml_edit::Value)>]) -> Vec<Option<usize>> {
    let mut used = vec![false; old.len()];
    let mut pick = vec![None; want.len()];
    for key in ["name", "addr"] {
        for (i, keys) in want.iter().enumerate() {
            let Some(w) = keys.iter().find(|(k, _)| k == key).and_then(|(_, v)| v.as_str()) else { continue };
            if pick[i].is_some() {
                continue;
            }
            if let Some(j) = (0..old.len()).find(|&j| !used[j] && old[j].get(key).and_then(toml_edit::Item::as_str) == Some(w)) {
                used[j] = true;
                pick[i] = Some(j);
            }
        }
    }
    let mut free = (0..old.len()).filter(|&j| !used[j]).collect::<Vec<_>>().into_iter();
    for p in pick.iter_mut().filter(|p| p.is_none()) {
        *p = free.next();
    }
    pick
}

/// Whether two TOML values are the same, however they are written (`'a'` and `"a"`).
fn same_value(a: &toml_edit::Value, b: &toml_edit::Value) -> bool {
    let read = |v: &toml_edit::Value| {
        let mut v = v.clone();
        v.decor_mut().clear();
        format!("v = {v}").parse::<toml::Table>().ok()
    };
    read(a).is_some_and(|a| Some(a) == read(b))
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

    /// The folders follow `<APP>_CONFIG_HOME` and `<APP>_STATE_HOME` (an app
    /// name no other test uses, so no test races on the variables).
    #[test]
    fn the_app_folders_follow_their_variables() {
        std::env::set_var("ITOTESTAPP_STATE_HOME", "/s/t");
        std::env::set_var("ITOTESTAPP_CONFIG_HOME", "");
        assert_eq!(state_dir("itotestapp"), Some(PathBuf::from("/s/t")));
        // Empty is the same as unset: the folder under uchmk's.
        if let Some(d) = app_dir("itotestapp") {
            assert!(d.ends_with("itotestapp"), "{d:?}");
        }
        std::env::remove_var("ITOTESTAPP_STATE_HOME");
        if let Some(d) = state_dir("itotestapp") {
            assert!(d.ends_with(if cfg!(target_os = "macos") { "itotestapp/state" } else { "itotestapp" }), "{d:?}");
            assert!(d.components().any(|c| c.as_os_str() == "uchmk"), "{d:?}");
        }
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
        assert!(Common::parse("[broken\n").is_err());
        let (c, warnings) = Common::parse("scale = \"x\"\ntheme = 3\nlanguage = \"ja\"\n").unwrap();
        assert_eq!(warnings.len(), 2, "every key of the wrong kind is said, not just the first: {warnings:?}");
        assert_eq!((c.scale, c.theme, c.language.as_deref()), (None, None, Some("ja")), "the wrong ones are not set, the rest are");
        let (c, warnings) = Common::parse("[clock]\nshow = \"yes\"\nhour24 = false\n").unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(c.clock, Some(Clock { hour24: false, ..Clock::default() }), "the clock keeps the keys that read");
        let (c, warnings) = Common::parse("clock = 1\n").unwrap();
        assert_eq!((c.clock, warnings.len()), (None, 1));
        assert_eq!(Common::read(&dir("none")), Ok((Common::default(), Vec::new())), "no file: nothing set");
    }

    #[test]
    fn the_scale_is_read_stepped_and_written() {
        assert_eq!(Common::parse("scale = 1.25\n").unwrap(), (Common { scale: Some(1.3), ..Common::default() }, Vec::new()));
        assert_eq!(Common::parse("scale = 2\n").unwrap().0.scale, Some(2.0), "a whole number is a number too");
        let (c, warnings) = Common::parse("scale = 9.0\n").unwrap();
        assert_eq!((c.scale, warnings.len()), (Some(SCALE_MAX), 1), "too big: the largest, and said");
        assert_eq!(Common::parse("scale = \"big\"\n").unwrap().0.scale, None, "not a number: not set");
        assert_eq!(Common::default().scale_or_one(), 1.0);
        let mut s = 1.0;
        for _ in 0..10 {
            s = scale_step(s, ScaleTo::In);
        }
        assert_eq!(s, 2.0, "ten steps make a whole one, with no drift");
        assert_eq!(scale_step(SCALE_MAX, ScaleTo::In), SCALE_MAX);
        assert_eq!(scale_step(SCALE_MIN, ScaleTo::Out), SCALE_MIN);
        assert_eq!(scale_step(3.3, ScaleTo::Reset), 1.0);
        assert_eq!(parse_scale(" 1.5 "), Some(1.5));
        assert_eq!(parse_scale("150%"), Some(1.5));
        assert_eq!(parse_scale("0.1"), None);
        assert_eq!(parse_scale("NaN"), None);
        assert_eq!(parse_scale("big"), None);
        assert_eq!(scale_text(1.2, "en"), "Scale 120%");
        assert_eq!(scale_text(SCALE_MAX, "en"), "Scale 500% (maximum)");
        assert_eq!(scale_text(SCALE_MIN, "ja"), "倍率 20%（最小）");
        assert_eq!(scale_change(1.25).apply("language = \"ja\"\n").unwrap(), "language = \"ja\"\nscale = 1.3\n");
    }

    #[test]
    fn an_older_place_is_moved_once_and_never_over_the_new() {
        let d = dir("move");
        let (old, new) = (d.join("old").join("settings.toml"), d.join("uchmk").join("app").join("config.toml"));
        assert_eq!(move_old(&old, &new), Ok(false), "nothing there: nothing to do");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "a = 1\n").unwrap();
        assert_eq!(move_old(&old, &new), Ok(true));
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "a = 1\n");
        assert!(!old.exists());
        std::fs::write(&old, "a = 2\n").unwrap();
        assert_eq!(move_old(&old, &new), Ok(false), "both there: both kept");
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "a = 1\n");
        let _ = std::fs::remove_dir_all(&d);
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
    fn tables_are_set_in_place_and_keep_what_a_person_wrote() {
        let text = "# my machines\n# the first two\n\n[[host]]\nname = \"a\" # the first host\naddr = \"10.0.0.1:7313\"\n\n[agent]\nport = 7323\n\n# the box\n[[host]]\nname = \"b\"\naddr = \"10.0.0.2:7313\"\nevery = 5\n";
        let a = vec![("name", quote("a")), ("addr", quote("10.0.0.1:7313"))];
        let b = vec![("name", quote("b")), ("addr", quote("10.0.0.2:7313")), ("every", "5".to_owned())];
        let c = vec![("name", quote("c")), ("addr", quote("10.0.0.3:7313"))];
        assert_eq!(set_tables(text, "host", &[a.clone(), b.clone()]).unwrap(), text, "nothing changed: the same text");
        let added = set_tables(text, "host", &[a.clone(), b.clone(), c.clone()]).unwrap();
        assert_eq!(added, format!("{text}\n[[host]]\nname = \"c\"\naddr = \"10.0.0.3:7313\"\n"), "a new one goes after the last");
        let gone = set_tables(&added, "host", &[a.clone(), c.clone()]).unwrap();
        assert_eq!(
            gone,
            "# my machines\n# the first two\n\n[[host]]\nname = \"a\" # the first host\naddr = \"10.0.0.1:7313\"\n\n[agent]\nport = 7323\n\n[[host]]\nname = \"c\"\naddr = \"10.0.0.3:7313\"\n",
            "the middle one taken out, the others as they were"
        );
        let renamed = vec![("name", quote("alpha")), ("addr", quote("10.0.0.1:7313"))];
        let t = set_tables(text, "host", &[renamed, vec![("name", quote("b")), ("addr", quote("10.0.0.2:7313"))]]).unwrap();
        assert!(t.contains("name = \"alpha\" # the first host\n"), "the comment after a changed value stays: {t}");
        assert!(t.contains("# the box\n[[host]]\nname = \"b\"\naddr = \"10.0.0.2:7313\"\n") && !t.contains("every"), "{t}");
        let swapped = set_tables(text, "host", &[b.clone(), a]).unwrap();
        let read: toml::Table = swapped.parse().unwrap();
        let names: Vec<_> = read["host"].as_array().unwrap().iter().map(|h| h["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["b", "a"], "the file reads in the new order: {swapped}");
        let none = set_tables(text, "host", &[]).unwrap();
        assert_eq!(none, "# my machines\n# the first two\n\n[agent]\nport = 7323\n");
        let first = set_tables(text, "host", std::slice::from_ref(&b)).unwrap();
        assert!(first.starts_with("# my machines\n# the first two\n\n[agent]\n"), "the file's own comments stay: {first}");
        assert_eq!(set_tables("x = 1\n\n# head\n\n[[host]]\nname = \"a\"\n", "host", &[]).unwrap(), "x = 1\n\n# head\n");
        let quoted = "[[host]]\nname = 'a'\n";
        assert_eq!(set_tables(quoted, "host", &[vec![("name", quote("a"))]]).unwrap(), quoted, "written another way, the same value");
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
