//! Document tabs for the apps that open several files in one window.
//!
//! * [`Tabs`]: the model, with no window. An ordered list of ids (the app
//!   chooses them, usually paths), the front one, and a limit. [`Op`] is what
//!   a person can ask of it, and [`Tabs::apply`] does it.
//! * [`labels`]: names that tell two `README.md` apart.
//! * [`Tabs::save`] / [`Tabs::load`]: the list kept as TOML in the state
//!   folder through [`ito_common::kept`].
//! * [`Settings`]: how the tabs behave (when the bar shows, where a file
//!   opened from outside goes, what reopens at start).
//! * [`command`]: the names the app binds keys to (`tab.next`, ...). The bar
//!   and the model hold no keys.
//! * `bar` (feature `egui`): the strip under the toolbar. It closes nothing
//!   itself: it returns an [`Op`] and the app decides (it may ask first when
//!   a tab has unsaved changes).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(feature = "egui")]
mod bar;
#[cfg(feature = "egui")]
pub use bar::{bar, TabView, HEIGHT};

/// How many tabs are kept unless the app says otherwise.
pub const DEFAULT_LIMIT: usize = 12;

/// What a person can ask of the tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// The tab to the right of the front one, wrapping.
    Next,
    /// The tab to the left of the front one, wrapping.
    Prev,
    /// Close the front tab.
    Close,
    /// Close the tab at this index.
    CloseAt(usize),
    /// Bring the tab at this index to the front.
    Goto(usize),
    /// Drag the tab at `.0` to index `.1`.
    Move(usize, usize),
}

/// The command names an app binds in its keymap. Suggested keys: Ctrl+Tab /
/// Ctrl+PageDown, Ctrl+Shift+Tab / Ctrl+PageUp, Ctrl+W, Alt+1..9.
pub mod cmd {
    pub const NEXT: &str = "tab.next";
    pub const PREV: &str = "tab.prev";
    pub const CLOSE: &str = "tab.close";
    pub const GOTO: [&str; 9] =
        ["tab.goto_1", "tab.goto_2", "tab.goto_3", "tab.goto_4", "tab.goto_5", "tab.goto_6", "tab.goto_7", "tab.goto_8", "tab.goto_9"];
}

/// The [`Op`] a command name means, or `None` for a name that is not a tab
/// command. `tab.goto_N` is index `N - 1`.
pub fn command(name: &str) -> Option<Op> {
    match name {
        cmd::NEXT => Some(Op::Next),
        cmd::PREV => Some(Op::Prev),
        cmd::CLOSE => Some(Op::Close),
        _ => cmd::GOTO.iter().position(|c| *c == name).map(Op::Goto),
    }
}

/// An ordered list of tabs and the front one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tabs<Id> {
    items: Vec<Id>,
    front: usize,
    limit: usize,
}

impl<Id: Eq + Clone> Default for Tabs<Id> {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl<Id: Eq + Clone> Tabs<Id> {
    /// No tabs; at most `limit` (at least 1) are kept.
    pub fn new(limit: usize) -> Self {
        Self { items: Vec::new(), front: 0, limit: limit.max(1) }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn items(&self) -> &[Id] {
        &self.items
    }

    /// The index of the front tab (0 when there are none).
    pub fn front_index(&self) -> usize {
        self.front
    }

    pub fn front(&self) -> Option<&Id> {
        self.items.get(self.front)
    }

    pub fn position(&self, id: &Id) -> Option<usize> {
        self.items.iter().position(|i| i == id)
    }

    /// Open `id`: the tab it already has comes forward, else a new tab
    /// opens beside the front one and comes forward. Returns its index.
    /// Over the limit the leftmost other tab goes; see
    /// [`open_evicting`](Self::open_evicting) to learn which.
    pub fn open(&mut self, id: Id) -> usize {
        self.open_evicting(id).0
    }

    /// [`open`](Self::open), also returning the id dropped to stay within
    /// the limit (the app may want to ask about it, or save it).
    pub fn open_evicting(&mut self, id: Id) -> (usize, Option<Id>) {
        if let Some(i) = self.position(&id) {
            self.front = i;
            return (i, None);
        }
        let at = if self.items.is_empty() { 0 } else { self.front + 1 };
        self.items.insert(at, id.clone());
        self.front = at;
        let mut dropped = None;
        if self.items.len() > self.limit {
            // The leftmost tab that is not the one just opened.
            let out = if at == 0 { 1 } else { 0 };
            dropped = Some(self.items.remove(out));
        }
        let i = self.position(&id).unwrap_or(0);
        self.front = i;
        (i, dropped)
    }

    /// Do `op`. Returns the id of the tab it closed, if it closed one.
    /// An index out of range does nothing.
    pub fn apply(&mut self, op: Op) -> Option<Id> {
        let n = self.items.len();
        match op {
            Op::Next if n > 0 => self.front = (self.front + 1) % n,
            Op::Prev if n > 0 => self.front = (self.front + n - 1) % n,
            Op::Goto(i) if i < n => self.front = i,
            Op::Close if n > 0 => return self.close_at(self.front),
            Op::CloseAt(i) if i < n => return self.close_at(i),
            Op::Move(from, to) if from < n && to < n && from != to => {
                let id = self.items.remove(from);
                self.items.insert(to, id);
                // The front tab stays the same tab.
                self.front = if self.front == from {
                    to
                } else if from < self.front && self.front <= to {
                    self.front - 1
                } else if to <= self.front && self.front < from {
                    self.front + 1
                } else {
                    self.front
                };
            }
            _ => {}
        }
        None
    }

    /// Close the tab at `i`. When it was the front one, the tab that took
    /// its place (the one to its right) comes forward, or the last one.
    fn close_at(&mut self, i: usize) -> Option<Id> {
        let id = self.items.remove(i);
        if i < self.front {
            self.front -= 1;
        }
        if self.front >= self.items.len() {
            self.front = self.items.len().saturating_sub(1);
        }
        Some(id)
    }
}

/// The saved form: the front tab and the ids in order.
#[derive(Serialize, Deserialize)]
struct Saved<Id> {
    front: usize,
    tabs: Vec<Id>,
}

/// The file the tabs are kept in: `tabs.toml` in the app's state folder.
pub fn state_path(app: &str) -> Option<PathBuf> {
    Some(ito_common::state_dir(app)?.join("tabs.toml"))
}

impl<Id: Eq + Clone + Serialize> Tabs<Id> {
    /// The tabs as TOML.
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string(&Saved { front: self.front, tabs: self.items.clone() }).map_err(|e| e.to_string())
    }

    /// Hand the tabs to `writer` (one on [`state_path`]). Its thread writes
    /// them, and nothing is written for a run a script drives.
    pub fn save(&self, writer: &ito_common::kept::Writer) {
        if let Ok(text) = self.to_toml() {
            writer.save(text);
        }
    }
}

impl<Id: Eq + Clone + for<'de> Deserialize<'de>> Tabs<Id> {
    /// Tabs from [`to_toml`](Tabs::to_toml)'s text with this `limit`; `None`
    /// when it does not read. Repeats are dropped, extras past the limit are
    /// cut, and the front is put in range.
    pub fn from_toml(text: &str, limit: usize) -> Option<Self> {
        let saved: Saved<Id> = toml::from_str(text).ok()?;
        Some(Self::restore(saved, limit))
    }

    /// The tabs kept in `path` ([`state_path`]); empty when there is none or
    /// it does not read.
    pub fn load(path: &Path, limit: usize) -> Self {
        match ito_common::kept::read::<Saved<Id>>(path) {
            Some(saved) => Self::restore(saved, limit),
            None => Self::new(limit),
        }
    }

    fn restore(saved: Saved<Id>, limit: usize) -> Self {
        let mut t = Self::new(limit);
        let front_id = saved.tabs.get(saved.front).cloned();
        for id in saved.tabs {
            if t.items.len() < t.limit && !t.items.contains(&id) {
                t.items.push(id);
            }
        }
        t.front = front_id.and_then(|f| t.position(&f)).unwrap_or(0);
        t
    }
}

/// Names for tabs of files: the file name, and where two files share one,
/// enough of the folders above it to tell them apart (`docs/README.md`,
/// `api/README.md`). A file that has no duplicate keeps its bare name.
pub fn labels<P: AsRef<Path>>(paths: &[P]) -> Vec<String> {
    let parts: Vec<Vec<String>> = paths
        .iter()
        .map(|p| {
            p.as_ref().components().filter_map(|c| c.as_os_str().to_str()).filter(|s| *s != "/" && *s != "\\").map(str::to_owned).collect()
        })
        .collect();
    let mut depth = vec![1usize; paths.len()];
    loop {
        let names: Vec<String> = (0..parts.len()).map(|i| join(&parts[i], depth[i])).collect();
        let mut grew = false;
        for i in 0..names.len() {
            let clash = (0..names.len()).any(|j| j != i && names[j] == names[i]);
            if clash && depth[i] < parts[i].len() {
                depth[i] += 1;
                grew = true;
            }
        }
        if !grew {
            return names;
        }
    }
}

fn join(parts: &[String], depth: usize) -> String {
    parts[parts.len().saturating_sub(depth)..].join("/")
}

/// When the tab bar shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShowBar {
    Always,
    /// Only while two or more tabs are open.
    #[default]
    TwoOrMore,
    Never,
}

impl ShowBar {
    pub fn visible(self, tabs: usize) -> bool {
        match self {
            ShowBar::Always => true,
            ShowBar::TwoOrMore => tabs >= 2,
            ShowBar::Never => false,
        }
    }
}

/// Where a file opened from outside (another app, the command line while the
/// app runs) goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenFromOutside {
    /// A new tab in the running window.
    #[default]
    NewTab,
    NewWindow,
}

/// What reopens at start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reopen {
    /// The tabs of last time.
    #[default]
    LastTabs,
    /// Only the file the app was given.
    OnlyGiven,
}

/// The tab settings, as they are written in a settings file (`[tabs]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub show_bar: ShowBar,
    pub open_from_outside: OpenFromOutside,
    pub reopen: Reopen,
    /// How many tabs are kept.
    pub limit: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_bar: ShowBar::default(),
            open_from_outside: OpenFromOutside::default(),
            reopen: Reopen::default(),
            limit: DEFAULT_LIMIT,
        }
    }
}

#[cfg(test)]
mod tests;
