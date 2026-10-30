//! macOS and Linux: not yet (the menu bar item and StatusNotifierItem come later).

use std::io;
use std::sync::mpsc::{self, Receiver};

use crate::{Icon, Item, State};

pub struct Tray {
    rx: Receiver<crate::Event>,
}

impl Tray {
    pub fn new(_: Icon, _: &str, _: Box<dyn Fn() + Send>) -> io::Result<Tray> {
        let _ = mpsc::channel::<crate::Event>().1;
        Err(io::Error::new(io::ErrorKind::Unsupported, "a tray icon is not available on this system yet"))
    }

    pub fn set_state(&self, _: Option<State>) {}
    pub fn set_tip(&self, _: &str) {}
    pub fn set_menu(&self, _: Vec<Item>) {}

    pub fn events(&self) -> &Receiver<crate::Event> {
        &self.rx
    }
}
