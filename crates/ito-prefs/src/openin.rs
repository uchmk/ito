//! The "Open in" card: the command lines that open a folder in a terminal or
//! the file manager, a file in an editor, two or three files in a diff / merge
//! tool (`ito_openin`), and the user's own additions -- diff / merge tools and
//! an editor's line argument. The card returns [`Edit`]s; the app writes them
//! with [`write_edits`] and applies them to its `Config` (`Config::apply`).

use egui::RichText;
use ito_openin::config::{Edit, UserTool, Write, AUTO};
use ito_openin::diff::{Found, KNOWN};
use ito_openin::{default_command, Config, Tool};

use crate::{button, field, row, sep, status, z, Drafts, Look};

/// The card's words, in the app's language.
pub struct OpenWords<'a> {
    pub head: &'a str,
    /// Labels of Terminal, Editor, Explorer, Diff, Merge (the order of `Tool::ALL`).
    pub tools: [&'a str; 5],
    /// Notes of the same.
    pub notes: [&'a str; 5],
    /// The row that picks a diff / merge tool, and its note.
    pub tool_label: &'a str,
    pub tool_note: &'a str,
    /// The choices that are not a named tool: the default, and the first one found.
    pub tool_default: &'a str,
    pub tool_auto: &'a str,
    /// The line under the rows while the tools are looked for, and when they are known;
    /// `{}` is the names, comma separated (or `none` for no tools).
    pub detecting: &'a str,
    pub detected: &'a str,
    pub none: &'a str,
    /// Heading of the user's own diff / merge tools, and the labels of their form.
    pub own_head: &'a str,
    pub own_name: &'a str,
    pub own_diff: &'a str,
    pub own_merge: &'a str,
    /// Heading of the editors' line arguments and the labels of their form.
    pub line_head: &'a str,
    pub line_editor: &'a str,
    pub line_template: &'a str,
    /// What the line-argument form is for, with an example (`-g {path}:{line}`).
    pub line_note: &'a str,
    pub add: &'a str,
    pub remove: &'a str,
}

/// The "add" forms' text while it is typed.
#[derive(Default)]
pub struct OpenForm {
    pub name: String,
    pub diff: String,
    pub merge: String,
    pub editor: String,
    pub template: String,
    /// Why the last "add" was refused.
    pub error: String,
}

/// The tool a user's form makes, or why not.
pub fn user_tool_edit(cfg: &Config, f: &OpenForm) -> Result<Edit, String> {
    let e = Edit::UserTool(f.name.trim().to_owned(), UserTool { diff: f.diff.clone(), merge: f.merge.clone() });
    cfg.check(&e).map(|()| e)
}

/// The line argument a form makes, or why not.
pub fn line_arg_edit(cfg: &Config, f: &OpenForm) -> Result<Edit, String> {
    let e = Edit::LineArg(f.editor.trim().to_owned(), f.template.clone());
    cfg.check(&e).map(|()| e)
}

/// The choices of the `diff_tool` row: the value written, and its label.
pub fn tool_choices(w: &OpenWords, cfg: &Config, found: &[Found]) -> Vec<(String, String)> {
    let mut v = vec![(String::new(), w.tool_default.to_owned()), (AUTO.to_owned(), w.tool_auto.to_owned())];
    for k in KNOWN.iter().filter(|k| found.iter().any(|f| f.tool.name == k.name)) {
        v.push((k.name.to_owned(), k.label.to_owned()));
    }
    v.extend(cfg.diff_tools.keys().map(|n| (n.clone(), n.clone())));
    // A tool written that is neither installed nor added is still shown, so the row does not lie.
    let now = cfg.open.diff_tool.trim();
    if !v.iter().any(|(k, _)| k == now) {
        v.push((now.to_owned(), now.to_owned()));
    }
    v
}

/// The line under the rows: what was found (`None`: still looking).
pub fn found_line(w: &OpenWords, found: Option<&[Found]>) -> String {
    match found {
        None => w.detecting.to_owned(),
        Some([]) => w.detected.replace("{}", w.none),
        Some(f) => w.detected.replace("{}", &f.iter().map(|x| x.tool.label).collect::<Vec<_>>().join(", ")),
    }
}

/// The card. `found` is `ito_openin::diff::detect_in_background`'s answer once
/// it has come (`None` before). The returned edits are checked already.
pub fn open_card(
    ui: &mut egui::Ui,
    l: Look,
    w: &OpenWords,
    cfg: &Config,
    found: Option<&[Found]>,
    drafts: &mut Drafts,
    form: &mut OpenForm,
) -> Vec<Edit> {
    let mut edits = Vec::new();
    let known = found.unwrap_or(&[]);
    crate::section(ui, l, w.head, |ui| {
        for (i, t) in Tool::ALL.into_iter().enumerate() {
            if i > 0 {
                sep(ui, l);
            }
            let now = match t {
                Tool::Terminal => &cfg.open.terminal,
                Tool::Editor => &cfg.open.editor,
                Tool::Explorer => &cfg.open.explorer,
                Tool::Diff => &cfg.open.diff,
                Tool::Merge => &cfg.open.merge,
            };
            // What would run when this is empty, as the hint.
            let hint = cfg.command(t, known);
            let hint = if hint.is_empty() { default_command(t).to_owned() } else { hint };
            let id = format!("open-{}", t.key());
            if let Some(v) = row(ui, l, w.tools[i], w.notes[i], |ui| field(ui, drafts, &id, now, &hint, 330.0)) {
                if cfg.check(&Edit::Command(t, v.clone())).is_ok() {
                    edits.push(Edit::Command(t, v));
                } else {
                    form.error = format!("{}: {v}", w.tools[i]);
                }
            }
        }
        sep(ui, l);
        let choices = tool_choices(w, cfg, known);
        let now = choices.iter().position(|(k, _)| k == cfg.open.diff_tool.trim()).unwrap_or(0);
        let options: Vec<(usize, &str)> = choices.iter().enumerate().map(|(i, (_, label))| (i, label.as_str())).collect();
        if let Some(i) = row(ui, l, w.tool_label, w.tool_note, |ui| crate::select(ui, "open-diff-tool", now, &options)) {
            edits.push(Edit::DiffTool(choices[i].0.clone()));
        }
        ui.add_space(4.0);
        ui.label(RichText::new(found_line(w, found)).size(z(ui, 12.0)).color(l.c.dim));
        ui.add_space(8.0);
    });

    crate::section(ui, l, w.own_head, |ui| {
        for (name, t) in &cfg.diff_tools {
            let mut change = None;
            row(ui, l, name, "", |ui| {
                if button(ui, l, w.remove) {
                    edits.push(Edit::RemoveUserTool(name.clone()));
                }
                let m = field(ui, drafts, &format!("own-merge-{name}"), &t.merge, w.own_merge, 250.0);
                let d = field(ui, drafts, &format!("own-diff-{name}"), &t.diff, w.own_diff, 250.0);
                change = d.map(|d| UserTool { diff: d, merge: t.merge.clone() }).or(m.map(|m| UserTool { diff: t.diff.clone(), merge: m }));
            });
            if let Some(t) = change {
                let e = Edit::UserTool(name.clone(), t);
                if cfg.check(&e).is_ok() {
                    edits.push(e);
                }
            }
            sep(ui, l);
        }
        row(ui, l, w.own_name, "", |ui| {
            if button(ui, l, w.add) {
                match user_tool_edit(cfg, form) {
                    Ok(e) => {
                        form.error.clear();
                        *form = OpenForm {
                            error: String::new(),
                            editor: std::mem::take(&mut form.editor),
                            template: std::mem::take(&mut form.template),
                            ..OpenForm::default()
                        };
                        edits.push(e);
                    }
                    Err(e) => form.error = e,
                }
            }
            ui.add(egui::TextEdit::singleline(&mut form.merge).hint_text(w.own_merge).desired_width(200.0));
            ui.add(egui::TextEdit::singleline(&mut form.diff).hint_text(w.own_diff).desired_width(200.0));
            ui.add(egui::TextEdit::singleline(&mut form.name).hint_text(w.own_name).desired_width(110.0));
        });
    });

    crate::section(ui, l, w.line_head, |ui| {
        let mut entries: Vec<(&String, &String)> = cfg.line_args.iter().collect();
        entries.sort();
        for (editor, template) in entries {
            if let Some(t) = row(ui, l, editor, "", |ui| {
                if button(ui, l, w.remove) {
                    edits.push(Edit::RemoveLineArg(editor.clone()));
                }
                field(ui, drafts, &format!("line-{editor}"), template, w.line_template, 300.0)
            }) {
                let e = Edit::LineArg(editor.clone(), t);
                if cfg.check(&e).is_ok() {
                    edits.push(e);
                }
            }
            sep(ui, l);
        }
        row(ui, l, w.line_editor, w.line_note, |ui| {
            if button(ui, l, w.add) {
                match line_arg_edit(cfg, form) {
                    Ok(e) => {
                        form.editor.clear();
                        form.template.clear();
                        form.error.clear();
                        edits.push(e);
                    }
                    Err(e) => form.error = e,
                }
            }
            ui.add(egui::TextEdit::singleline(&mut form.template).hint_text(w.line_template).desired_width(220.0));
            ui.add(egui::TextEdit::singleline(&mut form.editor).hint_text(w.line_editor).desired_width(130.0));
        });
        if !form.error.is_empty() {
            status(ui, &form.error, l.c.err);
            ui.add_space(8.0);
        }
    });
    edits
}

/// `text` with the edits written in, one key at a time (comments stay).
pub fn write_edits(text: &str, edits: &[Edit]) -> Result<String, String> {
    let mut out = text.to_owned();
    for w in edits.iter().flat_map(Edit::writes) {
        out = match w {
            Write::Set { table, key, value } => ito_common::set_key(&out, Some(&table), &key, &value)?,
            Write::Remove { table, key } => ito_common::remove_key(&out, Some(&table), &key)?,
        };
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: OpenWords = OpenWords {
        head: "OPEN IN",
        tools: ["Terminal", "Editor", "Explorer", "Diff", "Merge"],
        notes: [""; 5],
        tool_label: "Diff tool",
        tool_note: "",
        tool_default: "Default",
        tool_auto: "First found",
        detecting: "looking…",
        detected: "Found: {}",
        none: "none",
        own_head: "YOUR TOOLS",
        own_name: "Name",
        own_diff: "diff",
        own_merge: "merge",
        line_head: "LINE ARGS",
        line_editor: "Editor",
        line_template: "Args",
        line_note: "-g {path}:{line}",
        add: "Add",
        remove: "Remove",
    };

    fn cfg(text: &str) -> Config {
        Config::read(Some(text)).0
    }

    #[test]
    fn the_found_line_says_what_was_found() {
        assert_eq!(found_line(&W, None), "looking…");
        assert_eq!(found_line(&W, Some(&[])), "Found: none");
    }

    #[test]
    fn the_choices_hold_what_is_installed_added_or_written() {
        let found = [Found { tool: ito_openin::diff::known("meld").unwrap(), program: "meld".into() }];
        let c = cfg("[open]\ndiff_tool = \"gone\"\n[diff_tools.mine]\ndiff = \"m\"\n");
        let keys: Vec<String> = tool_choices(&W, &c, &found).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["", "auto", "meld", "mine", "gone"]);
    }

    #[test]
    fn the_forms_make_checked_edits_or_say_why_not() {
        let c = cfg("");
        let mut f = OpenForm { name: "mine".into(), diff: "mine {left} {right}".into(), ..OpenForm::default() };
        assert!(matches!(user_tool_edit(&c, &f), Ok(Edit::UserTool(n, _)) if n == "mine"));
        f.name = "meld".into();
        assert!(user_tool_edit(&c, &f).is_err());
        let f = OpenForm { editor: "MyEd.exe".into(), template: "--at={line}".into(), ..OpenForm::default() };
        assert!(line_arg_edit(&c, &f).is_err());
        let f = OpenForm { editor: "MyEd.exe".into(), template: "--at={line} {path}".into(), ..OpenForm::default() };
        assert!(line_arg_edit(&c, &f).is_ok());
    }

    #[test]
    fn edits_are_written_into_the_file_and_read_back() {
        let text = "# mine\n[open]\neditor = \"old\" # keep\n";
        let edits = [
            Edit::Command(Tool::Editor, "zed {path}".into()),
            Edit::DiffTool("mine".into()),
            Edit::UserTool("mine".into(), UserTool { diff: "mine {left} {right}".into(), merge: String::new() }),
            Edit::LineArg("MyEd".into(), "--at={line} {path}".into()),
        ];
        let out = write_edits(text, &edits).unwrap();
        assert!(out.starts_with("# mine\n") && out.contains("# keep"), "{out}");
        let (c, w) = Config::read(Some(&out));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.open.editor, "zed {path}");
        assert_eq!(c.command(Tool::Diff, &[]), "mine {left} {right}");
        assert_eq!(c.line_args["myed"], "--at={line} {path}");
        // Remove puts the default back.
        let out = write_edits(&out, &[Edit::Command(Tool::Editor, String::new()), Edit::RemoveUserTool("mine".into())]).unwrap();
        let (c, _) = Config::read(Some(&out));
        assert_eq!(c.command(Tool::Editor, &[]), default_command(Tool::Editor));
        assert!(c.diff_tools.is_empty());
    }

    #[test]
    fn the_card_draws_and_a_pass_without_input_changes_nothing() {
        let ctx = egui::Context::default();
        let c = cfg("[line_args]\nmyed = \"--at={line} {path}\"\n[diff_tools.mine]\ndiff = \"m {left} {right}\"\n");
        let (mut drafts, mut form) = (Drafts::new(), OpenForm::default());
        let mut got = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let l = Look { c: ito_theme::colors(), q: "" };
            got = Some(open_card(ui, l, &W, &c, None, &mut drafts, &mut form));
        });
        out.textures_delta.clear();
        assert_eq!(got, Some(vec![]));
    }
}
