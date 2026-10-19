//! {{app}}: {{description}}
//!
//! What is wired here: the window (its place kept, its own title bar with the
//! bell and the notice list), common.toml's theme, language and scale,
//! `--version` / `--help`, and F12 (the bug report).
//! What the uchmk apps also have and is still to wire is in TODO.md, each
//! item with the ito crate and an app that already does it.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod i18n;
mod icon;

use ito_common::window::{Saved, Tracker};

/// The window's icon, in pixels square.
const ICON_PX: u32 = 128;

const REPO: &str = "https://github.com/uchmk/{{app}}";

const USAGE: &str = "\
{{app}} — {{description}}

usage: {{app}} [--version | --help]
";

struct App {
    table: ito_i18n::Table,
    common: ito_common::Common,
    tracker: Option<Tracker>,
    lang: String,
    /// The last scale change, as the line the status bar shows.
    scale_note: Option<String>,
    /// What the bell lists, oldest first. `()` is where a click goes: this app
    /// has no place to jump to yet (use a type of the app's own for one).
    notices: Vec<ito_notices::Notice<()>>,
    /// The list under the bell is open, and where the bell was last drawn.
    bell_open: bool,
    bell_at: egui::Rect,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, saved: &Saved) -> Self {
        saved.put(cc);
        let base = ito_common::base_dir();
        let (common, mut warnings) = match base.as_deref().map(ito_common::Common::read) {
            Some(Ok(read)) => read,
            Some(Err(e)) => (Default::default(), vec![e]),
            None => Default::default(),
        };
        let lang = i18n::resolve(common.language.as_deref());
        let (table, errors) = i18n::table(&lang);
        warnings.extend(errors);
        for w in warnings {
            eprintln!("{{app}}: {w}");
        }
        let themes = ito_theme::builtin();
        let choice = common.theme.as_deref().unwrap_or("system");
        let dark = common.dark_theme.as_deref().unwrap_or("{{app}} Dark");
        let light = common.light_theme.as_deref().unwrap_or("{{app}} Light");
        if let Ok(theme) = ito_theme::pick(&themes, choice, dark, light, false) {
            ito_theme::set(theme.colors);
        }
        cc.egui_ctx.set_visuals(ito_theme::colors().visuals());
        // The scale is common.toml's, so egui's own Ctrl+= / Ctrl+- stay out of it.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        cc.egui_ctx.set_zoom_factor(common.scale_or_one());
        Self { table, common, tracker: saved.tracker(), lang, scale_note: None, notices: Vec::new(), bell_open: false, bell_at: egui::Rect::NOTHING }
    }

    /// Ctrl+wheel and Ctrl+= / Ctrl+- / Ctrl+0: the whole window's scale, one
    /// step at a time. It is applied now and written to common.toml off the UI
    /// thread, so the other apps at the same desk follow within 2 seconds.
    fn rescale(&mut self, ctx: &egui::Context) {
        use ito_common::ScaleTo;
        let wheel = ctx.input(|i| if i.modifiers.command { i.smooth_scroll_delta.y } else { 0.0 });
        let to = if wheel > 0.0 {
            Some(ScaleTo::In)
        } else if wheel < 0.0 {
            Some(ScaleTo::Out)
        } else {
            let key = |k| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, k));
            if key(egui::Key::Equals) || key(egui::Key::Plus) {
                Some(ScaleTo::In)
            } else if key(egui::Key::Minus) {
                Some(ScaleTo::Out)
            } else if key(egui::Key::Num0) {
                Some(ScaleTo::Reset)
            } else {
                None
            }
        };
        let Some(to) = to else { return };
        let next = ito_common::scale_step(self.common.scale_or_one(), to);
        if next == self.common.scale_or_one() {
            return;
        }
        self.common.scale = Some(next);
        ctx.set_zoom_factor(next);
        self.scale_note = Some(ito_common::scale_text(next, &self.lang));
        if let Some(base) = ito_common::base_dir() {
            std::thread::spawn(move || {
                let change = ito_common::scale_change(next);
                if let Err(e) = ito_common::edit_common(&base, |text| change.apply(text)) {
                    eprintln!("{{app}}: {e}");
                }
            });
        }
    }

    /// The window's own bar: the name, the palette box, the bell and the
    /// caption buttons. The bell opens the list of notices.
    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let unread = ito_notices::unread(&self.notices);
        let bell = self.table.trf("title.bell", &[&unread.to_string()]);
        let spec = ito_titlebar::Spec {
            own: !cfg!(target_os = "macos"),
            left: if cfg!(target_os = "macos") { 76.0 } else { 0.0 },
            maximized,
            unread,
            any_error: ito_notices::any_error(&self.notices),
            centre: true,
            words: ito_titlebar::Words {
                search: self.table.tr("title.search"),
                search_key: "Ctrl+Shift+P",
                bell: &bell,
                minimize: self.table.tr("title.minimize"),
                maximize: self.table.tr("title.maximize"),
                restore: self.table.tr("title.restore"),
                close: self.table.tr("title.close"),
            },
        };
        let name = self.table.tr("app.title").to_string();
        let out = ito_titlebar::show(
            ui,
            &spec,
            |ui, rect| {
                let galley = ui.painter().layout_no_wrap(name, egui::FontId::proportional(13.0), ito_theme::colors().fg);
                let x = rect.left() + 12.0;
                let right = x + galley.size().x;
                ui.painter().galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, egui::Color32::WHITE);
                right
            },
            |_, _| {},
        );
        if let Some(r) = out.bell_rect {
            self.bell_at = r;
        }
        if out.bell {
            self.bell_open = !self.bell_open;
        }
        // TODO: `out.search` is the palette box being clicked (see TODO.md).
        if let Some(op) = out.window {
            ito_titlebar::apply(&ctx, op, maximized);
        }
    }

    /// The list under the bell, while it is open.
    fn notice_list(&mut self, ctx: &egui::Context) {
        if !self.bell_open {
            return;
        }
        let t = &self.table;
        let title = t.trf("notices.title", &[&ito_notices::unread(&self.notices).to_string()]);
        let words = ito_notices::Words {
            title: &title,
            mark_read: t.tr("notices.mark_read"),
            empty: t.tr("notices.empty"),
            ago: t.tr("notices.ago"),
            kinds: [t.tr("notices.ok"), t.tr("notices.info"), t.tr("notices.warn"), t.tr("notices.err"), t.tr("notices.wait")],
        };
        match ito_notices::show(ctx, self.bell_at, &self.notices, &words) {
            Some(ito_notices::Action::Open(_, id)) => {
                self.notices.iter_mut().filter(|n| n.id == id).for_each(|n| n.read = true);
                self.bell_open = false;
            }
            Some(ito_notices::Action::ReadAll) => self.notices.iter_mut().for_each(|n| n.read = true),
            Some(ito_notices::Action::Close) => self.bell_open = false,
            None => {}
        }
    }

    /// F12: the browser opens the issue form filled with the version, the OS
    /// and what the app knows.
    fn report_bug(&self, ctx: &egui::Context) {
        let spec = ito_bugreport::Spec { app: "{{app}}", version: env!("CARGO_PKG_VERSION"), repo: REPO, template: "bug_report.yml" };
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(std::path::PathBuf::from);
        let context = ito_bugreport::Context { home: home.as_deref(), ..Default::default() };
        let url = ito_bugreport::url(&spec, &[("context", ito_bugreport::context(&context))]);
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F12)) {
            self.report_bug(&ctx);
        }
        self.rescale(&ctx);
        if let Some(t) = &mut self.tracker {
            t.follow(&ctx);
        }
        egui::Panel::top("titlebar")
            .exact_size(ito_titlebar::HEIGHT)
            .frame(egui::Frame::NONE.fill(ito_theme::colors().bg))
            .show(ui, |ui| self.title_bar(ui));
        self.notice_list(&ctx);
        ui.heading(self.table.tr("app.hello"));
        ui.label(self.table.tr("app.bug"));
        let note = self.scale_note.clone();
        ui.label(note.unwrap_or_else(|| ito_common::scale_text(self.common.scale_or_one(), &self.lang)));
        // Last, so the edges of the frameless window win over what is under them.
        if !cfg!(target_os = "macos") {
            ito_titlebar::resize_edges(&ctx);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        ito_theme::colors().bg.to_normalized_gamma_f32()
    }
}

/// Puts a line where whoever typed the command is looking, on standard error
/// when `err`. A release build on Windows has no console of its own;
/// `ito_console` writes to the console it was started from, or to the pipe
/// `{{app}}.com` hands on, and lets go of a write that fails.
fn say(text: &str, err: bool) {
    let line = format!("{text}\n");
    match err {
        true => ito_console::err(&line),
        false => ito_console::out(&line),
    }
}

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    if let Some(a) = args.next() {
        match a.as_str() {
            "--version" | "-V" => say(&ito_bugreport::version_line("{{app}}", env!("CARGO_PKG_VERSION")), false),
            "--help" | "-h" => say(USAGE, false),
            other => {
                say(&format!("{{app}}: unknown argument `{other}`\n\n{USAGE}"), true);
                std::process::exit(2);
            }
        }
        return Ok(());
    }
    let saved = Saved::load(ito_common::state_dir("{{app}}"));
    let viewport = egui::ViewportBuilder::default()
        .with_title("{{app}}")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([720.0, 420.0])
        .with_icon(std::sync::Arc::new(egui::IconData { rgba: icon::pixels(ICON_PX), width: ICON_PX, height: ICON_PX }));
    // The bar is the app's own (the Mac keeps its traffic lights over it).
    let viewport = ito_titlebar::frameless(viewport);
    let options = eframe::NativeOptions { viewport: saved.viewport(viewport), ..Default::default() };
    eframe::run_native("{{app}}", options, Box::new(move |cc| Ok(Box::new(App::new(cc, &saved)))))
}
