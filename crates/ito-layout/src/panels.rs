//! egui's resizable side panels with their sizes kept between runs (the
//! `panels` feature): `panels.toml` in the app's state folder, one size per
//! panel by name, written when a drag of the panel's edge ends.
//!
//! A panel also fills its whole size. egui keeps a resizable panel at the
//! size of what it drew, so one that drew little (an empty reader) would
//! otherwise shrink to it and stay so.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ito_common::kept::{self, Writer};
use serde::{Deserialize, Serialize};

/// The file in the state folder.
pub const FILE: &str = "panels.toml";

/// Which edge of the window a panel stands at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    size: BTreeMap<String, f32>,
}

/// The panels' sizes: what was kept, and what the edges are dragged to.
#[derive(Debug, Default)]
pub struct Panels {
    size: BTreeMap<String, f32>,
    writer: Option<Writer>,
    /// A size changed by a drag not yet written.
    unsaved: bool,
}

impl Panels {
    /// The sizes kept in `dir`, the app's state folder. `None` keeps nothing
    /// and writes nothing (a `--keys` run): every panel starts at its default.
    pub fn load(dir: Option<PathBuf>) -> Self {
        let Some(path) = dir.map(|d| d.join(FILE)) else { return Self::default() };
        let size = kept::read::<File>(&path).map(|f| f.size).unwrap_or_default();
        Self { size, writer: Some(Writer::new(path, true)), unsaved: false }
    }

    /// The size kept for the panel `name`: its width at the left or right,
    /// its height at the top or bottom.
    pub fn size(&self, name: &str) -> Option<f32> {
        self.size.get(name).copied()
    }

    /// Show the resizable panel `name` at `side`, starting at the size kept
    /// for it, else `default`. `tune` sets the rest (frame, size range); it
    /// leaves the default size alone. `add` draws the contents over all of
    /// the panel.
    pub fn show<R>(
        &mut self,
        ui: &mut egui::Ui,
        side: Side,
        name: &str,
        default: f32,
        tune: impl FnOnce(egui::Panel) -> egui::Panel,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let id = egui::Id::new(name);
        let panel = match side {
            Side::Left => egui::Panel::left(id),
            Side::Right => egui::Panel::right(id),
            Side::Top => egui::Panel::top(id),
            Side::Bottom => egui::Panel::bottom(id),
        };
        let start = self.size(name).unwrap_or(default);
        let r = tune(panel.resizable(true).default_size(start)).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            add(ui)
        });
        // Only a drag of the edge counts: the window made small (or
        // minimized) squeezes the panel without its size being chosen.
        if ui.ctx().is_being_dragged(id.with("__resize")) {
            let now = match side {
                Side::Left | Side::Right => r.response.rect.width(),
                Side::Top | Side::Bottom => r.response.rect.height(),
            };
            if self.size(name).is_none_or(|was| (was - now).abs() >= 0.5) {
                self.size.insert(name.to_owned(), now.round());
                self.unsaved = true;
            }
        } else if self.size(name).is_none() && {
            let screen = ui.ctx().content_rect();
            // Points shrink as the zoom grows (10000 / 1.2 = 8333), so measure in unzoomed points.
            let zoom = ui.ctx().zoom_factor();
            screen.width() * zoom >= 9000.0 || screen.height() * zoom >= 9000.0
        } {
            // The first frame has no window size (egui uses 10000x10000), and a default worked
            // out from that would stick. Forget it for that frame only: later on a panel with
            // no chosen size must still be free to grow to its contents.
            ui.ctx().data_mut(|d| d.remove::<egui::containers::panel::PanelState>(id));
        } else if self.unsaved && !ui.ctx().input(|i| i.pointer.any_down()) {
            self.unsaved = false;
            if let Some(w) = &self.writer {
                w.save_toml(&File { size: self.size.clone() });
            }
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-panels-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// One frame with a reader panel at the right that draws one word, the
    /// pointer doing `events`; the reader's width.
    fn frame(ctx: &egui::Context, panels: &mut Panels, events: Vec<egui::Event>) -> f32 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut width = 0.0;
        let mut out = ctx.run_ui(input, |ui| {
            width = panels.show(ui, Side::Right, "reader", 400.0, |p| p, |ui| ui.label("x")).response.rect.width();
        });
        out.textures_delta.clear();
        width
    }

    /// The first frame has no window size; a default worked out from it must not stick.
    #[test]
    fn a_default_from_the_sizeless_first_frame_does_not_stick() {
        sizeless_first_frame(1.0);
    }

    #[test]
    fn a_default_from_the_sizeless_first_frame_does_not_stick_when_zoomed() {
        sizeless_first_frame(1.2);
    }

    fn sizeless_first_frame(zoom: f32) {
        let (ctx, mut panels) = (egui::Context::default(), Panels::load(None));
        ctx.set_zoom_factor(zoom);
        let share = |ui: &egui::Ui| ui.available_width() * 0.55;
        let mut show = |ctx: &egui::Context, rect: Option<egui::Rect>| {
            let mut width = 0.0;
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: rect, ..Default::default() }, |ui| {
                let d = share(ui);
                width = panels.show(ui, Side::Right, "reader", d, |p| p, |ui| ui.label("x")).response.rect.width();
            });
            out.textures_delta.clear();
            width
        };
        show(&ctx, None);
        let real = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0)));
        let mut w = 0.0;
        for _ in 0..3 {
            w = show(&ctx, real);
        }
        assert!((w - 550.0).abs() < 1.0, "got {w}");
    }

    #[test]
    fn a_panel_with_no_saved_size_grows_to_its_contents() {
        let (ctx, mut panels) = (egui::Context::default(), Panels::load(None));
        let real = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0)));
        let mut h = 0.0;
        for _ in 0..4 {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: real, ..Default::default() }, |ui| {
                h = panels
                    .show(ui, Side::Bottom, "compose", 200.0, |p| p, |ui| ui.allocate_space(egui::vec2(10.0, 260.0)).0)
                    .response
                    .rect
                    .height();
            });
            out.textures_delta.clear();
        }
        assert!(h >= 260.0, "got {h}");
    }

    fn button(at: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    /// A panel that draws little keeps its size; a drag of its edge is kept
    /// in `panels.toml` once it ends, and the next run starts there.
    #[test]
    fn a_dragged_size_is_kept() {
        let d = dir("drag");
        let (ctx, mut panels) = (egui::Context::default(), Panels::load(Some(d.clone())));
        for _ in 0..3 {
            assert_eq!(frame(&ctx, &mut panels, vec![]), 400.0, "one word does not shrink it");
        }
        let edge = egui::pos2(600.0, 300.0);
        frame(&ctx, &mut panels, vec![egui::Event::PointerMoved(edge)]);
        frame(&ctx, &mut panels, vec![button(edge, true)]);
        for x in [580.0, 550.0, 500.0] {
            frame(&ctx, &mut panels, vec![egui::Event::PointerMoved(egui::pos2(x, 300.0))]);
        }
        assert_eq!(panels.size("reader"), Some(500.0));
        frame(&ctx, &mut panels, vec![button(egui::pos2(500.0, 300.0), false)]);
        frame(&ctx, &mut panels, vec![]);
        let path = d.join(FILE);
        for _ in 0..100 {
            if path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[size]\nreader = 500.0\n");
        let (ctx, mut again) = (egui::Context::default(), Panels::load(Some(d.clone())));
        assert_eq!(frame(&ctx, &mut again, vec![]), 500.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Without a folder (`--keys`), the default and nothing written.
    #[test]
    fn no_folder_keeps_nothing() {
        let mut panels = Panels::load(None);
        assert_eq!(frame(&egui::Context::default(), &mut panels, vec![]), 400.0);
        assert_eq!(panels.size("reader"), None);
    }
}
