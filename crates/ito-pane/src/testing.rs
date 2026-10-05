//! A terminal built without a PTY, for tests here and in the apps that use
//! the pane (filer's scrollback tests). Not part of the API proper.

use std::sync::Arc;

use alacritty_terminal::term::{Config, Term};

use crate::*;

/// A terminal with `lines` rows of screen and room to scroll back.
pub fn term(cols: usize, lines: usize) -> Term<Proxy> {
    let (tx, _rx) = crossbeam_channel::unbounded();
    let proxy = Proxy { tx, wake: Arc::new(|| {}) };
    let cfg = Config { scrolling_history: 200, ..Default::default() };
    Term::new(cfg, &Size::new(cols, lines), proxy)
}

/// Feed `text` through the parser, as the PTY reader thread would.
pub fn feed(t: &mut Term<Proxy>, text: &str) {
    let mut parser = alacritty_terminal::vte::ansi::Processor::<
        alacritty_terminal::vte::ansi::StdSyncHandler,
    >::default();
    for b in text.as_bytes() {
        parser.advance(t, &[*b]);
    }
}
