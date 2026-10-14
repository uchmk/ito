//! The Notifications page every app has (`docs/common-spec.md`): how to
//! tell when one of the app's events happens (a system notification, a
//! sound, a webhook), which sound, Windows' focus mode, and the webhook's
//! address and format. The events -- the table's columns -- are the app's.

use egui::{Color32, FontId, RichText};
use ito_notify::{is_http, FORMATS, SOUNDS};

use crate::{button, row, section, select, sep, status, switch, Drafts, Look, Words};

/// One of the app's events, a column of the table: its name and colour, and
/// the label of the row that picks its sound.
#[derive(Clone, Copy)]
pub struct End<'a> {
    pub name: &'a str,
    pub color: Color32,
    pub sound: &'a str,
}

/// What the page shows and changes. The lists are one per [`End`], in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notify {
    pub system: Vec<bool>,
    pub sound: Vec<bool>,
    /// Empty when the webhook is told of every event: no row for it.
    pub webhook: Vec<bool>,
    /// The sound's name, one of [`SOUNDS`].
    pub sounds: Vec<String>,
    /// No sound in Windows' focus mode.
    pub focus: bool,
    pub webhook_url: String,
    /// One of [`FORMATS`].
    pub webhook_format: String,
}

impl Notify {
    /// Back to `defaults`, but for the webhook's address and format: they
    /// are not a choice but a place.
    pub fn reset(&mut self, defaults: &Notify) {
        *self = Notify {
            webhook_url: std::mem::take(&mut self.webhook_url),
            webhook_format: std::mem::take(&mut self.webhook_format),
            ..defaults.clone()
        };
    }
}

/// A sound's name in the words of `w`.
pub fn sound_name<'a>(w: &'a Words, sound: &str) -> &'a str {
    SOUNDS.iter().position(|s| *s == sound).map_or(w.sound_names[3], |k| w.sound_names[k])
}

/// A webhook's format as it is written.
pub fn format_name(format: &str) -> &'static str {
    match format {
        "slack" => "Slack",
        "json" => "JSON",
        _ => "ntfy",
    }
}

/// The page's words, for the settings' search.
pub fn notify_index(w: &Words) -> Vec<&'static str> {
    vec![
        w.notify_test,
        w.notify_reset,
        w.notify_system,
        w.notify_sound,
        w.notify_webhook,
        w.notify_sounds,
        w.focus,
        w.focus_note,
        w.webhook_url,
        w.webhook_url_note,
        w.webhook_format,
    ]
}

/// The Notifications page: "Send a test" and "Reset to defaults" (back to
/// `defaults`, keeping the webhook's address and format) at the top, then
/// the cards. Changes `n` in place; true when "Send a test" was pressed.
///
/// With one event the ways are rows with a switch each; with more, a table
/// of a column per event. The webhook's row is left out when `n.webhook` is
/// empty.
pub fn notify_page(ui: &mut egui::Ui, l: Look, w: &Words, ends: &[End], n: &mut Notify, defaults: &Notify, drafts: &mut Drafts) -> bool {
    let c = l.c;
    let mut test = false;
    ui.horizontal(|ui| {
        test = button(ui, l, w.notify_test);
        if button(ui, l, w.notify_reset) {
            n.reset(defaults);
        }
    });
    ui.add_space(8.0);
    section(ui, l, w.notify_when, |ui| {
        let mut ways = vec![(w.notify_system, w.notify_system_note, &mut n.system), (w.notify_sound, w.notify_sound_note, &mut n.sound)];
        if !n.webhook.is_empty() {
            ways.push((w.notify_webhook, w.notify_webhook_note, &mut n.webhook));
        }
        if ends.len() == 1 {
            for (k, (label, note, list)) in ways.into_iter().enumerate() {
                if k > 0 {
                    sep(ui, l);
                }
                if let Some(on) = list.first_mut() {
                    if row(ui, l, label, note, |ui| switch(ui, l, *on)) {
                        *on = !*on;
                    }
                }
            }
        } else {
            table(ui, l, ends, ways);
        }
    });
    section(ui, l, w.notify_sounds, |ui| {
        let names: Vec<(&str, &str)> = SOUNDS.iter().map(|s| (*s, sound_name(w, s))).collect();
        for (k, (end, now)) in ends.iter().zip(n.sounds.iter_mut()).enumerate() {
            if k > 0 {
                sep(ui, l);
            }
            let picked = row(ui, l, end.sound, "", |ui| {
                if ui.small_button("▶").on_hover_text(w.sound_play).clicked() {
                    ito_notify::sound::play(now);
                }
                select(ui, &format!("notify-sound-{k}"), now.as_str(), &names).map(str::to_owned)
            });
            if let Some(s) = picked {
                *now = s;
            }
        }
        sep(ui, l);
        if row(ui, l, w.focus, w.focus_note, |ui| switch(ui, l, n.focus)) {
            n.focus = !n.focus;
        }
    });
    section(ui, l, w.webhook, |ui| {
        let typed = row(ui, l, w.webhook_url, w.webhook_url_note, |ui| {
            crate::field(ui, drafts, "webhook", &n.webhook_url, "https://ntfy.sh/my-topic", 260.0)
        });
        if let Some(t) = typed {
            n.webhook_url = t.trim().to_owned();
        }
        if !n.webhook_url.is_empty() && !is_http(&n.webhook_url) {
            status(ui, w.webhook_bad, c.err);
            ui.add_space(4.0);
        }
        sep(ui, l);
        let names: Vec<(&str, &str)> = FORMATS.iter().map(|f| (*f, format_name(f))).collect();
        let picked =
            row(ui, l, w.webhook_format, w.webhook_format_note, |ui| select(ui, "webhook-format", n.webhook_format.as_str(), &names));
        if let Some(f) = picked {
            n.webhook_format = f.to_owned();
        }
    });
    test
}

/// The ways to tell as a table: the way on the left, then a column of
/// switches per event under its name and dot.
fn table(ui: &mut egui::Ui, l: Look, ends: &[End], ways: Vec<(&str, &str, &mut Vec<bool>)>) {
    let c = l.c;
    let gap = 16.0;
    let width = ui.available_width();
    let n = ends.len() as f32;
    // A column per event, the label's 1.6 times as wide; not wider than it
    // needs to be when there are few.
    let unit = ((width - n * gap) / (1.6 + n)).min(140.0);
    let columns = |row: egui::Rect| -> Vec<egui::Rect> {
        let label = width - n * (unit + gap);
        let mut x = row.left();
        let mut out = Vec::new();
        for w in std::iter::once(label).chain(ends.iter().map(|_| unit)) {
            out.push(egui::Rect::from_min_max(egui::pos2(x, row.top()), egui::pos2(x + w, row.bottom())));
            x += w + gap;
        }
        out
    };
    let (head, _) = ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::hover());
    for (end, r) in ends.iter().zip(&columns(head)[1..]) {
        let text = ui.fonts_mut(|f| f.layout_no_wrap(end.name.to_owned(), FontId::proportional(13.0), end.color));
        let left = r.center().x - (text.size().x + 14.0) / 2.0;
        ui.painter().circle_filled(egui::pos2(left + 4.0, r.center().y), 4.0, end.color);
        ui.painter().galley(egui::pos2(left + 14.0, r.center().y - text.size().y / 2.0), text, end.color);
    }
    for (label, note, list) in ways {
        sep(ui, l);
        let (row, _) = ui.allocate_exact_size(egui::vec2(width, 66.0), egui::Sense::hover());
        let cols = columns(row);
        let hit = !l.q.is_empty() && (label.to_lowercase().contains(l.q) || note.to_lowercase().contains(l.q));
        if hit {
            ui.painter().rect_filled(row.expand2(egui::vec2(10.0, 0.0)), 6.0, c.wait.gamma_multiply(0.14));
        }
        let words = egui::Rect::from_center_size(cols[0].center(), egui::vec2(cols[0].width(), 40.0));
        // Children of their own: a scope would move the card's cursor to
        // their bottom and take back the row's room.
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(words));
        inner.spacing_mut().item_spacing.y = 2.0;
        inner.label(RichText::new(label).size(13.0).color(c.strong()));
        inner.add(egui::Label::new(RichText::new(note).size(12.0).color(c.dim)).truncate());
        for (on, r) in list.iter_mut().zip(&cols[1..]) {
            let at = egui::Rect::from_center_size(r.center(), egui::vec2(40.0, 22.0));
            if switch(&mut ui.new_child(egui::UiBuilder::new().max_rect(at)), l, *on) {
                *on = !*on;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{show, Keys, Nav, State, EN, JA};

    fn notify(ends: usize, webhook: bool) -> Notify {
        Notify {
            system: vec![true; ends],
            sound: vec![false; ends],
            webhook: if webhook { vec![true; ends] } else { Vec::new() },
            sounds: vec!["chime".to_owned(); ends],
            focus: true,
            webhook_url: String::new(),
            webhook_format: "ntfy".to_owned(),
        }
    }

    /// One frame of the page: the texts drawn and where, and whether
    /// "Send a test" was pressed.
    fn page(ctx: &egui::Context, ends: &[End], n: &mut Notify, events: Vec<egui::Event>) -> (Vec<(String, egui::Rect)>, bool) {
        let defaults = notify(ends.len(), !n.webhook.is_empty());
        let nav = Nav { pages: &[("Notifications", "")], index: &[], words: &EN, file: "config.toml" };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 1600.0))),
            events,
            ..Default::default()
        };
        let mut state = State::default();
        let mut drafts = Drafts::new();
        let mut test = false;
        let mut out = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show(ui, ito_theme::colors(), &mut state, &nav, Keys::default(), |ui, _, l| {
                    test = notify_page(ui, l, &EN, ends, n, &defaults, &mut drafts);
                });
            });
        });
        out.textures_delta.clear();
        fn walk(s: &egui::Shape, v: &mut Vec<(String, egui::Rect)>) {
            match s {
                egui::Shape::Text(t) => v.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2()))),
                egui::Shape::Vec(list) => list.iter().for_each(|s| walk(s, v)),
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for c in &out.shapes {
            walk(&c.shape, &mut texts);
        }
        (texts, test)
    }

    fn at(texts: &[(String, egui::Rect)], words: &str) -> Option<egui::Rect> {
        texts.iter().find(|(t, _)| t == words).map(|(_, r)| *r)
    }

    fn click(p: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![egui::Event::PointerMoved(p), button(true), button(false)]
    }

    const THREE: [End; 3] = [
        End { name: "Done", color: Color32::GREEN, sound: "When done" },
        End { name: "Failed", color: Color32::RED, sound: "When failed" },
        End { name: "Cancelled", color: Color32::GRAY, sound: "When cancelled" },
    ];

    #[test]
    fn many_events_make_a_table_and_a_switch_flips_its_cell() {
        let ctx = egui::Context::default();
        let mut n = notify(3, true);
        let (texts, _) = page(&ctx, &THREE, &mut n, Vec::new());
        for words in ["Done", "Failed", "Cancelled", "System notification", "Sound", "Webhook", "When failed", "WEBHOOK", "Format"] {
            assert!(at(&texts, words).is_some(), "{words} is on the page");
        }
        // The switch under Failed, in the Sound row.
        let x = at(&texts, "Failed").unwrap().center().x - 7.0;
        let y = at(&texts, "Sound").unwrap().bottom() + 2.0;
        let (_, _) = page(&ctx, &THREE, &mut n, click(egui::pos2(x, y)));
        assert_eq!(n.sound, [false, true, false]);
        assert_eq!(n.system, [true; 3], "the row above is as it was");
    }

    #[test]
    fn one_event_makes_rows_and_no_webhook_row_when_it_gets_them_all() {
        let ctx = egui::Context::default();
        let one = [End { name: "Alert", color: Color32::RED, sound: "Alert sound" }];
        let mut n = notify(1, false);
        let (texts, _) = page(&ctx, &one, &mut n, Vec::new());
        assert!(at(&texts, "System notification").is_some());
        assert!(at(&texts, "Alert sound").is_some());
        assert!(at(&texts, "Alert").is_none(), "no table's heading");
        assert!(at(&texts, "Webhook").is_none(), "no row for the webhook");
    }

    #[test]
    fn the_test_button_is_handed_back_and_reset_keeps_the_address() {
        let ctx = egui::Context::default();
        let mut n = notify(3, true);
        n.webhook_url = "ntfy.sh/x".to_owned();
        n.webhook_format = "json".to_owned();
        n.focus = false;
        let (texts, _) = page(&ctx, &THREE, &mut n, Vec::new());
        assert!(at(&texts, EN.webhook_bad).is_some(), "an address without http(s) is pointed out");
        let (_, test) = page(&ctx, &THREE, &mut n, click(at(&texts, "Send a test").unwrap().center()));
        assert!(test);
        page(&ctx, &THREE, &mut n, click(at(&texts, "Reset to defaults").unwrap().center()));
        assert!(n.focus, "back to the defaults");
        assert_eq!((n.webhook_url.as_str(), n.webhook_format.as_str()), ("ntfy.sh/x", "json"), "but the place stays");
    }

    #[test]
    fn every_sound_and_format_has_a_name() {
        for w in [&EN, &JA] {
            let names: Vec<&str> = SOUNDS.iter().map(|s| sound_name(w, s)).collect();
            assert_eq!(names.len(), 4);
            assert!(names.iter().all(|n| !n.is_empty()));
        }
        assert_eq!(sound_name(&JA, "low"), "低い音");
        assert_eq!(sound_name(&EN, "nope"), "System default");
        assert_eq!(FORMATS.map(format_name), ["ntfy", "Slack", "JSON"]);
        assert!(notify_index(&JA).contains(&"Windows の集中モードに従う"));
    }
}
