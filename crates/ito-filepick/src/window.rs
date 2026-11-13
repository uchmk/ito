//! The egui window (feature `egui`): [`Picker::show`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    pos2, vec2, Align, Color32, Context, CornerRadius, Event, EventFilter, FontId, Frame, Galley, Id, ImeEvent, Key, LayerId, Layout,
    Margin, Modifiers, Order, Pos2, Rect, RichText, ScrollArea, Sense, Shadow, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo,
    WidgetType,
};
use ito_a11y::Role;
use ito_keys::Input;
use ito_prefs::{Mark, Tone};
use ito_theme::scale::{height, radius};
use ito_theme::{colors, zoomed};

use crate::model::{Kind, Mode, Picker, Shown, Sort, Status};
use crate::path::display_name;
use crate::words::Words;

/// What the window needs from the app: the words, the clock for the Modified column, the language for the clock.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    pub words: &'a Words,
    pub clock: &'a ito_common::Clock,
    pub lang: &'a str,
}

/// How long a load takes before the dialog says "Loading".
const LOADING_AFTER: f64 = 0.15;

/// What the window remembers between frames.
#[derive(Default)]
pub(crate) struct UiState {
    pub(crate) started: bool,
    pub(crate) editing: bool,
    pub(crate) path_text: String,
    focus_path: bool,
    focus_list: bool,
    focus_name: bool,
    pub(crate) name_id: Option<Id>,
    list_id: Option<Id>,
    pub(crate) path_id: Option<Id>,
    scroll_y: f32,
    last_cursor: Option<usize>,
    page_rows: usize,
    loading_since: Option<f64>,
    pub(crate) loading_visible: bool,
    said_status: String,
    said_note: String,
    said_path: String,
    /// After a bad typed path the edit stays open with the caret at the end instead of the whole text selected.
    keep_caret: bool,
    composing: bool,
    foreign_focus: bool,
    /// The name the replace question was last seen asking about (to notice it opening).
    asked: Option<String>,
    /// Which button of the replace question the keyboard is on: 0 Replace, 1 Cancel.
    replace_focus: usize,
    /// The question's title, to be announced once in the next drawn frame.
    announce: Option<String>,
}

fn z(ctx: &Context, size: f32) -> f32 {
    zoomed(ctx, size)
}

/// A layout size (a box, a gap, a breakpoint) under the text zoom, so the boxes grow with their text.
fn sc(ctx: &Context, size: f32) -> f32 {
    size * ito_theme::text_zoom(ctx)
}

/// The size the dialog wants, or what the screen leaves (a 16 px margin each side); a screen under 32 px is used whole.
fn fit(preferred: f32, screen: f32) -> f32 {
    if screen <= 32.0 {
        screen
    } else {
        preferred.min(screen - 32.0)
    }
}

/// The filter select's own width (ito-prefs fixes it), which the footer lays out around.
const SELECT_W: f32 = 190.0;

/// Takes the key press with exactly these modifiers (egui's `consume_key` also takes extra Shift and Alt).
fn take_key(ctx: &Context, mods: Modifiers, key: Key) -> bool {
    ctx.input_mut(|i| {
        let mut hit = false;
        i.events.retain(|e| match e {
            Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.matches_exact(mods) => {
                hit = true;
                false
            }
            _ => true,
        });
        hit
    })
}

fn lock_keys(ctx: &Context, id: Id) {
    let filter = EventFilter { horizontal_arrows: true, vertical_arrows: true, escape: true, tab: false };
    ctx.memory_mut(|m| m.set_focus_lock_filter(id, filter));
}

/// One line of text, cut with an ellipsis at `width`.
fn elided(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = TextWrapping { max_width: width.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
    ui.painter().layout_job(job)
}

fn text_w(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x
}

fn at<R>(ui: &mut Ui, r: Rect, layout: Layout, f: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(UiBuilder::new().max_rect(r).layout(layout), f).inner
}

fn ltr() -> Layout {
    Layout::left_to_right(Align::Center)
}

impl Picker {
    fn tid(&self, name: &str) -> String {
        format!("{}.{}", self.id(), name)
    }

    fn put_id(&self, resp: &egui::Response, name: &str) {
        ito_a11y::test_id(resp, &self.tid(name));
    }

    /// Draw the dialog over the app and handle its keys. Returns what [`Picker::take`] would: the request to answer and the
    /// end of the dialog. Call it every frame until `done` comes.
    pub fn show(&mut self, ctx: &Context, look: &Look) -> Shown {
        self.begin();
        if self.is_done() {
            return self.take();
        }
        let mine = Modal::layer(self.id());
        self.note_question(ctx, look);
        self.keys(ctx, mine);
        if self.is_done() {
            return self.take();
        }
        self.window(ctx, look);
        self.note_question(ctx, look);
        self.question(ctx, look);
        self.ui.foreign_focus = foreign_focus(ctx, mine);
        self.take()
    }

    // ---- The replace question ----

    /// Notice the question opening: the keyboard goes to Cancel, the name box lets go of the focus, and the title is to be
    /// announced.
    fn note_question(&mut self, ctx: &Context, look: &Look) {
        let now = self.replace_question().map(str::to_owned);
        if now == self.ui.asked {
            return;
        }
        if let Some(name) = &now {
            self.ui.replace_focus = 1;
            self.ui.focus_name = false;
            self.ui.focus_list = false;
            self.ui.announce = Some(look.words.replace_title.replace("{name}", name));
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        self.ui.asked = now;
    }

    /// Draw the question over the dialog (outside its modal) and take a click on its buttons.
    fn question(&mut self, ctx: &Context, look: &Look) {
        let Some(name) = self.replace_question().map(str::to_owned) else { return };
        let words = look.words;
        let c = colors();
        let style = ito_confirm::Style {
            panel: c.panel,
            border: c.border_strong(),
            accent: Some(c.err),
            title: c.strong(),
            text: c.fg,
            dim: c.dim,
            warn: c.wait,
            button_bg: c.bg,
            button_text: c.fg,
        };
        let title = words.replace_title.replace("{name}", &name);
        let body = [words.replace_body.clone()];
        let buttons = [words.replace.clone(), words.cancel.clone()];
        let q = ito_confirm::Question {
            title: &title,
            body: &body,
            buttons: &buttons,
            primary: Some(0),
            danger: true,
            focus: Some(self.ui.replace_focus),
            ..Default::default()
        };
        match ito_confirm::show(ctx, &q, &style) {
            Some(0) => self.answer_replace(true),
            Some(_) => self.answer_no(),
            None => {}
        }
    }

    fn answer_no(&mut self) {
        self.answer_replace(false);
        self.ui.focus_name = true;
    }

    /// The keys while the question is open: Esc says no, Enter presses the button the keyboard is on, the arrows and Tab move
    /// between the two. Every key and text event of the frame is dropped, so nothing reaches the name box or the list.
    fn question_keys(&mut self, ctx: &Context) {
        let none = Modifiers::NONE;
        if take_key(ctx, none, Key::Escape) {
            self.answer_no();
        } else if take_key(ctx, none, Key::Enter) {
            if self.ui.replace_focus == 0 {
                self.answer_replace(true);
            } else {
                self.answer_no();
            }
        } else if take_key(ctx, none, Key::ArrowLeft) {
            self.ui.replace_focus = 0;
        } else if take_key(ctx, none, Key::ArrowRight) {
            self.ui.replace_focus = 1;
        } else if take_key(ctx, none, Key::Tab) || take_key(ctx, Modifiers::SHIFT, Key::Tab) {
            self.ui.replace_focus = 1 - self.ui.replace_focus.min(1);
        }
        ctx.input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_) | Event::Ime(_) | Event::Paste(_) | Event::Copy | Event::Cut))
        });
    }

    // ---- Keys ----

    fn keys(&mut self, ctx: &Context, mine: LayerId) {
        ctx.input(|i| {
            for e in &i.events {
                if let Event::Ime(ime) = e {
                    match ime {
                        ImeEvent::Preedit { text, .. } => self.ui.composing = !text.is_empty(),
                        ImeEvent::Commit(_) => self.ui.composing = false,
                        _ => {}
                    }
                }
            }
        });
        let top = ctx.memory(|m| m.top_modal_layer()) == Some(mine);
        let focus = ctx.memory(|m| m.focused());
        // egui clears the focus on Escape before this runs, so the end of the last frame counts too.
        let over = self.ui.foreign_focus || foreign_focus(ctx, mine);
        if self.replace_question().is_some() {
            if top && !egui::Popup::is_any_open(ctx) && !over {
                self.question_keys(ctx);
            }
            return;
        }
        if !top || egui::Popup::is_any_open(ctx) || over || self.ui.composing {
            return;
        }
        let none = Modifiers::NONE;
        if take_key(ctx, none, Key::Escape) {
            if self.ui.editing {
                self.ui.editing = false;
                self.ui.focus_list = true;
                self.clear_path_error();
            } else {
                self.cancel();
                return;
            }
        }
        if take_key(ctx, Modifiers::ALT, Key::ArrowLeft)
            || (cfg!(target_os = "macos") && take_key(ctx, Modifiers::MAC_CMD, Key::OpenBracket))
        {
            self.back();
        }
        if take_key(ctx, Modifiers::ALT, Key::ArrowRight)
            || (cfg!(target_os = "macos") && take_key(ctx, Modifiers::MAC_CMD, Key::CloseBracket))
        {
            self.forward();
        }
        if take_key(ctx, Modifiers::ALT, Key::ArrowUp) || (cfg!(target_os = "macos") && take_key(ctx, Modifiers::MAC_CMD, Key::ArrowUp)) {
            self.up();
        }
        if take_key(ctx, none, Key::F5) || (cfg!(target_os = "macos") && take_key(ctx, Modifiers::MAC_CMD, Key::R)) {
            self.refresh();
        }
        if take_key(ctx, Modifiers::COMMAND, Key::L) || take_key(ctx, Modifiers::ALT, Key::D) {
            self.begin_edit();
        }
        if take_key(ctx, Modifiers::COMMAND, Key::H) {
            let on = !self.hidden_shown();
            self.set_show_hidden(on);
        }
        // With nothing focused (after a click on a label, a failed folder), Enter is the default button.
        if focus.is_none() && take_key(ctx, none, Key::Enter) {
            self.accept();
        }
        let name_has = focus.is_some() && focus == self.ui.name_id;
        let list_has = focus.is_some() && focus == self.ui.list_id;
        if name_has && take_key(ctx, none, Key::ArrowDown) {
            self.ui.focus_list = true;
        }
        if !list_has {
            return;
        }
        let page = self.ui.page_rows.max(1) as isize;
        if take_key(ctx, none, Key::ArrowDown) {
            self.move_cursor(1, false);
        }
        if take_key(ctx, none, Key::ArrowUp) {
            self.move_cursor(-1, false);
        }
        if take_key(ctx, Modifiers::SHIFT, Key::ArrowDown) {
            self.move_cursor(1, true);
        }
        if take_key(ctx, Modifiers::SHIFT, Key::ArrowUp) {
            self.move_cursor(-1, true);
        }
        // Shift extends the selection from the anchor in many-file mode; in the single modes it moves the cursor like the plain key.
        for (shift, mods) in [(false, none), (true, Modifiers::SHIFT)] {
            for (key, by) in [(Key::PageDown, page), (Key::PageUp, -page), (Key::Home, isize::MIN / 2), (Key::End, isize::MAX / 2)] {
                if take_key(ctx, mods, key) {
                    self.move_cursor(by, shift);
                }
            }
        }
        if take_key(ctx, Modifiers::COMMAND, Key::A) {
            self.select_all();
        }
        if take_key(ctx, Modifiers::COMMAND, Key::Space) {
            if let Some(r) = self.cursor_row() {
                self.click(r, true, false);
            }
        }
        if take_key(ctx, none, Key::Backspace) {
            self.up();
        }
        if take_key(ctx, none, Key::Enter) {
            match self.cursor_row() {
                Some(r) => self.open_row(r),
                None => self.accept(),
            }
        }
        let (events, mods) = ctx.input(|i| (i.events.clone(), i.modifiers));
        if !(mods.command && !mods.alt) {
            for input in ito_keys::read(&events, &mods, |_| false) {
                if let Input::Text(t) = input {
                    self.type_ahead(&t, Instant::now());
                }
            }
        }
    }

    fn begin_edit(&mut self) {
        self.ui.editing = true;
        self.ui.focus_path = true;
        self.ui.path_text = self.target().to_owned();
    }

    // ---- Drawing ----

    fn window(&mut self, ctx: &Context, look: &Look) {
        let c = colors();
        let screen = ctx.content_rect();
        let w = fit(sc(ctx, 880.0), screen.width());
        let h = fit(sc(ctx, 580.0), screen.height());
        let frame = Frame::new()
            .fill(c.panel)
            .stroke(Stroke::new(1.0, c.border_strong()))
            .corner_radius(CornerRadius::same(radius::BOX))
            .inner_margin(Margin::ZERO)
            .shadow(Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(90) });
        let area = Modal::area(self.id());
        egui::Modal::new(Id::new(("ito-filepick", self.id().to_owned())))
            .area(area)
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(110))
            .show(ctx, |ui| {
                let (outer, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
                if self.replace_question().is_some() {
                    ui.disable();
                }
                self.contents(ui, outer, look);
            });
    }

    /// Whether the filter box is drawn at all (not in the folder mode: no files are listed there).
    fn filter_shown(&self) -> bool {
        !self.filter_list().is_empty() && self.mode() != Mode::OpenFolder
    }

    /// Whether the filter goes on its own row under the name box: `min(200, 35% of the row)` is under the select's own width.
    fn filter_stacked(&self, row_w: f32) -> bool {
        self.filter_shown() && row_w * 0.35 < SELECT_W
    }

    fn footer_height(&self, ctx: &Context, row_w: f32) -> f32 {
        let s = |v: f32| sc(ctx, v);
        let stacked = if self.filter_stacked(row_w) { s(4.0) + s(30.0) } else { 0.0 };
        s(30.0) + stacked + s(4.0) + s(18.0) + s(4.0) + s(30.0)
    }

    fn contents(&mut self, ui: &mut Ui, outer: Rect, look: &Look) {
        let ctx = ui.ctx().clone();
        let s = |v: f32| sc(&ctx, v);
        let inner = outer.shrink2(vec2(s(16.0), 0.0));
        let title_r = Rect::from_min_size(pos2(inner.min.x, outer.min.y + s(12.0)), vec2(inner.width(), s(28.0)));
        let tool_r = Rect::from_min_size(pos2(inner.min.x, title_r.max.y + s(8.0)), vec2(inner.width(), s(30.0)));
        // A path error takes a line under the bar and pushes the body down while it shows.
        let err_h = if self.path_error().is_some() { s(20.0) } else { 0.0 };
        let foot_h = self.footer_height(&ctx, inner.width());
        let foot_r = Rect::from_min_max(pos2(inner.min.x, outer.max.y - s(12.0) - foot_h), pos2(inner.max.x, outer.max.y - s(12.0)));
        let body_top = tool_r.max.y + err_h + s(8.0);
        let body_r = Rect::from_min_max(pos2(inner.min.x, body_top), pos2(inner.max.x, (foot_r.min.y - s(8.0)).max(body_top + s(32.0))));

        // The tab order is the order of drawing: the footer first (the name box has the focus), then the top, then the body.
        self.footer(ui, foot_r, look);
        self.title_row(ui, title_r, look);
        self.toolbar(ui, tool_r, look);
        let show_places = outer.width() >= s(600.0) && !self.place_list().is_empty();
        let places_w = if show_places { s(180.0) } else { 0.0 };
        if show_places {
            let pr = Rect::from_min_size(body_r.min, vec2(places_w, body_r.height()));
            self.places_column(ui, pr, look);
        }
        let list_r = Rect::from_min_max(pos2(body_r.min.x + if show_places { places_w + s(8.0) } else { 0.0 }, body_r.min.y), body_r.max);
        self.list(ui, list_r, look);

        // The first frame: the name box has the focus. Later: put it back where the person left it.
        let focus_id = if self.ui.focus_list {
            self.ui.list_id
        } else if self.ui.focus_name || !self.ui.started {
            self.ui.name_id
        } else {
            None
        };
        if self.replace_question().is_some() {
            self.ui.started = true;
        } else if let Some(id) = focus_id {
            let first = !self.ui.started;
            ctx.memory_mut(|mem| mem.request_focus(id));
            self.ui.focus_list = false;
            self.ui.focus_name = false;
            self.ui.started = true;
            if first && self.mode() == Mode::SaveFile && Some(id) == self.ui.name_id {
                // Save as: the name is selected up to its extension, so typing replaces the stem.
                let name = self.name();
                let end = if crate::extension(name).is_some() {
                    name.rfind('.').map_or(name.len(), |b| name[..b].chars().count())
                } else {
                    name.chars().count()
                };
                if let Some(mut st) = egui::TextEdit::load_state(&ctx, id) {
                    let range = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end));
                    st.cursor.set_char_range(Some(range));
                    st.store(&ctx, id);
                }
            }
        }
        if let Some(text) = self.ui.announce.take() {
            ito_a11y::announce(ui, &text);
        }
    }

    fn title_row(&mut self, ui: &mut Ui, r: Rect, look: &Look) {
        let c = colors();
        let ctx = ui.ctx().clone();
        let words = look.words;
        let btn = sc(&ctx, 28.0);
        let title = self.custom_title().unwrap_or_else(|| words.title(self.mode())).to_owned();
        at(ui, r, ltr(), |ui| {
            ui.label(RichText::new(&title).size(z(&ctx, 16.0)).strong().color(c.strong()));
        });
        let close = Rect::from_min_size(pos2(r.max.x - btn, r.min.y), vec2(btn, btn));
        let resp = at(ui, close, ltr(), |ui| ito_prefs::mark_button(ui, Mark::Close, &words.close, 28.0));
        self.put_id(&resp, "close");
        if resp.clicked() {
            self.cancel();
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, r: Rect, look: &Look) {
        let ctx = ui.ctx().clone();
        let s = |v: f32| sc(&ctx, v);
        let words = look.words;
        let btn = s(28.0);
        let mut x = r.min.x;
        let nav = [
            (Mark::ChevronLeft, &words.back, self.can_back(), "back"),
            (Mark::ChevronRight, &words.forward, self.can_forward(), "forward"),
            (Mark::ChevronUp, &words.up, self.can_up(), "up"),
        ];
        for (k, (mark, said, enabled, name)) in nav.into_iter().enumerate() {
            let rect = Rect::from_min_size(pos2(x, r.min.y + s(1.0)), vec2(btn, btn));
            let resp = at(ui, rect, ltr(), |ui| ui.add_enabled_ui(enabled, |ui| ito_prefs::mark_button(ui, mark, said, 28.0)).inner);
            self.put_id(&resp, name);
            if resp.clicked() {
                match k {
                    0 => self.back(),
                    1 => self.forward(),
                    _ => self.up(),
                }
            }
            x += s(32.0);
        }
        let refresh = Rect::from_min_size(pos2(r.max.x - btn, r.min.y + s(1.0)), vec2(btn, btn));
        let bar = Rect::from_min_max(pos2(x + s(4.0), r.min.y), pos2(refresh.min.x - s(8.0), r.max.y));
        self.path_bar(ui, bar, look);
        self.path_error_line(ui, bar, look);
        let resp = at(ui, refresh, ltr(), |ui| ito_prefs::mark_button(ui, Mark::Refresh, &words.refresh, 28.0));
        self.put_id(&resp, "refresh");
        if resp.clicked() {
            self.refresh();
        }
    }

    /// Why the typed path could not be read: a line right under the bar. Said once.
    fn path_error_line(&mut self, ui: &mut Ui, bar: Rect, look: &Look) {
        let Some(e) = self.path_error() else {
            self.ui.said_path.clear();
            return;
        };
        let text = look.words.path_error(e);
        let c = colors();
        let ctx = ui.ctx().clone();
        let s = |v: f32| sc(&ctx, v);
        let r = Rect::from_min_size(pos2(bar.min.x, bar.max.y + s(2.0)), vec2(bar.width(), s(18.0)));
        let icon = Rect::from_center_size(pos2(r.min.x + s(7.0), r.center().y), Vec2::splat(s(13.0)));
        ito_prefs::mark(ui.painter(), icon, Mark::Warning, c.err);
        let g = elided(ui, &text, FontId::proportional(z(&ctx, 12.0)), c.err, r.width() - s(20.0));
        ui.painter().galley(pos2(r.min.x + s(20.0), r.center().y - g.size().y / 2.0), g, c.err);
        let resp = ui.interact(r, Id::new((self.id().to_owned(), "path-error")), Sense::hover());
        ito_a11y::say(&resp, Role::Label, &text);
        self.put_id(&resp, "path.error");
        if self.ui.said_path != text {
            ito_a11y::announce(ui, &text);
            self.ui.said_path = text;
        }
    }

    fn path_bar(&mut self, ui: &mut Ui, bar: Rect, look: &Look) {
        let c = colors();
        let ctx = ui.ctx().clone();
        let s = |v: f32| sc(&ctx, v);
        let words = look.words;
        if self.ui.editing {
            let mut text = std::mem::take(&mut self.ui.path_text);
            let resp = at(ui, bar, ltr(), |ui| {
                ito_prefs::InputField::new(&format!("{}_path", self.id()))
                    .name(&words.path)
                    .mono()
                    .width(bar.width())
                    .error(self.path_error().is_some())
                    .show(ui, &mut text)
            });
            self.ui.path_id = Some(resp.id);
            self.put_id(&resp, "path");
            lock_keys(&ctx, resp.id);
            if resp.changed() {
                self.clear_path_error();
            }
            if self.ui.focus_path {
                self.ui.focus_path = false;
                ctx.memory_mut(|m| m.request_focus(resp.id));
                if let Some(mut st) = egui::TextEdit::load_state(&ctx, resp.id) {
                    let end = egui::text::CCursor::new(text.chars().count());
                    // A fresh edit selects all; after a bad path the caret goes to the end of what was typed.
                    let start = if self.ui.keep_caret { end } else { egui::text::CCursor::new(0) };
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(start, end)));
                    st.store(&ctx, resp.id);
                }
                self.ui.keep_caret = false;
            } else if resp.lost_focus() {
                let enter = ctx.input(|i| i.key_pressed(Key::Enter));
                if enter {
                    let typed = text.clone();
                    self.go(&typed);
                    if self.path_error().is_some() {
                        // The edit stays open with the draft, so the person can fix it.
                        self.ui.focus_path = true;
                        self.ui.keep_caret = true;
                    } else {
                        self.ui.editing = false;
                        self.ui.focus_list = true;
                    }
                } else {
                    // Left without Enter (Esc, a click elsewhere): the draft is dropped, and its error with it.
                    self.ui.editing = false;
                    self.clear_path_error();
                }
            }
            self.ui.path_text = text;
            return;
        }
        let ph = s(28.0);
        let frame = Rect::from_min_size(bar.min, vec2(bar.width(), ph.min(bar.height())));
        ui.painter().rect_filled(frame, radius::CONTROL, c.bg);
        ui.painter().rect_stroke(frame, radius::CONTROL, Stroke::new(1.0, c.border), StrokeKind::Inside);
        let bar_resp = ui.interact(frame, Id::new((self.id().to_owned(), "path-bar")), Sense::click());
        self.put_id(&bar_resp, "path");
        ito_a11y::say(&bar_resp, Role::Button, &words.edit_path);
        let pencil = Rect::from_min_size(pos2(frame.max.x - s(26.0), frame.min.y + s(2.0)), vec2(s(24.0), s(24.0)));
        let font = FontId::proportional(z(&ctx, 12.5));
        let crumbs = self.style().crumbs(self.target());
        let n = crumbs.len();
        let pad = s(6.0);
        let sep_w = s(14.0);
        let min_last = s(40.0);
        let widths: Vec<f32> = crumbs.iter().map(|(l, _)| text_w(ui, l, font.clone()) + 2.0 * pad).collect();
        let avail = (pencil.min.x - frame.min.x - s(12.0)).max(min_last);
        let ell_w = text_w(ui, "\u{2026}", font.clone()) + 2.0 * pad;
        // The root and the last crumb always show. When the path does not fit, one ellipsis stands right after the root for
        // the middle crumbs, and as many crumbs from the end as fit stay.
        let all: f32 = widths.iter().sum::<f32>() + sep_w * n.saturating_sub(1) as f32;
        let mut start = 1.min(n);
        let mut elide_mid = false;
        if n > 2 && all > avail {
            elide_mid = true;
            start = n - 1;
            let mut used = widths[0] + sep_w + ell_w + sep_w + widths[n - 1];
            while start > 1 {
                let more = widths[start - 1] + sep_w;
                if used + more > avail {
                    break;
                }
                used += more;
                start -= 1;
            }
            if start == 1 {
                elide_mid = false;
            }
        }
        let mut x = frame.min.x + pad;
        let mid = frame.center().y;
        let mut clicked: Option<String> = None;
        let mut edit_asked = false;
        let mut item = |ui: &mut Ui, x: &mut f32, label: &str, path: &str, idx: Option<usize>, w: f32, last: bool, s: &Self| {
            let rect = Rect::from_center_size(pos2(*x + w / 2.0, mid), vec2(w, sc(ui.ctx(), 24.0)));
            let id = Id::new((s.id().to_owned(), "crumb", idx.unwrap_or(usize::MAX)));
            let resp = ui.interact(rect, id, Sense::CLICK).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                ui.painter().rect_filled(rect, radius::MARK, c.hover());
            }
            let color = if last { c.strong() } else { c.dim };
            let g = elided(ui, label, font.clone(), color, w - 2.0 * pad);
            ui.painter().galley(pos2(rect.min.x + pad, rect.center().y - g.size().y / 2.0), g, color);
            ito_a11y::say(&resp, Role::Button, if idx.is_none() { path } else { label });
            if let Some(i) = idx {
                ito_a11y::test_id(&resp, &s.tid(&format!("path.{i}")));
            }
            if resp.clicked() {
                if idx.is_some() {
                    clicked = Some(path.to_owned());
                } else {
                    edit_asked = true;
                }
            }
            *x += w;
        };
        let sep = |ui: &mut Ui, x: &mut f32| {
            let g = ui.painter().layout_no_wrap("\u{203a}".to_owned(), font.clone(), c.faint());
            ui.painter().galley(pos2(*x + (sep_w - g.size().x) / 2.0, mid - g.size().y / 2.0), g, c.faint());
            *x += sep_w;
        };
        if n > 0 {
            let w0 = if n == 1 { widths[0].min(avail).max(min_last) } else { widths[0] };
            item(ui, &mut x, &crumbs[0].0, &crumbs[0].1, Some(0), w0, n == 1, self);
        }
        if elide_mid {
            sep(ui, &mut x);
            let whole = self.target().to_owned();
            item(ui, &mut x, "\u{2026}", &whole, None, ell_w, false, self);
        }
        for (i, (label, path)) in crumbs.iter().enumerate().skip(start) {
            sep(ui, &mut x);
            let w = if i == n - 1 { widths[i].min(avail - (x - frame.min.x - pad)).max(min_last) } else { widths[i] };
            item(ui, &mut x, label, path, Some(i), w, i == n - 1, self);
        }
        let pen = at(ui, pencil, ltr(), |ui| ito_prefs::mark_button(ui, Mark::Pencil, &words.edit_path, 24.0));
        self.put_id(&pen, "edit");
        match clicked {
            Some(p) if !edit_asked => self.go(&p),
            _ if edit_asked || pen.clicked() || bar_resp.clicked() => self.begin_edit(),
            _ => {}
        }
    }

    fn places_column(&mut self, ui: &mut Ui, r: Rect, look: &Look) {
        let c = colors();
        let ctx = ui.ctx().clone();
        let s = |v: f32| sc(&ctx, v);
        ui.painter().rect_filled(r, radius::CONTROL, c.side);
        let places: Vec<_> = self.place_list().to_vec();
        let lit = self.lit_place();
        let font = FontId::proportional(z(&ctx, 12.5));
        let mut clicked = None;
        at(ui, r.shrink(s(4.0)), Layout::top_down(Align::Min), |ui| {
            ui.label(RichText::new(&look.words.places).size(z(&ctx, 11.0)).color(c.dim));
            ScrollArea::vertical().id_salt("places").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = s(2.0);
                for (k, p) in places.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), s(28.0)), Sense::click());
                    if lit == Some(k) {
                        ui.painter().rect_filled(rect, radius::CONTROL, c.selection());
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, radius::CONTROL, c.hover());
                    }
                    let mark = match p.kind {
                        crate::PlaceKind::Home => Mark::Person,
                        crate::PlaceKind::Root | crate::PlaceKind::Drive => Mark::Server,
                        crate::PlaceKind::Folder => Mark::Folder,
                    };
                    let icon = Rect::from_center_size(pos2(rect.min.x + s(14.0), rect.center().y), Vec2::splat(s(14.0)));
                    ito_prefs::mark(ui.painter(), icon, mark, c.dim);
                    let g = elided(ui, &p.label, font.clone(), c.fg, rect.width() - s(32.0));
                    ui.painter().galley(pos2(rect.min.x + s(28.0), rect.center().y - g.size().y / 2.0), g, c.fg);
                    ito_a11y::say(&resp, Role::Button, &p.label);
                    ito_prefs::focus_ring(ui, &resp);
                    self.put_id(&resp, &format!("place.{k}"));
                    if resp.clicked() {
                        clicked = Some(p.path.clone());
                    }
                }
            });
        });
        if let Some(p) = clicked {
            self.go(&p);
        }
    }

    fn list(&mut self, ui: &mut Ui, r: Rect, look: &Look) {
        let c = colors();
        let ctx = ui.ctx().clone();
        let words = look.words;
        ui.painter().rect_filled(r, radius::CONTROL, c.bg);
        ui.painter().rect_stroke(r, radius::CONTROL, Stroke::new(1.0, c.border), StrokeKind::Inside);
        let list_id = Id::new((self.id().to_owned(), "list"));
        let list_resp = ui.interact(r, list_id, Sense::focusable_noninteractive());
        self.ui.list_id = Some(list_id);
        lock_keys(&ctx, list_id);
        self.put_id(&list_resp, "list");
        let s = |v: f32| sc(&ctx, v);
        let hdr_h = z(&ctx, height::ROW);
        let row_h = z(&ctx, 26.0);
        let hdr = Rect::from_min_size(r.min, vec2(r.width(), hdr_h));
        let body = Rect::from_min_max(pos2(r.min.x, hdr.max.y), r.max);

        // Columns, from the right: size, kind, modified (they give way when narrow); the name takes the rest.
        let scroll_w = s(12.0);
        let wide = r.width();
        let size_w = s(80.0);
        let kind_w = if wide >= s(520.0) { s(120.0) } else { 0.0 };
        let mod_w = if wide >= s(400.0) { s(140.0) } else { 0.0 };
        let right = r.max.x - scroll_w;
        let size_x = right - size_w;
        let kind_x = size_x - kind_w;
        let mod_x = kind_x - mod_w;
        let name_x = r.min.x + s(8.0);
        let name_w = (mod_x - name_x - s(8.0)).max(s(40.0));

        // Header: the sort cells.
        ui.painter().rect_filled(hdr, CornerRadius { nw: radius::CONTROL, ne: radius::CONTROL, sw: 0, se: 0 }, c.side);
        let (sort, desc) = self.sort_state();
        let cells = [
            (Sort::Name, &words.col_name, name_x - s(4.0), name_w + s(8.0), "name"),
            (Sort::Modified, &words.col_modified, mod_x, mod_w, "modified"),
            (Sort::Kind, &words.col_kind, kind_x, kind_w, "kind"),
            (Sort::Size, &words.col_size, size_x, size_w, "size"),
        ];
        let mut sort_to = None;
        for (by, label, x, w, name) in cells {
            if w <= 0.0 {
                continue;
            }
            let rect = Rect::from_min_size(pos2(x, hdr.min.y), vec2(w, hdr_h));
            let resp = ui
                .interact(rect, Id::new((self.id().to_owned(), "sort", name)), Sense::CLICK)
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() {
                ui.painter().rect_filled(rect, 0.0, c.hover());
            }
            let active = by == sort;
            let color = if active { c.strong() } else { c.dim };
            let g = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(z(&ctx, 11.5)), color);
            let right_aligned = by == Sort::Size;
            let tx =
                if right_aligned { rect.max.x - s(8.0) - g.size().x - if active { s(14.0) } else { 0.0 } } else { rect.min.x + s(4.0) };
            ui.painter().galley(pos2(tx, rect.center().y - g.size().y / 2.0), g.clone(), color);
            if active {
                let mark = if desc { Mark::ChevronDown } else { Mark::ChevronUp };
                let at = Rect::from_center_size(pos2(tx + g.size().x + s(8.0), rect.center().y), Vec2::splat(s(10.0)));
                ito_prefs::mark(ui.painter(), at, mark, color);
            }
            ito_a11y::say(&resp, Role::Button, label);
            self.put_id(&resp, &format!("sort.{name}"));
            if resp.clicked() {
                sort_to = Some(by);
            }
        }
        if let Some(by) = sort_to {
            self.set_sort(by);
        }

        // Loading text only after a short wait, so a quick answer does not flash.
        let now = ctx.input(|i| i.time);
        if matches!(self.status(), Status::Loading) {
            let since = *self.ui.loading_since.get_or_insert(now);
            self.ui.loading_visible = now - since >= LOADING_AFTER;
            if !self.ui.loading_visible {
                ctx.request_repaint_after(Duration::from_millis(160));
            }
        } else {
            self.ui.loading_since = None;
            self.ui.loading_visible = false;
        }

        // The status line for a reader, once per change.
        let said = match self.status() {
            Status::Loading if self.ui.loading_visible => Some(words.loading.clone()),
            Status::Failed(t) => Some(format!("{}: {t}", words.failed)),
            _ => None,
        };
        if let Some(text) = said {
            if self.ui.said_status != text {
                ito_a11y::announce(ui, &text);
                self.ui.said_status = text;
            }
        } else if !matches!(self.status(), Status::Loading) {
            self.ui.said_status.clear();
        }

        // Empty states fill the body.
        let look_p = ito_prefs::Look { c, q: "" };
        let state_rect = body.shrink(s(8.0));
        match self.status() {
            Status::Loading => {
                if self.ui.loading_visible {
                    // A small spinner and the text, the pair centered in the body. The spinner repaints itself while it shows.
                    let font = FontId::proportional(z(&ctx, 13.0));
                    let g = ui.painter().layout_no_wrap(words.loading.clone(), font, c.dim);
                    let (sp, gap) = (s(14.0), s(8.0));
                    let left = body.center().x - (sp + gap + g.size().x) / 2.0;
                    let spin = Rect::from_center_size(pos2(left + sp / 2.0, body.center().y), Vec2::splat(sp));
                    ui.put(spin, egui::Spinner::new().size(sp).color(c.dim));
                    ui.painter().galley(pos2(left + sp + gap, body.center().y - g.size().y / 2.0), g, c.dim);
                }
                return;
            }
            Status::Failed(t) => {
                let t = t.to_owned();
                let r = at(ui, state_rect, Layout::top_down(Align::Center), |ui| {
                    ui.add_space(body.height() * 0.2);
                    ito_prefs::empty_state(ui, look_p, Mark::Warning, &words.failed, &t, Some(&words.retry))
                });
                if let Some(resp) = r {
                    self.put_id(&resp, "retry");
                    if resp.clicked() {
                        self.retry();
                        self.ui.focus_list = true;
                    }
                }
                return;
            }
            Status::Ready => {}
        }
        if self.rows().is_empty() {
            let (title, hint) =
                if self.entry_count() == 0 { (&words.empty, &words.empty_hint) } else { (&words.no_match, &words.no_match_hint) };
            at(ui, state_rect, Layout::top_down(Align::Center), |ui| {
                ui.add_space(body.height() * 0.2);
                ito_prefs::empty_state(ui, look_p, Mark::Folder, title, hint, None);
            });
            return;
        }

        // Keep the cursor row in sight, whatever moved it.
        let n = self.rows().len();
        let vh = body.height();
        self.ui.page_rows = ((vh / row_h).floor() as usize).saturating_sub(1).max(1);
        let cur = self.cursor_row();
        let mut offset = None;
        if cur != self.ui.last_cursor {
            self.ui.last_cursor = cur;
            if let Some(row) = cur {
                let top = row as f32 * row_h;
                let off = self.ui.scroll_y;
                if top < off {
                    offset = Some(top);
                } else if top + row_h > off + vh {
                    offset = Some(top + row_h - vh);
                }
            }
        }

        let focused = ctx.memory(|m| m.has_focus(list_id));
        // A script (or a window system) can put the modifiers on the click's own event and not on the frame.
        let mods = ctx.input(|i| {
            let mut m = i.modifiers;
            for e in &i.events {
                if let Event::PointerButton { modifiers, .. } = e {
                    m = Modifiers {
                        alt: m.alt || modifiers.alt,
                        ctrl: m.ctrl || modifiers.ctrl,
                        shift: m.shift || modifiers.shift,
                        mac_cmd: m.mac_cmd || modifiers.mac_cmd,
                        command: m.command || modifiers.command,
                    };
                }
            }
            m
        });
        let mut area = ScrollArea::vertical().id_salt("rows").auto_shrink([false, false]).max_height(vh);
        if let Some(o) = offset {
            area = area.vertical_scroll_offset(o);
        }
        let mut clicked = None;
        let mut opened = None;
        let mut active_row: Option<Id> = None;
        let out = at(ui, body, Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            area.show_rows(ui, row_h, n, |ui, range| {
                let clip = ui.clip_rect();
                for row in range {
                    let Some(e) = self.row_entry(row) else { continue };
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::CLICK);
                    let seen = rect.intersects(clip);
                    let selected = self.is_selected(row);
                    let pickable = self.pickable(e);
                    let is_cursor = self.cursor_row() == Some(row);
                    if selected {
                        ui.painter().rect_filled(rect.shrink2(vec2(2.0, 0.0)), radius::MARK, c.selection());
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect.shrink2(vec2(2.0, 0.0)), radius::MARK, c.hover());
                    }
                    if is_cursor && focused {
                        ui.painter().rect_stroke(rect.shrink2(vec2(2.0, 1.0)), radius::MARK, Stroke::new(1.5, c.run), StrokeKind::Inside);
                    }
                    let text_color = if pickable { c.fg } else { c.faint() };
                    let icon_rect = Rect::from_center_size(pos2(name_x + s(8.0), rect.center().y), Vec2::splat(s(14.0)));
                    let mark = if e.kind == Kind::Dir { Mark::Folder } else { Mark::File };
                    ito_prefs::mark(ui.painter(), icon_rect, mark, if pickable { c.dim } else { c.faint() });
                    let shown_name = display_name(&e.name);
                    let g = elided(ui, &shown_name, FontId::proportional(z(&ctx, 13.0)), text_color, name_w - s(24.0));
                    let gx = name_x + s(22.0);
                    ui.painter().galley(pos2(gx, rect.center().y - g.size().y / 2.0), g.clone(), text_color);
                    if e.link && gx + g.size().x + s(16.0) < mod_x {
                        let l = Rect::from_center_size(pos2(gx + g.size().x + s(10.0), rect.center().y), Vec2::splat(s(10.0)));
                        ito_prefs::mark(ui.painter(), l, Mark::Link, c.dim);
                    }
                    let small = FontId::proportional(z(&ctx, 12.0));
                    if mod_w > 0.0 {
                        if let Some(secs) = e.modified {
                            let t = when(look, secs);
                            let g = elided(ui, &t, small.clone(), c.dim, mod_w - s(8.0));
                            ui.painter().galley(pos2(mod_x + s(4.0), rect.center().y - g.size().y / 2.0), g, c.dim);
                        }
                    }
                    let kind_text = words.kind_word(e.kind, e.extension().as_deref());
                    if kind_w > 0.0 {
                        let g = elided(ui, &kind_text, small.clone(), c.dim, kind_w - s(8.0));
                        ui.painter().galley(pos2(kind_x + s(4.0), rect.center().y - g.size().y / 2.0), g, c.dim);
                    }
                    if let (Kind::File, Some(bytes)) = (e.kind, e.size) {
                        let t = crate::format_size(bytes);
                        let g = ui.painter().layout_no_wrap(t, small.clone(), c.dim);
                        ui.painter().galley(pos2(right - s(8.0) - g.size().x, rect.center().y - g.size().y / 2.0), g, c.dim);
                    }
                    if seen {
                        let mut label = format!("{}, {kind_text}", e.name);
                        if let (Kind::File, Some(bytes)) = (e.kind, e.size) {
                            label.push_str(&format!(", {}", crate::format_size(bytes)));
                        }
                        resp.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, &label));
                        ctx.accesskit_node_builder(resp.id, |node| node.set_role(egui::accesskit::Role::ListBoxOption));
                        ito_a11y::test_id_in(ui, &resp, &self.tid(&format!("row.{row}")), Some(&g));
                        if is_cursor {
                            active_row = Some(resp.id);
                        }
                    }
                    if resp.clicked() {
                        clicked = Some(row);
                    }
                    if resp.double_clicked() {
                        opened = Some(row);
                    }
                }
            })
        });
        self.ui.scroll_y = out.state.offset.y;
        let multi = self.mode() == Mode::OpenFiles;
        ctx.accesskit_node_builder(list_id, |node| {
            node.set_role(egui::accesskit::Role::ListBox);
            node.set_label(self.title_text(words));
            if multi {
                node.set_multiselectable();
            }
            if let Some(id) = active_row {
                node.set_active_descendant(id.accesskit_id());
            }
        });
        if let Some(row) = clicked {
            self.ui.focus_list = true;
            self.click(row, mods.command, mods.shift);
        }
        if let Some(row) = opened {
            self.open_row(row);
        }
    }

    fn title_text<'a>(&'a self, words: &'a Words) -> String {
        self.custom_title().unwrap_or_else(|| words.title(self.mode())).to_owned()
    }

    fn footer(&mut self, ui: &mut Ui, r: Rect, look: &Look) {
        let c = colors();
        let ctx = ui.ctx().clone();
        let s = |v: f32| sc(&ctx, v);
        let words = look.words;

        // Row A: the name, and the filter (beside it, or on its own row under it when the window is narrow).
        let row_h = s(30.0);
        let row_a = Rect::from_min_size(r.min, vec2(r.width(), row_h));
        let has_filter = self.filter_shown();
        let stacked = self.filter_stacked(r.width());
        let side = has_filter && !stacked;
        let name_r = Rect::from_min_max(row_a.min, pos2(row_a.max.x - if side { SELECT_W + s(8.0) } else { 0.0 }, row_a.max.y));
        let label = if self.mode() == Mode::OpenFolder { &words.folder_name } else { &words.file_name };
        let label_w = (text_w(ui, label, FontId::proportional(z(&ctx, 12.5))) + s(12.0)).clamp(s(60.0), s(160.0));
        let mut text = self.name().to_owned();
        let resp = at(ui, name_r, ltr(), |ui| {
            ui.allocate_ui(vec2(label_w, s(28.0)), |ui| ui.label(RichText::new(label).size(z(&ctx, 12.5)).color(c.dim)));
            ito_prefs::InputField::new(&format!("{}_name", self.id()))
                .name(label)
                .width((name_r.width() - label_w).max(s(60.0)))
                .error(self.error().is_some())
                .show(ui, &mut text)
        });
        self.ui.name_id = Some(resp.id);
        self.put_id(&resp, "name");
        lock_keys(&ctx, resp.id);
        if resp.changed() {
            self.set_name(&text);
        }
        if resp.lost_focus() && ctx.input(|i| i.key_pressed(Key::Enter)) && !self.ui.composing && self.window_active(&ctx) {
            // Save as: Enter does what the button does, so a disabled button means Enter does nothing.
            if self.mode() != Mode::SaveFile || self.can_accept() {
                self.accept();
            }
            // A replace question that this Enter opened has the keyboard.
            self.ui.focus_name = self.replace_question().is_none();
        }
        // The select has the width ito-prefs gives it, so its slot is that wide and ends at the footer's right edge; it is
        // centred on the name box's middle (the select of ito-prefs takes no height).
        let mut below = r.min.y + row_h;
        if has_filter {
            let fy = if stacked { below + s(4.0) } else { row_a.min.y };
            let fr = Rect::from_min_size(pos2(r.max.x - SELECT_W, fy), vec2(SELECT_W, row_h));
            let filters = self.filter_list().len();
            let mut opts: Vec<(usize, &str)> = self.filter_list().iter().enumerate().map(|(i, f)| (i, f.name.as_str())).collect();
            opts.push((filters, words.all_files.as_str()));
            let now = self.filter_position();
            if let Some(pick) =
                at(ui, fr, Layout::right_to_left(Align::Center), |ui| ito_prefs::select(ui, &format!("{}_filter", self.id()), now, &opts))
            {
                self.set_filter(pick);
            }
            if stacked {
                below = fr.max.y;
            }
        }

        // Row B: what is wrong, or a note.
        let row_b = Rect::from_min_size(pos2(r.min.x, below + s(4.0)), vec2(r.width(), s(18.0)));
        let line = if let Some(e) = self.error() {
            Some((words.error(e), c.err, true))
        } else if self.truncated() {
            Some((words.first_n(self.entry_count()), c.dim, false))
        } else if self.left_out() > 0 {
            Some((words.left_out_n(self.left_out()), c.dim, false))
        } else if self.mode() == Mode::OpenFiles && self.selected_count() > 1 {
            Some((words.selected_n.replace("{n}", &self.selected_count().to_string()), c.dim, false))
        } else {
            None
        };
        if let Some((text, color, bad)) = line {
            let x0 = row_b.min.x + label_w;
            if bad {
                let icon = Rect::from_center_size(pos2(x0 + s(7.0), row_b.center().y), Vec2::splat(s(13.0)));
                ito_prefs::mark(ui.painter(), icon, Mark::Warning, color);
            }
            let g = elided(ui, &text, FontId::proportional(z(&ctx, 12.0)), color, row_b.max.x - x0 - s(20.0));
            ui.painter().galley(pos2(x0 + if bad { s(20.0) } else { 0.0 }, row_b.center().y - g.size().y / 2.0), g, color);
            if self.ui.said_note != text && !matches!(self.status(), Status::Loading | Status::Failed(_)) {
                ito_a11y::announce(ui, &text);
                self.ui.said_note = text;
            }
        } else {
            self.ui.said_note.clear();
        }

        // Row C: hidden items, and the buttons.
        let row_c = Rect::from_min_size(pos2(r.min.x, row_b.max.y + s(4.0)), vec2(r.width(), row_h));
        let checked = self.hidden_shown();
        let font = FontId::proportional(z(&ctx, 12.5));
        let tw = text_w(ui, &words.show_hidden, font.clone());
        let check_r = Rect::from_min_size(pos2(row_c.min.x, row_c.min.y + s(3.0)), vec2(s(24.0) + tw + s(8.0), s(24.0)));
        let resp =
            ui.interact(check_r, Id::new((self.id().to_owned(), "hidden")), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let bx = Rect::from_min_size(pos2(check_r.min.x, check_r.center().y - s(8.0)), Vec2::splat(s(16.0)));
        if checked {
            ui.painter().rect_filled(bx, radius::MARK, c.accent());
            ito_prefs::mark(ui.painter(), bx.shrink(s(2.0)), Mark::Check, c.on_accent());
        } else {
            ui.painter().rect_filled(bx, radius::MARK, c.bg);
            ui.painter().rect_stroke(bx, radius::MARK, Stroke::new(1.0, c.border_strong()), StrokeKind::Inside);
        }
        let g = ui.painter().layout_no_wrap(words.show_hidden.clone(), font.clone(), c.fg);
        ui.painter().galley(pos2(check_r.min.x + s(24.0), check_r.center().y - g.size().y / 2.0), g, c.fg);
        let hidden_label = words.show_hidden.clone();
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, checked, &hidden_label));
        ito_prefs::focus_ring(ui, &resp);
        self.put_id(&resp, "hidden");
        if resp.clicked() {
            self.set_show_hidden(!checked);
        }

        // The drawn boxes are the slots: Cancel ends at the footer's right edge, 8 apart, each at least 76 wide.
        let ok_text = match self.mode() {
            Mode::OpenFolder => &words.choose,
            Mode::SaveFile if !self.opens_folder() => &words.save,
            _ => &words.open,
        };
        let pad = ui.spacing().button_padding.x * 2.0 + 10.0;
        let ok_w = (text_w(ui, ok_text, font.clone()) + pad).max(s(76.0));
        let cancel_w = (text_w(ui, &words.cancel, font) + pad).max(s(76.0));
        let cancel_r = Rect::from_min_size(pos2(row_c.max.x - cancel_w, row_c.min.y), vec2(cancel_w, row_h));
        let ok_r = Rect::from_min_size(pos2(cancel_r.min.x - s(8.0) - ok_w, row_c.min.y), vec2(ok_w, row_h));
        let look_p = ito_prefs::Look { c, q: "" };
        let can = self.can_accept();
        // Centre on the cross axis: egui places a button's words by the layout's horizontal align.
        let fill = Layout::top_down(Align::Center).with_cross_justify(true);
        let ok = at(ui, ok_r, fill, |ui| ui.add_enabled_ui(can, |ui| ito_prefs::button_with(ui, look_p, ok_text, Tone::Primary)).inner);
        self.put_id(&ok, "ok");
        let cancel = at(ui, cancel_r, fill, |ui| ito_prefs::button_with(ui, look_p, &words.cancel, Tone::Plain));
        self.put_id(&cancel, "cancel");
        if ok.clicked() {
            self.accept();
            self.ui.focus_name = self.ui.focus_name && self.replace_question().is_none();
        }
        if cancel.clicked() {
            self.cancel();
        }
    }

    /// Whether the keys belong to the dialog right now (the same test as at the top of the frame).
    fn window_active(&self, ctx: &Context) -> bool {
        let mine = Modal::layer(self.id());
        let over = self.ui.foreign_focus || foreign_focus(ctx, mine);
        ctx.memory(|m| m.top_modal_layer()) == Some(mine) && !egui::Popup::is_any_open(ctx) && !over
    }
}

/// Whether the focused widget lives on another layer at `Foreground` or above (an overlay of the app's).
///
/// Reads the widget's layer from the pass state; `Context::read_response` would count as a pointer interaction and take the focus away.
fn foreign_focus(ctx: &Context, mine: LayerId) -> bool {
    let Some(id) = ctx.memory(|m| m.focused()) else { return false };
    let layer = ctx.viewport(|v| v.this_pass.widgets.get(id).or_else(|| v.prev_pass.widgets.get(id)).map(|w| w.layer_id));
    layer.is_some_and(|l| l != mine && l.order >= Order::Foreground)
}

/// A modified time as the clock writes it (local time, no weekday).
fn when(look: &Look, secs: i64) -> String {
    match chrono::DateTime::from_timestamp(secs, 0) {
        Some(dt) => {
            let local = dt.with_timezone(&chrono::Local).naive_local();
            let clock = ito_common::Clock { weekday: false, ..look.clock.clone() };
            clock.stamp(&local, look.lang)
        }
        None => String::new(),
    }
}

/// The modal's area and layer, from the picker's id.
struct Modal;

impl Modal {
    fn area(id: &str) -> egui::Area {
        egui::Modal::default_area(Id::new(("ito-filepick", id.to_owned())))
    }
    fn layer(id: &str) -> LayerId {
        Self::area(id).layer()
    }
}

#[allow(dead_code)]
fn _unused(_: Pos2) {}
