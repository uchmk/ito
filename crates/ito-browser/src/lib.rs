//! A web page beside the panes (tsumugi's built-in browser, issue #66 there).
//! On Windows and macOS it is the system's own WebView (WebView2, WKWebView)
//! as a child of the window, under an address bar drawn by egui. Linux has
//! none (wry's child views need GTK): [`EMBEDDED`] is false there, and the
//! app opens the page in the system's browser instead.
//!
//! The app keeps an `Option<Browser>`, calls [`Browser::show`] in a panel each
//! frame (or [`Browser::hide`] when the panel is not drawn), and acts on the
//! [`Answer`]: open [`Answer::external`] in the system's browser, drop the
//! browser on [`Answer::close`]. The words of the bar are the app's
//! ([`Words`]), so they follow its language.

/// Whether a page can be shown inside the window on this OS.
pub const EMBEDDED: bool = cfg!(any(windows, target_os = "macos"));

/// The address a typed line means: a full `http(s)://` address as it is, `3000`
/// or `:3000` as `http://localhost:3000`, and a bare host (`localhost:5173/x`,
/// `127.0.0.1:8080`, `example.com`) with `http://` in front. Nothing else
/// (`file:`, `javascript:`…) is let in: the page is a web page.
pub fn normalize(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() || s.contains(|c: char| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let rest = &s[s.find("://")? + 3..];
        return (!rest.is_empty()).then(|| s.to_owned());
    }
    if s.contains("://") || lower.starts_with("javascript:") || lower.starts_with("data:") || lower.starts_with("file:") {
        return None;
    }
    let port = s.strip_prefix(':').unwrap_or(s);
    if port.parse::<u16>().is_ok_and(|p| p > 0) {
        return Some(format!("http://localhost:{port}"));
    }
    Some(format!("http://{s}"))
}

/// The page of a dev server to start on: the first of the ports given (those
/// a tab's programs listen on), else `http://localhost:3000`.
pub fn start_url(ports: &[u16]) -> String {
    ports.first().map_or_else(|| "http://localhost:3000".to_owned(), |p| format!("http://localhost:{p}"))
}

/// The words of the address bar, in the app's language.
#[derive(Clone, Copy, Debug)]
pub struct Words<'a> {
    /// The hover of ←.
    pub back: &'a str,
    /// The hover of ⟳.
    pub reload: &'a str,
    /// The hover of ↗.
    pub external: &'a str,
    /// The hover of ✕.
    pub close: &'a str,
    /// Shown, with the error after it, when the WebView could not be made.
    pub failed: &'a str,
}

/// What the panel asks of the app after a frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// Drop the browser.
    pub close: bool,
    /// Open this page in the system's browser.
    pub external: Option<String>,
}

#[cfg(any(windows, target_os = "macos"))]
pub use native::Browser;

#[cfg(not(any(windows, target_os = "macos")))]
pub use none::Browser;

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use super::{normalize, Answer, Words};
    use raw_window_handle::HasWindowHandle;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    pub struct Browser {
        url: String,
        edit: String,
        /// The address field had the keys last frame: a page loading does
        /// not overwrite what is being typed.
        editing: bool,
        view: Option<wry::WebView>,
        /// Where the WebView keeps its cookies and cache (see
        /// [`Browser::with_data_dir`]), and the context made from it, kept
        /// as long as the view.
        data: Option<PathBuf>,
        context: Option<wry::WebContext>,
        /// Why there is no page (the WebView could not be made).
        failed: Option<String>,
        /// A page to go to when the view is next shown.
        goto: Option<String>,
        /// What the view's handlers saw, read on the next frame.
        seen: Rc<RefCell<Seen>>,
        /// Windows: the window's menu kept out while the view is there.
        #[cfg(windows)]
        menu: Option<menu::KeepOut>,
    }

    /// Told by the WebView between frames.
    #[derive(Default)]
    struct Seen {
        /// The page it moved to (a link followed, back).
        loaded: Option<String>,
        /// Its title changed: a page that moves on without loading (GitHub
        /// does, by the history API) may be at another address.
        retitled: bool,
        /// A page it wanted in a new window: it comes in this view instead.
        popup: Option<String>,
    }

    impl Browser {
        pub fn new(url: String) -> Self {
            Self {
                edit: url.clone(),
                editing: false,
                goto: Some(url.clone()),
                url,
                view: None,
                data: None,
                context: None,
                failed: None,
                seen: Default::default(),
                #[cfg(windows)]
                menu: None,
            }
        }

        /// Keep the WebView's cookies and cache in `dir` (the app's state
        /// folder), so a sign-in lasts from one start to the next. Without
        /// it, WebView2 keeps them beside the `.exe`. macOS keeps them per
        /// app whatever is given.
        pub fn with_data_dir(mut self, dir: Option<PathBuf>) -> Self {
            self.data = dir;
            self
        }

        pub fn go(&mut self, url: String) {
            self.edit = url.clone();
            self.url = url.clone();
            self.goto = Some(url);
        }

        /// The page shown (or to be shown).
        pub fn url(&self) -> &str {
            &self.url
        }

        /// Draws the address bar and puts the view in the room left. `hidden`
        /// while something drawn by egui is to be seen over the page: a view
        /// of the system is always above what egui paints. `window` is the
        /// app's (`eframe::Frame`), the view's parent.
        pub fn show(&mut self, ui: &mut egui::Ui, window: &impl HasWindowHandle, hidden: bool, words: &Words) -> Answer {
            let mut answer = Answer::default();
            let seen = std::mem::take(&mut *self.seen.borrow_mut());
            if let Some(url) = seen.popup {
                self.go(url);
            }
            let moved = seen.retitled.then(|| self.view.as_ref().and_then(|v| v.url().ok())).flatten().filter(|u| *u != self.url);
            if let Some(url) = seen.loaded.or(moved) {
                if !self.editing {
                    self.edit = url.clone();
                }
                self.url = url;
            }
            ui.horizontal(|ui| {
                if ui.small_button("←").on_hover_text(words.back).clicked() {
                    if let Some(view) = &self.view {
                        let _ = view.evaluate_script("history.back()");
                    }
                }
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.edit)
                        .desired_width((ui.available_width() - 116.0).max(60.0))
                        .font(egui::FontId::monospace(ito_theme::zoomed(ui.ctx(), 12.0))),
                );
                self.editing = field.has_focus();
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Some(url) = normalize(&self.edit) {
                        self.go(url);
                    }
                }
                if ui.small_button("⟳").on_hover_text(words.reload).clicked() {
                    self.goto = Some(self.url.clone());
                }
                if ui.small_button("↗").on_hover_text(words.external).clicked() {
                    answer.external = Some(self.url.clone());
                }
                if ui.small_button("✕").on_hover_text(words.close).clicked() {
                    answer.close = true;
                }
            });
            let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
            if let Some(why) = &self.failed {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    why,
                    egui::FontId::proportional(ito_theme::zoomed(ui.ctx(), 13.0)),
                    ui.visuals().weak_text_color(),
                );
                return answer;
            }
            if self.view.is_none() {
                self.make(window, ui.ctx(), rect, ui.pixels_per_point(), words);
            }
            if let Some(view) = &self.view {
                if let Some(url) = self.goto.take() {
                    let _ = view.load_url(&url);
                }
                let _ = view.set_bounds(bounds(rect, ui.pixels_per_point()));
                let _ = view.set_visible(!hidden);
                // A click in the window outside the page gives the keys back
                // to the window (the address field, the app's keys): the
                // page keeps them from its own click on. On Windows the
                // window does it too, before egui sees the click (`menu`).
                if ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))) {
                    let _ = view.focus_parent();
                }
            }
            answer
        }

        /// The view goes out of sight (the panel is not drawn this frame).
        pub fn hide(&self) {
            if let Some(view) = &self.view {
                let _ = view.set_visible(false);
            }
        }

        fn make(&mut self, window: &impl HasWindowHandle, ctx: &egui::Context, rect: egui::Rect, ppp: f32, words: &Words) {
            let Ok(_) = window.window_handle() else { return };
            let url = self.goto.take().unwrap_or_else(|| self.url.clone());
            let mut context = self.data.clone().map(|dir| wry::WebContext::new(Some(dir)));
            let builder = match context.as_mut() {
                Some(c) => wry::WebViewBuilder::new_with_web_context(c),
                None => wry::WebViewBuilder::new(),
            };
            let (seen, wake) = (self.seen.clone(), ctx.clone());
            let builder = builder.with_on_page_load_handler(move |event, url| {
                if matches!(event, wry::PageLoadEvent::Started) {
                    seen.borrow_mut().loaded = Some(url);
                    wake.request_repaint();
                }
            });
            let (seen, wake) = (self.seen.clone(), ctx.clone());
            let builder = builder.with_document_title_changed_handler(move |_| {
                seen.borrow_mut().retitled = true;
                wake.request_repaint();
            });
            let (seen, wake) = (self.seen.clone(), ctx.clone());
            let builder = builder.with_new_window_req_handler(move |url, _| {
                seen.borrow_mut().popup = Some(url);
                wake.request_repaint();
                wry::NewWindowResponse::Deny
            });
            // The keys stay with the app (`o` then `j` moves on in its list):
            // the page takes them from a click in it.
            let builder = builder.with_focused(false);
            match builder.with_url(&url).with_bounds(bounds(rect, ppp)).build_as_child(window) {
                Ok(view) => {
                    self.view = Some(view);
                    self.context = context;
                    #[cfg(windows)]
                    if let Ok(raw_window_handle::RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) {
                        self.menu = Some(menu::KeepOut::new(h.hwnd.get()));
                    }
                }
                Err(e) => self.failed = Some(format!("{}: {e}", words.failed)),
            }
        }
    }

    /// A lone Alt (or F10) tapped while the page has the keys: WebView2 hands
    /// the keys the page does not use to the window, and the system would put
    /// the window, which has no menu, in its menu mode. The next click would
    /// only end that mode and never reach egui, so the keys stayed with the
    /// page (tsumugi #109). winit leaves the menu out for its own keys; this
    /// does the same for the page's, and gives the keys back to the window on
    /// a click in it before egui sees the click: the page covers its own
    /// room, so a click the window gets is outside the page.
    #[cfg(windows)]
    mod menu {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
        use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
        use windows::Win32::UI::WindowsAndMessaging::{
            SC_KEYMENU, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCDESTROY, WM_RBUTTONDOWN, WM_SYSCOMMAND, WM_XBUTTONDOWN,
        };

        /// The subclasses' ids on the window, one for each browser ("itob"
        /// and up): a second browser in the window, or a new one made before
        /// the old is dropped, keeps its own.
        static NEXT: AtomicUsize = AtomicUsize::new(0x6974_6f62);

        /// The subclass on the app's window, taken off when dropped (with the
        /// browser) or when the window goes.
        pub struct KeepOut(HWND, usize);

        impl KeepOut {
            pub fn new(window: isize) -> Self {
                let (window, id) = (HWND(window as *mut _), NEXT.fetch_add(1, Ordering::Relaxed));
                // SAFETY: the window is the app's and lives on this thread, the
                // UI thread; the browser holding this is not `Send`, so it is
                // dropped (and the subclass taken off) on the same thread.
                unsafe {
                    let _ = SetWindowSubclass(window, Some(subclass), id, 0);
                }
                Self(window, id)
            }
        }

        impl Drop for KeepOut {
            fn drop(&mut self) {
                // SAFETY: as in `new`; a window already gone has none to take off.
                unsafe {
                    let _ = RemoveWindowSubclass(self.0, Some(subclass), self.1);
                }
            }
        }

        unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, id: usize, _: usize) -> LRESULT {
            match msg {
                WM_SYSCOMMAND if bare_menu_key(wparam.0, lparam.0) => return LRESULT(0),
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN if unsafe { GetFocus() } != hwnd => {
                    let _ = unsafe { SetFocus(Some(hwnd)) };
                }
                WM_NCDESTROY => {
                    let _ = unsafe { RemoveWindowSubclass(hwnd, Some(subclass), id) };
                }
                _ => {}
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }

        /// The menu the system opens for a lone Alt or F10 (`SC_KEYMENU` with
        /// no letter). Alt+Space (the window's menu) and Alt+letter keep
        /// theirs. The low 4 bits of `wparam` are the system's own.
        pub(super) fn bare_menu_key(wparam: usize, lparam: isize) -> bool {
            wparam & 0xFFF0 == SC_KEYMENU as usize && lparam == 0
        }

        #[cfg(test)]
        mod tests {
            use super::*;
            use windows::Win32::UI::WindowsAndMessaging::{SC_CLOSE, SC_MOVE};

            #[test]
            fn only_a_lone_menu_key_is_kept_out() {
                assert!(bare_menu_key(SC_KEYMENU as usize, 0));
                assert!(bare_menu_key(SC_KEYMENU as usize | 0x3, 0));
                assert!(!bare_menu_key(SC_KEYMENU as usize, ' ' as isize));
                assert!(!bare_menu_key(SC_KEYMENU as usize, 'f' as isize));
                assert!(!bare_menu_key(SC_CLOSE as usize, 0));
                assert!(!bare_menu_key(SC_MOVE as usize | 0x2, 0));
            }

            #[test]
            fn the_subclass_goes_on_and_comes_off() {
                use windows::core::w;
                use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};

                // Taken off and put back: `GetWindowSubclass` has no name in
                // the comctl32 a binary without a manifest loads (5.82).
                let on = |window, id| unsafe {
                    let on = RemoveWindowSubclass(window, Some(subclass), id).as_bool();
                    if on {
                        let _ = SetWindowSubclass(window, Some(subclass), id, 0);
                    }
                    on
                };
                // A hidden window of the test's own, on this thread.
                let window =
                    unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), None, WS_POPUP, 0, 0, 10, 10, None, None, None, None) }
                        .expect("a window");
                let first = KeepOut::new(window.0 as isize);
                let second = KeepOut::new(window.0 as isize);
                assert_ne!(first.1, second.1);
                assert!(on(window, first.1) && on(window, second.1));
                let id = first.1;
                drop(first);
                assert!(!on(window, id));
                assert!(on(window, second.1));
                let id = second.1;
                drop(second);
                assert!(!on(window, id));
                unsafe { DestroyWindow(window) }.expect("the window goes");
            }
        }
    }

    fn bounds(rect: egui::Rect, ppp: f32) -> wry::Rect {
        let px = |v: f32| (v * ppp).round().max(0.0) as u32;
        wry::Rect {
            position: wry::dpi::PhysicalPosition::new(px(rect.left()), px(rect.top())).into(),
            size: wry::dpi::PhysicalSize::new(px(rect.width()).max(1), px(rect.height()).max(1)).into(),
        }
    }
}

/// Where there is no WebView ([`EMBEDDED`] is false) the same API, so the app
/// needs no `cfg`: the first [`Browser::show`] hands the page to the system's
/// browser and closes. An app that checks [`EMBEDDED`] never makes one.
#[cfg(not(any(windows, target_os = "macos")))]
mod none {
    use super::{Answer, Words};
    use raw_window_handle::HasWindowHandle;
    use std::path::PathBuf;

    pub struct Browser {
        url: String,
    }

    impl Browser {
        pub fn new(url: String) -> Self {
            Self { url }
        }

        pub fn with_data_dir(self, _dir: Option<PathBuf>) -> Self {
            self
        }

        pub fn go(&mut self, url: String) {
            self.url = url;
        }

        /// The page shown (or to be shown).
        pub fn url(&self) -> &str {
            &self.url
        }

        pub fn show(&mut self, _ui: &mut egui::Ui, _window: &impl HasWindowHandle, _hidden: bool, _words: &Words) -> Answer {
            Answer { close: true, external: Some(self.url.clone()) }
        }

        pub fn hide(&self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lines_become_web_addresses() {
        assert_eq!(normalize("3000").as_deref(), Some("http://localhost:3000"));
        assert_eq!(normalize(" :5173 ").as_deref(), Some("http://localhost:5173"));
        assert_eq!(normalize("localhost:8080/x").as_deref(), Some("http://localhost:8080/x"));
        assert_eq!(normalize("example.com").as_deref(), Some("http://example.com"));
        assert_eq!(normalize("https://example.com/a").as_deref(), Some("https://example.com/a"));
        assert_eq!(normalize("HTTP://127.0.0.1:1").as_deref(), Some("HTTP://127.0.0.1:1"));
    }

    #[test]
    fn other_schemes_and_blanks_are_refused() {
        for bad in ["", "  ", "javascript:alert(1)", "file:///c:/x", "data:text/html,x", "ftp://x", "http://", "a b"] {
            assert_eq!(normalize(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_first_port_starts_the_page() {
        assert_eq!(start_url(&[5173, 3000]), "http://localhost:5173");
        assert_eq!(start_url(&[]), "http://localhost:3000");
    }

    #[test]
    fn the_page_is_kept_and_moved() {
        let mut b = Browser::new("http://localhost:3000".into()).with_data_dir(None);
        assert_eq!(b.url(), "http://localhost:3000");
        b.go("http://localhost:5173".into());
        assert_eq!(b.url(), "http://localhost:5173");
    }
}
