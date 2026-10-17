//! The monospace face every uchmk app carries inside it, so a path or a
//! line of code has the same shapes on every machine (JetBrains Mono, under
//! the SIL Open Font License; the licence and the authors are in `fonts/`
//! and [`LICENSE`]).
//!
//! Whether an app draws with it is common.toml's `mono_font`
//! ([`ito_common::MonoFont`]): `bundled` (the default) puts it first among
//! egui's monospace faces, `system` leaves the app's own search of the
//! system's faces (`ito_common::fonts`) as it was. An app's own
//! `font.family`, when it names one, still wins over both: that is the
//! user's choice for that app.
//!
//! Only the Latin face is here. Japanese, icons (Nerd Fonts) and emoji stay
//! with `ito_common::fonts` and the app, and come after this one in the
//! family, so what it lacks falls through to them.

pub use ito_common::MonoFont;

/// JetBrains Mono Regular (TrueType, about 270 KB).
pub const JETBRAINS_MONO: &[u8] = include_bytes!("../fonts/JetBrainsMono-Regular.ttf");

/// The family name the face is known by.
pub const NAME: &str = "JetBrains Mono";

/// The SIL Open Font License the face comes under; show it where the app
/// shows its other licences.
pub const LICENSE: &str = include_str!("../fonts/OFL.txt");

/// Who made the face.
pub const AUTHORS: &str = include_str!("../fonts/AUTHORS.txt");

/// The key the face has in egui's `FontDefinitions::font_data`.
#[cfg(feature = "egui")]
pub const KEY: &str = "ito-jetbrains-mono";

/// Hand the face to egui for `mono`: `Bundled` adds it and puts it first in
/// the monospace family (the faces already there stay after it, for what it
/// lacks); `System` leaves `fonts` as it is. Call it before the app adds its
/// own faces or after, as it likes; calling it twice is the same as once.
#[cfg(feature = "egui")]
pub fn register(fonts: &mut egui::FontDefinitions, mono: MonoFont) {
    if mono != MonoFont::Bundled {
        return;
    }
    fonts.font_data.insert(KEY.to_owned(), egui::FontData::from_static(JETBRAINS_MONO).into());
    let family = fonts.families.entry(egui::FontFamily::Monospace).or_default();
    family.retain(|n| n != KEY);
    family.insert(0, KEY.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A TrueType file starts with the 0x00010000 tag, and the licence is the OFL's.
    #[test]
    fn the_face_and_its_licence_are_inside() {
        assert_eq!(&JETBRAINS_MONO[..4], &[0, 1, 0, 0]);
        assert!(JETBRAINS_MONO.len() > 100_000);
        assert!(LICENSE.contains("SIL OPEN FONT LICENSE"));
        assert!(AUTHORS.contains("JetBrains"));
    }

    #[cfg(feature = "egui")]
    #[test]
    fn bundled_goes_first_and_system_leaves_the_fonts_alone() {
        let mut f = egui::FontDefinitions::default();
        let before = f.families[&egui::FontFamily::Monospace].clone();
        register(&mut f, MonoFont::System);
        assert_eq!(f.families[&egui::FontFamily::Monospace], before);
        assert!(!f.font_data.contains_key(KEY));
        register(&mut f, MonoFont::Bundled);
        register(&mut f, MonoFont::Bundled);
        let mono = &f.families[&egui::FontFamily::Monospace];
        assert_eq!(mono[0], KEY);
        assert_eq!(mono.len(), before.len() + 1);
        assert!(f.font_data.contains_key(KEY));
    }
}
