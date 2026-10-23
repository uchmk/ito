//! Where the other uchmk apps are, so one app can start another.
//!
//! Each app tells where it is when it starts ([`register`]: its own exe and
//! version, into `<state>/uchmk/apps/<app>.toml`, which is per machine and not
//! roaming). A person can pin an app somewhere else with `[apps]` in
//! common.toml (`kura = 'C:\tools\kura.exe'`), for a second copy or a portable
//! one. [`find`] looks, in this order:
//!
//! 1. `[apps]` in common.toml ([`Source::Setting`]);
//! 2. what the app registered ([`Source::Registered`]);
//! 3. the folders on `PATH` ([`Source::Path`]).
//!
//! A path that is not a file any more (a build on a RAM disk after a restart,
//! an uninstalled copy) is skipped, so the next place is tried. A build that
//! runs out of a Cargo `target` folder does not register itself: it would
//! take the place of the installed one and be gone with the next clean.
//!
//! Looking touches the disk: [`scan_in_background`] is for a settings screen,
//! and starting the app is `ito_openin::app`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use serde::{Deserialize, Serialize};

/// What an app writes about itself.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
struct Stored {
    exe: String,
    #[serde(default)]
    version: String,
}

/// An app that registered itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub exe: PathBuf,
    pub version: String,
}

/// Where an exe was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `[apps]` in common.toml.
    Setting,
    /// The app wrote it itself.
    Registered,
    /// A folder on `PATH`.
    Path,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub exe: PathBuf,
    pub source: Source,
}

/// An app as a settings screen shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    /// What common.toml pins, as written.
    pub setting: Option<String>,
    /// Whether the pinned path is a file; `false` when nothing is pinned.
    pub setting_ok: bool,
    /// Where it is started from, if anywhere.
    pub found: Option<Found>,
    /// The version it registered.
    pub version: Option<String>,
}

/// One change to `[apps]` in common.toml: pin `name` to a path, or (`None`)
/// let it be found again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub name: String,
    pub exe: Option<String>,
}

/// An app's name as a file name and a key: lower case letters, digits, `-`
/// and `_`. Anything else (a path, `code.cmd`) is not an uchmk app.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The folder the apps write themselves into.
pub fn dir() -> Option<PathBuf> {
    crate::state_dir("apps")
}

/// Whether `exe` is a build in a Cargo output folder (`target`, or the
/// RAM disk's `cargo-target`), which an app does not register.
pub fn is_build_output(exe: &Path) -> bool {
    exe.components().any(|c| matches!(c.as_os_str().to_str(), Some("target" | "cargo-target")))
}

/// Tell the others where `app` is: this process's exe and `version`. Call it
/// once at the start, best effort (`Err` is only worth logging). `Ok(false)`
/// when nothing was written: the file already says so, this is a build in a
/// Cargo `target` folder, or a `--keys` script is running.
pub fn register(app: &str, version: &str) -> Result<bool, String> {
    if crate::is_scripted() {
        return Ok(false);
    }
    let exe = std::env::current_exe().map_err(|e| format!("own exe: {e}"))?;
    let dir = dir().ok_or("no state folder")?;
    register_in(&dir, app, version, &exe)
}

/// [`register`] for a given `exe`, in the folder `dir`.
pub fn register_in(dir: &Path, app: &str, version: &str, exe: &Path) -> Result<bool, String> {
    if !valid_name(app) {
        return Err(format!("`{app}` is not an app name (lower case letters, digits, - and _)"));
    }
    if is_build_output(exe) {
        return Ok(false);
    }
    let now = Stored { exe: exe.to_string_lossy().into_owned(), version: version.to_owned() };
    let file = dir.join(format!("{app}.toml"));
    if read_stored(&file).as_ref() == Some(&now) {
        return Ok(false);
    }
    let text = toml::to_string_pretty(&now).map_err(|e| e.to_string())?;
    crate::write_atomic(&file, text)?;
    Ok(true)
}

fn read_stored(file: &Path) -> Option<Stored> {
    toml::from_str(&std::fs::read_to_string(file).ok()?).ok()
}

/// Every app that registered in `dir`, by name.
pub fn registered_in(dir: &Path) -> BTreeMap<String, Entry> {
    let Ok(read) = std::fs::read_dir(dir) else { return BTreeMap::new() };
    read.flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.strip_suffix(".toml")?.to_owned();
            let s = read_stored(&path).filter(|_| valid_name(&name))?;
            Some((name, Entry { exe: PathBuf::from(s.exe), version: s.version }))
        })
        .collect()
}

/// `[apps]` of the common.toml in the `uchmk` folder (empty when there is
/// none, or it does not read).
pub fn settings() -> BTreeMap<String, String> {
    crate::base_dir().and_then(|b| crate::Common::read(&b).ok()).map(|(c, _)| c.apps).unwrap_or_default()
}

/// `path`, or `path` with the OS's exe suffix, when that is a file.
fn file_at(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_owned());
    }
    let with = PathBuf::from(format!("{}{}", path.display(), std::env::consts::EXE_SUFFIX));
    (!std::env::consts::EXE_SUFFIX.is_empty() && with.is_file()).then_some(with)
}

fn on_path(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    dirs.iter().map(|d| d.join(&file)).find(|p| p.is_file())
}

/// [`find`] over given places: common.toml's `[apps]`, the folder the apps
/// register in, and the `PATH` folders.
pub fn find_with(app: &str, settings: &BTreeMap<String, String>, registry: &Path, path_dirs: &[PathBuf]) -> Option<Found> {
    if !valid_name(app) {
        return None;
    }
    pinned_with(app, settings, registry).or_else(|| on_path(app, path_dirs).map(|exe| Found { exe, source: Source::Path }))
}

/// [`find`] without `PATH`: only what a person pinned or the app registered.
pub fn pinned_with(app: &str, settings: &BTreeMap<String, String>, registry: &Path) -> Option<Found> {
    if !valid_name(app) {
        return None;
    }
    let pinned = settings.get(app).map(|s| s.trim()).filter(|s| !s.is_empty()).and_then(|s| file_at(Path::new(s)));
    if let Some(exe) = pinned {
        return Some(Found { exe, source: Source::Setting });
    }
    let own = read_stored(&registry.join(format!("{app}.toml"))).and_then(|s| file_at(Path::new(&s.exe)));
    own.map(|exe| Found { exe, source: Source::Registered })
}

/// Where `app` is: the pinned path, else the registered one, else on `PATH`;
/// `None` when it is nowhere.
pub fn find(app: &str) -> Option<Found> {
    let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    find_with(app, &settings(), &dir()?, &dirs)
}

/// [`pinned_with`] on this machine's settings and registry.
pub fn pinned(app: &str) -> Option<Found> {
    pinned_with(app, &settings(), &dir()?)
}

/// The apps to show: every one pinned or registered, with where each is
/// started from.
pub fn scan_with(settings: &BTreeMap<String, String>, registry: &Path, path_dirs: &[PathBuf]) -> Vec<Row> {
    let regs = registered_in(registry);
    let mut names: Vec<&String> = settings.keys().chain(regs.keys()).filter(|n| valid_name(n)).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let setting = settings.get(name).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
            let setting_ok = setting.as_deref().is_some_and(|s| file_at(Path::new(s)).is_some());
            Row {
                found: find_with(name, settings, registry, path_dirs),
                version: regs.get(name).map(|e| e.version.clone()).filter(|v| !v.is_empty()),
                name: name.clone(),
                setting,
                setting_ok,
            }
        })
        .collect()
}

/// [`scan_with`] on this machine, from `settings` (common.toml's `[apps]`).
pub fn scan(settings: &BTreeMap<String, String>) -> Vec<Row> {
    let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    match dir() {
        Some(registry) => scan_with(settings, &registry, &dirs),
        None => Vec::new(),
    }
}

/// [`scan`] on a thread of its own; the rows arrive once on the receiver.
pub fn scan_in_background(settings: BTreeMap<String, String>) -> mpsc::Receiver<Vec<Row>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(scan(&settings));
    });
    rx
}

/// common.toml's `text` with the edits made, one key of `[apps]` each (the
/// rest of the file stays). Hand it to [`crate::edit_common`].
pub fn write_edits(text: &str, edits: &[Edit]) -> Result<String, String> {
    let mut out = text.to_owned();
    for e in edits {
        if !valid_name(&e.name) {
            return Err(format!("`{}` is not an app name (lower case letters, digits, - and _)", e.name));
        }
        out = match e.exe.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(exe) => crate::set_key(&out, Some("apps"), &e.name, &crate::quote(exe))?,
            None => crate::remove_key(&out, Some("apps"), &e.name)?,
        };
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-apps-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn names_are_plain() {
        assert!(valid_name("kura") && valid_name("my-app_2"));
        assert!(!valid_name("") && !valid_name("Kura") && !valid_name("a/b") && !valid_name("code.cmd") && !valid_name(".."));
    }

    #[test]
    fn a_registration_is_written_once_and_read_back() {
        let d = scratch("reg");
        let exe = d.join("bin").join("kura.exe");
        assert_eq!(register_in(&d.join("apps"), "kura", "1.2.3", &exe), Ok(true));
        assert_eq!(register_in(&d.join("apps"), "kura", "1.2.3", &exe), Ok(false));
        assert_eq!(register_in(&d.join("apps"), "kura", "1.2.4", &exe), Ok(true));
        let got = registered_in(&d.join("apps"));
        assert_eq!(got["kura"], Entry { exe, version: "1.2.4".into() });
        assert!(register_in(&d.join("apps"), "../x", "1", Path::new("a")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_cargo_build_does_not_register() {
        let d = scratch("dev");
        for p in ["/w/kura/target/release/kura.exe", r"R:\cargo-target\kura\release\kura.exe"] {
            assert!(is_build_output(Path::new(p)), "{p}");
            assert_eq!(register_in(&d, "kura", "1", Path::new(p)), Ok(false));
        }
        assert!(!is_build_output(Path::new("/home/me/.cargo/bin/kura")));
        assert!(registered_in(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_setting_beats_the_registration_beats_path() {
        let d = scratch("find");
        let (pinned, own, onpath) = (d.join("pin").join("kura"), d.join("own").join("kura"), d.join("bin"));
        let (pinned, own) = (
            PathBuf::from(format!("{}{}", pinned.display(), std::env::consts::EXE_SUFFIX)),
            PathBuf::from(format!("{}{}", own.display(), std::env::consts::EXE_SUFFIX)),
        );
        let (reg, dirs) = (d.join("apps"), vec![onpath.clone()]);
        let set = BTreeMap::from([("kura".to_owned(), pinned.display().to_string())]);
        let none = BTreeMap::new();
        assert_eq!(find_with("kura", &none, &reg, &dirs), None);

        touch(&onpath.join(format!("kura{}", std::env::consts::EXE_SUFFIX)));
        assert_eq!(find_with("kura", &none, &reg, &dirs).map(|f| f.source), Some(Source::Path));

        touch(&own);
        register_in(&reg, "kura", "1", &own).unwrap();
        assert_eq!(find_with("kura", &none, &reg, &dirs), Some(Found { exe: own.clone(), source: Source::Registered }));
        // The setting names a file that is not there: the next place is used.
        assert_eq!(find_with("kura", &set, &reg, &dirs).map(|f| f.source), Some(Source::Registered));

        touch(&pinned);
        assert_eq!(find_with("kura", &set, &reg, &dirs), Some(Found { exe: pinned.clone(), source: Source::Setting }));
        // `pinned_with` leaves PATH out.
        assert_eq!(pinned_with("kura", &none, &d.join("nothing")), None);
        // A name that is not an app name finds nothing, whatever is on PATH.
        assert_eq!(find_with("../kura", &none, &reg, &dirs), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_registration_that_is_gone_is_skipped() {
        let d = scratch("gone");
        register_in(&d, "kura", "1", &d.join("vanished").join("kura.exe")).unwrap();
        assert_eq!(pinned_with("kura", &BTreeMap::new(), &d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_rows_hold_what_is_pinned_registered_and_found() {
        let d = scratch("rows");
        let reg = d.join("apps");
        let michi = d.join("michi.bin");
        touch(&michi);
        register_in(&reg, "michi", "0.5.0", &michi).unwrap();
        let set = BTreeMap::from([("kura".to_owned(), d.join("missing").display().to_string()), ("Bad Name".to_owned(), "x".to_owned())]);
        let rows = scan_with(&set, &reg, &[]);
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["kura", "michi"]);
        assert!(rows[0].setting.is_some() && !rows[0].setting_ok && rows[0].found.is_none());
        assert_eq!(rows[1].version.as_deref(), Some("0.5.0"));
        assert_eq!(rows[1].found.as_ref().map(|f| f.source), Some(Source::Registered));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn edits_pin_and_unpin_one_key_and_keep_the_rest() {
        let text = "# mine\ntheme = \"dark\" # keep\n";
        let out = write_edits(text, &[Edit { name: "kura".into(), exe: Some(r"C:\tools\kura.exe".into()) }]).unwrap();
        assert!(out.starts_with("# mine\ntheme = \"dark\" # keep\n"), "{out}");
        let (c, w) = crate::Common::parse(&out).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.apps["kura"], r"C:\tools\kura.exe");
        let out = write_edits(&out, &[Edit { name: "kura".into(), exe: None }]).unwrap();
        assert!(crate::Common::parse(&out).unwrap().0.apps.is_empty());
        assert!(write_edits(text, &[Edit { name: "No Good".into(), exe: Some("x".into()) }]).is_err());
    }
}
