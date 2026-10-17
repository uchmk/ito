//! The keys help: what each layer of the keymap binds, as headings and
//! `keys  description` rows, drawn into a rect the app gives and scrolled by
//! a line index the app keeps.
//!
//! The list comes from [`ito_keymap::help_lines`] (the keys that do the same
//! thing already on one line); [`push_layer`] turns a layer into [`Line`]s.
//! A help of titled [`Group`]s that has to fit without scrolling goes through
//! [`plan`] (columns, then smaller letters) and [`draw_plan`].
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
    draw_rows(&painter, body, lines, scroll, visible, style);
    bar(&painter, body, lines.len(), visible, scroll, max, style);
    Drawn { visible, max, scroll }
}

/// `visible` rows of `lines` from `scroll`, into `body`; no scroll bar.
fn draw_rows(painter: &Painter, body: Rect, lines: &[Line], scroll: usize, visible: usize, style: &Style) {
    let cell = painter.layout_no_wrap("M".repeat(10), style.font.clone(), style.keys).size().x / 10.0;
    let key_w = key_width(body.width(), cell);
    for (i, line) in lines.iter().skip(scroll).take(visible).enumerate() {
        let y = body.top() + i as f32 * style.row_h;
        match line {
            Line::Head(t) => {
                painter.text(
                    pos2(body.left() + 4.0, y),
                    Align2::LEFT_TOP,
                    fit(painter, t, &style.font, body.width() - 8.0),
                    style.font.clone(),
                    style.head,
                );
            }
            Line::Bind { keys, desc } => {
                let k = fit(painter, keys, &style.font, key_w - 16.0);
                painter.text(pos2(body.left() + 12.0, y), Align2::LEFT_TOP, k, style.font.clone(), style.keys);
                let d = fit(painter, desc, &style.font, body.width() - key_w - 8.0);
                painter.text(pos2(body.left() + key_w, y), Align2::LEFT_TOP, d, style.font.clone(), style.desc);
            }
            Line::Blank => {}
        }
    }
}

/// Where the view sits in a list of `total` lines, when it is longer than the view.
fn bar(painter: &Painter, body: Rect, total: usize, visible: usize, scroll: usize, max: usize, style: &Style) {
    if max > 0 {
        let track = Rect::from_min_max(pos2(body.right() - 3.0, body.top()), pos2(body.right(), body.bottom()));
        let th = (track.height() * visible as f32 / total as f32).max(12.0);
        let ty = track.top() + (track.height() - th) * scroll as f32 / max as f32;
        painter.rect_filled(Rect::from_min_size(Pos2::new(track.left(), ty), vec2(3.0, th)), 1.5, style.bar);
    }
}

/// A titled group of `(keys, description)` rows, for a help that is laid out
/// in columns by [`plan`] rather than listed from a keymap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

impl Group {
    /// Its lines: the heading, then a row each.
    pub fn lines(&self) -> Vec<Line> {
        let head = Line::Head(self.title.clone());
        std::iter::once(head).chain(self.rows.iter().map(|(k, d)| Line::Bind { keys: k.clone(), desc: d.clone() })).collect()
    }
}

/// `groups` dealt, in order and whole, into at most `cols` columns of about
/// equal height (the tallest as short as it can be); a blank line separates
/// the groups of one column. Fewer columns than `cols` when there are fewer
/// groups.
pub fn deal(groups: &[Group], cols: usize) -> Vec<Vec<Line>> {
    let cols = cols.max(1);
    let heights: Vec<usize> = groups.iter().map(|g| g.rows.len() + 1).collect();
    // Whether `cols` columns can hold the groups with none taller than `limit`.
    let holds = |limit: usize| {
        let (mut used, mut h) = (1, 0);
        for &g in &heights {
            if h == 0 {
                h = g;
            } else if h + 1 + g <= limit {
                h += 1 + g;
            } else {
                used += 1;
                h = g;
            }
        }
        used <= cols
    };
    let (mut lo, mut hi) = (heights.iter().copied().max().unwrap_or(0), heights.iter().sum::<usize>() + heights.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if holds(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let mut out: Vec<Vec<Line>> = Vec::new();
    for (g, &gh) in groups.iter().zip(&heights) {
        match out.last_mut() {
            Some(col) if col.len() + 1 + gh <= lo => {
                col.push(Line::Blank);
                col.extend(g.lines());
            }
            _ => out.push(g.lines()),
        }
    }
    out
}

/// How many columns of at least `min_col` wide fit in `width` (1 to `max`).
pub fn columns_for(width: f32, min_col: f32, max: usize) -> usize {
    ((width / min_col.max(1.0)).floor() as usize).clamp(1, max.max(1))
}

/// How [`plan`] laid the help out.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub columns: Vec<Vec<Line>>,
    /// The letters' size, 1.0 down to the floor given.
    pub scale: f32,
    /// Still taller than the room at the floor: the list scrolls.
    pub scrolls: bool,
}

/// Fit `groups` in `room` tall without scrolling if it can be done: one
/// column, then one more at a time up to `max_cols`, then the letters smaller
/// down to `floor` (0.7 is readable); below that it scrolls. Rows are cut with
/// `…`, never wrapped, so the height is known without drawing.
pub fn plan(groups: &[Group], room: f32, row_h: f32, max_cols: usize, floor: f32) -> Plan {
    let mut columns = Vec::new();
    for cols in 1..=max_cols.max(1) {
        columns = deal(groups, cols);
        let tall = columns.iter().map(Vec::len).max().unwrap_or(0) as f32 * row_h;
        if tall <= room {
            return Plan { columns, scale: 1.0, scrolls: false };
        }
    }
    let tall = columns.iter().map(Vec::len).max().unwrap_or(0) as f32 * row_h;
    let floor = floor.clamp(0.1, 1.0);
    let scale = (room / tall).clamp(floor, 1.0);
    Plan { scrolls: tall * scale > room + 0.01, columns, scale }
}

/// Draw a [`Plan`]'s columns side by side into `body`, from line `scroll`,
/// with `style` scaled by `plan.scale`. The scroll bar is the body's right edge.
pub fn draw_plan(painter: &Painter, body: Rect, plan: &Plan, scroll: usize, style: &Style) -> Drawn {
    let painter = painter.with_clip_rect(body.intersect(painter.clip_rect()));
    let mut st = style.clone();
    st.font.size *= plan.scale;
    st.row_h *= plan.scale;
    let visible = ((body.height() / st.row_h).floor() as usize).max(1);
    let total = plan.columns.iter().map(Vec::len).max().unwrap_or(0);
    let max = total.saturating_sub(visible);
    let scroll = scroll.min(max);
    let n = plan.columns.len().max(1);
    let gutter = 16.0;
    let w = (body.width() - gutter * (n - 1) as f32) / n as f32;
    for (i, col) in plan.columns.iter().enumerate() {
        let left = body.left() + i as f32 * (w + gutter);
        let rect = Rect::from_min_size(pos2(left, body.top()), vec2(w, body.height()));
        draw_rows(&painter, rect, col, scroll, visible, &st);
    }
    bar(&painter, body, total, visible, scroll, max, &st);
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

    fn group(title: &str, n: usize) -> Group {
        Group { title: title.into(), rows: (0..n).map(|i| (format!("k{i}"), "d".into())).collect() }
    }

    #[test]
    fn groups_are_dealt_whole_and_in_order_to_even_columns() {
        let g = [group("A", 5), group("B", 2), group("C", 2), group("D", 5)];
        let c = deal(&g, 2);
        assert_eq!(c.len(), 2);
        // A + B is 6 + 1 + 3 lines, C + D is 3 + 1 + 6.
        assert_eq!((c[0].len(), c[1].len()), (10, 10));
        assert_eq!(c[0][0], Line::Head("A".into()));
        assert_eq!(c[1][0], Line::Head("C".into()));
        assert_eq!(deal(&g, 9).len(), 4, "no more columns than groups");
        assert_eq!(deal(&g, 1)[0].len(), 6 + 1 + 3 + 1 + 3 + 1 + 6);
        assert!(deal(&[], 3).is_empty());
    }

    #[test]
    fn the_plan_adds_columns_then_shrinks_then_scrolls() {
        let g = [group("A", 9), group("B", 9), group("C", 9), group("D", 9)];
        // 43 lines tall in one column, 21 in two, 10 in four.
        let p = plan(&g, 440.0, 10.0, 4, 0.7);
        assert_eq!((p.columns.len(), p.scale, p.scrolls), (1, 1.0, false));
        let p = plan(&g, 220.0, 10.0, 4, 0.7);
        assert_eq!((p.columns.len(), p.scale, p.scrolls), (2, 1.0, false));
        let p = plan(&g, 80.0, 10.0, 4, 0.7);
        assert_eq!(p.columns.len(), 4);
        assert!((p.scale - 0.8).abs() < 1e-6 && !p.scrolls);
        let p = plan(&g, 50.0, 10.0, 4, 0.7);
        assert!((p.scale - 0.7).abs() < 1e-6 && p.scrolls);
        assert_eq!(columns_for(1000.0, 300.0, 4), 3);
        assert_eq!(columns_for(100.0, 300.0, 4), 1);
    }

    #[test]
    fn a_plan_draws_and_reports_how_far_it_scrolls() {
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 400.0))), ..Default::default() };
        let style = Style {
            font: FontId::monospace(13.0),
            row_h: 20.0,
            head: Color32::WHITE,
            keys: Color32::WHITE,
            desc: Color32::GRAY,
            bar: Color32::GRAY,
        };
        let p = plan(&[group("A", 30), group("B", 30)], 100.0, 20.0, 2, 0.5);
        assert!(p.scrolls);
        let mut drawn = None;
        for _ in 0..2 {
            let mut o = ctx.run_ui(input.clone(), |ui| {
                drawn = Some(draw_plan(ui.painter(), Rect::from_min_size(pos2(10.0, 10.0), vec2(500.0, 100.0)), &p, 999, &style));
            });
            o.textures_delta.clear();
        }
        let d = drawn.unwrap();
        assert_eq!((d.visible, d.max, d.scroll), (10, 21, 21), "31 lines at a scale of 0.5, 10 rows of 10");
    }
}
