//! Ligatures (tsumugi's QUESTIONS.md Q8): `->` drawn as one arrow, `!=` as
//! `≠`, in a font that has them (Fira Code, JetBrains Mono, Cascadia Code).
//! egui draws a character, never a glyph by its number, so a run of cells
//! is shaped here with harfrust, and where the font put a glyph other than
//! the character's own, that glyph's outline is read from the font file
//! with skrifa and filled with vello_cpu, onto the same cells -- the three
//! egui itself draws text with. The grid stays a grid: each glyph starts
//! at its character's cell. Shapes and drawn glyphs are kept, so a frame
//! shapes only what it has not seen.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, ColorImage, Painter, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use skrifa::raw::TableProvider as _;
use skrifa::MetadataProvider as _;
use vello_cpu::kurbo;

/// The characters ligatures are made of; a run without two of them side
/// by side is not shaped at all.
const LIGA: &str = "-=<>!&|+*/\\:.~?#_%$^@;[](){}w";

/// Whether `text` could hold a ligature: two of [`LIGA`] side by side
/// (`www` is Fira Code's too).
pub fn worth_shaping(text: &str) -> bool {
    let mut last = false;
    for c in text.chars() {
        let now = LIGA.contains(c);
        if now && last {
            return true;
        }
        last = now;
    }
    false
}

/// A glyph shaping put in place of a character's own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sub {
    /// The character it starts at, from the run's start.
    pub at: usize,
    pub glyph: u16,
    /// How many characters it stands for (more than one when the font
    /// merged them; theirs are not drawn).
    pub covers: usize,
    /// From the start of its character's cell, in font units.
    pub dx: i32,
    pub dy: i32,
}

struct Drawn {
    tex: TextureHandle,
    /// The image's top left from the pen (on the baseline), in pixels.
    offset: Vec2,
    size: Vec2,
}

/// Drawn glyphs by number and scale; `None` for one with nothing to draw.
type Glyphs = HashMap<(u16, u32), Option<Arc<Drawn>>>;

/// One font file to shape with and draw from.
pub struct Shaper {
    data: Arc<Vec<u8>>,
    index: u32,
    /// The font's layout tables, read once.
    shaping: harfrust::ShaperData,
    runs: Mutex<HashMap<String, Arc<Vec<Sub>>>>,
    glyphs: Mutex<Glyphs>,
}

impl std::fmt::Debug for Shaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shaper").field("bytes", &self.data.len()).field("index", &self.index).finish()
    }
}

/// How many shaped runs are kept before starting again.
const RUNS_KEPT: usize = 4000;

impl Shaper {
    /// The font in `data` (`index` in a collection), when it reads and has
    /// outlines to draw.
    pub fn new(data: Vec<u8>, index: u32) -> Option<Arc<Shaper>> {
        let font = harfrust::FontRef::from_index(&data, index).ok()?;
        font.outline_glyphs().get(font.charmap().map('M')?)?;
        let shaping = harfrust::ShaperData::new(&font);
        Some(Arc::new(Shaper { data: Arc::new(data), index, shaping, runs: Mutex::new(HashMap::new()), glyphs: Mutex::new(HashMap::new()) }))
    }

    fn font(&self) -> Option<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(&self.data, self.index).ok()
    }

    /// What shaping `text` puts in place of its characters' own glyphs.
    pub fn subs(&self, text: &str) -> Arc<Vec<Sub>> {
        if let Some(s) = self.runs.lock().ok().and_then(|r| r.get(text).cloned()) {
            return s;
        }
        let found = Arc::new(self.shape(text));
        if let Ok(mut r) = self.runs.lock() {
            if r.len() > RUNS_KEPT {
                r.clear();
            }
            r.insert(text.to_owned(), found.clone());
        }
        found
    }

    fn shape(&self, text: &str) -> Vec<Sub> {
        let Some(font) = self.font() else { return Vec::new() };
        let charmap = font.charmap();
        let mut buf = harfrust::UnicodeBuffer::new();
        buf.push_str(text);
        buf.guess_segment_properties();
        let out = self.shaping.shaper(&font).build().shape(buf, harfrust::ShapeOptions::new());
        let (infos, places) = (out.glyph_infos(), out.glyph_positions());
        let starts: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        let chars: Vec<char> = text.chars().collect();
        let char_at = |byte: u32| starts.partition_point(|b| *b < byte as usize);
        let mut subs = Vec::new();
        let mut pen_in_cluster = 0;
        for (k, (info, pos)) in infos.iter().zip(places).enumerate() {
            let at = char_at(info.cluster);
            let first_of_cluster = k == 0 || infos[k - 1].cluster != info.cluster;
            if first_of_cluster {
                pen_in_cluster = 0;
            }
            // The cluster runs to the next one's start (or the end).
            let end = infos[k..].iter().find(|n| n.cluster != info.cluster).map_or(chars.len(), |n| char_at(n.cluster));
            let covers = end.saturating_sub(at).max(1);
            let own = chars.get(at).and_then(|c| charmap.map(*c)).map(|g| g.to_u32());
            if Some(info.glyph_id) != own || covers > 1 || !first_of_cluster {
                subs.push(Sub { at, glyph: info.glyph_id as u16, covers, dx: pen_in_cluster + pos.x_offset, dy: pos.y_offset });
            }
            pen_in_cluster += pos.x_advance;
        }
        subs
    }

    /// Draw glyph `glyph` with its pen at `pen` (a cell's left, on the
    /// baseline), scaled so that `M` is `cell_w` points wide.
    pub fn draw(&self, ctx: &egui::Context, painter: &Painter, sub: &Sub, pen: Pos2, cell_w: f32, color: Color32) {
        let Some(font) = self.font() else { return };
        let ppp = ctx.pixels_per_point();
        let advances = font.glyph_metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
        let m = font.charmap().map('M').and_then(|g| advances.advance_width(g)).unwrap_or(0.0).max(1.0);
        // Pixels a font unit: the grid's own measure.
        let px_unit = cell_w * ppp / m;
        let key = (sub.glyph, (px_unit * 1e5) as u32);
        let drawn = {
            let cached = self.glyphs.lock().ok().and_then(|g| g.get(&key).cloned());
            match cached {
                Some(d) => d,
                None => {
                    let d = raster(ctx, &font, sub.glyph, px_unit).map(Arc::new);
                    if let Ok(mut g) = self.glyphs.lock() {
                        g.insert(key, d.clone());
                    }
                    d
                }
            }
        };
        let Some(d) = drawn else { return };
        let at = pen + Vec2::new(sub.dx as f32 * px_unit, -sub.dy as f32 * px_unit) / ppp + d.offset / ppp;
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(d.tex.id(), Rect::from_min_size(at, d.size / ppp), uv, color);
    }
}

/// The glyph's coverage as a white image, to be tinted when drawn; `None`
/// for a glyph with nothing to draw (a ligature's spacer).
fn raster(ctx: &egui::Context, font: &harfrust::FontRef, glyph: u16, px_unit: f32) -> Option<Drawn> {
    let upem = font.head().ok()?.units_per_em();
    let outline = font.outline_glyphs().get(skrifa::GlyphId::new(u32::from(glyph)))?;
    let mut path = kurbo::BezPath::new();
    let size = skrifa::instance::Size::new(px_unit * f32::from(upem));
    outline.draw(skrifa::outline::DrawSettings::unhinted(size, skrifa::instance::LocationRef::default()), &mut Pen(&mut path)).ok()?;
    let b = path.control_box().expand();
    let (w, h) = (b.width() as u16, b.height() as u16);
    if w == 0 || h == 0 {
        return None;
    }
    let mut fill = vello_cpu::RenderContext::new(w, h);
    fill.set_transform(kurbo::Affine::translate((-b.x0, -b.y0)));
    fill.set_paint(vello_cpu::color::OpaqueColor::<vello_cpu::color::Srgb>::WHITE);
    fill.fill_path(&path);
    let mut out = vello_cpu::Pixmap::new(w, h);
    fill.render(&mut out, &mut vello_cpu::Resources::new());
    // White, so each pixel's coverage is its alpha.
    let pixels = out.data_as_u8_slice().as_chunks::<4>().0.iter().map(|&[_, _, _, a]| Color32::from_rgba_premultiplied(a, a, a, a)).collect();
    let image = ColorImage::new([usize::from(w), usize::from(h)], pixels);
    let tex = ctx.load_texture(format!("liga-{glyph}-{px_unit}"), image, TextureOptions::LINEAR);
    Some(Drawn { tex, offset: Vec2::new(b.x0 as f32, b.y0 as f32), size: Vec2::new(f32::from(w), f32::from(h)) })
}

/// An outline into a path, y down as the screen has it.
struct Pen<'a>(&'a mut kurbo::BezPath);

impl skrifa::outline::OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), -f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), -f64::from(y)));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((f64::from(cx0), -f64::from(cy0)), (f64::from(x), -f64::from(y)));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((f64::from(cx0), -f64::from(cy0)), (f64::from(cx1), -f64::from(cy1)), (f64::from(x), -f64::from(y)));
    }

    fn close(&mut self) {
        self.0.close_path();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_runs_that_can_hold_a_ligature_are_shaped() {
        assert!(worth_shaping("a->b"));
        assert!(worth_shaping("x != y") || worth_shaping("!="));
        assert!(worth_shaping("www"));
        assert!(!worth_shaping("hello"));
        assert!(!worth_shaping("a-b"));
        assert!(!worth_shaping(""));
    }

    /// Shaping finds what the font puts in place of characters. A font
    /// with ligatures is not in the repository: DejaVu Sans (its `fi`)
    /// stands in where it is installed, and the test passes quietly where
    /// it is not.
    #[test]
    fn a_ligature_is_found_and_its_characters_hidden() {
        let path = ["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/dejavu/DejaVuSans.ttf"].into_iter().find(|p| std::path::Path::new(p).exists());
        let Some(path) = path else { return };
        let shaper = Shaper::new(std::fs::read(path).unwrap(), 0).expect("the font reads");
        let subs = shaper.subs("a fine fish");
        assert!(subs.iter().any(|s| s.at == 2 && s.covers == 2), "fi at the third character, standing for two: {subs:?}");
        assert!(shaper.subs("abc").is_empty(), "nothing to put in place");
        assert!(Arc::ptr_eq(&shaper.subs("a fine fish"), &shaper.subs("a fine fish")), "shaped once");
        assert!(Shaper::new(vec![1, 2, 3], 0).is_none());

        // The `fi` glyph filled: an image about a cell high, with ink in it,
        // drawn up from the baseline.
        let fi = subs.iter().find(|s| s.covers == 2).unwrap();
        let font = shaper.font().unwrap();
        let ctx = egui::Context::default();
        let px_unit = 16.0 / 2048.0;
        let d = raster(&ctx, &font, fi.glyph, px_unit).expect("ink");
        assert!(d.size.x > 4.0 && d.size.y > 8.0 && d.size.y < 24.0, "{:?}", d.size);
        assert!(d.offset.y < -8.0, "above the baseline: {:?}", d.offset);
        let space = font.charmap().map(' ').unwrap().to_u32() as u16;
        assert!(raster(&ctx, &font, space, px_unit).is_none(), "nothing to draw");
    }
}
