//! Native open / save file dialogs (Windows: the common item dialog; macOS: NSOpenPanel; Linux: the desktop portal), a thin
//! wrapper over `rfd` so an app does not add it itself.
//!
//! The calls block until the person answers, so run them off the UI thread: [`spawn`] does that and hands back a channel to
//! poll from the egui update loop.
//!
//! ```no_run
//! use ito_dialog::{Dialog, Filter};
//! let d = Dialog::new().title("Import").filter(Filter::new("TOML", &["toml"]));
//! let rx = ito_dialog::spawn(move || d.open_file());
//! // each frame: if let Ok(picked) = rx.try_recv() { … }
//! ```
//!
//! "Cancelled" is `None` (one file) or an empty list (several), never an error.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

/// A named set of extensions (no dots), e.g. `Filter::new("Images", &["png", "jpg"])`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub name: String,
    pub extensions: Vec<String>,
}

impl Filter {
    pub fn new(name: &str, extensions: &[&str]) -> Self {
        Self { name: name.to_owned(), extensions: extensions.iter().map(|e| e.trim_start_matches('.').to_owned()).collect() }
    }
}

/// What to show. Build with [`Dialog::new`] and the setters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dialog {
    pub title: Option<String>,
    /// The folder the dialog starts in.
    pub directory: Option<PathBuf>,
    /// The suggested name (save) or the preselected one (open).
    pub file_name: Option<String>,
    pub filters: Vec<Filter>,
}

impl Dialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: &str) -> Self {
        self.title = Some(title.to_owned());
        self
    }

    pub fn directory(mut self, dir: impl Into<PathBuf>) -> Self {
        self.directory = Some(dir.into());
        self
    }

    pub fn file_name(mut self, name: &str) -> Self {
        self.file_name = Some(name.to_owned());
        self
    }

    pub fn filter(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    fn build(&self) -> rfd::FileDialog {
        let mut d = rfd::FileDialog::new();
        if let Some(t) = &self.title {
            d = d.set_title(t);
        }
        if let Some(dir) = &self.directory {
            d = d.set_directory(dir);
        }
        if let Some(n) = &self.file_name {
            d = d.set_file_name(n);
        }
        for f in &self.filters {
            d = d.add_filter(&f.name, &f.extensions);
        }
        d
    }

    /// Pick one existing file. Blocks.
    pub fn open_file(&self) -> Option<PathBuf> {
        self.build().pick_file()
    }

    /// Pick several existing files; empty when cancelled. Blocks.
    pub fn open_files(&self) -> Vec<PathBuf> {
        self.build().pick_files().unwrap_or_default()
    }

    /// Pick an existing folder. Blocks.
    pub fn open_folder(&self) -> Option<PathBuf> {
        self.build().pick_folder()
    }

    /// Choose where to save (the person may type a new name). Blocks.
    pub fn save_file(&self) -> Option<PathBuf> {
        self.build().save_file()
    }
}

/// Run a dialog call on its own thread; the answer arrives on the channel (poll it with `try_recv` each frame).
pub fn spawn<T: Send + 'static>(call: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(call());
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_drop_the_leading_dot() {
        assert_eq!(Filter::new("Text", &[".txt", "md"]).extensions, ["txt", "md"]);
    }

    #[test]
    fn setters_fill_the_request() {
        let d = Dialog::new().title("t").directory("/x").file_name("a.toml").filter(Filter::new("T", &["toml"]));
        assert_eq!((d.title.as_deref(), d.file_name.as_deref(), d.filters.len()), (Some("t"), Some("a.toml"), 1));
        assert_eq!(d.directory.as_deref(), Some(std::path::Path::new("/x")));
    }

    #[test]
    fn spawn_hands_the_answer_back() {
        assert_eq!(spawn(|| 7).recv().unwrap(), 7);
    }
}
