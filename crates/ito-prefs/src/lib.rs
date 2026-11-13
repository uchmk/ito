//! uchmk's settings screen, as every app draws it (`docs/common-spec.md` in ito):
//! a nav down the left -- the search, the pages, a button for the file --
//! and the page on the right, its title and lead over cards of rows, each
//! row its label and note on the left and its control on the right.
//!
//! An app hands [`show`] its pages and draws each page's cards in the
//! closure, with [`section`], [`row`] and the controls here. The pages every
//! app shares -- the language, the clock, the theme -- are here too
//! ([`language_row`], [`clock_card`], [`theme_page`]); they hand back
//! [`CommonChange`]s, which go into common.toml
//! ([`ito_common::edit_common`]), so one change reaches every app. So is the
//! Notifications page ([`notify_page`]), whose settings are the app's.

use std::collections::HashMap;

use egui::{Color32, FontId, RichText};
use ito_common::{quote, scale_change, Clock, CommonChange, DATE_FORMATS};
use ito_theme::{Colors, Theme};

mod apps;
mod keys;
mod mark;
mod more;
mod nav;
pub use keys::{keys_index, keys_page, Capture, KeyTable, KeyWords, KeysEdit, KEYS_EN, KEYS_JA};
pub use mark::{icon_button, mark, mark_button, IconButton, Mark};
mod compose;
mod date;
mod emoji;
mod form;
mod pairs;
mod pick;
#[cfg(test)]
mod shots;
pub use chrono::{NaiveDate, NaiveTime};
pub use compose::{chips, chips_with, ChipsResponse, FloatCard, FloatResponse, FloatWords, SplitButton, SplitResponse};
pub use date::{format_date, month_grid, month_picker, parse_date, shift_months, DateField, DateResponse, DateWords, DayLook, RangeField};
pub use emoji::{emoji_picker, emoji_picker_open, EmojiWords, CATEGORIES as EMOJI_CATEGORIES};
pub use form::{fieldset, form_label, CodeBox, FormWords, InputField, Need};
pub use pairs::{PairWords, Pairs, PairsEditor, PairsResponse};
pub use pick::{format_time, parse_time, ComboField, DurationField, FieldResponse, FieldWords, NumberField, TimeField, Unit};
mod parts;
pub use parts::{attachment, avatar, empty_state, notice, pill, AttachmentResponse};
mod notify;
mod notify_cfg;
mod openin;
mod secret;
mod shell;
#[cfg(feature = "tabs")]
mod tabs;
mod tag;
mod updates;
pub use apps::{add_edit, apps_card, note as apps_note, AppsForm, AppsWords};
pub use more::{
    about_index, about_page, about_text, advanced_card, advanced_index, backup_card, backup_index, default_order, default_order_grouped,
    fonts_card, fonts_index, forget_place, keeps_place, mono_font_card, mono_font_index, startup_card, startup_card_with, startup_index,
    window_card, window_index, About, AboutAction, Advanced, Backup, FontChange, Fonts, MoreWords, Page, WindowOut, FONT_KEYS,
    MONO_FONT_KEY, MORE_EN, MORE_JA, WINDOW_KEYS,
};
pub use nav::{nav_row, Row as NavRow};
pub use notify::{
    format_name, hook_label, mail_label, mail_rows, new_mail, notify_index, notify_page, sound_name, webhook_rows, write_mails,
    write_webhooks, End, Hook, Notify, Test,
};
pub use notify_cfg::{notify_window, take_clicks, test as test_notify, test_mail, Event, HookCfg, MailCfg, NotifyCfg, DONE_FAILED};
pub use openin::{found_line, line_arg_edit, open_card, tool_choices, user_tool_edit, write_edits, OpenForm, OpenWords};
pub use secret::{secret_field, taken as secret_taken, Hidden, SecretField, SecretResponse, SecretWords};
pub use shell::{env_changes, shell_card, shell_index, Shell, ShellChange, ShellEdit, ShellWords, SHELL_EN, SHELL_JA, SHELL_KEYS};
#[cfg(feature = "tabs")]
pub use tabs::{tabs_index, tabs_rows, tabs_rows_in, TabsWords, TABS_EN, TABS_JA, TABS_KEYS};
pub use tag::{
    fit as fit_tag, paint_tag, parse_hex, settle_tag_colors, tag, tag_colors, tag_hue, tag_picker, tag_slot, tag_width, Tag, TAG_PALETTE,
};
pub use updates::{status as update_status, update_card, UpdateWords};

/// The words the screen and the shared pages say, in one language.
pub struct Words {
    pub search: &'static str,
    pub nothing: &'static str,
    /// Before the file's name on the button at the foot of the nav.
    pub open: &'static str,
    pub language: &'static str,
    pub auto: &'static str,
    pub scale: &'static str,
    pub clock: &'static str,
    pub show_time: &'static str,
    pub show_time_note: &'static str,
    pub time_format: &'static str,
    pub hour24: &'static str,
    pub hour12: &'static str,
    pub show_date: &'static str,
    pub show_date_note: &'static str,
    pub date_format: &'static str,
    pub weekday: &'static str,
    pub mode: &'static str,
    pub follow_os: &'static str,
    pub light: &'static str,
    pub dark: &'static str,
    pub themes: &'static str,
    pub when_light: &'static str,
    pub when_dark: &'static str,
    pub preview: &'static str,
    pub notify: &'static str,
    pub notify_when: &'static str,
    pub notify_system: &'static str,
    pub notify_system_note: &'static str,
    pub notify_sound: &'static str,
    pub notify_sound_note: &'static str,
    pub notify_webhook: &'static str,
    pub notify_sounds: &'static str,
    pub sound_play: &'static str,
    /// The names of [`ito_notify::SOUNDS`], in order.
    pub sound_names: [&'static str; 4],
    pub focus: &'static str,
    pub focus_note: &'static str,
    pub notify_test: &'static str,
    pub notify_reset: &'static str,
    pub webhook: &'static str,
    pub webhook_url: &'static str,
    pub webhook_url_note: &'static str,
    /// The switch that keeps the addresses in the OS credential store, not in the file.
    pub webhook_hide: &'static str,
    pub webhook_hide_note: &'static str,
    pub webhook_bad: &'static str,
    pub webhook_format: &'static str,
    pub webhook_format_note: &'static str,
    /// A webhook whose address is still empty.
    pub webhook_empty: &'static str,
    pub webhook_add: &'static str,
    pub webhook_remove: &'static str,
    pub webhook_test: &'static str,
    /// The row of "Add an address" when there is no webhook.
    pub webhook_none: &'static str,
    /// The row of "Add an address" after the webhooks.
    pub webhook_more: &'static str,
    /// The mail places (`[[notify.mails]]`): the label of a place (numbered when there are several), its server,
    /// port, sender, recipients, password row, the warning for a password that is not a `secret:` reference, the
    /// buttons, and the row of "Add" when there is none / after the places.
    pub mail: &'static str,
    pub mail_host: &'static str,
    pub mail_host_note: &'static str,
    pub mail_port: &'static str,
    pub mail_port_note: &'static str,
    pub mail_from: &'static str,
    pub mail_to: &'static str,
    pub mail_to_note: &'static str,
    pub mail_password: &'static str,
    /// The password row's note: `{}` is the command that stores it (`koyomi secret set mail-ops`).
    pub mail_password_note: &'static str,
    /// A password written in the file itself: it is never shown, sent or kept.
    pub mail_password_plain: &'static str,
    pub mail_empty: &'static str,
    pub mail_add: &'static str,
    pub mail_remove: &'static str,
    pub mail_test: &'static str,
    pub mail_none: &'static str,
    pub mail_more: &'static str,
    /// The Feedback card: its heading, the row's label and note, the button.
    pub bug_head: &'static str,
    pub bug_label: &'static str,
    pub bug_note: &'static str,
    pub bug_button: &'static str,
}

pub const EN: Words = Words {
    search: "🔍  Search settings",
    nothing: "Nothing matches",
    open: "Open",
    language: "Language",
    auto: "Auto (system)",
    scale: "Scale",
    clock: "CLOCK",
    show_time: "Show the time in the status bar",
    show_time_note: "Stays visible when the window is maximized or full screen",
    time_format: "Time format",
    hour24: "14:32 (24-hour)",
    hour12: "2:32 PM (12-hour)",
    show_date: "Show the date",
    show_date_note: "Next to the time",
    date_format: "Date format",
    weekday: "Show the weekday",
    mode: "Mode",
    follow_os: "Follow OS",
    light: "Light",
    dark: "Dark",
    themes: "THEME",
    when_light: "when light",
    when_dark: "when dark",
    preview: "PREVIEW",
    notify: "Notifications",
    notify_when: "WHEN TO TELL",
    notify_system: "System notification",
    notify_system_note: "The OS's notification; it stays in the notification centre",
    notify_sound: "Sound",
    notify_sound_note: "The sound picked below, once",
    notify_webhook: "Webhook",
    notify_sounds: "SOUNDS",
    sound_play: "Play",
    sound_names: ["Soft chime", "Low tone", "Alert", "System default"],
    focus: "Keep quiet in Windows' focus mode",
    focus_note: "No sound during focus mode, a presentation or a full-screen game (Windows only)",
    notify_test: "Send a test",
    notify_reset: "Reset to defaults",
    webhook: "WEBHOOK",
    webhook_url: "Address",
    webhook_url_note:
        "An ntfy topic for the phone, a Slack or Discord webhook, or any address that takes JSON; nothing is sent while it is empty",
    webhook_hide: "Keep the addresses in the system's credential store",
    webhook_hide_note: "A Slack or Discord address carries its token. The file then holds only a reference (secret:webhook-1), so sharing or committing it leaks nothing",
    webhook_bad: "Start the address with http:// or https://; nothing is sent until then",
    webhook_format: "Format",
    webhook_format_note: "What the address takes",
    webhook_empty: "The address is not filled in yet",
    webhook_add: "Add an address",
    webhook_remove: "Remove",
    webhook_test: "Send a test here",
    webhook_none: "Nothing is sent elsewhere until an address is added",
    webhook_more: "Another address",
    mail: "Mail",
    mail_host: "Server",
    mail_host_note: "The SMTP server; always encrypted",
    mail_port: "Port",
    mail_port_note: "587 starts TLS on the connection; 465 is TLS from the start",
    mail_from: "From",
    mail_to: "To",
    mail_to_note: "One or more addresses, separated by commas",
    mail_password: "Password",
    mail_password_note: "Kept in the system's credential store; the file holds only a reference. Store it with: {}",
    mail_password_plain: "The password is written in the file, so it is not used. Replace it with a secret: reference",
    mail_empty: "The server or an address is not filled in yet",
    mail_add: "Add a mail place",
    mail_remove: "Remove",
    mail_test: "Send a test here",
    mail_none: "No mail is sent until a place is added",
    mail_more: "Another mail place",
    bug_head: "FEEDBACK",
    bug_label: "Report a bug",
    bug_note: "Opens a GitHub issue with the version and system already filled in (F12)",
    bug_button: "Report a bug",
};

pub const JA: Words = Words {
    search: "🔍  設定を検索",
    nothing: "当てはまる設定はありません",
    open: "開く:",
    language: "言語",
    auto: "自動（OS に従う）",
    scale: "倍率",
    clock: "時計",
    show_time: "ステータスバーに時刻を出す",
    show_time_note: "窓を最大化・全画面にしても出たまま",
    time_format: "時刻の形",
    hour24: "14:32（24 時間）",
    hour12: "2:32 PM（12 時間）",
    show_date: "日付を出す",
    show_date_note: "時刻の隣に",
    date_format: "日付の形",
    weekday: "曜日を出す",
    mode: "モード",
    follow_os: "OS に従う",
    light: "ライト",
    dark: "ダーク",
    themes: "テーマ",
    when_light: "ライトのとき",
    when_dark: "ダークのとき",
    preview: "見本",
    notify: "通知",
    notify_when: "知らせるとき",
    notify_system: "システムの通知",
    notify_system_note: "OS の通知。通知センターにも残ります",
    notify_sound: "音",
    notify_sound_note: "下で選んだ音を 1 回",
    notify_webhook: "Webhook",
    notify_sounds: "鳴らす音",
    sound_play: "鳴らす",
    sound_names: ["やわらかいチャイム", "低い音", "警告音", "システムの既定"],
    focus: "Windows の集中モードに従う",
    focus_note: "集中モード・プレゼンテーション・全画面のゲームの間は音を鳴らしません（Windows だけ）",
    notify_test: "試しに送る",
    notify_reset: "既定に戻す",
    webhook: "Webhook",
    webhook_url: "アドレス",
    webhook_url_note: "ntfy のトピック・Slack や Discord の webhook・JSON を受けるアドレス。空なら送りません",
    webhook_hide: "アドレスを OS の資格情報ストアに置く",
    webhook_hide_note: "Slack や Discord のアドレスは token を含みます。ファイルには参照（secret:webhook-1）だけが残り、ファイルを共有・コミットしても漏れません",
    webhook_bad: "http:// か https:// で始めてください。それまでは送りません",
    webhook_format: "形式",
    webhook_format_note: "アドレスが受ける形",
    webhook_empty: "アドレスがまだ空です",
    webhook_add: "アドレスを足す",
    webhook_remove: "外す",
    webhook_test: "ここに試しに送る",
    webhook_none: "アドレスを足すまで、ほかへは送りません",
    webhook_more: "別のアドレス",
    mail: "メール",
    mail_host: "サーバー",
    mail_host_note: "SMTP サーバー。通信は必ず暗号化します",
    mail_port: "ポート",
    mail_port_note: "587 は接続の途中から TLS、465 は最初から TLS",
    mail_from: "差出人",
    mail_to: "宛先",
    mail_to_note: "アドレスをコンマで区切って 1 つ以上",
    mail_password: "パスワード",
    mail_password_note: "OS の資格情報ストアに置き、ファイルには参照だけが残ります。次で登録します: {}",
    mail_password_plain: "パスワードがファイルにそのまま書かれているため使いません。secret: の参照に置き換えてください",
    mail_empty: "サーバーかアドレスがまだ空です",
    mail_add: "メールの送り先を足す",
    mail_remove: "外す",
    mail_test: "ここに試しに送る",
    mail_none: "送り先を足すまで、メールは送りません",
    mail_more: "別のメールの送り先",
    bug_head: "フィードバック",
    bug_label: "不具合を報告",
    bug_note: "版と環境を埋めた GitHub の issue を開きます（F12）",
    bug_button: "不具合を報告",
};

impl Words {
    /// The words for a language code (`ja`, `en`); English for the rest.
    pub fn of(language: &str) -> &'static Words {
        match language {
            "ja" => &JA,
            _ => &EN,
        }
    }
}

/// The screen's own state: the page, the search, and whether a control had
/// the keys at the end of the last frame.
#[derive(Default)]
pub struct State {
    /// The page, as an index into [`Nav::pages`].
    pub page: usize,
    /// What the search field above the pages holds.
    pub query: String,
    /// A control had the keys at the end of the last frame: an Esc then
    /// leaves it (egui lets go of it before this frame is drawn), and only
    /// the next Esc leaves the screen.
    pub held: bool,
    /// The search field was that control. egui lets go of the focus on Esc
    /// before the frame is drawn, so only this tells the search's Esc from
    /// another control's.
    pub search_held: bool,
}

impl State {
    /// A control has the keys, or had them a moment ago: Esc is its.
    pub fn busy(&self, ui: &egui::Ui) -> bool {
        self.held || ui.memory(|m| m.focused().is_some())
    }
}

/// The screen's keys this frame, read by the app (from its keymap, or with
/// [`Keys::standard`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub close: bool,
    pub next: bool,
    pub back: bool,
    pub find: bool,
}

impl Keys {
    /// The keys as tsumugi has them, taken from egui's input: Esc closes
    /// unless `busy`; Ctrl+Tab and Ctrl+PageDown go to the next page, with
    /// Shift or PageUp to the one before, as tabs do elsewhere; Ctrl+F (Cmd+F
    /// on macOS) goes to the search. An open popup (a dropdown) keeps Esc for
    /// itself: the first Esc closes it, the next one the screen.
    pub fn standard(ui: &egui::Ui, busy: bool) -> Self {
        let busy = busy || egui::Popup::is_any_open(ui.ctx());
        let close = !busy && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        ui.input_mut(|i| {
            let ctrl = egui::Modifiers::CTRL;
            let back = i.consume_key(ctrl | egui::Modifiers::SHIFT, egui::Key::Tab) || i.consume_key(ctrl, egui::Key::PageUp);
            let next = i.consume_key(ctrl, egui::Key::Tab) || i.consume_key(ctrl, egui::Key::PageDown);
            Self { close, next, back, find: i.consume_key(egui::Modifiers::COMMAND, egui::Key::F) }
        })
    }
}

/// The pages: each one's title and lead, the rows on each (the page's
/// index, the row's words) for the search, the words and the file.
pub struct Nav<'a> {
    pub pages: &'a [(&'a str, &'a str)],
    pub index: &'a [(usize, &'a str)],
    pub words: &'a Words,
    /// The settings file's name, on the button at the foot.
    pub file: &'a str,
}

/// What [`show_with`] adds to [`show`]: nav headings, a tag on the pages ito
/// shares, a back button, and a search that reads every page at once.
/// Everything is off by default.
#[derive(Clone, Copy, Debug, Default)]
pub struct Extras<'a> {
    /// Nav headings: each one's words and the index of its first page, in
    /// order ("Notebook" at 0, "Common (ito)" at 5).
    pub groups: &'a [(&'a str, usize)],
    /// The pages the apps share (indexes into [`Nav::pages`]); each gets
    /// `shared_tag` beside its title.
    pub shared: &'a [usize],
    /// The tag on a shared page's title ("ito's common page").
    pub shared_tag: &'a str,
    /// The back button in the page's head: an arrow and these words ("Back to
    /// the notebook"), then the key (Esc). A click does what Esc does
    /// ([`Out::close`]). Empty for none.
    pub back: &'a str,
    /// With the words, a search shows the cards that hit on every page at
    /// once, and the pages that have none are dimmed in the nav; Esc then
    /// clears the search before it closes the screen.
    pub results: Option<Results<'a>>,
    /// The app's own settings files (`["koyomi.toml"]`), as buttons stacked above the common file's, in this order.
    /// A click is [`Out::open_file`]` = Some(1 + i)`. Tab reaches them before the common file.
    pub files: &'a [&'a str],
}

/// The words of the search's results page ([`Extras::results`]).
#[derive(Clone, Copy, Debug)]
pub struct Results<'a> {
    /// The heading, `{q}` standing for the search (`Settings with "{q}"`).
    pub head: &'a str,
    /// The line under it ("Searching all pages.").
    pub lead: &'a str,
    /// When nothing hits ("Not found.").
    pub none: &'a str,
}

/// What the frame was asked for, besides what the page hands back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Out {
    pub close: bool,
    /// The file button that was clicked: `Some(0)` for the common file ([`Nav::file`]), `Some(1 + i)` for
    /// [`Extras::files`]`[i]`.
    pub open_file: Option<usize>,
}

/// The colours and the search's words, for every row.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub c: Colors,
    /// The search, lower case; empty for none.
    pub q: &'a str,
}

/// `size` points at the letters' zoom (`ito_theme::set_text_zoom`), for every
/// size written in these screens.
pub(crate) fn z(ui: &egui::Ui, size: f32) -> f32 {
    ito_theme::zoomed(ui.ctx(), size)
}

/// The screen across all of `ui`: the nav and the page, which `body` draws
/// (it gets the page's index and the [`Look`]).
pub fn show(ui: &mut egui::Ui, c: Colors, state: &mut State, nav: &Nav, keys: Keys, body: impl FnMut(&mut egui::Ui, usize, Look)) -> Out {
    show_with(ui, c, state, nav, keys, Extras::default(), body)
}

/// The height one file button takes at the foot of the nav: the button and the gap above it.
const FILE_STEP: f32 = 40.0;

/// [`show`] with the [`Extras`].
///
/// With [`Extras::results`] a search calls `body` for every page that has a
/// hit, and [`section`] then draws only the cards that hit, each under
/// "Page > Card". Draw a page's content inside [`section`]s so the results
/// carry it; anything outside one is drawn whole.
pub fn show_with(
    ui: &mut egui::Ui,
    c: Colors,
    state: &mut State,
    nav: &Nav,
    keys: Keys,
    extras: Extras,
    mut body: impl FnMut(&mut egui::Ui, usize, Look),
) -> Out {
    let mut out = Out { close: keys.close, open_file: None };
    // Esc in the search clears what it holds first, and the keys stay in the
    // field. egui let go of the focus on this Esc already, so the search is
    // known by `search_held` (what had the keys at the end of the last frame).
    let search_id = egui::Id::new("prefs-search");
    if extras.results.is_some()
        && !state.query.is_empty()
        && (state.search_held || ui.memory(|m| m.has_focus(search_id)))
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        state.query.clear();
        ui.memory_mut(|m| m.request_focus(search_id));
        out.close = false;
    }
    let n = nav.pages.len().max(1);
    state.page = state.page.min(n - 1);
    if keys.next || keys.back {
        let k = state.page;
        state.page = if keys.next { (k + 1) % n } else { (k + n - 1) % n };
        state.query.clear();
        // The keys to the page's first control with the next Tab.
        ui.memory_mut(|m| m.surrender_focus(m.focused().unwrap_or(egui::Id::NULL)));
    }
    // What is left of the ui, not its `max_rect`: under a top panel (a title bar) the root ui keeps the whole window.
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0.0, c.bg);
    // The nav down the left: the search, the pages, and the file.
    let side = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + 240.0, rect.bottom()));
    ui.painter().rect_filled(side, 0.0, c.side);
    ui.painter().line_segment([side.right_top(), side.right_bottom()], egui::Stroke::new(1.0, c.border));
    let mut nav_ui = ui.new_child(egui::UiBuilder::new().max_rect(side.shrink2(egui::vec2(10.0, 16.0))));
    let search = egui::TextEdit::singleline(&mut state.query)
        .id(search_id)
        .hint_text(nav.words.search)
        .desired_width(f32::INFINITY)
        .margin(egui::vec2(10.0, 8.0))
        .font(FontId::proportional(z(ui, 12.5)));
    let searched = nav_ui.add(search);
    focus_ring(&nav_ui, &searched);
    if keys.find {
        searched.request_focus();
    }
    nav_ui.add_space(ito_theme::scale::space::MD);
    let mut q = state.query.trim().to_lowercase();
    // Esc clears the search first, when the screen reads every page.
    if extras.results.is_some() && !q.is_empty() && keys.close {
        state.query.clear();
        q.clear();
        out.close = false;
    }
    let cross = extras.results.is_some() && !q.is_empty();
    let hits = |p: usize| -> usize {
        if q.is_empty() {
            return 1;
        }
        nav.index.iter().filter(|(at, words)| *at == p && words.to_lowercase().contains(&q)).count()
            + usize::from(nav.pages[p].0.to_lowercase().contains(&q))
    };
    let with_hits: Vec<usize> = (0..nav.pages.len()).filter(|p| hits(*p) > 0).collect();
    // Reading every page, the nav keeps them all, the ones without a hit dim.
    let shown: Vec<usize> = if cross { (0..nav.pages.len()).collect() } else { with_hits.clone() };
    // The search moves to the first page that has it.
    if !q.is_empty() && !cross && !shown.contains(&state.page) {
        if let Some(first) = shown.first() {
            state.page = *first;
        }
    }
    if searched.lost_focus() && nav_ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Some(first) = with_hits.first() {
            state.page = *first;
            // The keys go on to the first row the search lights.
            if !q.is_empty() {
                ui.data_mut(|d| d.insert_temp(egui::Id::new(JUMP), true));
            }
        }
    }
    if shown.is_empty() {
        nav_ui.label(RichText::new(nav.words.nothing).size(z(ui, 12.5)).color(c.dim));
    }
    // The pages scroll when the window is too short for them, above the file's button.
    let list_room = (side.bottom() - 56.0 - FILE_STEP * extras.files.len() as f32 - nav_ui.cursor().top()).max(60.0);
    egui::ScrollArea::vertical().id_salt("prefs-nav").max_height(list_room).auto_shrink([false, true]).show(&mut nav_ui, |nav_ui| {
        let mut group = None;
        for p in shown {
            let at = extras.groups.iter().rposition(|(_, start)| *start <= p);
            if at != group {
                group = at;
                if let Some((head, _)) = at.map(|g| extras.groups[g]) {
                    nav_ui.add_space(if p == 0 { 2.0 } else { 10.0 });
                    nav_ui.label(RichText::new(head).size(z(ui, 11.0)).strong().color(c.faint()));
                    nav_ui.add_space(2.0);
                }
            }
            let on = !cross && state.page == p;
            // Mouse only: the keys go page to page with Ctrl+Tab, and Tab goes
            // from the search straight into the page.
            let (r, resp) = nav_ui.allocate_exact_size(egui::vec2(nav_ui.available_width(), z(nav_ui, 32.0)), egui::Sense::CLICK);
            if on {
                nav_ui.painter().rect_filled(r, 6.0, c.chosen());
            } else if resp.hovered() {
                nav_ui.painter().rect_filled(r, 6.0, c.hover());
            }
            let color = if on {
                c.strong()
            } else if cross && hits(p) == 0 {
                c.faint()
            } else {
                c.dim
            };
            nav_ui.painter().text(
                egui::pos2(r.left() + 10.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                nav.pages[p].0,
                FontId::proportional(z(ui, 13.0)),
                color,
            );
            if !q.is_empty() {
                nav_ui.painter().text(
                    egui::pos2(r.right() - 10.0, r.center().y),
                    egui::Align2::RIGHT_CENTER,
                    hits(p).to_string(),
                    FontId::proportional(z(ui, 11.0)),
                    c.faint(),
                );
            }
            if resp.clicked() {
                state.page = p;
                // A page picked from the results leaves them.
                if cross {
                    state.query.clear();
                }
            }
        }
    });

    // The page.
    let l = Look { c, q: &q };
    let area = egui::Rect::from_min_max(egui::pos2(side.right() + 1.0, rect.top()), rect.max);
    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
    let page = state.page;
    let (title, lead) = nav.pages.get(page).copied().unwrap_or(("", ""));
    let results = extras.results.filter(|_| cross);
    let heading = match results {
        Some(r) => r.head.replace("{q}", state.query.trim()),
        None => title.to_owned(),
    };
    let lead = results.map_or(lead, |r| r.lead);
    // A narrow window keeps the rows a readable width: the margins give way.
    let side_margin = (area.width() * 0.1).clamp(16.0, 48.0) as i8;
    egui::ScrollArea::vertical().id_salt(("prefs-page", title)).auto_shrink([false, false]).show(&mut body_ui, |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin { left: side_margin, right: side_margin, top: 28, bottom: 40 }).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(820.0));
            if !extras.back.is_empty() && results.is_none() {
                ui.horizontal(|ui| {
                    let back = egui::Button::new(RichText::new(format!("\u{2190} {}", extras.back)).size(z(ui, 12.5)).color(c.dim))
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(1.0, c.border_strong()))
                        .corner_radius(ito_theme::scale::radius::CONTROL);
                    let back = ui.add(back);
                    focus_ring(ui, &back);
                    ui.label(RichText::new("Esc").size(z(ui, 11.0)).color(c.faint()));
                    out.close |= back.clicked();
                });
                ui.add_space(ito_theme::scale::space::MD);
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(heading).size(z(ui, 24.0)).strong().color(c.strong()));
                if results.is_none() && !extras.shared_tag.is_empty() && extras.shared.contains(&page) {
                    egui::Frame::NONE
                        .stroke(egui::Stroke::new(1.0, c.border_strong()))
                        .corner_radius(ito_theme::scale::radius::CONTROL)
                        .inner_margin(egui::Margin::symmetric(8, 2))
                        .show(ui, |ui| {
                            ui.label(RichText::new(extras.shared_tag).size(z(ui, 11.0)).color(c.dim));
                        });
                }
            });
            ui.add_space(2.0);
            ui.label(RichText::new(lead).size(z(ui, 13.5)).color(c.dim));
            ui.add_space(ito_theme::scale::space::LG);
            match results {
                None => body(ui, page, l),
                Some(r) => {
                    if with_hits.is_empty() {
                        ui.label(RichText::new(r.none).size(z(ui, 13.0)).color(c.dim));
                    }
                    for p in &with_hits {
                        let at = egui::Id::new(RESULTS);
                        ui.data_mut(|d| d.insert_temp(at, nav.pages[*p].0.to_owned()));
                        body(ui, *p, l);
                        ui.data_mut(|d| d.remove::<String>(at));
                    }
                }
            }
        });
    });
    // No row lit took the keys: they stay where the search left them.
    ui.data_mut(|d| d.remove::<bool>(egui::Id::new(JUMP)));
    // The files last, so Tab comes to them after the page: the app's own, then the common one at the foot.
    let n = extras.files.len();
    let mut file = None;
    for j in 0..=n {
        let name = if j < n { extras.files[j] } else { nav.file };
        let top = side.bottom() - 48.0 - FILE_STEP * (n - j) as f32;
        let foot = egui::Rect::from_min_max(egui::pos2(side.left() + 10.0, top), egui::pos2(side.right() - 10.0, top + 32.0));
        let words = format!("{} {}", nav.words.open, name);
        let button = egui::Button::new(RichText::new(words).size(z(ui, 12.5)).color(c.dim))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0, c.border_strong()))
            .corner_radius(ito_theme::scale::radius::CONTROL);
        let button = nav_ui.put(foot, button);
        focus_ring(&nav_ui, &button);
        if button.clicked() {
            out.open_file = Some(if j < n { j + 1 } else { 0 });
        }
        file = Some(button);
    }
    let file = file.expect("the common file's button is always drawn");
    // Tab off the file, the last stop, leaves egui with nothing to hand the
    // keys to until the next frame comes round to the search: hand them
    // over now, so no frame has nothing (tsumugi #61, #106). The file is
    // named again after, so Shift+Tab from the search still comes to it.
    if ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.modifiers.is_none() && i.key_pressed(egui::Key::Tab)) {
        ui.memory_mut(|m| {
            m.interested_in_focus(searched.id, nav_ui.layer_id());
            m.interested_in_focus(file.id, nav_ui.layer_id());
        });
    }
    state.held = ui.memory(|m| m.focused().is_some());
    state.search_held = ui.memory(|m| m.has_focus(search_id));
    out
}

/// The search is reading every page ([`Extras::results`]): a page then draws
/// only its cards, not what stands outside them (buttons at its top).
pub fn in_results(ui: &egui::Ui) -> bool {
    ui.data(|d| d.get_temp::<String>(egui::Id::new(RESULTS))).is_some()
}

/// While the search reads every page: the title of the page being drawn.
const RESULTS: &str = "prefs-results";
/// Set by a row the search lights, so [`section`] can tell a card has a hit.
const HIT_SEEN: &str = "prefs-hit-seen";

/// A row the search lights says so (for the results, which keep only the
/// cards that have one). Custom rows call this when they light.
pub(crate) fn mark_hit(ui: &egui::Ui) {
    ui.data_mut(|d| d.insert_temp(egui::Id::new(HIT_SEEN), true));
}

/// Enter in the search: the first row it lights takes the keys.
const JUMP: &str = "prefs-jump";
/// The row taking the keys: its control takes them.
const JUMP_HERE: &str = "prefs-jump-here";

/// The control `r` takes the keys, when the row it is in is to have them.
fn take_keys(ui: &egui::Ui, r: &egui::Response) {
    if ui.data_mut(|d| d.remove_temp::<bool>(egui::Id::new(JUMP_HERE))).is_some() {
        r.request_focus();
    }
}

/// A card of rows under a small heading.
///
/// While the search reads every page ([`Extras::results`]) a card with no hit
/// is left out, and the heading reads "Page > Card". `rows` is then run once
/// unseen to find the hit, so keep it free of side effects.
pub fn section(ui: &mut egui::Ui, l: Look, head: &str, mut rows: impl FnMut(&mut egui::Ui)) {
    let c = l.c;
    let hit = !l.q.is_empty() && head.to_lowercase().contains(l.q);
    let page = ui.data(|d| d.get_temp::<String>(egui::Id::new(RESULTS)));
    let label = match &page {
        Some(page) => format!("{page} \u{203a} {head}"),
        None => head.to_owned(),
    };
    if page.is_some() && !hit {
        let seen = egui::Id::new(HIT_SEEN);
        ui.data_mut(|d| d.remove::<bool>(seen));
        let mut dry = ui.new_child(egui::UiBuilder::new().id_salt("prefs-dry").max_rect(ui.available_rect_before_wrap()).invisible());
        rows(&mut dry);
        if ui.data_mut(|d| d.remove_temp::<bool>(seen)).is_none() {
            return;
        }
    }
    ui.label(RichText::new(label).size(z(ui, 11.0)).strong().color(if hit { c.wait } else { c.dim }));
    ui.add_space(ito_theme::scale::space::SM);
    egui::Frame::NONE
        .fill(c.panel)
        .stroke(egui::Stroke::new(1.0, c.border))
        .corner_radius(ito_theme::scale::radius::BOX)
        .inner_margin(egui::Margin::symmetric(18, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            rows(ui);
        });
    ui.add_space(ito_theme::scale::space::XL);
}

/// The line between two rows of a card, across the whole card.
pub fn sep(ui: &mut egui::Ui, l: Look) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(r.x_range().expand(18.0), r.center().y, egui::Stroke::new(1.0, l.c.border));
}

/// A row: its label and note on the left, its control on the right. Lit
/// when the search's words are in it.
pub fn row<R>(ui: &mut egui::Ui, l: Look, label: &str, note: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    keyed_row(ui, l, "", label, note, control)
}

/// A [`row`] the search also finds by its key in the settings file
/// (`ui.font_size`, `clock.show`), which it does not show: the same words
/// reach it whatever the language, as a `--keys` script needs (kura #388).
/// Put the key in [`Nav::index`] too.
pub fn keyed_row<R>(ui: &mut egui::Ui, l: Look, key: &str, label: &str, note: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let c = l.c;
    let hit = !l.q.is_empty() && [key, label, note].iter().any(|w| w.to_lowercase().contains(l.q));
    if hit {
        mark_hit(ui);
    }
    let behind = ui.painter().add(egui::Shape::Noop);
    let mut r = None;
    let shown = ui.horizontal(|ui| {
        ui.set_min_height(z(ui, 48.0));
        // The control first, on the right; the words wrap in what is left,
        // never under it.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let jump = hit && ui.is_visible() && ui.data_mut(|d| d.remove_temp::<bool>(egui::Id::new(JUMP))).is_some();
            if jump {
                ui.data_mut(|d| d.insert_temp(egui::Id::new(JUMP_HERE), true));
            }
            r = Some(control(ui));
            if jump {
                ui.data_mut(|d| d.remove::<bool>(egui::Id::new(JUMP_HERE)));
            }
            ui.add_space(ito_theme::scale::space::LG);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width());
                    // A label alone sits in the middle of the row, as the
                    // control does.
                    ui.add_space(if note.is_empty() { 15.0 } else { 8.0 });
                    ui.label(RichText::new(label).size(z(ui, 13.0)).color(c.strong()));
                    if !note.is_empty() {
                        ui.label(RichText::new(note).size(z(ui, 12.0)).color(c.dim));
                    }
                    ui.add_space(ito_theme::scale::space::SM);
                });
            });
        });
    });
    if hit {
        let lit = egui::Shape::rect_filled(shown.response.rect.expand2(egui::vec2(10.0, 0.0)), 6.0, c.wait.gamma_multiply(0.14));
        ui.painter().set(behind, lit);
    }
    scroll_to_first_hit(ui, hit, l.q, shown.response.rect);
    r.expect("the control ran")
}

/// The query whose first hit has been scrolled to.
const SCROLLED: &str = "prefs-scrolled-to";

/// Scrolls the page to the first row a search lights, once per query: a hit
/// low on a long page would otherwise be off-screen (kura #443). Rows come
/// top to bottom, so the first lit one in a pass is the first hit.
fn scroll_to_first_hit(ui: &egui::Ui, hit: bool, q: &str, rect: egui::Rect) {
    // Not in the results (the cards come top to bottom) nor in a dry run.
    if !ui.is_visible() || ui.data(|d| d.get_temp::<String>(egui::Id::new(RESULTS))).is_some() {
        return;
    }
    let id = egui::Id::new(SCROLLED);
    if q.is_empty() {
        ui.data_mut(|d| d.remove::<String>(id));
    } else if hit && ui.data(|d| d.get_temp::<String>(id)).as_deref() != Some(q) {
        ui.data_mut(|d| d.insert_temp(id, q.to_owned()));
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
}

/// The design's switch: a pill with a knob, cyan when on.
pub fn switch(ui: &mut egui::Ui, l: Look, on: bool) -> bool {
    let c = l.c;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(z(ui, 40.0), z(ui, 22.0)), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 11.0, if on { c.run } else { c.border_strong() });
    let x = if on { rect.right() - 11.0 } else { rect.left() + 11.0 };
    p.circle_filled(egui::pos2(x, rect.center().y), 8.0, if on { c.on_accent() } else { c.dim });
    take_keys(ui, &resp);
    focus_ring(ui, &resp);
    resp.clicked()
}

/// How far [`focus_ring`] reaches out of its widget, in points: a gap of 1.5
/// and a stroke of 1.5. It is egui's default vertical item spacing, so two
/// widgets laid out with that spacing (or more) never get each other's ring.
/// Leave this much round a widget that has a ring when laying it closer.
pub const FOCUS_ROOM: f32 = RING_GAP + RING_WIDTH;

const RING_GAP: f32 = 1.5;
const RING_WIDTH: f32 = 1.5;

/// A ring round the control that has the keys, so Tab shows where it is.
///
/// It reaches [`FOCUS_ROOM`] out of the widget. A side the ui's clip would
/// cut (a widget flush with the top of a scroll area) gives up its gap and is
/// drawn just inside the widget's edge, so a widget inside the clip always
/// gets its whole ring. A side whose widget edge is itself out of the clip
/// (scrolled partly out of view) is left alone, so no line is drawn across the
/// widget. A widget too thin for the ring inside its clip gets the ring
/// outside, cut as it falls.
pub fn focus_ring(ui: &egui::Ui, r: &egui::Response) {
    if r.has_focus() {
        let rect = ring_rect(r.rect, ui.clip_rect(), ui.ctx().pixels_per_point());
        ui.painter().rect_stroke(rect, 8.0, egui::Stroke::new(RING_WIDTH, ito_theme::colors().run), egui::StrokeKind::Outside);
    }
}

/// The rect whose outside stroke is the focus ring of `widget` in `clip`.
fn ring_rect(widget: egui::Rect, clip: egui::Rect, pixels_per_point: f32) -> egui::Rect {
    let r = ring_in_clip(widget, clip, pixels_per_point);
    if r.is_positive() {
        r
    } else {
        widget.expand(RING_GAP)
    }
}

fn ring_in_clip(widget: egui::Rect, clip: egui::Rect, pixels_per_point: f32) -> egui::Rect {
    // Half a pixel more than the stroke: egui rounds the rect to pixels
    // before it strokes it.
    let room = RING_WIDTH + 0.5 / pixels_per_point.max(0.1);
    let mut r = widget.expand(RING_GAP);
    if widget.top() >= clip.top() {
        r.min.y = r.min.y.max(clip.top() + room);
    }
    if widget.left() >= clip.left() {
        r.min.x = r.min.x.max(clip.left() + room);
    }
    if widget.bottom() <= clip.bottom() {
        r.max.y = r.max.y.min(clip.bottom() - room);
    }
    if widget.right() <= clip.right() {
        r.max.x = r.max.x.min(clip.right() - room);
    }
    r
}

/// A list to pick one of: the value picked, when it changed.
///
/// The keys stay with the button the whole time, so the list is used from the
/// keyboard alone: Enter or Space opens it with the current value lit, Up and
/// Down (Home and End) move the light, Enter or Space picks and shuts it, Esc
/// shuts it as it was, and Tab shuts it and goes on to the next control.
pub fn select<T: PartialEq + Copy>(ui: &mut egui::Ui, id: &str, now: T, options: &[(T, &str)]) -> Option<T> {
    SelectField::new(id).show(ui, now, options)
}

/// [`select`] with more: an error state and a short note per option.
pub struct SelectField<'a> {
    id: &'a str,
    error: bool,
    notes: &'a [&'a str],
}

impl<'a> SelectField<'a> {
    pub fn new(id: &'a str) -> Self {
        SelectField { id, error: false, notes: &[] }
    }

    /// Show the value as wrong (red border and glow, like `InputField::error`), and say it to a screen reader.
    pub fn error(mut self, on: bool) -> Self {
        self.error = on;
        self
    }

    /// One note per option, in the order of the options (a missing or empty one is no note). A row of the open list shows its
    /// note on hover, the closed button the note of the option it shows; a screen reader hears them as the description.
    pub fn notes(mut self, notes: &'a [&'a str]) -> Self {
        self.notes = notes;
        self
    }

    /// The list; the value picked, when it changed. Test IDs as for [`select`].
    pub fn show<T: PartialEq + Copy>(self, ui: &mut egui::Ui, now: T, options: &[(T, &str)]) -> Option<T> {
        let (pick, resp) = select_with_response(ui, self.id, now, options, self.error, self.notes);
        // Only a name `is_test_id` accepts: app ids may hold other characters, and those get no ID.
        let name = format!("settings.select.{}", self.id);
        if ito_a11y::is_test_id(&name) {
            ito_a11y::test_id(&resp, &name);
        }
        pick
    }
}

/// [`select`] and the list button's response, for a caller that gives the button a test ID.
pub(crate) fn select_with_response<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    id: &str,
    now: T,
    options: &[(T, &str)],
    error: bool,
    notes: &[&str],
) -> (Option<T>, egui::Response) {
    let mut pick = now;
    let shown = options.iter().find(|(v, _)| *v == now).map_or("", |(_, t)| *t).to_owned();
    let salt = ("prefs", id);
    let button = ui.make_persistent_id(egui::IdSalt::new(salt));
    // egui keeps the list under this id (`ComboBox::widget_to_popup_id`, private).
    let popup = button.with("popup");
    let light_id = popup.with("light");
    let ctx = ui.ctx().clone();
    let open = egui::Popup::is_id_open(&ctx, popup);
    let has_keys = ui.memory(|m| m.has_focus(button));
    let mut light = if open { ui.data(|d| d.get_temp::<usize>(light_id)) } else { None };
    let mut moved = false;
    if open && has_keys {
        let at = light.unwrap_or_else(|| options.iter().position(|(v, _)| *v == now).unwrap_or(0));
        let last = options.len().saturating_sub(1);
        let (up, down, home, end, enter, esc, tab) = ui.input_mut(|i| {
            let none = egui::Modifiers::NONE;
            (
                i.consume_key(none, egui::Key::ArrowUp),
                i.consume_key(none, egui::Key::ArrowDown),
                i.consume_key(none, egui::Key::Home),
                i.consume_key(none, egui::Key::End),
                i.consume_key(none, egui::Key::Enter) | i.consume_key(none, egui::Key::Space),
                i.consume_key(none, egui::Key::Escape),
                i.key_pressed(egui::Key::Tab),
            )
        });
        let to = if home {
            0
        } else if end {
            last
        } else if up {
            at.saturating_sub(1)
        } else if down {
            (at + 1).min(last)
        } else {
            at
        };
        moved = to != at || light.is_none();
        light = Some(to);
        if enter {
            if let Some((v, _)) = options.get(to) {
                pick = *v;
            }
        }
        if enter || esc || tab {
            egui::Popup::close_id(&ctx, popup);
        }
    }
    let combo = egui::ComboBox::from_id_salt(salt).selected_text(shown).width(190.0).show_ui(ui, |ui| {
        for (k, (v, t)) in options.iter().enumerate() {
            let behind = ui.painter().add(egui::Shape::Noop);
            let mut r = ui.selectable_value(&mut pick, *v, *t);
            if let Some(note) = notes.get(k).filter(|n| !n.is_empty()) {
                r = r.on_hover_text(*note);
                ui.ctx().accesskit_node_builder(r.id, |n| n.set_description(*note));
            }
            let name = format!("settings.select.{id}.{k}");
            if ito_a11y::is_test_id(&name) {
                ito_a11y::test_id(&r, &name);
            }
            if light == Some(k) {
                ui.painter().set(behind, egui::Shape::rect_filled(r.rect, 3.0, ito_theme::colors().wait.gamma_multiply(0.25)));
                if moved {
                    r.scroll_to_me(None);
                }
            }
        }
    });
    if combo.inner.is_some() && ui.memory(|m| m.has_focus(button)) {
        // The arrows and Esc are the list's while it is open, from the next
        // frame on (egui reads the filter as the keys come in): it must not
        // move the focus off the button or drop it.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(button, egui::EventFilter { tab: false, horizontal_arrows: false, vertical_arrows: true, escape: true })
        });
    }
    ui.data_mut(|d| match light {
        Some(k) if combo.inner.is_some() => {
            d.insert_temp(light_id, k);
        }
        _ => {
            d.remove_temp::<usize>(light_id);
        }
    });
    if pick != now {
        // A pick with the mouse leaves the keys on the list that just shut.
        ui.memory_mut(|m| m.request_focus(button));
    }
    take_keys(ui, &combo.response);
    let button_rect = combo.response.rect;
    if error {
        // Over the button's own border, then the glow outside it; both under the focus ring.
        let c = ito_theme::colors();
        let radius = ito_theme::scale::radius::CONTROL;
        ui.painter().rect_stroke(button_rect, radius, egui::Stroke::new(1.0, c.err), egui::StrokeKind::Inside);
        ui.painter().rect_stroke(
            button_rect.expand(1.5),
            7.5,
            egui::Stroke::new(2.0, ito_theme::mix(c.bg, c.err, 0.3)),
            egui::StrokeKind::Outside,
        );
        ui.ctx().accesskit_node_builder(combo.response.id, |n| n.set_invalid(egui::accesskit::Invalid::True));
    }
    let on = options.iter().position(|(v, _)| *v == now).and_then(|k| notes.get(k)).filter(|n| !n.is_empty());
    let mut response = combo.response;
    if let Some(note) = on {
        response = response.on_hover_text(*note);
        ui.ctx().accesskit_node_builder(response.id, |n| n.set_description(*note));
    }
    focus_ring(ui, &response);
    ((pick != now).then_some(pick), response)
}

/// A segmented control: one outlined group of parts, the chosen one filled.
/// For a 2 to 4 way toggle in a toolbar, where a [`select`] is too much. It is
/// 26 points high, so it fits a 30 point row.
///
/// The group is one stop for Tab; Left and Right (Home and End) move the pick
/// while it has the keys, and a click picks the part under it. The value
/// picked, when it changed.
pub fn seg<T: PartialEq + Copy>(ui: &mut egui::Ui, l: Look, id: &str, now: T, options: &[(T, &str)]) -> Option<T> {
    seg_named(ui, l, id, "", now, options).0
}

/// [`seg`] with a screen-reader `name` for the group ("Day type"; the reader hears the name and the picked option, e.g.
/// "Day type: Off") and the group's `Response` besides the changed value. An empty `name` reads just the picked option.
pub fn seg_named<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    l: Look,
    id: &str,
    name: &str,
    now: T,
    options: &[(T, &str)],
) -> (Option<T>, egui::Response) {
    let c = l.c;
    let font = FontId::proportional(z(ui, 12.5));
    let pad = z(ui, 12.0);
    let galleys: Vec<_> =
        options.iter().map(|(_, t)| ui.painter().layout_no_wrap((*t).to_owned(), font.clone(), egui::Color32::PLACEHOLDER)).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + pad * 2.0).collect();
    let size = egui::vec2(widths.iter().sum::<f32>(), z(ui, 26.0));
    let button = ui.make_persistent_id(egui::IdSalt::new(("prefs-seg", id)));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let resp = ui.interact(rect, button, egui::Sense::click());
    let at = options.iter().position(|(v, _)| *v == now);
    let mut to = at;
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
        let mut x = rect.left();
        for (k, w) in widths.iter().enumerate() {
            if pos.x < x + w {
                to = Some(k);
                break;
            }
            x += w;
        }
        ui.memory_mut(|m| m.request_focus(button));
    }
    if resp.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                button,
                egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: false, escape: false },
            )
        });
        let (left, right, home, end) = ui.input_mut(|i| {
            let none = egui::Modifiers::NONE;
            (
                i.consume_key(none, egui::Key::ArrowLeft),
                i.consume_key(none, egui::Key::ArrowRight),
                i.consume_key(none, egui::Key::Home),
                i.consume_key(none, egui::Key::End),
            )
        });
        to = seg_step(to, options.len(), left, right, home, end);
    }
    let p = ui.painter();
    p.rect_stroke(rect, 6.0, egui::Stroke::new(1.0, c.border_strong()), egui::StrokeKind::Inside);
    let mut x = rect.left();
    for (k, (w, g)) in widths.iter().zip(galleys).enumerate() {
        let part = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*w, rect.height()));
        let chosen = to == Some(k);
        if chosen {
            p.rect_filled(part.shrink(1.0), 5.0, c.run);
        } else if k > 0 && to != Some(k - 1) {
            p.line_segment(
                [part.left_top() + egui::vec2(0.0, 6.0), part.left_bottom() - egui::vec2(0.0, 6.0)],
                egui::Stroke::new(1.0, c.border),
            );
        }
        let color = if chosen { c.on_accent() } else { c.fg };
        p.galley(part.center() - g.size() / 2.0, g, color);
        x += w;
    }
    focus_ring(ui, &resp);
    let picked = to.and_then(|k| options.get(k)).map_or("", |o| o.1);
    let said = if name.is_empty() { picked.to_owned() } else { format!("{name}: {picked}") };
    ito_a11y::say(&resp, ito_a11y::Role::Button, &said);
    (to.filter(|k| Some(*k) != at).map(|k| options[k].0), resp)
}

/// Where the pick goes for this frame's keys. Left and Right stop at the ends.
fn seg_step(at: Option<usize>, len: usize, left: bool, right: bool, home: bool, end: bool) -> Option<usize> {
    let last = len.checked_sub(1)?;
    if home {
        Some(0)
    } else if end {
        Some(last)
    } else if left {
        Some(at.map_or(0, |k| k.saturating_sub(1)))
    } else if right {
        Some(at.map_or(0, |k| (k + 1).min(last)))
    } else {
        at
    }
}

/// How a [`button_with`] is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// An outline, words in the text colour.
    #[default]
    Plain,
    /// Filled with the theme's accent: the one main action on a screen.
    Primary,
    /// Filled with the theme's error colour: the action that throws something away.
    Danger,
    /// Filled with the theme's warn colour (`wait`): undoing a paused or held state, not the main action nor a destructive one.
    Warn,
}

/// The design's plain button: an outline, words in the text colour.
pub fn button(ui: &mut egui::Ui, l: Look, words: &str) -> bool {
    button_with(ui, l, words, Tone::Plain).clicked()
}

/// A button of the given [`Tone`]: the same size and focus ring as [`button`].
/// A filled one takes the theme's accent or error colour with the text colour
/// for words on an accent ([`ito_theme::Colors::on_accent`]), so it follows the theme.
pub fn button_with(ui: &mut egui::Ui, l: Look, words: &str, tone: Tone) -> egui::Response {
    let (fill, stroke, text) = tone_colors(&l.c, tone);
    let b = egui::Button::new(RichText::new(words).size(z(ui, 12.5)).color(text))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(ito_theme::scale::radius::CONTROL)
        .min_size(egui::vec2(0.0, z(ui, 30.0)));
    let r = ui.add(b);
    take_keys(ui, &r);
    focus_ring(ui, &r);
    r
}

/// The fill, outline and words of a button.
fn tone_colors(c: &Colors, tone: Tone) -> (Color32, egui::Stroke, Color32) {
    match tone {
        Tone::Plain => (Color32::TRANSPARENT, egui::Stroke::new(1.0, c.border_strong()), c.fg),
        Tone::Primary => (c.run, egui::Stroke::NONE, c.on_accent()),
        Tone::Danger => (c.err, egui::Stroke::NONE, c.on_accent()),
        Tone::Warn => (c.wait, egui::Stroke::NONE, ink_on(c.wait)),
    }
}

/// Dark or white words, whichever reads better on `fill`.
fn ink_on(fill: Color32) -> Color32 {
    let luma = 0.299 * fill.r() as f32 + 0.587 * fill.g() as f32 + 0.114 * fill.b() as f32;
    if luma > 140.0 {
        Color32::from_rgb(0x1a, 0x1a, 0x1a)
    } else {
        Color32::WHITE
    }
}

pub fn status(ui: &mut egui::Ui, words: &str, color: Color32) {
    ui.label(RichText::new(words).size(z(ui, 12.5)).color(color));
}

/// A text field's words while it is typed in, and just after.
pub struct Draft {
    text: String,
    /// Sent to the file, and what the file said then: shown until the file
    /// says something else, or for a moment.
    sent: Option<(String, std::time::Instant)>,
}

/// The fields' drafts, by the field's id.
pub type Drafts = HashMap<String, Draft>;

/// A one-line field that writes when Enter is pressed or it is left (Esc
/// leaves it as it was): the new words, when they differ. The words are all
/// picked when it takes the keys, so what is typed replaces them.
pub fn field(ui: &mut egui::Ui, drafts: &mut Drafts, id: &str, now: &str, hint: &str, width: f32) -> Option<String> {
    // The results' unseen pass only measures: the field's id is absolute, so
    // a second field with it would clash with the one drawn after.
    if !ui.is_visible() {
        ui.allocate_space(egui::vec2(width, z(ui, 32.0)));
        return None;
    }
    let eid = field_id(id);
    let focused = ui.memory(|m| m.has_focus(eid));
    if !focused {
        let done = drafts.get(id).is_some_and(|d| match &d.sent {
            Some((was, at)) => now != was || at.elapsed() > std::time::Duration::from_secs(2),
            None => true,
        });
        if done {
            drafts.remove(id);
        }
    }
    let mut text = drafts.get(id).map_or_else(|| now.to_owned(), |d| d.text.clone());
    let edit = egui::TextEdit::singleline(&mut text)
        .id(eid)
        .hint_text(hint)
        .desired_width(width)
        .font(FontId::monospace(z(ui, 12.5)))
        .margin(egui::vec2(10.0, 6.0));
    let shown = edit.show(ui);
    let resp = &shown.response.response;
    take_keys(ui, resp);
    // Only a name `is_test_id` accepts: app ids may hold other characters, and those get no ID.
    let name = format!("settings.field.{id}");
    if ito_a11y::is_test_id(&name) {
        ito_a11y::test_id(resp, &name);
    }
    if resp.gained_focus() {
        let mut st = shown.state.clone();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::select_all(&shown.galley)));
        st.store(ui.ctx(), eid);
    }
    focus_ring(ui, resp);
    if resp.has_focus() || resp.changed() {
        drafts.insert(id.to_owned(), Draft { text: text.clone(), sent: None });
    }
    if resp.lost_focus() {
        // Enter is reported on two frames in a row; what was sent is not sent
        // again until the field has had the keys (which clears `sent`).
        if drafts.get(id).is_some_and(|d| d.sent.is_some()) {
            return None;
        }
        drafts.remove(id);
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || text == now {
            return None;
        }
        drafts.insert(id.to_owned(), Draft { text: text.clone(), sent: Some((now.to_owned(), std::time::Instant::now())) });
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
        return Some(text);
    }
    None
}

/// The egui id of the field `id`: `ctx.memory_mut(|m| m.request_focus(..))`
/// on it puts the caret back, as an app does when it refuses what Enter sent.
pub fn field_id(id: &str) -> egui::Id {
    egui::Id::new(("prefs-field", id))
}

/// A key's cap, as the design draws one.
pub fn keycap(ui: &mut egui::Ui, l: Look, words: &str, lit: bool) -> egui::Response {
    let c = l.c;
    let stroke = if lit { c.run } else { c.border_strong() };
    let b = egui::Button::new(RichText::new(words).font(FontId::monospace(z(ui, 12.0))).color(c.strong()))
        .fill(c.side)
        .stroke(egui::Stroke::new(1.0, stroke))
        .corner_radius(ito_theme::scale::radius::CONTROL);
    ui.add(b)
}

fn common(table: Option<&'static str>, key: &'static str, value: String) -> CommonChange {
    CommonChange { table, key, value }
}

/// The keys of the General page's shared rows ([`language_row`],
/// [`scale_row`], [`clock_card`]) in common.toml, for [`Nav::index`].
pub const GENERAL_KEYS: [&str; 7] = ["language", "scale", "clock.show", "clock.hour24", "clock.date", "clock.date_format", "clock.weekday"];

/// The language row, common.toml's `language` (`now`; `auto` when unset).
/// The note is the app's: what its own words do.
pub fn language_row(ui: &mut egui::Ui, l: Look, w: &Words, now: &str, note: &str) -> Option<CommonChange> {
    let options: Vec<(&str, &str)> =
        ito_i18n::LANGUAGES.iter().map(|(code, name)| (*code, if *code == "auto" { w.auto } else { *name })).collect();
    let code = keyed_row(ui, l, "language", w.language, note, |ui| select(ui, "language", now, &options))?;
    Some(common(None, "language", quote(code)))
}

/// The scales the scale row offers, in percent: 50% to 200% by tens, then
/// a few bigger ones.
const SCALES: [i32; 19] = [50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200, 250, 300, 400];

/// The scale row, common.toml's `scale` (`now`; 1.0 when unset), the size
/// the whole window is drawn at. The note is the app's: its keys.
pub fn scale_row(ui: &mut egui::Ui, l: Look, w: &Words, now: f32, note: &str) -> Option<CommonChange> {
    let now = (now * 100.0).round() as i32;
    let mut percents = SCALES.to_vec();
    if !percents.contains(&now) {
        percents.push(now);
        percents.sort_unstable();
    }
    let names: Vec<String> = percents.iter().map(|p| format!("{p}%")).collect();
    let options: Vec<(i32, &str)> = percents.iter().zip(&names).map(|(p, n)| (*p, n.as_str())).collect();
    let picked = keyed_row(ui, l, "scale", w.scale, note, |ui| select(ui, "scale", now, &options))?;
    Some(scale_change(picked as f32 / 100.0))
}

/// The Feedback card: one row with the bug-report button. True when it was
/// pressed; the app opens its report as F12 does (`ito_bugreport::url`).
pub fn bug_card(ui: &mut egui::Ui, l: Look, w: &Words) -> bool {
    let mut pressed = false;
    section(ui, l, w.bug_head, |ui| {
        pressed = row(ui, l, w.bug_label, w.bug_note, |ui| button(ui, l, w.bug_button));
    });
    pressed
}

/// The card's words, for the settings' search.
pub fn bug_index(w: &Words) -> Vec<&'static str> {
    vec![w.bug_head, w.bug_label, w.bug_note]
}

/// The CLOCK card: common.toml's `[clock]`.
pub fn clock_card(ui: &mut egui::Ui, l: Look, w: &Words, k: &Clock) -> Vec<CommonChange> {
    let mut out = Vec::new();
    section(ui, l, w.clock, |ui| {
        let set = |key: &'static str, v: bool| common(Some("clock"), key, v.to_string());
        if keyed_row(ui, l, "clock.show", w.show_time, w.show_time_note, |ui| switch(ui, l, k.show)) {
            out.push(set("show", !k.show));
        }
        sep(ui, l);
        let hours = [(true, w.hour24), (false, w.hour12)];
        if let Some(h) = keyed_row(ui, l, "clock.hour24", w.time_format, "", |ui| select(ui, "clock-hours", k.hour24, &hours)) {
            out.push(set("hour24", h));
        }
        sep(ui, l);
        if keyed_row(ui, l, "clock.date", w.show_date, w.show_date_note, |ui| switch(ui, l, k.date)) {
            out.push(set("date", !k.date));
        }
        sep(ui, l);
        let formats: Vec<(&str, &str)> = DATE_FORMATS.iter().map(|f| (*f, *f)).collect();
        if let Some(f) =
            keyed_row(ui, l, "clock.date_format", w.date_format, "", |ui| select(ui, "clock-date", k.date_format.as_str(), &formats))
        {
            out.push(common(Some("clock"), "date_format", quote(f)));
        }
        sep(ui, l);
        if keyed_row(ui, l, "clock.weekday", w.weekday, "", |ui| switch(ui, l, k.weekday)) {
            out.push(set("weekday", !k.weekday));
        }
    });
    out
}

/// The theme the choice in force names, given a theme's name and whether it
/// is light: `system` keeps one for each kind, `dark` and `light` are the
/// two built in.
fn picked(choice: (&str, &str, &str), name: &str, light: bool) -> bool {
    let (theme, dark, bright) = choice;
    match theme {
        "system" => name == if light { bright } else { dark },
        "dark" => name == "tsumugi Dark",
        "light" => name == "tsumugi Light",
        other => name.eq_ignore_ascii_case(other),
    }
}

/// The name of the theme in force: in Follow OS, the one picked for the kind
/// shown (`light`). As the theme list spells it, when it has the theme.
fn in_force<'a>(themes: &'a [Theme], choice: (&'a str, &'a str, &'a str), light: bool) -> &'a str {
    let (theme, dark, bright) = choice;
    let name = match theme {
        "system" if light => bright,
        "system" => dark,
        "dark" => "tsumugi Dark",
        "light" => "tsumugi Light",
        other => other,
    };
    themes.iter().find(|t| t.name.eq_ignore_ascii_case(name)).map_or(name, |t| t.name.as_str())
}

/// Flip the mode shown now, for the title bar's tag and the palette's
/// "Switch the theme": a light screen goes to the dark theme picked for dark
/// (`choice`'s second, as on the theme page), a dark one to the light theme.
/// Follow OS becomes an explicit mode, since what the OS shows is what the
/// tag read. Write the result like the theme page's.
/// `choice` is common.toml's `theme`, `dark_theme` and `light_theme`.
pub fn flip_theme(choice: (&str, &str, &str), light_now: bool) -> Vec<CommonChange> {
    let (_, dark, light) = choice;
    vec![common(None, "theme", quote(if light_now { dark } else { light }))]
}

/// The theme page: the mode (Follow OS, Light, Dark) under the heading, the
/// themes of that mode on the left, and on the right the preview the app
/// draws in the theme in force (`l.c`). `choice` is common.toml's `theme`,
/// `dark_theme` and `light_theme`; `note` goes under the list (where the
/// app's own themes are read from).
pub fn theme_page(
    ui: &mut egui::Ui,
    l: Look,
    w: &Words,
    themes: &[Theme],
    choice: (&str, &str, &str),
    note: &str,
    preview: impl FnOnce(&mut egui::Ui),
) -> Vec<CommonChange> {
    let c = &l.c;
    let mut out = Vec::new();
    let (theme, dark, light) = choice;
    let mode = match theme {
        "system" => "system",
        "light" => "light",
        "dark" => "dark",
        name => {
            if themes.iter().any(|t| t.name.eq_ignore_ascii_case(name) && t.colors.light) {
                "light"
            } else {
                "dark"
            }
        }
    };
    // The mode as one segmented control at the left, under the heading, in
    // a row of its own height.
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 36.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.label(RichText::new(w.mode).size(z(ui, 12.0)).color(c.dim));
        ui.add_space(ito_theme::scale::space::SM);
        egui::Frame::NONE
            .stroke(egui::Stroke::new(1.0, c.border_strong()))
            .corner_radius(ito_theme::scale::radius::BOX)
            .inner_margin(egui::Margin::same(3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (k, label) in [("system", w.follow_os), ("light", w.light), ("dark", w.dark)] {
                        let on = mode == k;
                        let b = egui::Button::new(RichText::new(label).size(z(ui, 12.5)).color(if on { c.strong() } else { c.dim }))
                            .fill(if on { c.chosen() } else { Color32::TRANSPARENT })
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(ito_theme::scale::radius::CONTROL)
                            .min_size(egui::vec2(z(ui, 84.0), z(ui, 26.0)));
                        let r = ui.add(b);
                        focus_ring(ui, &r);
                        if r.clicked() && !on {
                            // A mode keeps the theme picked for it.
                            let value = match k {
                                "system" => "system",
                                "light" => light,
                                _ => dark,
                            };
                            out.push(common(None, "theme", quote(value)));
                        }
                    }
                });
            });
    });
    ui.add_space(ito_theme::scale::space::MD);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            // Light and Dark list their own kind; Follow OS lists the kind the
            // OS is showing now (the other kind's theme is set when the OS
            // turns to it, and could not be seen here).
            let list: Vec<&Theme> =
                themes.iter().filter(|t| t.colors.light == if mode == "system" { l.c.light } else { mode == "light" }).collect();
            ui.label(RichText::new(format!("{} · {}", w.themes, list.len())).size(z(ui, 11.0)).strong().color(c.dim));
            egui::Frame::NONE
                .fill(c.panel)
                .stroke(egui::Stroke::new(1.0, c.border))
                .corner_radius(ito_theme::scale::radius::BOX)
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    for t in list {
                        let on = picked(choice, &t.name, t.colors.light);
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), z(ui, 34.0)), egui::Sense::click());
                        let p = ui.painter();
                        if on {
                            p.rect_filled(r, 7.0, c.chosen());
                            p.rect_stroke(r, 7.0, egui::Stroke::new(1.0, c.run), egui::StrokeKind::Inside);
                        } else if resp.hovered() {
                            p.rect_filled(r, 7.0, c.hover());
                        }
                        // A small chip of four: its ground and three of its states.
                        let chip = egui::Rect::from_min_size(
                            egui::pos2(r.left() + 10.0, r.center().y - z(ui, 9.0)),
                            egui::vec2(z(ui, 40.0), z(ui, 18.0)),
                        );
                        for (k, sw) in [t.colors.bg, t.colors.wait, t.colors.run, t.colors.err].iter().enumerate() {
                            let x = chip.left() + k as f32 * 10.0;
                            let corner = match k {
                                0 => egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 },
                                3 => egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 },
                                _ => egui::CornerRadius::ZERO,
                            };
                            p.rect_filled(egui::Rect::from_min_size(egui::pos2(x, chip.top()), egui::vec2(10.0, 18.0)), corner, *sw);
                        }
                        p.rect_stroke(chip, 4.0, egui::Stroke::new(1.0, c.border_strong()), egui::StrokeKind::Outside);
                        p.text(
                            egui::pos2(r.left() + 62.0, r.center().y),
                            egui::Align2::LEFT_CENTER,
                            &t.name,
                            FontId::proportional(z(ui, 13.0)),
                            if on { c.strong() } else { c.fg },
                        );
                        let tagged = on && mode == "system";
                        let kind = match (tagged, t.colors.light) {
                            (true, true) => w.when_light,
                            (true, false) => w.when_dark,
                            (false, true) => w.light,
                            (false, false) => w.dark,
                        };
                        p.text(
                            egui::pos2(r.right() - 10.0, r.center().y),
                            egui::Align2::RIGHT_CENTER,
                            kind.to_lowercase(),
                            FontId::proportional(z(ui, 11.0)),
                            if tagged { c.run } else { c.faint() },
                        );
                        if resp.clicked() {
                            // In Follow OS, a theme becomes the one for its kind.
                            let key = match mode {
                                "system" if t.colors.light => "light_theme",
                                "system" => "dark_theme",
                                _ => "theme",
                            };
                            out.push(common(None, key, quote(&t.name)));
                        }
                    }
                });
            ui.label(RichText::new(note).size(z(ui, 12.0)).color(c.dim));
        });
        ui.add_space(ito_theme::scale::space::XL);
        ui.vertical(|ui| {
            // By the name picked, not the colours: a theme.toml that repaints
            // the window leaves no theme with these colours (kura #321).
            let name = in_force(themes, choice, l.c.light);
            ui.label(RichText::new(format!("{} · {name}", w.preview)).size(z(ui, 11.0)).strong().color(c.fg));
            preview(ui);
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tag's flip writes the other mode's remembered theme.
    #[test]
    fn flip_goes_to_the_other_modes_theme() {
        let to_dark = flip_theme(("system", "nord", "paper"), true);
        let to_light = flip_theme(("system", "nord", "paper"), false);
        assert_eq!((to_dark.len(), to_dark[0].key, to_dark[0].value.as_str()), (1, "theme", "\"nord\""));
        assert_eq!(to_light[0].value, "\"paper\"");
    }

    #[test]
    fn the_ring_reaches_focus_room_out_and_moves_in_only_where_the_clip_cuts_it() {
        let r = |x0, y0, x1, y1| egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1));
        let w = r(50.0, 50.0, 150.0, 76.0);
        // Free: the gap all round, so the outside of the stroke is FOCUS_ROOM out.
        assert_eq!(ring_rect(w, r(0.0, 0.0, 400.0, 300.0), 1.0).expand(RING_WIDTH), w.expand(FOCUS_ROOM));
        // Flush with every side of the clip: each side moves in, the stroke and half a pixel inside the clip.
        assert_eq!(ring_rect(w, w, 1.0), w.shrink(RING_WIDTH + 0.5));
        assert_eq!(ring_rect(w, w, 1.25), w.shrink(RING_WIDTH + 0.4), "half a pixel at 125 %");
        // Scrolled partly out at the top: that side stays where it was (no line across the widget).
        let got = ring_rect(w, r(0.0, 51.0, 400.0, 300.0), 1.0);
        assert_eq!(got, w.expand(RING_GAP));
        // Out at the left, flush at the right: only the right moves in.
        let got = ring_rect(w, r(60.0, 0.0, 150.0, 300.0), 1.0);
        assert_eq!((got.min.x, got.max.x), (w.min.x - RING_GAP, w.max.x - RING_WIDTH - 0.5));
        // A clip inside the widget on every side: nothing moves.
        assert_eq!(ring_rect(w, w.shrink(5.0), 1.0), w.expand(RING_GAP));
        // Wholly above or below the clip: nothing moves (and nothing of it shows).
        assert_eq!(ring_rect(w, r(0.0, 100.0, 400.0, 300.0), 1.0), w.expand(RING_GAP));
        assert_eq!(ring_rect(w, r(0.0, 0.0, 400.0, 40.0), 1.0), w.expand(RING_GAP));
        // A widget too thin for a ring inside a clip that fits it: the ring outside, as the clip cuts it.
        let thin = r(0.0, 0.0, 40.0, 2.0);
        assert_eq!(ring_rect(thin, thin, 1.0), thin.expand(RING_GAP));
    }

    /// A button flush with the top of a scroll area, with the keys, and a label right under it (egui's spacing):
    /// the ring is whole inside the scroll area's clip and stays off the label.
    #[test]
    fn a_ring_at_the_top_of_a_scroll_area_is_whole_and_keeps_off_the_text_below() {
        let ctx = egui::Context::default();
        let (mut button, mut label) = (egui::Rect::NOTHING, egui::Rect::NOTHING);
        let mut ring = None;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let b = ui.button("Day");
                        b.request_focus();
                        focus_ring(ui, &b);
                        button = b.rect;
                        label = ui.label("A note right under it").rect;
                    });
                });
            });
            ring = out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Rect(s) if s.stroke_kind == egui::StrokeKind::Outside && s.stroke.width == RING_WIDTH => {
                    Some((s.rect, c.clip_rect))
                }
                _ => None,
            });
            out.textures_delta.clear();
        }
        let (ring, clip) = ring.expect("a focus ring was painted");
        assert!(button.top() - FOCUS_ROOM < clip.top(), "the button is flush with the clip: {button:?} in {clip:?}");
        assert!(ring.top() - RING_WIDTH >= clip.top(), "the top of the ring is cut: {ring:?} in {clip:?}");
        assert!(ring.top() <= button.top() + RING_WIDTH + 0.5, "the ring went too far into the button");
        assert!(ring.bottom() + RING_WIDTH <= label.top(), "the ring crosses the label: {ring:?} over {label:?}");
    }

    /// One frame with AccessKit on: a `select` with three options; the test IDs it registered, in order.
    fn select_frame(ctx: &egui::Context, id: &str, events: Vec<egui::Event>) -> Vec<ito_a11y::TestIdEntry> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                select(ui, id, 1u8, &[(0, "zero"), (1, "one"), (2, "two")]);
            });
        });
        out.textures_delta.clear();
        ito_a11y::take_test_ids(ctx, egui::ViewportId::ROOT)
    }

    fn select_names(found: &[ito_a11y::TestIdEntry]) -> Vec<&str> {
        found.iter().map(|e| e.name.as_str()).collect()
    }

    /// Opens a `select_frame` list for real: a click on the box, over three frames (move, press, release).
    fn open_select(ctx: &egui::Context, id: &str, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        select_frame(ctx, id, vec![egui::Event::PointerMoved(at)]);
        select_frame(ctx, id, vec![button(true)]);
        select_frame(ctx, id, vec![button(false)]);
    }

    /// One frame with AccessKit on: the AccessKit nodes of whatever `show` draws.
    fn field_nodes(ctx: &egui::Context, show: impl Fn(&mut egui::Ui)) -> Vec<(egui::accesskit::NodeId, egui::accesskit::Node)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| show(ui));
        });
        out.textures_delta.clear();
        out.platform_output.accesskit_update.expect("an AccessKit update").nodes
    }

    fn count_invalid(nodes: &[(egui::accesskit::NodeId, egui::accesskit::Node)]) -> usize {
        nodes.iter().filter(|(_, n)| n.invalid() == Some(egui::accesskit::Invalid::True)).count()
    }

    #[test]
    fn a_select_in_error_is_invalid_to_a_screen_reader_and_one_that_is_not_is_not() {
        let options = [(0u8, "zero"), (1, "one")];
        for (error, want) in [(true, 1), (false, 0)] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let nodes = field_nodes(&ctx, |ui| {
                SelectField::new("mode").error(error).show(ui, 1u8, &options);
            });
            assert_eq!(count_invalid(&nodes), want, "error {error}");
        }
    }

    #[test]
    fn a_text_field_in_error_is_invalid_to_a_screen_reader() {
        for (error, want) in [(true, 1), (false, 0)] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let nodes = field_nodes(&ctx, |ui| {
                InputField::new("a").error(error).show(ui, &mut String::new());
            });
            assert_eq!(count_invalid(&nodes), want, "InputField error {error}");
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let nodes = field_nodes(&ctx, |ui| {
                CodeBox::new("b").error(error).show(ui, &mut String::new());
            });
            assert_eq!(count_invalid(&nodes), want, "CodeBox error {error}");
        }
    }

    #[test]
    fn the_closed_select_button_is_described_by_the_note_of_the_option_it_shows() {
        let options = [(0u8, "zero"), (1, "one")];
        let notes = ["", "from PATH"];
        let described = |now: u8| {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let nodes = field_nodes(&ctx, |ui| {
                SelectField::new("mode").notes(&notes).show(ui, now, &options);
            });
            nodes.iter().filter(|(_, n)| n.description() == Some("from PATH")).count()
        };
        assert_eq!(described(1), 1);
        assert_eq!(described(0), 0);
    }

    #[test]
    fn a_codebox_paints_its_hint_while_empty() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                CodeBox::new("b").hint("echo hi").show(ui, &mut String::new());
            });
        });
        out.textures_delta.clear();
        let painted = |s: &egui::Shape| matches!(s, egui::Shape::Text(t) if t.galley.text() == "echo hi");
        assert!(out.shapes.iter().any(|c| painted(&c.shape)), "the hint was painted");
    }

    #[test]
    fn a_closed_select_has_one_test_id_and_an_open_one_names_its_options() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let found = select_frame(&ctx, "mode", vec![]);
        assert_eq!(select_names(&found), ["settings.select.mode"]);
        open_select(&ctx, "mode", found[0].rect.center());
        let found = select_frame(&ctx, "mode", vec![]);
        let names = select_names(&found);
        assert!(names.contains(&"settings.select.mode"), "{names:?}");
        let options: Vec<&str> = names.iter().copied().filter(|n| n.starts_with("settings.select.mode.")).collect();
        assert_eq!(options, ["settings.select.mode.0", "settings.select.mode.1", "settings.select.mode.2"]);
        // The options are in the order of the list, top to bottom.
        let ys: Vec<f32> = found.iter().filter(|e| e.name.starts_with("settings.select.mode.")).map(|e| e.rect.top()).collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
    }

    #[test]
    fn a_select_id_that_is_not_a_test_id_gets_no_name() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        assert!(!ito_a11y::is_test_id("settings.select.Two Words"));
        assert!(select_frame(&ctx, "Two Words", vec![]).is_empty());
        // Open as well, since the options are named in their own loop. This box has no rect to read, so a good id's box gives the place.
        let good = egui::Context::default();
        good.enable_accesskit();
        let at = select_frame(&good, "mode", vec![])[0].rect.center();
        open_select(&ctx, "Two Words", at);
        assert!(select_frame(&ctx, "Two Words", vec![]).is_empty());
        assert!(egui::Popup::is_any_open(&ctx), "the list was open while no name was registered");
    }

    /// Esc closes the screen, but not while a popup is open: that Esc is the popup's.
    #[test]
    fn esc_is_the_popups_while_one_is_open() {
        let ctx = egui::Context::default();
        let esc = || egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let frame = |events: Vec<egui::Event>| {
            let mut close = false;
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| close = Keys::standard(ui, false).close);
            out.textures_delta.clear();
            close
        };
        assert!(frame(vec![esc()]), "no popup: Esc closes the screen");
        egui::Popup::open_id(&ctx, egui::Id::new("dropdown"));
        assert!(!frame(vec![esc()]), "a popup is open: Esc is left to it");
    }

    #[test]
    fn filled_buttons_take_the_themes_accent_and_error_colours() {
        let c = ito_theme::colors();
        assert_eq!(tone_colors(&c, Tone::Primary).0, c.run);
        assert_eq!(tone_colors(&c, Tone::Danger).0, c.err);
        assert_eq!(tone_colors(&c, Tone::Danger).2, c.on_accent());
        assert_eq!(tone_colors(&c, Tone::Warn).0, c.wait);
        assert_eq!(tone_colors(&c, Tone::Plain).0, Color32::TRANSPARENT);
    }

    #[test]
    fn seg_keys_move_the_pick_and_stop_at_the_ends() {
        assert_eq!(seg_step(Some(1), 4, true, false, false, false), Some(0));
        assert_eq!(seg_step(Some(0), 4, true, false, false, false), Some(0));
        assert_eq!(seg_step(Some(3), 4, false, true, false, false), Some(3));
        assert_eq!(seg_step(Some(1), 4, false, false, false, true), Some(3));
        assert_eq!(seg_step(Some(2), 4, false, false, false, false), Some(2));
        assert_eq!(seg_step(None, 0, true, false, false, false), None);
    }

    /// Through a frame: a click on the second part picks it.
    #[test]
    fn a_click_on_a_part_of_seg_picks_it() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0));
        let look = Look { c: ito_theme::colors(), q: "" };
        let options = [(0u8, "A"), (1, "B"), (2, "C")];
        let mut rect = egui::Rect::NOTHING;
        let mut got = None;
        for frame in 0..4 {
            let mut events = Vec::new();
            if frame >= 2 {
                let pos = egui::pos2(rect.right() - 20.0, rect.center().y);
                events.push(egui::Event::PointerMoved(pos));
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: frame == 2,
                    modifiers: Default::default(),
                });
            }
            let input = egui::RawInput { screen_rect: Some(screen), time: Some(f64::from(frame) * 0.1), events, ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let before = ui.cursor().min;
                let r = seg(ui, look, "x", 0u8, &options);
                rect = egui::Rect::from_min_size(before, egui::vec2(0.0, 26.0)).union(ui.min_rect());
                got = got.or(r);
            });
            out.textures_delta.clear();
        }
        assert_eq!(got, Some(2));
    }

    #[test]
    fn the_theme_named_is_the_one_picked() {
        let choice = ("system", "Nord", "tsumugi Light");
        assert!(picked(choice, "Nord", false));
        assert!(picked(choice, "tsumugi Light", true));
        assert!(!picked(choice, "Nord", true), "Nord is the dark one");
        assert!(picked(("dark", "Nord", "x"), "tsumugi Dark", false), "dark is the built-in one");
        assert!(picked(("nord", "", ""), "Nord", false), "a name, whatever its case");
    }

    /// The first hit of a search, low on a long page, is scrolled into view
    /// once per query (kura #443).
    #[test]
    fn a_search_hit_low_on_the_page_is_scrolled_into_view() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
        let mut seen = egui::Rect::NOTHING;
        for frame in 0..40 {
            // The scroll glides, so time has to move.
            let input = egui::RawInput { screen_rect: Some(screen), time: Some(f64::from(frame) * 0.1), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let l = Look { c: ito_theme::colors(), q: "needle" };
                        for i in 0..40 {
                            row(ui, l, &format!("filler {i}"), "", |ui| ui.label("x"));
                        }
                        seen = row(ui, l, "needle", "", |ui| ui.label("x")).rect;
                    });
                });
            });
            out.textures_delta.clear();
        }
        assert!(screen.contains_rect(seen), "the hit at {seen:?} is not in {screen:?}");
    }

    #[test]
    fn every_language_has_its_words() {
        assert_eq!(Words::of("ja").language, "言語");
        assert_eq!(Words::of("en").language, "Language");
        assert_eq!(Words::of("fr").language, "Language", "English for the rest");
        assert_eq!(Words::of("ja").scale, "倍率");
    }

    type Texts = Vec<(String, egui::Rect)>;

    /// One frame of the frame and the shared pages, drawn without a window.
    fn frame(ctx: &egui::Context, state: &mut State, events: Vec<egui::Event>, keys: Keys) -> (Vec<CommonChange>, Texts) {
        let themes = ito_theme::builtin();
        let nav =
            Nav { pages: &[("General", "One"), ("Theme", "Two")], index: &[(0, "Language"), (1, "Mode")], words: &EN, file: "config.toml" };
        let mut changes = Vec::new();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 1600.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(ui, ito_theme::colors(), state, &nav, keys, |ui, page, l| {
                    if page == 1 {
                        changes = theme_page(ui, l, &EN, &themes, ("dark", "tsumugi Dark", "tsumugi Light"), "", |_| {});
                    } else if let Some(c) = language_row(ui, l, &EN, "auto", "") {
                        changes.push(c);
                    }
                });
            });
        });
        out.textures_delta.clear();
        fn walk(s: &egui::Shape, v: &mut Texts) {
            match s {
                egui::Shape::Text(t) => v.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, v)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        (changes, texts)
    }

    /// One frame of two pages of cards, with the extras, and the text drawn.
    fn cards(ctx: &egui::Context, state: &mut State, keys: Keys) -> (Out, Vec<String>) {
        cards_with(ctx, state, keys, Vec::new())
    }

    fn cards_with(ctx: &egui::Context, state: &mut State, keys: Keys, events: Vec<egui::Event>) -> (Out, Vec<String>) {
        let nav = Nav {
            pages: &[("General", "One"), ("Theme", "Two"), ("Keys", "Three")],
            index: &[(0, "Language"), (0, "Show clock"), (1, "Dark")],
            words: &EN,
            file: "config.toml",
        };
        let extras = Extras {
            groups: &[("Notebook", 0), ("Common", 2)],
            shared: &[2],
            shared_tag: "Common page",
            back: "Back",
            results: Some(Results { head: "Settings with \"{q}\"", lead: "All pages.", none: "None." }),
            files: &[],
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 1600.0))),
            events,
            ..Default::default()
        };
        let mut out = Out::default();
        let mut run = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                out = show_with(ui, ito_theme::colors(), state, &nav, keys, extras, |ui, page, l| match page {
                    0 => {
                        section(ui, l, "Words", |ui| {
                            row(ui, l, "Language", "", |_| {});
                        });
                        section(ui, l, "Clock", |ui| {
                            row(ui, l, "Show clock", "", |_| {});
                        });
                    }
                    1 => section(ui, l, "Mode", |ui| {
                        row(ui, l, "Dark", "", |_| {});
                    }),
                    _ => {}
                });
            });
        });
        run.textures_delta.clear();
        fn walk(s: &egui::Shape, v: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => v.push(t.galley.text().to_owned()),
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, v)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for c in &run.shapes {
            walk(&c.shape, &mut texts);
        }
        (out, texts)
    }

    /// The app's own files stack above the common one, and the click says which (common 0, the app's 1..).
    #[test]
    fn the_files_stack_above_the_common_one_and_a_click_names_it() {
        let nav = Nav { pages: &[("General", "One")], index: &[(0, "Language")], words: &EN, file: "common.toml" };
        let files = ["app.toml", "other.toml"];
        let extras = Extras { groups: &[], shared: &[], shared_tag: "", back: "", results: None, files: &files };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let ctx = egui::Context::default();
        let mut state = State::default();
        let mut frame = |events: Vec<egui::Event>| {
            let mut out = Out::default();
            let mut run = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                out = show_with(ui, ito_theme::colors(), &mut state, &nav, Keys::default(), extras, |_, _, _| {});
            });
            run.textures_delta.clear();
            out
        };
        assert_eq!(frame(vec![]).open_file, None);
        // The buttons sit 40 points apart, the common one lowest (48..16 above the bottom of the nav).
        let click = |y: f32| {
            let pos = egui::pos2(100.0, y);
            let b =
                |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            [egui::Event::PointerMoved(pos), b(true), b(false)].to_vec()
        };
        let bottom = screen.bottom();
        let mut got = Vec::new();
        for y in [bottom - 32.0 - 80.0, bottom - 32.0 - 40.0, bottom - 32.0] {
            let seen = (0..3).map(|_| frame(click(y)).open_file).find(Option::is_some).flatten();
            got.push(seen);
        }
        assert_eq!(got, vec![Some(1), Some(2), Some(0)], "buttons by height");
    }

    /// On the root ui under a top panel (a title bar), the screen leaves the bar alone (uchmk/ito#63).
    #[test]
    fn the_screen_stays_below_a_top_panel() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        let nav = Nav { pages: &[("General", "One")], index: &[(0, "Language")], words: &EN, file: "config.toml" };
        let extras = Extras { groups: &[], shared: &[], shared_tag: "", back: "Back", results: None, files: &[] };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            ..Default::default()
        };
        let mut run = ctx.run_ui(input, |ui| {
            egui::Panel::top("bar").exact_size(40.0).show(ui, |_| {});
            show_with(ui, ito_theme::colors(), &mut state, &nav, Keys::default(), extras, |_, _, _| {});
        });
        run.textures_delta.clear();
        let bg = ito_theme::colors().bg;
        let top = run
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.fill == bg => Some(r.rect.top()),
                _ => None,
            })
            .fold(f32::MAX, f32::min);
        assert!(top >= 40.0, "the screen painted from y = {top}, over the bar");
    }

    #[test]
    fn a_search_reads_every_page_and_esc_clears_it_first() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        let (_, plain) = cards(&ctx, &mut state, Keys::default());
        assert!(plain.iter().any(|t| t == "Notebook") && plain.iter().any(|t| t == "Common"), "the nav headings: {plain:?}");
        assert!(plain.iter().any(|t| t == "Common page") || state.page != 2, "the tag is only on the shared page");
        assert!(plain.iter().any(|t| t.ends_with("Back")), "the back button: {plain:?}");

        state.query = "dark".into();
        let _ = cards(&ctx, &mut state, Keys::default());
        let (_, found) = cards(&ctx, &mut state, Keys::default());
        assert!(found.iter().any(|t| t == "Settings with \"dark\""), "{found:?}");
        assert!(found.iter().any(|t| t == "Theme \u{203a} Mode"), "{found:?}");
        assert!(!found.iter().any(|t| t.contains("General \u{203a}")), "a page with no hit leaves no card: {found:?}");
        assert!(found.iter().any(|t| t == "Keys"), "the pages without a hit stay in the nav");

        state.query = "clock".into();
        let (_, found) = cards(&ctx, &mut state, Keys::default());
        assert!(found.iter().any(|t| t == "General \u{203a} Clock"), "{found:?}");
        assert!(!found.iter().any(|t| t == "General \u{203a} Words"), "a card with no hit is left out: {found:?}");

        let (out, _) = cards(&ctx, &mut state, Keys { close: true, ..Keys::default() });
        assert!((out.close, state.query.as_str()) == (false, ""), "the first Esc clears the search");
        let (out, _) = cards(&ctx, &mut state, Keys { close: true, ..Keys::default() });
        assert!(out.close, "the next one closes the screen");
    }

    /// A real Esc in the focused search: egui drops the focus before the
    /// frame, and the search still keeps its keys and loses its words (fude #58).
    #[test]
    fn a_real_esc_in_the_search_clears_it_and_keeps_the_focus() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        let _ = cards(&ctx, &mut state, Keys::default());
        let search = egui::Id::new("prefs-search");
        ctx.memory_mut(|m| m.request_focus(search));
        state.query = "clock".into();
        let _ = cards(&ctx, &mut state, Keys::default());
        assert!(state.search_held, "the search has the keys");
        // The app's keys: `held` makes Esc not close the screen.
        let (out, _) = cards_with(&ctx, &mut state, Keys::default(), tap(egui::Key::Escape));
        assert!((out.close, state.query.as_str()) == (false, ""), "the words are gone");
        assert!(ctx.memory(|m| m.has_focus(search)), "the keys stay in the search");
        let _ = cards(&ctx, &mut state, Keys::default());
        assert!(ctx.memory(|m| m.has_focus(search)));
    }

    #[test]
    fn the_preview_names_the_theme_picked() {
        let themes = ito_theme::builtin();
        let choice = ("system", "Nord", "tsumugi Light");
        assert_eq!(in_force(&themes, choice, false), "Nord");
        assert_eq!(in_force(&themes, choice, true), "tsumugi Light");
        assert_eq!(in_force(&themes, ("light", "", ""), false), "tsumugi Light");
        assert_eq!(in_force(&themes, ("nord", "", ""), false), "Nord", "as the list spells it");
        assert_eq!(in_force(&themes, ("Mine", "", ""), false), "Mine", "an app's own, not built in");
    }

    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    fn tap(k: egui::Key) -> Vec<egui::Event> {
        vec![key(k, true), key(k, false)]
    }

    /// One frame of a page of a field and a switch: what the field wrote.
    fn form(ctx: &egui::Context, state: &mut State, drafts: &mut Drafts, events: Vec<egui::Event>) -> Option<String> {
        let nav = Nav { pages: &[("Form", "")], index: &[(0, "Width"), (0, "list.width"), (0, "Wrap")], words: &EN, file: "config.toml" };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut wrote = None;
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(ui, ito_theme::colors(), state, &nav, Keys::default(), |ui, _, l| {
                    section(ui, l, "LIST", |ui| {
                        wrote = keyed_row(ui, l, "list.width", "Width", "", |ui| field(ui, drafts, "width", "14", "", 80.0));
                        sep(ui, l);
                        row(ui, l, "Wrap", "", |ui| switch(ui, l, false));
                    });
                });
            });
        });
        out.textures_delta.clear();
        wrote
    }

    /// One frame of a page of two lists and a switch: what the first picked.
    fn lists(ctx: &egui::Context, state: &mut State, events: Vec<egui::Event>, now: &mut u8) {
        let nav = Nav { pages: &[("Form", "")], index: &[(0, "A"), (0, "B")], words: &EN, file: "config.toml" };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(ui, ito_theme::colors(), state, &nav, Keys::default(), |ui, _, l| {
                    section(ui, l, "LIST", |ui| {
                        if let Some(v) = row(ui, l, "A", "", |ui| select(ui, "a", *now, &[(1u8, "one"), (2, "two"), (3, "three")])) {
                            *now = v;
                        }
                        sep(ui, l);
                        row(ui, l, "B", "", |ui| select(ui, "b", 1u8, &[(1u8, "x"), (2, "y")]));
                        sep(ui, l);
                        row(ui, l, "C", "", |ui| switch(ui, l, false));
                    });
                });
            });
        });
        out.textures_delta.clear();
    }

    /// A list is used from the keyboard alone: the keys stay on its button,
    /// Enter picks and shuts, Esc shuts, Tab shuts and goes on.
    #[test]
    fn a_list_is_used_from_the_keyboard_alone() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        let mut now = 1u8;
        let mut key = |ctx: &egui::Context, now: &mut u8, k: egui::Key| {
            lists(ctx, &mut state, tap(k), now);
            lists(ctx, &mut state, Vec::new(), now);
        };
        let on = |ctx: &egui::Context, which: &str| {
            let at = focused(ctx).expect("something has the keys");
            // The button's id is its salt below the page's ids.
            format!("{at:?}").ends_with(&format!("{:?})", egui::IdSalt::new(("prefs", which))))
        };
        key(&ctx, &mut now, egui::Key::Tab);
        key(&ctx, &mut now, egui::Key::Tab);
        assert!(on(&ctx, "a"), "Tab reaches the list");
        key(&ctx, &mut now, egui::Key::Enter);
        assert!(egui::Popup::is_any_open(&ctx), "Enter opens it");
        assert!(on(&ctx, "a"));
        key(&ctx, &mut now, egui::Key::ArrowDown);
        key(&ctx, &mut now, egui::Key::ArrowDown);
        assert!(on(&ctx, "a"), "the arrows move the light, not the keys");
        assert_eq!(now, 1, "and pick nothing");
        key(&ctx, &mut now, egui::Key::Enter);
        assert_eq!(now, 3, "Enter picks what is lit");
        assert!(!egui::Popup::is_any_open(&ctx), "and shuts it");
        assert!(on(&ctx, "a"));
        key(&ctx, &mut now, egui::Key::Space);
        key(&ctx, &mut now, egui::Key::ArrowUp);
        key(&ctx, &mut now, egui::Key::Escape);
        assert!(!egui::Popup::is_any_open(&ctx), "Esc shuts it");
        assert_eq!(now, 3, "as it was");
        assert!(on(&ctx, "a"), "with the keys kept");
        key(&ctx, &mut now, egui::Key::Enter);
        key(&ctx, &mut now, egui::Key::Tab);
        assert!(!egui::Popup::is_any_open(&ctx), "Tab shuts it");
        assert!(on(&ctx, "b"), "and goes on to the next");
    }

    fn focused(ctx: &egui::Context) -> Option<egui::Id> {
        ctx.memory(|m| m.focused())
    }

    /// Tab comes to the field with its words picked, so what is typed
    /// replaces them (kura #324, #340).
    #[test]
    fn a_field_tabbed_to_is_typed_over() {
        let ctx = egui::Context::default();
        let (mut state, mut drafts) = (State::default(), Drafts::new());
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Tab));
        assert_eq!(focused(&ctx), Some(egui::Id::new("prefs-search")), "the search first");
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Tab));
        assert_eq!(focused(&ctx), Some(egui::Id::new(("prefs-field", "width"))));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, vec![egui::Event::Text("abc".into())]);
        assert_eq!(form(&ctx, &mut state, &mut drafts, tap(egui::Key::Enter)), Some("abc".into()));
    }

    /// Tab walks the ring with something always holding the keys: off the
    /// file, the last stop, it comes straight round to the search, and
    /// Shift+Tab from there goes back to the file (tsumugi #61, #106).
    #[test]
    fn tab_off_the_file_comes_straight_to_the_search() {
        let ctx = egui::Context::default();
        let (mut state, mut drafts) = (State::default(), Drafts::new());
        form(&ctx, &mut state, &mut drafts, Vec::new());
        let search = egui::Id::new("prefs-search");
        let mut walk = Vec::new();
        for _ in 0..5 {
            form(&ctx, &mut state, &mut drafts, tap(egui::Key::Tab));
            walk.push(focused(&ctx));
        }
        assert!(walk.iter().all(Option::is_some), "no frame with nothing: {walk:?}");
        assert_eq!(walk[0], Some(search));
        assert_eq!(walk[4], Some(search), "round again after the field, the switch and the file: {walk:?}");
        let file = walk[3];
        let back = vec![egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        }];
        form(&ctx, &mut state, &mut drafts, back);
        form(&ctx, &mut state, &mut drafts, Vec::new());
        assert_eq!(focused(&ctx), file, "Shift+Tab from the search to the file");
    }

    /// Enter in the search hands the keys to the first row it lights.
    #[test]
    fn enter_in_the_search_goes_to_the_row_found() {
        let ctx = egui::Context::default();
        let (mut state, mut drafts) = (State::default(), Drafts::new());
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Tab));
        form(&ctx, &mut state, &mut drafts, vec![egui::Event::Text("wrap".into())]);
        assert_eq!(state.query, "wrap");
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Enter));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        let at = focused(&ctx).expect("a control has the keys");
        assert_ne!(at, egui::Id::new("prefs-search"));
        assert_ne!(at, egui::Id::new(("prefs-field", "width")), "the row the search lit, not the first");
        // The width row next, as the search lights it.
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Escape));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("prefs-search")));
        state.query = "wid".into();
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Enter));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        assert_eq!(focused(&ctx), Some(egui::Id::new(("prefs-field", "width"))));
        // And by its key in the file, which is not on the screen.
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Escape));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("prefs-search")));
        state.query = "LIST.W".into();
        form(&ctx, &mut state, &mut drafts, Vec::new());
        form(&ctx, &mut state, &mut drafts, tap(egui::Key::Enter));
        form(&ctx, &mut state, &mut drafts, Vec::new());
        assert_eq!(focused(&ctx), Some(egui::Id::new(("prefs-field", "width"))), "found by its key");
    }

    /// A click on a theme writes it into common.toml; the keys walk the
    /// pages round; the file's button is at the foot.
    #[test]
    fn a_theme_clicked_goes_to_common_toml_and_the_keys_walk_the_pages() {
        let ctx = egui::Context::default();
        let mut state = State { page: 1, ..State::default() };
        let (_, texts) = frame(&ctx, &mut state, Vec::new(), Keys::default());
        let themes = ito_theme::builtin();
        let second = themes.iter().find(|t| !t.colors.light && t.name != "tsumugi Dark").expect("another dark theme");
        let at = texts.iter().find(|(t, _)| *t == second.name).map(|(_, r)| r.center()).expect("the theme is listed");
        let click = |down| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, &mut state, vec![egui::Event::PointerMoved(at)], Keys::default());
        let mut changes = frame(&ctx, &mut state, vec![click(true)], Keys::default()).0;
        changes.extend(frame(&ctx, &mut state, vec![click(false)], Keys::default()).0);
        assert_eq!(changes, vec![CommonChange { table: None, key: "theme", value: quote(&second.name) }]);
        assert!(texts.iter().any(|(t, _)| t == "Open config.toml"), "the file at the foot");
        frame(&ctx, &mut state, Vec::new(), Keys { next: true, ..Keys::default() });
        assert_eq!(state.page, 0, "round from the last");
        frame(&ctx, &mut state, Vec::new(), Keys { back: true, ..Keys::default() });
        assert_eq!(state.page, 1);
        frame(&ctx, &mut state, Vec::new(), Keys { find: true, ..Keys::default() });
        let (_, texts) = frame(&ctx, &mut state, Vec::new(), Keys::default());
        assert!(texts.iter().any(|(t, _)| t == "Mode"), "the theme page's own rows");
    }
}

#[cfg(test)]
mod seg_named_tests {
    use super::*;

    #[test]
    fn a_named_group_returns_its_response() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let look = Look { c: ito_theme::colors(), q: "" };
            let (changed, r) = seg_named(ui, look, "day", "Day type", 1u8, &[(0, "On"), (1, "Off")]);
            assert_eq!(changed, None);
            assert!(r.rect.width() > 0.0);
        });
        out.textures_delta.clear();
    }
}
