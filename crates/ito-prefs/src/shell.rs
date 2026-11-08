//! The Shell card: the program new terminals run, its arguments and its
//! environment variables. The values are the app's own (a terminal's, a file
//! manager's), so the card returns [`ShellChange`]s and the app writes them.

use egui::RichText;
use std::collections::BTreeMap;

use crate::{button_with, field, focus_ring, keyed_row, section, sep, take_keys, z, Drafts, Look, Tone};

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
    vars: Option<Vec<EnvRow>>,
    /// The key the next added row gets.
    next: u32,
}

/// One variable being edited. The key is fixed for the row's life (removing another row does not renumber it), so its test IDs
/// (`settings.shell.env.row.<key>.name`) keep naming the same row.
struct EnvRow {
    key: u32,
    name: String,
    value: String,
}

impl ShellEdit {
    /// Opens the editor on `env`: its rows get keys `0..len`, in name order.
    fn open(&mut self, env: &BTreeMap<String, String>) {
        let rows: Vec<EnvRow> =
            env.iter().enumerate().map(|(k, (n, v))| EnvRow { key: k as u32, name: n.clone(), value: v.clone() }).collect();
        self.next = rows.len() as u32;
        self.vars = Some(rows);
    }

    /// Adds an empty row with a new key; nothing while the editor is shut.
    fn add(&mut self) {
        if let Some(vars) = &mut self.vars {
            vars.push(EnvRow { key: self.next, name: String::new(), value: String::new() });
            self.next += 1;
        }
    }
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
        let program = keyed_row(ui, l, SHELL_KEYS[0], w.shell, w.shell_note, |ui| {
            let (picked, resp) = crate::select_with_response(ui, "shell", now.program, &shells);
            ito_a11y::test_id(&resp, "settings.shell.program");
            picked
        });
        if let Some(p) = program {
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
        let clicked = keyed_row(ui, l, SHELL_KEYS[2], w.env, w.env_note, |ui| {
            let r = button_with(ui, l, &words, Tone::Plain);
            ito_a11y::test_id(&r, "settings.shell.env.edit");
            r.clicked()
        });
        if clicked {
            if edit.vars.is_some() {
                edit.vars = None;
            } else {
                edit.open(now.env);
            }
        }
        let mut done = false;
        let mut adding = false;
        if let Some(vars) = &mut edit.vars {
            let mut gone = None;
            for row in vars.iter_mut() {
                let key = row.key;
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut row.name)
                            .hint_text("NAME")
                            .desired_width(160.0)
                            .font(egui::FontId::monospace(z(ui, 12.5))),
                    );
                    ito_a11y::test_id(&r, &format!("settings.shell.env.row.{key}.name"));
                    ui.label("=");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut row.value)
                            .hint_text(w.value)
                            .desired_width(280.0)
                            .font(egui::FontId::monospace(z(ui, 12.5))),
                    );
                    ito_a11y::test_id(&r, &format!("settings.shell.env.row.{key}.value"));
                    let r = ui.small_button(w.remove);
                    ito_a11y::test_id(&r, &format!("settings.shell.env.row.{key}.remove"));
                    if r.clicked() {
                        gone = Some(key);
                    }
                });
            }
            if let Some(key) = gone {
                vars.retain(|r| r.key != key);
            }
            let bad = vars.iter().any(|r| bad_name(r.name.trim()));
            ui.horizontal(|ui| {
                let r = button_with(ui, l, w.add, Tone::Plain);
                ito_a11y::test_id(&r, "settings.shell.env.add");
                if r.clicked() {
                    // The row is added after the list is drawn, so draw again to show it.
                    adding = true;
                    ui.ctx().request_repaint();
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
                            let r = button_with(ui, l, w.cancel, Tone::Plain);
                            ito_a11y::test_id(&r, "settings.shell.env.cancel");
                            if r.clicked() {
                                done = true;
                            }
                            let b = egui::Button::new(RichText::new(w.save).size(z(ui, 12.5)))
                                .corner_radius(ito_theme::scale::radius::CONTROL)
                                .min_size(egui::vec2(0.0, z(ui, 30.0)));
                            let r = ui.add_enabled(!bad, b);
                            take_keys(ui, &r);
                            focus_ring(ui, &r);
                            ito_a11y::test_id(&r, "settings.shell.env.save");
                            if r.clicked() {
                                let pairs = vars.iter().map(|r| (r.name.clone(), r.value.clone())).collect::<Vec<_>>();
                                out.extend(env_changes(now.env, &pairs));
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
            ui.add_space(ito_theme::scale::space::SM);
        }
        if adding {
            edit.add();
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

    fn frame(ctx: &egui::Context, drafts: &mut Drafts, edit: &mut ShellEdit, now: &Shell) -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0))),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                shell_card(ui, Look { c: ito_theme::colors(), q: "" }, &SHELL_EN, drafts, edit, now);
            });
        });
        out.textures_delta.clear();
        let mut names: Vec<String> = ito_a11y::take_test_ids(ctx, egui::ViewportId::ROOT).into_iter().map(|e| e.name).collect();
        names.sort();
        names
    }

    fn keys(edit: &ShellEdit) -> Vec<u32> {
        edit.vars.as_ref().map_or_else(Vec::new, |v| v.iter().map(|r| r.key).collect())
    }

    #[test]
    fn the_card_has_test_ids_and_env_rows_keep_their_keys() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let env = map(&[("A", "1"), ("B", "2")]);
        let now = Shell { program: "", auto: "Automatic", shells: &[], args: &[], env: &env };
        let (mut drafts, mut edit) = (Drafts::new(), ShellEdit::default());
        let has = |names: &[String], n: &str| names.iter().any(|x| x == n);

        let names = frame(&ctx, &mut drafts, &mut edit, &now);
        for n in ["settings.shell.program", "settings.field.shell-args", "settings.shell.env.edit"] {
            assert!(has(&names, n), "{n} in {names:?}");
        }
        assert!(!has(&names, "settings.shell.env.add"));

        edit.open(&env);
        let names = frame(&ctx, &mut drafts, &mut edit, &now);
        for n in [
            "settings.shell.env.row.0.name",
            "settings.shell.env.row.1.value",
            "settings.shell.env.row.1.remove",
            "settings.shell.env.add",
            "settings.shell.env.cancel",
            "settings.shell.env.save",
        ] {
            assert!(has(&names, n), "{n} in {names:?}");
        }

        edit.add();
        edit.add();
        assert_eq!(keys(&edit), [0, 1, 2, 3]);
        edit.vars.as_mut().unwrap().retain(|r| r.key != 1);
        let names = frame(&ctx, &mut drafts, &mut edit, &now);
        assert_eq!(keys(&edit), [0, 2, 3]);
        assert!(has(&names, "settings.shell.env.row.0.name") && has(&names, "settings.shell.env.row.2.name"));
        assert!(has(&names, "settings.shell.env.row.3.value"));
        assert!(!names.iter().any(|n| n.starts_with("settings.shell.env.row.1.")), "{names:?}");

        edit.vars = None;
        let names = frame(&ctx, &mut drafts, &mut edit, &now);
        assert!(!has(&names, "settings.shell.env.add"));
    }

    #[test]
    fn without_accesskit_the_card_draws_and_notes_no_ids() {
        let ctx = egui::Context::default();
        let env = map(&[("A", "1")]);
        let now = Shell { program: "", auto: "Automatic", shells: &[], args: &[], env: &env };
        let (mut drafts, mut edit) = (Drafts::new(), ShellEdit::default());
        edit.open(&env);
        assert!(frame(&ctx, &mut drafts, &mut edit, &now).is_empty());
    }

    #[test]
    fn a_field_with_an_id_that_is_not_a_test_id_attaches_nothing() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut drafts = Drafts::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                field(ui, &mut drafts, "Shell Args", "", "", 100.0);
                field(ui, &mut drafts, "ok-one", "", "", 100.0);
            });
        });
        out.textures_delta.clear();
        let names: Vec<String> = ito_a11y::take_test_ids(&ctx, egui::ViewportId::ROOT).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["settings.field.ok-one"]);
    }
}
