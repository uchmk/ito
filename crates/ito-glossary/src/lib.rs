//! Technical words made easy: one table of the words a beginner stumbles on,
//! with a plain wording for each ("easy display") and a one-or-two-line
//! explanation for the hover. English and Japanese.
//!
//! The words are the app's (Git's for michi, a terminal's for tsumugi): each
//! app writes its own `const TERMS: &[Term]` and wraps it in a [`Glossary`].
//! What is shared is the shape, the labels, the check of a table ([`check`],
//! for the app's own test), the switch (`easy = true` in common.toml, read as
//! `ito_common::Common::easy_or_off`, so the choice is made once for all the
//! apps) and, with the `egui` feature, the dotted underline and the hover.

/// One term: `id` is how the screen asks for it, `word` the technical word.
#[derive(Clone, Copy, Debug)]
pub struct Term {
    pub id: &'static str,
    pub word: &'static str,
    pub plain_en: &'static str,
    pub plain_ja: &'static str,
    pub tip_en: &'static str,
    pub tip_ja: &'static str,
}

fn ja(lang: &str) -> bool {
    lang == "ja"
}

impl Term {
    /// The plain wording in `lang` (`ja`, else English).
    pub fn plain(&self, lang: &str) -> &'static str {
        if ja(lang) {
            self.plain_ja
        } else {
            self.plain_en
        }
    }

    /// The hover text in `lang`.
    pub fn tip(&self, lang: &str) -> &'static str {
        if ja(lang) {
            self.tip_ja
        } else {
            self.tip_en
        }
    }

    /// The easy display's label: the plain wording with the technical word
    /// beside it, `set aside for later (stash)`.
    pub fn easy(&self, lang: &str) -> String {
        if ja(lang) {
            format!("{}（{}）", self.plain_ja, self.word)
        } else {
            format!("{} ({})", self.plain_en, self.word)
        }
    }
}

/// An app's table.
#[derive(Clone, Copy, Debug)]
pub struct Glossary(pub &'static [Term]);

impl Glossary {
    /// The term with `id`.
    pub fn find(&self, id: &str) -> Option<&'static Term> {
        self.0.iter().find(|t| t.id == id)
    }

    /// The label for `id`: the easy wording when `easy`, else `normal` (the
    /// screen's usual word). `normal` also stands in for an unknown `id`.
    pub fn label(&self, id: &str, lang: &str, easy: bool, normal: &str) -> String {
        match self.find(id) {
            Some(t) if easy => t.easy(lang),
            _ => normal.to_owned(),
        }
    }
}

/// What is wrong with a table, for the app's own test (`assert_eq!(check(TERMS), Vec::<String>::new())`):
/// an id twice, an empty text, a plain wording that is the word itself,
/// Japanese that is not Japanese (or English that is), a hover text too long
/// to read at a glance (170 characters in English, 90 in Japanese).
pub fn check(terms: &[Term]) -> Vec<String> {
    let has_ja = |s: &str| s.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{4e00}'..='\u{9fff}').contains(&c));
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in terms {
        if !seen.insert(t.id) {
            problems.push(format!("`{}` twice", t.id));
        }
        for s in [t.word, t.plain_en, t.plain_ja, t.tip_en, t.tip_ja] {
            if s.trim().is_empty() {
                problems.push(format!("`{}` has an empty text", t.id));
                break;
            }
        }
        if t.plain_en.to_lowercase() == t.word.to_lowercase() {
            problems.push(format!("`{}` is not reworded", t.id));
        }
        if !(has_ja(t.plain_ja) && has_ja(t.tip_ja)) {
            problems.push(format!("`{}`: the Japanese has no Japanese in it", t.id));
        }
        if has_ja(t.plain_en) || has_ja(t.tip_en) {
            problems.push(format!("`{}`: the English has Japanese in it", t.id));
        }
        if t.tip_en.chars().count() > 170 {
            problems.push(format!("`{}`: the English hover text is long", t.id));
        }
        if t.tip_ja.chars().count() > 90 {
            problems.push(format!("`{}`: the Japanese hover text is long", t.id));
        }
    }
    problems
}

/// Underline `rect` (a term just drawn with `painter`) with dots and explain
/// the term `id` of `glossary` when the pointer rests on it. `salt` tells
/// apart the same term drawn twice. Nothing happens for an unknown `id`.
#[cfg(feature = "egui")]
pub fn hover(
    ui: &egui::Ui,
    painter: &egui::Painter,
    rect: egui::Rect,
    glossary: &Glossary,
    id: &'static str,
    lang: &str,
    salt: &'static str,
) {
    let Some(term) = glossary.find(id) else { return };
    let c = ito_theme::colors();
    let y = rect.bottom() + 1.0;
    let mut x = rect.left();
    while x < rect.right() {
        painter.circle_filled(egui::pos2(x + 0.5, y), 0.6, c.dim);
        x += 3.0;
    }
    let resp = ui.interact(rect.expand2(egui::vec2(0.0, 2.0)), egui::Id::new(("ito-glossary", id, salt)), egui::Sense::hover());
    resp.on_hover_ui(|ui| {
        ui.set_max_width(280.0);
        ui.label(egui::RichText::new(format!("{} — {}", term.word, term.plain(lang))).strong());
        ui.label(term.tip(lang));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TERMS: &[Term] = &[
        Term {
            id: "stash",
            word: "stash",
            plain_en: "set aside for later",
            plain_ja: "あとで使うために置いておく",
            tip_en: "Put the current changes on a shelf and get a clean worktree.",
            tip_ja: "今の変更をいったん棚に置きます。",
        },
        Term {
            id: "push",
            word: "push",
            plain_en: "send commits to the server",
            plain_ja: "コミットをサーバーに送る",
            tip_en: "Upload your commits.",
            tip_ja: "自分のコミットをサーバーに送ります。",
        },
    ];

    #[test]
    fn a_good_table_has_no_problems() {
        assert_eq!(check(TERMS), Vec::<String>::new());
    }

    #[test]
    fn a_bad_table_says_what_is_wrong() {
        let mut bad = TERMS.to_vec();
        bad.push(Term { id: "stash", word: "x", plain_en: "x", plain_ja: "english", tip_en: "", tip_ja: "かな" });
        let p = check(&bad).join("\n");
        assert!(p.contains("`stash` twice"), "{p}");
        assert!(p.contains("empty text"), "{p}");
        assert!(p.contains("not reworded"), "{p}");
        assert!(p.contains("no Japanese"), "{p}");
    }

    #[test]
    fn the_easy_label_keeps_the_word_in_brackets() {
        let g = Glossary(TERMS);
        let t = g.find("stash").unwrap();
        assert_eq!(t.easy("en"), "set aside for later (stash)");
        assert_eq!(t.easy("ja"), "あとで使うために置いておく（stash）");
        assert_eq!(t.easy("fr"), t.easy("en"));
        assert_eq!(t.plain("ja"), "あとで使うために置いておく");
        assert!(t.tip("en").starts_with("Put"));
    }

    #[test]
    fn label_is_the_usual_word_unless_easy_and_known() {
        let g = Glossary(TERMS);
        assert_eq!(g.label("stash", "en", false, "Stash"), "Stash");
        assert_eq!(g.label("stash", "en", true, "Stash"), "set aside for later (stash)");
        assert_eq!(g.label("nope", "en", true, "Whatever"), "Whatever");
        assert!(g.find("nope").is_none());
    }

    #[cfg(feature = "egui")]
    #[test]
    fn the_hover_draws_for_a_known_term_and_not_for_an_unknown_one() {
        let ctx = egui::Context::default();
        let g = Glossary(TERMS);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let r = ui.label("stash").rect;
            hover(ui, ui.painter(), r, &g, "stash", "en", "a");
            hover(ui, ui.painter(), r, &g, "nope", "en", "a");
        })
        .drop_without_applying_deltas();
    }
}
