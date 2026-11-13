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
//! (`（Ａ１）`) are not drawn from the icon font. The Japanese face has its own
//! entry in each family, moved onto the baseline of the family's first face
//! (egui centres a fallback face's line box on the first face's, which draws
//! faces with a tall line box high); in the monospace family with the bundled
//! mono and no Nerd Font it is scaled so a full-width character is two cells.

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
    /// The Japanese face in the monospace family: with the bundled mono and no
    /// Nerd Font it is scaled to two cells ([`two_column_scale`](crate::two_column_scale)),
    /// otherwise it is unscaled. Either way it sits on the baseline of the
    /// family's first face.
    pub const JAPANESE_MONO: &str = "ito-japanese-mono";
    /// The emoji face.
    pub const EMOJI: &str = "ito-emoji";
    /// The Nerd Font.
    pub const NERD: &str = "ito-nerd";
}

/// A face's vertical metrics in em, as egui reads them (hhea, or OS/2 typo when the face sets USE_TYPO_METRICS):
/// `ascent` above the baseline (positive), `descent` below it (negative), and the line gap.
#[cfg_attr(not(feature = "egui"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct VMetrics {
    ascent: f32,
    descent: f32,
    line_gap: f32,
}

#[cfg_attr(not(feature = "egui"), allow(dead_code))]
impl VMetrics {
    /// `ascent - descent + line_gap`: the face's line box, as egui counts it.
    fn height(&self) -> f32 {
        self.ascent - self.descent + self.line_gap
    }

    /// Read them the way epaint does (skrifa, default location). `None` when the file cannot be read.
    #[cfg(feature = "egui")]
    fn read(bytes: &[u8], index: u32) -> Option<VMetrics> {
        use skrifa::MetadataProvider as _;
        let font = skrifa::FontRef::from_index(bytes, index).ok()?;
        let m = font.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
        if m.units_per_em == 0 {
            return None;
        }
        let u = f32::from(m.units_per_em);
        Some(VMetrics { ascent: m.ascent / u, descent: m.descent / u, line_gap: m.leading / u })
    }
}

/// Where the first face of a family sits: its metrics and its `FontTweak`'s `scale`, `y_offset_factor`, `y_offset`.
#[cfg_attr(not(feature = "egui"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    metrics: VMetrics,
    scale: f32,
    y_offset_factor: f32,
    y_offset: f32,
}

/// The distance in em from the first face's baseline to the baseline of a fallback face drawn at `scale`
/// (negative: the fallback is higher), before any tweak.
#[cfg_attr(not(feature = "egui"), allow(dead_code))]
fn baseline_gap(first: Placement, fallback: VMetrics, scale: f32) -> f32 {
    let (s_l, s_j) = (first.scale, scale);
    fallback.ascent * s_j + 0.5 * (first.metrics.height() * s_l - fallback.height() * s_j) - first.metrics.ascent * s_l
}

/// The `(y_offset_factor, y_offset)` for a fallback face drawn at `scale` so its baseline is on the baseline of the
/// family's first face `first`. egui centres a fallback face's line box on the first face's, so without this a face
/// with a tall line box, such as Yu Gothic's line gap of half an em, draws high. Non-finite or non-positive scales
/// give `(0.0, 0.0)`.
#[cfg_attr(not(feature = "egui"), allow(dead_code))]
fn baseline_offset(first: Placement, fallback: VMetrics, scale: f32) -> (f32, f32) {
    if !scale.is_finite() || scale <= 0.0 || !first.scale.is_finite() || first.scale <= 0.0 {
        return (0.0, 0.0);
    }
    let factor = (first.scale * first.y_offset_factor - baseline_gap(first, fallback, scale)) / scale;
    if factor.is_finite() {
        (factor, first.y_offset)
    } else {
        (0.0, 0.0)
    }
}

/// One copy of each distinct face file per process, so two entries of the same face (and egui's own blob) share it.
#[cfg(feature = "egui")]
fn kept(bytes: &[u8]) -> &'static [u8] {
    use std::sync::{Mutex, OnceLock};
    static KEPT: OnceLock<Mutex<Vec<&'static [u8]>>> = OnceLock::new();
    let mut list = KEPT.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(k) = list.iter().find(|k| k.len() == bytes.len() && ***k == *bytes) {
        return k;
    }
    let leaked: &'static [u8] = Box::leak(bytes.to_vec().into_boxed_slice());
    list.push(leaked);
    leaked
}

/// Build both families: the faces already in `fonts` (the app's own, egui's
/// Ubuntu-Light / Hack), the bundled mono for `mono`, the Nerd Font, then
/// the Japanese face, the emoji face and finally egui's icon faces. Call it
/// last, after the app has added its own faces; calling it again replaces
/// what it added before.
///
/// The Japanese face is its own entry in each family (`key::JAPANESE` in the
/// proportional one, `key::JAPANESE_MONO` in the monospace one), sharing one
/// copy of the bytes, and each is moved so its baseline is on the baseline of
/// the family's first face (egui would otherwise centre the Japanese line box
/// on the Latin one, which draws Yu Gothic about 0.28 em high). With
/// `mono = Bundled` and no Nerd Font, the monospace entry is scaled so a
/// full-width character is two cells wide ([`two_column_scale`]); with
/// `System` or a Nerd Font (a 1:2 font such as HackGen) nothing is scaled.
///
/// Each distinct face file is copied once and kept for the life of the
/// process (a changed file is new content and is kept again). That is fine for
/// the few system faces an app loads, not for loading arbitrary fonts over and
/// over.
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
        fonts.font_data.insert(key.to_owned(), FontData { tweak, ..FontData::from_static(kept(&f.bytes)) }.into());
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
    if let Some(e) = &faces.emoji {
        put(fonts, key::EMOJI, e, FontTweak::default());
        prop_behind.push(key::EMOJI.to_owned());
        mono_behind.push(key::EMOJI.to_owned());
    }
    if let Some(j) = &faces.japanese {
        // The first face of a family decides where the Japanese face has to sit, so the tweaks come
        // after the orders. The Japanese entries go in `behind` ahead of the emoji face.
        prop_behind.insert(0, key::JAPANESE.to_owned());
        mono_behind.insert(0, key::JAPANESE_MONO.to_owned());
        let current = |fonts: &egui::FontDefinitions, family: &FontFamily| fonts.families.get(family).cloned().unwrap_or_default();
        let prop_order = order(&current(fonts, &FontFamily::Proportional), &[], &prop_behind);
        let mono_order = order(&current(fonts, &FontFamily::Monospace), &mono_front, &mono_behind);
        let jm = VMetrics::read(&j.bytes, 0);
        let tweak = |fonts: &egui::FontDefinitions, list: &[String], scale: f32| {
            let first = list.first().and_then(|k| fonts.font_data.get(k)).and_then(|d| {
                Some(Placement {
                    metrics: VMetrics::read(&d.font, d.index)?,
                    scale: d.tweak.scale,
                    y_offset_factor: d.tweak.y_offset_factor,
                    y_offset: d.tweak.y_offset,
                })
            });
            let (y_offset_factor, y_offset) = match (first, jm) {
                (Some(f), Some(jm)) => baseline_offset(f, jm, scale),
                _ => (0.0, 0.0),
            };
            FontTweak { scale, y_offset_factor, y_offset, ..FontTweak::default() }
        };
        let prop_tweak = tweak(fonts, &prop_order, 1.0);
        let mono_tweak = tweak(fonts, &mono_order, if scaled { two_column_scale() } else { 1.0 });
        put(fonts, key::JAPANESE, j, prop_tweak);
        put(fonts, key::JAPANESE_MONO, j, mono_tweak);
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
        // Fake bytes have no metrics: no offsets.
        assert_eq!((f.font_data[key::JAPANESE].tweak.y_offset_factor, f.font_data[key::JAPANESE_MONO].tweak.y_offset_factor), (0.0, 0.0));
        // System: no bundled face, no scaling, the monospace family still has its own entry.
        let mut s = egui::FontDefinitions::default();
        install(&mut s, &faces, MonoFont::System);
        assert!(!s.font_data.contains_key(KEY));
        assert!(s.families[&Monospace].contains(&key::JAPANESE_MONO.to_owned()));
        assert!(!s.families[&Monospace].contains(&key::JAPANESE.to_owned()));
        assert_eq!(s.font_data[key::JAPANESE_MONO].tweak.scale, 1.0);
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

    fn vm(ascent: f32, descent: f32, line_gap: f32) -> VMetrics {
        VMetrics { ascent, descent, line_gap }
    }

    fn at(metrics: VMetrics) -> Placement {
        Placement { metrics, scale: 1.0, y_offset_factor: 0.0, y_offset: 0.0 }
    }

    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 0.002, "{a} vs {b}");
    }

    #[test]
    fn baseline_offset_follows_the_measured_table() {
        let ubuntu = at(vm(0.932, -0.189, 0.028));
        // Yu Gothic: hhea 1802/-455/1024 of 2048.
        let yu = vm(1802.0 / 2048.0, -455.0 / 2048.0, 1024.0 / 2048.0);
        near(baseline_offset(ubuntu, yu, 1.0).0, 0.279);
        // Meiryo: 2171/-901/0 of 2048.
        let meiryo = vm(2171.0 / 2048.0, -901.0 / 2048.0, 0.0);
        near(baseline_offset(ubuntu, meiryo, 1.0).0, 0.047);
        // JetBrains Mono first, Meiryo at 1.2.
        let jb = Placement { metrics: vm(1.02, -0.30, 0.0), scale: 1.0, y_offset_factor: 0.0, y_offset: 0.0 };
        near(baseline_offset(jb, meiryo, 1.2).0, -0.010);
        // The same metrics at the same scale sit already.
        assert_eq!(baseline_offset(ubuntu, ubuntu.metrics, 1.0), (0.0, 0.0));
    }

    #[test]
    fn baseline_offset_carries_the_first_faces_own_tweak() {
        let m = vm(0.932, -0.189, 0.028);
        let plain = baseline_offset(at(m), m, 1.2);
        let first = Placement { metrics: m, scale: 0.9, y_offset_factor: 0.1, y_offset: 2.0 };
        let moved = baseline_offset(first, m, 1.2);
        let base = baseline_offset(Placement { y_offset_factor: 0.0, y_offset: 0.0, ..first }, m, 1.2);
        near(moved.0 - base.0, 0.1 * 0.9 / 1.2);
        assert_eq!(moved.1, 2.0);
        assert!(plain.0.is_finite());
    }

    #[test]
    fn baseline_offset_refuses_odd_scales() {
        let p = at(vm(0.9, -0.2, 0.0));
        for s in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(baseline_offset(p, p.metrics, s), (0.0, 0.0));
        }
    }

    #[cfg(feature = "egui")]
    #[test]
    fn metrics_are_read_like_egui_reads_them() {
        let m = VMetrics::read(JETBRAINS_MONO, 0).unwrap();
        assert_eq!((m.ascent, m.descent, m.line_gap), (1.02, -0.30, 0.0));
        let d = egui::FontDefinitions::default();
        let u = VMetrics::read(&d.font_data["Ubuntu-Light"].font, 0).unwrap();
        near(u.ascent, 0.932);
        near(u.descent, -0.189);
        near(u.line_gap, 0.028);
        assert_eq!(VMetrics::read(b"not a font at all", 0), None);
    }

    #[cfg(feature = "egui")]
    #[test]
    fn the_two_japanese_entries_share_one_copy_of_the_bytes() {
        use std::borrow::Cow;
        let faces = Faces { japanese: Some(fake(b"shared-face-bytes")), emoji: Some(fake(b"e")), nerd: Some(fake(b"n")) };
        let ptr = |f: &egui::FontDefinitions, k: &str| match &f.font_data[k].font {
            Cow::Borrowed(b) => b.as_ptr(),
            Cow::Owned(_) => panic!("{k} is a copy"),
        };
        let mut f = egui::FontDefinitions::default();
        install(&mut f, &faces, MonoFont::Bundled);
        let first = ptr(&f, key::JAPANESE);
        assert_eq!(first, ptr(&f, key::JAPANESE_MONO));
        install(&mut f, &faces, MonoFont::Bundled);
        assert_eq!(first, ptr(&f, key::JAPANESE));
        install(&mut f, &faces, MonoFont::System);
        assert_eq!(first, ptr(&f, key::JAPANESE));
        assert_eq!(first, ptr(&f, key::JAPANESE_MONO));
        let mut g = egui::FontDefinitions::default();
        install(&mut g, &faces, MonoFont::Bundled);
        assert_eq!(first, ptr(&g, key::JAPANESE_MONO));
    }

    #[cfg(feature = "egui")]
    #[test]
    fn the_offsets_follow_whichever_face_is_first() {
        use egui::FontFamily::Monospace;
        let ubuntu = egui::FontDefinitions::default().font_data["Ubuntu-Light"].font.to_vec();
        let faces = Faces { japanese: Some(Face { path: "x".into(), bytes: ubuntu.clone() }), emoji: None, nerd: None };
        // The first face has a tweak of its own.
        let mut f = egui::FontDefinitions::default();
        std::sync::Arc::make_mut(f.font_data.get_mut("Hack").unwrap()).tweak.scale = 0.9;
        std::sync::Arc::make_mut(f.font_data.get_mut("Hack").unwrap()).tweak.y_offset_factor = 0.05;
        install(&mut f, &faces, MonoFont::System);
        assert_eq!(f.families[&Monospace][0], "Hack");
        let hack = VMetrics::read(&f.font_data["Hack"].font, 0).unwrap();
        let jm = VMetrics::read(&ubuntu, 0).unwrap();
        let want = baseline_offset(Placement { metrics: hack, scale: 0.9, y_offset_factor: 0.05, y_offset: 0.0 }, jm, 1.0);
        assert_eq!(want.0, f.font_data[key::JAPANESE_MONO].tweak.y_offset_factor);
        assert!(want.0 != 0.0);
        // A Nerd Font in front, when the machine has one.
        let Some(path) = ito_common::fonts::nerd_font() else {
            eprintln!("no Nerd Font here; skipped");
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else { return };
        let faces = Faces { nerd: Some(Face { path, bytes: bytes.clone() }), ..faces };
        let mut n = egui::FontDefinitions::default();
        install(&mut n, &faces, MonoFont::Bundled);
        assert_eq!(n.families[&Monospace][0], key::NERD);
        let nm = VMetrics::read(&bytes, 0).unwrap();
        let want = baseline_offset(at(nm), jm, 1.0);
        assert_eq!(want.0, n.font_data[key::JAPANESE_MONO].tweak.y_offset_factor);
    }

    /// The worst gap, in physical pixels, between the baseline of the Japanese glyphs and of the Latin ones.
    #[cfg(feature = "egui")]
    fn worst_gap(fonts: &egui::FontDefinitions, jm: VMetrics, ppp: f32) -> f32 {
        use egui::FontFamily::{Monospace, Proportional};
        const R: f32 = egui::emath::GUI_ROUNDING;
        let r = |v: f32| (v / R).round() * R;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts.clone());
        ctx.set_pixels_per_point(ppp);
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        let mut worst = 0.0f32;
        for (family, text, jkey) in [(Proportional, "Ag日あ", key::JAPANESE), (Monospace, "0日", key::JAPANESE_MONO)] {
            let latin_key = &fonts.families[&family][0];
            let drawn = |tweak: &egui::FontTweak, size: f32| r(size * tweak.scale * tweak.y_offset_factor + tweak.y_offset);
            for size in [12.5f32, 14.0, 20.0] {
                let id = egui::FontId::new(size, family.clone());
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), id, egui::Color32::WHITE));
                let jt = &fonts.font_data[jkey].tweak;
                let pt = size * jt.scale;
                let jh = r(jm.ascent * pt) - r(jm.descent * pt) + r(jm.line_gap * pt);
                let (mut latin, mut japanese) = (Vec::new(), Vec::new());
                for g in &galley.rows[0].glyphs {
                    if g.chr.is_ascii() {
                        assert!(g.font_face_height == g.font_height, "{:?} not drawn by the first face", g.chr);
                        latin.push(g.pos.y + drawn(&fonts.font_data[latin_key].tweak, size));
                    } else {
                        assert!((g.font_face_height - jh).abs() < 1e-3, "{:?} not drawn by the Japanese face", g.chr);
                        japanese.push(g.pos.y + drawn(jt, size));
                    }
                }
                assert!(!latin.is_empty() && !japanese.is_empty());
                for j in &japanese {
                    for l in &latin {
                        worst = worst.max((j - l).abs() * ppp);
                    }
                }
            }
        }
        worst
    }

    #[cfg(feature = "egui")]
    #[test]
    fn real_japanese_faces_sit_on_the_latin_baseline() {
        let mut paths: Vec<String> = ito_common::fonts::JAPANESE.iter().map(|s| s.to_string()).collect();
        if cfg!(windows) {
            // Not in `JAPANESE`: a variable face whose line box sits low rather than high.
            paths.push(r"C:\Windows\Fonts\NotoSansJP-VF.ttf".into());
        }
        for path in paths {
            let Ok(bytes) = std::fs::read(&path) else {
                eprintln!("{path}: missing; skipped");
                continue;
            };
            let Some(jm) = VMetrics::read(&bytes, 0) else {
                eprintln!("{path}: unreadable; skipped");
                continue;
            };
            let faces = Faces { japanese: Some(Face { path: path.clone().into(), bytes }), emoji: None, nerd: None };
            for mono in [MonoFont::Bundled, MonoFont::System] {
                let mut f = egui::FontDefinitions::default();
                install(&mut f, &faces, mono);
                for ppp in [1.0f32, 1.5] {
                    let after = worst_gap(&f, jm, ppp);
                    let mut old = f.clone();
                    for k in [key::JAPANESE, key::JAPANESE_MONO] {
                        let t = &mut std::sync::Arc::make_mut(old.font_data.get_mut(k).unwrap()).tweak;
                        (t.y_offset_factor, t.y_offset) = (0.0, 0.0);
                    }
                    let before = worst_gap(&old, jm, ppp);
                    let tol = 1.0 + 3.0 / 32.0 * ppp;
                    let first = &f.font_data[&f.families[&egui::FontFamily::Proportional][0]];
                    let d = baseline_gap(
                        Placement {
                            metrics: VMetrics::read(&first.font, first.index).unwrap(),
                            scale: first.tweak.scale,
                            y_offset_factor: 0.0,
                            y_offset: 0.0,
                        },
                        jm,
                        1.0,
                    );
                    eprintln!(
                        "{path} {mono:?} ppp {ppp}: d {d:.3} em, offset {:.3}, worst gap before {before:.2} px, after {after:.2} px",
                        f.font_data[key::JAPANESE].tweak.y_offset_factor
                    );
                    assert!(after <= tol, "{path} {mono:?} ppp {ppp}: gap {after} px");
                    if path.contains("YuGoth") {
                        assert!(before > tol, "{path}: the old placement should be off, gap {before} px");
                    }
                }
            }
        }
    }
}
