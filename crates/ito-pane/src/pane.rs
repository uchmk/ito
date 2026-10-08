//! What a view needs from a pane, whichever side of a process boundary the
//! shell is on: [`Terminal`] here, or a session the tsumugi server holds.
//! `show` and `input::feed` work through this, so the same drawing and keys
//! serve both.

use alacritty_terminal::grid::Scroll;

use crate::{CellView, MouseReport, Size, Terminal};

/// The visible screen, copied out at one moment.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Screen {
    pub rows: Vec<Vec<CellView>>,
    /// Column and line of the cursor.
    pub cursor: (usize, usize),
    /// The program asked for application cursor keys (`\e[?1h`).
    pub app_cursor: bool,
    /// The program is on the alternate screen (a full-screen program).
    pub alt_screen: bool,
    /// How the program wants the mouse reported, if at all.
    pub mouse: MouseReport,
}

pub trait Pane {
    /// The grid's new shape; the cell size in pixels is what the PTY is told.
    fn resize(&mut self, size: Size, cell: (u16, u16));
    fn screen(&self) -> Screen;
    /// How many lines back in the scrollback the view is.
    fn scrolled_back(&self) -> usize;
    fn scroll(&self, by: Scroll);
    /// Begin a selection at a cell (`start`), or carry one on to it.
    fn select(&self, cell: (usize, usize), right_half: bool, start: bool);
    fn select_word(&self, cell: (usize, usize));
    fn clear_selection(&self);
    /// The selected text, when the pane has it to hand. A pane across a
    /// process boundary may answer later through its own channel instead.
    fn selection(&self) -> Option<String>;
    /// Bytes for the shell, as typed.
    fn send(&self, bytes: Vec<u8>);
    /// Text for the shell, as a paste (bracketed when the program asked).
    fn paste(&self, text: &str);
    /// Keys go as win32-input-mode records (the other end asked).
    fn win32_input(&self) -> bool;
    /// The program asked for bracketed paste: a paste of several lines is
    /// held as one, not run line by line. Unknown is no.
    fn bracketed_paste(&self) -> bool {
        false
    }
    /// The OSC 8 links on the screen as shown. None known is none.
    fn hyperlinks(&self) -> Vec<crate::Hyperlink> {
        Vec::new()
    }
}

impl Pane for Terminal {
    fn resize(&mut self, size: Size, cell: (u16, u16)) {
        Terminal::resize(self, size, cell)
    }

    fn screen(&self) -> Screen {
        self.with_grid(|t| Screen {
            rows: crate::snapshot(t),
            cursor: crate::cursor_cell(t),
            app_cursor: crate::app_cursor(t),
            alt_screen: crate::alt_screen(t),
            mouse: crate::mouse_report(t),
        })
    }

    fn scrolled_back(&self) -> usize {
        Terminal::scrolled_back(self)
    }

    fn scroll(&self, by: Scroll) {
        Terminal::scroll(self, by)
    }

    fn select(&self, cell: (usize, usize), right_half: bool, start: bool) {
        Terminal::select(self, cell, right_half, start)
    }

    fn select_word(&self, cell: (usize, usize)) {
        Terminal::select_word(self, cell)
    }

    fn clear_selection(&self) {
        Terminal::clear_selection(self)
    }

    fn selection(&self) -> Option<String> {
        Terminal::selection(self)
    }

    fn send(&self, bytes: Vec<u8>) {
        Terminal::send(self, bytes)
    }

    fn paste(&self, text: &str) {
        Terminal::paste(self, text)
    }

    fn win32_input(&self) -> bool {
        Terminal::win32_input(self)
    }

    fn bracketed_paste(&self) -> bool {
        Terminal::bracketed_paste(self)
    }

    fn hyperlinks(&self) -> Vec<crate::Hyperlink> {
        Terminal::hyperlinks(self)
    }
}
