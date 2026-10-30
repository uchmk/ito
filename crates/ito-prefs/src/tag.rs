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
}
