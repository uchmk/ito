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

    pub struct Browser {
        url: String,
        edit: String,
        view: Option<wry::WebView>,
        /// Why there is no page (the WebView could not be made).
        failed: Option<String>,
        /// A page to go to when the view is next shown.
        goto: Option<String>,
    }

    impl Browser {
        pub fn new(url: String) -> Self {
            Self { edit: url.clone(), goto: Some(url.clone()), url, view: None, failed: None }
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
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.edit)
                        .desired_width((ui.available_width() - 90.0).max(60.0))
                        .font(egui::FontId::monospace(12.0)),
                );
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
                    egui::FontId::proportional(13.0),
                    ui.visuals().weak_text_color(),
                );
                return answer;
            }
            if self.view.is_none() {
                self.make(window, rect, ui.pixels_per_point(), words);
            }
            if let Some(view) = &self.view {
                if let Some(url) = self.goto.take() {
                    let _ = view.load_url(&url);
                }
                let _ = view.set_bounds(bounds(rect, ui.pixels_per_point()));
                let _ = view.set_visible(!hidden);
            }
            answer
        }

        /// The view goes out of sight (the panel is not drawn this frame).
        pub fn hide(&self) {
            if let Some(view) = &self.view {
                let _ = view.set_visible(false);
            }
        }

        fn make(&mut self, window: &impl HasWindowHandle, rect: egui::Rect, ppp: f32, words: &Words) {
            let Ok(_) = window.window_handle() else { return };
            let url = self.goto.take().unwrap_or_else(|| self.url.clone());
            match wry::WebViewBuilder::new().with_url(&url).with_bounds(bounds(rect, ppp)).build_as_child(window) {
                Ok(view) => self.view = Some(view),
                Err(e) => self.failed = Some(format!("{}: {e}", words.failed)),
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

    pub struct Browser {
        url: String,
    }

    impl Browser {
        pub fn new(url: String) -> Self {
            Self { url }
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
        let mut b = Browser::new("http://localhost:3000".into());
        assert_eq!(b.url(), "http://localhost:3000");
        b.go("http://localhost:5173".into());
        assert_eq!(b.url(), "http://localhost:5173");
    }
}
