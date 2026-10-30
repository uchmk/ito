//! A resident icon in the system tray, for an app that keeps running with no window.
//!
//! - [`Tray::new`] puts the icon there; dropping the [`Tray`] takes it away.
//! - [`Tray::set_state`] draws a badge on the icon: a colour **and** a symbol (the one of
//!   [`ito_a11y::State::mark`]), so the state is not told by colour alone. The tooltip begins with the
//!   same mark.
//! - [`Tray::set_tip`] is the hover text, [`Tray::set_menu`] the right-click menu.
//! - What the user does comes back as [`Event`]s on [`Tray::events`]; [`Tray::new_with`] also takes a
//!   function that is called after each event is queued, to wake an egui loop (`ctx.request_repaint()`).
//!
//! Windows has its own thread with a hidden window for the icon; nothing here touches the UI thread.
//! macOS and Linux answer `Unsupported` for now.

use std::io;
use std::sync::mpsc::Receiver;

pub use ito_a11y::State;

#[cfg(windows)]
#[path = "windows.rs"]
mod sys;
#[cfg(not(windows))]
#[path = "other.rs"]
mod sys;

/// The picture for the tray, straight RGBA (4 bytes a pixel, rows from the top).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Icon {
    /// `None` when `rgba` is not `width * height * 4` bytes or is empty.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Icon> {
        (width > 0 && height > 0 && rgba.len() == width as usize * height as usize * 4).then_some(Icon { width, height, rgba })
    }
}

/// A line of the right-click menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A choice. `id` comes back in [`Event::Menu`].
    Entry {
        id: String,
        label: String,
        enabled: bool,
        checked: bool,
    },
    Separator,
}

impl Item {
    /// An enabled, unchecked choice.
    pub fn entry(id: &str, label: &str) -> Item {
        Item::Entry { id: id.to_owned(), label: label.to_owned(), enabled: true, checked: false }
    }
}

/// What the user did with the icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A left click (bring the window forward, or start it).
    Click,
    /// A menu choice, by its [`Item::Entry`] `id`.
    Menu(String),
}

/// The icon in the tray. Dropping it removes the icon.
pub struct Tray {
    inner: sys::Tray,
}

impl Tray {
    /// Put `icon` in the tray with the hover text `tip`.
    pub fn new(icon: Icon, tip: &str) -> io::Result<Tray> {
        Tray::new_with(icon, tip, || {})
    }

    /// [`Tray::new`], and `wake` is called (on the tray's thread) each time an [`Event`] is queued.
    pub fn new_with(icon: Icon, tip: &str, wake: impl Fn() + Send + 'static) -> io::Result<Tray> {
        Ok(Tray { inner: sys::Tray::new(icon, tip, Box::new(wake))? })
    }

    /// The badge on the icon and the mark before the tip. `None` shows the plain icon.
    pub fn set_state(&self, state: Option<State>) {
        self.inner.set_state(state);
    }

    /// The hover text (the system cuts it at 127 characters).
    pub fn set_tip(&self, tip: &str) {
        self.inner.set_tip(tip);
    }

    /// The right-click menu. Empty: no menu.
    pub fn set_menu(&self, items: Vec<Item>) {
        self.inner.set_menu(items);
    }

    /// What the user did, oldest first. Use `try_recv` from the UI loop.
    pub fn events(&self) -> &Receiver<Event> {
        self.inner.events()
    }
}

/// The badge colour of a state (RGB). Never the only sign: the symbol is drawn too.
pub fn badge_color(state: State) -> [u8; 3] {
    match state {
        State::Error | State::Conflict => [200, 40, 40],
        State::Warning => [214, 140, 0],
        State::Done => [30, 140, 60],
        State::Running | State::Focused => [40, 100, 200],
        State::Selected => [40, 100, 200],
        State::Disabled => [110, 110, 110],
    }
}

/// `icon` with the badge of `state` in its bottom right corner: a disc in [`badge_color`] with a white symbol
/// that matches [`State::mark`].
pub fn with_badge(icon: &Icon, state: State) -> Icon {
    let mut out = icon.clone();
    let (w, h) = (icon.width as f32, icon.height as f32);
    let side = w.min(h);
    let r = side * 0.34;
    let (cx, cy) = (w - r, h - r);
    let [cr, cg, cb] = badge_color(state);
    for y in 0..icon.height {
        for x in 0..icon.width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            if d > r + 0.5 {
                continue;
            }
            let cover = (r + 0.5 - d).clamp(0.0, 1.0);
            let white = symbol(state, dx / r, dy / r);
            let c = if white { [255, 255, 255] } else { [cr, cg, cb] };
            let i = (y as usize * icon.width as usize + x as usize) * 4;
            for (px, c) in out.rgba[i..i + 3].iter_mut().zip(c) {
                *px = (*px as f32 * (1.0 - cover) + c as f32 * cover) as u8;
            }
            out.rgba[i + 3] = out.rgba[i + 3].max((255.0 * cover) as u8);
        }
    }
    out
}

/// Whether the point (u, v), each in -1..1 from the badge centre, is part of the white symbol.
fn symbol(state: State, u: f32, v: f32) -> bool {
    let t = 0.17; // half the stroke
    match state {
        State::Done => seg(u, v, (-0.5, 0.05), (-0.12, 0.45), t) || seg(u, v, (-0.12, 0.45), (0.5, -0.4), t),
        State::Error => seg(u, v, (-0.45, -0.45), (0.45, 0.45), t) || seg(u, v, (-0.45, 0.45), (0.45, -0.45), t),
        State::Warning => seg(u, v, (0.0, -0.55), (0.0, 0.12), t) || (u * u + (v - 0.52).powi(2)).sqrt() < t * 1.2,
        State::Running => [-0.55f32, 0.0, 0.55].iter().any(|&x| ((u - x).powi(2) + v * v).sqrt() < t * 1.15),
        State::Disabled => seg(u, v, (-0.5, 0.0), (0.5, 0.0), t),
        State::Selected => (u * u + v * v).sqrt() < 0.32,
        State::Focused => in_triangle((u, v), (-0.3, -0.5), (-0.3, 0.5), (0.5, 0.0)),
        State::Conflict => {
            seg(u, v, (-0.45, -0.18), (0.45, -0.18), t * 0.8)
                || seg(u, v, (-0.45, 0.18), (0.45, 0.18), t * 0.8)
                || seg(u, v, (-0.35, 0.55), (0.35, -0.55), t * 0.8)
        }
    }
}

/// Whether (u, v) is within `r` of the segment a-b.
fn seg(u: f32, v: f32, a: (f32, f32), b: (f32, f32), r: f32) -> bool {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 == 0.0 { 0.0 } else { (((u - a.0) * abx + (v - a.1) * aby) / len2).clamp(0.0, 1.0) };
    let (px, py) = (a.0 + abx * t, a.1 + aby * t);
    ((u - px).powi(2) + (v - py).powi(2)).sqrt() <= r
}

fn in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let s = |p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)| (p1.0 - p3.0) * (p2.1 - p3.1) - (p2.0 - p3.0) * (p1.1 - p3.1);
    let (d1, d2, d3) = (s(p, a, b), s(p, b, c), s(p, c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// The hover text with the state's mark before it, cut to what the system shows.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn tip_text(state: Option<State>, tip: &str) -> String {
    let full = match state {
        Some(s) => s.marked(tip),
        None => tip.to_owned(),
    };
    full.chars().take(127).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(n: u32) -> Icon {
        Icon::new(n, n, vec![90; (n * n * 4) as usize]).unwrap()
    }

    const ALL: [State; 8] =
        [State::Selected, State::Focused, State::Conflict, State::Error, State::Warning, State::Done, State::Running, State::Disabled];

    #[test]
    fn an_icon_needs_the_right_number_of_bytes() {
        assert!(Icon::new(2, 2, vec![0; 16]).is_some());
        assert!(Icon::new(2, 2, vec![0; 15]).is_none());
        assert!(Icon::new(0, 2, vec![]).is_none());
    }

    #[test]
    fn the_badge_sits_in_the_bottom_right_and_leaves_the_rest() {
        let base = plain(32);
        let b = with_badge(&base, State::Error);
        assert_eq!(b.rgba.len(), base.rgba.len());
        assert_eq!(&b.rgba[..4], &base.rgba[..4], "top left untouched");
        let corner = (31 * 32 + 25) * 4;
        assert_ne!(&b.rgba[corner..corner + 3], &base.rgba[corner..corner + 3]);
    }

    #[test]
    fn every_state_has_its_own_picture_and_a_white_symbol() {
        let base = plain(32);
        let pics: Vec<_> = ALL.iter().map(|&s| with_badge(&base, s)).collect();
        for (i, a) in pics.iter().enumerate() {
            for b in &pics[i + 1..] {
                assert_ne!(a.rgba, b.rgba);
            }
        }
        for (s, p) in ALL.iter().zip(&pics) {
            let white = p.rgba.chunks(4).filter(|c| c[..3] == [255, 255, 255]).count();
            assert!(white > 0, "{s:?} draws a symbol");
        }
    }

    #[test]
    fn two_states_that_share_a_colour_still_differ_by_symbol() {
        assert_eq!(badge_color(State::Error), badge_color(State::Conflict));
        let base = plain(32);
        assert_ne!(with_badge(&base, State::Error).rgba, with_badge(&base, State::Conflict).rgba);
    }

    #[test]
    fn the_tip_begins_with_the_mark_and_is_cut() {
        assert_eq!(tip_text(Some(State::Error), "1 error"), "✖ 1 error");
        assert_eq!(tip_text(None, "idle"), "idle");
        assert_eq!(tip_text(None, &"x".repeat(300)).chars().count(), 127);
    }

    #[cfg(windows)]
    #[test]
    fn the_icon_comes_and_goes_and_takes_changes() {
        // A session without a shell (a service) may refuse; then there is nothing more to check.
        let Ok(tray) = Tray::new(plain(32), "ito-tray test") else { return };
        tray.set_state(Some(State::Warning));
        tray.set_tip("2 running");
        tray.set_menu(vec![Item::entry("open", "Open"), Item::Separator, Item::entry("quit", "Quit")]);
        tray.set_state(None);
        assert!(tray.events().try_recv().is_err());
        drop(tray);
    }

    #[test]
    fn a_small_icon_does_not_panic() {
        for n in [1, 2, 16] {
            for &s in &ALL {
                let _ = with_badge(&plain(n), s);
            }
        }
    }
}
