//! The Notifications page every app has (`docs/common-spec.md`): how to
//! tell when one of the app's events happens (a system notification, a
//! sound, each webhook), which sound, Windows' focus mode, and the webhooks'
//! addresses and formats (`[[notify.webhooks]]`, as many as are wanted). The
//! events -- the table's columns -- are the app's.

use egui::{Color32, FontId, RichText};
use ito_common::{quote, quote_list, remove_key, set_tables};
use ito_notify::webhook::Place;
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
    /// The sound's name, one of [`SOUNDS`].
    pub sounds: Vec<String>,
    /// No sound in Windows' focus mode.
    pub focus: bool,
    /// The places a notice is sent, in the settings' order.
    pub hooks: Vec<Hook>,
}

impl Notify {
    /// Back to `defaults`, but for the webhooks: they are not a choice but
    /// places.
    pub fn reset(&mut self, defaults: &Notify) {
        *self = Notify { hooks: std::mem::take(&mut self.hooks), ..defaults.clone() };
    }
}

/// One webhook as the page shows it: its address, its format and the events
/// it is told of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hook {
    pub url: String,
    /// One of [`FORMATS`].
    pub format: String,
    /// One per [`End`]. Empty when it is told of every event: no row for it.
    pub on: Vec<bool>,
}

impl Hook {
    /// A hook just added: ntfy, told of every event (a row for it when there
    /// are `ends` of more than one).
    pub fn new(ends: usize) -> Hook {
        Hook { url: String::new(), format: "ntfy".to_owned(), on: if ends > 1 { vec![true; ends] } else { Vec::new() } }
    }

    /// The place it is, `events` being the app's words for its [`End`]s in
    /// order (what `states` holds).
    pub fn place(&self, events: &[&str]) -> Place {
        let states = events
            .iter()
            .enumerate()
            .filter(|(k, _)| self.on.is_empty() || self.on.get(*k).copied().unwrap_or(false))
            .map(|(_, e)| (*e).to_owned())
            .collect();
        Place { url: self.url.clone(), format: self.format.clone(), states }
    }

    /// The hook of the place `p`: with a switch per event when `rows`, else
    /// told of every event.
    pub fn of(p: &Place, events: &[&str], rows: bool) -> Hook {
        let on = if rows { events.iter().map(|e| p.states.iter().any(|s| s == e)).collect() } else { Vec::new() };
        Hook { url: p.url.clone(), format: p.format.clone(), on }
    }
}

/// What a "Send a test" asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Test {
    /// The button at the top: every way, every webhook.
    All,
    /// A webhook's own button: only that one, by its place in the list.
    Hook(usize),
}

/// The name of the `k`th of `count` webhooks: "Webhook" when there is one,
/// "Webhook 1", "Webhook 2", ... when there are more.
pub fn hook_label(w: &Words, count: usize, k: usize) -> String {
    if count == 1 {
        w.notify_webhook.to_owned()
    } else {
        format!("{} {}", w.notify_webhook, k + 1)
    }
}

/// `text` (the app's settings file) with `places` as its
/// `[[notify.webhooks]]`, each its `url`, `format` and, when `states`, its
/// `states`; the old one address (`[notify] webhook`, `webhook_format`,
/// `webhook_states`) is taken out, being the first of `places` now (see
/// [`ito_notify::webhook::places`]). The tables that stay keep their
/// comments ([`set_tables`], which knows them by `url`).
pub fn write_webhooks(text: &str, places: &[Place], states: bool) -> Result<String, String> {
    let mut t = text.to_owned();
    for key in ["webhook", "webhook_format", "webhook_states"] {
        t = remove_key(&t, Some("notify"), key)?;
    }
    let blocks: Vec<Vec<(&str, String)>> = places
        .iter()
        .map(|p| {
            let mut b = vec![("url", quote(&p.url)), ("format", quote(&p.format))];
            if states {
                b.push(("states", quote_list(&p.states)));
            }
            b
        })
        .collect();
    set_tables(&t, "notify.webhooks", &blocks)
}

/// A sound's name in the words of `w`.
pub fn sound_name<'a>(w: &'a Words, sound: &str) -> &'a str {
    SOUNDS.iter().position(|s| *s == sound).map_or(w.sound_names[3], |k| w.sound_names[k])
}

/// A webhook's format as it is written.
pub fn format_name(format: &str) -> &'static str {
    match format {
        "slack" => "Slack",
        "discord" => "Discord",
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
        w.webhook_add,
        w.webhook_test,
    ]
}

/// The Notifications page: "Send a test" and "Reset to defaults" (back to
/// `defaults`, keeping the webhooks) at the top, then the cards. Changes `n`
/// in place; what a "Send a test" asked for, when one was pressed.
///
/// With one event the ways are rows with a switch each; with more, a table
/// of a column per event. A webhook has a row when its `on` is not empty.
pub fn notify_page(
    ui: &mut egui::Ui,
    l: Look,
    w: &Words,
    ends: &[End],
    n: &mut Notify,
    defaults: &Notify,
    drafts: &mut Drafts,
) -> Option<Test> {
    let mut test = None;
    ui.horizontal(|ui| {
        if button(ui, l, w.notify_test) {
            test = Some(Test::All);
        }
        if button(ui, l, w.notify_reset) {
            n.reset(defaults);
        }
    });
    ui.add_space(8.0);
    section(ui, l, w.notify_when, |ui| {
        let Notify { system, sound, hooks, .. } = n;
        let names: Vec<(String, String)> = hooks
            .iter()
            .enumerate()
            .map(|(k, h)| (hook_label(w, hooks.len(), k), if h.url.is_empty() { w.webhook_empty.to_owned() } else { h.url.clone() }))
            .collect();
        let mut ways = vec![(w.notify_system, w.notify_system_note, system), (w.notify_sound, w.notify_sound_note, sound)];
        for ((label, note), h) in names.iter().zip(hooks.iter_mut()) {
            if !h.on.is_empty() {
                ways.push((label.as_str(), note.as_str(), &mut h.on));
            }
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
        if let Some(k) = webhook_rows(ui, l, w, &mut n.hooks, ends.len(), drafts) {
            test = Some(Test::Hook(k));
        }
    });
    test
}

/// The webhooks' rows, for a card the caller opens (tsumugi puts its own
/// rows after them): for each, its name with "Remove" and "Send a test
/// here", its address and its format; then "Add an address", whose new hook
/// is [`Hook::new`]`(ends)`. Changes `hooks` in place; the one whose test was
/// pressed, by its place in the list.
pub fn webhook_rows(ui: &mut egui::Ui, l: Look, w: &Words, hooks: &mut Vec<Hook>, ends: usize, drafts: &mut Drafts) -> Option<usize> {
    let c = l.c;
    let mut test = None;
    let mut gone = None;
    let count = hooks.len();
    let names: Vec<(&str, &str)> = FORMATS.iter().map(|f| (*f, format_name(f))).collect();
    for (k, h) in hooks.iter_mut().enumerate() {
        if k > 0 {
            sep(ui, l);
        }
        let note = if h.url.is_empty() { w.webhook_empty } else { "" };
        row(ui, l, &hook_label(w, count, k), note, |ui| {
            // Right to left: the first one added is the rightmost.
            if button(ui, l, w.webhook_remove) {
                gone = Some(k);
            }
            if button(ui, l, w.webhook_test) {
                test = Some(k);
            }
        });
        sep(ui, l);
        let typed =
            row(ui, l, w.webhook_url, w.webhook_url_note, |ui| crate::field(ui, drafts, &format!("webhook-{k}"), &h.url, HINT, 260.0));
        if let Some(t) = typed {
            h.url = t.trim().to_owned();
        }
        if !h.url.is_empty() && !is_http(&h.url) {
            status(ui, w.webhook_bad, c.err);
            ui.add_space(4.0);
        }
        sep(ui, l);
        let picked =
            row(ui, l, w.webhook_format, w.webhook_format_note, |ui| select(ui, &format!("webhook-format-{k}"), h.format.as_str(), &names));
        if let Some(f) = picked {
            h.format = f.to_owned();
        }
    }
    let (label, note) = if hooks.is_empty() { (w.notify_webhook, w.webhook_none) } else { (w.webhook_more, "") };
    if !hooks.is_empty() {
        sep(ui, l);
    }
    if row(ui, l, label, note, |ui| button(ui, l, w.webhook_add)) {
        hooks.push(Hook::new(ends));
    }
    if let Some(k) = gone {
        hooks.remove(k);
        // The fields' drafts are by place: the ones after it moved up.
        drafts.retain(|id, _| !id.starts_with("webhook-"));
    }
    test
}

/// What an empty address field shows.
const HINT: &str = "https://ntfy.sh/my-topic";

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

    /// `hooks` webhooks, each with a switch per event when `rows`.
    fn notify(ends: usize, hooks: usize, rows: bool) -> Notify {
        let hook = |k: usize| Hook {
            url: format!("https://ntfy.sh/{k}"),
            format: "ntfy".to_owned(),
            on: if rows { vec![true; ends] } else { Vec::new() },
        };
        Notify {
            system: vec![true; ends],
            sound: vec![false; ends],
            sounds: vec!["chime".to_owned(); ends],
            focus: true,
            hooks: (0..hooks).map(hook).collect(),
        }
    }

    /// One frame of the page: the texts drawn and where, and what a "Send a
    /// test" asked for.
    fn page(ctx: &egui::Context, ends: &[End], n: &mut Notify, events: Vec<egui::Event>) -> (Vec<(String, egui::Rect)>, Option<Test>) {
        let defaults = notify(ends.len(), 0, false);
        let nav = Nav { pages: &[("Notifications", "")], index: &[], words: &EN, file: "config.toml" };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 1600.0))),
            events,
            ..Default::default()
        };
        let mut state = State::default();
        let mut drafts = Drafts::new();
        let mut test = None;
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
        all(texts, words).first().copied()
    }

    /// Everywhere `words` is drawn, top first.
    fn all(texts: &[(String, egui::Rect)], words: &str) -> Vec<egui::Rect> {
        let mut out: Vec<egui::Rect> = texts.iter().filter(|(t, _)| t == words).map(|(_, r)| *r).collect();
        out.sort_by(|a, b| a.top().total_cmp(&b.top()));
        out
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
        let mut n = notify(3, 1, true);
        let (texts, _) = page(&ctx, &THREE, &mut n, Vec::new());
        for words in ["Done", "Failed", "Cancelled", "System notification", "Sound", "Webhook", "When failed", "WEBHOOK", "Format"] {
            assert!(at(&texts, words).is_some(), "{words} is on the page");
        }
        assert_eq!(all(&texts, "Webhook").len(), 2, "its row in the table and its card");
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
        let mut n = notify(1, 1, false);
        let (texts, _) = page(&ctx, &one, &mut n, Vec::new());
        assert!(at(&texts, "System notification").is_some());
        assert!(at(&texts, "Alert sound").is_some());
        assert!(at(&texts, "Alert").is_none(), "no table's heading");
        assert_eq!(all(&texts, "Webhook").len(), 1, "its card, but no row for the webhook");
    }

    #[test]
    fn the_test_buttons_are_handed_back_and_reset_keeps_the_addresses() {
        let ctx = egui::Context::default();
        let mut n = notify(3, 2, true);
        n.hooks[0].url = "ntfy.sh/x".to_owned();
        n.hooks[1].format = "json".to_owned();
        n.focus = false;
        let hooks = n.hooks.clone();
        let (texts, _) = page(&ctx, &THREE, &mut n, Vec::new());
        assert!(at(&texts, EN.webhook_bad).is_some(), "an address without http(s) is pointed out");
        assert_eq!(all(&texts, "Webhook 2").len(), 2, "each has its name: its row and its card");
        let (_, test) = page(&ctx, &THREE, &mut n, click(at(&texts, "Send a test").unwrap().center()));
        assert_eq!(test, Some(Test::All));
        let (_, test) = page(&ctx, &THREE, &mut n, click(all(&texts, "Send a test here")[1].center()));
        assert_eq!(test, Some(Test::Hook(1)), "the second's own");
        page(&ctx, &THREE, &mut n, click(at(&texts, "Reset to defaults").unwrap().center()));
        assert!(n.focus, "back to the defaults");
        assert_eq!(n.hooks, hooks, "but the places stay");
    }

    #[test]
    fn webhooks_are_added_and_removed() {
        let ctx = egui::Context::default();
        let mut n = notify(3, 0, false);
        let (texts, _) = page(&ctx, &THREE, &mut n, Vec::new());
        assert!(at(&texts, EN.webhook_none).is_some(), "none yet, and what that means");
        page(&ctx, &THREE, &mut n, click(at(&texts, "Add an address").unwrap().center()));
        assert_eq!(n.hooks, [Hook::new(3)]);
        let (texts, _) = page(&ctx, &THREE, &mut n, Vec::new());
        assert!(at(&texts, EN.webhook_empty).is_some() && at(&texts, "Another address").is_some());
        page(&ctx, &THREE, &mut n, click(at(&texts, "Add an address").unwrap().center()));
        assert_eq!(n.hooks.len(), 2);
        n.hooks[0].url = "https://ntfy.sh/a".to_owned();
        n.hooks[1].url = "https://ntfy.sh/b".to_owned();
        let (texts, _) = page(&ctx, &THREE, &mut n, Vec::new());
        assert_eq!(all(&texts, "Remove").len(), 2);
        page(&ctx, &THREE, &mut n, click(all(&texts, "Remove")[0].center()));
        assert_eq!(n.hooks.iter().map(|h| h.url.as_str()).collect::<Vec<_>>(), ["https://ntfy.sh/b"], "the first went");
    }

    #[test]
    fn a_hook_is_its_place_and_back() {
        let events = ["waiting", "error", "done"];
        let some = Hook { url: "https://ntfy.sh/x".into(), format: "discord".into(), on: vec![true, false, true] };
        let p = some.place(&events);
        assert_eq!(p.states, ["waiting", "done"]);
        assert_eq!(Hook::of(&p, &events, true), some);
        let every = Hook { on: Vec::new(), ..some.clone() };
        assert_eq!(every.place(&events).states, events, "no rows: every event");
        assert_eq!(Hook::of(&p, &events, false), every);
        assert!(Hook::new(1).on.is_empty() && Hook::new(2).on == [true, true]);
        assert_eq!((hook_label(&EN, 1, 0), hook_label(&JA, 3, 2)), ("Webhook".to_owned(), "Webhook 3".to_owned()));
    }

    #[test]
    fn the_webhooks_are_written_as_tables_and_the_old_address_goes() {
        let place = |url: &str, format: &str, states: &[&str]| Place {
            url: url.into(),
            format: format.into(),
            states: states.iter().map(|s| (*s).to_owned()).collect(),
        };
        let old = "[notify]\nsystem = [\"waiting\"]\nwebhook = \"https://ntfy.sh/a\" # the phone\nwebhook_format = \"ntfy\"\n\
                   webhook_states = [\"waiting\", \"error\"]\nwebhook_after = 120\n";
        let places =
            [place("https://ntfy.sh/a", "ntfy", &["waiting", "error"]), place("https://discord.com/api/webhooks/1", "discord", &["done"])];
        let new = write_webhooks(old, &places, true).unwrap();
        assert_eq!(
            new,
            "[notify]\nsystem = [\"waiting\"]\nwebhook_after = 120\n\n[[notify.webhooks]]\nurl = \"https://ntfy.sh/a\"\nformat = \"ntfy\"\n\
             states = [\"waiting\", \"error\"]\n\n[[notify.webhooks]]\nurl = \"https://discord.com/api/webhooks/1\"\nformat = \"discord\"\n\
             states = [\"done\"]\n"
        );
        assert_eq!(write_webhooks(&new, &places, true).unwrap(), new, "the same again changes nothing");
        let kept = write_webhooks(&new.replace("format = \"discord\"", "format = \"discord\" # chat"), &places[1..], true).unwrap();
        assert!(!kept.contains("ntfy.sh/a") && kept.contains("# chat"), "the one left keeps its comment: {kept}");
        let bare = write_webhooks("", &places[..1], false).unwrap();
        assert_eq!(bare, "[[notify.webhooks]]\nurl = \"https://ntfy.sh/a\"\nformat = \"ntfy\"\n", "states only when asked");
        assert_eq!(write_webhooks("[ui]\nscale = 1.0\n", &[], true).unwrap(), "[ui]\nscale = 1.0\n", "none: nothing written");
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
        assert_eq!(FORMATS.map(format_name), ["ntfy", "Slack", "Discord", "JSON"]);
        assert!(notify_index(&JA).contains(&"Windows の集中モードに従う"));
    }
}
