//! {{app}}: {{description}}
//!
//! What is wired here: the window (its place kept), common.toml's theme,
//! language and scale, `--version` / `--help`, and F12 (the bug report).
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
        let (table, errors) = i18n::table(&i18n::resolve(common.language.as_deref()));
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
        Self { table, common, tracker: saved.tracker() }
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
        if let Some(t) = &mut self.tracker {
            t.follow(&ctx);
        }
        ui.heading(self.table.tr("app.hello"));
        ui.label(self.table.tr("app.bug"));
        ui.label(format!("scale {:.1}", self.common.scale_or_one()));
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
    let options = eframe::NativeOptions { viewport: saved.viewport(viewport), ..Default::default() };
    eframe::run_native("{{app}}", options, Box::new(move |cc| Ok(Box::new(App::new(cc, &saved)))))
}
