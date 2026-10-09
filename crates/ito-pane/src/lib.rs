//! A terminal pane, shared by filer and tsumugi.
//!
//! `alacritty_terminal` does the parts that are a terminal emulator: it owns
//! the PTY -- a real one on Unix, ConPTY on Windows -- reads it on a thread of
//! its own, and parses the escape sequences into a grid. This crate adds what
//! a pane in an app needs on top: starting the shell, turning key presses into
//! the bytes a shell expects (win32-input-mode records for ConPTY), reading
//! what the shell says outside the grid (OSC 7, OSC 133), and reading the grid
//! back out to draw it.
//!
//! The core knows nothing of egui. Drawing the pane with egui is the `egui`
//! feature ([`show`]), and choosing wgpu's backends is the `wgpu` feature
//! ([`gpu`]). What differs by OS is in `sys`.
//! The plan this crate is being built by is `docs/pane-extraction.md`.

mod cast;
mod charset;
mod grid;
mod image;
mod keys;
pub mod kitty;
mod link;
mod log;
mod osc;
mod pane;
mod shell;
mod sys;
mod terminal;
mod util;
#[cfg(feature = "egui")]
mod view;
#[cfg(feature = "egui")]
mod liga;
#[cfg(feature = "egui")]
pub mod input;
#[cfg(feature = "wgpu")]
pub mod gpu;

#[doc(hidden)]
pub mod testing;

#[cfg(test)]
mod tests;

/// The emulator underneath, for the types the API hands out (`Term`, `Scroll`,
/// cell flags and colors), so a user needs no `alacritty_terminal` of its own.
pub use alacritty_terminal;

pub use charset::{Charset, CHARSETS};
pub use grid::*;
pub use image::{Picture, Placement, IMAGE_LINK};
pub use keys::*;
pub use link::{hints, link_at, Hint, Link};
pub use log::escape_bytes;
pub use pane::{Pane, Screen};
pub use shell::*;
pub use sys::{children, descendants, listening_ports, process_table, restrict_dll_search, stem, Proc};
pub use terminal::*;
#[cfg(feature = "egui")]
pub use view::*;
#[cfg(feature = "egui")]
pub use liga::Shaper;
