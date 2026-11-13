//! Typed-or-picked fields: a text field you can type or paste in, with a small list or buttons to pick from.
//!
//! [`TimeField`] is a time of day (`09:30`) with an hour and a minute column, [`DurationField`] a length of time (`30m`, `2h`) with a
//! number box and a unit list, [`NumberField`] a whole number with `−` and `+`, and [`ComboField`] any text with a list of usual values.
//! Every one edits a `&mut String` in place, like the `TextEdit` it replaces, and takes its words from [`FieldWords`].
//!
//! Keys in an open list: Up/Down (Home/End) move the light, Enter picks, Esc shuts it, Tab shuts it and goes on. The time lists also take
//! Left/Right to change columns. Alt+Down in the field opens the list. The keys of a list belong to the field only while the list is open
//! and its text or its button has the focus, so two fields on one screen never answer each other's keys; the exceptions are
//! [`DurationField`] and [`NumberField`], whose box also takes Up/Down (and PageUp/PageDown for the number) whenever it has the focus.
//!
//! Test IDs (`--keys`), for a field with the id `retry`: the text `pick.retry`, the buttons `pick.retry.open`, `.clear`, `.less`, `.more`
//! and `.unit`, the time cells `pick.retry.hour.09` and `pick.retry.minute.30`, the units `pick.retry.unit.m` and the list rows
//! `pick.retry.item.0`. An id that is not a test ID (capitals, spaces) gets none.

use crate::date::{input, Accessory, Input};
use crate::Mark;
use crate::{z, Look};
use chrono::{NaiveTime, Timelike};
use egui::{Align2, Color32, Context, FontId, Id, Key, Modifiers, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};
use ito_common::text::parse_duration;
use ito_theme::mix;

/// The result of a typed-or-picked field.
#[derive(Debug)]
pub struct FieldResponse {
    /// The text field.
    pub response: Response,
    /// The text changed, typed or picked, stepped or cleared.
    pub changed: bool,
    /// The final text is fine (empty where that is allowed, else readable and in range).
    pub valid: bool,
    /// A list of this field is open.
    pub open: bool,
}

/// The words of the fields, in the language in force. `hour` has `{h}` (two digits), `minute` has `{m}`.
#[derive(Clone, Debug)]
pub struct FieldWords<'a> {
    pub open: &'a str,
    pub clear: &'a str,
    pub less: &'a str,
    pub more: &'a str,
    pub hint_time: &'a str,
    pub hour: &'a str,
    pub minute: &'a str,
    /// Seconds, minutes, hours.
    pub units: [&'a str; 3],
    pub unit: &'a str,
}

impl FieldWords<'static> {
    pub fn en() -> Self {
        FieldWords {
            open: "Open the list",
            clear: "Clear",
            less: "Decrease",
            more: "Increase",
            hint_time: "HH:MM",
            hour: "hour {h}",
            minute: "minute {m}",
            units: ["seconds", "minutes", "hours"],
            unit: "Unit",
        }
    }

    pub fn ja() -> Self {
        FieldWords {
            open: "一覧を開く",
            clear: "消す",
            less: "減らす",
            more: "増やす",
            hint_time: "HH:MM",
            hour: "{h}時",
            minute: "{m}分",
            units: ["秒", "分", "時間"],
            unit: "単位",
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

impl Default for FieldWords<'static> {
    fn default() -> Self {
        Self::en()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Pure parts: they are what the widgets call, and they test without a window.
// ---------------------------------------------------------------------------------------------------------------------

/// `H:MM` or `HH:MM` (00:00 to 23:59), spaces round it ignored.
pub fn parse_time(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.trim().split_once(':')?;
    let digits = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
    if h.is_empty() || h.len() > 2 || m.len() != 2 || !digits(h) || !digits(m) {
        return None;
    }
    NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)
}

/// `09:05`.
pub fn format_time(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

fn at(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h.min(23), m.min(59), 0).unwrap_or(NaiveTime::MIN)
}

/// The times of a comma-separated list, and the words that are not times.
fn split_times(s: &str) -> (Vec<NaiveTime>, Vec<String>) {
    let (mut ok, mut bad) = (Vec::new(), Vec::new());
    for part in s.split([',', '、', '\n']).map(str::trim).filter(|p| !p.is_empty()) {
        match parse_time(part) {
            Some(t) => ok.push(t),
            None => bad.push(part.to_owned()),
        }
    }
    (ok, bad)
}

/// The list with `t` added, or taken out when it is there: sorted, no doubles, the words that are not times kept at the end as typed.
fn toggle_time(text: &str, t: NaiveTime) -> String {
    let (mut times, bad) = split_times(text);
    if let Some(i) = times.iter().position(|x| *x == t) {
        times.remove(i);
    } else {
        times.push(t);
    }
    times.sort();
    times.dedup();
    let mut parts: Vec<String> = times.into_iter().map(format_time).collect();
    parts.extend(bad);
    parts.join(", ")
}

fn time_ok(text: &str, empty_ok: bool, multi: bool) -> bool {
    if text.trim().is_empty() {
        return empty_ok;
    }
    if multi {
        split_times(text).1.is_empty()
    } else {
        parse_time(text).is_some()
    }
}

/// The minutes of the column: 0, `step`, 2 * `step` ... below 60 (a step is kept to 1..=60).
fn minute_steps(step: u32) -> Vec<u32> {
    (0..60).step_by(step.clamp(1, 60) as usize).collect()
}

/// A unit of a length of time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Seconds,
    Minutes,
    Hours,
}

impl Unit {
    fn secs(self) -> u64 {
        match self {
            Unit::Seconds => 1,
            Unit::Minutes => 60,
            Unit::Hours => 3600,
        }
    }

    /// The letter `ito_common::text::parse_duration` reads.
    fn letter(self) -> char {
        match self {
            Unit::Seconds => 's',
            Unit::Minutes => 'm',
            Unit::Hours => 'h',
        }
    }

    fn index(self) -> usize {
        match self {
            Unit::Seconds => 0,
            Unit::Minutes => 1,
            Unit::Hours => 2,
        }
    }
}

/// The units in the order seconds, minutes, hours, each once; an empty list means all three.
fn allowed_units(units: &[Unit]) -> Vec<Unit> {
    let all = [Unit::Seconds, Unit::Minutes, Unit::Hours];
    let got: Vec<Unit> = all.into_iter().filter(|u| units.contains(u)).collect();
    if got.is_empty() {
        all.to_vec()
    } else {
        got
    }
}

/// What the number box and the unit show for a text. A readable length that one allowed unit divides exactly shows in the largest such
/// unit (`7200s` is 2 hours, `90s` 90 seconds, `120s` 2 minutes, `0s` is 0 of the smallest unit). Else (empty, unreadable, or no unit
/// divides it) the box shows the text as typed and the unit stays as it was (or the smallest allowed one).
fn derive(text: &str, allowed: &[Unit], prev: Unit) -> (String, Unit) {
    let prev = if allowed.contains(&prev) { prev } else { allowed[0] };
    match parse_duration(text) {
        Ok(0) => ("0".to_owned(), allowed[0]),
        Ok(secs) => match allowed.iter().rev().find(|u| secs % u.secs() == 0) {
            Some(u) => ((secs / u.secs()).to_string(), *u),
            None => (text.to_owned(), prev),
        },
        Err(_) => (text.to_owned(), prev),
    }
}

fn is_digits(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
}

/// The text a box means: digits only is that many of `unit` (`7` and minutes: `7m`); anything else is the box exactly as typed
/// (`1h30m`). A number too big for seconds is left as typed too, and so is not valid.
fn text_for(box_: &str, unit: Unit) -> String {
    if is_digits(box_) {
        if let Ok(n) = box_.trim().parse::<u64>() {
            if n.checked_mul(unit.secs()).is_some() {
                return format!("{n}{}", unit.letter());
            }
        }
    }
    box_.to_owned()
}

/// The box a step up or down gives (by 1, not below 0); `None` when the box is not all digits or the number is too big.
fn step_digits(box_: &str, up: bool) -> Option<String> {
    if !is_digits(box_) {
        return None;
    }
    let n: u64 = box_.trim().parse().ok()?;
    Some((if up { n.checked_add(1)? } else { n.saturating_sub(1) }).to_string())
}

fn duration_ok(text: &str, empty_ok: bool, range: (Option<u64>, Option<u64>)) -> bool {
    if text.trim().is_empty() {
        return empty_ok;
    }
    parse_duration(text).is_ok_and(|s| range.0.is_none_or(|a| s >= a) && range.1.is_none_or(|b| s <= b))
}

fn clamp_number(v: i64, min: Option<i64>, max: Option<i64>) -> i64 {
    let v = min.map_or(v, |m| v.max(m));
    max.map_or(v, |m| v.min(m))
}

/// The number after a step: `dir` is 1 or -1, `mult` 1 (a key, a button) or 10 (PageUp/PageDown). From a text that is not a number it
/// is `min` (else 0). Saturates, and keeps to the range.
fn step_number(text: &str, dir: i64, mult: i64, step: i64, min: Option<i64>, max: Option<i64>) -> i64 {
    match text.trim().parse::<i64>() {
        Ok(n) => clamp_number(n.saturating_add(step.max(1).saturating_mul(mult).saturating_mul(dir)), min, max),
        Err(_) => clamp_number(min.unwrap_or(0), min, max),
    }
}

fn number_ok(text: &str, empty_ok: bool, range: (Option<i64>, Option<i64>)) -> bool {
    if text.trim().is_empty() {
        return empty_ok;
    }
    text.trim().parse::<i64>().is_ok_and(|n| range.0.is_none_or(|a| n >= a) && range.1.is_none_or(|b| n <= b))
}

// ---------------------------------------------------------------------------------------------------------------------
// What the four fields share.
// ---------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Common<'a> {
    id: &'a str,
    words: FieldWords<'a>,
    empty_ok: bool,
    width: Option<f32>,
    name: &'a str,
    hint: Option<&'a str>,
}

impl<'a> Common<'a> {
    fn new(id: &'a str, words: FieldWords<'a>) -> Self {
        Common { id, words, empty_ok: false, width: None, name: "", hint: None }
    }

    fn base(&self, kind: &str) -> Id {
        Id::new(("ito-pick", kind, self.id))
    }

    /// `pick.{id}`, or empty when that is not a test ID.
    fn test(&self) -> String {
        let name = format!("pick.{}", self.id);
        if ito_a11y::is_test_id(&name) {
            name
        } else {
            String::new()
        }
    }

    fn width(&self, ui: &Ui) -> f32 {
        self.width.unwrap_or_else(|| ui.available_width().min(z(ui, 190.0)))
    }

    /// What a screen reader says for one of the field's parts: the field's name, then the word.
    fn said(&self, word: &str) -> String {
        if self.name.is_empty() {
            word.to_owned()
        } else {
            format!("{}, {word}", self.name)
        }
    }
}

macro_rules! common_builders {
    () => {
        /// An empty text is valid, and a clear button shows while there is text.
        pub fn empty_ok(mut self) -> Self {
            self.c.empty_ok = true;
            self
        }

        pub fn width(mut self, w: f32) -> Self {
            self.c.width = Some(w);
            self
        }

        /// The name a screen reader says for the text field and, before the word, for its buttons and cells (else the hint).
        pub fn name(mut self, name: &'a str) -> Self {
            self.c.name = name;
            self
        }

        /// The grey text of an empty field.
        pub fn hint(mut self, hint: &'a str) -> Self {
            self.c.hint = Some(hint);
            self
        }
    };
}

/// What stays between frames for a field with a list.
#[derive(Clone, Copy, Debug, Default)]
struct Pop {
    open: bool,
    /// The column the keys are in.
    col: usize,
    /// The row of the keys in each column.
    cur: [usize; 2],
    /// The hour the minutes belong to (time fields).
    hour: u32,
    /// Scroll the row of the keys into view in this column on the next draw.
    scroll: [bool; 2],
    /// The frame this was last drawn in.
    frame: u64,
    /// The field had the keys at the end of the last frame (egui drops the focus on Esc before we look).
    keys: bool,
}

impl Pop {
    /// A list whose field was not drawn in the last frame is shut (a drawer that closed leaves nothing open).
    fn load(ctx: &Context, key: Id) -> Pop {
        let mut p = ctx.data(|d| d.get_temp::<Pop>(key)).unwrap_or_default();
        if ctx.cumulative_frame_nr().saturating_sub(p.frame) > 1 {
            p.open = false;
        }
        p
    }

    fn store(mut self, ctx: &Context, key: Id, button: &str) {
        self.frame = ctx.cumulative_frame_nr();
        self.keys = focus_in(ctx, key, button);
        ctx.data_mut(|d| d.insert_temp(key, self));
    }
}

type Lit = Box<dyn Fn(u32) -> bool>;

/// The text field or the button of `base` has the keys.
fn focus_in(ctx: &Context, base: Id, button: &str) -> bool {
    ctx.memory(|m| m.has_focus(base.with("text")) || m.has_focus(base.with(button)))
}

/// As `focus_in`, and also on the frame Esc was pressed in a field that had the keys (egui drops the focus on Esc first).
fn has_keys(ctx: &Context, base: Id, button: &str, st: &Pop) -> bool {
    focus_in(ctx, base, button) || (st.keys && ctx.input(|i| i.key_pressed(Key::Escape)))
}

#[derive(Clone, Copy, Debug, Default)]
struct Keys {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    home: bool,
    end: bool,
    enter: bool,
    esc: bool,
    tab: bool,
}

/// Alt+Down, taken first: a plain Down below would match it too (egui ignores extra Alt).
fn take_alt_down(ctx: &Context) -> bool {
    ctx.input_mut(|i| i.consume_key(Modifiers::ALT, Key::ArrowDown))
}

/// The keys of an open list. Taken before the text edit is added, so the text cursor does not move too. Tab is only looked at.
fn take_keys(ctx: &Context, horizontal: bool) -> Keys {
    ctx.input_mut(|i| {
        let n = Modifiers::NONE;
        Keys {
            up: i.consume_key(n, Key::ArrowUp),
            down: i.consume_key(n, Key::ArrowDown),
            left: horizontal && i.consume_key(n, Key::ArrowLeft),
            right: horizontal && i.consume_key(n, Key::ArrowRight),
            home: i.consume_key(n, Key::Home),
            end: i.consume_key(n, Key::End),
            enter: i.consume_key(n, Key::Enter),
            esc: i.consume_key(n, Key::Escape),
            tab: i.key_pressed(Key::Tab),
        }
    })
}

/// The row after Up/Down/Home/End in a column of `len` rows.
fn walk(at: usize, len: usize, k: &Keys) -> usize {
    let last = len.saturating_sub(1);
    if k.home {
        0
    } else if k.end {
        last
    } else if k.up {
        at.saturating_sub(1)
    } else if k.down {
        (at + 1).min(last)
    } else {
        at.min(last)
    }
}

/// One row of a list.
pub(crate) struct Cell {
    pub text: String,
    /// A dim note at the right.
    pub note: String,
    pub lit: bool,
    pub said: String,
    /// The whole test ID; empty for none.
    pub test: String,
}

pub(crate) struct Column {
    pub cells: Vec<Cell>,
    pub width: f32,
    /// The row the keys are on (drawn as a ring).
    pub cursor: Option<usize>,
    /// Scroll this row into view.
    pub scroll_to: Option<usize>,
}

pub(crate) struct PopOut {
    /// `(column, row)` clicked.
    pub picked: Option<(usize, usize)>,
    /// A press outside the list and its field.
    pub outside: bool,
}

/// The list under `anchor`: columns of rows, at most 8 rows high before it scrolls.
pub(crate) fn list_popup(ctx: &Context, l: Look, id: Id, anchor: Rect, cols: &[Column], mono: bool) -> PopOut {
    let c = l.c;
    let zoom = ito_theme::zoomed(ctx, 1.0);
    let row_h = 26.0 * zoom;
    let pad = 6.0 * zoom;
    let mut picked = None;
    let area = egui::Area::new(id.with("list"))
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
                ui.spacing_mut().item_spacing = Vec2::new(4.0 * zoom, 0.0);
                ui.horizontal_top(|ui| {
                    for (ci, col) in cols.iter().enumerate() {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::ZERO;
                            egui::ScrollArea::vertical().id_salt(id.with(("col", ci))).max_height(row_h * 8.0).show(ui, |ui| {
                                for (ri, cell) in col.cells.iter().enumerate() {
                                    let (rect, _) = ui.allocate_exact_size(Vec2::new(col.width, row_h), Sense::hover());
                                    let r = ui.interact(rect, id.with(("row", ci, ri)), Sense::click());
                                    let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
                                    let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &cell.said);
                                    if cell.lit {
                                        info.selected = Some(true);
                                    }
                                    r.widget_info(|| info.clone());
                                    if !cell.test.is_empty() && ito_a11y::is_test_id(&cell.test) {
                                        ito_a11y::test_id(&r, &cell.test);
                                    }
                                    let body = rect.shrink2(Vec2::new(0.0, 1.0));
                                    if cell.lit {
                                        ui.painter().rect_filled(body, 5.0, c.run);
                                    } else if r.hovered() {
                                        ui.painter().rect_filled(body, 5.0, c.raised());
                                    }
                                    if col.cursor == Some(ri) {
                                        ui.painter().rect_stroke(body, 5.0, Stroke::new(1.0, c.strong()), StrokeKind::Inside);
                                    }
                                    let font = if mono { FontId::monospace(z(ui, 13.0)) } else { FontId::proportional(z(ui, 13.0)) };
                                    let (ink, dim) = if cell.lit { (c.bg, lit_note(&c)) } else { (c.fg, c.dim) };
                                    let mid = rect.center().y;
                                    ui.painter().text(egui::pos2(rect.left() + 8.0, mid), Align2::LEFT_CENTER, &cell.text, font, ink);
                                    if !cell.note.is_empty() {
                                        let small = FontId::proportional(z(ui, 11.0));
                                        ui.painter().text(
                                            egui::pos2(rect.right() - 8.0, mid),
                                            Align2::RIGHT_CENTER,
                                            &cell.note,
                                            small,
                                            dim,
                                        );
                                    }
                                    if col.scroll_to == Some(ri) {
                                        r.scroll_to_me(Some(egui::Align::Center));
                                    }
                                    if r.clicked() {
                                        picked = Some((ci, ri));
                                    }
                                }
                            });
                        });
                    }
                });
            });
        });
    let (outside, inside) = ctx.input(|i| {
        let pos = i.pointer.interact_pos();
        let pressed = pos.filter(|_| i.pointer.any_pressed());
        let touched = pos.filter(|_| i.pointer.any_pressed() || i.pointer.any_click());
        (
            pressed.is_some_and(|p| !area.response.rect.contains(p) && !anchor.contains(p)),
            touched.is_some_and(|p| area.response.rect.contains(p)),
        )
    });
    // A press or a click on the list but on no row (its padding, its scroll bar) takes egui's focus off the text (egui's
    // `SurrenderFocusOn`, clicks by default); give it back, or the list's keys stop. A picked row is given back by the caller.
    if inside && picked.is_none() {
        focus_text(ctx, id);
    }
    PopOut { picked, outside }
}

/// The colour of a row's note on the lit row (a `c.run` fill with `c.bg` text): mostly the text's colour, so it reads in light and
/// dark themes, and still a little dimmer than the text.
fn lit_note(c: &ito_theme::Colors) -> Color32 {
    mix(c.run, c.bg, 0.85)
}

fn focus_text(ctx: &Context, base: Id) {
    ctx.memory_mut(|m| m.request_focus(base.with("text")));
}

// ---------------------------------------------------------------------------------------------------------------------
// TimeField
// ---------------------------------------------------------------------------------------------------------------------

/// A time of day: build it, then [`TimeField::show`].
#[derive(Clone, Debug)]
pub struct TimeField<'a> {
    c: Common<'a>,
    step: u32,
    multi: bool,
}

impl<'a> TimeField<'a> {
    /// `id` tells this field from the others on the screen (and is part of its test IDs).
    pub fn new(id: &'a str, words: FieldWords<'a>) -> Self {
        TimeField { c: Common::new(id, words), step: 15, multi: false }
    }

    common_builders!();

    /// The minute column goes in steps of this many minutes (1 to 60; 15 by default). Any minute can still be typed.
    pub fn step(mut self, minutes: u32) -> Self {
        self.step = minutes.clamp(1, 60);
        self
    }

    /// A comma-separated list (`09:00, 13:30`): sorted, no doubles, what does not read kept at the end. A pick of a minute adds the time
    /// (the chosen hour and that minute) or takes it out, and the list stays open.
    pub fn multi(mut self) -> Self {
        self.multi = true;
        self.c.empty_ok = true;
        self
    }

    /// A pick in a column. The text it makes, if it changed.
    fn pick(&self, text: &mut String, st: &mut Pop, steps: &[u32], col: usize, row: usize) -> bool {
        if col == 0 {
            let h = row.min(23) as u32;
            st.hour = h;
            st.col = 1;
            st.cur[0] = row.min(23);
            st.scroll[1] = true;
            if self.multi {
                return false;
            }
            *text = format_time(at(h, parse_time(text).map_or(0, |t| t.minute())));
            true
        } else {
            let row = row.min(steps.len().saturating_sub(1));
            st.cur[1] = row;
            let m = steps.get(row).copied().unwrap_or(0);
            if self.multi {
                *text = toggle_time(text, at(st.hour, m));
            } else {
                let h = parse_time(text).map_or(st.hour, |t| t.hour());
                *text = format_time(at(h, m));
                st.open = false;
            }
            true
        }
    }

    fn open_at(&self, text: &str, st: &mut Pop, steps: &[u32]) {
        let seed = if self.multi { split_times(text).0.last().copied() } else { parse_time(text) };
        let t = seed.unwrap_or_else(|| chrono::Local::now().time());
        st.open = true;
        st.col = 0;
        st.hour = t.hour();
        st.cur = [t.hour() as usize, if seed.is_some() { steps.iter().rposition(|&m| m <= t.minute()).unwrap_or(0) } else { 0 }];
        st.scroll = [true, true];
    }

    pub fn show(self, ui: &mut Ui, l: Look, text: &mut String) -> FieldResponse {
        let ctx = ui.ctx().clone();
        let key = self.c.base("time");
        let mut st = Pop::load(&ctx, key);
        let steps = minute_steps(self.step);
        let empty_ok = self.c.empty_ok;
        let mut changed = false;
        let keyed = has_keys(&ctx, key, "open", &st);
        let alt_down = keyed && take_alt_down(&ctx);
        if st.open && keyed {
            let k = take_keys(&ctx, true);
            if k.esc || k.tab {
                st.open = false;
            } else {
                if k.left || k.right {
                    st.col = usize::from(k.right && !k.left);
                    st.scroll[st.col] = true;
                }
                let len = if st.col == 0 { 24 } else { steps.len() };
                let to = walk(st.cur[st.col], len, &k);
                if to != st.cur[st.col] {
                    st.cur[st.col] = to;
                    st.scroll[st.col] = true;
                }
                if k.enter {
                    let (col, row) = (st.col, st.cur[st.col]);
                    changed |= self.pick(text, &mut st, &steps, col, row);
                }
            }
        }
        let words = &self.c.words;
        let open_said = self.c.said(words.open);
        let clear_said = self.c.said(words.clear);
        let test = self.c.test();
        let open = Accessory { mark: Mark::Clock, tip: words.open, said: &open_said, key: "open", enabled: true, text: "" };
        let shown = input(
            ui,
            l,
            &Input {
                id: key,
                hint: self.c.hint.unwrap_or(words.hint_time),
                clear: &clear_said,
                width: self.c.width(ui),
                valid: time_ok(text, empty_ok, self.multi),
                clearable: empty_ok,
                name: self.c.name,
                buttons: &[open],
                unit: "",
                mono: false,
                edit_role: true,
                alt_open: false,
                test: &test,
            },
            text,
        );
        changed |= shown.changed;
        if shown.cleared {
            text.clear();
            changed = true;
        }
        if shown.pressed.is_some() {
            if st.open {
                st.open = false;
            } else {
                self.open_at(text, &mut st, &steps);
                // A click on the button leaves the focus on neither; the keys of the list need it on the text.
                focus_text(&ctx, key);
            }
        } else if alt_down && !st.open {
            self.open_at(text, &mut st, &steps);
        }
        if st.open {
            let (parsed, list) = if self.multi { (None, split_times(text).0) } else { (parse_time(text), Vec::new()) };
            let (hour_lit, minute_lit): (Lit, Lit) = if self.multi {
                let hour = st.hour;
                (Box::new(move |h| h == hour), Box::new(move |m| list.contains(&at(hour, m))))
            } else {
                (Box::new(move |h| parsed.is_some_and(|t| t.hour() == h)), Box::new(move |m| parsed.is_some_and(|t| t.minute() == m)))
            };
            let cell = |text: String, word: String, lit: bool, test_part: String| Cell {
                said: self.c.said(&word),
                text,
                note: String::new(),
                lit,
                test: if test.is_empty() { String::new() } else { format!("{test}.{test_part}") },
            };
            let hours = (0..24u32)
                .map(|h| {
                    let hh = format!("{h:02}");
                    cell(hh.clone(), words.hour.replace("{h}", &hh), hour_lit(h), format!("hour.{hh}"))
                })
                .collect();
            let minutes = steps
                .iter()
                .map(|&m| {
                    let mm = format!("{m:02}");
                    cell(mm.clone(), words.minute.replace("{m}", &mm), minute_lit(m), format!("minute.{mm}"))
                })
                .collect();
            let w = z(ui, 52.0);
            let cols = [
                Column { cells: hours, width: w, cursor: (st.col == 0).then_some(st.cur[0]), scroll_to: st.scroll[0].then_some(st.cur[0]) },
                Column {
                    cells: minutes,
                    width: w,
                    cursor: (st.col == 1).then_some(st.cur[1]),
                    scroll_to: st.scroll[1].then_some(st.cur[1]),
                },
            ];
            let out = list_popup(&ctx, l, key, shown.outer, &cols, false);
            st.scroll = [false, false];
            if let Some((col, row)) = out.picked {
                changed |= self.pick(text, &mut st, &steps, col, row);
                focus_text(&ctx, key);
            } else if out.outside {
                st.open = false;
            }
        }
        let open = st.open;
        st.store(&ctx, key, "open");
        FieldResponse { response: shown.text, changed, valid: time_ok(text, empty_ok, self.multi), open }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// DurationField
// ---------------------------------------------------------------------------------------------------------------------

/// What the number box and unit button hold between frames, and the text they were made from.
#[derive(Clone, Debug)]
struct Dur {
    for_text: String,
    number: String,
    unit: Unit,
}

/// A length of time (`30s`, `30m`, `2h`, `1h30m`): build it, then [`DurationField::show`].
///
/// The box and a unit button edit the text together. Digits in the box mean that many of the unit (`2` and hours is `2h`); anything
/// else typed (`1h30m`) is the text as it is. Picking another unit while the box holds only digits keeps the number and changes the
/// unit (2 minutes becomes 2 hours). When the app changes the text, the box shows it in the largest unit that divides it exactly.
#[derive(Clone, Debug)]
pub struct DurationField<'a> {
    c: Common<'a>,
    units: Vec<Unit>,
    min: Option<u64>,
    max: Option<u64>,
}

impl<'a> DurationField<'a> {
    pub fn new(id: &'a str, words: FieldWords<'a>) -> Self {
        DurationField { c: Common::new(id, words), units: Vec::new(), min: None, max: None }
    }

    common_builders!();

    /// The units to pick from (shown seconds, minutes, hours); none given means all three.
    pub fn units(mut self, units: &[Unit]) -> Self {
        self.units = units.to_vec();
        self
    }

    /// Lengths outside `min..=max` seconds are invalid.
    pub fn range(mut self, min_secs: Option<u64>, max_secs: Option<u64>) -> Self {
        self.min = min_secs;
        self.max = max_secs;
        self
    }

    pub fn show(self, ui: &mut Ui, l: Look, text: &mut String) -> FieldResponse {
        let ctx = ui.ctx().clone();
        let key = self.c.base("duration");
        let mut st = Pop::load(&ctx, key);
        let allowed = allowed_units(&self.units);
        let empty_ok = self.c.empty_ok;
        let range = (self.min, self.max);
        let mut dur = ctx.data(|d| d.get_temp::<Dur>(key));
        if dur.as_ref().is_none_or(|d| d.for_text != *text) {
            let (number, unit) = derive(text, &allowed, dur.as_ref().map_or(allowed[0], |d| d.unit));
            dur = Some(Dur { for_text: text.clone(), number, unit });
        }
        let mut dur = dur.unwrap_or(Dur { for_text: text.clone(), number: text.clone(), unit: allowed[0] });
        if !allowed.contains(&dur.unit) {
            dur.unit = allowed[0];
        }
        let mut changed = false;
        let text_id = key.with("text");
        let focused = ctx.memory(|m| m.has_focus(text_id));
        let keyed = has_keys(&ctx, key, "unit", &st);
        let alt_down = keyed && take_alt_down(&ctx);
        if st.open && keyed {
            let k = take_keys(&ctx, false);
            if k.esc || k.tab {
                st.open = false;
            } else {
                let to = walk(st.cur[0], allowed.len(), &k);
                if to != st.cur[0] {
                    st.cur[0] = to;
                    st.scroll[0] = true;
                }
                if k.enter {
                    changed |= pick_unit(&mut dur, text, allowed[st.cur[0].min(allowed.len() - 1)]);
                    st.open = false;
                }
            }
        } else if !st.open && focused {
            let (up, down) =
                ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown)));
            if up || down {
                if let Some(n) = step_digits(&dur.number, up) {
                    dur.number = n;
                    *text = text_for(&dur.number, dur.unit);
                    changed = true;
                }
            }
        }
        let words = &self.c.words;
        let unit_word = words.units[dur.unit.index()];
        let unit_said = self.c.said(&format!("{}: {unit_word}", words.unit));
        let clear_said = self.c.said(words.clear);
        let test = self.c.test();
        let button = Accessory { mark: Mark::ChevronDown, tip: words.unit, said: &unit_said, key: "unit", enabled: true, text: unit_word };
        let mut buf = dur.number.clone();
        let shown = input(
            ui,
            l,
            &Input {
                id: key,
                hint: self.c.hint.unwrap_or(""),
                clear: &clear_said,
                width: self.c.width(ui),
                valid: duration_ok(text, empty_ok, range),
                clearable: empty_ok,
                name: self.c.name,
                buttons: &[button],
                unit: "",
                mono: false,
                edit_role: true,
                alt_open: false,
                test: &test,
            },
            &mut buf,
        );
        if buf != dur.number {
            dur.number = buf;
            *text = text_for(&dur.number, dur.unit);
            changed = true;
        }
        if shown.cleared {
            dur.number.clear();
            text.clear();
            changed = true;
        }
        let open_now = |st: &mut Pop, unit: Unit| {
            st.open = true;
            st.col = 0;
            st.cur[0] = allowed.iter().position(|u| *u == unit).unwrap_or(0);
            st.scroll[0] = true;
        };
        if shown.pressed.is_some() {
            if st.open {
                st.open = false;
            } else {
                open_now(&mut st, dur.unit);
                focus_text(&ctx, key);
            }
        } else if alt_down && !st.open {
            open_now(&mut st, dur.unit);
        }
        if st.open {
            let rows = allowed
                .iter()
                .map(|u| {
                    let word = words.units[u.index()];
                    Cell {
                        text: word.to_owned(),
                        note: String::new(),
                        lit: *u == dur.unit,
                        said: self.c.said(word),
                        test: if test.is_empty() { String::new() } else { format!("{test}.unit.{}", u.letter()) },
                    }
                })
                .collect();
            let cols = [Column {
                cells: rows,
                width: shown.outer.width().max(z(ui, 110.0)),
                cursor: Some(st.cur[0]),
                scroll_to: st.scroll[0].then_some(st.cur[0]),
            }];
            let out = list_popup(&ctx, l, key, shown.outer, &cols, false);
            st.scroll = [false, false];
            if let Some((_, row)) = out.picked {
                changed |= pick_unit(&mut dur, text, allowed[row.min(allowed.len() - 1)]);
                st.open = false;
                focus_text(&ctx, key);
            } else if out.outside {
                st.open = false;
            }
        }
        if changed {
            ctx.request_repaint();
        }
        dur.for_text = text.clone();
        ctx.data_mut(|d| d.insert_temp(key, dur));
        let open = st.open;
        st.store(&ctx, key, "unit");
        FieldResponse { response: shown.text, changed, valid: duration_ok(text, empty_ok, range), open }
    }
}

/// Picks a unit: with only digits in the box the number stays and the text follows, else only the unit changes. If the text changed.
fn pick_unit(dur: &mut Dur, text: &mut String, unit: Unit) -> bool {
    dur.unit = unit;
    if !is_digits(&dur.number) {
        return false;
    }
    let now = text_for(&dur.number, unit);
    let changed = now != *text;
    *text = now;
    changed
}

// ---------------------------------------------------------------------------------------------------------------------
// NumberField
// ---------------------------------------------------------------------------------------------------------------------

/// A whole number with `−` and `+`: build it, then [`NumberField::show`]. Up/Down in the box step by `step`, PageUp/PageDown by ten
/// steps; the result stays in the range.
#[derive(Clone, Debug)]
pub struct NumberField<'a> {
    c: Common<'a>,
    min: Option<i64>,
    max: Option<i64>,
    step: i64,
    unit: &'a str,
}

impl<'a> NumberField<'a> {
    pub fn new(id: &'a str, words: FieldWords<'a>) -> Self {
        NumberField { c: Common::new(id, words), min: None, max: None, step: 1, unit: "" }
    }

    common_builders!();

    /// Numbers outside `min..=max` are invalid, and the buttons and keys stay inside.
    pub fn range(mut self, min: Option<i64>, max: Option<i64>) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// How far a button or an arrow key goes (at least 1; 1 by default).
    pub fn step(mut self, n: i64) -> Self {
        self.step = n.max(1);
        self
    }

    /// A dim word after the number (`seconds`, `MB`).
    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = unit;
        self
    }

    pub fn show(self, ui: &mut Ui, l: Look, text: &mut String) -> FieldResponse {
        let ctx = ui.ctx().clone();
        let key = self.c.base("number");
        let (min, max, empty_ok) = (self.min, self.max, self.c.empty_ok);
        let mut changed = false;
        if ctx.memory(|m| m.has_focus(key.with("text"))) {
            let (up, down, pu, pd) = ctx.input_mut(|i| {
                let n = Modifiers::NONE;
                (
                    i.consume_key(n, Key::ArrowUp),
                    i.consume_key(n, Key::ArrowDown),
                    i.consume_key(n, Key::PageUp),
                    i.consume_key(n, Key::PageDown),
                )
            });
            for (hit, dir, mult) in [(up, 1, 1), (down, -1, 1), (pu, 1, 10), (pd, -1, 10)] {
                if hit {
                    *text = step_number(text, dir, mult, self.step, min, max).to_string();
                    changed = true;
                }
            }
        }
        let n = text.trim().parse::<i64>().ok();
        let words = &self.c.words;
        let less_said = self.c.said(words.less);
        let more_said = self.c.said(words.more);
        let clear_said = self.c.said(words.clear);
        let test = self.c.test();
        let less = Accessory {
            mark: Mark::Minus,
            tip: words.less,
            said: &less_said,
            key: "less",
            enabled: !n.is_some_and(|n| min.is_some_and(|m| n <= m)),
            text: "",
        };
        let more = Accessory {
            mark: Mark::Plus,
            tip: words.more,
            said: &more_said,
            key: "more",
            enabled: !n.is_some_and(|n| max.is_some_and(|m| n >= m)),
            text: "",
        };
        let shown = input(
            ui,
            l,
            &Input {
                id: key,
                hint: self.c.hint.unwrap_or(""),
                clear: &clear_said,
                width: self.c.width(ui),
                valid: number_ok(text, empty_ok, (min, max)),
                clearable: empty_ok,
                name: self.c.name,
                buttons: &[less, more],
                unit: self.unit,
                mono: false,
                edit_role: true,
                alt_open: false,
                test: &test,
            },
            text,
        );
        changed |= shown.changed;
        if shown.cleared {
            text.clear();
            changed = true;
        }
        if let Some(k) = shown.pressed {
            *text = step_number(text, if k == 0 { -1 } else { 1 }, 1, self.step, min, max).to_string();
            changed = true;
            ctx.request_repaint();
        }
        FieldResponse { response: shown.text, changed, valid: number_ok(text, empty_ok, (min, max)), open: false }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ComboField
// ---------------------------------------------------------------------------------------------------------------------

/// Any text, with a list of usual values to pick from: build it, then [`ComboField::show`]. A pick writes the value only.
#[derive(Clone, Debug)]
pub struct ComboField<'a> {
    c: Common<'a>,
    suggestions: &'a [(&'a str, &'a str)],
    mono: bool,
    error: bool,
}

impl<'a> ComboField<'a> {
    pub fn new(id: &'a str, words: FieldWords<'a>) -> Self {
        ComboField { c: Common::new(id, words), suggestions: &[], mono: false, error: false }
    }

    common_builders!();

    /// The usual values, each `(value, note)`; the note is dim at the right of the row (`""` for none). The row that equals the text
    /// is lit. The list is not filtered while typing.
    pub fn suggestions(mut self, s: &'a [(&'a str, &'a str)]) -> Self {
        self.suggestions = s;
        self
    }

    /// A monospace font, for values like `application/json`.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// The app's own check failed: the field gets the error border and is not valid.
    pub fn error(mut self, yes: bool) -> Self {
        self.error = yes;
        self
    }

    fn valid(&self, text: &str) -> bool {
        !self.error && (self.c.empty_ok || !text.trim().is_empty())
    }

    pub fn show(self, ui: &mut Ui, l: Look, text: &mut String) -> FieldResponse {
        let ctx = ui.ctx().clone();
        let key = self.c.base("combo");
        let mut st = Pop::load(&ctx, key);
        let rows = self.suggestions;
        let mut changed = false;
        let keyed = has_keys(&ctx, key, "open", &st);
        let alt_down = keyed && take_alt_down(&ctx);
        if st.open && keyed {
            let k = take_keys(&ctx, false);
            if k.esc || k.tab {
                st.open = false;
            } else {
                let to = walk(st.cur[0], rows.len(), &k);
                if to != st.cur[0] {
                    st.cur[0] = to;
                    st.scroll[0] = true;
                }
                if k.enter {
                    if let Some((v, _)) = rows.get(st.cur[0]) {
                        *text = (*v).to_owned();
                        changed = true;
                    }
                    st.open = false;
                }
            }
        }
        let words = &self.c.words;
        let open_said = self.c.said(words.open);
        let clear_said = self.c.said(words.clear);
        let test = self.c.test();
        let open = Accessory { mark: Mark::ChevronDown, tip: words.open, said: &open_said, key: "open", enabled: true, text: "" };
        let shown = input(
            ui,
            l,
            &Input {
                id: key,
                hint: self.c.hint.unwrap_or(""),
                clear: &clear_said,
                width: self.c.width(ui),
                valid: self.valid(text),
                clearable: self.c.empty_ok,
                name: self.c.name,
                buttons: &[open],
                unit: "",
                mono: self.mono,
                edit_role: true,
                alt_open: false,
                test: &test,
            },
            text,
        );
        changed |= shown.changed;
        if shown.cleared {
            text.clear();
            changed = true;
        }
        let open_now = |st: &mut Pop, text: &str| {
            st.open = true;
            st.col = 0;
            st.cur[0] = rows.iter().position(|(v, _)| *v == text.trim()).unwrap_or(0);
            st.scroll[0] = true;
        };
        if shown.pressed.is_some() {
            if st.open {
                st.open = false;
            } else {
                open_now(&mut st, text);
                focus_text(&ctx, key);
            }
        } else if alt_down && !st.open {
            open_now(&mut st, text);
        }
        if st.open {
            let cells = rows
                .iter()
                .enumerate()
                .map(|(n, (v, note))| Cell {
                    text: (*v).to_owned(),
                    note: (*note).to_owned(),
                    lit: *v == text.trim(),
                    said: self.c.said(&if note.is_empty() { (*v).to_owned() } else { format!("{v}, {note}") }),
                    test: if test.is_empty() { String::new() } else { format!("{test}.item.{n}") },
                })
                .collect();
            let cols = [Column {
                cells,
                width: shown.outer.width().max(z(ui, 160.0)),
                cursor: Some(st.cur[0]),
                scroll_to: st.scroll[0].then_some(st.cur[0]),
            }];
            let out = list_popup(&ctx, l, key, shown.outer, &cols, self.mono);
            st.scroll = [false, false];
            if let Some((_, row)) = out.picked {
                if let Some((v, _)) = rows.get(row) {
                    *text = (*v).to_owned();
                    changed = true;
                }
                st.open = false;
                focus_text(&ctx, key);
            } else if out.outside {
                st.open = false;
            }
        }
        let open = st.open;
        st.store(&ctx, key, "open");
        let valid = self.valid(text);
        FieldResponse { response: shown.text, changed, valid, open }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn times_parse_the_way_they_are_written() {
        assert_eq!(parse_time("09:05"), Some(t(9, 5)));
        assert_eq!(parse_time("9:05"), Some(t(9, 5)));
        assert_eq!(parse_time("  23:59 "), Some(t(23, 59)));
        assert_eq!(parse_time("00:00"), Some(t(0, 0)));
        for bad in ["", "24:00", "9:5", "09:60", "9", "09-05", "009:05", "09:005", "a:bc", ":30", "9:05:00", "+9:05"] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
        assert_eq!(format_time(t(9, 5)), "09:05");
    }

    #[test]
    fn a_time_list_is_sorted_has_no_doubles_and_keeps_what_does_not_read() {
        assert_eq!(toggle_time("13:30, 09:00", t(10, 0)), "09:00, 10:00, 13:30");
        assert_eq!(toggle_time("09:00, 13:30", t(9, 0)), "13:30");
        assert_eq!(toggle_time("09:00, 9:00, soon", t(8, 0)), "08:00, 09:00, soon");
        assert_eq!(toggle_time("", t(9, 0)), "09:00");
        assert_eq!(toggle_time("09:00", t(9, 0)), "");
    }

    #[test]
    fn a_time_field_is_valid_by_its_rules() {
        assert!(!time_ok("", false, false));
        assert!(time_ok("", true, false));
        assert!(time_ok("9:30", false, false));
        assert!(!time_ok("25:00", true, false));
        assert!(time_ok("09:00, 13:30", true, true));
        assert!(!time_ok("09:00, soon", true, true));
    }

    #[test]
    fn the_minute_column_goes_in_steps() {
        assert_eq!(minute_steps(15), vec![0, 15, 30, 45]);
        assert_eq!(minute_steps(1).len(), 60);
        assert_eq!(minute_steps(60), vec![0]);
        assert_eq!(minute_steps(0), minute_steps(1));
        assert_eq!(minute_steps(7).last(), Some(&56));
    }

    const ALL: [Unit; 3] = [Unit::Seconds, Unit::Minutes, Unit::Hours];

    #[test]
    fn a_length_shows_in_the_largest_unit_that_divides_it() {
        assert_eq!(derive("7200s", &ALL, Unit::Seconds), ("2".to_owned(), Unit::Hours));
        assert_eq!(derive("90s", &ALL, Unit::Hours), ("90".to_owned(), Unit::Seconds));
        assert_eq!(derive("120s", &ALL, Unit::Seconds), ("2".to_owned(), Unit::Minutes));
        assert_eq!(derive("1h30m", &ALL, Unit::Seconds), ("90".to_owned(), Unit::Minutes));
        assert_eq!(derive("0s", &ALL, Unit::Hours), ("0".to_owned(), Unit::Seconds));
        assert_eq!(derive("0s", &[Unit::Minutes, Unit::Hours], Unit::Hours), ("0".to_owned(), Unit::Minutes));
    }

    #[test]
    fn a_length_no_unit_divides_or_that_does_not_read_is_shown_as_typed() {
        let mh = [Unit::Minutes, Unit::Hours];
        assert_eq!(derive("1m30s", &mh, Unit::Hours), ("1m30s".to_owned(), Unit::Hours));
        assert_eq!(derive("1m30s", &mh, Unit::Seconds), ("1m30s".to_owned(), Unit::Minutes));
        assert_eq!(derive("", &ALL, Unit::Minutes), (String::new(), Unit::Minutes));
        assert_eq!(derive("soon", &ALL, Unit::Hours), ("soon".to_owned(), Unit::Hours));
    }

    #[test]
    fn units_are_listed_in_order_and_empty_means_all() {
        assert_eq!(allowed_units(&[]), ALL.to_vec());
        assert_eq!(allowed_units(&[Unit::Hours, Unit::Seconds, Unit::Hours]), vec![Unit::Seconds, Unit::Hours]);
    }

    #[test]
    fn digits_in_the_box_mean_that_many_of_the_unit() {
        assert_eq!(text_for("7", Unit::Minutes), "7m");
        assert_eq!(text_for(" 007 ", Unit::Hours), "7h");
        assert_eq!(text_for("1h30m", Unit::Minutes), "1h30m");
        assert_eq!(text_for("", Unit::Minutes), "");
        assert_eq!(text_for("x", Unit::Seconds), "x");
        // Too big for seconds: left as typed, so not valid.
        let huge = u64::MAX.to_string();
        assert_eq!(text_for(&huge, Unit::Hours), huge);
        let past = "99999999999999999999999";
        assert_eq!(text_for(past, Unit::Seconds), past);
        assert!(!duration_ok(&text_for(&huge, Unit::Hours), false, (None, None)));
    }

    #[test]
    fn a_unit_pick_keeps_the_number_when_the_box_is_digits() {
        let mut dur = Dur { for_text: "2m".into(), number: "2".into(), unit: Unit::Minutes };
        let mut text = "2m".to_owned();
        assert!(pick_unit(&mut dur, &mut text, Unit::Hours));
        assert_eq!((text.as_str(), dur.number.as_str(), dur.unit), ("2h", "2", Unit::Hours));
        let mut dur = Dur { for_text: "1h30m".into(), number: "1h30m".into(), unit: Unit::Minutes };
        let mut text = "1h30m".to_owned();
        assert!(!pick_unit(&mut dur, &mut text, Unit::Hours));
        assert_eq!((text.as_str(), dur.unit), ("1h30m", Unit::Hours));
    }

    #[test]
    fn the_box_steps_by_one_and_not_below_zero() {
        assert_eq!(step_digits("5", true).as_deref(), Some("6"));
        assert_eq!(step_digits("5", false).as_deref(), Some("4"));
        assert_eq!(step_digits("0", false).as_deref(), Some("0"));
        assert_eq!(step_digits("1h", true), None);
        assert_eq!(step_digits("", true), None);
        assert_eq!(step_digits(&u64::MAX.to_string(), true), None);
    }

    #[test]
    fn a_length_is_valid_when_it_reads_and_is_in_range() {
        let none = (None, None);
        assert!(!duration_ok("", false, none));
        assert!(duration_ok("", true, none));
        assert!(duration_ok("1h30m", false, none));
        assert!(!duration_ok("30", true, none));
        let r = (Some(60), Some(3600));
        assert!(duration_ok("1m", false, r));
        assert!(duration_ok("1h", false, r));
        assert!(!duration_ok("30s", false, r));
        assert!(!duration_ok("2h", false, r));
    }

    #[test]
    fn a_number_steps_and_stays_in_range() {
        let r = (Some(0), Some(100));
        assert_eq!(step_number("5", 1, 1, 1, r.0, r.1), 6);
        assert_eq!(step_number("5", -1, 1, 2, r.0, r.1), 3);
        assert_eq!(step_number("5", 1, 10, 2, r.0, r.1), 25);
        assert_eq!(step_number("95", 1, 10, 1, r.0, r.1), 100);
        assert_eq!(step_number("0", -1, 1, 1, r.0, r.1), 0);
        assert_eq!(step_number("-50", 1, 1, 1, r.0, r.1), 0);
        // From nothing: the minimum, else 0, inside the range.
        assert_eq!(step_number("", 1, 1, 1, Some(5), None), 5);
        assert_eq!(step_number("x", -1, 1, 1, None, None), 0);
        assert_eq!(step_number("", 1, 1, 1, None, Some(-3)), -3);
        // Saturates instead of overflowing.
        assert_eq!(step_number(&i64::MAX.to_string(), 1, 10, i64::MAX, None, None), i64::MAX);
        assert_eq!(step_number(&i64::MIN.to_string(), -1, 10, i64::MAX, None, None), i64::MIN);
    }

    #[test]
    fn a_number_is_valid_when_it_reads_and_is_in_range() {
        let none = (None, None);
        assert!(!number_ok("", false, none));
        assert!(number_ok("", true, none));
        assert!(number_ok(" -3 ", false, none));
        assert!(!number_ok("3.5", true, none));
        assert!(!number_ok("1e3", true, none));
        assert!(number_ok("5", false, (Some(1), Some(5))));
        assert!(!number_ok("6", false, (Some(1), Some(5))));
        assert!(!number_ok("0", false, (Some(1), None)));
    }

    #[test]
    fn a_combo_is_valid_when_it_has_text_and_the_app_agrees() {
        let w = FieldWords::en;
        let f = ComboField::new("c", w());
        assert!(!f.valid(""));
        assert!(f.valid("us-east-1"));
        assert!(ComboField::new("c", w()).empty_ok().valid(""));
        assert!(!ComboField::new("c", w()).error(true).valid("x"));
    }

    #[test]
    fn words_follow_the_language() {
        assert_eq!(FieldWords::for_lang("ja-JP").units[2], "時間");
        assert_eq!(FieldWords::for_lang("en").units[2], "hours");
        assert_eq!(FieldWords::for_lang("de").open, "Open the list");
        assert_eq!(FieldWords::default().clear, "Clear");
    }

    #[test]
    fn the_spoken_name_comes_first() {
        let c = Common::new("x", FieldWords::en());
        assert_eq!(c.said("Increase"), "Increase");
        let named = Common { name: "Retry count", ..c };
        assert_eq!(named.said("Increase"), "Retry count, Increase");
        assert_eq!(named.test(), "pick.x");
        assert_eq!(Common::new("Two Words", FieldWords::en()).test(), "");
    }

    // With a window ---------------------------------------------------------------------------------------------------------

    fn key(k: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn press(k: Key) -> egui::Event {
        key(k, Modifiers::NONE)
    }

    fn alt_down() -> egui::Event {
        key(Key::ArrowDown, Modifiers::ALT)
    }

    /// One frame; `f` draws the fields.
    fn frame(ctx: &Context, events: Vec<egui::Event>, f: impl FnMut(&mut Ui)) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(600.0, 500.0))),
            events,
            ..Default::default()
        };
        let mut f = f;
        let mut out = ctx.run_ui(input, |ui| f(ui));
        out.textures_delta.clear();
    }

    fn look() -> Look<'static> {
        Look { c: ito_theme::colors(), q: "" }
    }

    fn focus(ctx: &Context, kind: &str, id: &str) {
        ctx.memory_mut(|m| m.request_focus(Id::new(("ito-pick", kind, id)).with("text")));
    }

    /// Draws one time field per frame and answers `(changed, open)`; the text is in `text`.
    fn time_frame(ctx: &Context, events: Vec<egui::Event>, text: &mut String) -> (bool, bool) {
        let mut r = (false, false);
        frame(ctx, events, |ui| {
            let o = TimeField::new("t", FieldWords::en()).empty_ok().show(ui, look(), text);
            r = (o.changed, o.open);
        });
        r
    }

    fn focused_time(text: &mut String) -> Context {
        let ctx = Context::default();
        time_frame(&ctx, vec![], text);
        focus(&ctx, "time", "t");
        time_frame(&ctx, vec![], text);
        ctx
    }

    #[test]
    fn alt_down_opens_the_time_list_and_enter_picks_hour_then_minute() {
        let mut text = "09:30".to_owned();
        let ctx = focused_time(&mut text);
        assert_eq!(time_frame(&ctx, vec![alt_down()], &mut text), (false, true));
        // The hour column: Down moves to 10, Enter sets the hour and goes to the minutes.
        time_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        let (changed, open) = time_frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert!(changed && open);
        assert_eq!(text, "10:30");
        // The minute column: the light is on 30; Down to 45, Enter picks and shuts.
        time_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        let (changed, open) = time_frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert!(changed && !open);
        assert_eq!(text, "10:45");
    }

    #[test]
    fn left_and_right_switch_columns_and_escape_changes_nothing() {
        let mut text = "09:00".to_owned();
        let ctx = focused_time(&mut text);
        time_frame(&ctx, vec![alt_down()], &mut text);
        time_frame(&ctx, vec![press(Key::ArrowRight), press(Key::ArrowDown)], &mut text);
        let (changed, open) = time_frame(&ctx, vec![press(Key::Escape)], &mut text);
        assert!(!changed && !open);
        assert_eq!(text, "09:00");
        // Left/Right are the text's again once it is shut.
        focus(&ctx, "time", "t");
        time_frame(&ctx, vec![], &mut text);
        time_frame(&ctx, vec![alt_down()], &mut text);
        time_frame(&ctx, vec![press(Key::ArrowRight)], &mut text);
        time_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        time_frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert_eq!(text, "09:15");
    }

    #[test]
    fn tab_shuts_the_list_without_picking() {
        let mut text = "09:00".to_owned();
        let ctx = focused_time(&mut text);
        time_frame(&ctx, vec![alt_down()], &mut text);
        let (changed, open) = time_frame(&ctx, vec![press(Key::ArrowDown), press(Key::Tab)], &mut text);
        assert!(!changed && !open);
        assert_eq!(text, "09:00");
    }

    #[test]
    fn a_time_field_that_was_not_drawn_for_a_frame_comes_back_shut() {
        let mut text = "09:00".to_owned();
        let ctx = focused_time(&mut text);
        assert!(time_frame(&ctx, vec![alt_down()], &mut text).1);
        assert!(time_frame(&ctx, vec![], &mut text).1);
        frame(&ctx, vec![], |_| {});
        assert!(!time_frame(&ctx, vec![], &mut text).1);
    }

    #[test]
    fn keys_reach_only_the_time_field_that_has_the_focus() {
        let ctx = Context::default();
        let (mut a, mut b) = ("09:00".to_owned(), "10:00".to_owned());
        let two = |ctx: &Context, events: Vec<egui::Event>, a: &mut String, b: &mut String| {
            let mut open = (false, false);
            frame(ctx, events, |ui| {
                open.0 = TimeField::new("a", FieldWords::en()).show(ui, look(), a).open;
                open.1 = TimeField::new("b", FieldWords::en()).show(ui, look(), b).open;
            });
            open
        };
        two(&ctx, vec![], &mut a, &mut b);
        focus(&ctx, "time", "a");
        two(&ctx, vec![], &mut a, &mut b);
        assert_eq!(two(&ctx, vec![alt_down()], &mut a, &mut b), (true, false));
        // Both lists are open now, but only the focused one's keys count.
        focus(&ctx, "time", "b");
        two(&ctx, vec![], &mut a, &mut b);
        assert_eq!(two(&ctx, vec![alt_down()], &mut a, &mut b), (true, true));
        two(&ctx, vec![press(Key::ArrowDown), press(Key::Enter)], &mut a, &mut b);
        assert_eq!((a.as_str(), b.as_str()), ("09:00", "11:00"));
        two(&ctx, vec![press(Key::Escape)], &mut a, &mut b);
        assert_eq!(two(&ctx, vec![], &mut a, &mut b), (true, false));
    }

    #[test]
    fn a_multi_time_field_toggles_times_and_stays_open() {
        let ctx = Context::default();
        let mut text = "13:30".to_owned();
        let run = |ctx: &Context, events: Vec<egui::Event>, text: &mut String| {
            let mut open = false;
            frame(ctx, events, |ui| {
                open = TimeField::new("m", FieldWords::ja()).multi().show(ui, look(), text).open;
            });
            open
        };
        run(&ctx, vec![], &mut text);
        focus(&ctx, "time", "m");
        run(&ctx, vec![], &mut text);
        assert!(run(&ctx, vec![alt_down()], &mut text));
        // The seed is the last time: hour 13, minute 30. Go to the hour 9 ... Home goes to 00, so use Home then the minutes.
        run(&ctx, vec![press(Key::Home), press(Key::Enter)], &mut text);
        assert_eq!(text, "13:30", "an hour pick in a list changes nothing");
        assert!(run(&ctx, vec![press(Key::Enter)], &mut text));
        assert_eq!(text, "00:30, 13:30");
        assert!(run(&ctx, vec![press(Key::Enter)], &mut text));
        assert_eq!(text, "13:30");
    }

    #[test]
    fn the_time_field_has_test_ids_and_spoken_names() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut text = "09:30".to_owned();
        let mut seen = Vec::new();
        for events in [vec![], vec![], vec![alt_down()], vec![]] {
            focus(&ctx, "time", "when");
            frame(&ctx, events, |ui| {
                TimeField::new("when", FieldWords::ja()).name("開始時刻").show(ui, look(), &mut text);
            });
            seen = ito_a11y::take_test_ids(&ctx, egui::ViewportId::ROOT);
        }
        let names: Vec<&str> = seen.iter().map(|e| e.name.as_str()).collect();
        for want in ["pick.when", "pick.when.open", "pick.when.hour.09", "pick.when.hour.23", "pick.when.minute.30", "pick.when.minute.45"]
        {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
    }

    #[test]
    fn the_note_on_a_lit_row_reads_in_every_theme() {
        for t in ito_theme::builtin() {
            let c = t.colors;
            let note = lit_note(&c);
            let seen = ito_theme::contrast(note, c.run);
            assert!(seen >= 3.0, "{}: the note on the lit row has contrast {seen:.2}", t.name);
            assert!(seen <= ito_theme::contrast(c.bg, c.run), "{}: the note is not dimmer than the text", t.name);
        }
    }

    /// A click on the field's list button (found by its test ID), as three frames: move, press, release. `draw` runs one frame.
    fn click_button(ctx: &Context, name: &str, draw: &mut dyn FnMut(Vec<egui::Event>)) {
        draw(vec![]);
        let at = ito_a11y::take_test_ids(ctx, egui::ViewportId::ROOT)
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} was not registered"))
            .rect
            .center();
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        draw(vec![egui::Event::PointerMoved(at)]);
        draw(vec![button(true)]);
        draw(vec![button(false)]);
        // The focus asked for on the release frame is the text's from the next frame on.
        draw(vec![]);
    }

    #[test]
    fn a_click_on_the_time_button_leaves_the_keys_with_the_list() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut text = "09:30".to_owned();
        let mut opened = false;
        click_button(&ctx, "pick.t.open", &mut |ev| opened = time_frame(&ctx, ev, &mut text).1);
        assert!(opened);
        time_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        let (changed, open) = time_frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert!(changed && open);
        assert_eq!(text, "10:30");
        // Esc shuts it.
        assert!(!time_frame(&ctx, vec![press(Key::Escape)], &mut text).1);
    }

    #[test]
    fn a_press_on_the_list_but_on_no_row_leaves_the_keys_with_the_list() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut text = "09:30".to_owned();
        let mut opened = false;
        click_button(&ctx, "pick.t.open", &mut |ev| opened = time_frame(&ctx, ev, &mut text).1);
        assert!(opened);
        // The list's padding left of an hour row in sight: in the list, on no row.
        let row = ito_a11y::take_test_ids(&ctx, egui::ViewportId::ROOT)
            .iter()
            .find(|e| e.name.starts_with("pick.t.hour.") && e.interact_rect.height() > 20.0)
            .expect("no hour row in sight")
            .interact_rect;
        let at = egui::pos2(row.left() - 3.0, row.center().y);
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        time_frame(&ctx, vec![egui::Event::PointerMoved(at)], &mut text);
        assert!(time_frame(&ctx, vec![button(true)], &mut text).1, "a press in the list does not shut it");
        time_frame(&ctx, vec![button(false)], &mut text);
        time_frame(&ctx, vec![], &mut text);
        time_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        let (changed, open) = time_frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert!(changed && open);
        assert_eq!(text, "10:30");
    }

    #[test]
    fn a_click_on_the_unit_button_leaves_the_keys_with_the_list() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut text = "2m".to_owned();
        let mut opened = false;
        click_button(&ctx, "pick.d.unit", &mut |ev| opened = duration_frame(&ctx, ev, &mut text).2);
        assert!(opened);
        duration_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        let (changed, _, open) = duration_frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert!(changed && !open);
        assert_eq!(text, "2h");
    }

    #[test]
    fn a_click_on_the_combo_button_leaves_the_keys_with_the_list() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut text = "text/plain".to_owned();
        let mut opened = false;
        click_button(&ctx, "pick.c.open", &mut |ev| opened = combo_frame(&ctx, ev, &mut text).2);
        assert!(opened);
        combo_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        let (changed, _, open) = combo_frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert!(changed && !open);
        assert_eq!(text, "text/html");
    }

    #[test]
    fn an_unreadable_value_is_told_to_a_screen_reader_as_invalid() {
        let invalid_nodes = |text: &mut String| {
            let ctx = Context::default();
            ctx.enable_accesskit();
            let input =
                egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(600.0, 500.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                TimeField::new("t", FieldWords::en()).empty_ok().show(ui, look(), text);
            });
            out.textures_delta.clear();
            let update = out.platform_output.accesskit_update.expect("an AccessKit update");
            update.nodes.iter().filter(|(_, n)| n.invalid() == Some(egui::accesskit::Invalid::True)).count()
        };
        assert_eq!(invalid_nodes(&mut "25:99".to_owned()), 1);
        assert_eq!(invalid_nodes(&mut "09:30".to_owned()), 0);
    }

    // A duration ------------------------------------------------------------------------------------------------------------

    fn duration_frame(ctx: &Context, events: Vec<egui::Event>, text: &mut String) -> (bool, bool, bool) {
        let mut r = (false, false, false);
        frame(ctx, events, |ui| {
            let o = DurationField::new("d", FieldWords::en()).empty_ok().range(Some(1), Some(86_400)).show(ui, look(), text);
            r = (o.changed, o.valid, o.open);
        });
        r
    }

    #[test]
    fn the_unit_list_opens_with_alt_down_and_picking_a_unit_keeps_the_number() {
        let ctx = Context::default();
        let mut text = "2m".to_owned();
        duration_frame(&ctx, vec![], &mut text);
        focus(&ctx, "duration", "d");
        duration_frame(&ctx, vec![], &mut text);
        assert!(duration_frame(&ctx, vec![alt_down()], &mut text).2);
        // The light is on minutes; Down moves to hours; Enter picks and shuts.
        duration_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        let (changed, valid, open) = duration_frame(&ctx, vec![press(Key::Enter)], &mut text);
        assert!(changed && valid && !open);
        assert_eq!(text, "2h");
        // Esc shuts without a change.
        duration_frame(&ctx, vec![alt_down()], &mut text);
        let (changed, _, open) = duration_frame(&ctx, vec![press(Key::ArrowUp), press(Key::Escape)], &mut text);
        assert!(!changed && !open);
        assert_eq!(text, "2h");
    }

    #[test]
    fn up_and_down_in_the_box_step_the_number_and_the_app_can_change_the_text() {
        let ctx = Context::default();
        let mut text = "30m".to_owned();
        duration_frame(&ctx, vec![], &mut text);
        focus(&ctx, "duration", "d");
        duration_frame(&ctx, vec![], &mut text);
        duration_frame(&ctx, vec![press(Key::ArrowUp)], &mut text);
        assert_eq!(text, "31m");
        duration_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        duration_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        assert_eq!(text, "29m");
        // The app sets another text while the box has the keys: the box follows.
        text = "2h".to_owned();
        duration_frame(&ctx, vec![], &mut text);
        duration_frame(&ctx, vec![press(Key::ArrowUp)], &mut text);
        assert_eq!(text, "3h");
        // Out of range is not valid.
        text = "48h".to_owned();
        assert!(!duration_frame(&ctx, vec![], &mut text).1);
    }

    #[test]
    fn tab_shuts_the_unit_list() {
        let ctx = Context::default();
        let mut text = "5s".to_owned();
        duration_frame(&ctx, vec![], &mut text);
        focus(&ctx, "duration", "d");
        duration_frame(&ctx, vec![], &mut text);
        assert!(duration_frame(&ctx, vec![alt_down()], &mut text).2);
        assert!(!duration_frame(&ctx, vec![press(Key::Tab)], &mut text).2);
        assert_eq!(text, "5s");
    }

    // A number --------------------------------------------------------------------------------------------------------------

    fn number_frame(ctx: &Context, events: Vec<egui::Event>, text: &mut String) -> (bool, bool) {
        let mut r = (false, false);
        frame(ctx, events, |ui| {
            let o = NumberField::new("n", FieldWords::en()).range(Some(0), Some(12)).step(2).unit("times").show(ui, look(), text);
            r = (o.changed, o.valid);
        });
        r
    }

    #[test]
    fn up_down_and_page_keys_step_a_number_and_stay_in_range() {
        let ctx = Context::default();
        let mut text = "5".to_owned();
        number_frame(&ctx, vec![], &mut text);
        focus(&ctx, "number", "n");
        number_frame(&ctx, vec![], &mut text);
        assert_eq!(number_frame(&ctx, vec![press(Key::ArrowUp)], &mut text), (true, true));
        assert_eq!(text, "7");
        number_frame(&ctx, vec![press(Key::PageUp)], &mut text);
        assert_eq!(text, "12");
        number_frame(&ctx, vec![press(Key::ArrowUp)], &mut text);
        assert_eq!(text, "12");
        number_frame(&ctx, vec![press(Key::PageDown)], &mut text);
        assert_eq!(text, "0");
        number_frame(&ctx, vec![press(Key::ArrowDown)], &mut text);
        assert_eq!(text, "0");
        // From empty: the minimum.
        text.clear();
        number_frame(&ctx, vec![press(Key::ArrowUp)], &mut text);
        assert_eq!(text, "0");
        text = "40".to_owned();
        assert!(!number_frame(&ctx, vec![], &mut text).1);
    }

    #[test]
    fn the_number_buttons_have_names_and_test_ids() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut text = "5".to_owned();
        let mut seen = Vec::new();
        for _ in 0..2 {
            frame(&ctx, vec![], |ui| {
                NumberField::new("retry", FieldWords::en()).name("Retry count").show(ui, look(), &mut text);
            });
            seen = ito_a11y::take_test_ids(&ctx, egui::ViewportId::ROOT);
        }
        let names: Vec<&str> = seen.iter().map(|e| e.name.as_str()).collect();
        for want in ["pick.retry", "pick.retry.less", "pick.retry.more"] {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
    }

    // A combo ---------------------------------------------------------------------------------------------------------------

    const TYPES: &[(&str, &str)] = &[("application/json", ""), ("text/plain", "plain text"), ("text/html", "")];

    fn combo_frame(ctx: &Context, events: Vec<egui::Event>, text: &mut String) -> (bool, bool, bool) {
        let mut r = (false, false, false);
        frame(ctx, events, |ui| {
            let o = ComboField::new("c", FieldWords::en()).suggestions(TYPES).mono().show(ui, look(), text);
            r = (o.changed, o.valid, o.open);
        });
        r
    }

    #[test]
    fn down_and_enter_pick_a_suggestion_and_esc_does_not() {
        let ctx = Context::default();
        let mut text = "text/plain".to_owned();
        combo_frame(&ctx, vec![], &mut text);
        focus(&ctx, "combo", "c");
        combo_frame(&ctx, vec![], &mut text);
        assert!(combo_frame(&ctx, vec![alt_down()], &mut text).2);
        let (changed, valid, open) = combo_frame(&ctx, vec![press(Key::ArrowDown), press(Key::Enter)], &mut text);
        assert!(changed && valid && !open);
        assert_eq!(text, "text/html");
        combo_frame(&ctx, vec![alt_down()], &mut text);
        let (changed, _, open) = combo_frame(&ctx, vec![press(Key::Home), press(Key::Escape)], &mut text);
        assert!(!changed && !open);
        assert_eq!(text, "text/html");
        combo_frame(&ctx, vec![alt_down()], &mut text);
        assert!(!combo_frame(&ctx, vec![press(Key::Tab)], &mut text).2);
        assert_eq!(text, "text/html");
    }

    #[test]
    fn a_combo_list_has_item_test_ids() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut text = String::new();
        let mut seen = Vec::new();
        for events in [vec![], vec![], vec![alt_down()], vec![]] {
            focus(&ctx, "combo", "c");
            combo_frame(&ctx, events, &mut text);
            seen = ito_a11y::take_test_ids(&ctx, egui::ViewportId::ROOT);
        }
        let names: Vec<&str> = seen.iter().map(|e| e.name.as_str()).collect();
        for want in ["pick.c", "pick.c.open", "pick.c.item.0", "pick.c.item.2"] {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
    }
}
