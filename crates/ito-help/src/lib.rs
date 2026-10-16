//! The keys help: what each layer of the keymap binds, as headings and
//! `keys  description` rows, drawn into a rect the app gives and scrolled by
//! a line index the app keeps.
//!
//! The list comes from [`ito_keymap::help_lines`] (the keys that do the same
//! thing already on one line); [`push_layer`] turns a layer into [`Line`]s.
//! The frame (title, border, dimmed background) stays the app's: this draws
//! only into the body, so an app with its own panel keeps it.

use egui::{pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect};
use ito_keymap::HelpLine;

/// One line of the help.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A layer's name.
    Head(String),
    /// The keys and what they do.
    Bind { keys: String, desc: String },
    /// An empty line between layers.
    Blank,
}

/// Add a layer: a blank line first if there is already something, then the
/// heading, then a row per [`HelpLine`] with the description run through
/// `desc` (the app translates its built-in descriptions; a binding without
/// one shows its command text).
pub fn push_layer(out: &mut Vec<Line>, title: &str, lines: &[HelpLine], desc: impl Fn(&str) -> String) {
    if !out.is_empty() {
        out.push(Line::Blank);
    }
    out.push(Line::Head(title.to_owned()));
    for l in lines {
        let text = if l.desc.is_empty() { l.run.to_owned() } else { desc(l.desc) };
        out.push(Line::Bind { keys: l.keys.clone(), desc: text });
    }
}

/// The colours and the type of the rows.
#[derive(Clone, Debug)]
pub struct Style {
    pub font: FontId,
    pub row_h: f32,
    pub head: Color32,
    pub keys: Color32,
    pub desc: Color32,
    /// The scroll bar, drawn when the list is longer than the rect.
    pub bar: Color32,
}

/// What a draw found out, which the keys that scroll need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// Rows that fit in the rect (at least 1): a page.
    pub visible: usize,
    /// The largest scroll: the last line sits at the bottom.
    pub max: usize,
    /// The scroll that was drawn, `scroll` held to `max`.
    pub scroll: usize,
}

/// The column the descriptions start at: 40% of the width, but no more than
/// `28` cells of the font.
fn key_width(width: f32, cell: f32) -> f32 {
    (width * 0.4).min(cell * 28.0)
}

/// Draw `lines` from line `scroll` into `body`, and report how far it can
/// scroll. A text longer than its column ends in `…` rather than running into
/// the next one.
pub fn draw(painter: &Painter, body: Rect, lines: &[Line], scroll: usize, style: &Style) -> Drawn {
    let painter = painter.with_clip_rect(body.intersect(painter.clip_rect()));
    let visible = ((body.height() / style.row_h).floor() as usize).max(1);
    let max = lines.len().saturating_sub(visible);
    let scroll = scroll.min(max);
    let cell = painter.layout_no_wrap("M".repeat(10), style.font.clone(), style.keys).size().x / 10.0;
    let key_w = key_width(body.width(), cell);
    for (i, line) in lines.iter().skip(scroll).take(visible).enumerate() {
        let y = body.top() + i as f32 * style.row_h;
        match line {
            Line::Head(t) => {
                painter.text(
                    pos2(body.left() + 4.0, y),
                    Align2::LEFT_TOP,
                    fit(&painter, t, &style.font, body.width() - 8.0),
                    style.font.clone(),
                    style.head,
                );
            }
            Line::Bind { keys, desc } => {
                let k = fit(&painter, keys, &style.font, key_w - 16.0);
                painter.text(pos2(body.left() + 12.0, y), Align2::LEFT_TOP, k, style.font.clone(), style.keys);
                let d = fit(&painter, desc, &style.font, body.width() - key_w - 8.0);
                painter.text(pos2(body.left() + key_w, y), Align2::LEFT_TOP, d, style.font.clone(), style.desc);
            }
            Line::Blank => {}
        }
    }
    if max > 0 {
        // Where the view sits in the list.
        let track = Rect::from_min_max(pos2(body.right() - 3.0, body.top()), pos2(body.right(), body.bottom()));
        let th = (track.height() * visible as f32 / lines.len() as f32).max(12.0);
        let ty = track.top() + (track.height() - th) * scroll as f32 / max as f32;
        painter.rect_filled(Rect::from_min_size(Pos2::new(track.left(), ty), vec2(3.0, th)), 1.5, style.bar);
    }
    Drawn { visible, max, scroll }
}

/// `s` cut to `width` with `…`, by whole characters.
pub fn fit(painter: &Painter, s: &str, font: &FontId, width: f32) -> String {
    let w = |t: &str| painter.layout_no_wrap(t.to_owned(), font.clone(), Color32::WHITE).size().x;
    if width <= 0.0 {
        return String::new();
    }
    if w(s) <= width {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    // The longest prefix that fits with the ellipsis, by bisection.
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let t: String = chars[..mid].iter().collect::<String>() + "…";
        if w(&t) <= width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

/// A scroll position moved by `delta` lines and held to `max`.
pub fn scrolled(scroll: usize, delta: i64, max: usize) -> usize {
    (scroll as i64 + delta).clamp(0, max as i64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use ito_keymap::HelpLine;

    #[test]
    fn layers_are_separated_and_the_description_is_translated() {
        let a = [HelpLine { keys: "q".into(), desc: "Quit", run: "quit" }, HelpLine { keys: "x".into(), desc: "", run: "noop" }];
        let mut out = Vec::new();
        push_layer(&mut out, "Main", &a, |d| d.to_uppercase());
        push_layer(&mut out, "Input", &[], |d| d.to_owned());
        assert_eq!(
            out,
            [
                Line::Head("Main".into()),
                Line::Bind { keys: "q".into(), desc: "QUIT".into() },
                Line::Bind { keys: "x".into(), desc: "noop".into() },
                Line::Blank,
                Line::Head("Input".into()),
            ]
        );
    }

    #[test]
    fn scrolling_stays_inside_the_list() {
        assert_eq!(scrolled(3, -10, 8), 0);
        assert_eq!(scrolled(3, 10, 8), 8);
        assert_eq!(scrolled(3, 2, 8), 5);
        assert!((key_width(1000.0, 10.0) - 280.0).abs() < 1e-3);
        assert!((key_width(300.0, 10.0) - 120.0).abs() < 1e-3);
    }

    #[test]
    fn it_draws_and_reports_how_far_it_scrolls() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 400.0));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let style = Style {
            font: FontId::monospace(13.0),
            row_h: 20.0,
            head: Color32::WHITE,
            keys: Color32::WHITE,
            desc: Color32::GRAY,
            bar: Color32::GRAY,
        };
        let lines: Vec<Line> = (0..50).map(|i| Line::Bind { keys: format!("<C-{i}>"), desc: "d".repeat(200) }).collect();
        let mut drawn = None;
        for _ in 0..2 {
            let mut o = ctx.run_ui(input.clone(), |ui| {
                let body = Rect::from_min_size(pos2(10.0, 10.0), vec2(400.0, 200.0));
                drawn = Some(draw(ui.painter(), body, &lines, 999, &style));
                // The ellipsis, whatever the font.
                let cut = fit(ui.painter(), &"w".repeat(200), &style.font, 100.0);
                assert!(cut.ends_with('…') && cut.chars().count() < 200);
                assert_eq!(fit(ui.painter(), "ab", &style.font, 100.0), "ab");
            });
            o.textures_delta.clear();
        }
        assert_eq!(drawn, Some(Drawn { visible: 10, max: 40, scroll: 40 }));
    }
}
