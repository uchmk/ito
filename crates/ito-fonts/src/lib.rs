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
//! Only the Latin face is bundled. The system's Japanese and emoji faces are
//! found by [`find`] (on a worker thread) and put in both egui families by
//! `install` (feature `egui`): the Latin faces first, then the Japanese face,
//! then egui's `NotoEmoji` and `emoji-icon-font`, so full-width forms
//! (`（Ａ１）`) are not drawn from the icon font. In the monospace family
//! the Japanese face is scaled so a full-width character is two cells.

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

/// The system's emoji faces, in order (outlines in one colour, as egui's own
/// emoji draw; macOS has only a bitmap face, which egui cannot read).
#[cfg(windows)]
pub const EMOJI: &[&str] = &[r"C:\Windows\Fonts\seguiemj.ttf"];
#[cfg(target_os = "macos")]
pub const EMOJI: &[&str] = &[];
#[cfg(not(any(windows, target_os = "macos")))]
pub const EMOJI: &[&str] = &["/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf", "/usr/share/fonts/noto/NotoEmoji-Regular.ttf"];

/// egui's own faces that draw icons and emoji, as named in its default
/// families. `emoji-icon-font` also has glyphs for the full-width forms
/// U+FF01..U+FF5A, so these must come after the Japanese face.
pub const EGUI_ICON_FACES: [&str; 2] = ["NotoEmoji-Regular", "emoji-icon-font"];

/// The advance of [`JETBRAINS_MONO`] in em.
pub const LATIN_ADVANCE: f32 = 0.6;
/// The advance of a full-width glyph of the system's Japanese faces, in em.
pub const FULL_WIDTH_ADVANCE: f32 = 1.0;

/// The scale that makes a full-width character exactly two cells in the
/// bundled monospace face (1.2), which is what `ito-mdedit::format_table`
/// pads for. Applied to the Japanese face in the monospace family only.
pub fn two_column_scale() -> f32 {
    2.0 * LATIN_ADVANCE / FULL_WIDTH_ADVANCE
}

/// A font file read from disk.
#[derive(Clone, Debug)]
pub struct Face {
    /// Where it came from.
    pub path: std::path::PathBuf,
    /// The file (tens of megabytes for a `.ttc`).
    pub bytes: Vec<u8>,
}

/// The system faces to put behind the Latin ones. Reading them is slow, so
/// [`find`] runs on a worker thread and `install` on the UI thread. Any of
/// them may be missing; `install` then leaves that layer out.
#[derive(Clone, Debug, Default)]
pub struct Faces {
    /// The first of `ito_common::fonts::JAPANESE` that reads.
    pub japanese: Option<Face>,
    /// The first of [`EMOJI`] that reads.
    pub emoji: Option<Face>,
    /// A Nerd Font (its icons are what a prompt such as starship draws), put
    /// in front of the monospace family when set. [`find`] leaves it `None`;
    /// an app that wants it fills it from `ito_common::fonts::nerd_font()`.
    pub nerd: Option<Face>,
}

/// Read the Japanese and emoji faces from the system. Slow: not on the UI thread.
pub fn find() -> Faces {
    let read = |l: &[&str]| ito_common::fonts::read_first(l).map(|(path, bytes)| Face { path, bytes });
    Faces { japanese: read(ito_common::fonts::JAPANESE), emoji: read(EMOJI), nerd: None }
}

/// The new order of one family: `front` first, then the faces already there
/// (minus egui's icon faces and anything in `front` or `behind`), then
/// `behind` (the Japanese face, emoji), and last egui's icon faces, so they
/// only draw what nothing else has. Pure, so the order is testable.
pub fn order(current: &[String], front: &[String], behind: &[String]) -> Vec<String> {
    let icon = |n: &String| EGUI_ICON_FACES.contains(&n.as_str());
    let mut out: Vec<String> = front.to_vec();
    out.extend(current.iter().filter(|n| !icon(n) && !front.contains(n) && !behind.contains(n)).cloned());
    out.extend(behind.iter().filter(|n| !front.contains(n)).cloned());
    out.extend(current.iter().filter(|n| icon(n)).cloned());
    out
}

/// The keys the faces have in egui's `FontDefinitions::font_data`.
#[cfg(feature = "egui")]
pub mod key {
    /// The Japanese face in the proportional family.
    pub const JAPANESE: &str = "ito-japanese";
    /// The Japanese face in the monospace family (scaled to two cells).
    pub const JAPANESE_MONO: &str = "ito-japanese-mono";
    /// The emoji face.
    pub const EMOJI: &str = "ito-emoji";
    /// The Nerd Font.
    pub const NERD: &str = "ito-nerd";
}

/// Build both families: the faces already in `fonts` (the app's own, egui's
/// Ubuntu-Light / Hack), the bundled mono for `mono`, the Nerd Font, then
/// the Japanese face, the emoji face and finally egui's icon faces. Call it
/// last, after the app has added its own faces; calling it again replaces
/// what it added before. With `mono = Bundled` and no Nerd Font, the
/// Japanese face in the monospace family is scaled so a full-width
/// character is two cells wide ([`two_column_scale`]); with `System` or a
/// Nerd Font (a 1:2 font such as HackGen) nothing is scaled.
#[cfg(feature = "egui")]
pub fn install(fonts: &mut egui::FontDefinitions, faces: &Faces, mono: MonoFont) {
    use egui::{FontData, FontFamily, FontTweak};
    let mine = [KEY, key::JAPANESE, key::JAPANESE_MONO, key::EMOJI, key::NERD];
    for k in mine {
        fonts.font_data.remove(k);
    }
    for list in fonts.families.values_mut() {
        list.retain(|n| !mine.contains(&n.as_str()));
    }
    let put = |fonts: &mut egui::FontDefinitions, key: &str, f: &Face, tweak: FontTweak| {
        fonts.font_data.insert(key.to_owned(), FontData { tweak, ..FontData::from_owned(f.bytes.clone()) }.into());
    };
    let mut mono_front: Vec<String> = Vec::new();
    if let Some(n) = &faces.nerd {
        put(fonts, key::NERD, n, FontTweak::default());
        mono_front.push(key::NERD.to_owned());
    }
    if mono == MonoFont::Bundled {
        fonts.font_data.insert(KEY.to_owned(), egui::FontData::from_static(JETBRAINS_MONO).into());
        mono_front.push(KEY.to_owned());
    }
    let scaled = mono == MonoFont::Bundled && faces.nerd.is_none();
    let mut prop_behind = Vec::new();
    let mut mono_behind = Vec::new();
    if let Some(j) = &faces.japanese {
        put(fonts, key::JAPANESE, j, FontTweak::default());
        prop_behind.push(key::JAPANESE.to_owned());
        if scaled {
            put(fonts, key::JAPANESE_MONO, j, FontTweak { scale: two_column_scale(), ..FontTweak::default() });
            mono_behind.push(key::JAPANESE_MONO.to_owned());
        } else {
            mono_behind.push(key::JAPANESE.to_owned());
        }
    }
    if let Some(e) = &faces.emoji {
        put(fonts, key::EMOJI, e, FontTweak::default());
        prop_behind.push(key::EMOJI.to_owned());
        mono_behind.push(key::EMOJI.to_owned());
    }
    for (family, front, behind) in [(FontFamily::Proportional, Vec::new(), prop_behind), (FontFamily::Monospace, mono_front, mono_behind)] {
        let cur = fonts.families.get(&family).cloned().unwrap_or_default();
        fonts.families.insert(family, order(&cur, &front, &behind));
    }
}

/// [`find`] on a thread, then [`install`] into egui's default fonts and set
/// them. Until then a CJK character draws as a box for a frame or two. For
/// apps with no faces of their own; the others call `find` and `install`
/// themselves.
#[cfg(feature = "egui")]
pub fn load_in_background(ctx: egui::Context, mono: MonoFont) {
    let _ = std::thread::Builder::new().name("fonts".into()).spawn(move || {
        let faces = find();
        let mut fonts = egui::FontDefinitions::default();
        install(&mut fonts, &faces, mono);
        ctx.set_fonts(fonts);
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn japanese_comes_before_egui_icon_faces() {
        let cur = v(&["Hack", "Ubuntu-Light", "NotoEmoji-Regular", "emoji-icon-font"]);
        let got = order(&cur, &v(&["front"]), &v(&["jp", "emoji"]));
        assert_eq!(got, v(&["front", "Hack", "Ubuntu-Light", "jp", "emoji", "NotoEmoji-Regular", "emoji-icon-font"]));
    }

    #[test]
    fn no_japanese_face_keeps_the_order_with_icons_last() {
        let cur = v(&["NotoEmoji-Regular", "a", "emoji-icon-font", "b"]);
        let once = order(&cur, &[], &[]);
        assert_eq!(once, v(&["a", "b", "NotoEmoji-Regular", "emoji-icon-font"]));
        assert_eq!(order(&once, &[], &[]), once);
    }

    #[test]
    fn two_column_scale_is_1_2() {
        assert!((two_column_scale() - 1.2).abs() < 1e-6);
    }

    #[cfg(feature = "egui")]
    fn fake(bytes: &[u8]) -> Face {
        Face { path: "x".into(), bytes: bytes.to_vec() }
    }

    #[cfg(feature = "egui")]
    #[test]
    fn install_orders_both_families_and_is_idempotent() {
        use egui::FontFamily::{Monospace, Proportional};
        let faces = Faces { japanese: Some(fake(b"j")), emoji: Some(fake(b"e")), nerd: None };
        let mut f = egui::FontDefinitions::default();
        install(&mut f, &faces, MonoFont::Bundled);
        let once = f.families.clone();
        install(&mut f, &faces, MonoFont::Bundled);
        assert_eq!(f.families, once);
        let pos = |fam: &egui::FontFamily, n: &str| f.families[fam].iter().position(|x| x == n).unwrap();
        for fam in [&Proportional, &Monospace] {
            assert!(pos(fam, key::EMOJI) < pos(fam, "emoji-icon-font"));
        }
        assert!(pos(&Proportional, key::JAPANESE) < pos(&Proportional, "emoji-icon-font"));
        assert_eq!(pos(&Monospace, KEY), 0);
        assert!(pos(&Monospace, key::JAPANESE_MONO) < pos(&Monospace, "emoji-icon-font"));
        assert!(!f.families[&Monospace].contains(&key::JAPANESE.to_owned()));
        assert_eq!(f.font_data[key::JAPANESE_MONO].tweak.scale, two_column_scale());
        // System: no bundled face, no scaling, the Japanese face is the same entry.
        let mut s = egui::FontDefinitions::default();
        install(&mut s, &faces, MonoFont::System);
        assert!(!s.font_data.contains_key(KEY) && !s.font_data.contains_key(key::JAPANESE_MONO));
        assert!(s.families[&Monospace].contains(&key::JAPANESE.to_owned()));
        // Nothing found: only the bundled face is added.
        let mut n = egui::FontDefinitions::default();
        install(&mut n, &Faces::default(), MonoFont::Bundled);
        assert_eq!(n.families[&Monospace][0], KEY);
        assert_eq!(n.families[&Monospace].len(), egui::FontDefinitions::default().families[&Monospace].len() + 1);
    }

    /// With a real Japanese face on the machine: a full-width character is
    /// two cells in monospace, and `（` is drawn by it, not the icon font.
    #[cfg(feature = "egui")]
    #[test]
    fn full_width_is_two_cells_with_the_system_face() {
        let faces = find();
        if faces.japanese.is_none() {
            eprintln!("no Japanese face here; skipped");
            return;
        }
        let mut f = egui::FontDefinitions::default();
        install(&mut f, &faces, MonoFont::Bundled);
        let ctx = egui::Context::default();
        ctx.set_fonts(f);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        let id = egui::FontId::monospace(20.0);
        let width = |text: &str| ctx.fonts_mut(|fo| fo.layout_no_wrap(text.to_owned(), id.clone(), egui::Color32::WHITE).size().x);
        let (zero, kanji, paren) = (width("0"), width("日"), width("（"));
        assert!((kanji - 2.0 * zero).abs() < 0.5, "kanji {kanji} vs zero {zero}");
        assert!((paren - kanji).abs() < 0.5, "paren {paren} kanji {kanji}");
    }

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
