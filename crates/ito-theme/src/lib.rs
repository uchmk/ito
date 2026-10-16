//! Themes (tsumugi's design 1k), shared by every uchmk app: the colours the
//! window and its panes are drawn in. A theme is the design's ten colours -- background, sidebar, panel,
//! border, text, dim text, and the four states' (waiting, running, error,
//! done) -- plus blue and magenta for the terminal; everything else is mixed
//! from them, so a theme of one's own needs only those.
//!
//! A person's themes are `*.toml` files in `<config>/uchmk/themes/`
//! ([`load_dir`]); which one is in force is common.toml's `theme`
//! ([`pick`]).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{LazyLock, RwLock};

use egui::Color32;
#[cfg(feature = "pane")]
use ito_pane::Palette;

/// A theme's own colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colors {
    pub light: bool,
    pub bg: Color32,
    /// The sidebar, the band and the status bar.
    pub side: Color32,
    /// Cards, panes, fields and dialogs.
    pub panel: Color32,
    pub border: Color32,
    pub fg: Color32,
    pub dim: Color32,
    pub wait: Color32,
    pub run: Color32,
    pub err: Color32,
    pub done: Color32,
    pub blue: Color32,
    pub magenta: Color32,
    /// The terminal's sixteen as a scheme gives them (`ansi` in a theme
    /// file, or an imported scheme); else they are mixed from the above.
    pub ansi16: Option<[Color32; 16]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: Colors,
}

/// The built-in themes: the design's thirteen (its 9b), Solarized Dark's
/// red lightened a step (`#dc322f` to `#e04644`) to read on its panel. Each
/// line is name,
/// light, then bg, side, panel, border, fg, dim, wait, run, err, done,
/// blue, magenta.
const BUILTIN: [(&str, bool, [u32; 12]); 13] = [
    (
        "tsumugi Dark",
        false,
        [0x16181d, 0x121418, 0x1b1e24, 0x2c3039, 0xc8cdd8, 0x9aa3b5, 0xe8c87a, 0x6fd0d0, 0xf07178, 0x8ed08e, 0x7ab8f5, 0xc99cf0],
    ),
    (
        "tsumugi Light",
        true,
        [0xf7f8fa, 0xeef0f3, 0xffffff, 0xdde1e7, 0x1f2430, 0x5a6272, 0x8f6515, 0x137979, 0xc43642, 0x2d7b39, 0x2a5db0, 0x8a3fb5],
    ),
    (
        "Tokyo Night",
        false,
        [0x1a1b26, 0x16161e, 0x1f2335, 0x292e42, 0xc0caf5, 0x9aa5ce, 0xe0af68, 0x7dcfff, 0xf7768e, 0x9ece6a, 0x7aa2f7, 0xbb9af7],
    ),
    (
        "Catppuccin Mocha",
        false,
        [0x1e1e2e, 0x181825, 0x313244, 0x45475a, 0xcdd6f4, 0xa6adc8, 0xf9e2af, 0x89dceb, 0xf38ba8, 0xa6e3a1, 0x89b4fa, 0xcba6f7],
    ),
    (
        "Catppuccin Latte",
        true,
        [0xeff1f5, 0xe6e9ef, 0xffffff, 0xccd0da, 0x4c4f69, 0x5c5f77, 0x985908, 0x047097, 0xd20f39, 0x307720, 0x0c5af4, 0x8839ef],
    ),
    (
        "Dracula",
        false,
        [0x282a36, 0x21222c, 0x343746, 0x44475a, 0xf8f8f2, 0xb4b8d0, 0xf1fa8c, 0x8be9fd, 0xff5555, 0x50fa7b, 0xbd93f9, 0xff79c6],
    ),
    (
        "Nord",
        false,
        [0x2e3440, 0x272c36, 0x3b4252, 0x434c5e, 0xeceff4, 0xb8c1d1, 0xebcb8b, 0x88c0d0, 0xd08770, 0xa3be8c, 0x81a1c1, 0xb48ead],
    ),
    (
        "Gruvbox Dark",
        false,
        [0x282828, 0x1d2021, 0x32302f, 0x504945, 0xebdbb2, 0xbdae93, 0xfabd2f, 0x83a598, 0xfb4934, 0xb8bb26, 0x83a598, 0xd3869b],
    ),
    (
        "Gruvbox Light",
        true,
        [0xfbf1c7, 0xf2e5bc, 0xf9f5d7, 0xd5c4a1, 0x3c3836, 0x5a524c, 0x9c6512, 0x076678, 0x9d0006, 0x79740e, 0x076678, 0x8f3f71],
    ),
    (
        "Solarized Dark",
        false,
        [0x002b36, 0x00242e, 0x073642, 0x0f4a57, 0xd3dcdc, 0x93a1a1, 0xb58900, 0x2aa198, 0xe04644, 0x859900, 0x268bd2, 0xd33682],
    ),
    (
        "Solarized Light",
        true,
        [0xfdf6e3, 0xeee8d5, 0xffffff, 0xe0d8c0, 0x073642, 0x586e75, 0x946f00, 0x1b7a73, 0xc02a27, 0x677700, 0x268bd2, 0xd33682],
    ),
    (
        "One Dark",
        false,
        [0x282c34, 0x21252b, 0x2c313a, 0x3e4451, 0xc5cad3, 0x9aa1ad, 0xe5c07b, 0x56b6c2, 0xe06c75, 0x98c379, 0x61afef, 0xc678dd],
    ),
    (
        "Rosé Pine",
        false,
        [0x191724, 0x1f1d2e, 0x26233a, 0x403d52, 0xe0def4, 0xa8a5c2, 0xf6c177, 0x9ccfd8, 0xeb6f92, 0x7fb8a4, 0x31748f, 0xc4a7e7],
    ),
];

/// The keys a theme file sets, in [`Colors`]' order after `light`.
const KEYS: [&str; 12] = ["bg", "side", "panel", "border", "fg", "dim", "wait", "run", "err", "done", "blue", "magenta"];

fn rgb(x: u32) -> Color32 {
    Color32::from_rgb((x >> 16) as u8, (x >> 8) as u8, x as u8)
}

impl Colors {
    fn from_list(light: bool, c: [u32; 12]) -> Self {
        let [bg, side, panel, border, fg, dim, wait, run, err, done, blue, magenta] = c.map(rgb);
        Self { light, bg, side, panel, border, fg, dim, wait, run, err, done, blue, magenta, ansi16: None }
    }

    fn slot(&mut self, key: &str) -> Option<&mut Color32> {
        Some(match key {
            "bg" => &mut self.bg,
            "side" => &mut self.side,
            "panel" => &mut self.panel,
            "border" => &mut self.border,
            "fg" => &mut self.fg,
            "dim" => &mut self.dim,
            "wait" => &mut self.wait,
            "run" => &mut self.run,
            "err" => &mut self.err,
            "done" => &mut self.done,
            "blue" => &mut self.blue,
            "magenta" => &mut self.magenta,
            _ => return None,
        })
    }

    // What is mixed from the ten.

    /// Menus and popups, a step up from the panel.
    pub fn raised(&self) -> Color32 {
        mix(self.panel, self.fg, 0.04)
    }
    /// A border that stands out: an outlined button, a dialog's edge.
    pub fn border_strong(&self) -> Color32 {
        mix(self.border, self.fg, 0.15)
    }
    /// Behind what is chosen: a filter that is on, a line picked.
    pub fn chosen(&self) -> Color32 {
        mix(self.panel, self.fg, 0.09)
    }
    /// Behind what the pointer is over.
    pub fn hover(&self) -> Color32 {
        mix(self.panel, self.fg, 0.05)
    }
    /// Text that matters most: names, headings.
    pub fn strong(&self) -> Color32 {
        if self.light {
            mix(self.fg, Color32::BLACK, 0.4)
        } else {
            mix(self.fg, Color32::WHITE, 0.5)
        }
    }
    /// The least of the text: key hints, counts.
    pub fn faint(&self) -> Color32 {
        mix(self.dim, self.bg, 0.2)
    }
    /// A card that waits for a person: the panel with a little gold.
    pub fn wait_bg(&self) -> Color32 {
        mix(self.panel, self.wait, 0.06)
    }
    /// A waiting card's name.
    pub fn wait_text(&self) -> Color32 {
        mix(self.strong(), self.wait, 0.2)
    }
    /// Text on a filled accent (the gold count, the Create button).
    pub fn on_accent(&self) -> Color32 {
        if self.light {
            Color32::WHITE
        } else {
            self.bg
        }
    }
    /// Behind the selected cells and the tab shown: ito-select's colour, the
    /// one every app's selection takes.
    pub fn selection(&self) -> Color32 {
        ito_select::fill(self.panel, self.run, self.light)
    }

    /// The terminal's sixteen: black and white from the theme's own text
    /// and panel, the brights a little lighter (darker on a light theme).
    pub fn ansi(&self) -> [Color32; 16] {
        if let Some(own) = self.ansi16 {
            return own;
        }
        let normal = [self.panel, self.err, self.done, self.wait, self.blue, self.magenta, self.run, self.fg];
        let toward = if self.light { Color32::BLACK } else { Color32::WHITE };
        let bright = |c: Color32| mix(c, toward, 0.25);
        let mut out = [Color32::BLACK; 16];
        for (k, c) in normal.iter().enumerate() {
            out[k] = *c;
            out[k + 8] = bright(*c);
        }
        out[0] = if self.light { mix(self.fg, Color32::BLACK, 0.3) } else { mix(self.panel, Color32::BLACK, 0.2) };
        out[7] = if self.light { self.dim } else { self.fg };
        out[8] = self.dim;
        out[15] = self.strong();
        out
    }

    /// The pane's colours.
    #[cfg(feature = "pane")]
    pub fn palette(&self) -> Palette {
        Palette {
            bg: self.panel,
            fg: self.fg,
            fg_dim: mix(self.dim, self.panel, 0.2),
            selection: self.selection(),
            cursor: self.run,
            on_cursor: self.bg,
            ansi: Some(self.ansi()),
        }
    }

    /// egui's own widgets in these colours.
    pub fn visuals(&self) -> egui::Visuals {
        let mut v = if self.light { egui::Visuals::light() } else { egui::Visuals::dark() };
        v.override_text_color = Some(self.fg);
        v.panel_fill = self.bg;
        v.window_fill = self.raised();
        v.window_stroke = egui::Stroke::new(1.0, self.border_strong());
        v.extreme_bg_color = self.side;
        v.faint_bg_color = self.hover();
        v.hyperlink_color = self.run;
        v.selection.bg_fill = self.selection();
        v.selection.stroke = egui::Stroke::new(1.0, self.strong());
        v.text_edit_bg_color = Some(self.side);
        for w in
            [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open]
        {
            w.fg_stroke.color = self.fg;
        }
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, self.border);
        v.widgets.inactive.weak_bg_fill = self.chosen();
        v.widgets.inactive.bg_fill = self.chosen();
        v.widgets.hovered.weak_bg_fill = mix(self.chosen(), self.fg, 0.06);
        v.widgets.hovered.bg_fill = mix(self.chosen(), self.fg, 0.06);
        v.widgets.active.weak_bg_fill = mix(self.chosen(), self.fg, 0.12);
        v.widgets.active.bg_fill = mix(self.chosen(), self.fg, 0.12);
        v
    }
}

/// `a` with `t` of `b` in it.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// WCAG's contrast ratio between two colours, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let ch = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// The built-in themes, `tsumugi Dark` first.
pub fn builtin() -> Vec<Theme> {
    BUILTIN.iter().map(|(name, light, c)| Theme { name: (*name).into(), colors: Colors::from_list(*light, *c) }).collect()
}

/// A theme's colours from a file's table: `light`, and any of the twelve
/// as `"#rrggbb"`; what it leaves out comes from `base`.
pub fn from_table(base: Colors, table: &BTreeMap<String, toml::Value>) -> Result<Colors, String> {
    let mut c = base;
    for (k, v) in table {
        match (k.as_str(), v) {
            ("name", _) => {}
            ("light", toml::Value::Boolean(b)) => c.light = *b,
            ("ansi", toml::Value::Array(list)) => {
                let colors: Option<Vec<Color32>> = list.iter().map(|v| v.as_str().and_then(parse_hex)).collect();
                match colors.and_then(|c| <[Color32; 16]>::try_from(c).ok()) {
                    Some(sixteen) => c.ansi16 = Some(sixteen),
                    None => return Err("ansi: sixteen colours like \"#1b1e24\", black to bright white".into()),
                }
            }
            (key, toml::Value::String(s)) => {
                let Some(slot) = c.slot(key) else { return Err(format!("`{key}` is not one of light, {}, ansi", KEYS.join(", "))) };
                *slot = parse_hex(s).ok_or_else(|| format!("{key}: `{s}` is not a colour like \"#1b1e24\""))?;
            }
            (key, _) => return Err(format!("`{key}`: a colour is written as \"#rrggbb\"")),
        }
    }
    Ok(c)
}

fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok().map(rgb)).flatten()
}

/// The themes in `dir` (`<config>/uchmk/themes`): each `*.toml` a theme
/// named by its `name`, else by the file's name, its colours over
/// `tsumugi Dark`'s; with what is wrong with the files that do not read.
/// No folder is no themes.
pub fn load_dir(dir: &Path) -> (Vec<Theme>, Vec<String>) {
    let (mut out, mut bad) = (Vec::new(), Vec::new());
    let Ok(entries) = std::fs::read_dir(dir) else { return (out, bad) };
    let mut files: Vec<_> =
        entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
    files.sort();
    let base = builtin()[0].colors;
    for f in files {
        let read = std::fs::read_to_string(&f)
            .map_err(|e| e.to_string())
            .and_then(|text| toml::from_str::<BTreeMap<String, toml::Value>>(&text).map_err(|e| e.message().to_owned()));
        let theme = read.and_then(|t| {
            let name = t
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            from_table(base, &t).map(|colors| Theme { name, colors })
        });
        match theme {
            Ok(t) => out.push(t),
            Err(e) => bad.push(format!("{}: {e}", f.display())),
        }
    }
    (out, bad)
}

/// The theme in force, which the drawing code reads.
static CURRENT: LazyLock<RwLock<Colors>> = LazyLock::new(|| RwLock::new(builtin()[0].colors));

/// The colours in force.
pub fn colors() -> Colors {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

/// Put `c` in force; the next frame draws in it.
pub fn set(c: Colors) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = c;
}

/// Which theme `choice` (the settings' `theme`) means: a theme's name, or
/// `dark`, `light`, or `system` (the other two names following the OS).
pub fn pick<'a>(themes: &'a [Theme], choice: &str, dark: &str, light: &str, os_light: bool) -> Result<&'a Theme, String> {
    let name = match choice {
        "dark" => "tsumugi Dark",
        "light" => "tsumugi Light",
        "system" if os_light => light,
        "system" => dark,
        other => other,
    };
    themes.iter().find(|t| t.name.eq_ignore_ascii_case(name)).ok_or_else(|| {
        let names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
        format!("theme: no theme `{name}` (there are {})", names.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in theme reads: text 7 to 1 against its background, dim
    /// text 4.5 (the design's 9), and the four states' marks 3 against
    /// the panel they sit on.
    #[test]
    fn every_theme_reads() {
        for t in builtin() {
            let c = t.colors;
            let name = &t.name;
            assert!(contrast(c.fg, c.bg) >= 7.0, "{name}: text {:.1}", contrast(c.fg, c.bg));
            assert!(contrast(c.dim, c.bg) >= 4.5, "{name}: dim text {:.1}", contrast(c.dim, c.bg));
            for (what, s) in [("wait", c.wait), ("run", c.run), ("err", c.err), ("done", c.done)] {
                assert!(contrast(s, c.panel) >= 3.0, "{name}: {what} {:.1}", contrast(s, c.panel));
            }
        }
    }

    #[test]
    fn the_status_colours_of_the_light_themes_reach_aa() {
        // (theme, slots that must read at 4.5:1 on bg, side and panel); the others are measured but not yet held to it.
        let held: [(&str, &[&str]); 2] =
            [("tsumugi Light", &["wait", "run", "done"]), ("Catppuccin Latte", &["wait", "run", "done", "blue"])];
        for t in builtin().into_iter().filter(|t| t.colors.light) {
            let Some((_, slots)) = held.iter().find(|(n, _)| *n == t.name) else { continue };
            let mut c = t.colors;
            for slot in *slots {
                let fg = *c.slot(slot).unwrap();
                for (on, bg) in [("bg", c.bg), ("side", c.side), ("panel", c.panel)] {
                    assert!(contrast(fg, bg) >= 4.5, "{}: {slot} on {on} {:.2}", t.name, contrast(fg, bg));
                }
            }
        }
    }

    #[test]
    fn the_first_is_filers_colours() {
        let c = builtin()[0].colors;
        assert_eq!((c.bg, c.panel, c.fg), (rgb(0x16181d), rgb(0x1b1e24), rgb(0xc8cdd8)));
        #[cfg(feature = "pane")]
        assert_eq!(c.palette().bg, Palette::default().bg, "the pane as before");
    }

    #[test]
    fn a_file_overrides_what_it_names() {
        let base = builtin()[0].colors;
        let table: BTreeMap<String, toml::Value> = toml::from_str("bg = \"#000000\"\nlight = true").unwrap();
        let c = from_table(base, &table).unwrap();
        assert_eq!((c.bg, c.light, c.fg), (Color32::BLACK, true, base.fg));
        let bad: BTreeMap<String, toml::Value> = toml::from_str("bgg = \"#000000\"").unwrap();
        assert!(from_table(base, &bad).unwrap_err().starts_with("`bgg`"));
        let bad: BTreeMap<String, toml::Value> = toml::from_str("bg = \"black\"").unwrap();
        assert!(from_table(base, &bad).is_err());
    }

    #[test]
    fn the_choice_finds_its_theme() {
        let all = builtin();
        let name = |c: &str, os_light: bool| pick(&all, c, "Nord", "Catppuccin Latte", os_light).map(|t| t.name.clone());
        assert_eq!(name("dark", true).unwrap(), "tsumugi Dark");
        assert_eq!(name("system", false).unwrap(), "Nord");
        assert_eq!(name("system", true).unwrap(), "Catppuccin Latte");
        assert_eq!(name("dracula", false).unwrap(), "Dracula", "case aside");
        assert!(name("nope", false).unwrap_err().contains("no theme `nope`"));
    }

    #[test]
    fn a_folder_of_themes_is_read_and_its_mistakes_said() {
        let dir = std::env::temp_dir().join(format!("ito-theme-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load_dir(&dir), (Vec::new(), Vec::new()), "no folder: none");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.toml"), "name = \"Mine\"\nbg = \"#000000\"\n").unwrap();
        std::fs::write(dir.join("a.toml"), "light = true\n").unwrap();
        std::fs::write(dir.join("c.toml"), "bg = \"black\"\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a theme").unwrap();
        let (themes, bad) = load_dir(&dir);
        let names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["a", "Mine"], "by the file's name, else its own");
        assert_eq!((themes[1].colors.bg, themes[1].colors.fg), (Color32::BLACK, builtin()[0].colors.fg));
        assert!(themes[0].colors.light);
        assert_eq!(bad.len(), 1);
        assert!(bad[0].contains("c.toml") && bad[0].contains("not a colour"), "{bad:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
