//! The Updates card: the switch for checking GitHub for a newer release, and
//! under it what the last check found. The switch's setting is the app's own
//! (`check_updates` in its settings file); the check is `ito_common::update`.

use ito_common::update::Update;

use crate::{row, section, switch, z, Look};

/// The card's words, in the app's language.
pub struct UpdateWords<'a> {
    /// The card's heading.
    pub head: &'a str,
    /// The switch's label and its note.
    pub label: &'a str,
    pub note: &'a str,
    /// A newer release exists; `{}` is its version.
    pub newer: &'a str,
    /// Ours is the latest.
    pub current: &'a str,
    /// The check found nothing out (offline, no `curl`).
    pub unknown: &'a str,
    /// Shown while the check is running, or before one has.
    pub checking: &'a str,
}

/// What the status line says for `found`; `None` is a check not answered yet.
/// Empty when the check is off, so nothing claims a version it did not look up.
pub fn status(w: &UpdateWords, on: bool, found: Option<&Update>) -> String {
    match (on, found) {
        (false, _) => String::new(),
        (true, None) => w.checking.to_owned(),
        (true, Some(Update::Newer(v))) => w.newer.replace("{}", v),
        (true, Some(Update::Current)) => w.current.to_owned(),
        (true, Some(Update::Unknown)) => w.unknown.to_owned(),
    }
}

/// The card. Returns the new state of the switch when it was clicked; the app
/// writes it to its file and starts or drops the check.
pub fn update_card(ui: &mut egui::Ui, l: Look, w: &UpdateWords, on: bool, found: Option<&Update>) -> Option<bool> {
    let mut toggled = None;
    section(ui, l, w.head, |ui| {
        if row(ui, l, w.label, w.note, |ui| switch(ui, l, on)) {
            toggled = Some(!on);
        }
        let line = status(w, on, found);
        if !line.is_empty() {
            let colour = if matches!(found, Some(Update::Newer(_))) { l.c.wait } else { l.c.dim };
            ui.add_space(4.0);
            ui.label(egui::RichText::new(line).size(z(ui, 12.0)).color(colour));
            ui.add_space(8.0);
        }
    });
    toggled
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: UpdateWords = UpdateWords {
        head: "UPDATES",
        label: "Check",
        note: "",
        newer: "{} is out",
        current: "up to date",
        unknown: "could not check",
        checking: "checking…",
    };

    #[test]
    fn the_status_says_only_what_was_found() {
        assert_eq!(status(&W, false, Some(&Update::Newer("1.2.3".into()))), "");
        assert_eq!(status(&W, true, None), "checking…");
        assert_eq!(status(&W, true, Some(&Update::Newer("1.2.3".into()))), "1.2.3 is out");
        assert_eq!(status(&W, true, Some(&Update::Current)), "up to date");
        assert_eq!(status(&W, true, Some(&Update::Unknown)), "could not check");
    }
}
