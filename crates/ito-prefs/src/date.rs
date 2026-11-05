//! Date fields: a text field for `YYYY-MM-DD` (typing and pasting still work) with a calendar button that opens a month grid under it.
//!
//! [`DateField`] is one date, [`DateField::multi`] a comma-separated list (a click toggles a day and the grid stays open),
//! [`RangeField`] a start and an end (the first click is the start, the second the end), and [`month_picker`] a year and twelve
//! months for jumping far. All four follow the theme, read aloud through `ito-a11y`, and answer `<Click:2026-11-05>` of `--keys`.
//!
//! Keys in an open grid: arrows move a day or a week, PageUp/PageDown a month, Home goes to today, Enter picks, Esc closes.
//! Alt+Down in the field opens it.

use crate::{focus_ring, mark, z, Look, Mark};
use chrono::{Datelike, Days, Months, NaiveDate};
use egui::{Align, Align2, Color32, Context, FontId, Id, Key, Modifiers, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};
use ito_a11y::Role as Said;
use ito_theme::mix;

/// The words of the grid, in the language in force. `heading` has `{y}` (year), `{m}` (month number) and `{month}` (a name from
/// `months`); `weekdays` start at Sunday.
#[derive(Clone, Debug)]
pub struct DateWords<'a> {
    pub months: [&'a str; 12],
    pub weekdays: [&'a str; 7],
    pub heading: &'a str,
    pub year: &'a str,
    pub today: &'a str,
    pub clear: &'a str,
    pub open: &'a str,
    pub prev: &'a str,
    pub next: &'a str,
    pub hint: &'a str,
}

impl DateWords<'static> {
    pub fn en() -> Self {
        DateWords {
            months: [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
            weekdays: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
            heading: "{month} {y}",
            year: "{y}",
            today: "Today",
            clear: "Clear",
            open: "Open the calendar",
            prev: "Previous",
            next: "Next",
            hint: "YYYY-MM-DD",
        }
    }

    pub fn ja() -> Self {
        DateWords {
            months: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
            weekdays: ["日", "月", "火", "水", "木", "金", "土"],
            heading: "{y}年{month}",
            year: "{y}年",
            today: "今日",
            clear: "消す",
            open: "カレンダーを開く",
            prev: "前",
            next: "次",
            hint: "YYYY-MM-DD",
        }
    }

    /// The words for a language code (`ja`, else English).
    pub fn for_lang(lang: &str) -> Self {
        if lang.starts_with("ja") {
            Self::ja()
        } else {
            Self::en()
        }
    }
}

impl DateWords<'_> {
    fn heading_of(&self, d: NaiveDate) -> String {
        self.heading
            .replace("{month}", self.months[d.month0() as usize])
            .replace("{m}", &d.month().to_string())
            .replace("{y}", &d.year().to_string())
    }
}

/// How a day is tinted (a holiday in red, a day with jobs in the accent) and what its tip says.
#[derive(Clone, Debug, Default)]
pub struct DayLook {
    pub ink: Option<Color32>,
    pub tip: Option<String>,
}

type DayLooker<'a> = &'a dyn Fn(NaiveDate) -> Option<DayLook>;

/// `2026-11-05`.
pub fn format_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// `2026-11-05` (also `2026/11/5`), spaces round it ignored.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let mut it = s.trim().split(['-', '/']);
    let (y, m, d) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || y.len() != 4 || m.is_empty() || m.len() > 2 || d.is_empty() || d.len() > 2 {
        return None;
    }
    NaiveDate::from_ymd_opt(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
}

/// The days of a comma-separated list, and the words that are not dates.
fn split_dates(s: &str) -> (Vec<NaiveDate>, Vec<String>) {
    let (mut ok, mut bad) = (Vec::new(), Vec::new());
    for part in s.split([',', '、', '\n']).map(str::trim).filter(|p| !p.is_empty()) {
        match parse_date(part) {
            Some(d) => ok.push(d),
            None => bad.push(part.to_owned()),
        }
    }
    (ok, bad)
}

/// `d` moved by `n` months, the day kept or cut to the end of that month.
pub fn shift_months(d: NaiveDate, n: i32) -> NaiveDate {
    let m = Months::new(n.unsigned_abs());
    (if n >= 0 { d.checked_add_months(m) } else { d.checked_sub_months(m) }).unwrap_or(d)
}

fn shift_days(d: NaiveDate, n: i64) -> NaiveDate {
    let k = Days::new(n.unsigned_abs());
    (if n >= 0 { d.checked_add_days(k) } else { d.checked_sub_days(k) }).unwrap_or(d)
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap_or(d)
}

/// The six weeks that show the month of `d`.
pub fn month_grid(d: NaiveDate, monday_first: bool) -> [[NaiveDate; 7]; 6] {
    let first = first_of_month(d);
    let back = if monday_first { first.weekday().num_days_from_monday() } else { first.weekday().num_days_from_sunday() };
    let start = shift_days(first, -(back as i64));
    std::array::from_fn(|w| std::array::from_fn(|i| shift_days(start, (w * 7 + i) as i64)))
}

fn today_local() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// What to do with the text of a field: empty is fine only when `empty_ok`; a date must parse and lie in `min..=max`.
fn text_ok(text: &str, empty_ok: bool, range: (Option<NaiveDate>, Option<NaiveDate>), multi: bool) -> bool {
    if text.trim().is_empty() {
        return empty_ok;
    }
    let in_range = |d: NaiveDate| range.0.is_none_or(|a| d >= a) && range.1.is_none_or(|b| d <= b);
    if multi {
        let (ok, bad) = split_dates(text);
        bad.is_empty() && ok.iter().all(|d| in_range(*d))
    } else {
        parse_date(text).is_some_and(in_range)
    }
}

#[derive(Clone, Debug)]
struct Pop {
    open: bool,
    /// A day of the month shown.
    shown: NaiveDate,
    /// The day the keys are on.
    cursor: NaiveDate,
    picker: bool,
    hover: Option<NaiveDate>,
    /// A range's start waiting for its end.
    pending: Option<NaiveDate>,
}

impl Pop {
    fn load(ctx: &Context, id: Id, today: NaiveDate) -> Pop {
        ctx.data(|d| d.get_temp::<Pop>(id)).unwrap_or(Pop {
            open: false,
            shown: today,
            cursor: today,
            picker: false,
            hover: None,
            pending: None,
        })
    }

    fn store(&self, ctx: &Context, id: Id) {
        ctx.data_mut(|d| d.insert_temp(id, self.clone()));
    }

    fn open_at(&mut self, d: NaiveDate) {
        self.open = true;
        self.picker = false;
        self.cursor = d;
        self.shown = d;
    }
}

/// What is chosen, for the grid to light.
enum Sel<'a> {
    One(Option<NaiveDate>),
    Many(&'a [NaiveDate]),
    Range(Option<NaiveDate>, Option<NaiveDate>),
}

struct Grid<'a> {
    words: &'a DateWords<'a>,
    min: Option<NaiveDate>,
    max: Option<NaiveDate>,
    monday_first: bool,
    today: NaiveDate,
    look: Option<DayLooker<'a>>,
    /// Only the year and months (a [`month_picker`]): Esc shuts it.
    picker_only: bool,
}

impl Grid<'_> {
    fn allowed(&self, d: NaiveDate) -> bool {
        self.min.is_none_or(|a| d >= a) && self.max.is_none_or(|b| d <= b)
    }
}

fn nav_button(ui: &mut Ui, id: Id, m: Mark, said: &str, side: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let r = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(said);
    ito_a11y::say(&r, Said::Button, said);
    let c = ito_theme::colors();
    if r.hovered() {
        ui.painter().rect_filled(rect, 6.0, c.raised());
    }
    mark(ui.painter(), rect.shrink(side * 0.25), m, if r.hovered() { c.strong() } else { c.dim });
    r
}

/// A text button of the grid (the heading, "Today").
fn word_button(ui: &mut Ui, id: Id, text: &str, said: &str, w: f32, h: f32, ink: Color32) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let r = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    ito_a11y::say(&r, Said::Button, said);
    let c = ito_theme::colors();
    if r.hovered() {
        ui.painter().rect_filled(rect, 6.0, c.raised());
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(z(ui, 13.0)),
        if r.hovered() { c.strong() } else { ink },
    );
    r
}

/// Draws the open grid under `anchor` and handles its keys; the day picked, if any.
fn popup(ctx: &Context, l: Look, id: Id, anchor: Rect, g: &Grid, sel: &Sel, st: &mut Pop) -> Option<NaiveDate> {
    if !st.open {
        return None;
    }
    let c = l.c;
    let mut picked = None;
    // Keys first, so the text field under does not also move its cursor.
    let key = |k: Key, m: Modifiers| ctx.input_mut(|i| i.consume_key(m, k));
    if key(Key::Escape, Modifiers::NONE) {
        if st.picker && !g.picker_only {
            st.picker = false;
        } else {
            st.open = false;
            st.pending = None;
        }
        return None;
    }
    if !st.picker {
        let go = |d: NaiveDate, st: &mut Pop| {
            st.cursor = d;
            st.shown = d;
        };
        if key(Key::ArrowLeft, Modifiers::NONE) {
            go(shift_days(st.cursor, -1), st);
        }
        if key(Key::ArrowRight, Modifiers::NONE) {
            go(shift_days(st.cursor, 1), st);
        }
        if key(Key::ArrowUp, Modifiers::NONE) {
            go(shift_days(st.cursor, -7), st);
        }
        if key(Key::ArrowDown, Modifiers::NONE) {
            go(shift_days(st.cursor, 7), st);
        }
        if key(Key::PageUp, Modifiers::NONE) {
            go(shift_months(st.cursor, -1), st);
        }
        if key(Key::PageDown, Modifiers::NONE) {
            go(shift_months(st.cursor, 1), st);
        }
        if key(Key::Home, Modifiers::NONE) {
            go(g.today, st);
        }
        if key(Key::Enter, Modifiers::NONE) && g.allowed(st.cursor) {
            picked = Some(st.cursor);
        }
    }
    let zoom = ito_theme::zoomed(ctx, 1.0);
    let cell = 30.0 * zoom;
    let pad = 10.0 * zoom;
    let width = cell * 7.0 + pad * 2.0;
    let area = egui::Area::new(id.with("grid"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor.left_bottom() + Vec2::new(0.0, 4.0))
        .constrain(true)
        .show(ctx, |ui| {
            let frame = egui::Frame::new()
                .fill(c.side)
                .stroke(Stroke::new(1.0, c.border_strong()))
                .corner_radius(ito_theme::scale::radius::BOX)
                .inner_margin(pad)
                .shadow(egui::Shadow { offset: [0, 6], blur: 20, spread: 1, color: Color32::from_black_alpha(80) });
            frame.show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.set_width(width - pad * 2.0);
                let words = g.words;
                let side = 26.0 * zoom;
                // Head: previous, the heading (a button to the picker), next.
                ui.horizontal(|ui| {
                    let (step, said_prev, said_next) = if st.picker { (12, words.prev, words.next) } else { (1, words.prev, words.next) };
                    if nav_button(ui, id.with("prev"), Mark::ChevronLeft, said_prev, side).clicked() {
                        st.shown = shift_months(st.shown, -step);
                    }
                    let mid = ui.available_width() - side;
                    let title =
                        if st.picker { words.year.replace("{y}", &st.shown.year().to_string()) } else { words.heading_of(st.shown) };
                    if word_button(ui, id.with("title"), &title, &title, mid, side, c.strong()).clicked() && !st.picker {
                        st.picker = true;
                    }
                    if nav_button(ui, id.with("next"), Mark::ChevronRight, said_next, side).clicked() {
                        st.shown = shift_months(st.shown, step);
                    }
                });
                ui.add_space(4.0 * zoom);
                if st.picker {
                    for row in 0..4 {
                        ui.horizontal(|ui| {
                            for col in 0..3 {
                                let m0 = row * 3 + col;
                                let w = (width - pad * 2.0) / 3.0;
                                let here = st.shown.month0() == m0 as u32;
                                let name = words.months[m0];
                                let ink = if here { c.run } else { c.fg };
                                if word_button(ui, id.with(("month", m0)), name, name, w, cell * 1.2, ink).clicked() {
                                    st.shown = st.shown.with_day(1).and_then(|d| d.with_month0(m0 as u32)).unwrap_or(st.shown);
                                    st.cursor = st.shown;
                                    st.picker = false;
                                }
                            }
                        });
                    }
                } else {
                    ui.horizontal(|ui| {
                        for i in 0..7 {
                            let wd = if g.monday_first { (i + 1) % 7 } else { i };
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(cell, cell * 0.8), Sense::hover());
                            let ink = match wd {
                                0 => mix(c.dim, c.err, 0.5),
                                6 => mix(c.dim, c.blue, 0.5),
                                _ => c.dim,
                            };
                            ui.painter().text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                words.weekdays[wd],
                                FontId::proportional(z(ui, 11.0)),
                                ink,
                            );
                        }
                    });
                    let weeks = month_grid(st.shown, g.monday_first);
                    let span = |d: NaiveDate, hover: Option<NaiveDate>| -> bool {
                        match sel {
                            Sel::Range(a, b) => match (st.pending.or(*a), if st.pending.is_some() { hover } else { *b }) {
                                (Some(x), Some(y)) => d >= x.min(y) && d <= x.max(y),
                                _ => false,
                            },
                            _ => false,
                        }
                    };
                    let is_sel = |d: NaiveDate| match sel {
                        Sel::One(x) => *x == Some(d),
                        Sel::Many(xs) => xs.contains(&d),
                        Sel::Range(a, b) => *a == Some(d) || *b == Some(d) || st.pending == Some(d),
                    };
                    let hover_was = st.hover;
                    let mut hover_now = None;
                    for week in weeks {
                        ui.horizontal(|ui| {
                            for d in week {
                                let (rect, _) = ui.allocate_exact_size(Vec2::splat(cell), Sense::hover());
                                let allowed = g.allowed(d);
                                let r = ui.interact(
                                    rect,
                                    id.with(("day", d.num_days_from_ce())),
                                    if allowed { Sense::click() } else { Sense::hover() },
                                );
                                let name = format!("{} {}", format_date(d), words.weekdays[d.weekday().num_days_from_sunday() as usize]);
                                ito_a11y::say(&r, Said::Button, &name);
                                let look = g.look.and_then(|f| f(d));
                                if let Some(tip) = look.as_ref().and_then(|x| x.tip.clone()) {
                                    r.clone().on_hover_text(tip);
                                }
                                if r.hovered() && allowed {
                                    hover_now = Some(d);
                                }
                                let inside = d.month() == st.shown.month();
                                let picked_day = is_sel(d);
                                let p = ui.painter();
                                let box_ = rect.shrink(1.5 * zoom);
                                if span(d, hover_was) && !picked_day {
                                    p.rect_filled(rect.shrink2(Vec2::new(0.0, 3.0 * zoom)), 0.0, mix(c.side, c.run, 0.18));
                                }
                                if picked_day {
                                    p.rect_filled(box_, 6.0, c.run);
                                } else if r.hovered() && allowed {
                                    p.rect_filled(box_, 6.0, c.raised());
                                }
                                if d == g.today {
                                    p.rect_stroke(
                                        box_,
                                        6.0,
                                        Stroke::new(1.5, if picked_day { c.strong() } else { c.run }),
                                        StrokeKind::Inside,
                                    );
                                }
                                if d == st.cursor {
                                    p.rect_stroke(box_.expand(1.0), 7.0, Stroke::new(1.0, c.strong()), StrokeKind::Outside);
                                }
                                let tint = look.and_then(|x| x.ink);
                                let ink = if picked_day {
                                    c.bg
                                } else if !allowed {
                                    mix(c.side, c.dim, 0.35)
                                } else if let Some(t) = tint {
                                    if inside {
                                        t
                                    } else {
                                        mix(c.side, t, 0.5)
                                    }
                                } else if inside {
                                    c.fg
                                } else {
                                    mix(c.side, c.dim, 0.6)
                                };
                                p.text(rect.center(), Align2::CENTER_CENTER, d.day().to_string(), FontId::proportional(z(ui, 13.0)), ink);
                                if r.clicked() {
                                    st.cursor = d;
                                    picked = Some(d);
                                }
                            }
                        });
                    }
                    st.hover = hover_now;
                }
                ui.add_space(6.0 * zoom);
                ui.horizontal(|ui| {
                    if word_button(ui, id.with("today"), words.today, words.today, 60.0 * zoom, side, c.run).clicked() {
                        st.cursor = g.today;
                        st.shown = g.today;
                        st.picker = false;
                    }
                });
            });
        });
    // A press outside the grid and its field shuts it.
    let outside = ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p))
    });
    if outside {
        st.open = false;
        st.pending = None;
    }
    picked
}

/// The result of a date field.
#[derive(Debug)]
pub struct DateResponse {
    /// The text field.
    pub response: Response,
    /// The text changed, typed or picked.
    pub changed: bool,
    /// The text is fine (a date in range, or empty where that is allowed).
    pub valid: bool,
    /// The grid is open.
    pub open: bool,
}

/// One text field with its calendar button: the text, whether the button was clicked, whether the text was cleared.
struct Input<'a> {
    id: Id,
    words: &'a DateWords<'a>,
    width: f32,
    valid: bool,
    clearable: bool,
    /// The spoken name of the text field; empty says the hint.
    name: &'a str,
}

struct Shown {
    text: Response,
    outer: Rect,
    button: bool,
    changed: bool,
    cleared: bool,
}

fn input(ui: &mut Ui, l: Look, i: &Input, text: &mut String) -> Shown {
    let c = l.c;
    let h = z(ui, 28.0);
    let (outer, _) = ui.allocate_exact_size(Vec2::new(i.width, h), Sense::hover());
    let text_id = i.id.with("text");
    let focused = ui.memory(|m| m.has_focus(text_id));
    let edge = if !i.valid {
        c.err
    } else if focused {
        c.run
    } else {
        c.border_strong()
    };
    ui.painter().rect(outer, 6.0, c.panel, Stroke::new(1.0, edge), StrokeKind::Inside);
    let side = h - 4.0;
    let button_rect = Rect::from_min_size(Pos2::new(outer.right() - side - 2.0, outer.top() + 2.0), Vec2::splat(side));
    let clear_rect = Rect::from_min_size(Pos2::new(button_rect.left() - side, outer.top() + 2.0), Vec2::splat(side));
    let has_clear = i.clearable && !text.is_empty();
    let text_rect = Rect::from_min_max(outer.min, Pos2::new(if has_clear { clear_rect.left() } else { button_rect.left() }, outer.max.y));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let r = child.add(
        egui::TextEdit::singleline(text)
            .id(text_id)
            .frame(egui::Frame::NONE)
            .hint_text(i.words.hint)
            .desired_width(text_rect.width())
            .margin(Vec2::new(8.0, 4.0))
            .vertical_align(Align::Center),
    );
    ito_a11y::say(&r, Said::Label, if i.name.is_empty() { i.words.hint } else { i.name });
    let mut cleared = false;
    if has_clear {
        let x = ui.interact(clear_rect, i.id.with("clear"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        ito_a11y::say(&x, Said::Button, i.words.clear);
        mark(ui.painter(), clear_rect.shrink(side * 0.3), Mark::Close, if x.hovered() { c.err } else { c.dim });
        cleared = x.clicked();
    }
    let b = ui
        .interact(button_rect, i.id.with("open"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(i.words.open);
    ito_a11y::say(&b, Said::Button, i.words.open);
    if b.hovered() {
        ui.painter().rect_filled(button_rect, 5.0, c.raised());
    }
    mark(ui.painter(), button_rect.shrink(side * 0.22), Mark::Calendar, if b.hovered() { c.strong() } else { c.dim });
    focus_ring(ui, &b);
    let alt_down = focused && ui.input_mut(|inp| inp.consume_key(Modifiers::ALT, Key::ArrowDown));
    Shown { changed: r.changed(), text: r, outer, button: b.clicked() || alt_down, cleared }
}

/// A date field: build it, then [`DateField::show`].
#[derive(Clone)]
pub struct DateField<'a> {
    id: Id,
    words: DateWords<'a>,
    empty_ok: bool,
    min: Option<NaiveDate>,
    max: Option<NaiveDate>,
    monday_first: bool,
    today: Option<NaiveDate>,
    multi: bool,
    width: Option<f32>,
    look: Option<DayLooker<'a>>,
    names: (&'a str, &'a str),
}

impl std::fmt::Debug for DateField<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateField").field("id", &self.id).field("multi", &self.multi).finish()
    }
}

impl<'a> DateField<'a> {
    /// `id` tells this field from the others on the screen.
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug, words: DateWords<'a>) -> Self {
        DateField {
            id: Id::new(("ito-date", id)),
            words,
            empty_ok: false,
            min: None,
            max: None,
            monday_first: false,
            today: None,
            multi: false,
            width: None,
            look: None,
            names: ("", ""),
        }
    }

    /// An empty text is valid, and a clear button shows while there is text.
    pub fn empty_ok(mut self) -> Self {
        self.empty_ok = true;
        self
    }

    /// Days outside `min..=max` cannot be picked, and typed ones are invalid.
    pub fn range(mut self, min: Option<NaiveDate>, max: Option<NaiveDate>) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    pub fn monday_first(mut self, yes: bool) -> Self {
        self.monday_first = yes;
        self
    }

    /// Today, when the app has its own (a test, a time zone); else the computer's.
    pub fn today(mut self, d: NaiveDate) -> Self {
        self.today = Some(d);
        self
    }

    /// A comma-separated list: a click toggles a day and the grid stays open.
    pub fn multi(mut self) -> Self {
        self.multi = true;
        self.empty_ok = true;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    /// The name a screen reader says for the text field (else the hint, `YYYY-MM-DD`).
    pub fn name(mut self, name: &'a str) -> Self {
        self.names.0 = name;
        self
    }

    /// Tints days (a holiday red) and gives them a tip.
    pub fn day_look(mut self, f: DayLooker<'a>) -> Self {
        self.look = Some(f);
        self
    }

    pub fn show(self, ui: &mut Ui, l: Look, text: &mut String) -> DateResponse {
        let ctx = ui.ctx().clone();
        let today = self.today.unwrap_or_else(today_local);
        let mut st = Pop::load(&ctx, self.id, today);
        let valid = text_ok(text, self.empty_ok, (self.min, self.max), self.multi);
        let width = self.width.unwrap_or_else(|| ui.available_width().min(z(ui, 190.0)));
        let shown =
            input(ui, l, &Input { id: self.id, words: &self.words, width, valid, clearable: self.empty_ok, name: self.names.0 }, text);
        let mut changed = shown.changed;
        if shown.cleared {
            text.clear();
            changed = true;
        }
        if shown.button {
            if st.open {
                st.open = false;
            } else {
                let start = if self.multi { split_dates(text).0.last().copied() } else { parse_date(text) };
                st.open_at(start.unwrap_or(today));
            }
        }
        let (many, one) = if self.multi { (split_dates(text).0, None) } else { (Vec::new(), parse_date(text)) };
        let grid = Grid {
            words: &self.words,
            min: self.min,
            max: self.max,
            monday_first: self.monday_first,
            today,
            look: self.look,
            picker_only: false,
        };
        let sel = if self.multi { Sel::Many(&many) } else { Sel::One(one) };
        if let Some(d) = popup(&ctx, l, self.id, shown.outer, &grid, &sel, &mut st) {
            if self.multi {
                let (mut days, bad) = split_dates(text);
                if let Some(at) = days.iter().position(|x| *x == d) {
                    days.remove(at);
                } else {
                    days.push(d);
                }
                days.sort();
                days.dedup();
                let mut parts: Vec<String> = days.into_iter().map(format_date).collect();
                parts.extend(bad);
                *text = parts.join(", ");
            } else {
                *text = format_date(d);
                st.open = false;
            }
            changed = true;
        }
        let open = st.open;
        st.store(&ctx, self.id);
        let valid = text_ok(text, self.empty_ok, (self.min, self.max), self.multi);
        DateResponse { response: shown.text, changed, valid, open }
    }
}

/// A start and an end: two text fields and one grid. The first click is the start, the second the end (the earlier of the two
/// is the start); the days between light up while the pointer is looking for the end.
#[derive(Clone, Debug)]
pub struct RangeField<'a> {
    inner: DateField<'a>,
}

impl<'a> RangeField<'a> {
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug, words: DateWords<'a>) -> Self {
        RangeField { inner: DateField::new(id, words) }
    }

    pub fn empty_ok(mut self) -> Self {
        self.inner = self.inner.empty_ok();
        self
    }

    pub fn range(mut self, min: Option<NaiveDate>, max: Option<NaiveDate>) -> Self {
        self.inner = self.inner.range(min, max);
        self
    }

    pub fn monday_first(mut self, yes: bool) -> Self {
        self.inner = self.inner.monday_first(yes);
        self
    }

    pub fn today(mut self, d: NaiveDate) -> Self {
        self.inner = self.inner.today(d);
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.inner = self.inner.width(w);
        self
    }

    /// The names a screen reader says for the start and the end field ("開始日", "終了日"); else the hint.
    pub fn names(mut self, from: &'a str, to: &'a str) -> Self {
        self.inner.names = (from, to);
        self
    }

    pub fn day_look(mut self, f: DayLooker<'a>) -> Self {
        self.inner = self.inner.day_look(f);
        self
    }

    /// `(from, to)` edit in place. `changed` is true when either text changed; `valid` when both are fine and `from <= to`.
    pub fn show(self, ui: &mut Ui, l: Look, from: &mut String, to: &mut String) -> DateResponse {
        let d = self.inner;
        let ctx = ui.ctx().clone();
        let today = d.today.unwrap_or_else(today_local);
        let mut st = Pop::load(&ctx, d.id, today);
        let width = d.width.unwrap_or_else(|| (ui.available_width() / 2.0 - z(ui, 14.0)).clamp(z(ui, 120.0), z(ui, 190.0)));
        let ok = |t: &str| text_ok(t, d.empty_ok, (d.min, d.max), false);
        let ordered = match (parse_date(from), parse_date(to)) {
            (Some(a), Some(b)) => a <= b,
            _ => true,
        };
        let (mut changed, mut button, mut outer) = (false, false, Rect::NOTHING);
        let mut first = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = z(ui, 6.0);
            let a = input(
                ui,
                l,
                &Input {
                    id: d.id.with("from"),
                    words: &d.words,
                    width,
                    valid: ok(from) && ordered,
                    clearable: d.empty_ok,
                    name: d.names.0,
                },
                from,
            );
            first = Some(a.text.clone());
            outer = a.outer;
            changed |= a.changed;
            button |= a.button;
            if a.cleared {
                from.clear();
                changed = true;
            }
            ui.label(egui::RichText::new("〜").color(l.c.dim));
            let b = input(
                ui,
                l,
                &Input { id: d.id.with("to"), words: &d.words, width, valid: ok(to) && ordered, clearable: d.empty_ok, name: d.names.1 },
                to,
            );
            outer = outer.union(b.outer);
            changed |= b.changed;
            button |= b.button;
            if b.cleared {
                to.clear();
                changed = true;
            }
        });
        if button {
            if st.open {
                st.open = false;
                st.pending = None;
            } else {
                st.open_at(parse_date(from).unwrap_or(today));
            }
        }
        let grid = Grid { words: &d.words, min: d.min, max: d.max, monday_first: d.monday_first, today, look: d.look, picker_only: false };
        let sel = Sel::Range(parse_date(from), parse_date(to));
        if let Some(day) = popup(&ctx, l, d.id, outer, &grid, &sel, &mut st) {
            match st.pending.take() {
                None => {
                    st.pending = Some(day);
                    *from = format_date(day);
                    to.clear();
                }
                Some(start) => {
                    let (a, b) = (start.min(day), start.max(day));
                    *from = format_date(a);
                    *to = format_date(b);
                    st.open = false;
                }
            }
            changed = true;
        }
        let open = st.open;
        st.store(&ctx, d.id);
        let ordered = match (parse_date(from), parse_date(to)) {
            (Some(a), Some(b)) => a <= b,
            _ => true,
        };
        DateResponse { response: first.expect("the first field is always added"), changed, valid: ok(from) && ok(to) && ordered, open }
    }
}

/// A year and twelve months, under any response (a calendar header's month text): `ym` is `(year, month 1..=12)` and the answer is
/// true when a month was picked. The popup shuts on a pick, on Esc and on a press outside.
pub fn month_picker(
    ui: &mut Ui,
    l: Look,
    id: impl std::hash::Hash + std::fmt::Debug,
    words: &DateWords,
    anchor: &Response,
    ym: &mut (i32, u32),
) -> bool {
    let ctx = ui.ctx().clone();
    let id = Id::new(("ito-month-picker", id));
    let now = NaiveDate::from_ymd_opt(ym.0, ym.1.clamp(1, 12), 1).unwrap_or_else(today_local);
    let mut st = Pop::load(&ctx, id, now);
    if anchor.clicked() {
        if st.open {
            st.open = false;
        } else {
            st.open_at(now);
            st.picker = true;
        }
    }
    st.picker = true;
    let grid = Grid { words, min: None, max: None, monday_first: false, today: today_local(), look: None, picker_only: true };
    let _ = popup(&ctx, l, id, anchor.rect, &grid, &Sel::One(None), &mut st);
    // A month button shuts the picker view; here that means the pick is done.
    let mut picked = false;
    if st.open && !st.picker {
        st.picker = true;
        st.open = false;
        picked = true;
        *ym = (st.shown.year(), st.shown.month());
    }
    st.store(&ctx, id);
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Weekday;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn dates_parse_the_way_they_are_written() {
        assert_eq!(parse_date("2026-11-05"), Some(day(2026, 11, 5)));
        assert_eq!(parse_date(" 2026/1/5 "), Some(day(2026, 1, 5)));
        for bad in ["", "2026-13-01", "2026-02-30", "26-01-01", "2026-01", "2026-01-01-01", "a-b-c"] {
            assert_eq!(parse_date(bad), None, "{bad}");
        }
        assert_eq!(format_date(day(2026, 1, 5)), "2026-01-05");
    }

    #[test]
    fn a_month_moves_to_the_end_of_a_shorter_one() {
        assert_eq!(shift_months(day(2026, 1, 31), 1), day(2026, 2, 28));
        assert_eq!(shift_months(day(2028, 1, 31), 1), day(2028, 2, 29));
        assert_eq!(shift_months(day(2026, 1, 15), -2), day(2025, 11, 15));
    }

    #[test]
    fn the_grid_is_six_weeks_that_start_on_the_chosen_day() {
        let sunday = month_grid(day(2026, 11, 15), false);
        assert_eq!(sunday[0][0], day(2026, 11, 1));
        assert_eq!(sunday[0][0].weekday(), Weekday::Sun);
        let monday = month_grid(day(2026, 11, 15), true);
        assert_eq!(monday[0][0], day(2026, 10, 26));
        assert_eq!(monday[0][0].weekday(), Weekday::Mon);
        assert_eq!(monday[5][6], day(2026, 12, 6));
    }

    #[test]
    fn a_field_is_valid_by_its_rules() {
        let none = (None, None);
        assert!(!text_ok("", false, none, false));
        assert!(text_ok("", true, none, false));
        assert!(text_ok("2026-11-05", false, none, false));
        assert!(!text_ok("2026-11-5x", false, none, false));
        let r = (Some(day(2026, 1, 1)), Some(day(2026, 12, 31)));
        assert!(!text_ok("2027-01-01", false, r, false));
        assert!(text_ok("2026-02-01, 2026-03-01", true, r, true));
        assert!(!text_ok("2026-02-01, soon", true, r, true));
    }

    #[test]
    fn words_follow_the_language() {
        let ja = DateWords::for_lang("ja");
        assert_eq!(ja.heading_of(day(2026, 11, 5)), "2026年11月");
        let en = DateWords::for_lang("en");
        assert_eq!(en.heading_of(day(2026, 11, 5)), "November 2026");
    }

    fn frame(ctx: &Context, events: Vec<egui::Event>, text: &mut String) -> DateResponse {
        let input =
            egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 500.0))), events, ..Default::default() };
        let mut out = None;
        let mut o = ctx.run_ui(input, |ui| {
            let l = Look { c: ito_theme::colors(), q: "" };
            out = Some(DateField::new("t", DateWords::en()).today(day(2026, 11, 5)).empty_ok().show(ui, l, text));
        });
        o.textures_delta.clear();
        out.unwrap()
    }

    fn press(key: Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    #[test]
    fn keys_walk_the_grid_and_enter_picks() {
        let ctx = Context::default();
        let mut text = String::new();
        let id = Id::new(("ito-date", "t"));
        let mut st = Pop::load(&ctx, id, day(2026, 11, 5));
        st.open_at(day(2026, 11, 5));
        st.store(&ctx, id);
        frame(&ctx, vec![], &mut text);
        let r = frame(&ctx, vec![press(Key::ArrowRight), press(Key::ArrowDown)], &mut text);
        assert!(!r.changed && r.open);
        let r = frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert!(r.changed && !r.open);
        assert_eq!(text, "2026-11-13");
    }

    #[test]
    fn escape_shuts_the_grid_and_an_empty_field_is_fine() {
        let ctx = Context::default();
        let mut text = String::new();
        let id = Id::new(("ito-date", "t"));
        let mut st = Pop::load(&ctx, id, day(2026, 11, 5));
        st.open_at(day(2026, 11, 5));
        st.store(&ctx, id);
        assert!(frame(&ctx, vec![], &mut text).open);
        let r = frame(&ctx, vec![press(Key::Escape)], &mut text);
        assert!(!r.open && r.valid && text.is_empty());
    }

    #[test]
    fn a_multi_field_toggles_days_and_stays_open() {
        let ctx = Context::default();
        let id = Id::new(("ito-date", "m"));
        let mut text = "2026-11-20".to_owned();
        let mut st = Pop::load(&ctx, id, day(2026, 11, 5));
        st.open_at(day(2026, 11, 5));
        st.store(&ctx, id);
        let run = |events: Vec<egui::Event>, text: &mut String| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 500.0))),
                events,
                ..Default::default()
            };
            let mut open = false;
            let mut o = ctx.run_ui(input, |ui| {
                let l = Look { c: ito_theme::colors(), q: "" };
                open = DateField::new("m", DateWords::ja()).today(day(2026, 11, 5)).multi().show(ui, l, text).open;
            });
            o.textures_delta.clear();
            open
        };
        run(vec![], &mut text);
        assert!(run(vec![press(Key::Enter)], &mut text));
        assert_eq!(text, "2026-11-05, 2026-11-20");
        assert!(run(vec![press(Key::Enter)], &mut text));
        assert_eq!(text, "2026-11-20");
    }

    #[test]
    fn the_month_picker_answers_a_pick() {
        let ctx = Context::default();
        let mut ym = (2026, 11);
        let run = |events: Vec<egui::Event>, ym: &mut (i32, u32)| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 500.0))),
                events,
                ..Default::default()
            };
            let mut picked = false;
            let mut o = ctx.run_ui(input, |ui| {
                let l = Look { c: ito_theme::colors(), q: "" };
                let r = ui.button("2026年11月");
                picked = month_picker(ui, l, "mp", &DateWords::ja(), &r, ym);
            });
            o.textures_delta.clear();
            picked
        };
        assert!(!run(vec![], &mut ym));
        assert_eq!(ym, (2026, 11));
    }
}
