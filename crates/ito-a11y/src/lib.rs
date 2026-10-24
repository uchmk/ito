//! Accessibility the apps share.
//!
//! - **Not by colour alone.** A state that is shown only by a colour (a
//!   selected row, the focused pane, a conflicting file) is lost on a
//!   colour-blind eye or a grey screen. [`State`] names the states every app
//!   has and gives each a [`mark`](State::mark) (a glyph put beside the text)
//!   and a [`name`](State::name) (what a screen reader says, English and
//!   Japanese). The marks are always safe to draw; with `high_contrast = true`
//!   in common.toml (`ito_common::Common::high_contrast_or_off`) the app draws
//!   them on every row, otherwise only where colour is not enough.
//! - **Colours** of the high-contrast variant: `ito_theme::Colors::high_contrast`.
//! - **A screen reader reads what egui paints** only when the painted text is
//!   given a label. With the `egui` feature, [`say`] gives a response that
//!   label and a role, and [`announce`] tells the reader about a change that
//!   has no widget (a toast, a finished job).

/// A state an app shows on a row, a pane, a tab or a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Selected,
    Focused,
    Conflict,
    Error,
    Warning,
    Done,
    Running,
    Disabled,
}

impl State {
    /// The glyph for the state, drawn before the text. Single-width, in the
    /// bundled mono font and in common system fonts.
    pub fn mark(self) -> &'static str {
        match self {
            State::Selected => "●",
            State::Focused => "▶",
            State::Conflict => "≠",
            State::Error => "✖",
            State::Warning => "▲",
            State::Done => "✔",
            State::Running => "…",
            State::Disabled => "–",
        }
    }

    /// What a screen reader says (`ja`, else English).
    pub fn name(self, lang: &str) -> &'static str {
        let ja = lang == "ja";
        match (self, ja) {
            (State::Selected, false) => "selected",
            (State::Selected, true) => "選択中",
            (State::Focused, false) => "focused",
            (State::Focused, true) => "フォーカス中",
            (State::Conflict, false) => "conflict",
            (State::Conflict, true) => "競合",
            (State::Error, false) => "error",
            (State::Error, true) => "エラー",
            (State::Warning, false) => "warning",
            (State::Warning, true) => "注意",
            (State::Done, false) => "done",
            (State::Done, true) => "完了",
            (State::Running, false) => "running",
            (State::Running, true) => "実行中",
            (State::Disabled, false) => "unavailable",
            (State::Disabled, true) => "使えません",
        }
    }

    /// `text` with the mark before it (`● main.rs`).
    pub fn marked(self, text: &str) -> String {
        format!("{} {text}", self.mark())
    }

    /// `text` as a screen reader says it: the text, then the states
    /// (`main.rs, selected, conflict`).
    pub fn spoken(text: &str, states: &[State], lang: &str) -> String {
        let mut s = text.to_owned();
        for st in states {
            s.push_str(", ");
            s.push_str(st.name(lang));
        }
        s
    }
}

/// What a painted thing is, for the screen reader.
#[cfg(feature = "egui")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Label,
    Button,
    /// A row of a list the user moves through.
    Row,
    Heading,
}

/// Give `resp` (the response of an area the app painted or interacted with)
/// a screen-reader `label` and `role`. Call it for text drawn by a painter,
/// which egui cannot name by itself.
#[cfg(feature = "egui")]
pub fn say(resp: &egui::Response, role: Role, label: &str) {
    use egui::{WidgetInfo, WidgetType};
    let kind = match role {
        Role::Label | Role::Heading => WidgetType::Label,
        Role::Button => WidgetType::Button,
        Role::Row => WidgetType::SelectableLabel,
    };
    let enabled = resp.enabled();
    resp.widget_info(|| WidgetInfo::labeled(kind, enabled, label));
}

/// Tell the screen reader about a change that has no widget (a toast, a job
/// finished): a hidden label for one frame, read as a live message. The
/// reader speaks it when it appears; call it once per message.
#[cfg(feature = "egui")]
pub fn announce(ui: &mut egui::Ui, message: &str) {
    let resp = ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
    say(&resp, Role::Label, message);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [State; 8] =
        [State::Selected, State::Focused, State::Conflict, State::Error, State::Warning, State::Done, State::Running, State::Disabled];

    #[test]
    fn every_state_has_its_own_mark_and_its_names() {
        let mut marks: Vec<_> = ALL.iter().map(|s| s.mark()).collect();
        marks.sort_unstable();
        marks.dedup();
        assert_eq!(marks.len(), ALL.len(), "two states share a mark");
        for s in ALL {
            assert!(!s.name("en").is_empty() && !s.name("ja").is_empty());
            assert_ne!(s.name("en"), s.name("ja"));
            assert_eq!(s.name("fr"), s.name("en"));
        }
    }

    #[test]
    fn text_is_marked_and_spoken() {
        assert_eq!(State::Selected.marked("main.rs"), "● main.rs");
        assert_eq!(State::spoken("main.rs", &[State::Selected, State::Conflict], "en"), "main.rs, selected, conflict");
        assert_eq!(State::spoken("main.rs", &[State::Error], "ja"), "main.rs, エラー");
        assert_eq!(State::spoken("x", &[], "en"), "x");
    }

    #[cfg(feature = "egui")]
    #[test]
    fn say_and_announce_run_in_a_frame() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let r = ui.label("row");
            say(&r, Role::Row, "row, selected");
            say(&r, Role::Button, "ok");
            announce(ui, "saved");
        })
        .drop_without_applying_deltas();
    }
}
