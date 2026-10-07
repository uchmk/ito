//! The pane itself: a shell on a PTY, the tap that reads what it says on the
//! way past, and the grid `alacritty_terminal` parses it into.

use std::io::{self};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event as PtyEvent, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty::{self, EventedReadWrite};

use crossbeam_channel::{Receiver, Sender};

#[allow(unused_imports)]
use crate::{grid::*, keys::*, log::*, osc::*, shell::*};

/// The grid's shape, which is all `Term` needs to know about the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Size {
    pub cols: usize,
    pub lines: usize,
}

impl Size {
    /// Columns and lines never usefully reach zero, and `Grid` divides by
    /// them, so the floor is one of each.
    pub fn new(cols: usize, lines: usize) -> Self {
        Self { cols: cols.max(1), lines: lines.max(1) }
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// Carries what the terminal wants to say back to the UI thread. The parser
/// runs on the PTY reader thread, so everything arrives over a channel and is
/// read where the rest of the app's channels are read.
#[derive(Clone)]
pub struct Proxy {
    pub(crate) tx: Sender<PtyEvent>,
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
}

impl EventListener for Proxy {
    fn send_event(&self, event: PtyEvent) {
        let _ = self.tx.send(event);
        (self.wake)();
    }
}

/// The PTY with a tap on the bytes coming out of it.
///
/// A shell says where it is with OSC 7, and `alacritty_terminal`'s parser does
/// not carry that one — vte handles the title, the clipboard and the colors,
/// and lets the rest fall on the floor. Rather than replace the event loop to
/// get at it, the PTY is wrapped: the loop reads through here, so the bytes
/// are seen on the way past and the terminal still gets every one of them.
///
/// `Reader = Self` because [`tty::EventedReadWrite::reader`] hands back a
/// reference into `self`, which leaves nowhere to put a wrapper that borrows
/// it. Being its own reader is how the tap gets to keep its state.
pub(crate) struct Tapped {
    inner: tty::Pty,
    /// The pane's character set: read into UTF-8 here, before anything looks.
    charset: crate::Charset,
    cwd: Sender<PathBuf>,
    /// Bytes of an OSC 7 that has begun but not ended, since a read can stop
    /// anywhere — including in the middle of one.
    partial: Vec<u8>,
    /// The PTY log, when `spawn` was given one; see [`PtyLog`].
    log: PtyLog,
    /// Whether the other end has asked for win32-input-mode; see
    /// [`Terminal::win32_input`]. Written here, where the request goes past.
    win32: Arc<AtomicBool>,
    /// The end of the last read, in case the request was cut in two.
    mode_tail: Vec<u8>,
    /// When the shell last wrote anything; see [`Terminal::quiet_for`].
    last_out: Arc<std::sync::Mutex<Option<Instant>>>,
    /// The shell has drawn a prompt and said so (OSC 133); see
    /// [`Terminal::prompt_seen`]. With the end of the last read, as above.
    prompt: Arc<AtomicBool>,
    prompt_tail: Vec<u8>,
    /// When the shell last marked a prompt; see [`Terminal::last_prompt`].
    prompt_at: Arc<Mutex<Option<Instant>>>,
    /// OSC 9 / 99 / 777 notifications, and the end of the last read.
    notices: Sender<String>,
    notice_tail: Vec<u8>,
}

impl io::Read for Tapped {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let inner = &mut self.inner;
        let n = self.charset.read(buf, |raw| inner.reader().read(raw))?;
        log_pty(&self.log, "out", &buf[..n]);
        for path in scan_osc7(&mut self.partial, &buf[..n]) {
            let _ = self.cwd.send(path);
        }
        if let Some(on) = scan_win32_mode(&mut self.mode_tail, &buf[..n]) {
            self.win32.store(on, Ordering::Relaxed);
        }
        if n > 0 {
            *self.last_out.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
        if scan_prompt_mark(&mut self.prompt_tail, &buf[..n]) {
            self.prompt.store(true, Ordering::Relaxed);
            *self.prompt_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
        for text in scan_notices(&mut self.notice_tail, &buf[..n]) {
            let _ = self.notices.send(text);
        }
        Ok(n)
    }
}

impl tty::EventedReadWrite for Tapped {
    type Reader = Self;
    type Writer = <tty::Pty as tty::EventedReadWrite>::Writer;

    unsafe fn register(
        &mut self,
        poller: &Arc<polling::Poller>,
        interest: polling::Event,
        mode: polling::PollMode,
    ) -> io::Result<()> {
        unsafe { self.inner.register(poller, interest, mode) }
    }

    fn reregister(
        &mut self,
        poller: &Arc<polling::Poller>,
        interest: polling::Event,
        mode: polling::PollMode,
    ) -> io::Result<()> {
        self.inner.reregister(poller, interest, mode)
    }

    fn deregister(&mut self, poller: &Arc<polling::Poller>) -> io::Result<()> {
        self.inner.deregister(poller)
    }

    fn reader(&mut self) -> &mut Self::Reader {
        self
    }

    fn writer(&mut self) -> &mut Self::Writer {
        self.inner.writer()
    }
}

impl tty::EventedPty for Tapped {
    fn next_child_event(&mut self) -> Option<tty::ChildEvent> {
        self.inner.next_child_event()
    }
}

impl alacritty_terminal::event::OnResize for Tapped {
    fn on_resize(&mut self, window_size: WindowSize) {
        self.inner.on_resize(window_size)
    }
}

/// The bytes to send for a paste of `text`.
///
/// `\x1b[200~` and `\x1b[201~` are the markers xterm defined and everything
/// since has followed. They go on only when the program asked for them: a shell
/// that did not ask would show them as `[200~` and then run the paste anyway,
/// which is worse than not bracketing at all.
pub(crate) fn bracket(text: &str, wanted: bool) -> Vec<u8> {
    // Nothing to paste needs no markers; a bare pair would reach a shell that
    // does not strip them as `[200~[201~` on the command line.
    if !wanted || text.is_empty() {
        return text.as_bytes().to_vec();
    }
    let mut out = Vec::with_capacity(text.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}

pub struct Terminal {
    term: Arc<FairMutex<Term<Proxy>>>,
    sender: EventLoopSender,
    rx: Receiver<PtyEvent>,
    /// What the shell has set the title to, when it has.
    pub title: String,
    /// The shell is gone; the pane says so rather than pretending.
    pub exited: bool,
    /// The pane's foreground and background as drawn, for a program that asks
    /// (OSC 10 / 11). Set from the theme each frame; none until then.
    colors: Option<([u8; 3], [u8; 3])>,
    size: Size,
    /// Where the shell was last told to go, so it is not told twice.
    followed: Option<PathBuf>,
    /// How to quote a word for the shell that was actually started, decided
    /// once at `spawn` because that is where the program's name is known.
    quoting: Quoting,
    /// Whether the other end asked for keys as win32-input-mode records.
    win32: Arc<AtomicBool>,
    cwd_rx: Receiver<PathBuf>,
    /// The last match, start and end, so the next search carries on past it
    /// in whichever direction it goes.
    found: Option<(Point, Point)>,
    /// Where the shell says it is, when it says so at all. A shell that does
    /// not send OSC 7 leaves this `None` for ever, which is why it only ever
    /// suppresses work rather than driving any.
    pub shell_cwd: Option<PathBuf>,
    /// The PTY log, when `spawn` was given one; see [`PtyLog`].
    log: PtyLog,
    /// The shell's process, to ask whether it has started anything. `None`
    /// where the PTY did not say, which reads as "nothing running".
    pub(crate) shell_pid: Option<u32>,
    /// When the shell last wrote, shared with the reader; see [`Terminal::quiet_for`].
    last_out: Arc<std::sync::Mutex<Option<Instant>>>,
    /// Whether the shell has marked a prompt (OSC 133), shared with the reader.
    prompt: Arc<AtomicBool>,
    prompt_at: Arc<Mutex<Option<Instant>>>,
    notices: Receiver<String>,
    /// When a key or a paste was last sent; see [`Terminal::last_input`].
    last_input: Mutex<Option<Instant>>,
    /// The character set the program reads and writes; UTF-8 by default.
    charset: crate::Charset,
    /// The reader thread. It hands the PTY back when it ends, and dropping
    /// that is what ends the shell -- so [`Drop`] waits for it.
    io: Option<std::thread::JoinHandle<(EventLoop<Tapped, Proxy>, alacritty_terminal::event_loop::State)>>,
}

/// What the pane tells the shell about itself. Off Windows a program learns
/// what the terminal can do from `TERM`, and filer's own may be anything: a
/// desktop launcher passes none, and with none bash's readline takes the
/// terminal for a dumb one and scrolls a long line sideways inside one row,
/// so a path `<A-t>` typed showed only its end (CI, v0.75.22). The grid is
/// alacritty's, which speaks xterm, in 24-bit colour. Windows programs do not
/// read `TERM` from ConPTY, and some (Git for Windows) change course when it
/// is set, so nothing is added there.
pub(crate) fn pane_env() -> std::collections::HashMap<String, String> {
    if cfg!(windows) {
        return Default::default();
    }
    [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")].into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect()
}

/// Held while a PTY is being made, so that the console hosts one spawn sees
/// appear are its own and not another's; see [`new_pty`].
pub(crate) static SPAWNING: Mutex<()> = Mutex::new(());

/// `tty::new`, but a shell that fails to start does not leave its
/// pseudoconsole running on Windows; see `sys::end_new_consoles`. The caller
/// holds [`SPAWNING`].
pub(crate) fn new_pty(options: &tty::Options, window: WindowSize) -> io::Result<tty::Pty> {
    let consoles = crate::sys::consoles();
    tty::new(options, window, 0).inspect_err(|_| crate::sys::end_new_consoles(&consoles))
}

impl Terminal {
    /// Start a shell in `cwd`. The cell size is what the PTY is told, so a
    /// program asking for pixels (an image protocol, say) gets the truth.
    /// `log` is the file to record the PTY's traffic in, when there is one
    /// (filer's `FILER_PTY_LOG`); see [`PtyLog`].
    pub fn spawn(
        cwd: &Path,
        size: Size,
        cell: (u16, u16),
        shell: Option<(String, Vec<String>)>,
        log: Option<&Path>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Self> {
        Self::spawn_with_env(cwd, size, cell, shell, log, Vec::new(), wake)
    }

    /// [`spawn`](Self::spawn), with variables added to the shell's
    /// environment (tsumugi's `TSUMUGI_SESSION`, which `tsumugi notify` reads).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_env(
        cwd: &Path,
        size: Size,
        cell: (u16, u16),
        shell: Option<(String, Vec<String>)>,
        log: Option<&Path>,
        env: Vec<(String, String)>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let mut vars = pane_env();
        vars.extend(env);
        let quoting = Quoting::for_shell(shell.as_ref().map(|(p, _)| p.as_str()));
        let options = tty::Options {
            // `None` is the platform default, which on Windows is
            // `powershell` -- Windows PowerShell 5.1. `default_shell` asks for
            // `pwsh` before it comes to that when 7 is installed. They read
            // different profiles, so a shell hook set up for one is simply not
            // there in the other; `[term] shell` is how you say which.
            shell: shell.map(|(program, args)| tty::Shell::new(program, args)),
            working_directory: Some(cwd.to_path_buf()),
            drain_on_exit: false,
            env: vars,
            #[cfg(target_os = "windows")]
            escape_args: true,
        };
        let window = window_size(size, cell);
        let pty = {
            let _one_at_a_time = SPAWNING.lock().unwrap_or_else(|e| e.into_inner());
            new_pty(&options, window)?
        };
        #[cfg(windows)]
        let shell_pid = pty.child_watcher().pid().map(|p| p.get());
        #[cfg(unix)]
        let shell_pid = Some(pty.child().id());
        let (cwd_tx, cwd_rx) = crossbeam_channel::unbounded();
        let log = open_pty_log(log);
        let win32 = Arc::new(AtomicBool::new(false));
        let last_out = Arc::new(std::sync::Mutex::new(None));
        let prompt = Arc::new(AtomicBool::new(false));
        let prompt_at = Arc::new(Mutex::new(None));
        let (notice_tx, notices) = crossbeam_channel::unbounded();
        let charset = crate::Charset::default();
        let pty = Tapped {
            inner: pty,
            charset: charset.clone(),
            cwd: cwd_tx,
            partial: Vec::new(),
            log: log.clone(),
            win32: win32.clone(),
            mode_tail: Vec::new(),
            last_out: last_out.clone(),
            prompt: prompt.clone(),
            prompt_tail: Vec::new(),
            prompt_at: prompt_at.clone(),
            notices: notice_tx,
            notice_tail: Vec::new(),
        };

        let (tx, rx) = crossbeam_channel::unbounded();
        let proxy = Proxy { tx, wake: Arc::new(wake) };
        let term = Term::new(Config::default(), &size, proxy.clone());
        let term = Arc::new(FairMutex::new(term));

        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)?;
        let sender = event_loop.channel();
        // The reader thread owns the PTY from here; it parses into `term` and
        // ends when the shell does, or when `Drop` asks.
        let io = Some(event_loop.spawn());

        Ok(Self {
            term,
            sender,
            rx,
            title: String::new(),
            exited: false,
            colors: None,
            size,
            followed: Some(cwd.to_path_buf()),
            quoting,
            cwd_rx,
            found: None,
            shell_cwd: None,
            log,
            shell_pid,
            win32,
            last_out,
            prompt,
            prompt_at,
            notices,
            last_input: Mutex::new(None),
            charset,
            io,
        })
    }

    /// Whether keys should go as win32-input-mode records: the other end
    /// (ConPTY, on Windows) asked for them. Never true anywhere else.
    pub fn win32_input(&self) -> bool {
        self.win32.load(Ordering::Relaxed)
    }

    /// Whether the shell is running something -- lazygit, an editor, a build
    /// -- rather than sitting at its prompt. Ending the shell ends that too,
    /// so `<C-S-t>` asks first when this is true (Q21).
    ///
    /// "Something" is any child process of the shell. A shell that keeps a
    /// helper of its own alive would read as busy and be asked about when it
    /// need not be; that errs on the side of the question, which costs a key.
    pub fn busy(&self) -> bool {
        !self.exited && self.shell_pid.is_some_and(|pid| !crate::sys::children(pid).is_empty())
    }

    /// What colors a program asking for the foreground and background gets.
    pub fn set_colors(&mut self, fg: [u8; 3], bg: [u8; 3]) {
        self.colors = Some((fg, bg));
    }

    /// Take everything the shell has said since the last frame. Returns the
    /// text it asked to put on the clipboard, which only the UI thread can do.
    pub fn drain(&mut self) -> Vec<String> {
        let mut clipboard = Vec::new();
        // Where the shell says it is. Only the last one matters.
        while let Ok(p) = self.cwd_rx.try_recv() {
            self.shell_cwd = Some(p);
        }
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                PtyEvent::Title(t) => self.title = t,
                PtyEvent::ResetTitle => self.title.clear(),
                // OSC 10 / 11 (and 12): lipgloss and bubbletea ask for the
                // colors to pick a light or dark style, and wait, then guess,
                // when nothing answers (gh-dash, #265). 256 is the foreground,
                // 257 the background, 258 the cursor; palette slots are left.
                PtyEvent::ColorRequest(index @ 256..=258, format) => {
                    if let Some((fg, bg)) = self.colors {
                        let [r, g, b] = if index == 257 { bg } else { fg };
                        let text = format(alacritty_terminal::vte::ansi::Rgb { r, g, b });
                        self.send_as(text.into_bytes(), "in reply");
                    }
                }
                PtyEvent::ClipboardStore(_, text) => clipboard.push(text),
                // A program answering a query writes back through the same
                // pipe it would if the user had typed it.
                PtyEvent::PtyWrite(text) => {
                    let text = answer_win32_query(text, self.win32_input());
                    self.send_as(text.into_bytes(), "in reply")
                }
                PtyEvent::Exit | PtyEvent::ChildExit(_) => self.exited = true,
                _ => {}
            }
        }
        clipboard
    }

    pub fn resize(&mut self, size: Size, cell: (u16, u16)) {
        if size.cols == self.size.cols && size.lines == self.size.lines {
            return;
        }
        self.size = size;
        // Both halves have to hear about it: the grid reflows, and the shell
        // is signalled so a full-screen program repaints itself.
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(window_size(size, cell)));
    }

    pub fn send(&self, bytes: Vec<u8>) {
        self.send_as(bytes, "in key");
    }

    /// `send`, labelled for the PTY log by where the bytes came from.
    fn send_as(&self, bytes: Vec<u8>, origin: &str) {
        let bytes = self.charset.encode(bytes);
        log_pty(&self.log, origin, &bytes);
        if origin != "in reply" {
            *self.last_input.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
        // Typing is an answer to what is on screen, so the view comes back to
        // the bottom — every terminal does this, and a key that seemed to do
        // nothing because the view was in the scrollback is a bad surprise.
        self.term.lock().scroll_display(Scroll::Bottom);
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Move the view through the scrollback. Lines are positive for older.
    pub fn scroll(&self, by: Scroll) {
        self.term.lock().scroll_display(by);
    }

    /// Whether the view is somewhere above the bottom, which the pane says so
    /// the scrollback is never a silent place to be lost in.
    pub fn scrolled_back(&self) -> usize {
        self.term.lock().grid().display_offset()
    }

    /// How far back the view is, against how far back it could go: the
    /// scrollback lines above the screen.
    pub fn scrollback(&self) -> (usize, usize) {
        let term = self.term.lock();
        (term.grid().display_offset(), term.grid().history_size())
    }

    /// Begin a selection at a cell, or carry one on to it. `start` is the
    /// press; everything after is the drag.
    pub fn select(&self, cell: (usize, usize), right_half: bool, start: bool) {
        select_at(&mut self.term.lock(), cell, right_half, start);
    }

    /// Select the word under a cell — what a double-click means everywhere.
    pub fn select_word(&self, cell: (usize, usize)) {
        let point = self.point(cell);
        let mut term = self.term.lock();
        term.selection = Some(Selection::new(SelectionType::Semantic, point, Side::Left));
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    /// The selected text, if any of it is.
    pub fn selection(&self) -> Option<String> {
        self.term.lock().selection_to_string().filter(|s| !s.is_empty())
    }

    /// Find `needle` from the top of the view, and put the match on screen.
    /// Returns whether anything matched.
    /// `Some(true)` when the match was found by running off the end and
    /// starting again; `None` when there is none.
    pub fn search(&mut self, needle: &str, back: bool) -> Option<bool> {
        let mut term = self.term.lock();
        search_in(&mut term, &mut self.found, needle, back)
    }

    /// The lines holding `needle` anywhere in the buffer (see
    /// [`find_lines`](crate::find_lines)).
    pub fn find_lines(&self, needle: &str, max: usize) -> Vec<(i32, usize, String)> {
        find_lines(&self.term.lock(), needle, max)
    }

    /// Read and write the program's bytes as `name` (one of
    /// [`CHARSETS`](crate::CHARSETS)); `false` for a name not known.
    pub fn set_charset(&self, name: &str) -> bool {
        self.charset.set(name)
    }

    /// The character set in use: `UTF-8`, `Shift_JIS`.
    pub fn charset(&self) -> &'static str {
        self.charset.name()
    }

    /// The whole buffer as text, scrollback and screen (see
    /// [`all_text`](crate::all_text)).
    pub fn all_text(&self) -> String {
        all_text(&self.term.lock())
    }

    /// Put a found line on screen, its match selected.
    pub fn reveal(&mut self, line: i32, col: usize, len: usize) {
        self.found = None;
        reveal(&mut self.term.lock(), line, col, len);
    }

    /// Keep `lines` of scrollback (alacritty's default is 10 000).
    pub fn set_scrollback(&mut self, lines: usize) {
        let config = alacritty_terminal::term::Config { scrolling_history: lines, ..Default::default() };
        self.term.lock().set_options(config);
    }

    /// Forget where a search got to, so the next one starts from the view.
    pub fn end_search(&mut self) {
        self.found = None;
    }

    /// A cell of the visible grid as a point in the whole buffer, which is
    /// where the scrollback lives above line zero.
    fn point(&self, cell: (usize, usize)) -> Point {
        point_at(&self.term.lock(), cell)
    }

    /// Type text in, as a paste rather than as keys.
    ///
    /// Wrapped in the bracketed-paste markers when the program on the other end
    /// asked for them, which bash, zsh, fish, PSReadLine and vim all do. The
    /// difference is not cosmetic: inside the brackets a line editor puts the
    /// text in the buffer and leaves it there, so a clipboard holding three
    /// lines ends up as three lines waiting to be read rather than two commands
    /// already run. Without the brackets a newline *is* Enter and there is no
    /// way to send one that is not.
    pub fn paste(&self, text: &str) {
        // Carriage returns are what a terminal calls Enter; a pasted `\n`
        // that stays a newline confuses a line editor.
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        self.send_as(bracket(&text, self.bracketed_paste()), "in paste");
    }

    /// Whether the program on the other end asked for bracketed paste.
    fn bracketed_paste(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        self.term.lock().mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Follow the pane into `cwd`, by typing the `cd` a person would.
    ///
    /// Typing is the only way in: a shell takes no other instruction. That
    /// makes it a line of input like any other — harmless at a prompt, a
    /// nuisance in the middle of a command — so it is sent as rarely as it
    /// can be. Twice over: not when the pane has not moved, and not when the
    /// shell has already said (through OSC 7) that it is there. A shell that
    /// reports its directory therefore never hears a `cd` it does not need,
    /// including the one that would otherwise follow its own.
    pub fn follow(&mut self, cwd: &Path) {
        if self.followed.as_deref() == Some(cwd) {
            return;
        }
        self.followed = Some(cwd.to_path_buf());
        if self.shell_cwd.as_deref() == Some(cwd) {
            return;
        }
        let quoted = quote(&cwd.to_string_lossy(), self.quoting);
        self.send(format!("cd {quoted}\r").into_bytes());
    }

    /// How a word has to be quoted for the shell in this pane.
    pub fn quoting(&self) -> Quoting {
        self.quoting
    }

    /// Run the terminal's own locked grid through `f`. Locking is the caller's
    /// business to keep short: the reader thread wants it back.
    pub fn with_grid<R>(&self, f: impl FnOnce(&Term<Proxy>) -> R) -> R {
        f(&self.term.lock())
    }

    /// The characters on screen, one line per row with the blanks at the end
    /// cut, as the view shows them (scrolled back or not). What a check wants
    /// to know about a full-screen program is what it wrote, and a picture
    /// made that a matter of eyes (#229).
    pub fn screen_text(&self) -> String {
        use alacritty_terminal::term::cell::Flags;
        let rows = self.with_grid(snapshot);
        let lines: Vec<String> = rows
            .iter()
            .map(|row| {
                let text: String = row.iter().filter(|c| !c.flags.contains(Flags::WIDE_CHAR_SPACER)).map(|c| c.c).collect();
                text.trim_end().to_owned()
            })
            .collect();
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    /// Whether the shell has put anything you could read on the screen yet --
    /// a banner or a prompt. ConPTY writes its own setup sequences the moment
    /// the pane opens, so "some bytes arrived" says nothing about whether the
    /// shell is ready to be typed at; a character on screen does.
    /// Whether the shell has written something and then nothing more for
    /// `quiet`: the prompt is up and it is waiting (Q39). The first output
    /// alone is not that -- pwsh prints its banner, then spends seconds on
    /// its profile, and a line typed in between can be dropped (#126).
    /// Whether the shell has said its prompt is up (OSC 133), which beats any
    /// guess from how long it has been quiet.
    pub fn prompt_seen(&self) -> bool {
        self.prompt.load(Ordering::Relaxed)
    }

    /// Where the shell is: what it said last (OSC 7), or else, where the
    /// system can tell (Linux, macOS), the folder its process is in.
    pub fn current_dir(&self) -> Option<PathBuf> {
        self.shell_cwd.clone().or_else(|| self.shell_pid.and_then(crate::sys::process_cwd))
    }

    /// When the shell last marked its prompt (OSC 133 `A` or `B`).
    pub fn last_prompt(&self) -> Option<Instant> {
        *self.prompt_at.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// When the shell last wrote anything.
    pub fn last_output(&self) -> Option<Instant> {
        *self.last_out.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// When a key or a paste last went to the shell (not the terminal's own
    /// replies to a program's questions).
    pub fn last_input(&self) -> Option<Instant> {
        *self.last_input.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The notifications (OSC 9 / 99 / 777) the shell's programs sent since
    /// the last call.
    pub fn take_notices(&self) -> Vec<String> {
        self.notices.try_iter().collect()
    }

    pub fn quiet_for(&self, quiet: Duration) -> bool {
        self.last_out.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|at| at.elapsed() >= quiet)
    }

    pub fn has_drawn(&self) -> bool {
        self.with_grid(|t| snapshot(t).iter().flatten().any(|c| !matches!(c.c, ' ' | '\0')))
    }

}

impl Drop for Terminal {
    /// End the shell, then ask the reader thread to stop and wait a moment
    /// for it, so the PTY it hands back is closed here. A test, or filer closed
    /// with the pane open, used to exit before any of that happened, and on
    /// Windows the shell and its OpenConsole lived on with no parent: one pair
    /// per test run on the x64 machine (#180).
    ///
    /// On Windows closing the pseudoconsole only asks the shell to go, and
    /// pwsh takes its time or does not go at all, so the shell and whatever
    /// runs under it are ended outright first -- which is what `<C-S-t>`
    /// promises. On Unix dropping the PTY hangs up and reaps the shell.
    /// The wait is bounded because this runs on the UI thread.
    ///
    /// On macOS the PTY's child is not the shell but `/usr/bin/login`, which
    /// alacritty starts it through. Dropping the `Pty` sends that child
    /// `SIGHUP` and then *waits* for it; `login` waits in turn for the shell,
    /// and the shell only hears the hangup when the PTY's master side closes
    /// -- after that wait. The first macOS test run hung there twice, and in
    /// filer it is the window freezing as the pane closes. So the shell under
    /// `login` is hung up first, as a terminal closing would.
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(pid) = self.shell_pid.filter(|_| !self.exited) {
            crate::sys::end_tree(pid);
        }
        #[cfg(target_os = "macos")]
        if let Some(pid) = self.shell_pid.filter(|_| !self.exited) {
            crate::sys::hang_up_children(pid);
        }
        #[cfg(unix)]
        if let Some(pid) = self.shell_pid.filter(|_| !self.exited) {
            crate::sys::end_shell(pid);
        }
        let _ = self.sender.send(Msg::Shutdown);
        let Some(io) = self.io.take() else { return };
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        while !io.is_finished() && Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if io.is_finished() {
            drop(io.join());
        }
    }
}

pub(crate) fn window_size(size: Size, cell: (u16, u16)) -> WindowSize {
    WindowSize {
        num_lines: size.lines as u16,
        num_cols: size.cols as u16,
        cell_width: cell.0.max(1),
        cell_height: cell.1.max(1),
    }
}
