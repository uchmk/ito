//! The Shell card: the program new terminals run, its arguments and its
//! environment variables. The values are the app's own (a terminal's, a file
//! manager's), so the card returns [`ShellChange`]s and the app writes them.

use egui::RichText;
use std::collections::BTreeMap;

use crate::{button, field, focus_ring, keyed_row, section, sep, take_keys, z, Drafts, Look};

/// The card's words, in one language.
pub struct ShellWords {
    pub head: &'static str,
    pub shell: &'static str,
    pub shell_note: &'static str,
    pub args: &'static str,
    pub args_note: &'static str,
    pub env: &'static str,
    pub env_note: &'static str,
    pub edit: &'static str,
    /// `{n}` is the number of variables.
    pub edit_n: &'static str,
    pub add: &'static str,
    pub remove: &'static str,
    pub value: &'static str,
    pub bad_name: &'static str,
    pub save: &'static str,
    pub cancel: &'static str,
}

pub const SHELL_EN: ShellWords = ShellWords {
    head: "SHELL",
    shell: "Default shell",
    shell_note: "What a new terminal runs",
    args: "Arguments",
    args_note: "Separated by spaces",
    env: "Environment variables",
    env_note: "Added to every new terminal",
    edit: "Edit",
    edit_n: "Edit ({n})",
    add: "Add",
    remove: "Remove",
    value: "value",
    bad_name: "A name cannot hold = or a space",
    save: "Save",
    cancel: "Cancel",
};

pub const SHELL_JA: ShellWords = ShellWords {
    head: "シェル",
    shell: "既定のシェル",
    shell_note: "新しい端末が動かすもの",
    args: "引数",
    args_note: "空白で区切る",
    env: "環境変数",
    env_note: "新しい端末すべてに足す",
    edit: "編集",
    edit_n: "編集（{n}）",
    add: "追加",
    remove: "削除",
    value: "値",
    bad_name: "名前に = と空白は使えません",
    save: "保存",
    cancel: "取り消し",
};

impl ShellWords {
    /// The words for a language code (`ja`, `en`); English for the rest.
    pub fn of(language: &str) -> &'static ShellWords {
        if language == "ja" {
            &SHELL_JA
        } else {
            &SHELL_EN
        }
    }
}

/// The card's rows in the app's settings file, for [`crate::Nav::index`].
pub const SHELL_KEYS: [&str; 3] = ["shell.program", "shell.args", "shell.env"];

/// What the card shows.
pub struct Shell<'a> {
    /// The program now; empty for the automatic one.
    pub program: &'a str,
    /// What the automatic one is called ("Automatic (PowerShell)").
    pub auto: &'a str,
    /// The shells found, as (label, program).
    pub shells: &'a [(String, String)],
    pub args: &'a [String],
    pub env: &'a BTreeMap<String, String>,
}

/// The variables being edited; the app keeps one across frames.
#[derive(Default)]
pub struct ShellEdit {
    vars: Option<Vec<(String, String)>>,
}

/// One change of the card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellChange {
    /// An empty program is the automatic one.
    Program(String),
    Args(Vec<String>),
    SetVar {
        name: String,
        value: String,
    },
    RemoveVar(String),
}

/// `=` and spaces cannot be in a variable's name.
fn bad_name(n: &str) -> bool {
    n.contains(['=', ' '])
}

/// The changes saving `vars` makes to `old`: removed names, then new or changed values.
pub fn env_changes(old: &BTreeMap<String, String>, vars: &[(String, String)]) -> Vec<ShellChange> {
    let kept: Vec<(&str, &str)> = vars.iter().map(|(n, v)| (n.trim(), v.as_str())).filter(|(n, _)| !n.is_empty()).collect();
    let mut out: Vec<ShellChange> =
        old.keys().filter(|k| !kept.iter().any(|(n, _)| n == k)).map(|k| ShellChange::RemoveVar(k.clone())).collect();
    for (n, v) in kept {
        if old.get(n).map(String::as_str) != Some(v) {
            out.push(ShellChange::SetVar { name: n.to_owned(), value: v.to_owned() });
        }
    }
    out
}

/// The Shell card.
pub fn shell_card(ui: &mut egui::Ui, l: Look, w: &ShellWords, drafts: &mut Drafts, edit: &mut ShellEdit, now: &Shell) -> Vec<ShellChange> {
    let mut out = Vec::new();
    section(ui, l, w.head, |ui| {
        let mut shells: Vec<(&str, &str)> = vec![("", now.auto)];
        shells.extend(now.shells.iter().map(|(label, program)| (program.as_str(), label.as_str())));
        if !now.program.is_empty() && !shells.iter().any(|(p, _)| *p == now.program) {
            shells.push((now.program, now.program));
        }
        if let Some(p) = keyed_row(ui, l, SHELL_KEYS[0], w.shell, w.shell_note, |ui| crate::select(ui, "shell", now.program, &shells)) {
            out.push(ShellChange::Program(p.to_owned()));
        }
        sep(ui, l);
        let args = now.args.join(" ");
        if let Some(t) = keyed_row(ui, l, SHELL_KEYS[1], w.args, w.args_note, |ui| field(ui, drafts, "shell-args", &args, "-NoLogo", 200.0))
        {
            out.push(ShellChange::Args(t.split_whitespace().map(str::to_owned).collect()));
        }
        sep(ui, l);
        let words = if now.env.is_empty() { w.edit.to_owned() } else { w.edit_n.replace("{n}", &now.env.len().to_string()) };
        if keyed_row(ui, l, SHELL_KEYS[2], w.env, w.env_note, |ui| button(ui, l, &words)) {
            edit.vars = match edit.vars {
                Some(_) => None,
                None => Some(now.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
            };
        }
        let mut done = false;
        if let Some(vars) = &mut edit.vars {
            let mut gone = None;
            for (k, (name, value)) in vars.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(name).hint_text("NAME").desired_width(160.0).font(egui::FontId::monospace(z(ui, 12.5))),
                    );
                    ui.label("=");
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .hint_text(w.value)
                            .desired_width(280.0)
                            .font(egui::FontId::monospace(z(ui, 12.5))),
                    );
                    if ui.small_button(w.remove).clicked() {
                        gone = Some(k);
                    }
                });
            }
            if let Some(k) = gone {
                vars.remove(k);
            }
            let bad = vars.iter().any(|(n, _)| bad_name(n.trim()));
            ui.horizontal(|ui| {
                if button(ui, l, w.add) {
                    vars.push((String::new(), String::new()));
                }
                if bad {
                    ui.label(egui::RichText::new(w.bad_name).size(z(ui, 12.0)).color(l.c.err));
                }
                // Cancel then Save at the right, in the order they read and Tab goes.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let key = ui.unique_id().with("env-foot");
                    let width = ui.data(|d| d.get_temp::<f32>(key)).unwrap_or(ui.available_width());
                    let height = ui.available_height();
                    let shown =
                        ui.allocate_ui_with_layout(egui::vec2(width, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if button(ui, l, w.cancel) {
                                done = true;
                            }
                            let b = egui::Button::new(RichText::new(w.save).size(z(ui, 12.5)))
                                .corner_radius(7.0)
                                .min_size(egui::vec2(0.0, z(ui, 30.0)));
                            let r = ui.add_enabled(!bad, b);
                            take_keys(ui, &r);
                            focus_ring(ui, &r);
                            if r.clicked() {
                                out.extend(env_changes(now.env, vars));
                                done = true;
                            }
                        });
                    let used = shown.response.rect.width();
                    if (used - width).abs() > 0.5 {
                        ui.data_mut(|d| d.insert_temp(key, used));
                        ui.ctx().request_repaint();
                    }
                });
            });
            ui.add_space(8.0);
        }
        if done {
            edit.vars = None;
        }
    });
    out
}

/// The card's words, for the settings' search.
pub fn shell_index(w: &ShellWords) -> Vec<&'static str> {
    let mut v = vec![w.head, w.shell, w.shell_note, w.args, w.args_note, w.env, w.env_note];
    v.extend(SHELL_KEYS);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(p: &[(&str, &str)]) -> BTreeMap<String, String> {
        p.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn saving_removes_dropped_names_and_sets_new_or_changed_ones() {
        let old = map(&[("A", "1"), ("B", "2"), ("C", "3")]);
        let vars = vec![("A".into(), "1".into()), ("B".into(), "9".into()), (" D ".into(), "4".into()), ("".into(), "x".into())];
        assert_eq!(
            env_changes(&old, &vars),
            vec![
                ShellChange::RemoveVar("C".into()),
                ShellChange::SetVar { name: "B".into(), value: "9".into() },
                ShellChange::SetVar { name: "D".into(), value: "4".into() },
            ]
        );
    }

    #[test]
    fn a_name_with_equals_or_a_space_is_bad() {
        assert!(bad_name("A=B") && bad_name("A B") && !bad_name("A_B"));
    }

    #[test]
    fn the_card_draws_and_asks_for_nothing_untouched() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0))),
            ..Default::default()
        };
        let shells = vec![("pwsh".to_owned(), "pwsh.exe".to_owned())];
        let env = map(&[("A", "1")]);
        let now = Shell { program: "gone.exe", auto: "Automatic", shells: &shells, args: &["-NoLogo".to_owned()], env: &env };
        let (mut drafts, mut edit, mut got) = (Drafts::new(), ShellEdit::default(), Vec::new());
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                got = shell_card(ui, Look { c: ito_theme::colors(), q: "" }, &SHELL_JA, &mut drafts, &mut edit, &now);
            });
        });
        out.textures_delta.clear();
        assert!(got.is_empty());
        assert!(shell_index(&SHELL_EN).contains(&"shell.env"));
    }
}
