//! The pages and cards every app shares beyond language, clock, theme, keys
//! and notifications: Window, Startup, Advanced and About, and the order the
//! pages come in ([`Page`], [`default_order`]). Each card hands back what was
//! asked for; the app does it (writes common.toml, runs
//! `ito_common::startup::set`, opens a folder).

use std::path::Path;

use ito_common::{CommonChange, WindowPrefs};

use crate::{bug_card, button, keyed_row, row, section, sep, switch, Look, Words};

/// The settings pages every app can have, in the order they come in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Appearance,
    Theme,
    /// Where the app's own pages go (none, one or several).
    App,
    Keys,
    Notifications,
    OpenIn,
    Advanced,
    About,
}

impl Page {
    pub const ALL: [Page; 9] = [
        Page::General,
        Page::Appearance,
        Page::Theme,
        Page::App,
        Page::Keys,
        Page::Notifications,
        Page::OpenIn,
        Page::Advanced,
        Page::About,
    ];

    /// The page's title and its lead line. `App` has none: the app names its own.
    pub fn words(self, w: &MoreWords) -> (&'static str, &'static str) {
        match self {
            Page::General => (w.p_general, w.p_general_lead),
            Page::Appearance => (w.p_appearance, w.p_appearance_lead),
            Page::Theme => (w.p_theme, w.p_theme_lead),
            Page::App => ("", ""),
            Page::Keys => (w.p_keys, w.p_keys_lead),
            Page::Notifications => (w.p_notify, w.p_notify_lead),
            Page::OpenIn => (w.p_open, w.p_open_lead),
            Page::Advanced => (w.p_advanced, w.p_advanced_lead),
            Page::About => (w.p_about, w.p_about_lead),
        }
    }
}

/// The pages an app shows, in the default order: General first, About last,
/// and of the rest only the ones in `has` (put [`Page::App`] in `has` to
/// have a place for the app's own pages).
pub fn default_order(has: &[Page]) -> Vec<Page> {
    Page::ALL.into_iter().filter(|p| matches!(p, Page::General | Page::About) || has.contains(p)).collect()
}

/// The words of these pages and cards, in one language.
pub struct MoreWords {
    pub p_general: &'static str,
    pub p_general_lead: &'static str,
    pub p_appearance: &'static str,
    pub p_appearance_lead: &'static str,
    pub p_theme: &'static str,
    pub p_theme_lead: &'static str,
    pub p_keys: &'static str,
    pub p_keys_lead: &'static str,
    pub p_notify: &'static str,
    pub p_notify_lead: &'static str,
    pub p_open: &'static str,
    pub p_open_lead: &'static str,
    pub p_advanced: &'static str,
    pub p_advanced_lead: &'static str,
    pub p_about: &'static str,
    pub p_about_lead: &'static str,
    pub window: &'static str,
    pub remember: &'static str,
    pub remember_note: &'static str,
    pub on_top: &'static str,
    pub on_top_note: &'static str,
    pub forget: &'static str,
    pub forget_note: &'static str,
    pub forget_button: &'static str,
    pub startup: &'static str,
    pub startup_label: &'static str,
    pub startup_note: &'static str,
    pub startup_checking: &'static str,
    pub files: &'static str,
    pub config_dir: &'static str,
    pub config_dir_note: &'static str,
    pub state_dir: &'static str,
    pub state_dir_note: &'static str,
    pub open_button: &'static str,
    pub reset: &'static str,
    pub reset_label: &'static str,
    pub reset_note: &'static str,
    pub reset_button: &'static str,
    /// The button after one press: pressing again does it.
    pub reset_sure: &'static str,
    pub about: &'static str,
    pub version: &'static str,
    pub commit: &'static str,
    pub system: &'static str,
    pub license: &'static str,
    pub copy_label: &'static str,
    pub copy_note: &'static str,
    pub copy_button: &'static str,
}

pub const MORE_EN: MoreWords = MoreWords {
    p_general: "General",
    p_general_lead: "Language, size, window and the clock.",
    p_appearance: "Appearance",
    p_appearance_lead: "Fonts and how things look.",
    p_theme: "Theme",
    p_theme_lead: "Light, dark and the colours.",
    p_keys: "Keys",
    p_keys_lead: "Change a key, or add one.",
    p_notify: "Notifications",
    p_notify_lead: "How the app tells you something happened.",
    p_open: "Open in",
    p_open_lead: "The programs a file or a folder is opened with.",
    p_advanced: "Advanced",
    p_advanced_lead: "Files, folders and starting over.",
    p_about: "About",
    p_about_lead: "The version, and where to report a problem.",
    window: "WINDOW",
    remember: "Remember the window's place",
    remember_note: "Open where it was, at the size it was",
    on_top: "Keep above other windows",
    on_top_note: "The window stays on top",
    forget: "Forget the saved place",
    forget_note: "The next start opens where the OS puts it",
    forget_button: "Forget",
    startup: "STARTUP",
    startup_label: "Start when I sign in",
    startup_note: "Starts the app at sign-in",
    startup_checking: "Looking…",
    files: "FILES",
    config_dir: "Settings folder",
    config_dir_note: "The settings, the keys and the themes",
    state_dir: "Data folder",
    state_dir_note: "What the app keeps between runs",
    open_button: "Open",
    reset: "START OVER",
    reset_label: "Reset all settings",
    reset_note: "The settings go back to what the app starts with; a copy is kept",
    reset_button: "Reset…",
    reset_sure: "Press again to reset",
    about: "THIS APP",
    version: "Version",
    commit: "Build",
    system: "System",
    license: "License",
    copy_label: "Copy the details",
    copy_note: "The version and system, to paste into a report",
    copy_button: "Copy",
};

pub const MORE_JA: MoreWords = MoreWords {
    p_general: "一般",
    p_general_lead: "言語・大きさ・窓・時計。",
    p_appearance: "外観",
    p_appearance_lead: "字体と見た目。",
    p_theme: "テーマ",
    p_theme_lead: "ライト・ダークと色。",
    p_keys: "キー",
    p_keys_lead: "キーを変える・足す。",
    p_notify: "通知",
    p_notify_lead: "出来事をどう知らせるか。",
    p_open: "外部ツール",
    p_open_lead: "ファイルやフォルダーを開くプログラム。",
    p_advanced: "詳細",
    p_advanced_lead: "ファイル・フォルダー・初期化。",
    p_about: "このアプリについて",
    p_about_lead: "版と、不具合の報告。",
    window: "窓",
    remember: "窓の場所を覚える",
    remember_note: "前の場所と大きさで開く",
    on_top: "ほかの窓の手前に置く",
    on_top_note: "窓が常に手前に出る",
    forget: "覚えた場所を忘れる",
    forget_note: "次の起動は OS が決めた場所に開く",
    forget_button: "忘れる",
    startup: "起動",
    startup_label: "サインインで起動する",
    startup_note: "サインインしたときにアプリを起動します",
    startup_checking: "調べています…",
    files: "ファイル",
    config_dir: "設定のフォルダー",
    config_dir_note: "設定・キー・テーマ",
    state_dir: "データのフォルダー",
    state_dir_note: "実行の間に残すもの",
    open_button: "開く",
    reset: "初期化",
    reset_label: "設定をすべて初期化",
    reset_note: "設定をアプリの起動時の状態に戻します。コピーは残します",
    reset_button: "初期化…",
    reset_sure: "もう一度押すと初期化",
    about: "このアプリ",
    version: "版",
    commit: "ビルド",
    system: "OS",
    license: "ライセンス",
    copy_label: "詳細をコピー",
    copy_note: "版と OS。報告に貼るため",
    copy_button: "コピー",
};

impl MoreWords {
    /// The words for a language code (`ja`, `en`); English for the rest.
    pub fn of(language: &str) -> &'static MoreWords {
        match language {
            "ja" => &MORE_JA,
            _ => &MORE_EN,
        }
    }
}

/// The keys of the Window card's rows in common.toml, for [`crate::Nav::index`].
pub const WINDOW_KEYS: [&str; 2] = ["window.remember_place", "window.always_on_top"];

/// What the Window card asked for.
#[derive(Debug, Default, PartialEq)]
pub struct WindowOut {
    /// common.toml's changes.
    pub changes: Vec<CommonChange>,
    /// Delete the saved place now.
    pub forget: bool,
}

/// The Window card: `[window]` in common.toml. Put it on the General page.
pub fn window_card(ui: &mut egui::Ui, l: Look, w: &MoreWords, now: &WindowPrefs) -> WindowOut {
    let mut out = WindowOut::default();
    let set = |key: &'static str, v: bool| CommonChange { table: Some("window"), key, value: v.to_string() };
    section(ui, l, w.window, |ui| {
        if keyed_row(ui, l, WINDOW_KEYS[0], w.remember, w.remember_note, |ui| switch(ui, l, now.remember_place)) {
            out.changes.push(set("remember_place", !now.remember_place));
        }
        sep(ui, l);
        if keyed_row(ui, l, WINDOW_KEYS[1], w.on_top, w.on_top_note, |ui| switch(ui, l, now.always_on_top)) {
            out.changes.push(set("always_on_top", !now.always_on_top));
        }
        sep(ui, l);
        out.forget = row(ui, l, w.forget, w.forget_note, |ui| button(ui, l, w.forget_button));
    });
    out
}

/// The Window card's words, for the settings' search.
pub fn window_index(w: &MoreWords) -> Vec<&'static str> {
    vec![w.window, w.remember, w.remember_note, w.on_top, w.on_top_note, w.forget, w.forget_note]
}

/// The Startup card. `now` is what the OS says (`ito_common::startup::enabled`,
/// asked on a worker thread), `None` until it answers. Returns the new state
/// of the switch when it was clicked; the app calls `startup::set` off the UI thread.
pub fn startup_card(ui: &mut egui::Ui, l: Look, w: &MoreWords, now: Option<bool>) -> Option<bool> {
    let mut toggled = None;
    section(ui, l, w.startup, |ui| {
        let note = if now.is_none() { w.startup_checking } else { w.startup_note };
        if row(ui, l, w.startup_label, note, |ui| ui.add_enabled_ui(now.is_some(), |ui| switch(ui, l, now.unwrap_or(false))).inner) {
            toggled = now.map(|n| !n);
        }
    });
    toggled
}

/// The Startup card's words, for the settings' search.
pub fn startup_index(w: &MoreWords) -> Vec<&'static str> {
    vec![w.startup, w.startup_label, w.startup_note]
}

/// What the Advanced card asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advanced {
    OpenConfigDir,
    OpenStateDir,
    /// Pressed twice: the app moves its settings aside and starts over.
    Reset,
}

const RESET_ARMED: &str = "ito-prefs-reset-armed";

/// The Advanced card: the folders, and the reset. The reset is pressed twice;
/// the first press only changes the button's words.
pub fn advanced_card(ui: &mut egui::Ui, l: Look, w: &MoreWords, state_dir: bool) -> Vec<Advanced> {
    let mut out = Vec::new();
    section(ui, l, w.files, |ui| {
        if row(ui, l, w.config_dir, w.config_dir_note, |ui| button(ui, l, w.open_button)) {
            out.push(Advanced::OpenConfigDir);
        }
        if state_dir {
            sep(ui, l);
            if row(ui, l, w.state_dir, w.state_dir_note, |ui| button(ui, l, w.open_button)) {
                out.push(Advanced::OpenStateDir);
            }
        }
    });
    section(ui, l, w.reset, |ui| {
        let id = egui::Id::new(RESET_ARMED);
        let armed = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let words = if armed { w.reset_sure } else { w.reset_button };
        if row(ui, l, w.reset_label, w.reset_note, |ui| button(ui, l, words)) {
            if armed {
                out.push(Advanced::Reset);
            }
            ui.data_mut(|d| d.insert_temp(id, !armed));
        } else if armed && ui.input(|i| i.pointer.any_click()) {
            // A click anywhere else disarms it.
            ui.data_mut(|d| d.insert_temp(id, false));
        }
    });
    out
}

/// The Advanced card's words, for the settings' search.
pub fn advanced_index(w: &MoreWords) -> Vec<&'static str> {
    vec![w.files, w.config_dir, w.config_dir_note, w.state_dir, w.state_dir_note, w.reset, w.reset_label, w.reset_note]
}

/// What the About page shows.
pub struct About<'a> {
    pub version: &'a str,
    /// The build (a commit), when the app knows it.
    pub commit: Option<&'a str>,
    /// `ito_bugreport::os_line()`.
    pub system: &'a str,
    pub license: Option<&'a str>,
}

/// What the About page asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AboutAction {
    /// Open the bug report, as F12 does.
    Report,
}

/// The text "Copy the details" puts on the clipboard.
pub fn about_text(name: &str, a: &About) -> String {
    let mut s = format!("{name} {}", a.version);
    if let Some(c) = a.commit.filter(|c| !c.is_empty()) {
        s.push_str(&format!(" ({c})"));
    }
    s.push('\n');
    s.push_str(a.system);
    s
}

/// The About page's cards: the version and system (and a button that copies
/// them), then the Feedback card.
pub fn about_page(ui: &mut egui::Ui, l: Look, w: &MoreWords, words: &Words, name: &str, a: &About) -> Vec<AboutAction> {
    let mut out = Vec::new();
    section(ui, l, w.about, |ui| {
        let line = |ui: &mut egui::Ui, label: &str, value: &str| {
            row(ui, l, label, "", |ui| ui.label(egui::RichText::new(value).size(13.0).color(l.c.dim)));
        };
        line(ui, w.version, a.version);
        if let Some(c) = a.commit.filter(|c| !c.is_empty()) {
            sep(ui, l);
            line(ui, w.commit, c);
        }
        sep(ui, l);
        line(ui, w.system, a.system);
        if let Some(x) = a.license {
            sep(ui, l);
            line(ui, w.license, x);
        }
        sep(ui, l);
        if row(ui, l, w.copy_label, w.copy_note, |ui| button(ui, l, w.copy_button)) {
            ui.ctx().copy_text(about_text(name, a));
        }
    });
    if bug_card(ui, l, words) {
        out.push(AboutAction::Report);
    }
    out
}

/// The About page's words, for the settings' search.
pub fn about_index(w: &MoreWords, words: &Words) -> Vec<&'static str> {
    let mut v = vec![w.about, w.version, w.commit, w.system, w.license, w.copy_label, w.copy_note];
    v.extend(crate::bug_index(words));
    v
}

/// Whether a saved place may be used at all: the one test an app makes
/// before `ito_common::window::Saved::load`.
pub fn keeps_place(p: &WindowPrefs) -> bool {
    p.remember_place
}

/// Delete the saved place in `dir` (where `window.toml` is). Nothing there is fine.
pub fn forget_place(dir: &Path) -> Result<(), String> {
    let path = ito_common::window::path(dir);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_order_starts_with_general_and_ends_with_about() {
        let all = default_order(&Page::ALL);
        assert_eq!(all, Page::ALL);
        let few = default_order(&[Page::Keys, Page::App]);
        assert_eq!(few, [Page::General, Page::App, Page::Keys, Page::About]);
        // Asking for none still gives General and About.
        assert_eq!(default_order(&[]), [Page::General, Page::About]);
    }

    #[test]
    fn every_page_but_the_apps_has_words() {
        for lang in ["en", "ja"] {
            let w = MoreWords::of(lang);
            for p in Page::ALL.into_iter().filter(|p| *p != Page::App) {
                let (t, lead) = p.words(w);
                assert!(!t.is_empty() && !lead.is_empty(), "{lang} {p:?}");
            }
        }
    }

    #[test]
    fn the_details_to_copy_say_the_version_and_the_system() {
        let a = About { version: "1.2.3", commit: Some("abc123"), system: "Windows 11", license: None };
        assert_eq!(about_text("kura", &a), "kura 1.2.3 (abc123)\nWindows 11");
        let b = About { commit: None, ..a };
        assert_eq!(about_text("kura", &b), "kura 1.2.3\nWindows 11");
    }

    #[test]
    fn forgetting_a_place_that_is_not_there_is_fine() {
        let d = std::env::temp_dir().join(format!("ito-prefs-forget-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(forget_place(&d).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_cards_draw_and_ask_for_nothing_untouched() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1400.0))),
            ..Default::default()
        };
        let w = &MORE_EN;
        let a = About { version: "1.0.0", commit: None, system: "Test OS", license: Some("MIT") };
        let mut got = (WindowOut::default(), None, Vec::new(), Vec::new());
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let l = Look { c: ito_theme::colors(), q: "" };
                got.0 = window_card(ui, l, w, &WindowPrefs::default());
                got.1 = startup_card(ui, l, w, Some(false));
                got.2 = advanced_card(ui, l, w, true);
                got.3 = about_page(ui, l, w, &crate::EN, "app", &a);
            });
        });
        out.textures_delta.clear();
        assert_eq!(got.0, WindowOut::default());
        assert_eq!(got.1, None);
        assert!(got.2.is_empty() && got.3.is_empty());
        assert!(
            !window_index(w).is_empty()
                && !startup_index(w).is_empty()
                && !advanced_index(w).is_empty()
                && !about_index(w, &crate::EN).is_empty()
        );
    }
}
