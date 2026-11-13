//! An open-file / choose-folder dialog whose listing comes from the app.
//!
//! The folder may be on another machine, so the dialog never touches a disk: it asks (`Picker::take` gives a [`Want::List`]
//! with a [`Ticket`]), the app lists the folder on a worker and answers with [`Picker::set_listing`]. Only the newest ticket
//! is live. The model (this crate without features) has no egui and no language; the window is behind feature `egui`
//! (`Picker::show`).
//!
//! Paths are text, read by [`Style`] (Windows or Posix) whatever machine runs the dialog.

#[cfg(feature = "local")]
mod local;
mod model;
mod path;
#[cfg(feature = "egui")]
mod window;
mod words;

#[cfg(feature = "local")]
pub use local::{check_local, list_local, list_local_with, local_places, local_places_with, local_style, ListError, ListOptions};
pub use model::{
    extension, format_size, natural_cmp, Done, Entry, Filter, InputError, Kind, Listing, Mode, Picker, Place, PlaceKind, Shown, Sort,
    Status, Ticket, Want,
};
pub use path::{display_name, is_component, PathError, Style};
#[cfg(feature = "egui")]
pub use window::Look;
pub use words::Words;

#[cfg(test)]
mod gate_tests;
#[cfg(all(test, feature = "local"))]
mod local_tests;
#[cfg(test)]
mod tests;
#[cfg(all(test, feature = "egui"))]
mod window_tests;
