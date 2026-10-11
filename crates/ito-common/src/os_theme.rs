//! Whether the OS asks apps for light colours, read from the setting itself.
//!
//! On Windows, winit answers WM_SETTINGCHANGE with uxtheme's
//! `ShouldAppsUseDarkMode` without first refreshing uxtheme's copy of the
//! setting, so egui's `system_theme` stays one switch behind the OS. The
//! registry value it copies is never behind.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

// 0: not known.
const DARK: u8 = 1;
const LIGHT: u8 = 2;

/// The OS's light or dark, watched on a thread of its own. Where it is not
/// read here (off Windows), `get` is `None` and the app keeps egui's
/// `system_theme`.
#[derive(Clone, Debug, Default)]
pub struct OsLight(Arc<AtomicU8>);

impl OsLight {
    /// Read the setting now and every `every` after, calling `wake` (to draw
    /// again) when it has changed. The thread ends when the last `OsLight`
    /// is dropped.
    pub fn watch(every: Duration, wake: impl Fn() + Send + 'static) -> Self {
        let me = Self::default();
        if cfg!(windows) {
            let cell = Arc::downgrade(&me.0);
            let _ = std::thread::Builder::new().name("uchmk-os-theme".into()).spawn(move || loop {
                let Some(cell) = cell.upgrade() else { return };
                if let Some(light) = read() {
                    let now = if light { LIGHT } else { DARK };
                    if cell.swap(now, Ordering::Relaxed) != now {
                        wake();
                    }
                }
                drop(cell);
                std::thread::sleep(every);
            });
        }
        me
    }

    /// `Some(true)` for light, `Some(false)` for dark, `None` when not known
    /// (not read yet, or not on this OS).
    pub fn get(&self) -> Option<bool> {
        match self.0.load(Ordering::Relaxed) {
            LIGHT => Some(true),
            DARK => Some(false),
            _ => None,
        }
    }

    /// The setting, else `fallback` (egui's `system_theme`).
    pub fn light_or(&self, fallback: bool) -> bool {
        self.get().unwrap_or(fallback)
    }
}

/// `AppsUseLightTheme` under `HKCU\…\Themes\Personalize`: 1 light, 0 dark.
#[cfg(windows)]
fn read() -> Option<bool> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: the out pointers point at `value` and `size`, which outlive the call.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&raw mut size),
        )
    };
    status.is_ok().then_some(value != 0)
}

#[cfg(not(windows))]
fn read() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_known_is_none_and_falls_back() {
        let o = OsLight::default();
        assert_eq!(o.get(), None);
        assert!(o.light_or(true));
        o.0.store(DARK, Ordering::Relaxed);
        assert_eq!(o.get(), Some(false));
        assert!(!o.light_or(true));
    }

    #[cfg(not(windows))]
    #[test]
    fn off_windows_it_is_never_known() {
        let o = OsLight::watch(Duration::from_millis(1), || {});
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(o.get(), None);
    }
}
