//! The tab rows (`ito_tabs::Settings`): when the bar shows, where a file opened
//! from outside goes, and what reopens at start. Values are per app, not
//! common.toml; the rows only edit the `Settings`, the app saves them.

use ito_tabs::{OpenFromOutside, Reopen, Settings, ShowBar};

use crate::{keyed_row, seg_named, sep, Look};

/// The rows' words, in one language.
pub struct TabsWords {
    pub show_bar: &'static str,
    pub show_bar_note: &'static str,
    /// Always, only with two or more tabs, never.
    pub show: [&'static str; 3],
    pub outside: &'static str,
    pub outside_note: &'static str,
    /// A new tab in the running window, a new window.
    pub outside_to: [&'static str; 2],
    pub start: &'static str,
    pub start_note: &'static str,
    /// The last tabs, only the file given.
    pub start_what: [&'static str; 2],
}

pub const TABS_EN: TabsWords = TabsWords {
    show_bar: "Show the tab bar",
    show_bar_note: "When the bar of open files is shown",
    show: ["Always", "With two or more tabs", "Never"],
    outside: "A file opened from outside",
    outside_note: "Applies when another program (kura's opener, Explorer) opens a file while the app is running",
    outside_to: ["A new tab in the running window", "A new window"],
    start: "At start",
    start_note: "What opens when the app starts",
    start_what: ["Reopen the last tabs", "Only the file given"],
};

pub const TABS_JA: TabsWords = TabsWords {
    show_bar: "タブバーを表示",
    show_bar_note: "開いているファイルのタブを出すとき",
    show: ["常に", "タブが 2 つ以上のとき", "表示しない"],
    outside: "外から開いたファイル",
    outside_note: "動いている間に別のプログラム（kura の開く操作・エクスプローラー）がファイルを開いたときに効きます",
    outside_to: ["動いている窓の新しいタブ", "新しい窓"],
    start: "起動したとき",
    start_note: "起動時に開くもの",
    start_what: ["前回のタブを開き直す", "渡されたファイルだけ"],
};

impl TabsWords {
    /// The words for a language code (`ja`, `en`); English for the rest.
    pub fn of(language: &str) -> &'static TabsWords {
        if language == "ja" {
            &TABS_JA
        } else {
            &TABS_EN
        }
    }
}

/// The rows' keys in the app's settings file, for [`crate::Nav::index`].
pub const TABS_KEYS: [&str; 3] = ["tabs.show_bar", "tabs.open_from_outside", "tabs.reopen"];

/// The rows' words, for the settings' search.
pub fn tabs_index(w: &TabsWords) -> Vec<&'static str> {
    let mut v = vec![w.show_bar, w.show_bar_note, w.outside, w.outside_note, w.start, w.start_note];
    v.extend(w.show);
    v.extend(w.outside_to);
    v.extend(w.start_what);
    v
}

/// The three tab rows, in the OS's language. True when `s` changed; the app
/// saves it. Use [`tabs_rows_in`] to give the app's own language.
pub fn tabs_rows(ui: &mut egui::Ui, l: Look, s: &mut Settings) -> bool {
    let os = ito_common::os_language();
    let lang = ito_i18n::resolve(None, None, os.as_deref(), &["ja", "en"]);
    tabs_rows_in(ui, l, TabsWords::of(&lang), s)
}

/// [`tabs_rows`] in the words given.
pub fn tabs_rows_in(ui: &mut egui::Ui, l: Look, w: &TabsWords, s: &mut Settings) -> bool {
    let before = *s;
    let [a, b, c] = w.show;
    let bar = [(ShowBar::Always, a), (ShowBar::TwoOrMore, b), (ShowBar::Never, c)];
    if let Some(v) =
        keyed_row(ui, l, TABS_KEYS[0], w.show_bar, w.show_bar_note, |ui| seg_named(ui, l, TABS_KEYS[0], w.show_bar, s.show_bar, &bar).0)
    {
        s.show_bar = v;
    }
    sep(ui, l);
    let out = [(OpenFromOutside::NewTab, w.outside_to[0]), (OpenFromOutside::NewWindow, w.outside_to[1])];
    if let Some(v) = keyed_row(ui, l, TABS_KEYS[1], w.outside, w.outside_note, |ui| {
        seg_named(ui, l, TABS_KEYS[1], w.outside, s.open_from_outside, &out).0
    }) {
        s.open_from_outside = v;
    }
    sep(ui, l);
    let start = [(Reopen::LastTabs, w.start_what[0]), (Reopen::OnlyGiven, w.start_what[1])];
    if let Some(v) = keyed_row(ui, l, TABS_KEYS[2], w.start, w.start_note, |ui| seg_named(ui, l, TABS_KEYS[2], w.start, s.reopen, &start).0)
    {
        s.reopen = v;
    }
    *s != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ctx: &egui::Context, w: &TabsWords, s: &mut Settings, events: Vec<egui::Event>) -> (bool, Vec<(String, egui::Rect)>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut changed = false;
        let mut out = ctx.run_ui(input, |ui| {
            changed = tabs_rows_in(ui, Look { c: ito_theme::colors(), q: "" }, w, s);
        });
        out.textures_delta.clear();
        fn walk(s: &egui::Shape, v: &mut Vec<(String, egui::Rect)>) {
            match s {
                egui::Shape::Text(t) => v.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
                egui::Shape::Vec(l) => l.iter().for_each(|s| walk(s, v)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        (changed, texts)
    }

    #[test]
    fn untouched_settings_are_not_changed() {
        let ctx = egui::Context::default();
        let mut s = Settings::default();
        let (changed, texts) = run(&ctx, &TABS_EN, &mut s, Vec::new());
        assert!(!changed);
        assert_eq!(s, Settings::default());
        for t in tabs_index(&TABS_EN) {
            assert!(texts.iter().any(|(x, _)| x == t), "{t} is drawn");
        }
    }

    #[test]
    fn words_exist_in_both_languages() {
        for w in [&TABS_EN, &TABS_JA] {
            assert!(tabs_index(w).iter().all(|t| !t.is_empty()));
            assert_eq!(tabs_index(w).len(), 6 + 3 + 2 + 2);
        }
        assert_eq!(TabsWords::of("ja").show_bar, "タブバーを表示");
        assert_eq!(TabsWords::of("fr").show_bar, "Show the tab bar");
        assert!(TABS_EN.outside_note.contains("another program"));
    }

    #[test]
    fn clicking_a_part_changes_only_that_setting() {
        let ctx = egui::Context::default();
        let mut s = Settings::default();
        let (_, texts) = run(&ctx, &TABS_EN, &mut s, Vec::new());
        let at = texts.iter().find(|(t, _)| t == "Never").expect("Never is drawn").1.center();
        let btn = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        run(&ctx, &TABS_EN, &mut s, vec![egui::Event::PointerMoved(at)]);
        run(&ctx, &TABS_EN, &mut s, vec![btn(true)]);
        let (changed, _) = run(&ctx, &TABS_EN, &mut s, vec![btn(false)]);
        assert!(changed);
        assert_eq!(s.show_bar, ShowBar::Never);
        assert_eq!(s.reopen, Reopen::LastTabs);
        assert_eq!(s.open_from_outside, OpenFromOutside::NewTab);
    }
}
