//! `[notify]` in an app's `config.toml`, read and written the one way every app
//! does (`docs/common-spec.md`, Notifications): which events are told through
//! the system's notification, a sound and the webhooks, the sound of each
//! event, Windows' focus mode, and `[[notify.webhooks]]`. The events are the
//! app's ([`Event`]); the page that shows this is [`crate::notify_page`].
//!
//! An app puts [`NotifyCfg`] in its own config struct (`#[serde(default)]
//! notify: NotifyCfg`), calls [`NotifyCfg::check`] once it is read, and then:
//! [`NotifyCfg::tell`] when an event happens, [`NotifyCfg::page`] to show it,
//! [`NotifyCfg::edits`] and [`NotifyCfg::webhooks_of`] to write the page's
//! changes back, and [`test`] for "Send a test".

use ito_notify::toast::{App, Icon};
use ito_notify::webhook::{self, Level, Place};
use serde::Deserialize;
use std::collections::BTreeMap;

use crate::{Hook, Notify, Test};

/// One thing the app tells of: its name (what the lists hold), the key of its
/// sound, the sound it starts with, and how the webhooks mark it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub name: &'static str,
    /// The `[notify]` key that picks its sound, e.g. `sound_done`.
    pub sound_key: &'static str,
    /// One of `ito_notify::SOUNDS`.
    pub sound: &'static str,
    pub level: Level,
}

/// The two events of a long job: it finished, or it failed (kura, michi).
pub const DONE_FAILED: [Event; 2] = [
    Event { name: "done", sound_key: "sound_done", sound: "chime", level: Level::Done },
    Event { name: "failed", sound_key: "sound_failed", sound: "alert", level: Level::Error },
];

/// A `[[notify.webhooks]]` table: its `url`, `format` and the events sent
/// there (`[]`: none; not written: every one, see [`NotifyCfg::check`]).
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HookCfg {
    #[serde(default)]
    pub url: String,
    #[serde(default = "ntfy")]
    pub format: String,
    pub states: Option<Vec<String>>,
}

fn ntfy() -> String {
    "ntfy".to_owned()
}

/// `[notify]`. What is not known is warned of and set back by
/// [`NotifyCfg::check`], which also fills in what was not written; read it
/// before using the rest.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct NotifyCfg {
    /// The events told through the system's notification (not written: all).
    pub system: Option<Vec<String>>,
    /// The events told with a sound (not written: none).
    pub sound: Option<Vec<String>>,
    /// The one webhook of before (`webhook`, `webhook_format`,
    /// `webhook_states`): read as the first of [`NotifyCfg::places`].
    pub webhook_states: Option<Vec<String>>,
    /// Windows: no sound in focus mode or under a full-screen program (not
    /// written: on).
    pub focus_mode: Option<bool>,
    /// Where to send it too; empty: nowhere. Kept when it is not http(s), for
    /// the settings to show in red, but not sent to.
    pub webhook: Option<String>,
    /// One of `ito_notify::FORMATS`.
    pub webhook_format: Option<String>,
    /// `[[notify.webhooks]]`, as written.
    #[serde(default)]
    pub webhooks: Vec<HookCfg>,
    /// The sound keys (`sound_done`, …): the app's own, so not fields.
    #[serde(flatten)]
    other: BTreeMap<String, toml::Value>,
    /// The sound of each event, in the order of the app's events. Made by
    /// [`NotifyCfg::check`].
    #[serde(skip)]
    pub sounds: Vec<String>,
    /// Where it is told of, in order: the one webhook of before, then
    /// `webhooks`. Made by [`NotifyCfg::check`], not read.
    #[serde(skip)]
    pub places: Vec<Place>,
}

fn names(events: &[Event]) -> Vec<&'static str> {
    events.iter().map(|e| e.name).collect()
}

fn list(events: &[Event], col: &[bool]) -> String {
    let on: Vec<String> = events.iter().zip(col).filter(|(_, on)| **on).map(|(e, _)| format!("\"{}\"", e.name)).collect();
    format!("[{}]", on.join(", "))
}

impl NotifyCfg {
    /// What no `[notify]` at all comes to, for the app's `events`.
    pub fn new(events: &[Event]) -> NotifyCfg {
        let mut n = NotifyCfg::default();
        n.check(events, "", &mut Vec::new());
        n
    }

    /// Fill in what was not written, and warn of an event, a sound or a format
    /// the app does not know (with `path` in front) and set it back; and of a
    /// webhook that is not http(s).
    pub fn check(&mut self, events: &[Event], path: &str, warnings: &mut Vec<String>) {
        let all = || events.iter().map(|e| e.name.to_owned()).collect::<Vec<_>>();
        let known = names(events);
        let mut lists = [
            ("system", self.system.take().unwrap_or_else(all)),
            ("sound", self.sound.take().unwrap_or_default()),
            ("webhook_states", self.webhook_states.take().unwrap_or_else(all)),
        ];
        for (key, l) in &mut lists {
            l.retain(|e| {
                let ok = known.contains(&e.as_str());
                if !ok {
                    warnings.push(format!("{path}: [notify] `{key}`: `{e}` is not {}", known.join(" or ")));
                }
                ok
            });
        }
        let [(_, system), (_, sound), (_, states)] = lists;
        (self.system, self.sound) = (Some(system), Some(sound));
        self.focus_mode.get_or_insert(true);
        self.sounds = events
            .iter()
            .map(|e| match self.other.get(e.sound_key) {
                None => e.sound.to_owned(),
                Some(v) => match v.as_str().filter(|s| ito_notify::SOUNDS.contains(s)) {
                    Some(s) => s.to_owned(),
                    None => {
                        warnings.push(format!("{path}: [notify] `{}` `{v}` is not {}", e.sound_key, ito_notify::SOUNDS.join(", ")));
                        e.sound.to_owned()
                    }
                },
            })
            .collect();
        let mut format = self.webhook_format.take().unwrap_or_else(ntfy);
        if !ito_notify::FORMATS.contains(&format.as_str()) {
            warnings.push(format!("{path}: [notify] `webhook_format` `{format}` is not {}", ito_notify::FORMATS.join(", ")));
            format = ntfy();
        }
        self.webhook_format = Some(format);
        let url = self.webhook.clone().unwrap_or_default();
        if !url.is_empty() && !ito_notify::is_http(&url) {
            warnings.push(format!("{path}: [notify] `webhook` `{url}` is not an http(s) address"));
        }
        let mut hooks = Vec::new();
        for (i, h) in self.webhooks.iter().enumerate() {
            let states = h.states.clone().unwrap_or_else(all);
            let mut p = Place { url: h.url.trim().to_owned(), format: h.format.clone(), states };
            warnings.extend(p.problems(&known).into_iter().map(|w| format!("{path}: [[notify.webhooks]] {}: {w}", i + 1)));
            if !ito_notify::FORMATS.contains(&p.format.as_str()) {
                p.format = ntfy();
            }
            p.states.retain(|s| known.contains(&s.as_str()));
            hooks.push(p);
        }
        let old = Place { url: url.trim().to_owned(), format: self.webhook_format.clone().unwrap_or_else(ntfy), states };
        self.places = webhook::places(old, hooks);
    }

    fn on(&self, which: &Option<Vec<String>>, event: &str) -> bool {
        which.as_ref().is_some_and(|l| l.iter().any(|e| e == event))
    }

    /// Whether sounds keep quiet in focus mode.
    pub fn focus_mode(&self) -> bool {
        self.focus_mode.unwrap_or(true)
    }

    /// Tell of `event` (one of the app's `events`) in every way this turns on
    /// for it: the system's notification with the app's icon, the sound, and
    /// the webhooks.
    pub fn tell(&self, events: &[Event], app: App, icon: Icon, event: &str, title: &str, body: &str) {
        let Some(k) = events.iter().position(|e| e.name == event) else { return };
        let sound = self.on(&self.sound, event).then(|| self.sounds.get(k).map_or(events[k].sound, String::as_str));
        send(self.focus_mode(), self.on(&self.system, event).then_some(app), icon, sound, title, body);
        webhook::send_all(&self.places, event, events[k].level, title, body, "");
    }

    /// As the settings' Notifications page shows it: a column per event, in
    /// the order of `events`.
    pub fn page(&self, events: &[Event]) -> Notify {
        let col = |l: &Option<Vec<String>>| events.iter().map(|e| self.on(l, e.name)).collect();
        let sounds = (0..events.len()).map(|k| self.sounds.get(k).cloned().unwrap_or_else(|| events[k].sound.to_owned())).collect();
        Notify {
            system: col(&self.system),
            sound: col(&self.sound),
            sounds,
            focus: self.focus_mode(),
            hooks: self.places.iter().map(|p| Hook::of(p, &names(events), true)).collect(),
        }
    }

    /// The webhooks to write when the page's `p` changed them: None when they
    /// are the same as now.
    pub fn webhooks_of(&self, events: &[Event], p: &Notify) -> Option<Vec<Place>> {
        let now: Vec<Place> = p.hooks.iter().map(|h| h.place(&names(events))).collect();
        (now != self.places).then_some(now)
    }

    /// The `[notify]` keys to write, as TOML, so that the file reads as the
    /// page's `p`: only those that changed.
    pub fn edits(&self, events: &[Event], p: &Notify) -> Vec<(&'static str, String)> {
        let was = self.page(events);
        let mut out = Vec::new();
        for (key, a, b) in [("system", &was.system, &p.system), ("sound", &was.sound, &p.sound)] {
            if a != b {
                out.push((key, list(events, b)));
            }
        }
        for (k, e) in events.iter().enumerate() {
            if let Some(now) = p.sounds.get(k).filter(|now| Some(*now) != was.sounds.get(k)) {
                out.push((e.sound_key, ito_common::quote(now)));
            }
        }
        if p.focus != was.focus {
            out.push(("focus_mode", p.focus.to_string()));
        }
        out
    }
}

fn send(focus_mode: bool, app: Option<App>, icon: Icon, sound: Option<&str>, title: &str, body: &str) {
    if let Some(app) = app {
        ito_notify::toast::show_with_icon(app, icon, title, body);
    }
    if let Some(name) = sound {
        if !(focus_mode && ito_notify::sound::quiet_time()) {
            ito_notify::sound::play(name);
        }
    }
}

/// "Send a test", as `p` on the page sets things now: all of `Test::All` (the
/// first event's notice, through the system's notification, the sound and
/// every webhook), or only the webhook `Test::Hook` names, whatever it is
/// told of.
pub fn test(events: &[Event], p: &Notify, what: Test, app: App, icon: Icon, title: &str, body: &str) {
    let places: Vec<Place> = p.hooks.iter().map(|h| h.place(&names(events))).collect();
    let (name, level) = events.first().map_or(("done", Level::Done), |e| (e.name, e.level));
    match what {
        Test::All => {
            send(p.focus, Some(app), icon, p.sounds.first().map(String::as_str), title, body);
            webhook::send_all(&places, name, level, title, body, "");
        }
        Test::Hook(k) => {
            if let Some(h) = places.get(k).filter(|h| ito_notify::is_http(&h.url)) {
                webhook::send(h.url.trim().to_owned(), h.format.clone(), level, title.to_owned(), body.to_owned(), String::new());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> (NotifyCfg, Vec<String>) {
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            notify: NotifyCfg,
        }
        let mut n = toml::from_str::<File>(text).unwrap().notify;
        let mut w = Vec::new();
        n.check(&DONE_FAILED, "config.toml", &mut w);
        (n, w)
    }

    /// The page shows the file's lists as columns, and writes back only what
    /// was changed, as the file spells it.
    #[test]
    fn the_page_writes_only_what_changed() {
        let was = NotifyCfg::new(&DONE_FAILED);
        let mut p = was.page(&DONE_FAILED);
        assert_eq!(p.system, [true, true]);
        assert_eq!(p.sound, [false, false]);
        assert_eq!(p.sounds, ["chime", "alert"]);
        assert!(was.edits(&DONE_FAILED, &p).is_empty());
        p.system[1] = false;
        p.sound[1] = true;
        p.sounds[1] = "low".into();
        p.focus = false;
        assert_eq!(
            was.edits(&DONE_FAILED, &p),
            [
                ("system", "[\"done\"]".to_owned()),
                ("sound", "[\"failed\"]".to_owned()),
                ("sound_failed", "\"low\"".to_owned()),
                ("focus_mode", "false".to_owned()),
            ]
        );
        assert_eq!(was.webhooks_of(&DONE_FAILED, &p), None, "no webhook, none to write");
        p.hooks.push(Hook::new(2));
        p.hooks[0].url = "http://127.0.0.1:8765/hook".into();
        let place = Place { url: "http://127.0.0.1:8765/hook".into(), format: "ntfy".into(), states: vec!["done".into(), "failed".into()] };
        assert_eq!(was.webhooks_of(&DONE_FAILED, &p), Some(vec![place.clone()]));
        let now = NotifyCfg { places: vec![place], ..was };
        assert_eq!(now.page(&DONE_FAILED).hooks, p.hooks, "and back");
        assert_eq!(now.webhooks_of(&DONE_FAILED, &p), None);
    }

    /// What is not known is warned of, with the file's name, and set back.
    #[test]
    fn what_is_not_known_is_warned_of_and_set_back() {
        let (n, w) = read(
            "[notify]\nsystem = [\"done\", \"oops\"]\nsound_done = \"nope\"\nsound_failed = \"low\"\nwebhook = \"ftp://x\"\nwebhook_format = \"xml\"\n\
             [[notify.webhooks]]\nurl = \"http://h/\"\nformat = \"rss\"\nstates = [\"failed\", \"x\"]\n",
        );
        assert_eq!(n.system.as_deref(), Some(&["done".to_owned()][..]));
        assert_eq!(n.sounds, ["chime", "low"]);
        assert_eq!(n.webhook_format.as_deref(), Some("ntfy"));
        assert_eq!(w.len(), 6, "{w:?}");
        assert!(w.iter().all(|w| w.starts_with("config.toml: ")));
        assert_eq!(n.places.len(), 2, "the ftp address is kept, for the page to show in red");
        assert_eq!(n.places[1].format, "ntfy");
        assert_eq!(n.places[1].states, ["failed"]);
    }

    /// The one webhook of before is the first place, ahead of the tables.
    #[test]
    fn the_old_single_webhook_is_the_first_place() {
        let (n, w) = read("[notify]\nwebhook = \"http://a/\"\nwebhook_states = [\"failed\"]\n[[notify.webhooks]]\nurl = \"http://b/\"\n");
        assert!(w.is_empty(), "{w:?}");
        let urls: Vec<&str> = n.places.iter().map(|p| p.url.as_str()).collect();
        assert_eq!(urls, ["http://a/", "http://b/"]);
        assert_eq!(n.places[0].states, ["failed"]);
        assert_eq!(n.places[1].states, ["done", "failed"], "not written: every event");
    }

    /// A `[notify]` with one key (or none) reads: the rest is not required.
    #[test]
    fn a_table_with_one_key_reads() {
        let (n, w) = read(
            "[notify]
system = [\"done\"]
",
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(n.system.as_deref(), Some(&["done".to_owned()][..]));
        assert!(n.places.is_empty());
        let (n, _) = read(
            "[notify]
",
        );
        assert_eq!(n, NotifyCfg::new(&DONE_FAILED));
    }

    /// An app with one event: no second column, the sound by its own key.
    #[test]
    fn one_event_has_one_column() {
        let one = [Event { name: "done", sound_key: "sound_done", sound: "default", level: Level::Done }];
        let n = NotifyCfg::new(&one);
        let p = n.page(&one);
        assert_eq!((p.system.len(), p.sounds.as_slice()), (1, &["default".to_owned()][..]));
    }
}
