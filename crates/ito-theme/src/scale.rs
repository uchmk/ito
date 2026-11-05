//! The size scale every uchmk screen is drawn with: letter sizes by *role*,
//! corner radii and gaps by step. Ask for `Text::ChromeBody`, not `13.0`, so
//! one change here moves every app together.
//!
//! Sizes are in points before the text zoom. [`Text::at`] applies the zoom of
//! [`crate::set_text_zoom`] (the status bar's scale), so a screen of the
//! window's own letters grows with the panes. Radii, gaps and heights do not
//! zoom: egui's `set_zoom_factor` already carries those.

/// A letter size, named by what it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Text {
    /// The text of an editor (`editor.text`).
    EditorText,
    /// Line numbers beside it (`editor.gutter`): a step below the text.
    EditorGutter,
    /// Running text of the chrome: lists, fields, labels (`chrome.body`).
    ChromeBody,
    /// Button labels (`chrome.button`).
    ChromeButton,
    /// Counts, captions, key hints in a row (`chrome.small`).
    ChromeSmall,
    /// A dialog's heading (`dialog.title`).
    DialogTitle,
    /// A key cap in a dialog; monospace (`dialog.key`).
    DialogKey,
    /// The line of explanation under a dialog's heading (`dialog.hint`).
    DialogHint,
    /// A page's heading, such as a settings page (`page.title`).
    PageTitle,
}

impl Text {
    /// Every role, in the order of the table in `docs/design-tokens.md`.
    pub const ALL: [Text; 9] = [
        Text::EditorText,
        Text::EditorGutter,
        Text::ChromeBody,
        Text::ChromeButton,
        Text::ChromeSmall,
        Text::DialogTitle,
        Text::DialogKey,
        Text::DialogHint,
        Text::PageTitle,
    ];

    /// The size in points at zoom 1.0.
    pub const fn size(self) -> f32 {
        match self {
            Text::EditorText => 14.0,
            Text::EditorGutter => 12.5,
            Text::ChromeBody => 13.0,
            Text::ChromeButton => 12.0,
            Text::ChromeSmall => 11.0,
            Text::DialogTitle => 16.0,
            Text::DialogKey => 12.0,
            Text::DialogHint => 11.5,
            Text::PageTitle => 18.0,
        }
    }

    /// The size at the letters' zoom ([`crate::zoomed`]): what goes into
    /// `RichText::size` or a `FontId` on a screen of the window's own.
    pub fn at(self, ctx: &egui::Context) -> f32 {
        crate::zoomed(ctx, self.size())
    }

    /// The role's name as the docs write it (`chrome.body`).
    pub const fn name(self) -> &'static str {
        match self {
            Text::EditorText => "editor.text",
            Text::EditorGutter => "editor.gutter",
            Text::ChromeBody => "chrome.body",
            Text::ChromeButton => "chrome.button",
            Text::ChromeSmall => "chrome.small",
            Text::DialogTitle => "dialog.title",
            Text::DialogKey => "dialog.key",
            Text::DialogHint => "dialog.hint",
            Text::PageTitle => "page.title",
        }
    }
}

/// Corner radii, in points.
pub mod radius {
    /// Every box: dialogs, cards, menus, popups. One value.
    pub const BOX: u8 = 8;
    /// Controls inside a box: buttons, fields, chips.
    pub const CONTROL: u8 = 6;
    /// Small marks: key caps, count pills' squares, badges.
    pub const MARK: u8 = 4;
}

/// Gaps and padding, in points. A 2-based ladder; pick the step, not a number.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 10.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Heights of repeated things, in points.
pub mod height {
    /// A button, a field, a toolbar item.
    pub const CONTROL: f32 = 28.0;
    /// A row of a list.
    pub const ROW: f32 = 24.0;
}

const _: () = {
    assert!(radius::MARK < radius::CONTROL && radius::CONTROL < radius::BOX);
    assert!(space::XS < space::SM && space::SM < space::MD && space::MD < space::LG && space::LG < space::XL);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_keep_their_order() {
        let s = |t: Text| t.size();
        assert!(s(Text::EditorGutter) < s(Text::EditorText), "the gutter sits below the text");
        assert!(s(Text::ChromeSmall) < s(Text::ChromeButton) && s(Text::ChromeButton) < s(Text::ChromeBody));
        assert!(s(Text::DialogHint) < s(Text::DialogTitle) && s(Text::DialogTitle) < s(Text::PageTitle));
        assert!(Text::ALL.iter().all(|t| t.size() >= 11.0), "nothing is smaller than the smallest role");
    }

    #[test]
    fn names_are_unique_and_dotted() {
        let mut names: Vec<_> = Text::ALL.iter().map(|t| t.name()).collect();
        assert!(names.iter().all(|n| n.contains('.')));
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Text::ALL.len());
    }

    #[test]
    fn the_zoom_applies_to_text() {
        let ctx = egui::Context::default();
        assert_eq!(Text::ChromeBody.at(&ctx), 13.0);
        crate::set_text_zoom(&ctx, 1.5);
        assert_eq!(Text::ChromeBody.at(&ctx), 19.5);
    }
}
