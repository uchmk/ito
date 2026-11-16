//! The picture of the keyboard on the Keys page (`keys_page_kb`): one layer of
//! the keymap laid out on the keys, to look at and to pick from. It only
//! shows and selects; changing a key stays in the table below it.
//!
//! What each key shows comes from `ito-keyboard` (`view`, `split`); this file
//! draws it and turns clicks into the page's `by_key` and `kb_latch`. The
//! keys the person presses are still read in one place, the page's capture.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, CornerRadius, FontId, Galley, Pos2, Rect, RichText, Sense, Stroke, StrokeKind};
use ito_a11y::Role;
use ito_common::{keyboard_change, CommonChange, Keyboard};
use ito_keyboard::{
    caps, detected, legend, modifier, place, produce, split, view, Block, Board, Cap, Family, KeyView, Kind, Modifier, Mods, Phys, View,
};
use ito_keymap::Row;
use ito_keys::{human_seq_for, render_seq, Key};
use ito_theme::scale::{radius, space, Text};

use crate::keys::{fill, KeyTable, KeyWords, KeysEdit};
use crate::{select, z, Look, Tone};

/// How the keyboard is laid out for the width there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The main block and the nav block side by side.
    Wide,
    /// The main block, the nav and arrow keys in a strip under it.
    Middle,
    /// As `Middle`, with small keys that show only their legend and marks.
    Narrow,
}

/// The largest key, so a very wide page does not get a huge keyboard.
const MAX_UNIT: f32 = 80.0;

fn sizing(width: f32) -> (Mode, f32) {
    if width / 18.5 >= 52.0 {
        (Mode::Wide, (width / 18.5).min(MAX_UNIT))
    } else if width / 15.0 >= 44.0 {
        (Mode::Middle, (width / 15.0).min(MAX_UNIT))
    } else {
        (Mode::Narrow, (width / 15.0).max(22.0))
    }
}

/// The keys of a modifier as the page's `Mods` tells them.
fn lit(active: Mods, m: Modifier, sup: bool) -> bool {
    match m {
        Modifier::Shift => active.shift,
        Modifier::Ctrl => active.ctrl,
        Modifier::Alt => active.alt,
        Modifier::Cmd if sup => active.sup,
        // Where the app does not tell Cmd from Ctrl, command is control.
        Modifier::Cmd => active.ctrl,
    }
}

fn set_mod(active: &mut Mods, m: Modifier, sup: bool, on: bool) {
    match m {
        Modifier::Shift => active.shift = on,
        Modifier::Ctrl => active.ctrl = on,
        Modifier::Alt => active.alt = on,
        Modifier::Cmd if sup => active.sup = on,
        Modifier::Cmd => active.ctrl = on,
    }
}

fn mod_name(m: Modifier) -> &'static str {
    match m {
        Modifier::Shift => "shift",
        Modifier::Ctrl => "ctrl",
        Modifier::Alt => "alt",
        Modifier::Cmd => "cmd",
    }
}

fn has_cjk(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x3000..=0x9fff | 0xff00..=0xffef))
}

/// `text` in at most `rows` rows of `width`, cut with an ellipsis.
fn text_galley(ui: &egui::Ui, text: &str, font: FontId, color: Color32, width: f32, rows: usize, underline: bool) -> Arc<Galley> {
    let format = TextFormat {
        font_id: font,
        color,
        underline: if underline { Stroke::new(1.0, color) } else { Stroke::NONE },
        ..Default::default()
    };
    let mut job = LayoutJob::single_section(text.to_owned(), format);
    job.wrap = TextWrapping {
        max_width: width.max(1.0),
        max_rows: rows.max(1),
        break_anywhere: has_cjk(text),
        overflow_character: Some('\u{2026}'),
    };
    ui.painter().layout_job(job)
}

/// Which keyboard: the one said, else what the machine has (asked once, off
/// this thread). The second is the family it found, `None` while it asks.
pub(crate) fn board_for(ctx: &egui::Context, setting: Keyboard) -> (Board, Option<Family>) {
    let guess = match setting {
        Keyboard::Us => Some(Family::Us),
        Keyboard::Jis => Some(Family::Jis),
        Keyboard::Auto => {
            let ctx = ctx.clone();
            detected(move || ctx.request_repaint())
        }
    };
    (Board::here(guess.unwrap_or(Family::Us)), guess)
}

/// What one frame of drawing needs to know.
struct Draw<'a> {
    c: ito_theme::Colors,
    w: &'a KeyWords,
    board: Board,
    v: &'a View,
    sel_phys: Vec<Phys>,
    sel: Option<Key>,
    prefix: &'a [Key],
    active: Mods,
    sup: bool,
    mode: Mode,
    u: f32,
    uy: f32,
    gap: f32,
    small: f32,
    tr: &'a dyn Fn(&str) -> String,
    layer_name: String,
}

/// What a frame of drawing asks for.
#[derive(Default)]
struct Asked {
    /// The page's `by_key`, from a click.
    by_key: Option<Vec<Key>>,
    /// The modifiers held for the person.
    latch: Mods,
}

fn kv_text(d: &Draw, kv: &KeyView) -> String {
    if kv.on.is_empty() {
        String::new()
    } else if kv.desc.is_empty() {
        kv.run.clone()
    } else {
        (d.tr)(&kv.desc)
    }
}

/// The picture, its header and the lists under it. Hands back the layout
/// change when another one is picked.
#[allow(clippy::too_many_arguments)]
pub(crate) fn picture<A>(
    ui: &mut egui::Ui,
    l: Look,
    w: &KeyWords,
    t: &KeyTable<A>,
    st: &mut KeysEdit,
    all: &[Row],
    layers: &[&str],
    setting: Keyboard,
) -> Option<CommonChange> {
    let c = l.c;
    let mut common = None;

    let (board, guess) = board_for(ui.ctx(), setting);
    let family_word = |f: Family| if f == Family::Jis { w.kb_jis } else { w.kb_us };

    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(w.kb_keyboard).size(z(ui, 12.5)).color(c.dim));
        let auto = match guess {
            Some(f) if setting == Keyboard::Auto => fill(w.kb_auto, &[family_word(f)]),
            _ => w.kb_auto_wait.to_owned(),
        };
        let options = [(Keyboard::Auto, auto.as_str()), (Keyboard::Us, w.kb_us), (Keyboard::Jis, w.kb_jis)];
        if let Some(k) = select(ui, "keys-keyboard", setting, &options) {
            common = Some(keyboard_change(k));
        }
        ui.add_space(space::SM);
        let r = crate::button_with(ui, l, if st.kb_hidden { w.kb_show } else { w.kb_hide }, Tone::Plain);
        ito_a11y::test_id(&r, "settings.keys.kb.toggle");
        if r.clicked() {
            st.kb_hidden = !st.kb_hidden;
        }
    });
    if st.kb_hidden || layers.is_empty() {
        return common;
    }
    ui.add_space(space::SM);

    // Which layer: the one chosen, else the first.
    let chosen = layers.contains(&st.layer.as_str());
    let layer = if chosen { st.layer.clone() } else { layers[0].to_owned() };
    let layer_name = (t.layer_name)(&layer);
    let bindings = t.now.iter().find(|(n, _)| *n == layer).map_or(&[][..], |(_, b)| b.as_slice());
    let (n, sel) = split(bindings, t.noop, &st.by_key);
    let prefix: Vec<Key> = st.by_key[..n].to_vec();
    let v = view(bindings, t.noop, all, &layer, &prefix, board, t.sup);

    // The modifiers held now, or kept by a click or by the last key pressed.
    let held = ui.input(|i| i.modifiers);
    let mac = board.is_mac();
    let held = if t.sup && mac {
        Mods { ctrl: held.ctrl, alt: held.alt, shift: held.shift, sup: held.mac_cmd }
    } else {
        Mods { ctrl: held.ctrl || held.mac_cmd, alt: held.alt, shift: held.shift, sup: false }
    };
    let latch = st.kb_latch;
    let active =
        Mods { ctrl: held.ctrl || latch.ctrl, alt: held.alt || latch.alt, shift: held.shift || latch.shift, sup: held.sup || latch.sup }
            .folded(board, t.sup);

    // Said once when the keys the picture follows change.
    let said_id = egui::Id::new("ito-prefs-kb-prefix");
    let before: Vec<Key> = ui.data(|d| d.get_temp(said_id)).unwrap_or_default();
    if before != prefix {
        ui.data_mut(|d| d.insert_temp(said_id, prefix.clone()));
        if !prefix.is_empty() {
            let line = format!("{}: {}", human_seq_for(&prefix, t.sup), fill(w.kb_follows, &[&v.next.to_string()]));
            ito_a11y::announce(ui, &line);
        }
    }

    let sel_phys: Vec<Phys> = sel.map(|k| place(board, &k, t.sup).into_iter().map(|(p, _)| p).collect()).unwrap_or_default();
    let mut asked = Asked { by_key: None, latch };

    let fr = egui::Frame::NONE
        .fill(c.panel)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(radius::BOX)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let small = Text::ChromeSmall.at(ui.ctx());

            // The keys followed so far, and which layer this is.
            if !prefix.is_empty() || !chosen {
                ui.horizontal_wrapped(|ui| {
                    if !prefix.is_empty() {
                        let human = human_seq_for(&prefix, t.sup);
                        let b =
                            egui::Button::new(RichText::new(format!("\u{2039} {human}")).font(FontId::monospace(small)).color(c.strong()))
                                .fill(c.side)
                                .stroke(Stroke::new(1.0, c.border_strong()))
                                .corner_radius(radius::CONTROL);
                        let r = ui.add(b);
                        ito_a11y::say(&r, Role::Button, &format!("{}: {human}", w.kb_back));
                        ito_a11y::test_id(&r, "settings.keys.kb.prefix");
                        if r.clicked() {
                            let mut up = prefix.clone();
                            up.pop();
                            asked.by_key = Some(up);
                        }
                    }
                    if !chosen && layers.len() > 1 {
                        let line = fill(w.kb_showing, &[&layer_name, w.layer]);
                        ui.label(RichText::new(line).size(small).color(c.dim));
                    }
                });
                ui.add_space(space::XS);
            }

            let (mode, u) = sizing(ui.available_width());
            let uy = if mode == Mode::Narrow { 0.9 * u } else { u };
            let d = Draw {
                c,
                w,
                board,
                v: &v,
                sel_phys: sel_phys.clone(),
                sel,
                prefix: &prefix,
                active,
                sup: t.sup,
                mode,
                u,
                uy,
                gap: (u * 0.08).clamp(2.0, space::XS),
                small,
                tr: t.tr,
                layer_name: layer_name.clone(),
            };
            if mode == Mode::Narrow && 15.0 * u > ui.available_width() {
                egui::ScrollArea::horizontal().id_salt("settings.keys.kb.scroll").show(ui, |ui| draw_board(ui, &d, &mut asked));
            } else {
                draw_board(ui, &d, &mut asked);
            }

            // The key picked: every way of pressing it with what it is on.
            if let (Some(_), Some(&p)) = (sel, sel_phys.first()) {
                let lines: Vec<&KeyView> = v.keys.iter().filter(|k| k.phys == p && !k.on.is_empty()).collect();
                if !lines.is_empty() {
                    ui.add_space(space::SM);
                    let r = ui
                        .scope(|ui| {
                            ui.label(RichText::new(w.kb_detail).size(small).color(c.dim));
                            for (i, kv) in lines.iter().enumerate() {
                                let text = format!("{}   {}", human_seq_for(&kv.on, t.sup), kv_text(&d, kv));
                                let r = ui.add(egui::Label::new(RichText::new(text).size(small).color(c.fg)).sense(Sense::CLICK));
                                ito_a11y::test_id(&r, &format!("settings.keys.kb.detail.{i}"));
                                if r.clicked() {
                                    asked.by_key = Some(kv.on.clone());
                                }
                            }
                        })
                        .response;
                    ito_a11y::say(&r, Role::Label, w.kb_detail);
                    ito_a11y::test_id(&r, "settings.keys.kb.detail");
                }
            }

            // Bindings with a key this keyboard has no place for.
            if !v.unplaced.is_empty() {
                ui.add_space(space::SM);
                let head = fill(w.kb_unplaced, &[&v.unplaced.len().to_string()]);
                let r = ui
                    .scope(|ui| {
                        ui.label(RichText::new(&head).size(small).color(c.dim));
                        let items: Vec<String> = v
                            .unplaced
                            .iter()
                            .map(|(on, text)| format!("{}  {}", human_seq_for(on, t.sup), if text.is_empty() { "" } else { text.as_str() }))
                            .collect();
                        ui.add(egui::Label::new(RichText::new(items.join("   \u{b7}   ")).size(small).color(c.fg)));
                    })
                    .response;
                ito_a11y::say(&r, Role::Label, &head);
                ito_a11y::test_id(&r, "settings.keys.kb.unplaced");
            }

            // What the marks mean.
            ui.add_space(space::SM);
            ui.horizontal_wrapped(|ui| {
                let note = |ui: &mut egui::Ui, mark: &str, color: Color32, words: &str| {
                    ui.label(RichText::new(mark).size(small).color(color));
                    ui.label(RichText::new(words).size(small).color(c.dim));
                    ui.add_space(space::SM);
                };
                note(ui, "\u{25c6}", c.magenta, w.kb_common);
                note(ui, "\u{25cf}", c.done, &fill(w.kb_layer_only, &[&layer_name]));
                note(ui, "\u{25b8}N", c.dim, &fill(w.kb_follows, &["N"]));
                note(ui, "\u{2022}", c.dim, w.kb_changed);
                note(ui, "\u{26a0}", c.err, w.kb_unreachable);
            });
        });
    ito_a11y::say(&fr.response, Role::Label, w.kb_keyboard);
    ito_a11y::test_id(&fr.response, "settings.keys.kb");

    if let Some(next) = asked.by_key {
        st.by_key = next;
    }
    st.kb_latch = asked.latch;
    common
}

/// Where each key goes, then the keys.
fn draw_board(ui: &mut egui::Ui, d: &Draw, asked: &mut Asked) {
    let all = caps(d.board);
    let main_h = all.iter().filter(|c| matches!(c.block, Block::Fn | Block::Main)).map(|c| c.y + c.h).fold(0.0, f32::max);
    let (width_u, height_u) = if d.mode == Mode::Wide {
        (all.iter().map(|c| c.x + c.w).fold(0.0, f32::max), all.iter().map(|c| c.y + c.h).fold(0.0, f32::max))
    } else {
        (15.0, main_h + 0.35 + 1.0)
    };
    let (area, _) = ui.allocate_exact_size(egui::vec2(width_u * d.u, height_u * d.uy), Sense::hover());
    let mut strip = 0.0;
    let mut first_mod: Vec<Modifier> = Vec::new();
    for cap in all {
        let in_strip = d.mode != Mode::Wide && matches!(cap.block, Block::Nav | Block::Arrows);
        let (x, y, w, h) = if in_strip {
            strip += 1.0;
            (strip - 1.0, main_h + 0.35, 1.0, 1.0)
        } else {
            (cap.x, cap.y, cap.w, cap.h)
        };
        let rect = Rect::from_min_size(area.min + egui::vec2(x * d.u, y * d.uy), egui::vec2(w * d.u, h * d.uy));
        let notch = cap
            .notch_rect()
            .filter(|_| !in_strip)
            .map(|(nx, ny, nw, nh)| Rect::from_min_size(area.min + egui::vec2(nx * d.u, ny * d.uy), egui::vec2(nw * d.u, nh * d.uy)));
        // A modifier gets its test ID once (left, first); its twin the key's.
        let md = modifier(d.board, cap.phys);
        let mod_id = md.filter(|m| !first_mod.contains(m));
        if let Some(m) = mod_id {
            first_mod.push(m);
        }
        draw_key(ui, d, cap, rect, notch, mod_id, asked);
    }
}

#[allow(clippy::too_many_lines)]
fn draw_key(ui: &mut egui::Ui, d: &Draw, cap: &Cap, rect: Rect, notch: Option<Rect>, test_mod: Option<Modifier>, asked: &mut Asked) {
    let c = d.c;
    let md = modifier(d.board, cap.phys);
    let kv = if cap.inert || md.is_some() { None } else { d.v.at(cap.phys, d.active) };
    let sense = if cap.inert { Sense::hover() } else { Sense::CLICK };
    let id = ui.id().with(("ito-kb-key", cap.phys));
    let main = ui.interact(rect, id, sense);
    let either = match notch {
        Some(n) => main.clone().union(ui.interact(n, id.with("notch"), sense)),
        None => main.clone(),
    };
    let is_lit = md.is_some_and(|m| lit(d.active, m, d.sup));
    let selected = d.sel_phys.contains(&cap.phys);

    // The shape.
    let half = d.gap / 2.0;
    let (fill_color, mut stroke) = if cap.inert {
        (c.side, Stroke::NONE)
    } else if is_lit {
        (c.chosen(), Stroke::new(1.0, c.border_strong()))
    } else if kv.is_some() {
        (c.raised(), Stroke::NONE)
    } else {
        (c.panel, Stroke::new(1.0, c.border))
    };
    if either.hovered() && !cap.inert {
        stroke = Stroke::new(1.0, c.border_strong());
    }
    if selected {
        stroke = Stroke::new(2.0, c.accent());
    }
    let body = rect.shrink(half);
    if let Some(n) = notch {
        // The inverted L: two rectangles under one fill, one outline round both.
        let top = n.top() + half;
        let join = n.bottom() - half;
        let pts = vec![
            Pos2::new(n.left() + half, top),
            Pos2::new(rect.right() - half, top),
            Pos2::new(rect.right() - half, rect.bottom() - half),
            Pos2::new(rect.left() + half, rect.bottom() - half),
            Pos2::new(rect.left() + half, join),
            Pos2::new(n.left() + half, join),
        ];
        let p = ui.painter();
        p.rect_filled(Rect::from_min_max(pts[0], Pos2::new(pts[1].x, join)), 0.0, fill_color);
        p.rect_filled(Rect::from_min_max(Pos2::new(pts[4].x, top), pts[2]), 0.0, fill_color);
        if stroke.width > 0.0 {
            p.add(egui::Shape::closed_line(pts, stroke));
        }
    } else {
        ui.painter().rect(body, CornerRadius::same(radius::CONTROL), fill_color, stroke, StrokeKind::Inside);
    }

    // What is on it.
    let pad = (d.u * 0.08).max(3.0);
    let inner = body.shrink(pad);
    let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
    let (plain, shifted) = legend(d.board, cap.phys);
    let word = if d.active.shift { shifted.unwrap_or(plain) } else { plain };
    let (legend_color, underline) = if is_lit {
        (c.strong(), true)
    } else if cap.inert {
        (c.faint(), false)
    } else {
        (c.dim, false)
    };
    let small = if d.mode == Mode::Narrow { d.small * 0.9 } else { d.small };
    let mark_w = if kv.is_some_and(|k| !k.on.is_empty()) { small * 1.3 } else { 0.0 };
    let lg = text_galley(ui, word, FontId::monospace(small), legend_color, inner.width() - mark_w, 1, underline);
    painter.galley(inner.min, lg.clone(), legend_color);

    // How many keys the modifier would show.
    if let Some(m) = md {
        if !lit(d.active, m, d.sup) {
            let mut next = d.active;
            set_mod(&mut next, m, d.sup, true);
            let n = d.v.per_mods.iter().find(|(x, _)| *x == next).map_or(0, |(_, n)| *n);
            if n > 0 {
                let g = text_galley(ui, &n.to_string(), FontId::monospace(small), c.dim, inner.width(), 1, false);
                painter.galley(Pos2::new(inner.right() - g.size().x, inner.top()), g, c.dim);
            }
        }
    }

    let mut shown = lg.clone();
    if let Some(kv) = kv {
        if !kv.on.is_empty() {
            let kind_color = if kv.kind == Kind::Common { c.magenta } else { c.done };
            let glyph = if kv.kind == Kind::Common { "\u{25c6}" } else { "\u{25cf}" };
            let g = text_galley(ui, glyph, FontId::monospace(small), kind_color, inner.width(), 1, false);
            painter.galley(Pos2::new(inner.right() - g.size().x, inner.top()), g, kind_color);
            let bar = Rect::from_min_size(body.min, egui::vec2(3.0, body.height()));
            painter.rect_filled(bar, CornerRadius { nw: radius::CONTROL, sw: radius::CONTROL, ne: 0, se: 0 }, kind_color);
        }

        // The marks at the foot: keys that follow, a changed key, one that never runs.
        let mut marks = LayoutJob::default();
        let mark = |job: &mut LayoutJob, text: String, color: Color32| {
            job.append(&text, 0.0, TextFormat { font_id: FontId::monospace(small), color, ..Default::default() });
        };
        if kv.follows > 0 {
            mark(&mut marks, format!("\u{25b8}{} ", kv.follows), c.dim);
        }
        if kv.changed {
            mark(&mut marks, "\u{2022} ".to_owned(), c.dim);
        }
        if kv.also.iter().any(|a| a.unreachable) {
            mark(&mut marks, "\u{26a0}".to_owned(), c.err);
        }
        let marks_h = if marks.sections.is_empty() {
            0.0
        } else {
            let g = ui.painter().layout_job(marks);
            let at = Pos2::new(inner.right() - g.size().x, inner.bottom() - g.size().y);
            let h = g.size().y;
            painter.galley(at, g, c.dim);
            h
        };

        // The words: not on the smallest keys.
        if d.mode != Mode::Narrow {
            let text = kv_text(d, kv);
            if !text.is_empty() {
                let top = inner.top() + lg.size().y + 1.0;
                let room = inner.bottom() - marks_h - top;
                let rows = if d.u > 70.0 { 3 } else { 2 };
                let font = FontId::proportional(small);
                let mut g = text_galley(ui, &text, font.clone(), c.fg, inner.width(), rows, false);
                let row_h = g.rows.first().map_or(small, |r| r.height());
                let fit = (room / row_h).floor().max(0.0) as usize;
                if fit < g.rows.len() && fit >= 1 {
                    g = text_galley(ui, &text, font, c.fg, inner.width(), fit, false);
                }
                if fit >= 1 {
                    painter.galley(Pos2::new(inner.left(), top), g.clone(), c.fg);
                    shown = g;
                }
            }
        }
    }

    // The words a screen reader reads.
    let label = match kv {
        Some(k) if !k.on.is_empty() => format!("{word}: {}", kv_text(d, k)),
        _ => word.to_owned(),
    };
    ito_a11y::say(&main, if cap.inert { Role::Label } else { Role::Button }, &label);
    let name = match test_mod {
        Some(m) => format!("settings.keys.kb.mod.{}", mod_name(m)),
        None => format!("settings.keys.kb.key.{}", cap.phys.id()),
    };
    ito_a11y::test_id_in(ui, &main, &name, Some(&shown));

    // What a click does.
    if either.clicked() && !cap.inert {
        if let Some(m) = md {
            let on = lit(asked.latch, m, d.sup);
            set_mod(&mut asked.latch, m, d.sup, !on);
        } else if let Some(k) = produce(d.board, cap.phys, d.active, d.sup) {
            if d.sel == Some(k) {
                asked.by_key = Some(d.prefix.to_vec());
            } else {
                let mut next = d.prefix.to_vec();
                next.push(k);
                asked.by_key = Some(next);
            }
        }
    }

    // What the person sees on hover.
    if let Some(kv) = kv {
        either.on_hover_ui(|ui| popup(ui, d, kv));
    }
}

fn popup(ui: &mut egui::Ui, d: &Draw, kv: &KeyView) {
    let c = d.c;
    let w = d.w;
    if !kv.on.is_empty() {
        ui.label(RichText::new(kv_text(d, kv)).strong().color(c.strong()));
        let keys = format!("{}   {}", human_seq_for(&kv.on, d.sup), render_seq(&kv.on));
        ui.label(RichText::new(keys).font(FontId::monospace(d.small)).color(c.dim));
        // "{layer} only" already names the layer; "all apps" does not.
        if kv.kind == Kind::Common {
            ui.label(RichText::new(&d.layer_name).size(d.small).color(c.dim));
            ui.label(RichText::new(w.kb_common).size(d.small).color(c.dim));
        } else {
            ui.label(RichText::new(fill(w.kb_layer_only, &[&d.layer_name])).size(d.small).color(c.dim));
        }
    }
    if kv.follows > 0 {
        ui.label(RichText::new(fill(w.kb_follows, &[&kv.follows.to_string()])).size(d.small).color(c.dim));
    }
    for a in &kv.also {
        let text = if a.desc.is_empty() { a.run.clone() } else { (d.tr)(&a.desc) };
        let mut line = format!("{}   {text}", human_seq_for(&a.on, d.sup));
        if a.unreachable {
            line.push_str(&format!("   \u{26a0} {}", w.kb_unreachable));
        }
        ui.label(RichText::new(line).size(d.small).color(if a.unreachable { c.err } else { c.fg }));
    }
    if kv.changed {
        ui.label(RichText::new(w.kb_changed).size(d.small).color(c.dim));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_picks_the_mode() {
        // A plain settings page is at most 820 wide (Middle); the Keys page is a wide one (`Extras::wide`, up to 1280).
        assert_eq!(sizing(1000.0).0, Mode::Wide);
        assert_eq!(sizing(800.0).0, Mode::Middle);
        assert_eq!(sizing(600.0).0, Mode::Narrow);
        assert_eq!(sizing(5000.0).1, MAX_UNIT);
        assert!(sizing(100.0).1 >= 22.0);
    }
}
