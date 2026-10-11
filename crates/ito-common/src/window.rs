//! Where the window was and how big: `window.toml` in the app's state folder
//! ([`crate::state_dir`]), read when the app starts and written when the
//! window has moved or been resized. The apps put it back (position, size,
//! maximized) so a window opens where it was closed.
//!
//! Nothing here knows about egui or winit: the app hands in the numbers it
//! reads from its viewport (logical points) and takes the numbers out.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The window's place. `x` / `y` are the outer position (the corner of the
/// frame) and `w` / `h` the inner size, all in logical points. A maximized
/// window keeps the place it had before, so leaving the maximum goes there.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub maximized: bool,
}

/// The smallest size worth restoring: a window of a few points is a broken
/// number, not a choice.
const MIN_SIDE: f32 = 200.0;
/// The largest, in points (an 8K screen at 100% is 7680).
const MAX_SIDE: f32 = 20000.0;
/// How far from the origin a corner may be: Windows parks a minimized window
/// at -32000, which must never be put back.
const FAR: f32 = 16000.0;

impl Window {
    /// The numbers could be a window someone sees: finite, not tiny, not huge,
    /// not where a minimized window is parked.
    pub fn sane(&self) -> bool {
        let finite = [self.x, self.y, self.w, self.h].iter().all(|v| v.is_finite());
        finite
            && (MIN_SIDE..=MAX_SIDE).contains(&self.w)
            && (MIN_SIDE..=MAX_SIDE).contains(&self.h)
            && self.x.abs() <= FAR
            && self.y.abs() <= FAR
    }

    /// Whether the window's title bar can be reached on a screen `monitor`
    /// wide and high that starts at the origin (the primary one): at least
    /// `grab` points of it inside. Several monitors may put a window at
    /// negative or large positions, so an app that does not know them all
    /// uses this only to refuse what no screen could show.
    pub fn reachable(&self, monitor: (f32, f32), grab: f32) -> bool {
        self.sane() && self.y >= -grab && self.y <= monitor.1 * 4.0 && self.x + self.w >= grab && self.x <= monitor.0 * 4.0
    }
}

/// `window.toml` in `dir`.
pub fn path(dir: &Path) -> PathBuf {
    dir.join("window.toml")
}

/// The saved place, if there is one and it is [`Window::sane`].
pub fn read(dir: &Path) -> Option<Window> {
    let text = std::fs::read_to_string(path(dir)).ok()?;
    toml::from_str::<Window>(&text).ok().filter(Window::sane)
}

/// Save the place; a window that is not [`Window::sane`] is not written.
pub fn write(dir: &Path, w: &Window) -> Result<(), String> {
    if !w.sane() {
        return Ok(());
    }
    let text = toml::to_string(w).map_err(|e| e.to_string())?;
    crate::write_atomic(&path(dir), text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(x: f32, y: f32, w: f32, h: f32) -> Window {
        Window { x, y, w, h, maximized: false }
    }

    #[test]
    fn a_place_round_trips_and_a_broken_one_is_not_kept() {
        let dir = std::env::temp_dir().join(format!("ito-window-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read(&dir), None);
        let w = Window { maximized: true, ..win(120.0, 80.0, 1100.0, 700.0) };
        write(&dir, &w).unwrap();
        assert_eq!(read(&dir), Some(w));
        // A minimized window's parking spot is neither written nor read.
        write(&dir, &win(-32000.0, -32000.0, 160.0, 28.0)).unwrap();
        assert_eq!(read(&dir), Some(w));
        std::fs::write(path(&dir), "x = 1\nw = 5\n").unwrap();
        assert_eq!(read(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_no_screen_could_show_is_refused() {
        assert!(win(0.0, 0.0, 800.0, 600.0).sane());
        assert!(!win(f32::NAN, 0.0, 800.0, 600.0).sane());
        assert!(!win(0.0, 0.0, 50.0, 600.0).sane());
        assert!(!win(-32000.0, -32000.0, 800.0, 600.0).sane());
        let screen = (1920.0, 1080.0);
        assert!(win(-1800.0, 40.0, 1920.0, 1000.0).reachable(screen, 100.0), "a monitor to the left");
        assert!(!win(-3000.0, 40.0, 800.0, 600.0).reachable(screen, 100.0), "left of every screen");
        assert!(!win(100.0, -400.0, 800.0, 600.0).reachable(screen, 100.0), "title bar above the top");
    }
}
