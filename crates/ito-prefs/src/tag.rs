//! A coloured tag (a chip): a label, a mail folder, a category. One look in every app.
//!
//! The colour comes from outside (Gmail hands over a colour pair) or, with none, from the text: the same
//! text is the same colour every time. Either way it is fitted to the theme in force, so a tag reads in
//! all 20 built-in themes, light and dark ([`tag_colors`], which needs no egui).

use egui::{Color32, FontId, Pos2, Rect, Sense, Vec2};
use ito_theme::{contrast, mix, Colors};

use crate::{focus_ring, take_keys, z, Look};

/// What a tag says and how it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tag<'a> {
    pub text: &'a str,
    /// The tag's own colour (any; it is fitted to the theme). `None`: picked from the theme by `text`.
    pub color: Option<Color32>,
    /// Chosen (a label that is on): filled with the colour instead of tinted.
    pub on: bool,
}

impl<'a> Tag<'a> {
    pub fn new(text: &'a str) -> Self {
        Tag { text, color: None, on: false }
    }

    pub fn color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }
}

/// The least contrast the label's text has against the tag's fill.
const MIN_CONTRAST: f32 = 4.5;

/// A stable colour for `text` from the theme's own accents.
pub fn tag_hue(c: &Colors, text: &str) -> Color32 {
    // FNV-1a: stable across runs and platforms (`DefaultHasher` is not promised to be).
    let h = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let pool = [c.blue, c.magenta, c.done, c.wait, c.run, c.err];
    pool[(h % pool.len() as u64) as usize]
}

/// `(fill, border, text)` of a tag in `c`. Tinted when off, solid when on; the text always has
/// [`MIN_CONTRAST`] against the fill.
pub fn tag_colors(c: &Colors, color: Option<Color32>, text: &str, on: bool) -> (Color32, Color32, Color32) {
    let base = color.unwrap_or_else(|| tag_hue(c, text));
    if on {
        let ink = [Color32::BLACK, Color32::WHITE].into_iter().max_by(|a, b| contrast(*a, base).total_cmp(&contrast(*b, base))).unwrap();
        return (base, mix(base, ink, 0.25), ink);
    }
    let fill = mix(c.bg, base, 0.2);
    // The colour itself as the text when it reads, else drawn toward the theme's text colour until it does.
    let mut ink = base;
    for step in 1..=10 {
        if contrast(ink, fill) >= MIN_CONTRAST {
            break;
        }
        ink = mix(base, c.fg, step as f32 / 10.0);
    }
    (fill, mix(c.bg, base, 0.5), ink)
}

/// `text` cut with "…" so that `width_of` of it is at most `max`. Whole text when it fits; "…" alone when
/// nothing more does.
pub fn fit(text: &str, max: f32, width_of: impl Fn(&str) -> f32) -> String {
    if width_of(text) <= max {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for n in (1..chars.len()).rev() {
        let cut: String = chars[..n].iter().collect::<String>().trim_end().to_owned() + "…";
        if width_of(&cut) <= max {
            return cut;
        }
    }
    "…".to_owned()
}

const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 2.0;

fn font(ctx: &egui::Context) -> FontId {
    FontId::proportional(ito_theme::zoomed(ctx, 11.5))
}

/// The width a tag takes (its text uncut), for laying a row of them out.
pub fn tag_width(ui: &egui::Ui, t: Tag) -> f32 {
    let g = ui.painter().layout_no_wrap(t.text.to_owned(), font(ui.ctx()), Color32::WHITE);
    g.size().x + PAD_X * 2.0
}

/// Paint a tag with its top-left at `at`, no wider than `max_width` (its text cut with "…" to fit), and
/// give its rectangle. For rows drawn without widgets.
pub fn paint_tag(painter: &egui::Painter, ctx: &egui::Context, c: &Colors, at: Pos2, t: Tag, max_width: f32) -> Rect {
    let (fill, border, ink) = tag_colors(c, t.color, t.text, t.on);
    let font = font(ctx);
    let width_of = |s: &str| painter.layout_no_wrap(s.to_owned(), font.clone(), ink).size().x;
    let shown = fit(t.text, (max_width - PAD_X * 2.0).max(0.0), width_of);
    let g = painter.layout_no_wrap(shown, font, ink);
    let rect = Rect::from_min_size(at, g.size() + Vec2::new(PAD_X * 2.0, PAD_Y * 2.0));
    painter.rect_filled(rect, rect.height() / 2.0, fill);
    painter.rect_stroke(rect, rect.height() / 2.0, egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
    painter.galley(rect.min + Vec2::new(PAD_X, PAD_Y), g, ink);
    rect
}

/// A tag as a widget: it takes the keys and shows the focus ring like [`crate::button`]. A click is the
/// caller's (toggle `on`, open the label).
pub fn tag(ui: &mut egui::Ui, l: Look, t: Tag) -> egui::Response {
    let max = ui.available_width();
    let h = ito_theme::zoomed(ui.ctx(), 11.5) + PAD_Y * 2.0 + z(ui, 6.0);
    let w = tag_width(ui, t).min(max);
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    if ui.is_rect_visible(rect) {
        let top = Pos2::new(rect.min.x, rect.center().y - (h - z(ui, 6.0)) / 2.0);
        paint_tag(ui.painter(), ui.ctx(), &l.c, top, t, w);
    }
    take_keys(ui, &r);
    focus_ring(ui, &r);
    r
}

/// The fixed palette of tag colours, `"#rrggbb"`. The first 8 are tsumugi's original ones (saved colours stay valid).
pub const TAG_PALETTE: [&str; 16] = [
    "#4a4f5c", "#5e4a86", "#2f6b66", "#7a5a22", "#2f4f7f", "#7a343a", "#3d6b3a", "#7a3a5e", "#2a6a8a", "#8a4a22", "#5d6b24", "#8a2f6a",
    "#4a5aa0", "#a04a3a", "#2f7a4f", "#6b6b78",
];

/// The palette slot of `hex` (case does not matter), if it is one of [`TAG_PALETTE`].
pub fn tag_slot(hex: &str) -> Option<usize> {
    TAG_PALETTE.iter().position(|p| p.eq_ignore_ascii_case(hex))
}

/// `"#rrggbb"` as a colour.
pub fn parse_hex(hex: &str) -> Option<Color32> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// The colours for the `tags` that have none in `own` yet (`tag -> hex`), each different from the others in
/// use where the palette allows. A tag's own slot is where its name hashes to (32-bit FNV-1a, the same on
/// every machine); if another tag in use has that, the first free slot; if none is free, the least shared.
/// Names with a `{` are skipped. Write the result into the app's settings once, so a tag keeps its colour.
pub fn settle_tag_colors(tags: &[&str], own: &std::collections::HashMap<String, String>) -> Vec<(String, String)> {
    let n = TAG_PALETTE.len();
    let mut uses = [0usize; 16];
    for t in tags {
        if let Some(k) = own.get(*t).and_then(|h| tag_slot(h)) {
            uses[k] += 1;
        }
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for t in tags {
        if t.contains('{') || own.contains_key(*t) || out.iter().any(|(o, _)| o == t) {
            continue;
        }
        let h = t.bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
        let home = h as usize % n;
        let k = if uses[home] == 0 {
            home
        } else if let Some(free) = (0..n).find(|k| uses[*k] == 0) {
            free
        } else {
            (0..n).min_by_key(|k| uses[*k]).unwrap_or(home)
        };
        uses[k] += 1;
        out.push(((*t).to_owned(), TAG_PALETTE[k].to_owned()));
    }
    out
}

/// A row of swatches for the settings screen: the palette, with `current` (a `#rrggbb`) ringed. The colour
/// pressed, if any.
pub fn tag_picker(ui: &mut egui::Ui, l: Look, current: &str) -> Option<&'static str> {
    let mut picked = None;
    let size = z(ui, 20.0);
    ui.horizontal_wrapped(|ui| {
        for (k, hex) in TAG_PALETTE.iter().enumerate() {
            let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
            let fill = parse_hex(hex).unwrap_or(l.c.dim);
            ui.painter().circle_filled(rect.center(), size / 2.0 - 3.0, fill);
            if tag_slot(current) == Some(k) {
                ui.painter().circle_stroke(rect.center(), size / 2.0 - 0.5, egui::Stroke::new(2.0, l.c.fg));
            }
            focus_ring(ui, &r);
            if r.clicked() {
                picked = Some(*hex);
            }
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_is_the_same_colour() {
        let c = ito_theme::colors();
        assert_eq!(tag_hue(&c, "Receipts"), tag_hue(&c, "Receipts"));
        let distinct: std::collections::HashSet<_> =
            ["a", "b", "c", "d", "e", "f", "g", "h", "Work", "Home"].iter().map(|s| tag_hue(&c, s)).collect();
        assert!(distinct.len() > 1);
    }

    #[test]
    fn a_tag_reads_in_every_theme_with_any_colour() {
        let given = [
            None,
            Some(Color32::from_rgb(0xff, 0xff, 0xff)),
            Some(Color32::BLACK),
            Some(Color32::from_rgb(0xfb, 0xe9, 0x83)),
            Some(Color32::from_rgb(0x16, 0xa7, 0x65)),
        ];
        for theme in ito_theme::builtin() {
            for color in given {
                for on in [false, true] {
                    let (fill, _, ink) = tag_colors(&theme.colors, color, "Label", on);
                    let ratio = contrast(ink, fill);
                    assert!(ratio >= MIN_CONTRAST, "{} {color:?} on={on}: {ratio}", theme.name);
                }
            }
        }
    }

    #[test]
    fn off_is_a_tint_and_on_is_the_colour_itself() {
        let c = ito_theme::colors();
        let red = Color32::from_rgb(0xcc, 0x30, 0x30);
        assert_eq!(tag_colors(&c, Some(red), "x", true).0, red);
        assert_ne!(tag_colors(&c, Some(red), "x", false).0, red);
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis() {
        let w = |s: &str| s.chars().count() as f32;
        assert_eq!(fit("short", 10.0, w), "short");
        assert_eq!(fit("a long label", 6.0, w), "a lon…");
        assert_eq!(fit("a long label", 0.0, w), "…");
        assert_eq!(fit("ab cd", 3.0, w), "ab…");
    }

    #[test]
    fn it_draws_and_a_click_is_seen() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let l = Look { c: ito_theme::colors(), q: "" };
            let r = tag(ui, l, Tag::new("Work").color(Color32::from_rgb(10, 120, 200)).on(true));
            assert!(r.rect.width() > 16.0 && !r.clicked());
            let wide = tag_width(ui, Tag::new("A very long label indeed"));
            let rect = paint_tag(ui.painter(), ui.ctx(), &l.c, Pos2::ZERO, Tag::new("A very long label indeed"), 60.0);
            assert!(rect.width() <= 60.5 && wide > 60.0, "{} {wide}", rect.width());
        });
        out.textures_delta.clear();
    }

    #[test]
    fn the_palette_is_valid_and_reads_in_every_theme() {
        assert!(TAG_PALETTE.iter().all(|h| parse_hex(h).is_some()));
        assert_eq!(TAG_PALETTE.iter().collect::<std::collections::HashSet<_>>().len(), 16);
        for theme in ito_theme::builtin() {
            for h in TAG_PALETTE {
                for on in [false, true] {
                    let (fill, _, ink) = tag_colors(&theme.colors, parse_hex(h), "x", on);
                    assert!(contrast(ink, fill) >= MIN_CONTRAST, "{} {h}", theme.name);
                }
            }
        }
    }

    #[test]
    fn sixteen_tags_get_sixteen_colours_and_a_kept_one_stays() {
        let names: Vec<String> = (0..16).map(|i| format!("tag{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut own = std::collections::HashMap::new();
        own.insert("tag0".to_owned(), TAG_PALETTE[3].to_owned());
        let got = settle_tag_colors(&refs, &own);
        assert_eq!(got.len(), 15);
        assert!(got.iter().all(|(t, _)| t != "tag0"));
        let mut slots: Vec<usize> = got.iter().map(|(_, h)| tag_slot(h).unwrap()).collect();
        slots.push(3);
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(slots.len(), 16);
        assert_eq!(settle_tag_colors(&refs, &own), got, "stable");
    }

    #[test]
    fn more_tags_than_colours_share_the_least_and_braces_are_skipped() {
        let names: Vec<String> = (0..20).map(|i| format!("t{i}")).collect();
        let mut refs: Vec<&str> = names.iter().map(String::as_str).collect();
        refs.push("{x}");
        let got = settle_tag_colors(&refs, &Default::default());
        assert_eq!(got.len(), 20);
        let mut count = [0; 16];
        for (_, h) in &got {
            count[tag_slot(h).unwrap()] += 1;
        }
        assert!(count.iter().all(|c| (1..=2).contains(c)), "{count:?}");
    }

    #[test]
    fn the_picker_draws() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let l = Look { c: ito_theme::colors(), q: "" };
            assert_eq!(tag_picker(ui, l, TAG_PALETTE[2]), None);
        });
        out.textures_delta.clear();
    }
}
