//! `--keys "2<A-S-Right><Shot:cpu>"`: keys an app presses itself once it has
//! started, so a test session can drive it without reaching into the window
//! from outside.
//!
//! The keys go in as the `egui::Event`s a keyboard would have produced,
//! through `raw_input_hook`, so they take the same road as a real press.
//! Which events a key becomes is read back out of [`ito_keys::from_egui`]
//! rather than written down a second time.
//!
//! The script's words are the same in every uchmk app (see [`Step`]); an app
//! adds its own readings and screenshot crops through a [`Dialect`]. Two
//! ways to play it: [`Runner`] does the pacing itself (yagura), or the app
//! keeps its own frame loop and takes the [`Press`] each step becomes (kura,
//! which waits for its panes to settle). The reports a run leaves in the
//! app's `<APP>_KEYS_DONE` file ([`stalled_report`], [`quit_report`],
//! [`closed_report`], [`refused_report`], [`install_panic_report`]) are
//! shared too; a [`Runner`] says how many steps are left for them
//! ([`Runner::left`]) and watches for a stall itself ([`Runner::watch`]).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ito_keys::{self as keys, Code, Key};

/// The pause after each step, so the frame that follows a key has drawn
/// before the next key goes in.
const GAP: Duration = Duration::from_millis(60);
/// Between the frames of a click or a drag.
const FRAME: Duration = Duration::from_millis(20);

/// One step of a script: a key to press, or a pause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key(Key),
    /// `<Wait:500>`: this long after the key before it went in, before the
    /// next one does. A shell in the pane runs at its own pace, which the app
    /// cannot see, so "settled" says nothing about it; every real-machine run
    /// that drove the pane filled the gap with harmless keys instead and
    /// guessed how long they took (#93, #119 and five more).
    Wait(Duration),
    /// `<Now>`: the key after it goes in on the next frame, without waiting
    /// for what the key before started to settle. For the rows that measure
    /// something halfway -- a trash still running (12.14), two keys inside the
    /// preview's 40 ms debounce (47.2) -- which the settled wait otherwise
    /// waits out (Q41).
    Now,
    /// `<Shot:name@preview>` crops it to the preview pane (the name keeps the
    /// suffix). `<Shot:name>`: the window as it is now, saved as `name.png` beside the
    /// done file file, before the next key goes in. A check that
    /// compares the screen between two keys started the app once per picture
    /// and relied on the windows coming out the same size (Q42, #154).
    Shot(String),
    /// `<State:name>`: what the done file would say now, written to
    /// `name.txt` beside it. That file is written when the script ends, so a
    /// script ending in `q` reports `overlay: none`, and reading a state
    /// halfway meant leaving the `q` off and stopping the app from outside (#230).
    State(String),
    /// `<Quit>`: end the app whatever is open, as the window's close button
    /// would. `q` is a key like any other, and the compare view, the pane
    /// and a prompt each take it for something else, so a script ending in
    /// `q` there never ended (#236).
    Quit,
    /// `<Tag:name>` for a reading the app defines in its [`Dialect`] (the app's
    /// `<PaneText:name>` and `<PreviewText:name>`): the app writes what it
    /// names to a file beside the done file.
    Read {
        tag: String,
        name: String,
    },
    /// `<Paste>` (and `<C-v>`): the clipboard as it is when the key goes in,
    /// as the paste event the platform makes of Ctrl+V. A real keyboard never
    /// sends the press, so a text field hears nothing from one (#260).
    Paste,
    /// `<Paste:text>`: that text as the paste event, with `\n`, `\t`, `\e`, `\\` and `\xNN`
    /// for what a control byte or the closing `>` (`\x3e`) cannot be written as. For a program that reads a bracketed
    /// paste off the pane (49.6), where the clipboard would need setting from outside first.
    PasteText(String),
    /// `<Click:0.5,0.4>` and `<RClick:…>`: the left or right button at that
    /// place in the window, as fractions of its width and height. The right-click
    /// and wheel rows were each driven by a hundred lines of `SendInput`
    /// written afresh in every real-machine run (#260).
    /// `C-` / `A-` / `S-` before the place holds that key (`<Click:C-0.5,0.4>`).
    Click {
        right: bool,
        mods: u8,
        at: At,
    },
    /// `<Hover:0.5,0.4>`: the pointer moved there with no button, so a hover
    /// style or a cursor shape can be read from `<State:>`.
    Hover {
        at: At,
    },
    /// `<Wheel:-3@0.5,0.4>`: this many lines of wheel there (negative is down).
    /// A fraction is a part of a notch (`<Wheel:0.25@…>`), and `C-` / `A-` / `S-`
    /// before the number holds that modifier (`<Wheel:C-1@…>`, #280).
    Wheel {
        milli: i32,
        mods: u8,
        at: At,
    },
    /// `<Drag:0.2,0.3-0.6,0.3>`: the left button down at the first place, moved
    /// to the second and let go there (#280; `SendInput` could not move the cursor).
    Drag {
        from: At,
        to: At,
    },
    /// `<Key:Tab>`, `<Key:C-A>`: that egui key pressed and let go, with no
    /// text. A key written as itself (`<Tab>`, `a`) arrives the way a keyboard
    /// sends it, which for a character is the text alone; this one shows
    /// whether a key event on its own reaches what was meant (#344). The name
    /// is egui's (`Space`, `A`, `ArrowDown`); `C-` / `A-` / `S-` before it hold
    /// that modifier.
    Raw {
        key: egui::Key,
        mods: u8,
    },
}

/// `Step::Wheel`'s modifiers: these bits.
pub const MOD_CTRL: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_SHIFT: u8 = 4;

/// A place in the window, in thousandths of its width and height.
pub type At = (u16, u16);

/// What a step becomes in the frame loop.
#[derive(Clone, Debug, PartialEq)]
pub enum Press {
    Events(Vec<egui::Event>),
    Wait(Duration),
    Now,
    Shot(String),
    State(String),
    Quit,
    Read {
        tag: String,
        name: String,
    },
    Paste,
    PasteText(String),
    /// Pointer steps wait for the window's size, known only in the frame loop.
    Click {
        right: bool,
        mods: u8,
        at: At,
    },
    Hover {
        at: At,
    },
    Wheel {
        milli: i32,
        mods: u8,
        at: At,
    },
    Drag {
        from: At,
        to: At,
    },
}

/// A step as the frame loop takes it; `None` for a key no keyboard can type.
pub fn press(step: &Step) -> Option<Press> {
    match step {
        Step::Key(k) if k.code == Code::Char('v') && k.ctrl && !k.alt && !k.sup => Some(Press::Paste),
        Step::Key(k) => clipboard_chord(k).or_else(|| events(k)).map(Press::Events),
        Step::Paste => Some(Press::Paste),
        Step::PasteText(t) => Some(Press::PasteText(t.clone())),
        Step::Read { tag, name } => Some(Press::Read { tag: tag.clone(), name: name.clone() }),
        Step::Click { right, mods, at } => Some(Press::Click { right: *right, mods: *mods, at: *at }),
        Step::Hover { at } => Some(Press::Hover { at: *at }),
        Step::Wheel { milli, mods, at } => Some(Press::Wheel { milli: *milli, mods: *mods, at: *at }),
        Step::Drag { from, to } => Some(Press::Drag { from: *from, to: *to }),
        Step::Raw { key, mods } => {
            let modifiers = modifiers(*mods);
            let press = |pressed| egui::Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers };
            Some(Press::Events(vec![press(true), press(false)]))
        }
        Step::Wait(d) => Some(Press::Wait(*d)),
        Step::Now => Some(Press::Now),
        Step::Shot(name) => Some(Press::Shot(name.clone())),
        Step::State(name) => Some(Press::State(name.clone())),
        Step::Quit => Some(Press::Quit),
    }
}

/// A step as it was written, for saying where a script stopped.
pub fn label(step: &Step) -> String {
    match step {
        Step::Key(k) => k.to_string(),
        Step::Wait(d) => format!("<Wait:{}>", d.as_millis()),
        Step::Now => "<Now>".into(),
        Step::Shot(name) => format!("<Shot:{name}>"),
        Step::State(name) => format!("<State:{name}>"),
        Step::Quit => "<Quit>".into(),
        Step::Paste => "<Paste>".into(),
        Step::PasteText(t) => format!("<Paste:{}>", paste_text_escape(t)),
        Step::Read { tag, name } => format!("<{tag}:{name}>"),
        Step::Click { right, mods, at } => format!("<{}Click:{}{}>", if *right { "R" } else { "" }, held_text(*mods), at_text(*at)),
        Step::Hover { at } => format!("<Hover:{}>", at_text(*at)),
        Step::Wheel { milli, mods, at } => {
            format!("<Wheel:{}{}@{}>", held_text(*mods), *milli as f32 / 1000.0, at_text(*at))
        }
        Step::Drag { from, to } => format!("<Drag:{}-{}>", at_text(*from), at_text(*to)),
        Step::Raw { key, mods } => format!("<Key:{}{}>", held_text(*mods), key.name()),
    }
}

/// `<Key:C-Tab>` as the egui key and modifiers it names; `None` for a token
/// that is not one.
fn raw(token: &str) -> Option<Result<Step, String>> {
    let (mods, name) = held_prefix(token.strip_prefix("<Key:")?.strip_suffix('>')?);
    Some(
        egui::Key::from_name(name)
            .map(|key| Step::Raw { key, mods })
            .ok_or_else(|| format!("`{token}` is not a key; write egui's name for it, as `<Key:Tab>`, `<Key:Space>` or `<Key:C-A>`")),
    )
}

fn held_text(mods: u8) -> String {
    [(MOD_CTRL, "C-"), (MOD_ALT, "A-"), (MOD_SHIFT, "S-")].iter().filter(|(bit, _)| mods & bit != 0).map(|(_, t)| *t).collect()
}

/// `C-A-` in front of `text`: the modifier bits and what is left.
fn held_prefix(mut text: &str) -> (u8, &str) {
    let mut mods = 0u8;
    while let Some((bit, rest)) =
        [(MOD_CTRL, "C-"), (MOD_ALT, "A-"), (MOD_SHIFT, "S-")].iter().find_map(|(bit, t)| text.strip_prefix(t).map(|rest| (*bit, rest)))
    {
        (text, mods) = (rest, mods | bit);
    }
    (mods, text)
}

fn modifiers(mods: u8) -> egui::Modifiers {
    egui::Modifiers {
        ctrl: mods & MOD_CTRL != 0,
        command: mods & MOD_CTRL != 0,
        alt: mods & MOD_ALT != 0,
        shift: mods & MOD_SHIFT != 0,
        ..Default::default()
    }
}

fn at_text((x, y): At) -> String {
    format!("{},{}", x as f32 / 1000.0, y as f32 / 1000.0)
}

/// `0.5,0.4` as a place in the window.
fn at(text: &str) -> Option<At> {
    let (x, y) = text.split_once(',')?;
    let part = |v: &str| v.parse::<f32>().ok().filter(|v| (0.0..=1.0).contains(v)).map(|v| (v * 1000.0).round() as u16);
    Some((part(x)?, part(y)?))
}

/// The pointer steps' tokens: `<Click:0.5,0.4>`, `<RClick:0.5,0.4>`,
/// `<Wheel:-3@0.5,0.4>`, `<Wheel:C-0.25@0.5,0.4>`, `<Drag:0.2,0.3-0.6,0.3>`. `None` for a token that is not one.
fn pointer(token: &str) -> Option<Result<Step, String>> {
    let inner = token.strip_prefix('<')?.strip_suffix('>')?;
    let (name, arg) = inner.split_once(':')?;
    let bad = || {
        format!("`{token}` is not a pointer step; write a place as fractions of the window, as `<Click:0.5,0.4>`, `<Wheel:-3@0.5,0.4>` or `<Drag:0.2,0.3-0.6,0.3>`")
    };
    Some(match name {
        "Click" | "RClick" => {
            let (mods, place) = held_prefix(arg);
            at(place).map(|at| Step::Click { right: name == "RClick", mods, at }).ok_or_else(bad)
        }
        "Hover" => at(arg).map(|at| Step::Hover { at }).ok_or_else(bad),
        "Wheel" => arg
            .split_once('@')
            .and_then(|(n, place)| {
                let (mods, n) = held_prefix(n);
                let lines = n.parse::<f32>().ok().filter(|n| n.is_finite() && n.abs() <= 1000.0)?;
                Some(Step::Wheel { milli: (lines * 1000.0).round() as i32, mods, at: at(place)? })
            })
            .ok_or_else(bad),
        "Drag" => arg.split_once('-').and_then(|(from, to)| Some(Step::Drag { from: at(from)?, to: at(to)? })).ok_or_else(bad),
        _ => return None,
    })
}

/// The events a pointer step becomes, in a window of `rect`.
pub fn pointer_events(press: &Press, rect: egui::Rect) -> Vec<egui::Event> {
    let place = |(x, y): At| rect.min + egui::vec2(rect.width() * x as f32 / 1000.0, rect.height() * y as f32 / 1000.0);
    match press {
        Press::Hover { at } => vec![egui::Event::PointerMoved(place(*at))],
        Press::Click { right, mods, at } => {
            let (pos, button) = (place(*at), if *right { egui::PointerButton::Secondary } else { egui::PointerButton::Primary });
            let modifiers = modifiers(*mods);
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button, pressed: true, modifiers },
                egui::Event::PointerButton { pos, button, pressed: false, modifiers },
            ]
        }
        Press::Wheel { milli, mods, at } => {
            let modifiers = modifiers(*mods);
            vec![
                egui::Event::PointerMoved(place(*at)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, *milli as f32 / 1000.0),
                    phase: egui::TouchPhase::Move,
                    modifiers,
                },
            ]
        }
        Press::Drag { from, to } => {
            let (a, b, button, modifiers) = (place(*from), place(*to), egui::PointerButton::Primary, egui::Modifiers::NONE);
            vec![
                egui::Event::PointerMoved(a),
                egui::Event::PointerButton { pos: a, button, pressed: true, modifiers },
                egui::Event::PointerMoved(a.lerp(b, 0.5)),
                egui::Event::PointerMoved(b),
                egui::Event::PointerButton { pos: b, button, pressed: false, modifiers },
            ]
        }
        _ => Vec::new(),
    }
}

/// `<Paste:text>`: the text itself as the paste event.
pub fn paste_text_event(text: &str) -> egui::Event {
    egui::Event::Paste(text.to_owned())
}

/// How long nothing may be pressed, past any `<Wait:N>` due, before a script
/// counts as stuck. A key waits at most five seconds for things to settle, so
/// this is only reached when the frame loop itself has stopped running.
pub const STALL: Duration = Duration::from_secs(30);

/// What the done file holds for a script that stopped part way (#168,
/// proposal 5): which keys went in, the last of them, and what was left.
/// Before this an unattended run that stalled left nothing at all, so the
/// result read as missing rather than as failed.
pub fn stalled_report(labels: &[String], left: usize, quiet: Duration) -> String {
    let pressed = labels.len().saturating_sub(left);
    let last = pressed.checked_sub(1).map_or("nothing yet".into(), |i| format!("`{}`", labels[i]));
    format!(
        "keys: stalled\nstalled: {} s with nothing pressed\npressed: {pressed} of {} (last: {last})\nleft: {}\n",
        quiet.as_secs(),
        labels.len(),
        labels[pressed..].join(" "),
    )
}

/// What a panic leaves for a script that is running: the done file reads
/// `keys: panicked`, with where and why, instead of never arriving or
/// reading as a stall. A run that vanished twice (#259, #173) could not say
/// whether the app had panicked or been ended from outside.
pub fn panic_report(info: &std::panic::PanicHookInfo<'_>) -> String {
    let why = match (info.payload().downcast_ref::<&str>(), info.payload().downcast_ref::<String>()) {
        (Some(m), _) => (*m).to_owned(),
        (_, Some(m)) => m.clone(),
        _ => "no message".to_owned(),
    };
    let at = info.location().map_or_else(|| "unknown".to_owned(), |l| format!("{}:{}", l.file().replace('\\', "/"), l.line()));
    let thread = std::thread::current().name().unwrap_or("unnamed").to_owned();
    format!("keys: panicked\nthread: {thread}\nat: {at}\nwhy: {}\n", why.replace('\n', " / "))
}

/// Before the window opens, for a run that writes the done file: a panic
/// anywhere writes [`panic_report`] to that file and to `<file>.panic`, and
/// ends the process with a code that is not 0, whichever thread it was on.
pub fn install_panic_report(done: std::path::PathBuf) {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = panic_report(info);
        let _ = std::fs::write(&done, &report);
        let mut named = done.clone().into_os_string();
        named.push(".panic");
        let _ = std::fs::write(std::path::PathBuf::from(named), &report);
        before(info);
        std::process::exit(101);
    }));
}

/// The last line of the done file when the app quit while the script ran
/// (`q` as its last key, say): the frame that would have written `keys: done`
/// never comes for a window that has closed, so the file was missing and read
/// as a stall. A quit with keys still to go says how many.
pub fn quit_report(left: usize) -> String {
    match left {
        // `keys: done` stays the last line, which is what readers look at.
        0 => "quit: yes\nkeys: done\n".to_owned(),
        n => format!("keys: quit\nleft: {n} not pressed\n"),
    }
}

/// The last line of the done file when the window was closed from outside
/// (the title bar's ×, `WM_CLOSE`) while the script ran: a script waiting on
/// the file otherwise waited out its timeout, like for a vanished process.
pub fn closed_report(left: usize) -> String {
    match left {
        0 => "quit: window closed\nkeys: done\n".to_owned(),
        n => format!("keys: closed\nleft: {n} not pressed\n"),
    }
}

/// What the done file holds for a script refused before any window opened
/// (#193, proposal 1): a run started detached never sees the message on the
/// command line, and without this the file simply never arrived.
pub fn refused_report(why: &str) -> String {
    format!("keys: refused\nwhy: {why}\n")
}

/// The name in `<Shot:name>` or `<State:name>`: letters, digits, `-` and `_`,
/// so it is a file name on every platform and cannot climb out of the folder
/// it is saved in. `what` is the tag, `noun` what the error calls it.
fn named(token: &str, what: &str, noun: &str) -> Result<String, String> {
    let name = token.strip_prefix(&format!("<{what}:")).and_then(|t| t.strip_suffix('>')).unwrap_or("");
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("`{token}` is not a {noun}; name it with letters, digits, - and _, as `<{what}:before>`"));
    }
    Ok(name.to_owned())
}

/// The text in `<Paste:text>`; see [`Step::PasteText`] for the escapes.
fn paste_text(token: &str) -> Result<String, String> {
    let bad = |why: &str| format!("`{token}` is not a paste: {why}");
    let inner = token.strip_prefix("<Paste:").and_then(|t| t.strip_suffix('>')).unwrap_or("");
    let mut out = String::new();
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('x') => {
                let hex: String = it.by_ref().take(2).collect();
                let b = u8::from_str_radix(&hex, 16).ok().filter(|_| hex.len() == 2).ok_or_else(|| bad("`\\x` takes two hex digits"))?;
                out.push(b as char);
            }
            Some('\\') => out.push('\\'),
            _ => return Err(bad("write a backslash as `\\\\`")),
        }
    }
    Ok(out)
}

/// `paste_text`'s inverse, for the label of a step.
fn paste_text_escape(t: &str) -> String {
    t.chars()
        .map(|c| match c {
            '\n' => "\\n".into(),
            '\t' => "\\t".into(),
            '\x1b' => "\\e".into(),
            '\\' => "\\\\".into(),
            '>' => "\\x3e".into(),
            c if (c as u32) < 0x20 => format!("\\x{:02x}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

/// The name in `<Shot:name>` or `<Shot:name@suffix>`; the suffix stays on
/// the name, which the app splits off where the picture is saved.
fn shot_name(token: &str, suffixes: &[&str]) -> Result<String, String> {
    for suffix in suffixes {
        if let Some(head) = token.strip_suffix(&format!("{suffix}>")) {
            return Ok(format!("{}{suffix}", named(&format!("{head}>"), "Shot", "shot")?));
        }
    }
    named(token, "Shot", "shot")
}

/// `<Wait:500>` as a pause, in milliseconds.
fn wait(token: &str) -> Option<Result<Duration, String>> {
    let ms = token.strip_prefix("<Wait:")?.strip_suffix('>')?;
    Some(match ms.parse::<u64>() {
        Ok(n) if n <= 60_000 => Ok(Duration::from_millis(n)),
        _ => Err(format!("`{token}` is not a wait; write milliseconds up to 60000, as `<Wait:500>`")),
    })
}

/// What an app adds to the common script words.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dialect<'a> {
    /// Readings of the app's own: `(tag, noun)`, as `("PaneText", "pane text")`
    /// for `<PaneText:name>`. The noun is for the error message.
    pub reads: &'a [(&'a str, &'a str)],
    /// Suffixes `<Shot:name@suffix>` may carry (kura's `@preview`); the
    /// name keeps the suffix, which the app splits off where it saves.
    pub shot_suffixes: &'a [&'a str],
}

/// `<Tab>C<C-S-t>gg` as the keys it names, in yazi's notation: `<…>` is one
/// key, anything else is one key per character. `<Wait:N>` pauses N ms, and
/// `<Now>` presses the next key without waiting for the last one to settle,
/// `<Shot:name>` saves the window as `name.png` and `<State:name>` the state
/// report as `name.txt`.
pub fn parse(script: &str) -> Result<Vec<Step>, String> {
    parse_with(script, &Dialect::default())
}

/// [`parse`], with the app's own readings and shot suffixes.
pub fn parse_with(script: &str, dialect: &Dialect) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    let mut rest = script;
    while let Some(c) = rest.chars().next() {
        // A plain space never arrived as one: the rest of the script went to
        // the list instead of the pane, so `<C-t>echo hi<Enter>` typed `echo`
        // and walked the list. Refused before the window opens.
        if c.is_whitespace() {
            return Err(format!("a plain space cannot be typed here; write `<Space>` (at `{rest}`)"));
        }
        let token = if c == '<' {
            match rest.find('>') {
                Some(end) => &rest[..=end],
                None => return Err(format!("`{rest}` has no closing `>`")),
            }
        } else {
            &rest[..c.len_utf8()]
        };
        let read = dialect.reads.iter().find(|(tag, _)| token.starts_with(&format!("<{tag}:")));
        let step = match wait(token) {
            Some(d) => Step::Wait(d?),
            None if token == "<Now>" => Step::Now,
            None if token == "<Quit>" => Step::Quit,
            None if token.starts_with("<Shot:") => Step::Shot(shot_name(token, dialect.shot_suffixes)?),
            None if token.starts_with("<State:") => Step::State(named(token, "State", "state")?),
            None if token == "<Paste>" => Step::Paste,
            None if token.starts_with("<Paste:") => Step::PasteText(paste_text(token)?),
            None if read.is_some() => {
                let (tag, noun) = read.copied().unwrap_or_else(|| unreachable!());
                Step::Read { tag: tag.to_owned(), name: named(token, tag, noun)? }
            }
            None if pointer(token).is_some() => pointer(token).unwrap_or_else(|| unreachable!())?,
            None if raw(token).is_some() => raw(token).unwrap_or_else(|| unreachable!())?,
            None => Step::Key(Key::parse(token).ok_or_else(|| format!("`{token}` is not a key"))?),
        };
        rest = &rest[token.len()..];
        // `<C-+>*45`: the key that many times. Only after a `<…>` key, since a
        // bare `*` is a key of its own.
        let mut times = 1;
        if c == '<' && !matches!(step, Step::Wait(_) | Step::Now | Step::Quit | Step::Shot(_) | Step::State(_) | Step::Read { .. }) {
            if let Some(after) = rest.strip_prefix('*') {
                let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
                match digits.parse::<usize>() {
                    Ok(n) if (1..=1000).contains(&n) => {
                        times = n;
                        rest = &after[digits.len()..];
                    }
                    _ => return Err(format!("`{token}*` has to be followed by a count from 1 to 1000, as `{token}*45`")),
                }
            }
        }
        for _ in 0..times {
            out.push(step.clone());
        }
    }
    // `<Now>` is about the key after it, so there has to be one.
    for (i, step) in out.iter().enumerate() {
        if *step == Step::Now && !matches!(out.get(i + 1), Some(s) if !matches!(s, Step::Wait(_) | Step::Now | Step::Quit)) {
            return Err(
                "`<Now>` has to come right before a key or a reading (`<State:x>`, `<Shot:x>`), as `d<Now>w` or `<A-c><Now><State:mid>`"
                    .into(),
            );
        }
    }
    Ok(out)
}

/// `<C-c>` and `<C-x>` as a keyboard's Ctrl+C and Ctrl+X arrive: egui-winit
/// turns the press into `Copy` or `Cut` and never sends it, and the key's
/// release follows with Ctrl held, which is where `ito_keys::from_clipboard`
/// reads the chord from. As a key press they ran the app's `<C-c>` at once,
/// where the keyboard copies a selection first (kura #383). On a Mac the
/// clipboard's chords are Cmd, which a script cannot press, so Ctrl+C stays a
/// key there. With Shift (which egui-winit also turns into `Copy`) they stay
/// key presses: the event carries no modifiers, and an app reads Shift from
/// the ones held in the frame, which a script does not hold.
fn clipboard_chord(key: &Key) -> Option<Vec<egui::Event>> {
    if cfg!(target_os = "macos") || !key.ctrl || key.alt || key.shift || key.sup {
        return None;
    }
    let (event, letter) = match key.code {
        Code::Char('c') => (egui::Event::Copy, egui::Key::C),
        Code::Char('x') => (egui::Event::Cut, egui::Key::X),
        _ => return None,
    };
    let modifiers = egui::Modifiers { ctrl: true, command: true, ..Default::default() };
    Some(vec![event, egui::Event::Key { key: letter, physical_key: None, pressed: false, repeat: false, modifiers }])
}

/// The events one press of `key` arrives as. `None` for a key no keyboard
/// event can produce, which `--keys` refuses before the window opens.
pub fn events(key: &Key) -> Option<Vec<egui::Event>> {
    // A character typed without a chord arrives as text, which is how
    // `handle_input` expects it -- a key event for it would be dropped.
    if let Code::Char(c) = key.code {
        if !key.ctrl && !key.alt && !key.sup {
            // Space is a key as well as a character: a switch or a button with
            // the focus toggles on the key, and the list and the pane take the
            // text. A keyboard sends both, the key first (egui-winit), so the
            // settings screen's switches could not be pressed from a script
            // (#336, #344).
            if c == ' ' && !key.shift {
                let press = |pressed| egui::Event::Key {
                    key: egui::Key::Space,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                };
                return Some(vec![press(true), egui::Event::Text(" ".into()), press(false)]);
            }
            return Some(vec![egui::Event::Text(c.to_string())]);
        }
    }
    // Everything else is a key event: the one, among every key and modifier
    // combination, that reads back as this key.
    for &k in egui::Key::ALL {
        for bits in 0..8u8 {
            let modifiers = egui::Modifiers {
                shift: bits & 1 != 0,
                ctrl: bits & 2 != 0,
                command: bits & 2 != 0,
                alt: bits & 4 != 0,
                ..Default::default()
            };
            if keys::from_egui(k, &modifiers).as_ref() == Some(key) {
                let press = |pressed| egui::Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers };
                return Some(vec![press(true), press(false)]);
            }
        }
    }
    None
}

/// What the frame loop does with a step that is not a plain input.
#[derive(Debug, PartialEq, Eq)]
pub enum Do {
    State(String),
    /// A reading of the app's own ([`Dialect::reads`]).
    Read {
        tag: String,
        name: String,
    },
    Quit,
    /// The last step has gone in.
    Done,
}

/// A click or a drag as the frames of pointer events it takes: egui sees a
/// press and a release in one frame as a click, never as a drag.
fn pointer_frames(press: &Press, rect: egui::Rect) -> VecDeque<Vec<egui::Event>> {
    let events = pointer_events(press, rect);
    match press {
        Press::Click { .. } => events.into_iter().map(|e| vec![e]).collect(),
        Press::Drag { from, to } => {
            let place = |(x, y): At| rect.min + egui::vec2(rect.width() * x as f32 / 1000.0, rect.height() * y as f32 / 1000.0);
            let (a, b) = (place(*from), place(*to));
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let mut frames = VecDeque::from([vec![egui::Event::PointerMoved(a)], vec![button(a, true)]]);
            frames.extend((1..=4).map(|k| vec![egui::Event::PointerMoved(a.lerp(b, k as f32 / 4.0))]));
            frames.push_back(vec![button(b, false)]);
            frames
        }
        _ => VecDeque::from([events]),
    }
}

/// How far a [`Runner`] has got, shared with the thread [`Runner::watch`]
/// starts.
struct Progress {
    /// Steps not yet taken.
    left: usize,
    /// When the last one was.
    at: Instant,
    /// How long the quiet after it is meant to be (a `<Wait:N>`).
    due: Duration,
    /// The run has ended; the thread can stop.
    finished: bool,
}

/// [`Runner::watch`]'s thread: nudge the frame loop once a second, and if
/// nothing is pressed for [`STALL`] past any wait due, write
/// [`stalled_report`] to `done`. It stops when the run ends or the runner is
/// dropped.
fn watch_progress(progress: Arc<Mutex<Progress>>, labels: Vec<String>, done: PathBuf, ctx: egui::Context) {
    let mut told = false;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if Arc::strong_count(&progress) == 1 {
            return;
        }
        {
            // Held across the write: a run that finishes meanwhile waits,
            // and its own report lands after this one.
            let Ok(p) = progress.lock() else { return };
            if p.finished {
                return;
            }
            let quiet = p.at.elapsed();
            if !told && quiet > p.due + STALL {
                told = true;
                let report = stalled_report(&labels, p.left, quiet);
                let _ = std::fs::write(&done, &report);
                use std::io::Write;
                // Not `eprintln!`: started from an app's `.com` front, this can be
                // a pipe nobody reads once the window is up, and that would panic.
                let _ = writeln!(std::io::stderr(), "--keys: {}", report.lines().collect::<Vec<_>>().join("; "));
            }
        }
        ctx.request_repaint();
    }
}

/// A script being played.
pub struct Runner {
    steps: VecDeque<Step>,
    next: Instant,
    /// The picture asked for, and whether the request has gone out.
    shot: Option<(String, bool)>,
    /// The frames of a click or a drag still to go in.
    pointer: VecDeque<Vec<egui::Event>>,
    /// Where `<Shot:>` and `<State:>` write.
    pub dir: PathBuf,
    /// The app's `<APP>_KEYS_DONE`.
    pub done: Option<PathBuf>,
    /// The clipboard's text for `<Paste>` and `<C-v>`.
    pub clipboard: fn() -> String,
    finished: bool,
    progress: Option<Arc<Mutex<Progress>>>,
}

impl Runner {
    pub fn new(steps: Vec<Step>, done: Option<PathBuf>) -> Self {
        let dir = done
            .as_ref()
            .and_then(|d| d.parent().map(PathBuf::from))
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        // The first frames lay the window out; a key before them can land
        // before the panes exist.
        Self {
            steps: steps.into(),
            next: Instant::now() + Duration::from_millis(500),
            shot: None,
            pointer: VecDeque::new(),
            dir,
            done,
            clipboard: String::new,
            finished: false,
            progress: None,
        }
    }

    /// Steps not yet taken: for [`quit_report`] and [`closed_report`] when
    /// the window closes while the script runs.
    pub fn left(&self) -> usize {
        self.steps.len()
    }

    /// Whether the script is still being played: not past its last step,
    /// nor [`finish`](Self::finish)ed.
    pub fn running(&self) -> bool {
        !self.finished
    }

    /// The run has ended some other way than its last step (the window
    /// closed while it ran): no more steps, and the watch stops. Called
    /// before the app writes [`closed_report`], so a stall report cannot
    /// land over it. A `<Quit>` finishes the run by itself.
    pub fn finish(&mut self) {
        self.finished = true;
        self.note(Instant::now());
    }

    /// For a run that writes the done file: a thread nudges the frame loop
    /// once a second (a window that gets no frames presses nothing, #168),
    /// and if nothing is pressed for [`STALL`] past any wait due, writes
    /// [`stalled_report`] there. A run that then goes on to finish writes its
    /// usual report over it. Nothing without a done file, or a second time.
    /// Called before the first [`input`](Self::input), as the report counts
    /// the steps from there.
    pub fn watch(&mut self, ctx: egui::Context) {
        let Some(done) = self.done.clone() else { return };
        if self.progress.is_some() {
            return;
        }
        let labels = self.steps.iter().map(label).collect();
        let now = Instant::now();
        let progress = Progress { left: self.steps.len(), at: now, due: self.next.saturating_duration_since(now), finished: self.finished };
        let progress = Arc::new(Mutex::new(progress));
        self.progress = Some(progress.clone());
        let _ = std::thread::Builder::new().name("keys-watch".into()).spawn(move || watch_progress(progress, labels, done, ctx));
    }

    /// Tell the watch the script has moved.
    fn note(&self, now: Instant) {
        let Some(progress) = &self.progress else { return };
        if let Ok(mut p) = progress.lock() {
            p.left = self.steps.len();
            p.at = now;
            p.due = self.next.saturating_duration_since(now);
            p.finished = self.finished;
        }
    }

    /// Put the next step's events into this frame's input when it is due.
    pub fn input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) -> Option<Do> {
        if self.finished || self.shot.is_some() {
            return None;
        }
        let now = Instant::now();
        if now < self.next {
            return None;
        }
        if let Some(frame) = self.pointer.pop_front() {
            raw.events.extend(frame);
            self.next = now + FRAME;
            self.note(now);
            return None;
        }
        let Some(step) = self.steps.pop_front() else {
            self.finished = true;
            self.note(now);
            return Some(Do::Done);
        };
        self.next = now + GAP;
        let did = self.take(&step, ctx, raw, now);
        self.note(now);
        did
    }

    /// One step into this frame's input.
    fn take(&mut self, step: &Step, ctx: &egui::Context, raw: &mut egui::RawInput, now: Instant) -> Option<Do> {
        // A key no keyboard has is skipped, as `--keys` refuses it up front.
        match press(step)? {
            Press::Events(events) => raw.events.extend(events),
            Press::Wait(d) => self.next = now + d,
            // This runner has no settling to skip.
            Press::Now => self.next = now,
            Press::Shot(name) => self.shot = Some((name, false)),
            Press::State(name) => return Some(Do::State(name)),
            Press::Read { tag, name } => return Some(Do::Read { tag, name }),
            Press::Quit => {
                // The window closes; no later step is taken, nor a stall
                // reported over the app's quit report.
                self.finished = true;
                return Some(Do::Quit);
            }
            Press::Paste => raw.events.push(egui::Event::Paste((self.clipboard)())),
            Press::PasteText(text) => raw.events.push(paste_text_event(&text)),
            p @ (Press::Click { .. } | Press::Hover { .. } | Press::Wheel { .. } | Press::Drag { .. }) => {
                let rect = raw.screen_rect.unwrap_or_else(|| ctx.content_rect());
                self.pointer = pointer_frames(&p, rect);
                self.next = now;
            }
        }
        None
    }

    /// When the next frame is wanted for the script: a repaint asked for in
    /// `raw_input_hook` is forgotten when the frame begins, so the frame asks.
    pub fn due(&self) -> Option<Duration> {
        match () {
            _ if self.finished => None,
            _ if self.shot.is_some() => Some(Duration::ZERO),
            _ => Some(self.next.saturating_duration_since(Instant::now())),
        }
    }

    /// `<Shot:name>`, once per frame: ask for the picture, then save it when
    /// it arrives. An error is returned for a toast.
    pub fn shot(&mut self, ctx: &egui::Context) -> Option<String> {
        let (name, sent) = self.shot.clone()?;
        if !sent {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(name.clone())));
            self.shot = Some((name, true));
            return None;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { user_data, image, .. }
                    if user_data.data.as_ref().and_then(|d| d.downcast_ref::<String>()) == Some(&name) =>
                {
                    Some(image.clone())
                }
                _ => None,
            })
        });
        let image = image?;
        self.shot = None;
        let now = Instant::now();
        self.next = now + GAP;
        self.note(now);
        let path = self.dir.join(format!("{name}.png"));
        save_png(&path, &image).err().map(|e| format!("Shot {name}: {e}"))
    }
}

/// `image` as an 8-bit RGBA PNG at `path`; also for an app that crops its own
/// shots (kura's `@preview`).
pub fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let [w, h] = image.size;
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgba).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    /// The two endings that used to leave no the done file at all: a
    /// script whose last key quits, and one refused before the window opened.
    #[test]
    fn a_quit_or_a_refusal_still_says_how_the_script_ended() {
        assert_eq!(quit_report(0), "quit: yes\nkeys: done\n");
        assert_eq!(quit_report(3), "keys: quit\nleft: 3 not pressed\n");
        assert_eq!(closed_report(0), "quit: window closed\nkeys: done\n");
        assert_eq!(closed_report(2), "keys: closed\nleft: 2 not pressed\n");
        let why = parse("j k").unwrap_err();
        assert_eq!(refused_report(&why), format!("keys: refused\nwhy: {why}\n"));
    }

    use super::*;

    /// #168: a script that stopped says how far it got, in the notation it
    /// was written in.
    #[test]
    fn a_stalled_script_says_where_it_stopped() {
        let labels: Vec<String> = parse("d<Wait:9000>u<Shot:after>").unwrap().iter().map(label).collect();
        assert_eq!(labels, ["d", "<Wait:9000>", "u", "<Shot:after>"]);
        let said = stalled_report(&labels, 2, Duration::from_secs(39));
        assert_eq!(
            said,
            "keys: stalled\nstalled: 39 s with nothing pressed\npressed: 2 of 4 (last: `<Wait:9000>`)\nleft: u <Shot:after>\n"
        );
        assert!(stalled_report(&labels, 4, STALL).contains("pressed: 0 of 4 (last: nothing yet)"));
    }

    /// Q42: `<Shot:name>` is a step of its own, and its name is a plain file name.
    #[test]
    fn a_shot_is_named() {
        let got = parse("j<Quit>").unwrap();
        assert_eq!(got[1], Step::Quit, "#236: `<Quit>` is a step, not a key");
        assert_eq!((label(&got[1]).as_str(), press(&got[1])), ("<Quit>", Some(Press::Quit)));
        let got = parse("j<State:mid>k").unwrap();
        assert_eq!(got[1], Step::State("mid".into()), "#230: `<State:name>` likewise");
        assert_eq!(label(&got[1]), "<State:mid>");
        for bad in ["<State:>", "<State:../x>"] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("<State:before>"), "{bad}: {err}");
        }
        let got = parse("j<Shot:after-j>k").unwrap();
        assert_eq!(got[1], Step::Shot("after-j".into()));
        assert_eq!(press(&got[1]), Some(Press::Shot("after-j".into())));
        let kura = Dialect { reads: &[], shot_suffixes: &["@preview"] };
        assert_eq!(parse_with("<Shot:p@preview>", &kura).unwrap(), [Step::Shot("p@preview".into())]);
        assert!(parse_with("<Shot:@preview>", &kura).is_err() && parse_with("<Shot:a@b>", &kura).is_err());
        assert!(parse("<Shot:p@preview>").is_err(), "no suffix unless the app names one");
        for bad in ["<Shot:>", "<Shot:../x>", "<Shot:a b>", "<Shot:a/b>"] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("<Shot:before>"), "{bad}: {err}");
        }
    }

    /// Q41: `<Now>` marks the key after it, and only a key can follow it.
    #[test]
    fn now_marks_the_next_key() {
        let got = parse("d<Now>w").unwrap();
        assert_eq!(got[1], Step::Now);
        assert!(matches!(got[2], Step::Key(_)));
        assert_eq!(press(&got[1]), Some(Press::Now));
        for bad in ["d<Now>", "<Now><Wait:100>x", "<Now><Now>x"] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("<Now>"), "{bad}: {err}");
        }
    }

    /// Named keys, chords and plain characters, in one run of text.
    #[test]
    fn a_script_splits_into_its_keys() {
        let got = parse("<Tab>C<C-S-t>gg").unwrap();
        let want: Vec<Step> = ["<Tab>", "C", "<C-S-t>", "g", "g"].iter().map(|t| Step::Key(Key::parse(t).unwrap())).collect();
        assert_eq!(got, want);
        assert!(parse("<Tab").is_err(), "an unclosed key is refused, not guessed at");
        assert!(parse("<Nonsense>").is_err());
        assert_eq!(parse("").unwrap(), Vec::new());
        // #110: a space is written `<Space>`; a plain one is refused, naming where.
        let err = parse("<C-t>echo hi<Enter>").unwrap_err();
        assert!(err.contains("<Space>") && err.contains("hi<Enter>"), "{err}");
        assert_eq!(parse("a<Space>b").unwrap().len(), 3);
    }

    /// `<Wait:N>` is a pause of N milliseconds between the keys either side.
    #[test]
    fn a_wait_is_a_pause_in_milliseconds() {
        let got = parse("<C-t><Wait:1500>x").unwrap();
        assert_eq!(got[1], Step::Wait(Duration::from_millis(1500)));
        assert_eq!(press(&got[1]), Some(Press::Wait(Duration::from_millis(1500))));
        assert!(matches!(press(&got[2]), Some(Press::Events(_))));
        for bad in ["<Wait:>", "<Wait:1.5s>", "<Wait:-1>", "<Wait:99999999>"] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("<Wait:500>"), "{bad}: {err}");
        }
    }

    /// Every key produces events that `from_egui` reads back as the same key --
    /// the round trip a real keyboard makes -- and a plain character is text.
    #[test]
    fn each_key_becomes_the_events_that_read_back_as_it() {
        for token in ["<Tab>", "<Enter>", "<Esc>", "<C-S-t>", "<A-d>", "<C-f>", "<F12>", "<Up>", "<S-Tab>", "<C-+>"] {
            let key = Key::parse(token).unwrap();
            let evs = events(&key).unwrap_or_else(|| panic!("{token} has no events"));
            match &evs[0] {
                egui::Event::Key { key: k, pressed: true, modifiers, .. } => {
                    assert_eq!(keys::from_egui(*k, modifiers).as_ref(), Some(&key), "{token}");
                }
                other => panic!("{token} should be a key press, got {other:?}"),
            }
        }
        let c = Key::parse("C").unwrap();
        assert_eq!(events(&c), Some(vec![egui::Event::Text("C".into())]));
    }

    /// #336, #344: `<Space>` is the key and the text, in a keyboard's order,
    /// and `<Key:X>` the bare key event with no text.
    #[test]
    fn space_is_a_key_and_text_and_key_names_a_bare_event() {
        let key = |pressed, modifiers| egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers };
        let none = egui::Modifiers::NONE;
        assert_eq!(events(&Key::parse("<Space>").unwrap()), Some(vec![key(true, none), egui::Event::Text(" ".into()), key(false, none)]));
        let got = parse("<Key:Space><Key:C-S-Tab>").unwrap();
        assert_eq!(got, [Step::Raw { key: egui::Key::Space, mods: 0 }, Step::Raw { key: egui::Key::Tab, mods: MOD_CTRL | MOD_SHIFT }]);
        assert_eq!(press(&got[0]), Some(Press::Events(vec![key(true, none), key(false, none)])));
        assert_eq!(got.iter().map(label).collect::<Vec<_>>(), ["<Key:Space>", "<Key:C-S-Tab>"]);
        assert_eq!(parse("<Key:Tab>*3").unwrap().len(), 3);
        for bad in ["<Key:>", "<Key:Nonsense>", "<Key:tab>"] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("<Key:Tab>"), "{bad}: {err}");
        }
    }

    /// `<Paste:text>` pastes that text, with the escapes of `Step::PasteText`.
    #[test]
    fn paste_text_reads_its_escapes() {
        let got = parse("<Paste:a b\\n\\x1b[200~\\\\\\x3e>").unwrap();
        assert_eq!(got, [Step::PasteText("a b\n\x1b[200~\\>".into())]);
        assert_eq!(press(&got[0]), Some(Press::PasteText("a b\n\x1b[200~\\>".into())));
        assert_eq!(label(&got[0]), "<Paste:a b\\n\\e[200~\\\\\\x3e>");
        for bad in ["<Paste:\\q>", "<Paste:\\x1>", "<Paste:\\>"] {
            assert!(parse(bad).unwrap_err().contains("not a paste"), "{bad}");
        }
    }

    /// A click holds `C-` / `A-` / `S-` like a wheel does, and `<Hover:>` only moves the pointer.
    #[test]
    fn clicks_hold_keys_and_hover_moves() {
        let got = parse("<Click:C-S-0.5,0.5><Hover:0.1,0.2>").unwrap();
        assert_eq!(got[0], Step::Click { right: false, mods: MOD_CTRL | MOD_SHIFT, at: (500, 500) });
        assert_eq!(got[1], Step::Hover { at: (100, 200) });
        assert_eq!(got.iter().map(label).collect::<Vec<_>>(), ["<Click:C-S-0.5,0.5>", "<Hover:0.1,0.2>"]);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let ev = pointer_events(&press(&got[0]).unwrap(), rect);
        assert!(
            matches!(ev[1], egui::Event::PointerButton { pressed: true, modifiers, .. } if modifiers.ctrl && modifiers.shift && !modifiers.alt),
            "{ev:?}"
        );
        let ev = pointer_events(&press(&got[1]).unwrap(), rect);
        assert_eq!(ev, [egui::Event::PointerMoved(egui::pos2(10.0, 20.0))]);
    }

    /// #260: the pointer steps parse, keep their place, and become the events
    /// a mouse makes inside the window they are given.
    #[test]
    fn pointer_steps_are_places_in_the_window() {
        let got = parse("<Click:0.5,0.25><RClick:1,0><Wheel:-3@0.1,0.9>").unwrap();
        assert_eq!(got[0], Step::Click { right: false, mods: 0, at: (500, 250) });
        assert_eq!(got[1], Step::Click { right: true, mods: 0, at: (1000, 0) });
        assert_eq!(got[2], Step::Wheel { milli: -3000, mods: 0, at: (100, 900) });
        assert_eq!(got.iter().map(label).collect::<Vec<_>>(), ["<Click:0.5,0.25>", "<RClick:1,0>", "<Wheel:-3@0.1,0.9>"]);
        for bad in ["<Click:2,0.5>", "<Click:0.5>", "<Wheel:3>", "<Wheel:x@0.5,0.5>", "<RClick:a,b>"] {
            assert!(parse(bad).unwrap_err().contains("pointer step"), "{bad}");
        }
        let more = parse("<Wheel:C-0.25@0.5,0.5><Wheel:A-S--1@0,0><Drag:0.25,0.5-0.75,0.5>").unwrap();
        assert_eq!(more[0], Step::Wheel { milli: 250, mods: MOD_CTRL, at: (500, 500) });
        assert_eq!(more[1], Step::Wheel { milli: -1000, mods: MOD_ALT | MOD_SHIFT, at: (0, 0) });
        assert_eq!(more[2], Step::Drag { from: (250, 500), to: (750, 500) });
        assert_eq!(
            more.iter().map(label).collect::<Vec<_>>(),
            ["<Wheel:C-0.25@0.5,0.5>", "<Wheel:A-S--1@0,0>", "<Drag:0.25,0.5-0.75,0.5>"]
        );
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let drag = pointer_events(&press(&more[2]).unwrap(), rect);
        assert!(matches!(drag[1], egui::Event::PointerButton { pos, pressed: true, .. } if pos == egui::pos2(200.0, 300.0)), "{drag:?}");
        assert!(matches!(drag[4], egui::Event::PointerButton { pos, pressed: false, .. } if pos == egui::pos2(600.0, 300.0)), "{drag:?}");
        let held = pointer_events(&press(&more[0]).unwrap(), rect);
        assert!(
            matches!(held[1], egui::Event::MouseWheel { delta, modifiers, .. } if delta == egui::vec2(0.0, 0.25) && modifiers.ctrl),
            "{held:?}"
        );
        let click = pointer_events(&press(&got[0]).unwrap(), rect);
        assert!(
            matches!(click[1], egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } if pos == egui::pos2(400.0, 150.0)),
            "{click:?}"
        );
        let wheel = pointer_events(&press(&got[2]).unwrap(), rect);
        assert!(matches!(wheel[1], egui::Event::MouseWheel { delta, .. } if delta == egui::vec2(0.0, -3.0)), "{wheel:?}");
    }

    /// #259, #173: a panic under a script is written down where the script's
    /// reader looks, with the thread, the place and the message.
    #[test]
    fn a_panic_is_reported_for_the_script_that_was_running() {
        use std::sync::{Arc, Mutex};
        let caught = Arc::new(Mutex::new(String::new()));
        let seen = caught.clone();
        let before = std::panic::take_hook();
        // Only this test's own thread: a panic elsewhere while the hook is
        // swapped must not be taken for the one under test.
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() == Some("worker-x") {
                *seen.lock().unwrap() = panic_report(info);
            }
        }));
        let _ = std::thread::Builder::new().name("worker-x".into()).spawn(|| panic!("boom {}", 7)).unwrap().join();
        std::panic::set_hook(before);
        let report = caught.lock().unwrap().clone();
        // `Location::file()` is `src\lib.rs` on Windows.
        assert!(
            report.replace('\\', "/").starts_with("keys: panicked\nthread: worker-x\nat: crates/ito-keyscript/src/lib.rs:"),
            "{report}"
        );
        assert!(report.ends_with("why: boom 7\n"), "{report}");
    }

    /// #216: `<C-+>*3` is the key three times; a bare `*` is still a key.
    #[test]
    fn a_count_after_a_key_repeats_it() {
        let key = |t| Step::Key(Key::parse(t).unwrap());
        assert_eq!(parse("<C-+>*3x").unwrap(), [key("<C-+>"), key("<C-+>"), key("<C-+>"), key("x")]);
        assert_eq!(parse("j*").unwrap(), [key("j"), key("*")], "a bare star is a key");
        assert_eq!(parse("<Up>*1").unwrap(), [key("<Up>")]);
        for bad in ["<Up>*", "<Up>*0", "<Up>*1001", "<Up>*x"] {
            assert!(parse(bad).unwrap_err().contains("count from 1 to 1000"), "{bad}");
        }
    }

    /// #268: `<Now>` may come before a reading, to take it with a job still
    /// running; before a wait it still means nothing.
    #[test]
    fn now_may_come_before_a_reading() {
        assert!(parse("<A-c><Now><State:mid>").is_ok());
        assert!(parse("<Now><Shot:a>").is_ok());
        assert!(parse("<Now><Wait:5>x").is_err());
        assert!(parse("<Now>").is_err());
    }

    /// An app's own readings parse with their tag, keep the name, and may
    /// follow `<Now>`.
    #[test]
    fn a_reading_is_a_named_step_of_the_dialect() {
        let kura = Dialect { reads: &[("PaneText", "pane text"), ("PreviewText", "preview text")], shot_suffixes: &[] };
        let got = parse_with("<PaneText:footer><Now><PreviewText:p>", &kura).unwrap();
        assert_eq!(got[0], Step::Read { tag: "PaneText".into(), name: "footer".into() });
        assert_eq!(got.iter().map(label).collect::<Vec<_>>(), ["<PaneText:footer>", "<Now>", "<PreviewText:p>"]);
        assert_eq!(press(&got[0]), Some(Press::Read { tag: "PaneText".into(), name: "footer".into() }));
        assert!(parse_with("<PaneText:>", &kura).unwrap_err().contains("not a pane text"));
        assert!(parse_with("<PaneText:../x>", &kura).is_err());
        assert!(parse("<PaneText:footer>").is_err(), "not a word of the common script");
    }

    /// Ctrl+V is the paste event; the runner takes the clipboard from its app.
    #[test]
    fn ctrl_v_is_a_paste() {
        assert_eq!(press(&Step::Key(Key::parse("<C-v>").unwrap())), Some(Press::Paste));
        assert_eq!(parse("<Paste>").unwrap(), [Step::Paste]);
        assert_eq!(paste_text_event("x"), egui::Event::Paste("x".into()));
    }

    /// kura #383: Ctrl+C and Ctrl+X are the copy and cut events and the key's
    /// release, which `from_clipboard` reads back as the chord; with Shift or
    /// Alt they are key presses as before.
    #[test]
    fn ctrl_c_and_x_are_the_clipboard_events() {
        let press_of = |token: &str| press(&Step::Key(Key::parse(token).unwrap())).unwrap();
        for (token, event) in [("<C-c>", egui::Event::Copy), ("<C-x>", egui::Event::Cut)] {
            let Press::Events(evs) = press_of(token) else { panic!("{token}") };
            if cfg!(target_os = "macos") {
                assert!(matches!(evs[0], egui::Event::Key { pressed: true, .. }), "{token}: Ctrl is not the clipboard's on a Mac");
                continue;
            }
            assert_eq!(evs[0], event, "{token}");
            assert!(matches!(evs[1], egui::Event::Key { pressed: false, .. }), "{token}: {evs:?}");
            let back = keys::from_clipboard(&evs, 0, &egui::Modifiers::NONE).map(|k| k.to_string());
            assert_eq!(back.as_deref(), Some(token));
        }
        for token in ["<C-S-c>", "<C-A-x>", "<C-v>", "<C-a>"] {
            let got = press_of(token);
            assert!(
                !matches!(&got, Press::Events(evs) if evs.iter().any(|e| matches!(e, egui::Event::Copy | egui::Event::Cut))),
                "{token}: {got:?}"
            );
        }
    }

    /// The runner says how many steps a `<Quit>` left, for the done file's
    /// report, and a run that quit takes no more steps.
    #[test]
    fn a_quit_leaves_the_steps_after_it() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("j<Quit>kl").unwrap(), None);
        let step = |r: &mut Runner| {
            r.next = Instant::now();
            r.input(&ctx, &mut egui::RawInput::default())
        };
        assert_eq!((r.left(), r.running()), (4, true));
        assert_eq!(step(&mut r), None);
        assert_eq!(step(&mut r), Some(Do::Quit));
        assert_eq!((r.left(), r.running(), step(&mut r)), (2, false, None));
        let mut r = Runner::new(parse("j").unwrap(), None);
        assert_eq!((step(&mut r), step(&mut r), r.running()), (None, Some(Do::Done), false));
    }

    /// #168 for the runner: a run nothing has moved for longer than the stall
    /// allows, past the wait it is on, leaves a report saying where it
    /// stopped; one still inside its wait does not.
    #[test]
    fn a_runner_that_stalls_leaves_a_report() {
        let dir = std::env::temp_dir().join(format!("ito-keyscript-stall-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let watched = |due: Duration, name: &str| {
            let done = dir.join(name);
            let mut r = Runner::new(parse("j<Wait:2000>k").unwrap(), Some(done.clone()));
            r.watch(egui::Context::default());
            r.steps.drain(..2);
            r.note(Instant::now());
            {
                let mut p = r.progress.as_ref().unwrap().lock().unwrap();
                p.at = Instant::now().checked_sub(STALL + Duration::from_secs(5)).unwrap();
                p.due = due;
            }
            // The thread looks once a second; a busy machine may be late.
            let until = Instant::now() + if due.is_zero() { Duration::from_secs(10) } else { Duration::from_millis(1500) };
            while Instant::now() < until && !(due.is_zero() && done.exists()) {
                std::thread::sleep(Duration::from_millis(50));
            }
            r.finish();
            std::fs::read_to_string(done).ok()
        };
        let said = watched(Duration::ZERO, "stuck.done").expect("a report");
        assert!(said.starts_with("keys: stalled\n"), "{said}");
        assert!(said.contains("pressed: 2 of 3 (last: `<Wait:2000>`)\nleft: k\n"), "{said}");
        assert_eq!(watched(Duration::from_secs(60), "waiting.done"), None, "still inside a <Wait:60000>");
        // No done file, nothing to watch.
        let mut r = Runner::new(parse("j").unwrap(), None);
        r.watch(egui::Context::default());
        assert!(r.progress.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
