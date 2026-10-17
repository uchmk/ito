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
    /// A row of a table: the cells of the rows in a run line up in columns
    /// as wide as their widest text (cut with `…` when the rect is too narrow).
    Row(Vec<String>),
    /// [`Line::Bind`] with the raw command at the right edge in the `dim`
    /// colour (cut with `…` first when the row is short of room).
    Cmd { keys: String, desc: String, cmd: String },
    /// A line in the `warn` colour, indented like a binding.
    Warn(String),
    /// A line the app can act on: drawn like a key, and [`hit`] tells which
    /// line the pointer is over (`target` is the app's to read back).
    Link { text: String, target: String },
    /// A line of plain words in the `desc` colour, indented like a binding
    /// (a note between groups, or a paragraph the app wrapped itself).
    Text(String),
    /// [`Line::Link`] in the `warn` colour (a link to something that needs
    /// attention, e.g. a file that failed to load).
    WarnLink { text: String, target: String },
}

/// The lines as plain text, one per line, the cells of a row split by tabs
/// (`keys<TAB>desc<TAB>cmd` for a [`Line::Cmd`]), so that "copy it all" and the
/// drawing cannot disagree.
pub fn text(lines: &[Line]) -> String {
    let mut out = String::new();
    for l in lines {
        match l {
            Line::Head(t) | Line::Warn(t) | Line::Text(t) => out.push_str(t),
            Line::Link { text, .. } | Line::WarnLink { text, .. } => out.push_str(text),
            Line::Bind { keys, desc } => out.push_str(&format!("{keys}\t{desc}")),
            Line::Cmd { keys, desc, cmd } => out.push_str(&format!("{keys}\t{desc}\t{cmd}")),
            Line::Row(cells) => out.push_str(&cells.join("\t")),
            Line::Blank => {}
        }
        out.push('\n');
    }
    out
}

/// The index in `lines` of the line at `pos`, for a list drawn by [`draw`] or
/// [`draw_with`] into `body` from `scroll` (hover and click on a
/// [`Line::Link`]). `None` outside `body` or below the last line.
pub fn hit(body: Rect, len: usize, scroll: usize, style: &Style, pos: Pos2) -> Option<usize> {
    if !body.contains(pos) {
        return None;
    }
    let visible = ((body.height() / style.row_h).floor() as usize).max(1);
    let scroll = scroll.min(len.saturating_sub(visible));
    let row = ((pos.y - body.top()) / style.row_h).floor() as usize;
    (row < visible && scroll + row < len).then_some(scroll + row)
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
    /// [`Line::Warn`].
    pub warn: Color32,
    /// The command of a [`Line::Cmd`].
    pub dim: Color32,
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
    /// Lines out of sight above and below the tallest column. With
    /// [`Overflow::Counts`] the two rows that say so are counted in.
    pub above: usize,
    pub below: usize,
}

/// What shows that the list goes on past the rect.
#[derive(Clone, Copy)]
pub enum Overflow<'a> {
    /// A thin scroll bar at the right edge (what [`draw`] does).
    Bar,
    /// No bar: the first visible row is replaced by `above(n)` when `n` lines
    /// are out of sight above, and the last by `below(n)` for those below.
    /// A page (`Drawn::visible`) can step over the line a marker covers; the
    /// count says how many there are.
    Counts { above: &'a dyn Fn(usize) -> String, below: &'a dyn Fn(usize) -> String },
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
    draw_with(painter, body, lines, scroll, style, Overflow::Bar)
}

/// [`draw`], with the way to show the rest of a long list chosen.
pub fn draw_with(painter: &Painter, body: Rect, lines: &[Line], scroll: usize, style: &Style, overflow: Overflow) -> Drawn {
    let painter = painter.with_clip_rect(body.intersect(painter.clip_rect()));
    let visible = ((body.height() / style.row_h).floor() as usize).max(1);
    let max = lines.len().saturating_sub(visible);
    let scroll = scroll.min(max);
    draw_rows(&painter, body, lines, scroll, visible, style, overflow);
    if let Overflow::Bar = overflow {
        bar(&painter, body, lines.len(), visible, scroll, max, style);
    }
    drawn(visible, max, scroll, overflow)
}

fn drawn(visible: usize, max: usize, scroll: usize, overflow: Overflow) -> Drawn {
    let (mut above, mut below) = (scroll, max - scroll);
    if let Overflow::Counts { .. } = overflow {
        above += usize::from(above > 0);
        below += usize::from(below > 0);
    }
    Drawn { visible, max, scroll, above, below }
}

/// The widths of the columns of each run of [`Line::Row`]s, by line; a run
/// shrinks its widest columns until it fits in `avail`.
fn table_widths(painter: &Painter, lines: &[Line], style: &Style, avail: f32, gap: f32) -> Vec<Option<Vec<f32>>> {
    let mut out: Vec<Option<Vec<f32>>> = vec![None; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        if !matches!(lines[i], Line::Row(_)) {
            i += 1;
            continue;
        }
        let end = (i..lines.len()).find(|&j| !matches!(lines[j], Line::Row(_))).unwrap_or(lines.len());
        let mut w: Vec<f32> = Vec::new();
        for l in &lines[i..end] {
            if let Line::Row(cells) = l {
                for (c, t) in cells.iter().enumerate() {
                    let x = painter.layout_no_wrap(t.clone(), style.font.clone(), style.desc).size().x;
                    if c == w.len() {
                        w.push(x);
                    } else {
                        w[c] = w[c].max(x);
                    }
                }
            }
        }
        let room = (avail - gap * w.len().saturating_sub(1) as f32).max(0.0);
        while w.iter().sum::<f32>() > room + 0.01 {
            let top = w.iter().copied().fold(0.0, f32::max);
            let next = w.iter().copied().filter(|&x| x < top - 0.01).fold(0.0, f32::max);
            let n = w.iter().filter(|&&x| x >= top - 0.01).count() as f32;
            let excess = w.iter().sum::<f32>() - room;
            let cut = (top - next).min(excess / n);
            for x in w.iter_mut().filter(|x| **x >= top - 0.01) {
                *x -= cut;
            }
        }
        for slot in &mut out[i..end] {
            *slot = Some(w.clone());
        }
        i = end;
    }
    out
}

/// `visible` rows of `lines` from `scroll`, into `body`; no scroll bar.
fn draw_rows(painter: &Painter, body: Rect, lines: &[Line], scroll: usize, visible: usize, style: &Style, overflow: Overflow) {
    let cell = painter.layout_no_wrap("M".repeat(10), style.font.clone(), style.keys).size().x / 10.0;
    let key_w = key_width(body.width(), cell);
    let tables = table_widths(painter, lines, style, body.width() - 16.0, cell * 2.0);
    for (i, line) in lines.iter().skip(scroll).take(visible).enumerate() {
        let y = body.top() + i as f32 * style.row_h;
        if let Overflow::Counts { above, below } = overflow {
            let marker = if i == 0 && scroll > 0 {
                Some(above(scroll + 1))
            } else if i + 1 == visible && lines.len() > scroll + visible {
                Some(below(lines.len() - (scroll + visible) + 1))
            } else {
                None
            };
            if let Some(m) = marker {
                let m = fit(painter, &m, &style.font, body.width() - 8.0);
                painter.text(pos2(body.left() + 4.0, y), Align2::LEFT_TOP, m, style.font.clone(), style.desc);
                continue;
            }
        }
        match line {
            Line::Row(cells) => {
                let widths = tables[scroll + i].as_deref().unwrap_or(&[]);
                let mut x = body.left() + 12.0;
                for (c, t) in cells.iter().enumerate() {
                    let w = widths.get(c).copied().unwrap_or(0.0);
                    let color = if c == 0 { style.keys } else { style.desc };
                    painter.text(pos2(x, y), Align2::LEFT_TOP, fit(painter, t, &style.font, w), style.font.clone(), color);
                    x += w + cell * 2.0;
                }
            }
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
            Line::Cmd { keys, desc, cmd } => {
                let k = fit(painter, keys, &style.font, key_w - 16.0);
                painter.text(pos2(body.left() + 12.0, y), Align2::LEFT_TOP, k, style.font.clone(), style.keys);
                let room = body.width() - key_w - 8.0;
                let natural = painter.layout_no_wrap(cmd.clone(), style.font.clone(), style.dim).size().x;
                let cmd_w = natural.min(room * 0.4);
                let c = fit(painter, cmd, &style.font, cmd_w);
                painter.text(pos2(body.right() - 4.0, y), Align2::RIGHT_TOP, c, style.font.clone(), style.dim);
                let d = fit(painter, desc, &style.font, room - cmd_w - cell);
                painter.text(pos2(body.left() + key_w, y), Align2::LEFT_TOP, d, style.font.clone(), style.desc);
            }
            Line::Warn(t) => {
                let t = fit(painter, t, &style.font, body.width() - 16.0);
                painter.text(pos2(body.left() + 12.0, y), Align2::LEFT_TOP, t, style.font.clone(), style.warn);
            }
            Line::Link { text, .. } => {
                let t = fit(painter, text, &style.font, body.width() - 16.0);
                painter.text(pos2(body.left() + 12.0, y), Align2::LEFT_TOP, t, style.font.clone(), style.keys);
            }
            Line::Text(t) => {
                let t = fit(painter, t, &style.font, body.width() - 16.0);
                painter.text(pos2(body.left() + 12.0, y), Align2::LEFT_TOP, t, style.font.clone(), style.desc);
            }
            Line::WarnLink { text, .. } => {
                let t = fit(painter, text, &style.font, body.width() - 16.0);
                painter.text(pos2(body.left() + 12.0, y), Align2::LEFT_TOP, t, style.font.clone(), style.warn);
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
    draw_plan_with(painter, body, plan, scroll, style, Overflow::Bar)
}

/// [`draw_plan`], with the way to show the rest of a long list chosen. Each
/// column that runs past the rect gets its own markers.
pub fn draw_plan_with(painter: &Painter, body: Rect, plan: &Plan, scroll: usize, style: &Style, overflow: Overflow) -> Drawn {
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
        draw_rows(&painter, rect, col, scroll, visible, &st, overflow);
    }
    if let Overflow::Bar = overflow {
        bar(&painter, body, total, visible, scroll, max, &st);
    }
    drawn(visible, max, scroll, overflow)
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
            warn: Color32::RED,
            dim: Color32::DARK_GRAY,
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
        assert_eq!(drawn, Some(Drawn { visible: 10, max: 40, scroll: 40, above: 40, below: 0 }));
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
            warn: Color32::RED,
            dim: Color32::DARK_GRAY,
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

    fn lay(lines: &[Line], scroll: usize, overflow: Overflow) -> (Drawn, Vec<String>) {
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 400.0))), ..Default::default() };
        let style = Style {
            font: FontId::monospace(13.0),
            row_h: 20.0,
            head: Color32::WHITE,
            keys: Color32::WHITE,
            desc: Color32::GRAY,
            warn: Color32::RED,
            dim: Color32::DARK_GRAY,
            bar: Color32::GRAY,
        };
        let (mut d, mut texts) = (None, Vec::new());
        for _ in 0..2 {
            let o = ctx.run_ui(input.clone(), |ui| {
                d = Some(draw_with(
                    ui.painter(),
                    Rect::from_min_size(pos2(10.0, 10.0), vec2(300.0, 100.0)),
                    lines,
                    scroll,
                    &style,
                    overflow,
                ));
            });
            texts = o
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            o.drop_without_applying_deltas();
        }
        (d.unwrap(), texts)
    }

    #[test]
    fn counts_replace_the_bar_and_say_how_many_lines_are_out_of_sight() {
        let lines: Vec<Line> = (0..20).map(|i| Line::Head(format!("l{i}"))).collect();
        let up = |n: usize| format!("up {n}");
        let down = |n: usize| format!("down {n}");
        let counts = Overflow::Counts { above: &up, below: &down };
        // 5 rows fit: lines 0..5 with the last covered by "down 16" (lines 4..19 are out of sight).
        let (d, t) = lay(&lines, 0, counts);
        assert_eq!(t, ["l0", "l1", "l2", "l3", "down 16"]);
        assert_eq!((d.above, d.below), (0, 16));
        let (d, t) = lay(&lines, 4, counts);
        assert_eq!(t, ["up 5", "l5", "l6", "l7", "down 12"]);
        assert_eq!((d.above, d.below), (5, 12));
        let (d, t) = lay(&lines, 99, counts);
        assert_eq!(t, ["up 16", "l16", "l17", "l18", "l19"]);
        assert_eq!((d.above, d.below), (16, 0));
        // The bar shows no words.
        assert_eq!(lay(&lines, 4, Overflow::Bar).1, ["l4", "l5", "l6", "l7", "l8"]);
    }

    #[test]
    fn table_rows_line_their_cells_up_by_the_widest_in_the_run() {
        let row = |a: &str, b: &str| Line::Row(vec![a.into(), b.into()]);
        let lines = vec![row("a", "x"), row("long one", "y"), Line::Blank, row("c", "z")];
        let (_, t) = lay(&lines, 0, Overflow::Bar);
        assert_eq!(t, ["a", "x", "long one", "y", "c", "z"]);
        let ctx = egui::Context::default();
        let style = Style {
            font: FontId::monospace(13.0),
            row_h: 20.0,
            head: Color32::WHITE,
            keys: Color32::WHITE,
            desc: Color32::GRAY,
            warn: Color32::RED,
            dim: Color32::DARK_GRAY,
            bar: Color32::GRAY,
        };
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 400.0))), ..Default::default() };
        let mut w = Vec::new();
        for _ in 0..2 {
            let o = ctx.run_ui(input.clone(), |ui| {
                w = table_widths(ui.painter(), &lines, &style, 300.0, 20.0);
            });
            o.drop_without_applying_deltas();
        }
        let (first, last) = (w[0].clone().unwrap(), w[3].clone().unwrap());
        assert_eq!(w[0], w[1], "one run, one set of widths");
        assert!(first[0] > last[0], "the separate run is as wide as its own widest cell");
        // Too narrow: the widest column gives way and the sum fits.
        let mut narrow = Vec::new();
        for _ in 0..2 {
            let o = ctx.run_ui(input.clone(), |ui| {
                narrow = table_widths(ui.painter(), &lines, &style, 60.0, 10.0);
            });
            o.drop_without_applying_deltas();
        }
        assert!(narrow[0].as_ref().unwrap().iter().sum::<f32>() <= 50.01);
    }

    #[test]
    fn plain_text_and_warning_links_are_lines_too() {
        let lines = vec![Line::Text("a note".into()), Line::WarnLink { text: "bad.toml".into(), target: "bad.toml".into() }];
        let (_, t) = lay(&lines, 0, Overflow::Bar);
        assert_eq!(t, ["a note", "bad.toml"]);
        assert_eq!(
            text(&lines),
            "a note
bad.toml
"
        );
        let style = Style {
            font: FontId::monospace(13.0),
            row_h: 20.0,
            head: Color32::WHITE,
            keys: Color32::WHITE,
            desc: Color32::GRAY,
            warn: Color32::RED,
            dim: Color32::DARK_GRAY,
            bar: Color32::GRAY,
        };
        let body = Rect::from_min_size(pos2(10.0, 10.0), vec2(300.0, 100.0));
        assert_eq!(hit(body, lines.len(), 0, &style, pos2(20.0, 32.0)), Some(1));
    }

    #[test]
    fn commands_warnings_and_links_are_drawn_and_told_as_text() {
        let lines = vec![
            Line::Link { text: "/etc/kura".into(), target: "/etc/kura".into() },
            Line::Warn("bad toml".into()),
            Line::Cmd { keys: "gh".into(), desc: "home".into(), cmd: "cd ~".into() },
        ];
        let (_, t) = lay(&lines, 0, Overflow::Bar);
        assert_eq!(t.len(), 5);
        for want in ["/etc/kura", "bad toml", "gh", "home", "cd ~"] {
            assert!(t.iter().any(|x| x == want), "{want}");
        }
        assert_eq!(
            text(&lines),
            "/etc/kura
bad toml
gh	home	cd ~
"
        );
        assert_eq!(
            text(&[Line::Bind { keys: "q".into(), desc: "quit".into() }, Line::Blank]),
            "q	quit

"
        );
    }

    #[test]
    fn hit_finds_the_line_under_the_pointer() {
        let style = Style {
            font: FontId::monospace(13.0),
            row_h: 20.0,
            head: Color32::WHITE,
            keys: Color32::WHITE,
            desc: Color32::GRAY,
            warn: Color32::RED,
            dim: Color32::DARK_GRAY,
            bar: Color32::GRAY,
        };
        let body = Rect::from_min_size(pos2(10.0, 10.0), vec2(300.0, 100.0));
        assert_eq!(hit(body, 20, 0, &style, pos2(50.0, 12.0)), Some(0));
        assert_eq!(hit(body, 20, 3, &style, pos2(50.0, 35.0)), Some(4));
        assert_eq!(hit(body, 2, 0, &style, pos2(50.0, 70.0)), None);
        assert_eq!(hit(body, 20, 0, &style, pos2(5.0, 12.0)), None);
        assert_eq!(hit(body, 20, 99, &style, pos2(50.0, 12.0)), Some(15));
    }
}
