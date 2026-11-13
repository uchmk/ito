//! The Shell card: the program new terminals run, its arguments and its
//! environment variables. The values are the app's own (a terminal's, a file
//! manager's), so the card returns [`ShellChange`]s and the app writes them.

use egui::RichText;
use std::collections::BTreeMap;

use crate::{button_with, field, focus_ring, keyed_row, section, sep, take_keys, z, Drafts, Look, PairWords, Pairs, PairsEditor, Tone};

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
    /// The head of the value column, and the grey text of an empty value box.
    pub value: &'static str,
    /// The head of the name column.
    pub name: &'static str,
    /// Shown while there is no variable.
    pub empty: &'static str,
    /// A value with no name.
    pub missing: &'static str,
    /// A name used by an earlier variable.
    pub twice: &'static str,
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
    value: "Value",
    name: "Name",
    empty: "None yet",
    missing: "Enter a name",
    twice: "This name is used above",
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
    name: "名前",
    empty: "まだありません",
    missing: "名前を入れてください",
    twice: "同じ名前が上にあります",
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
    vars: Option<Pairs>,
}

impl ShellEdit {
    /// Opens the editor on `env`: its rows get keys `0..len`, in name order.
    pub(crate) fn open(&mut self, env: &BTreeMap<String, String>) {
        self.vars = Some(Pairs::new(env.iter().map(|(n, v)| (n.as_str(), v.as_str()))));
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
            let (picked, resp) = crate::select_with_response(ui, "shell", now.program, &shells, false, &[]);
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
        if let Some(vars) = &mut edit.vars {
            let bad = |n: &str| bad_name(n).then(|| w.bad_name.to_owned());
            let words = PairWords {
                name: w.name,
                value: w.value,
                add: w.add,
                remove: w.remove,
                empty: w.empty,
                missing: w.missing,
                twice: w.twice,
            };
            let shown = PairsEditor::new("settings.shell.env", words)
                .hints("NAME", w.value)
                .mono_names()
                .mono_values()
                .check(&bad)
                .unique(cfg!(windows))
                .show(ui, vars);
            ui.add_space(ito_theme::scale::space::SM);
            // Cancel then Save at the right, in the order they read and Tab goes.
            let foot = egui::vec2(ui.available_width(), z(ui, 30.0));
            ui.allocate_ui_with_layout(foot, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let key = ui.unique_id().with("env-foot");
                let width = ui.data(|d| d.get_temp::<f32>(key)).unwrap_or(ui.available_width());
                let height = foot.y;
                let drawn = ui.allocate_ui_with_layout(egui::vec2(width, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let r = button_with(ui, l, w.cancel, Tone::Plain);
                    ito_a11y::test_id(&r, "settings.shell.env.cancel");
                    if r.clicked() {
                        done = true;
                    }
                    let b = egui::Button::new(RichText::new(w.save).size(z(ui, 12.5)))
                        .corner_radius(ito_theme::scale::radius::CONTROL)
                        .min_size(egui::vec2(0.0, z(ui, 30.0)));
                    let r = ui.add_enabled(shown.valid, b);
                    take_keys(ui, &r);
                    focus_ring(ui, &r);
                    ito_a11y::test_id(&r, "settings.shell.env.save");
                    if r.clicked() {
                        out.extend(env_changes(now.env, &vars.pairs()));
                        done = true;
                    }
                });
                let used = drawn.response.rect.width();
                if (used - width).abs() > 0.5 {
                    ui.data_mut(|d| d.insert_temp(key, used));
                    ui.ctx().request_repaint();
                }
            });
            ui.add_space(ito_theme::scale::space::SM);
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
        edit.vars.as_ref().map_or_else(Vec::new, |v| v.rows().map(|r| r.0).collect())
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
        assert!(!names.iter().any(|n| n.starts_with("settings.select.")), "the shell button has one name: {names:?}");

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

        edit.vars.as_mut().unwrap().add();
        edit.vars.as_mut().unwrap().add();
        assert_eq!(keys(&edit), [0, 1, 2, 3]);
        edit.vars.as_mut().unwrap().remove(1);
        let names = frame(&ctx, &mut drafts, &mut edit, &now);
        assert_eq!(keys(&edit), [0, 2, 3]);
        assert!(has(&names, "settings.shell.env.row.0.name") && has(&names, "settings.shell.env.row.2.name"));
        assert!(has(&names, "settings.shell.env.row.3.value"));
        assert!(!names.iter().any(|n| n.starts_with("settings.shell.env.row.1.")), "{names:?}");

        edit.vars = None;
        let names = frame(&ctx, &mut drafts, &mut edit, &now);
        assert!(!has(&names, "settings.shell.env.add"));
    }

    /// One frame of the card with `events`; the changes it returns.
    fn step(ctx: &egui::Context, drafts: &mut Drafts, edit: &mut ShellEdit, now: &Shell, events: Vec<egui::Event>) -> Vec<ShellChange> {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0))),
            ..Default::default()
        };
        input.events = events;
        let mut got = Vec::new();
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                got = shell_card(ui, Look { c: ito_theme::colors(), q: "" }, &SHELL_EN, drafts, edit, now);
            });
        });
        out.textures_delta.clear();
        got
    }

    fn click_id(ctx: &egui::Context, name: &str, mut draw: impl FnMut(Vec<egui::Event>) -> Vec<ShellChange>) -> Vec<ShellChange> {
        draw(vec![]);
        draw(vec![]);
        let at = ito_a11y::take_test_ids(ctx, egui::ViewportId::ROOT)
            .into_iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name}"))
            .rect
            .center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        draw(vec![egui::Event::PointerMoved(at)]);
        draw(vec![button(true)]);
        let got = draw(vec![button(false)]);
        draw(vec![]);
        got
    }

    fn save_enabled(ctx: &egui::Context, out: &egui::FullOutput) -> bool {
        let _ = ctx;
        let update = out.platform_output.accesskit_update.as_ref().expect("accesskit");
        update.nodes.iter().any(|(_, n)| n.author_id() == Some("settings.shell.env.save") && !n.is_disabled())
    }

    #[test]
    fn save_is_off_for_a_bad_a_repeated_or_a_nameless_row() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let env = map(&[("A", "1")]);
        let now = Shell { program: "", auto: "Automatic", shells: &[], args: &[], env: &env };
        let (mut drafts, mut edit) = (Drafts::new(), ShellEdit::default());
        let run = |ctx: &egui::Context, drafts: &mut Drafts, edit: &mut ShellEdit| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 800.0))),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    shell_card(ui, Look { c: ito_theme::colors(), q: "" }, &SHELL_EN, drafts, edit, &now);
                });
            });
            out.textures_delta.clear();
            ito_a11y::take_test_ids(ctx, egui::ViewportId::ROOT);
            save_enabled(ctx, &out)
        };
        edit.open(&env);
        assert!(run(&ctx, &mut drafts, &mut edit), "the saved rows are fine");
        let vars = edit.vars.as_mut().unwrap();
        let bad = vars.push("A B", "x");
        assert!(!run(&ctx, &mut drafts, &mut edit), "a name with a space");
        edit.vars.as_mut().unwrap().remove(bad);
        edit.vars.as_mut().unwrap().push("", "orphan");
        assert!(!run(&ctx, &mut drafts, &mut edit), "a value with no name");
        edit.open(&env);
        edit.vars.as_mut().unwrap().push("A", "2");
        assert!(!run(&ctx, &mut drafts, &mut edit), "a repeated name");
    }

    #[test]
    fn the_cancel_button_closes_the_editor_and_a_reopen_shows_the_saved_rows() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let env = map(&[("A", "1")]);
        let now = Shell { program: "", auto: "Automatic", shells: &[], args: &[], env: &env };
        let (mut drafts, mut edit) = (Drafts::new(), ShellEdit::default());
        edit.open(&env);
        edit.vars.as_mut().unwrap().add();
        let got = click_id(&ctx, "settings.shell.env.cancel", |ev| step(&ctx, &mut drafts, &mut edit, &now, ev));
        assert!(got.is_empty() && edit.vars.is_none());
        let got = click_id(&ctx, "settings.shell.env.edit", |ev| step(&ctx, &mut drafts, &mut edit, &now, ev));
        assert!(got.is_empty());
        assert_eq!(keys(&edit), [0], "the saved rows, not the cancelled ones");
    }

    #[test]
    fn the_save_button_returns_the_changes() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let env = map(&[("A", "1")]);
        let now = Shell { program: "", auto: "Automatic", shells: &[], args: &[], env: &env };
        let (mut drafts, mut edit) = (Drafts::new(), ShellEdit::default());
        edit.open(&env);
        edit.vars.as_mut().unwrap().push(" B ", "2");
        let got = click_id(&ctx, "settings.shell.env.save", |ev| step(&ctx, &mut drafts, &mut edit, &now, ev));
        assert_eq!(got, vec![ShellChange::SetVar { name: "B".into(), value: "2".into() }]);
        assert!(edit.vars.is_none());
    }

    #[test]
    fn the_words_cover_both_languages() {
        for w in [&SHELL_EN, &SHELL_JA] {
            assert!(![w.name, w.empty, w.missing, w.twice, w.value].iter().any(|t| t.is_empty()));
        }
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
