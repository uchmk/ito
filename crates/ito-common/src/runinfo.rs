//! What the last run actually used, written down so `<app> env` and a bug
//! report can say it.
//!
//! The adapter and backend wgpu ended up on, the fonts that were loaded and the
//! scale the window came out at are only knowable with a window open, while
//! `<app> env` exits without one (initialising wgpu in a diagnostic is slow and
//! the thing most likely to be broken on the machine being diagnosed). So the
//! run that knows writes `last-run.toml` into the state folder, and the
//! diagnostic reads it back. It reports the *last* run, which is the right
//! one: the run that went wrong, not the one typing `env` afterwards.
//!
//! The fields every app can fill are typed. What only one app knows (kura's
//! pane grid and launches) goes in [`RunInfo::extra`], which keeps its TOML
//! types and survives the round trip.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The file in the state folder.
pub const FILE: &str = "last-run.toml";

// No `Eq`: `ppp` is an `f32`, and the struct is only written and read back.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone)]
pub struct RunInfo {
    /// The version that wrote this, so a stale file cannot be read as current.
    #[serde(default)]
    pub version: String,
    /// The adapter's own name, e.g. `NVIDIA GeForce RTX 4070`.
    #[serde(default)]
    pub adapter: String,
    /// `Vulkan`, `Dx12`, `Gl`, ...
    #[serde(default)]
    pub backend: String,
    /// `DiscreteGpu`, `IntegratedGpu`, `Cpu`, ...; a software fallback is a
    /// finding by itself when the complaint is that it is slow.
    #[serde(default)]
    pub device: String,
    /// The regular faces that were loaded, in the order they are consulted.
    #[serde(default)]
    pub fonts: Vec<PathBuf>,
    /// The bold faces, a separate search and separately absent.
    #[serde(default)]
    pub bold: Vec<PathBuf>,
    /// The window as egui laid it out, in points. `[0.0, 0.0]` means no frame
    /// has been drawn yet. Measuring a window from outside depends on the DPI
    /// awareness of the asker, so the app's own view is written down.
    #[serde(default)]
    pub window_pt: [f32; 2],
    /// Pixels per point on the frame this was written on; `0.0` before one.
    #[serde(default)]
    pub ppp: f32,
    /// When the run that wrote this started, in seconds since 1970; `0` when
    /// unknown.
    #[serde(default)]
    pub started: u64,
    /// What asked for the backend: `[ui] backend = "gl"`, or
    /// `WGPU_BACKEND=vulkan` when the variable won.
    #[serde(default)]
    pub backend_setting: String,
    /// What only this app knows. Keys are the app's own.
    #[serde(default)]
    pub extra: toml::Table,
}

impl RunInfo {
    /// A record for this run, with the time it started.
    pub fn new(version: &str) -> Self {
        let started = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        Self { version: version.to_owned(), started, ..Self::default() }
    }

    /// How long ago the run started, or nothing for a record without it.
    pub fn started_ago(&self, now: SystemTime) -> Option<Duration> {
        if self.started == 0 {
            return None;
        }
        Some(now.duration_since(UNIX_EPOCH + Duration::from_secs(self.started)).unwrap_or_default())
    }

    /// The window as one line: pixels first (what a person measures), with
    /// the points and the scale that made them. `None` before any frame.
    pub fn window_line(&self) -> Option<String> {
        let [w, h] = self.window_pt;
        if self.ppp <= 0.0 || w <= 0.0 || h <= 0.0 {
            return None;
        }
        // `1.5`, not `1.5000001`: the scale is a setting, not a measurement.
        Some(format!("{:.0} x {:.0} px ({:.0} x {:.0} pt @ {})", w * self.ppp, h * self.ppp, w, h, (self.ppp * 1000.0).round() / 1000.0))
    }

    /// `[adapter, backend, device]` for the Rendering row; `None` when no
    /// window has chosen an adapter.
    pub fn rendering(&self) -> Option<[&str; 3]> {
        (!self.adapter.is_empty()).then_some([self.adapter.as_str(), self.backend.as_str(), self.device.as_str()])
    }
}

/// Keep at most `keep` entries, newest last: add `line`, drop the oldest.
pub fn push_recent(list: &mut Vec<String>, line: &str, keep: usize) {
    list.push(line.to_owned());
    let over = list.len().saturating_sub(keep);
    list.drain(..over);
}

/// `last-run.toml` in `app`'s state folder.
pub fn path(app: &str) -> Option<PathBuf> {
    crate::state_dir(app).map(|d| d.join(FILE))
}

/// Write it, best effort: a diagnostic that could stop a program from starting
/// would be worse than none.
pub fn save_to(p: &Path, info: &RunInfo) {
    if let Some(dir) = p.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    if let Ok(text) = toml::to_string_pretty(info) {
        let _ = std::fs::write(p, text);
    }
}

/// What the last run wrote; `None` when the app has not run here, which is
/// worth printing rather than papering over.
pub fn load_from(p: &Path) -> Option<RunInfo> {
    toml::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// [`save_to`] in `app`'s state folder.
pub fn save(app: &str, info: &RunInfo) {
    if let Some(p) = path(app) {
        save_to(&p, info);
    }
}

/// [`load_from`] in `app`'s state folder.
pub fn load(app: &str) -> Option<RunInfo> {
    load_from(&path(app)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_line_gives_pixels_points_and_the_scale() {
        let at = |w: f32, h: f32, ppp: f32| RunInfo { window_pt: [w, h], ppp, ..Default::default() }.window_line();
        assert_eq!(at(1360.0, 860.0, 1.5).as_deref(), Some("2040 x 1290 px (1360 x 860 pt @ 1.5)"));
        assert_eq!(at(1000.0, 500.0, 1.25).as_deref(), Some("1250 x 625 px (1000 x 500 pt @ 1.25)"));
        assert_eq!(RunInfo::default().window_line(), None);
    }

    #[test]
    fn started_ago_and_rendering() {
        let at = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let info = RunInfo { started: 1_800_000_000, ..Default::default() };
        assert_eq!(info.started_ago(at + Duration::from_secs(750)), Some(Duration::from_secs(750)));
        assert_eq!(RunInfo::default().started_ago(at), None);
        assert_eq!(RunInfo::default().rendering(), None);
        let r = RunInfo { adapter: "GPU".into(), backend: "Gl".into(), device: "Cpu".into(), ..Default::default() };
        assert_eq!(r.rendering(), Some(["GPU", "Gl", "Cpu"]));
    }

    #[test]
    fn recent_keeps_the_newest() {
        let mut l = Vec::new();
        for i in 1..=7 {
            push_recent(&mut l, &format!("run {i}"), 5);
        }
        assert_eq!(l, ["run 3", "run 4", "run 5", "run 6", "run 7"]);
    }

    #[test]
    fn it_round_trips_with_extra_and_old_records_load() {
        let dir = std::env::temp_dir().join(format!("ito-runinfo-{}", std::process::id()));
        let p = dir.join(FILE);
        assert_eq!(load_from(&p), None);
        let mut info = RunInfo::new("9.9.9");
        info.fonts = vec![PathBuf::from(r"C:\fonts\A-Regular.ttf")];
        info.window_pt = [1360.0, 860.0];
        info.ppp = 1.5;
        info.extra.insert("pane".into(), toml::Value::Array(vec![12.into(), 159.into()]));
        info.extra.insert("shell".into(), "pwsh".into());
        save_to(&p, &info);
        assert_eq!(load_from(&p), Some(info));
        let old: RunInfo = toml::from_str("version = \"0.58.0\"\nadapter = \"\"\n").unwrap();
        assert_eq!(old.started, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
