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
        self.json(None)
    }

    /// [`to_json`](Self::to_json) with the capture's facts appended after `pixels_per_point`.
    pub fn to_json_with(&self, c: &Capture) -> String {
        self.json(Some(c))
    }

    fn json(&self, cap: Option<&Capture>) -> String {
        let n = |v: f32| if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v}") };
        let [w, h] = self.size;
        let [x, y, rw, rh] = self.rect;
        let mut out = format!(
            "{{\n  \"name\": {},\n  \"app\": {},\n  \"version\": {},\n  \"theme\": {},\n  \"theme_requested\": {},\n  \"lang\": {},\n  \"lang_requested\": {},\n  \"size\": [{}, {}],\n  \"zoom\": {},\n  \"state\": {},\n  \"component\": {},\n  \"rect\": [{}, {}, {}, {}],\n  \"time\": {},\n  \"pixels_per_point\": {}",
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
        );
        if let Some(c) = cap {
            let opt_u = |v: Option<u64>| v.map_or_else(|| "null".to_owned(), |v| v.to_string());
            let flag = c.state_unchanged.map_or_else(|| "null".to_owned(), |b| b.to_string());
            out.push_str(&format!(
                ",\n  \"run_id\": {},\n  \"snapshot_seq\": {},\n  \"capture_id\": {},\n  \"png\": {},\n  \"png_sha256\": {},\n  \"size_px\": [{}, {}],\n  \"state_before\": {},\n  \"state_after\": {},\n  \"state_unchanged\": {},\n  \"requested_frame\": {},\n  \"received_frame\": {},\n  \"env\": {{\"os\": {}, \"arch\": {}, \"wgpu_backend\": {}, \"font_set_sha256\": {}, \"theme\": {}, \"pixels_per_point\": {}}}",
                opt_str(&c.run_id),
                opt_u(c.snapshot_seq),
                esc(&c.capture_id),
                esc(&c.png),
                esc(&c.png_sha256),
                c.size_px[0],
                c.size_px[1],
                state_ref_json(&c.state_before),
                state_ref_json(&c.state_after),
                flag,
                c.requested_frame,
                c.received_frame,
                esc(&c.env.os),
                esc(&c.env.arch),
                opt_str(&c.env.wgpu_backend),
                opt_str(&c.env.font_set_sha256),
                esc(&c.env.theme),
                n(c.env.pixels_per_point),
            ));
        }
        out.push_str("\n}\n");
        out
    }
}

/// Facts about one capture, added to the side JSON by [`Meta::to_json_with`]. Build with `Default` and assign the fields.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Capture {
    pub run_id: Option<String>,
    pub snapshot_seq: Option<u64>,
    pub capture_id: String,
    /// The PNG's file name beside this JSON (`<name>.png`).
    pub png: String,
    pub png_sha256: String,
    pub size_px: [u32; 2],
    pub state_before: Option<StateRef>,
    pub state_after: Option<StateRef>,
    /// `Some(true)` when both states have the same `ui_sha256` and `app_sha256`; `None` without both states.
    pub state_unchanged: Option<bool>,
    pub requested_frame: u64,
    pub received_frame: u64,
    pub env: Env,
}

/// A state JSON file a capture refers to.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct StateRef {
    /// Relative to the done file's folder, `/` separators (`state/a-before.json`).
    pub path: String,
    pub sha256: String,
    pub projection: u32,
    pub ui_sha256: String,
    pub app_sha256: String,
}

/// Where the picture was taken.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Env {
    pub os: String,
    pub arch: String,
    pub wgpu_backend: Option<String>,
    pub font_set_sha256: Option<String>,
    pub theme: String,
    pub pixels_per_point: f32,
}

fn opt_str(v: &Option<String>) -> String {
    v.as_deref().map_or_else(|| "null".to_owned(), esc)
}

fn state_ref_json(r: &Option<StateRef>) -> String {
    match r {
        None => "null".to_owned(),
        Some(r) => format!(
            "{{\"path\": {}, \"sha256\": {}, \"stable\": {{\"projection\": {}, \"ui_sha256\": {}, \"app_sha256\": {}}}}}",
            esc(&r.path),
            esc(&r.sha256),
            r.projection,
            esc(&r.ui_sha256),
            esc(&r.app_sha256)
        ),
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
    fn to_json_with_appends_the_capture_in_order() {
        let m = Meta { name: "a".into(), pixels_per_point: 1.5, ..Meta::default() };
        let r = StateRef { path: "state/a-before.json".into(), sha256: "aa".into(), projection: 1, ..StateRef::default() };
        let mut c = Capture {
            capture_id: "1-a".into(),
            png: "a.png".into(),
            png_sha256: "ff".into(),
            size_px: [8, 6],
            state_before: Some(r),
            ..Capture::default()
        };
        c.env.os = "os".into();
        let j = m.to_json_with(&c);
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        // `serde_json::Value` sorts its keys, so read the order from the text: the top-level keys are the ones at 2 spaces.
        let keys: Vec<&str> = j.lines().filter_map(|l| l.strip_prefix("  \"")).filter_map(|l| l.split('"').next()).collect();
        let tail = [
            "pixels_per_point",
            "run_id",
            "snapshot_seq",
            "capture_id",
            "png",
            "png_sha256",
            "size_px",
            "state_before",
            "state_after",
            "state_unchanged",
            "requested_frame",
            "received_frame",
            "env",
        ];
        let at = keys.iter().position(|k| *k == "pixels_per_point").unwrap();
        assert_eq!(&keys[at..], &tail);
        assert!(v["run_id"].is_null() && v["snapshot_seq"].is_null() && v["state_after"].is_null() && v["state_unchanged"].is_null());
        assert!(v["env"]["wgpu_backend"].is_null() && v["env"]["font_set_sha256"].is_null());
        assert_eq!(v["state_before"]["stable"]["projection"], 1);
        assert_eq!(v["size_px"], serde_json::json!([8, 6]));
        assert_eq!(m.to_json(), m.to_json().trim_end().to_owned() + "\n");
        assert!(!m.to_json().contains("capture_id"));
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

    fn meta() -> Meta {
        Meta {
            name: "main-dark".into(),
            app: "demo".into(),
            version: "1.2.3".into(),
            theme: "ito-dark".into(),
            theme_requested: "ito-dark".into(),
            lang: "ja".into(),
            lang_requested: "ja".into(),
            size: [800.0, 600.0],
            zoom: 1.0,
            state: "empty".into(),
            component: "window".into(),
            rect: [0.0, 0.0, 800.0, 600.0],
            time: 1_731_050_000,
            pixels_per_point: 1.5,
        }
    }

    fn state_ref(name: &str) -> StateRef {
        StateRef {
            path: format!("state/{name}.json"),
            sha256: "aa".repeat(32),
            projection: 1,
            ui_sha256: "bb".repeat(32),
            app_sha256: "cc".repeat(32),
        }
    }

    /// The files in `docs/schemas/examples/` have the shape (keys, order, types) of what the writer writes.
    #[test]
    fn the_side_json_has_the_shape_of_the_examples() {
        let mut c = Capture {
            run_id: Some("20261108-101203-41_15-3f9a".into()),
            snapshot_seq: Some(2),
            capture_id: "1-main-dark".into(),
            png: "main-dark.png".into(),
            png_sha256: "dd".repeat(32),
            size_px: [1200, 900],
            state_before: Some(state_ref("main-dark-before")),
            state_after: Some(state_ref("main-dark-after")),
            state_unchanged: Some(true),
            requested_frame: 40,
            received_frame: 42,
            ..Capture::default()
        };
        c.env = Env {
            os: "windows".into(),
            arch: "x86_64".into(),
            wgpu_backend: Some("gl".into()),
            font_set_sha256: Some("ee".repeat(32)),
            theme: "ito-dark".into(),
            pixels_per_point: 1.5,
        };
        crate::shape::check("shot-snap", &meta().to_json_with(&c), crate::shape::example!("shot-snap"));

        // A shot without `<Snap:>`: no states, no run id, and the env facts ito does not fill yet are null.
        let mut p = Capture {
            capture_id: "2-main".into(),
            png: "main.png".into(),
            png_sha256: "dd".repeat(32),
            size_px: [1200, 900],
            ..Capture::default()
        };
        p.env = Env { os: "windows".into(), arch: "x86_64".into(), theme: "ito-dark".into(), pixels_per_point: 1.5, ..Env::default() };
        crate::shape::check("shot-plain", &meta().to_json_with(&p), crate::shape::example!("shot-plain"));
    }
}
