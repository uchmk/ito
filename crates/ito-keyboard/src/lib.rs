//! What a keymap looks like on a keyboard, without drawing anything: the
//! physical boards (PC and Mac, US and JIS), where a key press lands and which
//! presses give a key, which keys a layer binds and what each shows, and which
//! key set this machine has. `ito-prefs` draws it on the Keys page.
//!
//! Resolution models the real conversion (`ito_keys::from_egui` and egui-winit),
//! not the legends on the keys, so a JIS keyboard's Ctrl+`@` is the same key as
//! Ctrl+`[`, as it is for the app.

mod detect;
mod layout;
mod phys;
mod place;
mod view;

pub use detect::{detect, detected, parse_default_keyboard, parse_vconsole, parse_xkb_layout, parse_xorg};
pub use layout::{caps, legend, modifier, types, Block, Board, Cap, Family, Modifier};
pub use phys::Phys;
pub use place::{place, produce, Mods};
pub use view::{split, view, Also, KeyView, Kind, View};

#[cfg(test)]
mod tests;
