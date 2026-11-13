//! The picker's state machine: no egui, no language, no disk. Everything the dialog decides is here.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::{Duration, Instant};

use crate::path::{has_bad_char, is_component, is_device_name, PathError, Style};

/// What the dialog picks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum Mode {
    /// One file.
    OpenFile,
    /// One or more files.
    OpenFiles,
    /// One folder.
    OpenFolder,
    /// One file to write: an existing one or a new name. The picker only picks the path; it never writes.
    SaveFile,
}

/// What a place in the places column is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "lowercase"))]
pub enum PlaceKind {
    Root,
    Drive,
    Home,
    Folder,
}

/// A shortcut in the places column. The app gives these (the engine knows its home and its drives).
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Place {
    pub label: String,
    pub path: String,
    pub kind: PlaceKind,
}

impl Place {
    pub fn new(label: &str, path: &str, kind: PlaceKind) -> Self {
        Place { label: label.to_owned(), path: path.to_owned(), kind }
    }
}

/// Which files show: `Filter::new("Scripts", &["sh", "py"])`. Extensions are written without the dot and compare
/// without case. A dot file such as `.bashrc` has no extension. No extensions at all matches every file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Filter {
    pub name: String,
    pub extensions: Vec<String>,
}

impl Filter {
    pub fn new(name: &str, extensions: &[&str]) -> Self {
        let extensions = extensions.iter().map(|e| e.trim_start_matches("*.").trim_start_matches('.').to_lowercase()).collect();
        Filter { name: name.to_owned(), extensions }
    }

    /// Whether a file called `name` passes.
    pub fn matches(&self, name: &str) -> bool {
        self.extensions.is_empty() || extension(name).is_some_and(|x| self.extensions.contains(&x))
    }
}

/// The lower-case extension of a file name; `None` for `.bashrc`, `Makefile` and `name.`.
pub fn extension(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_lowercase())
}

/// What an entry is. `link` on the entry says it is reached through a link; the kind is what it points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(rename_all = "lowercase"))]
#[non_exhaustive]
pub enum Kind {
    Dir,
    File,
    /// A device, a socket, a pipe.
    Other,
    /// A broken link, or no permission to look.
    Unknown,
}

/// One item of a folder, as the engine spells it.
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Entry {
    /// One path component, exactly as the engine spells it.
    pub name: String,
    pub kind: Kind,
    /// A symlink or junction; `kind` is what it points at (`Unknown` when that cannot be read).
    #[cfg_attr(feature = "serde", serde(default))]
    pub link: bool,
    /// The engine's hidden flag (a Windows attribute). On a Posix machine a dot name is hidden anyway.
    #[cfg_attr(feature = "serde", serde(default))]
    pub hidden: bool,
    /// Files only; `None` is unknown.
    #[cfg_attr(feature = "serde", serde(default))]
    pub size: Option<u64>,
    /// Unix seconds (UTC); `None` is unknown.
    #[cfg_attr(feature = "serde", serde(default))]
    pub modified: Option<i64>,
}

impl Entry {
    fn of(name: &str, kind: Kind) -> Self {
        Entry { name: name.to_owned(), kind, link: false, hidden: false, size: None, modified: None }
    }
    pub fn dir(name: &str) -> Self {
        Self::of(name, Kind::Dir)
    }
    pub fn file(name: &str) -> Self {
        Self::of(name, Kind::File)
    }
    pub fn other(name: &str) -> Self {
        Self::of(name, Kind::Other)
    }
    pub fn unknown(name: &str) -> Self {
        Self::of(name, Kind::Unknown)
    }
    pub fn size(mut self, n: u64) -> Self {
        self.size = Some(n);
        self
    }
    pub fn modified(mut self, secs: i64) -> Self {
        self.modified = Some(secs);
        self
    }
    pub fn link(mut self, on: bool) -> Self {
        self.link = on;
        self
    }
    pub fn hidden(mut self, on: bool) -> Self {
        self.hidden = on;
        self
    }
    /// The lower-case extension (files only).
    pub fn extension(&self) -> Option<String> {
        if self.kind == Kind::File {
            extension(&self.name)
        } else {
            None
        }
    }
}

/// What the engine sends for one folder.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// The folder has more than `entries`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub truncated: bool,
    /// Entries the engine could not send (a name that is not valid text). The dialog adds the entries it refuses.
    #[cfg_attr(feature = "serde", serde(default))]
    pub left_out: u32,
}

impl Listing {
    pub fn new(entries: Vec<Entry>) -> Self {
        Listing { entries, truncated: false, left_out: 0 }
    }
    pub fn truncated(mut self, on: bool) -> Self {
        self.truncated = on;
        self
    }
    pub fn left_out(mut self, n: u32) -> Self {
        self.left_out = n;
        self
    }
}

/// Which request an answer belongs to. Only the newest one is live.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Ticket(u64);

impl fmt::Display for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What the dialog asks of the app. Match with a `_ => {}` arm: more may come.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Want {
    /// List the folder `path` and answer with [`Picker::set_listing`].
    List { ticket: Ticket, path: String },
}

/// How the dialog ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Done {
    /// Paths on the engine's machine: the target folder itself (folder mode) or the target joined with an entry's name.
    Picked(Vec<String>),
    Cancelled,
}

/// What [`Picker::take`] hands over.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Shown {
    pub want: Option<Want>,
    pub done: Option<Done>,
}

/// The column the list is sorted by.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Sort {
    Name,
    Modified,
    Kind,
    Size,
}

/// Where the listing stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status<'a> {
    Loading,
    Ready,
    /// The engine's own text for why the folder could not be listed.
    Failed(&'a str),
}

/// What is wrong with what was typed. No words here: [`crate::Words`] turns it into text.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum InputError {
    /// Not in a complete listing.
    NotFound {
        name: String,
    },
    /// Not in an incomplete listing (cut off, or items left out): that proves nothing.
    NotAmongShown {
        name: String,
        shown: usize,
    },
    /// A file was typed in folder mode.
    ChooseFolder,
    /// The item is neither a file nor a folder.
    CannotOpen,
    /// Several quoted names were typed (Windows).
    OneName,
    /// The name cannot be a file name (save mode).
    BadName {
        name: String,
        why: NameFault,
    },
    /// The name is an existing folder, special item or unknown item in this folder (save mode).
    CannotReplace {
        name: String,
    },
    /// Windows: no item has exactly this name but several match it without case (save mode).
    Ambiguous {
        name: String,
    },
    Path(PathError),
}

/// Why a name cannot be a file name.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum NameFault {
    /// A NUL or control character, or on Windows a character Windows does not allow in a name (`: * ? " < > |`).
    Char,
    /// A Windows device name such as `CON`, `nul.txt` or `COM1`.
    Reserved,
    /// Windows: the name ends with a dot.
    Trailing,
}

/// Where a name is among the entries (save mode).
enum Found {
    At(usize),
    Missing,
    Ambiguous,
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum St {
    Loading,
    Ready,
    Failed(String),
}

/// A typed path whose last part is picked once its folder lists.
#[derive(Clone, Debug)]
struct Pending {
    ticket: Ticket,
    name: String,
    rev: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    Push,
    History,
    Refresh,
}

/// The dialog's state. Build it, then each frame: `show` it (feature `egui`) or call the actions yourself, `take()` what it
/// wants, and answer a [`Want::List`] with [`set_listing`](Picker::set_listing).
pub struct Picker {
    id: String,
    mode: Mode,
    style: Style,
    title: Option<String>,
    start: Option<String>,
    places: Vec<Place>,
    filters: Vec<Filter>,
    filter_i: usize,
    sort: Sort,
    desc: bool,
    hidden: bool,
    first_name: Option<String>,
    select_first: Option<String>,
    started: bool,
    target: String,
    current: Option<String>,
    history: Vec<String>,
    hpos: usize,
    next_ticket: u64,
    live: Option<Ticket>,
    want: Option<Want>,
    status: St,
    entries: Vec<Entry>,
    truncated: bool,
    left_out: u32,
    rows: Vec<usize>,
    cursor: Option<usize>,
    cursor_row: Option<usize>,
    anchor: Option<usize>,
    selected: HashSet<usize>,
    keep: Option<(Vec<String>, Option<String>)>,
    came_from: Option<String>,
    name: String,
    edited: bool,
    rev: u64,
    pending: Option<Pending>,
    error: Option<InputError>,
    path_error: Option<PathError>,
    done: Option<Done>,
    finished: bool,
    ahead: String,
    ahead_at: Option<Instant>,
    save_name: String,
    confirm_replace: bool,
    asking: Option<String>,
    replaces: Option<bool>,
    #[cfg(feature = "egui")]
    pub(crate) ui: crate::window::UiState,
}

impl Picker {
    // ---- Building ----

    /// `id` names the dialog's test IDs (`{id}.ok`, ...): lowercase letters, digits, `-` and `_` joined by `.`.
    pub fn new(id: &str, mode: Mode, style: Style) -> Self {
        debug_assert!(!id.is_empty() && id.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.')), "bad id {id:?}");
        Picker {
            id: id.to_owned(),
            mode,
            style,
            title: None,
            start: None,
            places: Vec::new(),
            filters: Vec::new(),
            filter_i: 0,
            sort: Sort::Name,
            desc: false,
            hidden: false,
            first_name: None,
            select_first: None,
            started: false,
            target: style.default_root().to_owned(),
            current: None,
            history: Vec::new(),
            hpos: 0,
            next_ticket: 0,
            live: None,
            want: None,
            status: St::Loading,
            entries: Vec::new(),
            truncated: false,
            left_out: 0,
            rows: Vec::new(),
            cursor: None,
            cursor_row: None,
            anchor: None,
            selected: HashSet::new(),
            keep: None,
            came_from: None,
            name: String::new(),
            edited: false,
            rev: 0,
            pending: None,
            error: None,
            path_error: None,
            done: None,
            finished: false,
            ahead: String::new(),
            ahead_at: None,
            save_name: String::new(),
            confirm_replace: true,
            asking: None,
            replaces: None,
            #[cfg(feature = "egui")]
            ui: Default::default(),
        }
    }

    /// The folder to open first (default: the first place, else the style's root).
    pub fn start(mut self, folder: &str) -> Self {
        self.start = Some(folder.to_owned());
        self
    }
    pub fn places(mut self, places: Vec<Place>) -> Self {
        self.places = places;
        self
    }
    /// With any filter, "All files" is added last.
    pub fn filters(mut self, filters: Vec<Filter>) -> Self {
        self.filters = filters;
        self
    }
    /// The chosen filter (the app's remembered one); `filters.len()` is "All files".
    pub fn filter_index(mut self, i: usize) -> Self {
        self.filter_i = i;
        self
    }
    pub fn sort(mut self, by: Sort, descending: bool) -> Self {
        self.sort = by;
        self.desc = descending;
        self
    }
    /// The name box's first text; it is also selected in the first listing when it is there, except in `Mode::SaveFile`,
    /// where the name box holds it and nothing is selected.
    pub fn file_name(mut self, name: &str) -> Self {
        self.first_name = Some(name.to_owned());
        self.name = name.to_owned();
        self.save_name = name.to_owned();
        self.edited = !name.is_empty();
        self
    }
    pub fn title(mut self, title: &str) -> Self {
        self.title = Some(title.to_owned());
        self
    }
    pub fn show_hidden(mut self, on: bool) -> Self {
        self.hidden = on;
        self
    }
    /// `SaveFile`: ask before picking an existing file (default `true`). With `false` an existing file is picked at once
    /// and [`replaces`](Self::replaces) says `Some(true)`.
    pub fn confirm_replace(mut self, on: bool) -> Self {
        self.confirm_replace = on;
        self
    }

    // ---- The app's side of the exchange ----

    /// The newest request not yet taken (at most one) and the end of the dialog (once).
    pub fn take(&mut self) -> Shown {
        self.begin();
        Shown { want: self.want.take(), done: self.done.take() }
    }

    /// The answer to a [`Want::List`]. `false`: it was dropped (an old ticket, or the dialog is over).
    pub fn set_listing(&mut self, ticket: Ticket, listing: Result<Listing, String>) -> bool {
        // While the replace question is open the listing behind it stays as it is; the answer lists again if one was lost.
        if self.locked() || self.live != Some(ticket) {
            return false;
        }
        self.live = None;
        match listing {
            Err(text) => {
                self.status = St::Failed(text);
                self.pending = None;
                self.entries.clear();
                self.rows.clear();
                self.cursor = None;
                self.cursor_row = None;
                self.selected.clear();
            }
            Ok(l) => self.arrive(ticket, l),
        }
        true
    }

    fn arrive(&mut self, ticket: Ticket, l: Listing) {
        let mut refused = 0u32;
        let mut entries = Vec::with_capacity(l.entries.len());
        for e in l.entries {
            if is_component(self.style, &e.name) {
                entries.push(e);
            } else {
                refused += 1;
            }
        }
        self.entries = entries;
        self.truncated = l.truncated;
        self.left_out = l.left_out.saturating_add(refused);
        self.status = St::Ready;
        self.current = Some(self.target.clone());
        self.cursor = None;
        self.anchor = None;
        self.selected.clear();
        self.rebuild_rows();
        let by_name: HashMap<&str, usize> = self.entries.iter().enumerate().map(|(i, e)| (e.name.as_str(), i)).collect();
        if let Some((names, cursor)) = self.keep.take() {
            let picked: Vec<usize> = names.iter().filter_map(|n| by_name.get(n.as_str()).copied()).collect();
            let cursor = cursor.and_then(|n| by_name.get(n.as_str()).copied());
            self.selected = picked.into_iter().filter(|&i| self.is_row(i) && self.pickable(&self.entries[i])).collect();
            self.cursor = cursor.filter(|&i| self.is_row(i));
            self.refresh_cursor_row();
            if !self.edited {
                self.write_name();
            }
        } else if let Some(n) = self.came_from.take() {
            self.cursor = by_name.get(n.as_str()).copied().filter(|&i| self.is_row(i));
            self.refresh_cursor_row();
        }
        if let Some(n) = self.select_first.take() {
            let hit = self.entries.iter().position(|e| e.name == n).filter(|&i| self.is_row(i) && self.pickable(&self.entries[i]));
            if let Some(i) = hit {
                self.cursor = Some(i);
                self.anchor = Some(i);
                self.selected = HashSet::from([i]);
                self.refresh_cursor_row();
                self.write_name();
                self.edited = false;
            }
        }
        if let Some(p) = self.pending.take() {
            if p.ticket == ticket && p.rev == self.rev {
                if self.mode == Mode::SaveFile {
                    self.save_lookup(&p.name);
                } else {
                    self.lookup(&p.name);
                }
            }
        }
    }

    pub fn status(&self) -> Status<'_> {
        match &self.status {
            St::Loading => Status::Loading,
            St::Ready => Status::Ready,
            St::Failed(t) => Status::Failed(t),
        }
    }
    /// The last folder that listed successfully (to remember). Never a failed one.
    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }
    /// The folder in the path bar: asked for, maybe still loading or failed.
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn sort_state(&self) -> (Sort, bool) {
        (self.sort, self.desc)
    }
    /// The chosen filter of the app's list; `None` is "All files" (or there are no filters).
    pub fn filter_chosen(&self) -> Option<usize> {
        (self.filter_i < self.filters.len()).then_some(self.filter_i)
    }
    pub fn is_done(&self) -> bool {
        self.finished
    }

    // ---- Things the window reads ----

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn style(&self) -> Style {
        self.style
    }
    pub fn custom_title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    pub fn place_list(&self) -> &[Place] {
        &self.places
    }
    pub fn filter_list(&self) -> &[Filter] {
        &self.filters
    }
    /// The chosen entry of "the filters, then All files".
    pub fn filter_position(&self) -> usize {
        self.filter_i.min(self.filters.len())
    }
    pub fn hidden_shown(&self) -> bool {
        self.hidden
    }
    pub fn error(&self) -> Option<&InputError> {
        self.error.as_ref()
    }
    /// Why the path bar's text could not be read.
    pub fn path_error(&self) -> Option<&PathError> {
        self.path_error.as_ref()
    }
    /// Forget the path error (the window does when the text is edited, or the edit is left).
    pub fn clear_path_error(&mut self) {
        self.path_error = None;
    }
    /// The one place that holds the target, the deepest when several do (`/` and `/home/u` both hold `/home/u/x`).
    pub fn lit_place(&self) -> Option<usize> {
        let mut best: Option<(usize, usize)> = None;
        for (i, p) in self.places.iter().enumerate() {
            if self.style.is_under(&self.target, &p.path) {
                let d = self.style.depth(&p.path);
                // A later place of the same depth does not take it from an earlier one.
                if best.is_none_or(|(_, bd)| d > bd) {
                    best = Some((i, d));
                }
            }
        }
        best.map(|(i, _)| i)
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Indexes into the listing's entries, in the order shown.
    pub fn rows(&self) -> &[usize] {
        &self.rows
    }
    pub fn entry(&self, i: usize) -> Option<&Entry> {
        self.entries.get(i)
    }
    /// The entry on row `row`.
    pub fn row_entry(&self, row: usize) -> Option<&Entry> {
        self.rows.get(row).map(|&i| &self.entries[i])
    }
    pub fn cursor_row(&self) -> Option<usize> {
        self.cursor_row
    }
    pub fn is_selected(&self, row: usize) -> bool {
        self.rows.get(row).is_some_and(|i| self.selected.contains(i))
    }
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }
    /// More than the list shows: the folder has more, or items were left out or refused.
    pub fn incomplete(&self) -> bool {
        self.truncated || self.left_out > 0
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
    /// Items that could not be shown: the engine's `left_out` plus the names the dialog refused.
    pub fn left_out(&self) -> u32 {
        self.left_out
    }
    /// How many entries the listing holds (before filters and hidden).
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    pub fn can_back(&self) -> bool {
        self.hpos > 0
    }
    pub fn can_forward(&self) -> bool {
        self.hpos + 1 < self.history.len()
    }
    pub fn can_up(&self) -> bool {
        self.style.parent(&self.target).is_some()
    }
    /// Whether an entry is hidden by its flag or (Posix) its dot.
    pub fn is_hidden(&self, e: &Entry) -> bool {
        e.hidden || (self.style == Style::Posix && e.name.starts_with('.'))
    }
    /// Whether an entry can be picked or entered: files and folders (folders only in folder mode).
    pub fn pickable(&self, e: &Entry) -> bool {
        match e.kind {
            Kind::Dir => true,
            Kind::File => self.mode != Mode::OpenFolder,
            Kind::Other | Kind::Unknown => false,
        }
    }

    // ---- Navigation ----

    pub(crate) fn begin(&mut self) {
        if self.started || self.finished {
            return;
        }
        self.started = true;
        let root = self.style.default_root();
        let style = self.style;
        let mut err = None;
        let mut resolved = |typed: &str| match style.resolve(typed, root) {
            Ok(p) => Some(p),
            Err(e) => {
                // The first fault is the one to tell: the start the app asked for.
                err.get_or_insert(e);
                None
            }
        };
        let start = self.start.clone().and_then(|s| resolved(&s));
        let path = start.or_else(|| self.places.first().and_then(|p| resolved(&p.path.clone()))).unwrap_or_else(|| root.to_owned());
        self.request(path, Nav::Push, false);
        self.path_error = err;
        // Save mode selects nothing at first: the name box holds the name.
        self.select_first = self.first_name.clone().filter(|_| self.mode != Mode::SaveFile);
    }

    fn request(&mut self, path: String, nav: Nav, clear_name: bool) {
        self.next_ticket += 1;
        let ticket = Ticket(self.next_ticket);
        if nav == Nav::Refresh {
            self.keep = Some((self.selected_names(), self.cursor.map(|i| self.entries[i].name.clone())));
        } else {
            self.keep = None;
            self.select_first = None;
        }
        self.came_from = None;
        self.cursor = None;
        self.cursor_row = None;
        self.anchor = None;
        self.selected.clear();
        self.entries.clear();
        self.rows.clear();
        self.truncated = false;
        self.left_out = 0;
        self.pending = None;
        self.error = None;
        self.path_error = None;
        self.ahead.clear();
        if clear_name {
            if self.mode == Mode::SaveFile {
                // Navigation keeps the name; a plain name is what the box goes back to when typed text only navigates.
                if self.plain_name() {
                    self.save_name = self.typed().to_owned();
                }
            } else {
                self.name.clear();
                self.edited = false;
            }
            self.rev += 1;
        }
        match nav {
            Nav::Push => {
                if !self.history.get(self.hpos).is_some_and(|h| self.style.same(h, &path)) {
                    self.history.truncate(if self.history.is_empty() { 0 } else { self.hpos + 1 });
                    self.history.push(path.clone());
                    self.hpos = self.history.len() - 1;
                }
            }
            Nav::History | Nav::Refresh => {}
        }
        self.target = path.clone();
        self.status = St::Loading;
        self.live = Some(ticket);
        self.want = Some(Want::List { ticket, path });
    }

    /// Go to a place, a crumb, or a path the person edited (read against [`target`](Self::target)).
    pub fn go(&mut self, path: &str) {
        if self.locked() {
            return;
        }
        self.begin();
        match self.style.resolve(path, &self.target) {
            Ok(p) => self.request(p, Nav::Push, true),
            Err(e) => self.path_error = Some(e),
        }
    }
    pub fn up(&mut self) {
        if self.locked() {
            return;
        }
        self.begin();
        if let Some(parent) = self.style.parent(&self.target) {
            let from = self.style.last_part(&self.target);
            self.request(parent, Nav::Push, true);
            self.came_from = from;
        }
    }
    pub fn back(&mut self) {
        if self.locked() {
            return;
        }
        self.begin();
        if self.hpos > 0 {
            self.hpos -= 1;
            let p = self.history[self.hpos].clone();
            self.request(p, Nav::History, true);
        }
    }
    pub fn forward(&mut self) {
        if self.locked() {
            return;
        }
        self.begin();
        if self.hpos + 1 < self.history.len() {
            self.hpos += 1;
            let p = self.history[self.hpos].clone();
            self.request(p, Nav::History, true);
        }
    }
    /// List the same folder again, keeping the selection and the cursor by name.
    pub fn refresh(&mut self) {
        if self.locked() {
            return;
        }
        self.begin();
        let t = self.target.clone();
        self.request(t, Nav::Refresh, false);
    }
    /// After a failure: ask again.
    pub fn retry(&mut self) {
        self.refresh();
    }

    fn go_into(&mut self, name: &str) {
        let p = self.style.join(&self.target, name);
        self.request(p, Nav::Push, true);
    }

    // ---- Rows, cursor, selection ----

    fn is_row(&self, entry: usize) -> bool {
        self.rows.contains(&entry)
    }

    fn selected_names(&self) -> Vec<String> {
        self.rows.iter().filter(|i| self.selected.contains(i)).map(|&i| self.entries[i].name.clone()).collect()
    }

    fn refresh_cursor_row(&mut self) {
        self.cursor_row = self.cursor.and_then(|c| self.rows.iter().position(|&i| i == c));
    }

    fn rebuild_rows(&mut self) {
        let shown: Vec<usize> = (0..self.entries.len()).filter(|&i| self.shown(&self.entries[i])).collect();
        let mut rows = shown;
        let (sort, desc) = (self.sort, self.desc);
        rows.sort_by(|&a, &b| compare(&self.entries[a], &self.entries[b], sort, desc));
        self.rows = rows;
        let rows: HashSet<usize> = self.rows.iter().copied().collect();
        self.selected.retain(|i| rows.contains(i));
        if self.cursor.is_some_and(|c| !rows.contains(&c)) {
            self.cursor = None;
        }
        if self.anchor.is_some_and(|c| !rows.contains(&c)) {
            self.anchor = None;
        }
        self.refresh_cursor_row();
    }

    fn rebuild_keep(&mut self) {
        let before = self.selected.clone();
        self.rebuild_rows();
        if self.selected != before {
            // A pending accept belongs to the selection it was typed against.
            self.pending = None;
            self.rev += 1;
            if !self.edited {
                self.write_name();
            }
        }
    }

    fn shown(&self, e: &Entry) -> bool {
        if self.is_hidden(e) && !self.hidden {
            return false;
        }
        match e.kind {
            Kind::Dir | Kind::Unknown => true,
            Kind::File => self.mode != Mode::OpenFolder && self.filters.get(self.filter_i).is_none_or(|f| f.matches(&e.name)),
            Kind::Other => self.mode != Mode::OpenFolder,
        }
    }

    /// The name box's text for the selection: one name, or several quoted (for display only).
    fn write_name(&mut self) {
        if self.mode == Mode::SaveFile {
            // Only one file names the save; a folder or nothing leaves the typed name alone.
            let files: Vec<usize> = self.selected.iter().copied().filter(|&i| self.entries[i].kind == Kind::File).collect();
            if let [i] = files[..] {
                self.name = self.entries[i].name.clone();
                self.save_name = self.name.clone();
            }
            return;
        }
        let names = self.selected_names();
        self.name = match names.len() {
            0 => String::new(),
            1 => names[0].clone(),
            _ => {
                let mut s = names.iter().take(20).map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(" ");
                if names.len() > 20 {
                    s.push_str(" ...");
                }
                s
            }
        };
    }

    fn selection_changed(&mut self) {
        self.write_name();
        self.edited = false;
        self.rev += 1;
        self.pending = None;
        self.error = None;
        self.ahead.clear();
    }

    /// The cursor goes to `row` and it is the only thing selected (when it can be picked).
    pub fn set_cursor(&mut self, row: usize) {
        if self.locked() {
            return;
        }
        self.begin();
        let Some(&i) = self.rows.get(row) else { return };
        self.cursor = Some(i);
        self.cursor_row = Some(row);
        self.anchor = Some(i);
        self.selected.clear();
        if self.pickable(&self.entries[i]) {
            self.selected.insert(i);
        }
        self.selection_changed();
    }

    /// Move the cursor `by` rows (clamped). `extend` (many-file mode) selects from the anchor to the cursor.
    pub fn move_cursor(&mut self, by: isize, extend: bool) {
        if self.locked() || self.rows.is_empty() {
            return;
        }
        self.begin();
        let last = self.rows.len() as isize - 1;
        let base = self.cursor_row.map_or(if by > 0 { -1 } else { last + 1 }, |r| r as isize);
        let to = (base + by).clamp(0, last) as usize;
        if extend && self.mode == Mode::OpenFiles {
            let anchor = self.anchor.and_then(|a| self.rows.iter().position(|&i| i == a)).or(self.cursor_row).unwrap_or(to);
            self.select_range(anchor, to);
            self.cursor = Some(self.rows[to]);
            self.cursor_row = Some(to);
        } else {
            self.set_cursor(to);
        }
    }

    fn select_range(&mut self, a: usize, b: usize) {
        let (lo, hi) = (a.min(b), a.max(b));
        if self.anchor.is_none() {
            self.anchor = self.rows.get(a).copied();
        }
        self.selected = self.rows[lo..=hi].iter().copied().filter(|&i| self.entries[i].kind == Kind::File).collect();
        self.selection_changed();
    }

    /// A click on `row`. `toggle` (Ctrl/Cmd) and `extend` (Shift) work in many-file mode only.
    pub fn click(&mut self, row: usize, toggle: bool, extend: bool) {
        if self.locked() {
            return;
        }
        self.begin();
        let Some(&i) = self.rows.get(row) else { return };
        if self.mode != Mode::OpenFiles || !(toggle || extend) {
            self.set_cursor(row);
        } else if extend {
            let anchor = self.anchor.and_then(|a| self.rows.iter().position(|&r| r == a)).unwrap_or(row);
            self.select_range(anchor, row);
            self.cursor = Some(i);
            self.cursor_row = Some(row);
        } else {
            if !self.selected.remove(&i) && self.entries[i].kind == Kind::File {
                self.selected.insert(i);
            }
            self.cursor = Some(i);
            self.cursor_row = Some(row);
            self.anchor = Some(i);
            self.selection_changed();
        }
    }

    /// Select every file shown (many-file mode).
    pub fn select_all(&mut self) {
        if self.locked() || self.mode != Mode::OpenFiles {
            return;
        }
        self.selected = self.rows.iter().copied().filter(|&i| self.entries[i].kind == Kind::File).collect();
        self.selection_changed();
    }

    /// A double click, or Enter, on `row`.
    pub fn open_row(&mut self, row: usize) {
        if self.locked() {
            return;
        }
        self.begin();
        let Some(&i) = self.rows.get(row) else { return };
        let e = &self.entries[i];
        match e.kind {
            Kind::Dir => {
                let name = e.name.clone();
                self.go_into(&name);
            }
            Kind::File if self.mode == Mode::SaveFile => {
                let name = e.name.clone();
                self.error = None;
                self.replace_flow(name);
            }
            Kind::Other | Kind::Unknown if self.mode == Mode::SaveFile => {
                let name = e.name.clone();
                self.save_name = name.clone();
                self.error = Some(InputError::CannotReplace { name });
            }
            Kind::File if self.mode != Mode::OpenFolder => {
                let paths = if self.mode == Mode::OpenFiles && self.selected.contains(&i) {
                    self.selected_files()
                } else {
                    vec![self.style.join(&self.target, &e.name)]
                };
                self.finish(Done::Picked(paths));
            }
            _ => {}
        }
    }

    fn selected_files(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|i| self.selected.contains(i) && self.entries[**i].kind == Kind::File)
            .map(|&i| self.style.join(&self.target, &self.entries[i].name))
            .collect()
    }

    // ---- The name box ----

    /// The person typed in the name box.
    pub fn set_name(&mut self, text: &str) {
        if self.locked() || self.name == text {
            return;
        }
        self.name = text.to_owned();
        self.edited = true;
        self.rev += 1;
        self.pending = None;
        self.error = None;
    }

    /// Whether [`accept`](Self::accept) would do anything (the main button is enabled).
    pub fn can_accept(&self) -> bool {
        if self.locked() {
            return false;
        }
        if self.mode == Mode::SaveFile {
            return self.can_accept_save();
        }
        let ready = self.status == St::Ready;
        if !self.edited && !self.selected.is_empty() {
            return match self.mode {
                Mode::OpenFolder => true,
                _ => {
                    let picks = self.selected.iter().map(|&i| &self.entries[i]);
                    picks.clone().any(|e| e.kind == Kind::File) || (self.selected.len() == 1 && ready)
                }
            };
        }
        let text = self.typed();
        if text.is_empty() {
            return self.mode == Mode::OpenFolder && ready;
        }
        if text == "." {
            return self.mode == Mode::OpenFolder && ready;
        }
        text == ".." || self.style.has_sep(text) || self.style.is_absolute(text) || ready
    }

    fn typed(&self) -> &str {
        match self.style {
            Style::Windows => self.name.trim_matches(' '),
            Style::Posix => &self.name,
        }
    }

    /// The main button, or Enter in the name box.
    pub fn accept(&mut self) {
        if self.locked() {
            return;
        }
        self.begin();
        self.error = None;
        if self.mode == Mode::SaveFile {
            self.accept_save();
            return;
        }
        if !self.edited && !self.selected.is_empty() {
            self.accept_selection();
        } else {
            self.accept_text();
        }
    }

    fn accept_selection(&mut self) {
        let picks: Vec<usize> = self.rows.iter().copied().filter(|i| self.selected.contains(i)).collect();
        if self.mode == Mode::OpenFolder {
            if let Some(&i) = picks.first() {
                let p = self.style.join(&self.target, &self.entries[i].name);
                self.finish(Done::Picked(vec![p]));
            }
            return;
        }
        let files = self.selected_files();
        if !files.is_empty() {
            self.finish(Done::Picked(files));
        } else if let [i] = picks[..] {
            if self.entries[i].kind == Kind::Dir {
                let name = self.entries[i].name.clone();
                self.go_into(&name);
            }
        }
    }

    fn pick_target(&mut self) {
        if self.mode == Mode::OpenFolder && self.status == St::Ready {
            let t = self.target.clone();
            self.finish(Done::Picked(vec![t]));
        }
    }

    fn accept_text(&mut self) {
        let text = self.typed().to_owned();
        let style = self.style;
        if text.is_empty() || text == "." {
            self.pick_target();
        } else if style == Style::Windows && text.contains('"') {
            self.error = Some(InputError::OneName);
        } else if text == ".." {
            self.up();
        } else if style.has_sep(&text) || style.is_absolute(&text) {
            match style.resolve(&text, &self.target) {
                Err(e) => self.error = Some(InputError::Path(e)),
                Ok(p) => {
                    let ends_sep = text.chars().last().is_some_and(|c| style.is_sep(c));
                    match style.parent(&p).zip(style.last_part(&p)) {
                        Some((parent, last)) if !ends_sep => {
                            if self.status == St::Ready && style.same(&parent, &self.target) {
                                self.lookup(&last);
                            } else {
                                self.request(parent, Nav::Push, false);
                                self.pending = Some(Pending { ticket: Ticket(self.next_ticket), name: last, rev: self.rev });
                            }
                        }
                        _ => self.request(p, Nav::Push, true),
                    }
                }
            }
        } else if self.status == St::Ready {
            self.lookup(&text);
        }
    }

    /// Find `name` in the listing and act on it: enter a folder, pick a file, or say why not.
    fn lookup(&mut self, name: &str) {
        let exact = self.entries.iter().position(|e| e.name == name);
        let found = exact.or_else(|| {
            if self.style == Style::Windows {
                let mut it = self.entries.iter().enumerate().filter(|(_, e)| self.style.same_name(&e.name, name)).map(|(i, _)| i);
                match (it.next(), it.next()) {
                    (Some(i), None) => Some(i),
                    _ => None,
                }
            } else {
                None
            }
        });
        let Some(i) = found else {
            self.error = Some(if self.incomplete() {
                InputError::NotAmongShown { name: name.to_owned(), shown: self.entries.len() }
            } else {
                InputError::NotFound { name: name.to_owned() }
            });
            return;
        };
        let e = &self.entries[i];
        match e.kind {
            Kind::Dir => {
                let n = e.name.clone();
                self.go_into(&n);
            }
            Kind::File if self.mode == Mode::OpenFolder => self.error = Some(InputError::ChooseFolder),
            Kind::File => {
                let p = self.style.join(&self.target, &e.name);
                self.finish(Done::Picked(vec![p]));
            }
            Kind::Other | Kind::Unknown => self.error = Some(InputError::CannotOpen),
        }
    }

    fn finish(&mut self, done: Done) {
        if self.finished {
            return;
        }
        let done = match done {
            Done::Picked(paths) => {
                let mut seen = HashSet::new();
                Done::Picked(paths.into_iter().filter(|p| seen.insert(p.clone())).collect())
            }
            d => d,
        };
        self.finished = true;
        self.done = Some(done);
        self.want = None;
        self.live = None;
        self.pending = None;
        self.asking = None;
    }

    pub fn cancel(&mut self) {
        self.finish(Done::Cancelled);
    }

    // ---- Saving ----

    /// No action of the model runs: the dialog is over, or the replace question is open (only `cancel` and
    /// [`answer_replace`](Self::answer_replace) act then).
    fn locked(&self) -> bool {
        self.finished || self.asking.is_some()
    }

    /// `SaveFile`: whether the path picked is a file that was in the listing. `Some(true)`: it is, whether or not the
    /// listing was complete. `Some(false)`: it is not and the listing was complete. `None`: it is not in an incomplete
    /// listing (cut off, or items left out), the mode is not `SaveFile`, or nothing was picked (open, or cancelled).
    /// Still readable after [`take`](Self::take) handed out the end. The listing is a snapshot: whoever writes the file
    /// must check again when it writes.
    pub fn replaces(&self) -> Option<bool> {
        self.replaces
    }

    /// The name the replace question is about while it is open (`SaveFile`): the listing's spelling.
    pub fn replace_question(&self) -> Option<&str> {
        self.asking.as_deref()
    }

    /// Answer the replace question. `true`: pick the file ([`replaces`](Self::replaces) is `Some(true)`). `false`: close
    /// the question; the name box keeps the name. Nothing happens when no question is open.
    pub fn answer_replace(&mut self, yes: bool) {
        if self.finished {
            return;
        }
        let Some(name) = self.asking.take() else { return };
        if yes {
            self.replaces = Some(true);
            let p = self.style.join(&self.target, &name);
            self.finish(Done::Picked(vec![p]));
        }
    }

    /// Whether the main button opens a folder (`SaveFile`: a folder is selected and nothing was typed since) rather than
    /// saves.
    pub fn opens_folder(&self) -> bool {
        self.mode == Mode::SaveFile && self.open_case().is_some()
    }

    /// The one selected folder when nothing was typed since (`SaveFile`).
    fn open_case(&self) -> Option<usize> {
        if self.edited {
            return None;
        }
        match self.selected.iter().copied().collect::<Vec<_>>()[..] {
            [i] if self.entries[i].kind == Kind::Dir => Some(i),
            _ => None,
        }
    }

    /// The typed text is one name: not empty, no separator, not absolute, not `.` or `..`.
    fn plain_name(&self) -> bool {
        let t = self.typed();
        !t.is_empty() && !self.style.has_sep(t) && !self.style.is_absolute(t) && t != "." && t != ".."
    }

    /// The text only navigates: it ends with a separator or its last component is `.` or `..`.
    fn nav_only(&self, t: &str) -> bool {
        let last = t.rsplit(|c| self.style.is_sep(c)).next().unwrap_or("");
        t.chars().last().is_some_and(|c| self.style.is_sep(c)) || last == "." || last == ".."
    }

    fn can_accept_save(&self) -> bool {
        if self.open_case().is_some() {
            return true;
        }
        let t = self.typed();
        if t.is_empty() {
            return false;
        }
        self.nav_only(t) || self.style.has_sep(t) || self.style.is_absolute(t) || self.status == St::Ready
    }

    fn find(&self, name: &str) -> Found {
        if let Some(i) = self.entries.iter().position(|e| e.name == name) {
            return Found::At(i);
        }
        if self.style == Style::Windows {
            let mut it = self.entries.iter().enumerate().filter(|(_, e)| self.style.same_name(&e.name, name)).map(|(i, _)| i);
            match (it.next(), it.next()) {
                (Some(i), None) => return Found::At(i),
                (Some(_), Some(_)) => return Found::Ambiguous,
                _ => {}
            }
        }
        Found::Missing
    }

    /// The extension a save adds when the name has none: the first extension of filter `i`, if it is a plain one.
    fn default_ext_of(&self, i: usize) -> Option<&str> {
        let e = self.filters.get(i)?.extensions.first()?.as_str();
        let bad = |c: char| matches!(c, '.' | '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ' ') || c.is_control();
        (!e.is_empty() && !e.chars().any(bad)).then_some(e)
    }

    /// `name` with its extension changed from filter `old` to filter `new`, when that applies.
    fn swap_extension(&self, name: &str, old: usize, new: usize) -> Option<String> {
        let had = self.filters.get(old).filter(|f| !f.extensions.is_empty())?;
        extension(name).filter(|x| had.extensions.contains(x))?;
        let to = self.default_ext_of(new)?;
        // Cut at the dot in the name itself: the lowercased extension can be longer or shorter in bytes.
        let (stem, _) = name.rsplit_once('.')?;
        Some(format!("{stem}.{to}"))
    }

    fn accept_save(&mut self) {
        if let Some(i) = self.open_case() {
            let n = self.entries[i].name.clone();
            self.go_into(&n);
            return;
        }
        let text = self.typed().to_owned();
        let style = self.style;
        if text.is_empty() {
            return;
        }
        if style == Style::Windows && text.contains('"') {
            self.error = Some(InputError::BadName { name: text, why: NameFault::Char });
            return;
        }
        if self.nav_only(&text) {
            match style.resolve(&text, &self.target) {
                Err(e) => self.error = Some(InputError::Path(e)),
                Ok(p) => self.navigate_to(p),
            }
        } else if style.has_sep(&text) || style.is_absolute(&text) {
            match style.resolve(&text, &self.target) {
                Err(e) => self.error = Some(InputError::Path(e)),
                Ok(p) => match style.parent(&p).zip(style.last_part(&p)) {
                    Some((parent, last)) => {
                        if self.status == St::Ready && style.same(&parent, &self.target) {
                            self.save_lookup(&last);
                        } else {
                            self.request(parent, Nav::Push, false);
                            self.name = last.clone();
                            self.pending = Some(Pending { ticket: Ticket(self.next_ticket), name: last, rev: self.rev });
                        }
                    }
                    // A root has no name to save.
                    None => self.navigate_to(p),
                },
            }
        } else if self.status == St::Ready {
            self.save_lookup(&text);
        }
    }

    /// Typed text that only navigates: the name box goes back to the saved name.
    fn navigate_to(&mut self, p: String) {
        self.name = self.save_name.clone();
        if self.style.same(&p, &self.target) {
            self.rev += 1;
            self.pending = None;
        } else {
            self.request(p, Nav::Push, true);
        }
    }

    fn save_lookup(&mut self, name: &str) {
        if let Found::At(i) = self.find(name) {
            if self.entries[i].kind == Kind::Dir {
                let n = self.entries[i].name.clone();
                self.name = self.save_name.clone();
                self.go_into(&n);
                return;
            }
        }
        let add = self
            .default_ext_of(self.filter_i.min(self.filters.len()))
            .filter(|_| extension(name).is_none() && !name.starts_with('.') && !name.ends_with('.'));
        let fin = match add {
            Some(ext) => format!("{name}.{ext}"),
            None => name.to_owned(),
        };
        self.name = fin.clone();
        let why = if fin.chars().any(|c| c == '\0' || c.is_control()) {
            Some(NameFault::Char)
        } else if self.style == Style::Windows {
            if has_bad_char(&fin) {
                Some(NameFault::Char)
            } else if is_device_name(&fin) {
                Some(NameFault::Reserved)
            } else if fin.ends_with('.') {
                Some(NameFault::Trailing)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(why) = why {
            self.error = Some(InputError::BadName { name: fin, why });
            return;
        }
        match self.find(&fin) {
            Found::At(i) => {
                let e = &self.entries[i];
                let n = e.name.clone();
                if e.kind == Kind::File {
                    self.replace_flow(n);
                } else {
                    self.name = n.clone();
                    self.save_name = n.clone();
                    self.error = Some(InputError::CannotReplace { name: n });
                }
            }
            Found::Ambiguous => self.error = Some(InputError::Ambiguous { name: fin }),
            Found::Missing => {
                self.save_name = fin.clone();
                self.replaces = if self.incomplete() { None } else { Some(false) };
                let p = self.style.join(&self.target, &fin);
                self.finish(Done::Picked(vec![p]));
            }
        }
    }

    /// An existing file `n` is the save: ask first, or pick at once.
    fn replace_flow(&mut self, n: String) {
        self.name = n.clone();
        self.save_name = n.clone();
        if self.confirm_replace {
            self.asking = Some(n);
        } else {
            self.replaces = Some(true);
            let p = self.style.join(&self.target, &n);
            self.finish(Done::Picked(vec![p]));
        }
    }

    // ---- Filter, sort, hidden ----

    pub fn set_filter(&mut self, i: usize) {
        if self.locked() {
            return;
        }
        let (old, new) = (self.filter_i.min(self.filters.len()), i.min(self.filters.len()));
        self.filter_i = new;
        if self.mode == Mode::SaveFile && old != new {
            let name = if self.plain_name() { self.swap_extension(self.typed(), old, new) } else { None };
            let saved = self.swap_extension(&self.save_name, old, new);
            if let Some(n) = name {
                self.name = n;
                self.rev += 1;
                self.pending = None;
                self.error = None;
            }
            if let Some(n) = saved {
                self.save_name = n;
            }
        }
        self.rebuild_keep();
    }
    /// Sort by `by`; the same column again flips the order.
    pub fn set_sort(&mut self, by: Sort) {
        if self.locked() {
            return;
        }
        if by == self.sort {
            self.desc = !self.desc;
        } else {
            self.sort = by;
            self.desc = false;
        }
        self.rebuild_keep();
    }
    pub fn set_show_hidden(&mut self, on: bool) {
        if self.locked() || self.hidden == on {
            return;
        }
        self.hidden = on;
        self.rebuild_keep();
    }

    // ---- Type-ahead ----

    /// Committed text typed with the list focused: jump to the next name that starts with it. The prefix resets after one
    /// second; the same letter again goes on to the next name starting with it.
    pub fn type_ahead(&mut self, text: &str, now: Instant) {
        if self.locked() || self.rows.is_empty() || text.is_empty() || text.chars().any(char::is_control) {
            return;
        }
        let t = text.to_lowercase();
        if self.ahead_at.is_none_or(|a| now.saturating_duration_since(a) > Duration::from_secs(1)) {
            self.ahead.clear();
        }
        self.ahead_at = Some(now);
        let mut chars = t.chars();
        let single = chars.next().filter(|_| chars.next().is_none());
        let cycle = single.is_some_and(|c| !self.ahead.is_empty() && self.ahead.chars().all(|a| a == c));
        let start = match (self.cursor_row, cycle) {
            (Some(r), true) => r + 1,
            (Some(r), false) => r,
            (None, _) => 0,
        };
        if cycle {
            self.ahead = t.clone();
        } else {
            self.ahead.push_str(&t);
        }
        let n = self.rows.len();
        let hit = (0..n).map(|k| (start + k) % n).find(|&r| self.entries[self.rows[r]].name.to_lowercase().starts_with(&self.ahead));
        if let Some(r) = hit {
            let keep = self.ahead.clone();
            self.set_cursor(r);
            self.ahead = keep;
        }
    }
}

/// Natural order: digits compare as numbers (`file2` < `file10`), letters without case; the exact names break a tie.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut s = String::new();
                    while let Some(&c) = it.peek() {
                        if !c.is_ascii_digit() {
                            break;
                        }
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (m, n) = (run(&mut x), run(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let o = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(p), Some(q)) => {
                let (lp, lq) = (p.to_lowercase().next().unwrap_or(p), q.to_lowercase().next().unwrap_or(q));
                if lp != lq {
                    return lp.cmp(&lq);
                }
                x.next();
                y.next();
            }
        }
    }
}

fn group(k: Kind) -> u8 {
    match k {
        Kind::Dir => 0,
        Kind::File => 1,
        Kind::Other | Kind::Unknown => 2,
    }
}

fn opt_cmp<T: Ord>(a: Option<T>, b: Option<T>, desc: bool) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => {
            if desc {
                y.cmp(&x)
            } else {
                x.cmp(&y)
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// The order of the list: folders first always, then files, then what is neither; inside, by the column (the kind column is
/// the lower-case extension, no extension first: no words, so no language), the natural name breaking ties.
fn compare(a: &Entry, b: &Entry, sort: Sort, desc: bool) -> Ordering {
    let (ga, gb) = (group(a.kind), group(b.kind));
    if ga != gb {
        return ga.cmp(&gb);
    }
    let by_name = || natural_cmp(&a.name, &b.name);
    let primary = match sort {
        Sort::Name => {
            let o = by_name();
            return if desc { o.reverse() } else { o };
        }
        Sort::Modified => opt_cmp(a.modified, b.modified, desc),
        Sort::Kind if ga == 1 => {
            let o = a.extension().unwrap_or_default().cmp(&b.extension().unwrap_or_default());
            if desc {
                o.reverse()
            } else {
                o
            }
        }
        Sort::Size if ga == 1 => opt_cmp(a.size, b.size, desc),
        _ => Ordering::Equal,
    };
    primary.then_with(by_name)
}

/// A size as people read it: 1024 base, `B` `KB` `MB` `GB` `TB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v >= 10.0 {
        format!("{} {}", v.round() as u64, UNITS[u])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}
