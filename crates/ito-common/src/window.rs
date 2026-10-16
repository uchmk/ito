//! Where the window was and how big: `window.toml` in the app's state folder
//! ([`crate::state_dir`]), read when the app starts and written when the
//! window has moved or been resized. The apps put it back (position, size,
//! maximized) so a window opens where it was closed.
//!
//! Nothing here needs egui or winit: the app takes out the numbers to make
//! the window with ([`Window::opening`]), hands the new window, still
//! hidden, to [`Window::put`], and each frame hands a [`Tracker`] what the
//! window looks like ([`Seen`]: logical points, with the points to pixels
//! and its own zoom they were taken at). With the `egui` feature,
//! [`Seen::of`] reads those off egui's context.

use raw_window_handle::HasWindowHandle;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    /// The points to pixels the numbers were taken at: egui's
    /// `pixels_per_point`, the scale of the screen the window was on times
    /// the app's zoom. 0 in a file from before it was kept: the numbers are
    /// then taken at the primary screen's scale, as they were read then.
    #[serde(default)]
    pub scale: f32,
    /// The app's own zoom when the numbers were taken (egui's
    /// `zoom_factor`), which a new window is not made at.
    #[serde(default = "one")]
    pub zoom: f32,
}

fn one() -> f32 {
    1.0
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
        let finite = [self.x, self.y, self.w, self.h, self.scale, self.zoom].iter().all(|v| v.is_finite());
        finite
            && self.scale >= 0.0
            && self.zoom > 0.0
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
    /// the screen's top and at least `grab` above its bottom. The points are
    /// turned into pixels at the `scale` they were taken at; a place from
    /// before that was kept, at the primary screen's (where it was put back).
    pub fn seen_on(&self, screens: &[Screen], grab: f32) -> bool {
        let Some(first) = screens.iter().find(|s| s.primary).or(screens.first()) else {
            return false;
        };
        let k = if self.scale > 0.0 { self.scale } else { first.scale };
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

    /// The corner, the inner size and the maximum to make the window with.
    ///
    /// A new window has no zoom yet, so the app's zoom is taken out: the size
    /// is in the window's own points, which Windows scales when the window
    /// reaches a screen of another scale. The corner is right where the
    /// window is made on a screen of the scale it was closed on. On Windows
    /// egui reads it at the scale of the screen the window is made on and
    /// then again at the one it reached (kura #401), so [`Window::put`] moves
    /// the window to its pixels and maximizes it there: it is not maximized
    /// here. A place from before `scale` was kept is handed back as it was
    /// read.
    pub fn opening(&self) -> Opening {
        self.opened(sys::PUTS)
    }

    /// [`Window::opening`] where [`Window::put`] does (`puts`) or does not
    /// move and maximize the window.
    fn opened(&self, puts: bool) -> Opening {
        let maximized = self.maximized && !puts;
        if self.scale <= 0.0 {
            return Opening { pos: [self.x, self.y], size: [self.w, self.h], maximized };
        }
        let z = self.zoom;
        Opening { pos: [self.x * z, self.y * z], size: [self.w * z, self.h * z], maximized }
    }

    /// Put the window made with [`Window::opening`] on the pixels the place
    /// was taken at and maximize it there, before it is first shown: eframe
    /// hands the window to the app's creator hidden (`CreationContext`).
    /// Only on Windows; elsewhere the window is already where `opening` said.
    pub fn put(&self, window: &impl HasWindowHandle) {
        sys::put(self, window);
    }

    /// The outer corner in pixels, where the place says at what scale it was
    /// taken.
    #[cfg(any(windows, test))]
    fn pixel(&self) -> Option<(i32, i32)> {
        (self.scale > 0.0).then(|| ((self.x * self.scale).round() as i32, (self.y * self.scale).round() as i32))
    }
}

/// How to make the window: egui's `ViewportBuilder::with_position`,
/// `with_inner_size` and `with_maximized`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opening {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub maximized: bool,
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
    use super::{Screen, Window};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
    use windows::Win32::UI::HiDpi::{
        GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_MAXIMIZE,
    };

    /// `MONITORINFOF_PRIMARY` (in WindowsAndMessaging, not worth the feature).
    const PRIMARY: u32 = 1;

    /// [`Window::put`] moves and maximizes the window.
    pub const PUTS: bool = true;

    pub fn put(place: &Window, window: &impl HasWindowHandle) {
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(raw) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(raw.hwnd.get() as *mut std::ffi::c_void);
        // SAFETY: `hwnd` is the app's window, alive while `window` is
        // borrowed, and this is the thread that made it.
        unsafe {
            if let Some((x, y)) = place.pixel() {
                // Onto a screen of another scale Windows sends WM_DPICHANGED,
                // and winit keeps the window's size in points.
                let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            if place.maximized {
                // As winit maximizes a window that is not shown yet: shown
                // maximized and hidden again. winit reads the maximum from
                // WM_SIZE and keeps it when eframe shows the window.
                let shown = IsWindowVisible(hwnd).as_bool();
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                if !shown {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
    }

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

    /// The window is made where [`super::Window::opening`] said.
    pub const PUTS: bool = false;

    pub fn put(_: &super::Window, _: &impl raw_window_handle::HasWindowHandle) {}
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

/// How long the window holds still before its place is written: a drag is
/// one write, after it stops.
pub const SETTLE: Duration = Duration::from_millis(800);

/// What the window looks like in a frame, as egui says it: the outer corner
/// and the inner size in points, whether it is maximized, minimized or full
/// screen, the points to pixels (`pixels_per_point`) and the app's zoom
/// (`zoom_factor`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seen {
    pub corner: [f32; 2],
    pub size: [f32; 2],
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub scale: f32,
    pub zoom: f32,
}

#[cfg(feature = "egui")]
impl Seen {
    /// What the window of `ctx` looks like now; nothing until the OS has
    /// said where it is.
    pub fn of(ctx: &egui::Context) -> Option<Seen> {
        let zoom = ctx.zoom_factor();
        ctx.input(|i| {
            let v = i.viewport();
            let (outer, inner) = (v.outer_rect?, v.inner_rect?);
            Some(Seen {
                corner: [outer.min.x, outer.min.y],
                size: [inner.width(), inner.height()],
                maximized: v.maximized.unwrap_or(false),
                minimized: v.minimized.unwrap_or(false),
                fullscreen: v.fullscreen.unwrap_or(false),
                scale: i.pixels_per_point(),
                zoom,
            })
        })
    }
}

/// Follows the window and writes its place to `window.toml` once a move or
/// a resize has held still for [`SETTLE`]. Maximized, the place stays the
/// one from before, so leaving the maximum goes back there; minimized or
/// full screen, the window is not followed. The uchmk apps each kept their
/// own, and kura #401 had to be fixed in each.
#[derive(Debug)]
pub struct Tracker {
    dir: PathBuf,
    /// Where the window is, and since when.
    now: Option<Window>,
    since: Instant,
    /// What `window.toml` has, as far as this window knows.
    saved: Option<Window>,
}

/// What [`Tracker::due`] says is next.
#[derive(Debug, PartialEq)]
enum Due {
    Nothing,
    Wait(Duration),
    Write(Window),
}

impl Tracker {
    /// Writing into `dir` (as [`write`]). `loaded` is the place the window
    /// was made with ([`read`]), so a start that moves nothing writes
    /// nothing, and a window that opens maximized keeps the place under it.
    pub fn new(dir: impl Into<PathBuf>, loaded: Option<Window>) -> Self {
        Tracker { dir: dir.into(), now: loaded, since: Instant::now(), saved: loaded }
    }

    /// Note what the window looks like this frame, and once its place has
    /// changed and held still, write it on a thread of its own. While it
    /// waits, how long for: the app asks for a frame then (egui's
    /// `request_repaint_after`), or a window that stops is not written
    /// until something else draws.
    pub fn see(&mut self, seen: Option<Seen>) -> Option<Duration> {
        let at = Instant::now();
        self.note(seen, at);
        match self.due(at) {
            Due::Nothing => None,
            Due::Wait(left) => Some(left),
            Due::Write(place) => {
                self.saved = Some(place);
                let dir = self.dir.clone();
                let _ = std::thread::Builder::new().name("window-place".into()).spawn(move || {
                    let _ = write(&dir, &place);
                });
                None
            }
        }
    }

    /// The app is closing: note what the window looks like and write its
    /// place now, held still or not, before the process ends.
    pub fn close(&mut self, seen: Option<Seen>) {
        self.note(seen, Instant::now());
        if let Some(place) = self.now.filter(|p| self.saved != Some(*p)) {
            self.saved = Some(place);
            let _ = write(&self.dir, &place);
        }
    }

    fn note(&mut self, seen: Option<Seen>, at: Instant) {
        let Some(s) = seen.filter(|s| !s.minimized && !s.fullscreen) else {
            return;
        };
        let place = match self.now.filter(|_| s.maximized) {
            Some(was) => Window { maximized: true, ..was },
            None => {
                Window { x: s.corner[0], y: s.corner[1], w: s.size[0], h: s.size[1], maximized: s.maximized, scale: s.scale, zoom: s.zoom }
            }
        };
        if self.now != Some(place) {
            self.now = Some(place);
            self.since = at;
        }
    }

    fn due(&self, at: Instant) -> Due {
        let Some(place) = self.now.filter(|p| self.saved != Some(*p)) else {
            return Due::Nothing;
        };
        let left = SETTLE.saturating_sub(at.saturating_duration_since(self.since));
        if left.is_zero() {
            Due::Write(place)
        } else {
            Due::Wait(left)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(x: f32, y: f32, w: f32, h: f32) -> Window {
        Window { x, y, w, h, maximized: false, scale: 0.0, zoom: 1.0 }
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
        // With the scale and the zoom it was taken at.
        let w = Window { scale: 1.875, zoom: 1.25, ..win(4493.0, 280.0, 1100.0, 700.0) };
        write(&dir, &w).unwrap();
        assert_eq!(read(&dir), Some(w));
        // A file from before those were kept reads as unscaled and unzoomed.
        std::fs::write(path(&dir), "x = 120.0\ny = 80.0\nw = 1100.0\nh = 700.0\n").unwrap();
        assert_eq!(read(&dir), Some(win(120.0, 80.0, 1100.0, 700.0)));
        std::fs::write(path(&dir), "x = 120.0\ny = 80.0\nw = 1100.0\nh = 700.0\nzoom = 0.0\n").unwrap();
        assert_eq!(read(&dir), None, "no zoom is a broken number");
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
    fn a_place_on_a_second_screen_at_another_scale_is_kept_and_put_back() {
        // kura #401: a 3840x2160 primary at 150% and a 2560x1440 screen at
        // 100% to its right. Closed on the second one, at pixels 4493,280.
        let two = [screen(0.0, 0.0, 3840.0, 2160.0, 1.5, true), screen(3840.0, 0.0, 2560.0, 1440.0, 1.0, false)];
        let on_second = Window { scale: 1.0, ..win(4493.0, 280.0, 1102.0, 721.0) };
        assert!(on_second.seen_on(&two, 100.0), "taken at 100%, the corner is pixel 4493");
        assert!(!win(4493.0, 280.0, 1102.0, 721.0).seen_on(&two, 100.0), "an older file is read at 150%: pixel 6739");
        // Put back on those pixels, whatever screen the window is made on.
        assert_eq!(on_second.pixel(), Some((4493, 280)));
        assert_eq!(on_second.opened(true), Opening { pos: [4493.0, 280.0], size: [1102.0, 721.0], maximized: false });
        // Closed on the primary: pixel 152, the numbers as they were.
        let on_first = Window { scale: 1.5, ..win(101.33, 101.33, 1102.0, 721.0) };
        assert!(on_first.seen_on(&two, 100.0));
        assert_eq!(on_first.pixel(), Some((152, 152)));
        assert_eq!(on_first.opened(false).pos, [101.33, 101.33]);
        // The other way round: a 100% primary, a 150% screen to its right.
        let other = [screen(0.0, 0.0, 2560.0, 1440.0, 1.0, true), screen(2560.0, 0.0, 3840.0, 2160.0, 1.5, false)];
        let on_big = Window { scale: 1.5, ..win(2000.0, 100.0, 1000.0, 700.0) };
        assert!(on_big.seen_on(&other, 100.0), "pixel 3000, on the 150% screen");
        assert_eq!(on_big.pixel(), Some((3000, 150)));
        // An older file says no scale: not moved.
        assert_eq!(win(2000.0, 100.0, 1000.0, 700.0).pixel(), None);
    }

    #[test]
    fn a_maximized_window_is_maximized_where_it_is_put() {
        let max = Window { maximized: true, scale: 1.0, ..win(4493.0, 280.0, 1100.0, 720.0) };
        // Where `put` moves the window, it maximizes it there too: made
        // maximized, it would be on the screen it was made on.
        assert!(!max.opened(true).maximized);
        assert!(max.opened(false).maximized);
        assert!(Window { scale: 0.0, ..max }.opened(false).maximized);
        assert_eq!(max.opening().maximized, !sys::PUTS);
    }

    #[test]
    fn the_app_s_zoom_is_taken_out_of_the_place() {
        // At 125% zoom on a 150% screen: pixel 300,150, 1875x1500 pixels.
        let zoomed = Window { scale: 1.875, zoom: 1.25, ..win(160.0, 80.0, 1000.0, 800.0) };
        let one = [screen(0.0, 0.0, 3840.0, 2160.0, 1.5, true)];
        assert!(zoomed.seen_on(&one, 100.0));
        assert_eq!(zoomed.opened(true), Opening { pos: [200.0, 100.0], size: [1250.0, 1000.0], maximized: false });
        assert_eq!(zoomed.pixel(), Some((300, 150)));
        // An older file is handed back as it was read.
        let old = win(160.0, 80.0, 1000.0, 800.0);
        assert_eq!(old.opened(true), Opening { pos: [160.0, 80.0], size: [1000.0, 800.0], maximized: false });
    }

    fn seen(x: f32, y: f32, w: f32, h: f32) -> Seen {
        Seen { corner: [x, y], size: [w, h], maximized: false, minimized: false, fullscreen: false, scale: 1.5, zoom: 1.0 }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_move_is_written_once_it_holds_still() {
        let t0 = Instant::now();
        let mut t = Tracker::new(PathBuf::new(), None);
        t.note(Some(seen(100.0, 80.0, 1100.0, 700.0)), t0);
        assert_eq!(t.due(t0), Due::Wait(SETTLE));
        assert_eq!(t.due(t0 + ms(300)), Due::Wait(SETTLE - ms(300)));
        // Moved again: the wait starts over; seen again where it is: not.
        t.note(Some(seen(120.0, 80.0, 1100.0, 700.0)), t0 + ms(500));
        t.note(Some(seen(120.0, 80.0, 1100.0, 700.0)), t0 + ms(900));
        assert_eq!(t.due(t0 + ms(1000)), Due::Wait(SETTLE - ms(500)));
        let place = Window { scale: 1.5, ..win(120.0, 80.0, 1100.0, 700.0) };
        assert_eq!(t.due(t0 + ms(1300)), Due::Write(place));
        t.saved = Some(place);
        assert_eq!(t.due(t0 + ms(5000)), Due::Nothing);
    }

    #[test]
    fn a_start_that_moves_nothing_writes_nothing() {
        let t0 = Instant::now();
        let loaded = Window { scale: 1.5, ..win(100.0, 80.0, 1100.0, 700.0) };
        let mut t = Tracker::new(PathBuf::new(), Some(loaded));
        t.note(Some(seen(100.0, 80.0, 1100.0, 700.0)), t0);
        assert_eq!(t.due(t0 + ms(5000)), Due::Nothing);
        // Opened maximized: the place under it is the one read.
        let loaded = Window { maximized: true, ..loaded };
        let mut t = Tracker::new(PathBuf::new(), Some(loaded));
        t.note(Some(Seen { maximized: true, ..seen(-8.0, -8.0, 2576.0, 1376.0) }), t0);
        assert_eq!(t.due(t0 + ms(5000)), Due::Nothing);
    }

    #[test]
    fn maximized_keeps_the_place_under_it_and_minimized_is_not_followed() {
        let t0 = Instant::now();
        let mut t = Tracker::new(PathBuf::new(), None);
        // On a second screen at 100% (kura #401), then maximized there.
        let under = Window { scale: 1.0, ..win(4493.0, 280.0, 1100.0, 720.0) };
        t.note(Some(Seen { scale: 1.0, ..seen(4493.0, 280.0, 1100.0, 720.0) }), t0);
        t.note(Some(Seen { maximized: true, scale: 1.0, ..seen(3832.0, -8.0, 2576.0, 1376.0) }), t0);
        let max = Window { maximized: true, ..under };
        assert_eq!(t.now, Some(max));
        // Minimized, full screen, or not said yet: as it was.
        t.note(Some(Seen { minimized: true, ..seen(-32000.0, -32000.0, 160.0, 28.0) }), t0);
        t.note(Some(Seen { fullscreen: true, scale: 1.0, ..seen(3840.0, 0.0, 2560.0, 1440.0) }), t0);
        t.note(None, t0);
        assert_eq!(t.now, Some(max));
        // Back from the maximum: where it was.
        t.note(Some(Seen { scale: 1.0, ..seen(4493.0, 280.0, 1100.0, 720.0) }), t0);
        assert_eq!(t.now, Some(under));
    }

    #[test]
    fn closing_writes_at_once_with_the_scale_and_the_zoom() {
        let dir = std::env::temp_dir().join(format!("ito-window-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut t = Tracker::new(&dir, None);
        // At 125% zoom on a 150% screen.
        t.close(Some(Seen { scale: 1.875, zoom: 1.25, ..seen(160.0, 80.0, 1000.0, 800.0) }));
        let place = Window { scale: 1.875, zoom: 1.25, ..win(160.0, 80.0, 1000.0, 800.0) };
        assert_eq!(read(&dir), Some(place));
        assert_eq!(place.opened(true).size, [1250.0, 1000.0], "made at the size without the zoom");
        // Nothing new: nothing written.
        std::fs::remove_file(path(&dir)).unwrap();
        t.close(Some(Seen { scale: 1.875, zoom: 1.25, ..seen(160.0, 80.0, 1000.0, 800.0) }));
        assert_eq!(read(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(feature = "egui")]
    #[test]
    fn what_egui_says_is_seen() {
        let ctx = egui::Context::default();
        ctx.set_zoom_factor(1.25);
        let mut input = egui::RawInput::default();
        let v = input.viewports.entry(egui::ViewportId::ROOT).or_default();
        v.native_pixels_per_point = Some(1.5);
        v.outer_rect = Some(egui::Rect::from_min_size(egui::pos2(160.0, 80.0), egui::vec2(1010.0, 830.0)));
        v.inner_rect = Some(egui::Rect::from_min_size(egui::pos2(165.0, 110.0), egui::vec2(1000.0, 800.0)));
        v.maximized = Some(false);
        let mut got = None;
        ctx.run_ui(input, |ui| got = Some(Seen::of(ui.ctx()))).drop_without_applying_deltas();
        // The rectangles are in egui's points already (egui-winit divides by
        // the zoomed points to pixels); the zoom is in the scale.
        let want = Seen { scale: 1.875, zoom: 1.25, ..seen(160.0, 80.0, 1000.0, 800.0) };
        assert_eq!(got, Some(Some(want)));
        // Before the OS has said where the window is: nothing.
        let mut got = None;
        egui::Context::default().run_ui(egui::RawInput::default(), |ui| got = Some(Seen::of(ui.ctx()))).drop_without_applying_deltas();
        assert_eq!(got, Some(None));
    }

    #[test]
    fn the_screens_there_are_now_are_whole() {
        let all = screens();
        assert!(all.iter().filter(|s| s.primary).count() <= 1, "{all:?}");
        assert!(all.iter().all(|s| s.w > 0.0 && s.h > 0.0 && s.scale > 0.0), "{all:?}");
    }
}
