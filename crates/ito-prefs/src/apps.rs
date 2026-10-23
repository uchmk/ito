//! The "Apps" card: where the other uchmk apps are started from
//! (`ito_common::apps`). Every app that has run registered itself, so a row
//! shows where it is and which way that was found; a path typed into the row
//! pins it (`[apps]` in common.toml) -- for a second copy, a portable one, or
//! one that registered from the wrong place. The card returns [`Edit`]s; the
//! app writes them with `ito_common::edit_common(base, |t|
//! ito_common::apps::write_edits(t, &edits))`, off the UI thread.

use ito_common::apps::{valid_name, Edit, Row, Source};

use crate::{button, field, row, sep, status, Drafts, Look};

/// The card's words, in the app's language.
pub struct AppsWords<'a> {
    pub head: &'a str,
    /// The line while the rows are looked for, and when there are none.
    pub looking: &'a str,
    pub none: &'a str,
    /// Where an app was found: pinned here, registered by itself, on PATH.
    pub from_setting: &'a str,
    pub from_registered: &'a str,
    pub from_path: &'a str,
    /// An app that is nowhere, and a pinned path that is not a file.
    pub not_found: &'a str,
    pub bad_path: &'a str,
    /// The hint of a row's field when nothing is pinned.
    pub path_hint: &'a str,
    /// Takes the pin away.
    pub clear: &'a str,
    /// The form that pins an app that has not registered, and its labels.
    pub add_name: &'a str,
    pub add_path: &'a str,
    pub add: &'a str,
    pub bad_name: &'a str,
}

/// The "add" form's text while it is typed.
#[derive(Default)]
pub struct AppsForm {
    pub name: String,
    pub path: String,
    /// Why the last "add" was refused.
    pub error: String,
}

/// The note under an app: how it was found, where, and the version it registered.
pub fn note(w: &AppsWords, r: &Row) -> String {
    let mut s = match &r.found {
        None => w.not_found.to_owned(),
        Some(f) => {
            let how = match f.source {
                Source::Setting => w.from_setting,
                Source::Registered => w.from_registered,
                Source::Path => w.from_path,
            };
            format!("{how}: {}", f.exe.display())
        }
    };
    if let Some(v) = &r.version {
        s.push_str(&format!(" · {v}"));
    }
    if r.setting.is_some() && !r.setting_ok {
        s.push_str(&format!(" · {}", w.bad_path));
    }
    s
}

/// The pin a form makes, or why not.
pub fn add_edit(w: &AppsWords, f: &AppsForm) -> Result<Edit, String> {
    let name = f.name.trim();
    if !valid_name(name) {
        return Err(format!("{}: {name}", w.bad_name));
    }
    if f.path.trim().is_empty() {
        return Err(w.add_path.to_owned());
    }
    Ok(Edit { name: name.to_owned(), exe: Some(f.path.trim().to_owned()) })
}

/// The card. `rows` is `ito_common::apps::scan_in_background`'s answer once
/// it has come (`None` before).
pub fn apps_card(ui: &mut egui::Ui, l: Look, w: &AppsWords, rows: Option<&[Row]>, drafts: &mut Drafts, form: &mut AppsForm) -> Vec<Edit> {
    let mut edits = Vec::new();
    crate::section(ui, l, w.head, |ui| {
        match rows {
            None => status(ui, w.looking, l.c.dim),
            Some([]) => status(ui, w.none, l.c.dim),
            Some(rows) => {
                for (i, r) in rows.iter().enumerate() {
                    if i > 0 {
                        sep(ui, l);
                    }
                    let now = r.setting.as_deref().unwrap_or("");
                    let hint = r.found.as_ref().map_or(w.path_hint.to_owned(), |f| f.exe.display().to_string());
                    let got = row(ui, l, &r.name, &note(w, r), |ui| {
                        if r.setting.is_some() && button(ui, l, w.clear) {
                            edits.push(Edit { name: r.name.clone(), exe: None });
                        }
                        field(ui, drafts, &format!("app-{}", r.name), now, &hint, 330.0)
                    });
                    if let Some(v) = got {
                        edits.push(Edit { name: r.name.clone(), exe: Some(v).filter(|v| !v.trim().is_empty()) });
                    }
                }
            }
        }
        sep(ui, l);
        row(ui, l, w.add_name, "", |ui| {
            if button(ui, l, w.add) {
                match add_edit(w, form) {
                    Ok(e) => {
                        *form = AppsForm::default();
                        edits.push(e);
                    }
                    Err(e) => form.error = e,
                }
            }
            ui.add(egui::TextEdit::singleline(&mut form.path).hint_text(w.add_path).desired_width(250.0));
            ui.add(egui::TextEdit::singleline(&mut form.name).hint_text(w.add_name).desired_width(110.0));
        });
        if !form.error.is_empty() {
            status(ui, &form.error, l.c.err);
        }
        ui.add_space(8.0);
    });
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use ito_common::apps::Found;
    use std::path::PathBuf;

    const W: AppsWords = AppsWords {
        head: "APPS",
        looking: "looking…",
        none: "none yet",
        from_setting: "pinned",
        from_registered: "registered",
        from_path: "PATH",
        not_found: "not found",
        bad_path: "pinned path is not a file",
        path_hint: "path to the exe",
        clear: "Clear",
        add_name: "App",
        add_path: "Path",
        add: "Add",
        bad_name: "not an app name",
    };

    fn row_of(found: Option<Source>, setting: Option<&str>, ok: bool) -> Row {
        Row {
            name: "kura".into(),
            setting: setting.map(str::to_owned),
            setting_ok: ok,
            found: found.map(|source| Found { exe: PathBuf::from("/x/kura"), source }),
            version: Some("1.0.0".into()),
        }
    }

    #[test]
    fn the_note_says_how_where_and_what_is_wrong() {
        assert_eq!(note(&W, &row_of(Some(Source::Registered), None, false)), "registered: /x/kura · 1.0.0");
        assert_eq!(note(&W, &row_of(None, Some("/gone"), false)), "not found · 1.0.0 · pinned path is not a file");
    }

    #[test]
    fn the_form_makes_a_pin_or_says_why_not() {
        let f = |n: &str, p: &str| AppsForm { name: n.into(), path: p.into(), error: String::new() };
        assert_eq!(add_edit(&W, &f(" kura ", " /x/kura ")), Ok(Edit { name: "kura".into(), exe: Some("/x/kura".into()) }));
        assert!(add_edit(&W, &f("Kura", "/x")).is_err());
        assert!(add_edit(&W, &f("kura", " ")).is_err());
    }

    #[test]
    fn the_card_draws_and_a_pass_without_input_changes_nothing() {
        let ctx = egui::Context::default();
        let rows = [row_of(Some(Source::Path), None, false), row_of(None, Some("/gone"), false)];
        for shown in [None, Some(&[][..]), Some(&rows[..])] {
            let (mut drafts, mut form) = (Drafts::new(), AppsForm::default());
            let mut got = None;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let l = Look { c: ito_theme::colors(), q: "" };
                got = Some(apps_card(ui, l, &W, shown, &mut drafts, &mut form));
            });
            out.textures_delta.clear();
            assert_eq!(got, Some(vec![]));
        }
    }
}
