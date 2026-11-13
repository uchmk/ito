//! Screenshots of the name | value editor, the Shell card and the secret field, for looking at by eye. A test that does nothing unless
//! `ITO_SHOTS=<folder>` is set; it draws with egui, rasterises the meshes on the CPU and writes PNGs.
//! Run: `ITO_SHOTS=<folder> cargo test -p ito-prefs shots -- --nocapture`.

use std::collections::{BTreeMap, HashMap};

use egui::epaint::{ImageDelta, Primitive};
use egui::{ColorImage, Context, Event, Modifiers, PointerButton, Pos2, Rect, TextureId, Vec2};

use crate::pairs::{PairWords, Pairs, PairsEditor};
use crate::secret::{Hidden, SecretField, SecretWords};
use crate::shell::{shell_card, Shell, ShellEdit, SHELL_EN, SHELL_JA};
use crate::{button_with, field_id, secret_field, Drafts, Look, Tone};

const PPP: f32 = 2.0;

#[derive(Default)]
struct Textures(HashMap<TextureId, ColorImage>);

impl Textures {
    fn apply(&mut self, id: TextureId, d: ImageDelta) {
        let egui::ImageData::Color(src) = d.image;
        match d.pos {
            None => {
                self.0.insert(id, (*src).clone());
            }
            Some([x0, y0]) => {
                if let Some(dst) = self.0.get_mut(&id) {
                    for y in 0..src.size[1] {
                        for x in 0..src.size[0] {
                            dst.pixels[(y0 + y) * dst.size[0] + x0 + x] = src.pixels[y * src.size[0] + x];
                        }
                    }
                }
            }
        }
    }

    fn sample(&self, id: TextureId, u: f32, v: f32) -> [f32; 4] {
        let Some(img) = self.0.get(&id) else { return [1.0; 4] };
        let (w, h) = (img.size[0] as i32, img.size[1] as i32);
        let (fx, fy) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let at = |x: i32, y: i32| {
            let p = img.pixels[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize];
            [p.r() as f32, p.g() as f32, p.b() as f32, p.a() as f32]
        };
        let mut o = [0.0; 4];
        for (dx, dy, wt) in [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)] {
            let p = at(x0 as i32 + dx, y0 as i32 + dy);
            for k in 0..4 {
                o[k] += p[k] * wt / 255.0;
            }
        }
        o
    }
}

/// The shapes of one frame as pixels (premultiplied alpha blended in gamma space, as egui does).
fn raster(ctx: &Context, shapes: Vec<egui::epaint::ClippedShape>, tex: &Textures, size_pts: Vec2, bg: egui::Color32) -> ColorImage {
    let (w, h) = ((size_pts.x * PPP) as usize, (size_pts.y * PPP) as usize);
    let mut img = ColorImage::filled([w, h], bg);
    for prim in ctx.tessellate(shapes, PPP) {
        let Primitive::Mesh(mesh) = prim.primitive else { continue };
        let clip = prim.clip_rect;
        let (cx0, cy0) = ((clip.min.x * PPP).floor().max(0.0) as i32, (clip.min.y * PPP).floor().max(0.0) as i32);
        let (cx1, cy1) = ((clip.max.x * PPP).ceil().min(w as f32) as i32, (clip.max.y * PPP).ceil().min(h as f32) as i32);
        for tri in mesh.indices.as_chunks::<3>().0 {
            let v = [mesh.vertices[tri[0] as usize], mesh.vertices[tri[1] as usize], mesh.vertices[tri[2] as usize]];
            let p = [v[0].pos * PPP, v[1].pos * PPP, v[2].pos * PPP];
            let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y);
            if area.abs() < 1e-6 {
                continue;
            }
            let min_x = p.iter().map(|q| q.x).fold(f32::MAX, f32::min).floor() as i32;
            let max_x = p.iter().map(|q| q.x).fold(f32::MIN, f32::max).ceil() as i32;
            let min_y = p.iter().map(|q| q.y).fold(f32::MAX, f32::min).floor() as i32;
            let max_y = p.iter().map(|q| q.y).fold(f32::MIN, f32::max).ceil() as i32;
            for y in min_y.max(cy0)..max_y.min(cy1) {
                for x in min_x.max(cx0)..max_x.min(cx1) {
                    let q = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((p[1].x - q.x) * (p[2].y - q.y) - (p[2].x - q.x) * (p[1].y - q.y)) / area;
                    let w1 = ((p[2].x - q.x) * (p[0].y - q.y) - (p[0].x - q.x) * (p[2].y - q.y)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                    let vv = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                    let t = tex.sample(mesh.texture_id, u, vv);
                    let mut col = [0.0f32; 4];
                    for k in 0..4 {
                        let c = |i: usize| [v[i].color.r(), v[i].color.g(), v[i].color.b(), v[i].color.a()][k] as f32 / 255.0;
                        col[k] = (w0 * c(0) + w1 * c(1) + w2 * c(2)) * t[k];
                    }
                    let d = &mut img.pixels[y as usize * w + x as usize];
                    let inv = 1.0 - col[3];
                    let mix = |s: f32, d: u8| ((s + d as f32 / 255.0 * inv).clamp(0.0, 1.0) * 255.0).round() as u8;
                    *d = egui::Color32::from_rgba_premultiplied(mix(col[0], d.r()), mix(col[1], d.g()), mix(col[2], d.b()), 255);
                }
            }
        }
    }
    img
}

struct Scene {
    ctx: Context,
    tex: Textures,
    width: f32,
    bg: egui::Color32,
    last: Option<(Vec<egui::epaint::ClippedShape>, f32)>,
}

impl Scene {
    fn new(dark: bool, width: f32) -> Self {
        let themes = ito_theme::builtin();
        let t = ito_theme::pick(&themes, if dark { "dark" } else { "light" }, "", "", false).expect("theme");
        ito_theme::set_theme(t);
        let ctx = Context::default();
        ctx.enable_accesskit();
        ctx.set_visuals(t.colors.visuals());
        let mut fonts = egui::FontDefinitions::default();
        for f in ["C:\\Windows\\Fonts\\YuGothM.ttc", "C:\\Windows\\Fonts\\meiryo.ttc"] {
            if let Ok(bytes) = std::fs::read(f) {
                fonts.font_data.insert("cjk".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts.families.get_mut(&fam).expect("family").push("cjk".into());
                }
                break;
            }
        }
        ctx.set_fonts(fonts);
        Scene { ctx, tex: Textures::default(), width, bg: t.colors.bg, last: None }
    }

    fn frame(&mut self, events: Vec<Event>, mut draw: impl FnMut(&mut egui::Ui)) {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(self.width, 900.0))),
            events,
            ..Default::default()
        };
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(PPP);
        let mut used = 0.0;
        let bg = self.bg;
        let mut out = self.ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(bg).inner_margin(16.0)).show(ui, |ui| {
                draw(ui);
                used = ui.cursor().min.y;
            });
        });
        for (id, ds) in std::mem::take(&mut out.textures_delta.set) {
            for d in ds {
                self.tex.apply(id, d);
            }
        }
        self.last = Some((out.shapes, used + 16.0));
    }

    fn click(&mut self, name: &str, mut draw: impl FnMut(&mut egui::Ui)) {
        self.frame(vec![], &mut draw);
        let at = ito_a11y::take_test_ids(&self.ctx, egui::ViewportId::ROOT)
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} not registered"))
            .rect
            .center();
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(at)], &mut draw);
        self.frame(vec![button(true)], &mut draw);
        self.frame(vec![button(false)], &mut draw);
        self.frame(vec![], &mut draw);
        self.frame(vec![], &mut draw);
    }

    fn save(&mut self, dir: &std::path::Path, name: &str) {
        let (shapes, used) = self.last.take().expect("a frame");
        let img = raster(&self.ctx, shapes, &self.tex, Vec2::new(self.width, used.max(60.0)), self.bg);
        ito_keyscript::save_png(&dir.join(format!("{name}.png")), &img).expect("png");
        println!("wrote {name}.png");
    }
}

#[test]
fn shots() {
    let Ok(dir) = std::env::var("ITO_SHOTS") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("dir");
    for (dark, ja, width) in [
        (false, false, 1000.0),
        (true, true, 420.0),
        (false, false, 420.0),
        (true, true, 1000.0),
        (true, false, 420.0),
        (false, true, 1000.0),
        (false, true, 300.0),
    ] {
        let tag = format!("{}-{}-{}", if dark { "dark" } else { "light" }, if ja { "ja" } else { "en" }, width as u32);
        let words = if ja { PairWords::ja() } else { PairWords::en() };
        let (hint_name, hint_value) = if ja { ("名前", "値") } else { ("NAME", "Value") };
        let editor = move |ui: &mut egui::Ui, p: &mut Pairs| {
            PairsEditor::new("hd", words).hints(hint_name, hint_value).mono_names().mono_values().unique(true).show(ui, p);
        };

        // The empty editor.
        let mut s = Scene::new(dark, width);
        let mut p = Pairs::new(std::iter::empty::<(&str, &str)>());
        s.frame(vec![], |ui| editor(ui, &mut p));
        s.frame(vec![], |ui| editor(ui, &mut p));
        s.save(&dir, &format!("pairs-empty-{tag}"));

        // Rows with a fault (a name used twice, a value without a name), then Add (the new name box has the keys).
        let mut s = Scene::new(dark, width);
        let mut p =
            Pairs::new([("Authorization", "Bearer abc123"), ("Accept", "application/json"), ("accept", "text/html"), ("", "orphan")]);
        s.frame(vec![], |ui| editor(ui, &mut p));
        s.frame(vec![], |ui| editor(ui, &mut p));
        s.save(&dir, &format!("pairs-fault-{tag}"));
        s.click("hd.add", |ui| editor(ui, &mut p));
        s.save(&dir, &format!("pairs-added-{tag}"));

        // A very long name and a long value, and a value that refers to a secret.
        let mut s = Scene::new(dark, width);
        let long_value = "https://example.invalid/a/very/long/value/that/goes/on/and/on/for/more/than/eighty/characters/in/total";
        let mut p = Pairs::new([
            ("Authorization", "secret:api"),
            ("X-Very-Long-Custom-Header-Name-For-Testing", long_value),
            ("Accept", "application/json"),
        ]);
        s.frame(vec![], |ui| editor(ui, &mut p));
        s.frame(vec![], |ui| editor(ui, &mut p));
        s.save(&dir, &format!("pairs-long-{tag}"));

        // The Shell card with the editor open.
        let mut s = Scene::new(dark, width);
        let env: BTreeMap<String, String> =
            [("PATH_EXTRA", "C:\\tools"), ("LANG", "ja_JP.UTF-8")].map(|(a, b)| (a.to_owned(), b.to_owned())).into();
        let shells = vec![("PowerShell".to_owned(), "pwsh.exe".to_owned())];
        let args = vec!["-NoLogo".to_owned()];
        let now = Shell { program: "", auto: "Automatic", shells: &shells, args: &args, env: &env };
        let (mut drafts, mut edit) = (Drafts::new(), ShellEdit::default());
        edit.open(&env);
        let w = if ja { &SHELL_JA } else { &SHELL_EN };
        for _ in 0..2 {
            s.frame(vec![], |ui| {
                shell_card(ui, Look { c: ito_theme::colors(), q: "" }, w, &mut drafts, &mut edit, &now);
            });
        }
        s.save(&dir, &format!("shell-{tag}"));
    }
}

/// The words of the small secret form.
struct SecretForm {
    title: &'static str,
    password: &'static str,
    token: &'static str,
    token_error: &'static str,
    client: &'static str,
    client_note: &'static str,
    kept: &'static str,
    not_kept: &'static str,
    save: &'static str,
    eye: SecretWords<'static>,
}

fn secret_words(ja: bool) -> SecretForm {
    if ja {
        SecretForm {
            title: "メール アカウント",
            password: "パスワード",
            token: "API トークン",
            token_error: "トークンを入力してください",
            client: "クライアント シークレット（入力した時点で保存）",
            client_note: "Enter か、欄を離れると保存します。Esc で破棄します",
            kept: "保存済み",
            not_kept: "未保存",
            save: "保存",
            eye: SecretWords { show: "表示", hide: "隠す" },
        }
    } else {
        SecretForm {
            title: "Mail account",
            password: "Password",
            token: "API token",
            token_error: "Enter the token",
            client: "Client secret (saved as soon as it is typed)",
            client_note: "Enter, or leaving the box, saves it. Esc drops it",
            kept: "kept",
            not_kept: "not kept",
            save: "Save",
            eye: SecretWords { show: "Show", hide: "Hide" },
        }
    }
}

struct SecretState {
    password: Hidden,
    token: Hidden,
    drafts: Drafts,
}

/// A label, a bound secret, a bound one with an error, a take-mode one, and Save. The secrets are fake.
fn secret_form(ui: &mut egui::Ui, w: &SecretForm, f: &mut SecretState, token_error: bool) {
    let c = ito_theme::colors();
    let l = Look { c, q: "" };
    let width = (ui.available_width() - 40.0).clamp(240.0, 460.0);
    ui.label(egui::RichText::new(w.title).size(16.0).strong().color(c.strong()));
    ui.add_space(12.0);
    ui.label(egui::RichText::new(w.password).size(12.5).color(c.dim));
    SecretField::new("password", &w.eye).name(w.password).hint(w.kept).width(width).show(ui, l, &mut f.password);
    ui.add_space(12.0);
    ui.label(egui::RichText::new(w.token).size(12.5).color(c.dim));
    SecretField::new("token", &w.eye).name(w.token).hint(w.not_kept).width(width).error(token_error).show(ui, l, &mut f.token);
    if token_error {
        ui.label(egui::RichText::new(w.token_error).size(11.5).color(c.err));
    }
    ui.add_space(12.0);
    ui.label(egui::RichText::new(w.client).size(12.5).color(c.dim));
    secret_field(ui, l, &mut f.drafts, "client", w.kept, &w.eye, width);
    ui.label(egui::RichText::new(w.client_note).size(11.5).color(c.dim));
    ui.add_space(14.0);
    button_with(ui, l, w.save, Tone::Primary);
}

#[test]
fn secret_shots() {
    let Ok(dir) = std::env::var("ITO_SHOTS") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let new_state = || SecretState { password: Hidden::new("fake-pass-1234"), token: Hidden::default(), drafts: Drafts::new() };
    for dark in [false, true] {
        for ja in [false, true] {
            for width in [420.0, 1000.0] {
                for reveal in [false, true] {
                    let w = secret_words(ja);
                    let mut f = new_state();
                    let mut s = Scene::new(dark, width);
                    let mut draw = |ui: &mut egui::Ui| secret_form(ui, &w, &mut f, true);
                    for _ in 0..4 {
                        s.frame(vec![], &mut draw);
                    }
                    if reveal {
                        s.click("secret.password.eye", &mut draw);
                    }
                    let tag = format!(
                        "{}-{}-{}-{}",
                        if dark { "dark" } else { "light" },
                        if ja { "ja" } else { "en" },
                        width as u32,
                        if reveal { "revealed" } else { "masked" }
                    );
                    s.save(&dir, &format!("secret-{tag}"));
                }
            }
        }
        // Take mode: typing into the client-secret box.
        let w = secret_words(false);
        let mut f = new_state();
        let mut s = Scene::new(dark, 420.0);
        let mut draw = |ui: &mut egui::Ui| secret_form(ui, &w, &mut f, false);
        for _ in 0..4 {
            s.frame(vec![], &mut draw);
        }
        s.ctx.memory_mut(|m| m.request_focus(field_id("client")));
        s.frame(vec![], &mut draw);
        s.frame(vec![Event::Text("fake-client-secret".to_owned())], &mut draw);
        s.frame(vec![], &mut draw);
        s.save(&dir, &format!("secret-take-{}", if dark { "dark" } else { "light" }));
    }
}
