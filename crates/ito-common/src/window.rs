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

    /// Whether the title bar would be on one of `screens`: at least `grab`
    /// points of the top edge across, the top edge no more than `grab` above
    /// the screen's top and at least `grab` above its bottom. The window is
    /// put back at the points times the primary screen's scale (it is made
    /// there before it moves), so that is the scale the place is taken at.
    pub fn seen_on(&self, screens: &[Screen], grab: f32) -> bool {
        let Some(first) = screens.iter().find(|s| s.primary).or(screens.first()) else {
            return false;
        };
        let k = first.scale;
        let (left, right, top, grab) = (self.x * k, (self.x + self.w) * k, self.y * k, grab * k);
        self.sane()
            && screens.iter().any(|s| {
                let across = right.min(s.x + s.w) - left.max(s.x);
                across >= grab && top >= s.y - grab && top <= s.y + s.h - grab
            })
    }

    /// [`Window::seen_on`] the screens there are now, or, where the app
    /// cannot ask for them, [`Window::reachable`] on a `guess` of the
    /// primary screen.
    pub fn reachable_now(&self, guess: (f32, f32), grab: f32) -> bool {
        let screens = screens();
        if screens.is_empty() {
            self.reachable(guess, grab)
        } else {
            self.seen_on(&screens, grab)
        }
    }
}

/// A screen: where it is and how big in pixels (the whole of it, the taskbar
/// too), its points to pixels, and whether it is the primary one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub scale: f32,
    pub primary: bool,
}

/// The screens there are now; empty where the OS is not asked (macOS and
/// Linux) or the answer is broken.
pub fn screens() -> Vec<Screen> {
    sys::screens()
}

#[cfg(windows)]
mod sys {
    use super::Screen;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
    use windows::Win32::UI::HiDpi::{
        GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
    };

    /// `MONITORINFOF_PRIMARY` (in WindowsAndMessaging, not worth the feature).
    const PRIMARY: u32 = 1;

    pub fn screens() -> Vec<Screen> {
        let mut out: Vec<Screen> = Vec::new();
        // The app asks before winit makes it DPI aware, and an unaware
        // thread is told scaled rectangles: ask as an aware one, then put
        // the thread back.
        // SAFETY: the callback only runs inside EnumDisplayMonitors, while
        // `out` is alive, and writes nothing else.
        unsafe {
            let was = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut out as *mut Vec<Screen> as isize));
            // Only null is a failure: `is_invalid` also refuses -1, which is
            // DPI_AWARENESS_CONTEXT_UNAWARE, the one an unaware thread had.
            if !was.0.is_null() {
                SetThreadDpiAwarenessContext(was);
            }
        }
        if out.iter().any(|s| !(s.w > 0.0 && s.h > 0.0 && s.scale > 0.0)) {
            Vec::new()
        } else {
            out
        }
    }

    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `Vec` handed to EnumDisplayMonitors above.
        let out = unsafe { &mut *(data.0 as *mut Vec<Screen>) };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: `info` is a MONITORINFO with its size set.
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            // A screen left out would refuse a place on it: an empty one
            // makes the whole answer broken instead.
            out.push(Screen { x: 0.0, y: 0.0, w: 0.0, h: 0.0, scale: 0.0, primary: false });
            return BOOL(1);
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        // SAFETY: plain out parameters.
        let dpi = if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_ok() { dx } else { 96 };
        let r = info.rcMonitor;
        out.push(Screen {
            x: r.left as f32,
            y: r.top as f32,
            w: (r.right - r.left) as f32,
            h: (r.bottom - r.top) as f32,
            scale: dpi as f32 / 96.0,
            primary: info.dwFlags & PRIMARY != 0,
        });
        BOOL(1)
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn screens() -> Vec<super::Screen> {
        Vec::new()
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

    fn screen(x: f32, y: f32, w: f32, h: f32, scale: f32, primary: bool) -> Screen {
        Screen { x, y, w, h, scale, primary }
    }

    #[test]
    fn a_place_is_kept_only_where_a_screen_is_now() {
        // kura #357: one 2560x1440 screen; places saved on a second screen
        // to the right that is gone.
        let one = [screen(0.0, 0.0, 2560.0, 1440.0, 1.0, true)];
        assert!(win(100.0, 80.0, 900.0, 600.0).seen_on(&one, 100.0));
        assert!(!win(5000.0, 3000.0, 900.0, 600.0).seen_on(&one, 100.0), "below and right of the screen");
        assert!(!win(2700.0, 100.0, 900.0, 600.0).seen_on(&one, 100.0), "just right of the screen");
        assert!(!win(2500.0, 100.0, 900.0, 600.0).seen_on(&one, 100.0), "only 60 points of the title bar on it");
        assert!(win(2400.0, 100.0, 900.0, 600.0).seen_on(&one, 100.0), "160 points of the title bar on it");
        assert!(!win(100.0, 1400.0, 900.0, 600.0).seen_on(&one, 100.0), "title bar below the bottom");
        assert!(win(100.0, -50.0, 900.0, 600.0).seen_on(&one, 100.0), "a little above the top");
        assert!(!win(-32000.0, -32000.0, 900.0, 600.0).seen_on(&one, 100.0));
        assert!(!win(100.0, 80.0, 900.0, 600.0).seen_on(&[], 100.0));
        assert!(win(-8.0, -8.0, 2576.0, 1416.0).seen_on(&one, 100.0), "a maximized window's outer rect");
        // A second screen to the left (at 150%) and one above the primary:
        // the place is in the primary's points.
        let three = [
            screen(0.0, 0.0, 2560.0, 1440.0, 1.0, true),
            screen(-1920.0, 200.0, 1920.0, 1080.0, 1.5, false),
            screen(0.0, -1440.0, 2560.0, 1440.0, 1.0, false),
        ];
        assert!(win(-1800.0, 300.0, 900.0, 600.0).seen_on(&three, 100.0), "on the left screen");
        assert!(!win(-1800.0, 50.0, 900.0, 600.0).seen_on(&three, 100.0), "above the left screen, left of the primary");
        assert!(win(300.0, -1000.0, 900.0, 600.0).seen_on(&three, 100.0), "on the screen above");
        // The primary need not come first.
        let [p, l, a] = three;
        let shuffled = [l, a, p];
        assert!(win(-1800.0, 300.0, 900.0, 600.0).seen_on(&shuffled, 100.0));
        assert!(!win(-1800.0, 50.0, 900.0, 600.0).seen_on(&shuffled, 100.0));
        // A primary at 150%: 2000 points is 3000 pixels, off a 2560-pixel screen.
        let big = [screen(0.0, 0.0, 2560.0, 1440.0, 1.5, true)];
        assert!(!win(2000.0, 100.0, 900.0, 600.0).seen_on(&big, 100.0));
        assert!(win(1500.0, 100.0, 900.0, 600.0).seen_on(&big, 100.0));
    }

    #[test]
    fn the_screens_there_are_now_are_whole() {
        let all = screens();
        assert!(all.iter().filter(|s| s.primary).count() <= 1, "{all:?}");
        assert!(all.iter().all(|s| s.w > 0.0 && s.h > 0.0 && s.scale > 0.0), "{all:?}");
    }
}
