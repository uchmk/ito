//! Themes (tsumugi's design 1k), shared by every uchmk app: the colours the
//! window and its panes are drawn in. A theme is the design's ten colours -- background, sidebar, panel,
//! border, text, dim text, and the four states' (waiting, running, error,
//! done) -- plus blue and magenta for the terminal; everything else is mixed
//! from them, so a theme of one's own needs only those.
//!
//! A person's themes are `*.toml` files in `<config>/uchmk/themes/`
//! ([`load_dir`]); which one is in force is common.toml's `theme`
//! ([`pick`]).

pub mod scale;

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
/// red lightened a step (`#dc322f` to `#e04644`) to read on its panel; then
/// Campbell (a deep black, Windows Terminal's default) and the editors'
/// VS Code Dark+ / Light+, GitHub Light, One Light, Rosé Pine Dawn and Tokyo
/// Night Day, whose text or muted colour is darkened a step where the
/// original is under the 7 and 4.5 [`tests::every_theme_reads`] holds. Each
/// line is name,
/// light, then bg, side, panel, border, fg, dim, wait, run, err, done,
/// blue, magenta.
const BUILTIN: [(&str, bool, [u32; 12]); 20] = [
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
    (
        "Campbell",
        false,
        [0x0c0c0c, 0x050505, 0x181818, 0x2f2f2f, 0xcccccc, 0x9a9a9a, 0xf9f1a5, 0x61d6d6, 0xe74856, 0x16c60c, 0x3b78ff, 0xb4009e],
    ),
    (
        "VS Code Dark+",
        false,
        [0x1e1e1e, 0x181818, 0x252526, 0x3c3c3c, 0xd4d4d4, 0x9d9d9d, 0xdcdcaa, 0x4ec9b0, 0xf48771, 0x73c991, 0x569cd6, 0xc586c0],
    ),
    (
        "VS Code Light+",
        true,
        [0xf3f3f3, 0xe8e8e8, 0xffffff, 0xd0d0d0, 0x333333, 0x616161, 0x795e26, 0x267f99, 0xcd3131, 0x098658, 0x0451a5, 0xaf00db],
    ),
    (
        "GitHub Light",
        true,
        [0xf6f8fa, 0xeaeef2, 0xffffff, 0xd0d7de, 0x1f2328, 0x59636e, 0x9a6700, 0x1b7c83, 0xcf222e, 0x1a7f37, 0x0969da, 0x8250df],
    ),
    (
        "One Light",
        true,
        [0xfafafa, 0xf0f0f1, 0xffffff, 0xdbdbdc, 0x383a42, 0x696c77, 0x986801, 0x0184bc, 0xe45649, 0x50a14f, 0x4078f2, 0xa626a4],
    ),
    (
        "Rosé Pine Dawn",
        true,
        [0xfaf4ed, 0xf2e9e1, 0xfffaf3, 0xdfdad9, 0x4a456a, 0x6e6a86, 0xb07219, 0x3d8791, 0xb4637a, 0x3c7f5a, 0x286983, 0x907aa9],
    ),
    (
        "Tokyo Night Day",
        true,
        [0xe1e2e7, 0xd0d5e3, 0xe9e9ed, 0xc4c8da, 0x343b58, 0x565f89, 0x8c6c3e, 0x007197, 0xd1345b, 0x587539, 0x2e7de9, 0x9854f1],
    ),
];

/// Campbell's sixteen terminal colours (Windows Terminal's default scheme),
/// black to white, then the brights.
const CAMPBELL_ANSI: [u32; 16] = [
    0x0c0c0c, 0xc50f1f, 0x13a10e, 0xc19c00, 0x0037da, 0x881798, 0x3a96dd, 0xcccccc, 0x767676, 0xe74856, 0x16c60c, 0xf9f1a5, 0x3b78ff,
    0xb4009e, 0x61d6d6, 0xf2f2f2,
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

    // Named roles over the same twelve (uchmk/ito#186). None is a new theme-file key, so a theme
    // file that sets the twelve gets all of them, and `high_contrast` carries them along.

    /// The one colour that says "this is the way": the primary button, the focus. It is `run`.
    pub fn accent(&self) -> Color32 {
        self.run
    }
    /// Something went through. It is `done`; draw it with `ito_a11y::State` as well.
    pub fn success(&self) -> Color32 {
        self.done
    }
    /// Something needs a look. It is `wait`; draw it with `ito_a11y::State` as well.
    pub fn warning(&self) -> Color32 {
        self.wait
    }

    /// Line numbers: quieter than the text.
    pub fn gutter(&self) -> Color32 {
        mix(self.dim, self.bg, 0.25)
    }
    /// The line number of the line the caret is on: the only strong one.
    pub fn gutter_active(&self) -> Color32 {
        mix(self.dim, self.fg, 0.7)
    }
    /// A faint band behind the line the caret is on.
    pub fn current_line(&self) -> Color32 {
        mix(self.bg, self.fg, 0.05)
    }
    /// The caret in a window that has the keyboard; it shows over a heading's colour too.
    pub fn caret(&self) -> Color32 {
        self.strong()
    }
    /// The caret in a window that does not.
    pub fn caret_idle(&self) -> Color32 {
        self.dim
    }
    /// A mark for what cannot be seen (a full-width space, the end of a line): quiet, but there in every theme.
    pub fn invisible(&self) -> Color32 {
        mix(self.dim, self.bg, 0.35)
    }
    /// The box behind such a mark.
    pub fn invisible_fill(&self) -> Color32 {
        mix(self.bg, self.dim, 0.14)
    }
    /// The underline of text an input method is still composing.
    pub fn ime_underline(&self) -> Color32 {
        self.run
    }

    /// An added line or gutter bar. Pair it with a shape (a `+` or a bar), never colour alone.
    pub fn diff_added(&self) -> Color32 {
        self.done
    }
    /// A changed line or gutter bar.
    pub fn diff_modified(&self) -> Color32 {
        self.blue
    }
    /// A removed line or gutter mark.
    pub fn diff_removed(&self) -> Color32 {
        self.err
    }
    /// The faint background of an added, changed or removed line (`diff_*` is the mark; this sits under the text).
    pub fn diff_bg(&self, mark: Color32) -> Color32 {
        mix(self.bg, mark, 0.14)
    }
    /// Behind each search match; background only, so the text keeps its colour.
    pub fn match_bg(&self) -> Color32 {
        mix(self.bg, self.wait, 0.15)
    }
    /// Behind the match the search is on now.
    pub fn match_current_bg(&self) -> Color32 {
        mix(self.bg, self.wait, 0.24)
    }
    /// A folded range's line.
    pub fn fold(&self) -> Color32 {
        mix(self.bg, self.dim, 0.18)
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
            found: self.wait,
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

impl Colors {
    /// The high-contrast variant of these colours (`high_contrast = true` in
    /// common.toml, read by `ito_common::Common::high_contrast_or_off`): same
    /// hue and light/dark, but text at 15:1 or more, dim text and the status
    /// colours at 7:1 against both the background and the panel, and borders
    /// that show. The terminal's own sixteen are kept.
    pub fn high_contrast(&self) -> Colors {
        let (ink, ground) = if self.light { (Color32::BLACK, Color32::WHITE) } else { (Color32::WHITE, Color32::BLACK) };
        let mut c = *self;
        // The grounds go to the extreme, one step apart so that a panel still
        // stands off its background.
        c.bg = mix(self.bg, ground, 0.85);
        c.side = mix(self.side, ground, 0.75);
        c.panel = mix(self.panel, ground, 0.7);
        c.fg = mix(self.fg, ink, 0.9);
        // Raise a colour toward the ink until it reads at `want` on every ground.
        let lift = |x: Color32, want: f32| {
            let mut t = 0.0;
            loop {
                let y = mix(x, ink, t);
                if [c.bg, c.side, c.panel].iter().all(|g| contrast(y, *g) >= want) || t >= 1.0 {
                    return y;
                }
                t += 0.05;
            }
        };
        c.dim = lift(self.dim, 7.0);
        for slot in [&mut c.wait, &mut c.run, &mut c.err, &mut c.done, &mut c.blue, &mut c.magenta] {
            *slot = lift(*slot, 7.0);
        }
        c.border = lift(mix(self.border, ink, 0.3), 3.0);
        c
    }
}

/// The built-in themes, `tsumugi Dark` first.
pub fn builtin() -> Vec<Theme> {
    BUILTIN
        .iter()
        .map(|(name, light, c)| {
            let mut colors = Colors::from_list(*light, *c);
            if *name == "Campbell" {
                colors.ansi16 = Some(CAMPBELL_ANSI.map(rgb));
            }
            Theme { name: (*name).into(), colors }
        })
        .collect()
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

/// The theme in force: its colours and, when the app said which one it is, its name (they change together).
struct InForce {
    colors: Colors,
    name: Option<String>,
}

/// The theme in force, which the drawing code reads.
static CURRENT: LazyLock<RwLock<InForce>> = LazyLock::new(|| RwLock::new(InForce { colors: builtin()[0].colors, name: None }));

/// The colours in force.
pub fn colors() -> Colors {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).colors
}

/// Put `c` in force; the next frame draws in it. The name of the theme in force becomes unknown ([`name`] is `None`):
/// an app that knows which theme it applies uses [`set_theme`].
pub fn set(c: Colors) {
    let mut g = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    g.colors = c;
    g.name = None;
}

/// Put `t` in force, colours and name together; the next frame draws in it.
pub fn set_theme(t: &Theme) {
    let mut g = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    g.colors = t.colors;
    g.name = Some(t.name.clone());
}

/// The name of the theme in force, `None` when unknown (never set, or the last write was [`set`]).
pub fn name() -> Option<String> {
    CURRENT.read().unwrap_or_else(|e| e.into_inner()).name.clone()
}

/// The key the letters' zoom is kept under in egui's context.
fn zoom_id() -> egui::Id {
    egui::Id::new("ito-text-zoom")
}

/// Set how much the app's own letters are zoomed (1.0 is the size the
/// settings give): the panes' size over the settings' size, so that a
/// screen of the window's own -- settings, help, palette, a confirm -- grows
/// and shrinks with the panes instead of staying small beside large ones.
/// Call it every frame, before the screens are drawn. Not the window's
/// `scale` (`set_zoom_factor` already carries that to everything).
pub fn set_text_zoom(ctx: &egui::Context, zoom: f32) {
    let zoom = if zoom.is_finite() { zoom.clamp(0.25, 8.0) } else { 1.0 };
    ctx.data_mut(|d| d.insert_temp(zoom_id(), zoom));
}

/// The letters' zoom set by [`set_text_zoom`], 1.0 until it is.
pub fn text_zoom(ctx: &egui::Context) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(zoom_id())).unwrap_or(1.0)
}

/// `size` points of text at the letters' zoom: what every size written in a
/// screen of the window's own goes through (`RichText::size`, `FontId`).
pub fn zoomed(ctx: &egui::Context, size: f32) -> f32 {
    size * text_zoom(ctx)
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

/// `#rrggbb`, or `rgba(r, g, b, a)` when the colour is not opaque.
fn css_color(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({r}, {g}, {b}, {:.3})", f32::from(a) / 255.0)
    }
}

impl Colors {
    /// The colours as CSS custom properties (one per line, no selector), for a page an app serves:
    /// the base ones (`--bg`, `--side`, `--panel`, `--border`, `--fg`, `--dim`, `--wait`, `--run`, `--err`,
    /// `--done`, `--blue`, `--magenta`), the mixed ones (`--raised`, `--border-strong`, `--chosen`, `--hover`,
    /// `--strong`, `--faint`, `--wait-bg`, `--wait-text`, `--on-accent`, `--selection`) and `color-scheme`.
    /// Each is `#rrggbb`, or `rgba(...)` where the alpha is under 255.
    pub fn css_vars(&self) -> String {
        let rows: [(&str, Color32); 42] = [
            ("bg", self.bg),
            ("side", self.side),
            ("panel", self.panel),
            ("border", self.border),
            ("fg", self.fg),
            ("dim", self.dim),
            ("wait", self.wait),
            ("run", self.run),
            ("err", self.err),
            ("done", self.done),
            ("blue", self.blue),
            ("magenta", self.magenta),
            ("raised", self.raised()),
            ("border-strong", self.border_strong()),
            ("chosen", self.chosen()),
            ("hover", self.hover()),
            ("strong", self.strong()),
            ("faint", self.faint()),
            ("wait-bg", self.wait_bg()),
            ("wait-text", self.wait_text()),
            ("on-accent", self.on_accent()),
            ("selection", self.selection()),
            ("accent", self.accent()),
            ("success", self.success()),
            ("warning", self.warning()),
            ("gutter", self.gutter()),
            ("gutter-active", self.gutter_active()),
            ("current-line", self.current_line()),
            ("caret", self.caret()),
            ("caret-idle", self.caret_idle()),
            ("invisible", self.invisible()),
            ("invisible-fill", self.invisible_fill()),
            ("ime-underline", self.ime_underline()),
            ("diff-added", self.diff_added()),
            ("diff-modified", self.diff_modified()),
            ("diff-removed", self.diff_removed()),
            ("diff-added-bg", self.diff_bg(self.diff_added())),
            ("diff-modified-bg", self.diff_bg(self.diff_modified())),
            ("diff-removed-bg", self.diff_bg(self.diff_removed())),
            ("match-bg", self.match_bg()),
            ("match-current-bg", self.match_current_bg()),
            ("fold", self.fold()),
        ];
        let mut out = String::new();
        for (name, c) in rows {
            out.push_str(&format!(
                "  --{name}: {};
",
                css_color(c)
            ));
        }
        out.push_str(&format!(
            "  color-scheme: {};
",
            if self.light { "light" } else { "dark" }
        ));
        out
    }
}

/// A whole style sheet for the theme `choice` names, picked the way the window picks ([`pick`] with the
/// same names; the choice `system` follows the browser instead of the OS this runs on).
///
/// - A fixed theme gives one `:root { … }`.
/// - `system` puts the `dark` theme in `:root` and the `light` one in
///   `@media (prefers-color-scheme: light) { :root { … } }`.
/// - `high_contrast` gives each theme's [`Colors::high_contrast`] colours.
pub fn stylesheet(themes: &[Theme], choice: &str, dark: &str, light: &str, high_contrast: bool) -> Result<String, String> {
    let colors = |t: &Theme| if high_contrast { t.colors.high_contrast() } else { t.colors };
    if choice == "system" {
        let (d, l) = (pick(themes, choice, dark, light, false)?, pick(themes, choice, dark, light, true)?);
        let (d, l) = (colors(d).css_vars(), colors(l).css_vars());
        let indented: String = l
            .lines()
            .map(|row| {
                format!(
                    "  {row}
"
                )
            })
            .collect();
        return Ok(format!(
            ":root {{
{d}}}
@media (prefers-color-scheme: light) {{
  :root {{
{indented}  }}
}}
"
        ));
    }
    let t = pick(themes, choice, dark, light, false)?;
    Ok(format!(
        ":root {{
{}}}
",
        colors(t).css_vars()
    ))
}

#[cfg(test)]
mod tests {
    /// The theme in force is a process global: this test is the only one that writes it.
    #[test]
    fn set_theme_keeps_the_name_and_set_forgets_it() {
        let all = builtin();
        set_theme(&all[1]);
        assert_eq!(name().as_deref(), Some(all[1].name.as_str()));
        assert_eq!(colors(), all[1].colors);
        set(all[2].colors);
        assert_eq!(name(), None);
        assert_eq!(colors(), all[2].colors);
    }

    #[test]
    fn letters_zoom_with_the_value_set() {
        let ctx = egui::Context::default();
        assert_eq!(zoomed(&ctx, 12.0), 12.0);
        set_text_zoom(&ctx, 1.5);
        assert_eq!(zoomed(&ctx, 12.0), 18.0);
        set_text_zoom(&ctx, f32::NAN);
        assert_eq!(text_zoom(&ctx), 1.0);
    }

    #[test]
    fn css_has_every_colour_and_the_scheme() {
        let t = builtin();
        let css = stylesheet(&t, "dark", "tsumugi Dark", "tsumugi Light", false).unwrap();
        for n in [
            "bg",
            "side",
            "panel",
            "border",
            "fg",
            "dim",
            "wait",
            "run",
            "err",
            "done",
            "blue",
            "magenta",
            "raised",
            "border-strong",
            "chosen",
            "hover",
            "strong",
            "faint",
            "wait-bg",
            "wait-text",
            "on-accent",
            "selection",
        ] {
            assert!(css.contains(&format!("--{n}: ")), "{n}");
        }
        assert!(css.starts_with(":root {") && css.contains("color-scheme: dark;"), "{css}");
        assert!(stylesheet(&t, "light", "", "", false).unwrap().contains("color-scheme: light;"));
        assert!(stylesheet(&t, "nope", "", "", false).is_err());
    }

    #[test]
    fn system_puts_light_under_the_media_query_and_alpha_is_rgba() {
        let t = builtin();
        let css = stylesheet(&t, "system", "tsumugi Dark", "tsumugi Light", false).unwrap();
        let (root, media) = css.split_once("@media (prefers-color-scheme: light)").unwrap();
        assert!(root.contains("color-scheme: dark;") && media.contains("color-scheme: light;"), "{css}");
        assert_eq!(css_color(Color32::from_rgb(1, 2, 255)), "#0102ff");
        assert!(css_color(Color32::from_rgba_unmultiplied(10, 20, 30, 128)).starts_with("rgba(10, 20, 30, 0.50"));
        let hc = stylesheet(&t, "dark", "", "", true).unwrap();
        assert_ne!(hc, stylesheet(&t, "dark", "", "", false).unwrap());
    }

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

    /// The editor and diff roles (uchmk/ito#186) read in every theme, and in its high-contrast variant.
    #[test]
    fn the_editor_and_diff_roles_read() {
        for t in builtin() {
            for (kind, c) in [("", t.colors), (" high-contrast", t.colors.high_contrast())] {
                let name = format!("{}{kind}", t.name);
                let on = |what: &str, fg: Color32, bg: Color32, want: f32| {
                    let got = contrast(fg, bg);
                    assert!(got >= want, "{name}: {what} {got:.2} < {want}");
                };
                on("gutter on bg", c.gutter(), c.bg, 2.5);
                on("active gutter on current line", c.gutter_active(), c.current_line(), 4.5);
                assert!(contrast(c.gutter_active(), c.bg) > contrast(c.gutter(), c.bg), "{name}: active gutter is stronger");
                on("text on current line", c.fg, c.current_line(), 6.0);
                on("caret on bg", c.caret(), c.bg, 4.5);
                on("idle caret on bg", c.caret_idle(), c.bg, 3.0);
                on("invisible on bg", c.invisible(), c.bg, 1.5);
                on("invisible on its fill", c.invisible(), c.invisible_fill(), 1.3);
                on("ime underline on bg", c.ime_underline(), c.bg, 3.0);
                for (what, m) in [("added", c.diff_added()), ("modified", c.diff_modified()), ("removed", c.diff_removed())] {
                    on(&format!("{what} mark on bg"), m, c.bg, 3.0);
                    on(&format!("text on {what} bg"), c.fg, c.diff_bg(m), 4.5);
                }
                on("text on match", c.fg, c.match_bg(), 4.5);
                on("text on current match", c.fg, c.match_current_bg(), 4.5);
                on("text on fold", c.fg, c.fold(), 4.5);
                assert!(c.match_current_bg() != c.match_bg(), "{name}: the current match stands out");
            }
        }
    }

    #[test]
    fn css_vars_carry_the_new_roles() {
        let css = builtin()[0].colors.css_vars();
        for name in ["accent", "gutter-active", "current-line", "caret", "invisible-fill", "diff-removed-bg", "match-current-bg", "fold"] {
            assert!(css.contains(&format!("--{name}: ")), "{name}");
        }
    }

    /// The high-contrast variant of every built-in theme: text, dim text and
    /// the status colours 7:1 on every ground, borders 3:1, light stays light.
    #[test]
    fn every_theme_has_a_high_contrast_variant_that_reads() {
        for t in builtin() {
            let h = t.colors.high_contrast();
            let name = &t.name;
            assert_eq!(h.light, t.colors.light, "{name}");
            for (what, g) in [("bg", h.bg), ("side", h.side), ("panel", h.panel)] {
                assert!(contrast(h.fg, g) >= 12.0, "{name}: text on {what} {:.1}", contrast(h.fg, g));
                for (slot, s) in [
                    ("dim", h.dim),
                    ("wait", h.wait),
                    ("run", h.run),
                    ("err", h.err),
                    ("done", h.done),
                    ("blue", h.blue),
                    ("magenta", h.magenta),
                ] {
                    assert!(contrast(s, g) >= 7.0, "{name}: {slot} on {what} {:.1}", contrast(s, g));
                }
            }
            assert!(contrast(h.border, h.panel) >= 3.0, "{name}: border {:.1}", contrast(h.border, h.panel));
            assert_eq!(h.ansi16, t.colors.ansi16, "{name}");
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
    fn the_built_in_set_has_unique_names_and_campbell_keeps_its_sixteen() {
        let all = builtin();
        let mut names: Vec<&str> = all.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!((names.len(), all.iter().filter(|t| t.colors.light).count()), (20, 9));
        let campbell = all.iter().find(|t| t.name == "Campbell").unwrap().colors;
        assert_eq!((campbell.bg, campbell.ansi()[0], campbell.ansi()[1]), (rgb(0x0c0c0c), rgb(0x0c0c0c), rgb(0xc50f1f)));
        assert!(all.iter().filter(|t| t.name != "Campbell").all(|t| t.colors.ansi16.is_none()));
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
