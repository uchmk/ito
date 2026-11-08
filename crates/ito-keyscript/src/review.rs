//! Visual review without a window: what a shot was (`shots/<name>.json` beside `shots/<name>.png`), how a
//! re-shot differs from the last one, a crop, and a generated theme x state script.
//!
//! Everything here works on PNG files and strings, so it is tested without egui and can
//! run from a tool as well as from an app.

use std::path::Path;

/// What a shot was taken of. Written as `shots/<name>.json` beside `shots/<name>.png`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub name: String,
    pub app: String,
    pub version: String,
    /// The theme in force when the picture was taken (`ito_theme::name`); empty when unknown.
    pub theme: String,
    /// The theme the script asked for with `<Theme:>`; empty when it asked for none.
    pub theme_requested: String,
    /// The language in force when the picture was taken (`ito_i18n::in_force`); empty when unknown.
    pub lang: String,
    /// The language the script asked for with `<Lang:>`; empty when it asked for none.
    pub lang_requested: String,
    /// The window in points.
    pub size: [f32; 2],
    /// The text zoom, 1.0 when the app has none.
    pub zoom: f32,
    /// The state the script put the app in (`empty`, `error`, ...).
    pub state: String,
    /// The part shown (`window`, or `preview` for a crop).
    pub component: String,
    /// The shot's place in the window, in points: `[x, y, w, h]`.
    pub rect: [f32; 4],
    /// Unix seconds.
    pub time: u64,
    /// Pixels per point: ties the PNG's pixels to `rect`.
    pub pixels_per_point: f32,
}

pub(crate) fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

impl Meta {
    /// One JSON object, keys in a fixed order.
    pub fn to_json(&self) -> String {
        let n = |v: f32| if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") };
        let [w, h] = self.size;
        let [x, y, rw, rh] = self.rect;
        format!(
            "{{\n  \"name\": {},\n  \"app\": {},\n  \"version\": {},\n  \"theme\": {},\n  \"theme_requested\": {},\n  \"lang\": {},\n  \"lang_requested\": {},\n  \"size\": [{}, {}],\n  \"zoom\": {},\n  \"state\": {},\n  \"component\": {},\n  \"rect\": [{}, {}, {}, {}],\n  \"time\": {},\n  \"pixels_per_point\": {}\n}}\n",
            esc(&self.name),
            esc(&self.app),
            esc(&self.version),
            esc(&self.theme),
            esc(&self.theme_requested),
            esc(&self.lang),
            esc(&self.lang_requested),
            n(w),
            n(h),
            n(self.zoom),
            esc(&self.state),
            esc(&self.component),
            n(x),
            n(y),
            n(rw),
            n(rh),
            self.time,
            n(self.pixels_per_point),
        )
    }
}

/// A decoded picture: 8-bit RGBA, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
        dec.set_transformations(png::Transformations::normalize_to_color8());
        let mut r = dec.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; r.output_buffer_size().ok_or("picture too large")?];
        let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
        buf.truncate(info.buffer_size());
        let rgba = match info.color_type {
            png::ColorType::Rgba => buf,
            png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
            png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::Indexed => return Err("indexed picture".into()),
        };
        Ok(Self { w: info.width, h: info.height, rgba })
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        Self::decode(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.w, self.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&self.rgba).map_err(|e| e.to_string())?;
        drop(w);
        Ok(out)
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, self.encode()?).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn px(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
}

/// How a re-shot differs from the last one.
#[derive(Clone, Debug, PartialEq)]
pub struct Diff {
    /// Pixels that changed, 0.0..=1.0 of the larger picture (a size change counts the missing part as changed).
    pub fraction: f32,
    /// The smallest pixel rectangle `[x, y, w, h]` holding every change; `None` when nothing changed.
    pub bounds: Option<[u32; 4]>,
    /// Changed pixels in red over a faded copy of the new picture.
    pub image: Image,
}

/// Compare two pictures. A pixel changed when any channel differs by more than `threshold`
/// (0 = exactly). Pictures of different sizes compare over the larger area.
pub fn compare(old: &Image, new: &Image, threshold: u8) -> Diff {
    let (w, h) = (old.w.max(new.w), old.h.max(new.h));
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let (mut changed, mut x0, mut y0, mut x1, mut y1) = (0u64, w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let a = (x < old.w && y < old.h).then(|| old.px(x, y));
            let b = (x < new.w && y < new.h).then(|| new.px(x, y));
            let differs = match (a, b) {
                (Some(a), Some(b)) => a.iter().zip(&b).any(|(p, q)| p.abs_diff(*q) > threshold),
                _ => true,
            };
            let i = ((y * w + x) * 4) as usize;
            if differs {
                changed += 1;
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                rgba[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
            } else if let Some(p) = b {
                let f = |c: u8| 255 - (255 - c) / 4;
                rgba[i..i + 4].copy_from_slice(&[f(p[0]), f(p[1]), f(p[2]), 255]);
            }
        }
    }
    let area = u64::from(w) * u64::from(h);
    Diff {
        fraction: if area == 0 { 0.0 } else { changed as f32 / area as f32 },
        bounds: (changed > 0).then(|| [x0, y0, x1 - x0 + 1, y1 - y0 + 1]),
        image: Image { w, h, rgba },
    }
}

/// Cut `rect` (`[x, y, w, h]` in pixels) out of `image`; the part outside the picture is dropped.
pub fn crop(image: &Image, rect: [u32; 4]) -> Result<Image, String> {
    let [x, y, w, h] = rect;
    let (x1, y1) = (x.saturating_add(w).min(image.w), y.saturating_add(h).min(image.h));
    if x >= x1 || y >= y1 {
        return Err(format!("the crop {rect:?} is outside the {}x{} picture", image.w, image.h));
    }
    let mut rgba = Vec::with_capacity(((x1 - x) * (y1 - y) * 4) as usize);
    for row in y..y1 {
        let a = ((row * image.w + x) * 4) as usize;
        rgba.extend_from_slice(&image.rgba[a..a + ((x1 - x) * 4) as usize]);
    }
    Ok(Image { w: x1 - x, h: y1 - y, rgba })
}

/// A point rectangle `[x, y, w, h]` as pixels at `pixels_per_point` (rounded outward).
pub fn rect_pixels(rect: [f32; 4], pixels_per_point: f32) -> [u32; 4] {
    let p = pixels_per_point;
    let (x0, y0) = ((rect[0] * p).floor().max(0.0), (rect[1] * p).floor().max(0.0));
    let (x1, y1) = (((rect[0] + rect[2]) * p).ceil().max(0.0), ((rect[1] + rect[3]) * p).ceil().max(0.0));
    [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32]
}

/// A script that shoots every theme x state: for each theme `<Theme:t>`, then for each state the
/// app's `enter(state)` keys, a `<Shot:state-theme>`, and `leave(state)` keys; it ends with `<Quit>`.
/// The app gives only the lists and the steps to get in and out of a state.
pub fn matrix(themes: &[&str], states: &[&str], enter: impl Fn(&str) -> String, leave: impl Fn(&str) -> String) -> String {
    let mut s = String::new();
    for t in themes {
        s.push_str(&format!("<Theme:{t}>"));
        for st in states {
            s.push_str(&enter(st));
            s.push_str(&format!("<Shot:{st}-{t}>"));
            s.push_str(&leave(st));
        }
    }
    s.push_str("<Quit>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: u32, h: u32, c: [u8; 4]) -> Image {
        Image { w, h, rgba: (0..w * h).flat_map(|_| c).collect() }
    }

    #[test]
    fn json_has_every_field_and_escapes() {
        let m =
            Meta { name: "a\"b".into(), app: "weft".into(), size: [800.0, 600.5], zoom: 1.0, pixels_per_point: 1.5, ..Default::default() };
        let j = m.to_json();
        for k in [
            "name",
            "app",
            "version",
            "theme",
            "theme_requested",
            "lang",
            "lang_requested",
            "size",
            "zoom",
            "state",
            "component",
            "rect",
            "time",
            "pixels_per_point",
        ] {
            assert!(j.contains(&format!("\"{k}\":")), "{k}");
        }
        assert!(j.contains("\"a\\\"b\"") && j.contains("[800, 600.5]") && j.contains("\"pixels_per_point\": 1.5"));
    }

    #[test]
    fn identical_pictures_do_not_differ() {
        let a = flat(4, 3, [10, 20, 30, 255]);
        let d = compare(&a, &a, 0);
        assert_eq!((d.fraction, d.bounds), (0.0, None));
    }

    #[test]
    fn a_change_gives_its_share_and_box_and_the_threshold_forgives() {
        let a = flat(10, 10, [0, 0, 0, 255]);
        let mut b = a.clone();
        for (x, y) in [(2, 3), (3, 3), (3, 4)] {
            b.rgba[((y * 10 + x) * 4) as usize] = 200;
        }
        let d = compare(&a, &b, 0);
        assert_eq!(d.bounds, Some([2, 3, 2, 2]));
        assert!((d.fraction - 0.03).abs() < 1e-6);
        assert_eq!(&d.image.rgba[((3 * 10 + 2) * 4) as usize..][..4], &[255, 0, 0, 255]);
        assert_eq!(compare(&a, &b, 200).bounds, None, "within the threshold");
    }

    #[test]
    fn a_size_change_counts_the_new_area() {
        let d = compare(&flat(2, 2, [0; 4]), &flat(4, 2, [0; 4]), 0);
        assert_eq!((d.image.w, d.bounds), (4, Some([2, 0, 2, 2])));
        assert!((d.fraction - 0.5).abs() < 1e-6);
    }

    #[test]
    fn png_round_trips_and_crops() {
        let mut a = flat(6, 4, [1, 2, 3, 255]);
        a.rgba[((6 + 2) * 4) as usize] = 99;
        let back = Image::decode(&a.encode().unwrap()).unwrap();
        assert_eq!(back, a);
        let c = crop(&a, [2, 1, 3, 2]).unwrap();
        assert_eq!((c.w, c.h, c.rgba[0]), (3, 2, 99));
        assert_eq!(crop(&a, [4, 3, 10, 10]).unwrap().w, 2, "clipped to the picture");
        assert!(crop(&a, [9, 9, 1, 1]).is_err());
    }

    #[test]
    fn points_become_pixels_outward() {
        assert_eq!(rect_pixels([10.0, 4.0, 100.5, 20.0], 1.5), [15, 6, 151, 30]);
    }

    #[test]
    fn the_matrix_parses_as_a_script() {
        let s = matrix(
            &["dark", "light"],
            &["empty", "error"],
            |st| if st == "error" { "<A-e>".into() } else { String::new() },
            |_| String::new(),
        );
        assert_eq!(s, "<Theme:dark><Shot:empty-dark><A-e><Shot:error-dark><Theme:light><Shot:empty-light><A-e><Shot:error-light><Quit>");
        assert!(crate::parse(&s).is_ok());
    }
}
