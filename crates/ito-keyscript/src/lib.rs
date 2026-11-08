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
//! shared too; the writer adds one `run: <id>` line to each ([`run_id`], [`with_run`]; ito's own stall and panic writers
//! do it themselves); a [`Runner`] says how many steps are left
//! for them ([`Runner::left`]) and watches for a stall itself ([`Runner::watch`]).

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub mod probe;
pub mod review;

use ito_keys::{self as keys, Code, Key};

/// The pause after each step, so the frame that follows a key has drawn
/// before the next key goes in.
const GAP: Duration = Duration::from_millis(60);
/// After a resize, before the next step.
const SETTLE: Duration = Duration::from_millis(400);
/// Between the frames of a click or a drag.
const FRAME: Duration = Duration::from_millis(20);

/// One step of a script: a key to press, or a pause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key(Key),
    /// `<Wait:500>`: this long after the key before it went in, before the
    /// next one does. Several waits in a row count one after the other, each
    /// from the end of the one before, and each may take a frame longer. A
    /// shell in the pane runs at its own pace, which the app
    /// cannot see, so "settled" says nothing about it; every real-machine run
    /// that drove the pane filled the gap with harmless keys instead and
    /// guessed how long they took (#93, #119 and five more).
    Wait(Duration),
    /// `<Now>` applies to the next step that is not a `<Wait:N>`: that step goes
    /// in without waiting for what came before to settle, and the waits before
    /// it do not wait for settle either. For the rows that measure something
    /// halfway: a trash still running (12.14), two keys inside the preview's
    /// 40 ms debounce (47.2) (Q41).
    ///
    /// A timed read is `x<Now><State:a><Now><Wait:150><State:b>`: `a` as soon
    /// as `x` is drawn, `b` about 150 ms after `a`. Each `<Now>` is spent by
    /// one step, so in `<Now><State:a><Wait:150><State:b>` the read `b` still
    /// waits for things to settle. `<Now>` does not skip the check that the
    /// last input was drawn, nor an app's wait for its config writes, nor the
    /// rule that a reading of the tree needs a frame drawn after the last
    /// action. So N is a lower bound, not a promise: [`Runner::since_action`]
    /// and [`since_action_line`] record when a reading was really taken.
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
    /// would. **A script that opens a window must end with it**: without a
    /// `<Quit>` the last step ends the script but not the app. `q` is a key like any other, and the compare view, the pane
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
    /// `<Preedit:にほん>`: an input method's composition so far, as the platform's preedit event (`Event::Ime(Preedit)`),
    /// with the escapes of `Step::PasteText`. `<Preedit:>` ends it. For `--keys` to drive a Japanese input line.
    Preedit(String),
    /// `<Commit:日本>`: what the input method settled on, as the commit event, with the same escapes.
    Commit(String),
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
    /// `<DClick:0.5,0.4>`: the left button pressed and let go twice at that place, close enough together to be egui's
    /// `double_clicked()` however slow the frames are (two `<Click:>` steps in a row are only that by luck). `C-` / `A-` / `S-`
    /// hold a key as for `<Click:>`.
    DClick {
        mods: u8,
        at: At,
    },
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
    /// `<Size:480x600>`: the window's inner size in points, for the narrow-window
    /// rows that no pointer step can reach (the harness had no way to resize).
    Size {
        w: u16,
        h: u16,
    },
    /// `<Theme:name>`: switch to that theme. How is the app's to say (it may write
    /// `common.toml` and let its watcher reload, or set its own state), so the
    /// run hands the name back ([`Do::Theme`], [`Press::Theme`]).
    Theme(String),
    /// `<Lang:ja>`: likewise for the language (`en`, `ja`, ...).
    Lang(String),
    /// `<Click:Copy>`, `<RClick:Copy>`: the widget whose label is that text (the
    /// exact label, else the one label that contains it), clicked at its centre.
    /// Read from egui's AccessKit tree, so a popup that has just opened can be
    /// clicked by what it says. Missing or ambiguous text fails the run (#98).
    ClickText {
        right: bool,
        text: String,
    },
    /// `<DClick:Copy>`: a double click at the centre of the widget with that text, which is looked up once.
    DClickText(String),
    /// `<Assert:text>` / `<Assert:!text>`: some label or value in the window
    /// contains the text (or none does). Waits a few seconds for it, then fails the run.
    Assert {
        text: String,
        present: bool,
    },
    /// `<Clipboard:text>`: the last text the app copied after the last action is exactly this. Read from egui's
    /// `CopyText` output, so no OS clipboard is touched; an app that copies by its own path calls [`note_copy`]. An
    /// action is a step that puts an input in: keys, text, `<Paste>`, `<Size:>`, `<Theme:>`, `<Lang:>` and the pointer
    /// steps. `<Wait>`, `<Now>`, `<State>`, `<Shot>` and the reading steps are not actions. Before the first action,
    /// copies since the run began count.
    Clipboard(String),
    /// `<ClipboardHas:text>`: the last text the app copied after the last action contains this (a path or a state inside
    /// a longer line). Actions are as for [`Step::Clipboard`].
    ClipboardHas(String),
    /// `<Enabled:text>`, `<Disabled:text>`, `<Focused:text>`, `<Checked:text>`, `<Unchecked:text>`: the one thing with that
    /// text (as `<Click:text>` finds it) is in that state. Waits a few seconds, like `<Assert:>`.
    Is {
        prop: Prop,
        text: String,
    },
    /// `<Tree:name>`: write the window's tree to `name.txt` beside the done file (see [`tree_text`]).
    Tree(String),
    /// `<Announced:text>`: a message `ito_a11y::announce`d after the last action contains the text. Actions are as for
    /// [`Step::Clipboard`]. Waits like `<Assert:>`.
    Announced(String),
    /// `<ClickId:name>`: press at the centre of the widget with that test ID (see `ito_a11y::test_id`), once it can
    /// be clicked (shown, enabled, not covered, not behind a modal). This guarantees only that the press lands on that
    /// point of that layer; it does not prove the widget took it. A script therefore has to follow it with a check of
    /// what the click did (`<FocusedId:>`, `<ValueId:>`, `<Id:>` or `<Id:!>`), which the parser enforces. A
    /// postcondition that was already true before the click proves nothing -- pick one the click changes.
    ClickId(String),
    /// `<Id:name>`, `<Id:!name>`, `<FocusedId:name>`, `<EnabledId:name>`, `<DisabledId:name>`, `<ValueId:name=text>`:
    /// a check on the widget with that test ID. Waits a few seconds, like `<Assert:>`.
    IdCheck(IdCheck),
    /// `<ReadHas:NAME=TEXT>`: what the app reads under NAME (a tag of its [`Dialect::reads`]) contains the text. Waits
    /// up to [`READ_WAIT`]. Not an operation: it needs no AccessKit tree and does not move the observation generation.
    ReadHas {
        tag: String,
        text: String,
    },
}

/// A check on a widget found by its test ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdCheck {
    /// `<Id:name>`: the widget is in the window and not hidden.
    Present(String),
    /// `<Id:!name>`: no widget has the ID (a hidden one is still there).
    Absent(String),
    /// `<FocusedId:name>`
    Focused(String),
    /// `<EnabledId:name>`
    Enabled(String),
    /// `<DisabledId:name>`
    Disabled(String),
    /// `<ValueId:name=text>`: the widget's value is exactly the text. `<ValueId:name=>` passes for an empty value and
    /// for a widget with no value at all.
    Value { name: String, text: String },
}

impl IdCheck {
    /// The test ID the check is about.
    pub fn name(&self) -> &str {
        match self {
            IdCheck::Present(n) | IdCheck::Absent(n) | IdCheck::Focused(n) | IdCheck::Enabled(n) | IdCheck::Disabled(n) => n,
            IdCheck::Value { name, .. } => name,
        }
    }

    /// Whether `obs` shows what the step expects: `Ok(())`, or why not (the runner retries until [`TEXT_WAIT`]).
    /// A name with a problem (a duplicate, two names on one widget) fails every kind, `Absent` included.
    pub fn check(&self, obs: &Observation) -> Result<(), String> {
        let name = self.name();
        let found = obs.id(name)?;
        if let IdCheck::Absent(_) = self {
            return if found.is_some() { Err(format!("`{name}` is still there")) } else { Ok(()) };
        }
        let Some(node) = found else { return Err(format!("no widget has the test ID `{name}`")) };
        match self {
            IdCheck::Present(_) if node.hidden => Err(format!("`{name}` is hidden")),
            IdCheck::Present(_) | IdCheck::Absent(_) => Ok(()),
            IdCheck::Focused(_) if node.focused => Ok(()),
            IdCheck::Focused(_) => Err(format!("`{name}` is not focused")),
            IdCheck::Enabled(_) if node.enabled => Ok(()),
            IdCheck::Enabled(_) => Err(format!("`{name}` is disabled")),
            IdCheck::Disabled(_) if !node.enabled => Ok(()),
            IdCheck::Disabled(_) => Err(format!("`{name}` is enabled")),
            IdCheck::Value { text, .. } if node.value.as_deref().unwrap_or("") == text => Ok(()),
            IdCheck::Value { text, .. } => Err(format!("`{name}` holds `{}`, not `{text}`", node.value.as_deref().unwrap_or("(no value)"))),
        }
    }

    fn token(&self) -> String {
        match self {
            IdCheck::Present(n) => format!("<Id:{n}>"),
            IdCheck::Absent(n) => format!("<Id:!{n}>"),
            IdCheck::Focused(n) => format!("<FocusedId:{n}>"),
            IdCheck::Enabled(n) => format!("<EnabledId:{n}>"),
            IdCheck::Disabled(n) => format!("<DisabledId:{n}>"),
            IdCheck::Value { name, text } => format!("<ValueId:{name}={}>", paste_text_escape(text)),
        }
    }
}

/// What `<Enabled:>` and its kin ask about a thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    Enabled,
    Disabled,
    Focused,
    Checked,
    Unchecked,
}

impl Prop {
    const ALL: [Prop; 5] = [Prop::Enabled, Prop::Disabled, Prop::Focused, Prop::Checked, Prop::Unchecked];

    /// The word in the token: `Enabled` in `<Enabled:text>`.
    pub fn name(self) -> &'static str {
        match self {
            Prop::Enabled => "Enabled",
            Prop::Disabled => "Disabled",
            Prop::Focused => "Focused",
            Prop::Checked => "Checked",
            Prop::Unchecked => "Unchecked",
        }
    }
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
    DClick {
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
    Size {
        w: u16,
        h: u16,
    },
    Theme(String),
    Lang(String),
    /// Steps that read the window's tree or the last copy: see [`Tree`], [`Step::ClickText`].
    ClickText {
        right: bool,
        text: String,
    },
    DClickText(String),
    Assert {
        text: String,
        present: bool,
    },
    Clipboard(String),
    ClipboardHas(String),
    Is {
        prop: Prop,
        text: String,
    },
    Tree(String),
    Announced(String),
    ClickId(String),
    IdCheck(IdCheck),
    ReadHas {
        tag: String,
        text: String,
    },
}

/// A step as the frame loop takes it; `None` for a key no keyboard can type.
pub fn press(step: &Step) -> Option<Press> {
    match step {
        Step::Key(k) if k.code == Code::Char('v') && k.ctrl && !k.alt && !k.sup => Some(Press::Paste),
        Step::Key(k) => clipboard_chord(k).or_else(|| events(k)).map(Press::Events),
        Step::Paste => Some(Press::Paste),
        Step::PasteText(t) => Some(Press::PasteText(t.clone())),
        Step::Preedit(t) => Some(Press::Events(vec![preedit_event(t)])),
        Step::Commit(t) => Some(Press::Events(vec![commit_event(t)])),
        Step::Read { tag, name } => Some(Press::Read { tag: tag.clone(), name: name.clone() }),
        Step::Click { right, mods, at } => Some(Press::Click { right: *right, mods: *mods, at: *at }),
        Step::DClick { mods, at } => Some(Press::DClick { mods: *mods, at: *at }),
        Step::Hover { at } => Some(Press::Hover { at: *at }),
        Step::Wheel { milli, mods, at } => Some(Press::Wheel { milli: *milli, mods: *mods, at: *at }),
        Step::Drag { from, to } => Some(Press::Drag { from: *from, to: *to }),
        Step::Raw { key, mods } => {
            let modifiers = modifiers(*mods);
            let press = |pressed| egui::Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers };
            Some(Press::Events(vec![press(true), press(false)]))
        }
        Step::Size { w, h } => Some(Press::Size { w: *w, h: *h }),
        Step::Theme(name) => Some(Press::Theme(name.clone())),
        Step::Lang(code) => Some(Press::Lang(code.clone())),
        Step::ClickText { right, text } => Some(Press::ClickText { right: *right, text: text.clone() }),
        Step::DClickText(text) => Some(Press::DClickText(text.clone())),
        Step::Assert { text, present } => Some(Press::Assert { text: text.clone(), present: *present }),
        Step::Clipboard(text) => Some(Press::Clipboard(text.clone())),
        Step::ClipboardHas(text) => Some(Press::ClipboardHas(text.clone())),
        Step::Is { prop, text } => Some(Press::Is { prop: *prop, text: text.clone() }),
        Step::Tree(name) => Some(Press::Tree(name.clone())),
        Step::Announced(text) => Some(Press::Announced(text.clone())),
        Step::ClickId(name) => Some(Press::ClickId(name.clone())),
        Step::IdCheck(check) => Some(Press::IdCheck(check.clone())),
        Step::ReadHas { tag, text } => Some(Press::ReadHas { tag: tag.clone(), text: text.clone() }),
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
        Step::Preedit(t) => format!("<Preedit:{}>", paste_text_escape(t)),
        Step::Commit(t) => format!("<Commit:{}>", paste_text_escape(t)),
        Step::Read { tag, name } => format!("<{tag}:{name}>"),
        Step::Click { right, mods, at } => format!("<{}Click:{}{}>", if *right { "R" } else { "" }, held_text(*mods), at_text(*at)),
        Step::DClick { mods, at } => format!("<DClick:{}{}>", held_text(*mods), at_text(*at)),
        Step::Hover { at } => format!("<Hover:{}>", at_text(*at)),
        Step::Wheel { milli, mods, at } => {
            format!("<Wheel:{}{}@{}>", held_text(*mods), *milli as f32 / 1000.0, at_text(*at))
        }
        Step::Drag { from, to } => format!("<Drag:{}-{}>", at_text(*from), at_text(*to)),
        Step::Raw { key, mods } => format!("<Key:{}{}>", held_text(*mods), key.name()),
        Step::Size { w, h } => format!("<Size:{w}x{h}>"),
        Step::Theme(name) => format!("<Theme:{name}>"),
        Step::Lang(code) => format!("<Lang:{code}>"),
        Step::ClickText { right, text } => format!("<{}Click:{}>", if *right { "R" } else { "" }, paste_text_escape(text)),
        Step::DClickText(text) => format!("<DClick:{}>", paste_text_escape(text)),
        Step::Assert { text, present } => {
            let text = paste_text_escape(text);
            match (present, text.strip_prefix('!')) {
                (false, _) => format!("<Assert:!{text}>"),
                (true, Some(rest)) => format!("<Assert:\\x21{rest}>"),
                (true, None) => format!("<Assert:{text}>"),
            }
        }
        Step::Clipboard(text) => format!("<Clipboard:{}>", paste_text_escape(text)),
        Step::ClipboardHas(text) => format!("<ClipboardHas:{}>", paste_text_escape(text)),
        Step::Is { prop, text } => format!("<{}:{}>", prop.name(), paste_text_escape(text)),
        Step::Tree(name) => format!("<Tree:{name}>"),
        Step::Announced(text) => format!("<Announced:{}>", paste_text_escape(text)),
        Step::ClickId(name) => format!("<ClickId:{name}>"),
        Step::IdCheck(check) => check.token(),
        Step::ReadHas { tag, text } => format!("<ReadHas:{tag}={}>", paste_text_escape(text)),
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
            // Only digits, `.` and `,` is a place (a mistyped one keeps the place error); anything else names a widget.
            if !place.is_empty() && place.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
                at(place).map(|at| Step::Click { right: name == "RClick", mods, at }).ok_or_else(bad)
            } else {
                search_text(token, name).map(|text| Step::ClickText { right: name == "RClick", text })
            }
        }
        "DClick" => {
            let (mods, place) = held_prefix(arg);
            if !place.is_empty() && place.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
                at(place).map(|at| Step::DClick { mods, at }).ok_or_else(bad)
            } else {
                search_text(token, name).map(Step::DClickText)
            }
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
        Press::DClick { mods, at } => {
            let (pos, button, modifiers) = (place(*at), egui::PointerButton::Primary, modifiers(*mods));
            let pair = [true, false].map(|pressed| egui::Event::PointerButton { pos, button, pressed, modifiers });
            let mut events = vec![egui::Event::PointerMoved(pos)];
            events.extend(pair.clone());
            events.extend(pair);
            events
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

/// `<Preedit:text>`: the composition so far, with no part of it marked active.
pub fn preedit_event(text: &str) -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Preedit { text: text.to_owned(), active_range_chars: None })
}

/// `<Commit:text>`: the text the input method settled on.
pub fn commit_event(text: &str) -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Commit(text.to_owned()))
}

/// How long `<Click:text>` and `<Assert:text>` keep looking for the text before the run fails: a popup
/// opens a frame or two after the click that asked for it.
pub const TEXT_WAIT: Duration = Duration::from_secs(3);
/// How long an input the runner put in may wait to be drawn by the main window before the run fails.
pub const DELIVER_WAIT: Duration = Duration::from_secs(10);
/// How long `<ReadHas:NAME=TEXT>` keeps reading before the run fails (a shell in a pane answers at its own pace).
pub const READ_WAIT: Duration = Duration::from_secs(15);

/// A labelled thing in the window, as egui's AccessKit tree names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Its label, or its value (a text field's contents).
    pub text: String,
    /// Where it is, in points.
    pub rect: egui::Rect,
    pub disabled: bool,
    /// The node holds the keyboard focus.
    pub focused: bool,
    /// A check box, switch or toggle button: whether it is on.
    pub toggled: Option<bool>,
}

/// What [`observe`] collects after each pass.
#[derive(Default)]
struct Tap {
    items: Vec<Item>,
    /// The last text copied, and how many copies (`CopyText` commands, any viewport) have been seen, this one included.
    /// [`last_copy`] adds the ones given to [`note_copy`].
    copied: Option<String>,
    copies: u64,
    /// The stamp of the last copy, to order it against [`note_copy`] (see [`COPY_STAMP`]).
    copied_stamp: u64,
    /// The tree, one line per named node, indented by depth (see [`tree_text`]).
    tree: Vec<String>,
    /// Counts main-window passes that brought a tree.
    generation: u64,
    /// The last main-window pass brought a tree.
    fresh: bool,
    pass_nr: u64,
    viewport_rect: Option<egui::Rect>,
    minimized: Option<bool>,
    focused: Option<bool>,
    /// The viewports of the passes begun and not yet ended (a pass ends in `output_hook`).
    passes: Vec<egui::ViewportId>,
    /// The test IDs of the last main-window pass that brought a tree (see [`Observation::id`]).
    ids: BTreeMap<String, Result<IdNode, String>>,
}

/// A widget found by its test ID (`ito_a11y::test_id`), as the last main-window pass left it.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct IdNode {
    /// The test ID.
    pub name: String,
    /// The AccessKit role, as its `Debug` name (`Button`, `TextInput`, ...).
    pub role: String,
    /// Where it is, in points, in window coordinates (the layer's transform applied).
    pub rect: egui::Rect,
    /// Where egui takes a click for it, in window coordinates.
    pub interact_rect: egui::Rect,
    /// The layer it is drawn on.
    pub layer: egui::LayerId,
    /// Enabled, as the widget said and as the tree says.
    pub enabled: bool,
    /// It holds the keyboard focus.
    pub focused: bool,
    /// Its value (a text field's contents), if the tree has one.
    pub value: Option<String>,
    /// The tree hides it.
    pub hidden: bool,
    /// The point to press to click it, or why it cannot be clicked now. This guarantees only that a press there lands
    /// on that point of that layer (inside the window, not covered, not behind a modal); it does not prove the widget
    /// takes it.
    pub click_at: Result<egui::Pos2, String>,
}

/// Join the test IDs a pass recorded with the author IDs in its tree. A name is good only when exactly one node and
/// one entry carry it, they are the same widget, and no other name is on that widget.
fn join_ids(
    ctx: &egui::Context,
    entries: Vec<ito_a11y::TestIdEntry>,
    update: &egui::accesskit::TreeUpdate,
    viewport_rect: egui::Rect,
) -> BTreeMap<String, Result<IdNode, String>> {
    let mut by_name_nodes: BTreeMap<&str, Vec<egui::accesskit::NodeId>> = BTreeMap::new();
    for (id, node) in &update.nodes {
        if let Some(name) = node.author_id() {
            by_name_nodes.entry(name).or_default().push(*id);
        }
    }
    let mut by_name_entries: BTreeMap<&str, Vec<&ito_a11y::TestIdEntry>> = BTreeMap::new();
    for entry in &entries {
        by_name_entries.entry(entry.name.as_str()).or_default().push(entry);
    }
    let mut names: Vec<&str> = by_name_nodes.keys().chain(by_name_entries.keys()).copied().collect();
    names.sort_unstable();
    names.dedup();
    let mut out = BTreeMap::new();
    for name in names {
        let nodes = by_name_nodes.get(name).map(Vec::as_slice).unwrap_or_default();
        let named = by_name_entries.get(name).map(Vec::as_slice).unwrap_or_default();
        let result = (|| {
            if nodes.len() > 1 || named.len() > 1 {
                let ids: Vec<_> = if nodes.len() > 1 { nodes.to_vec() } else { named.iter().map(|e| e.egui_id.accesskit_id()).collect() };
                if nodes.len() <= 1 && ids.iter().all(|i| *i == ids[0]) {
                    return Err(format!("the test ID `{name}` is given {} times to one widget ({:?})", ids.len(), ids[0]));
                }
                return Err(format!("the test ID `{name}` is on {} widgets ({ids:?})", ids.len()));
            }
            if let [entry] = named {
                if let Some(other) = entries.iter().find(|o| o.name != name && o.egui_id == entry.egui_id) {
                    let id = entry.egui_id.accesskit_id();
                    let (a, b) = if name < other.name.as_str() { (name, other.name.as_str()) } else { (other.name.as_str(), name) };
                    return Err(format!("the test IDs `{a}` and `{b}` are on the same widget ({id:?}); the last one wins in the tree"));
                }
            }
            let (Some(&node_id), [entry]) = (nodes.first(), named) else {
                return Err(match (nodes.first(), named.first()) {
                    (Some(_), None) => format!("the test ID `{name}` is in the tree but was not set with ito_a11y::test_id"),
                    (None, Some(e)) => {
                        format!("the test ID `{name}` was set but its node is not in the tree ({:?})", e.egui_id.accesskit_id())
                    }
                    _ => unreachable!(),
                });
            };
            let want = entry.egui_id.accesskit_id();
            if node_id != want {
                return Err(format!("the test ID `{name}` was set but its node is not in the tree ({want:?})"));
            }
            let node = update
                .nodes
                .iter()
                .find(|(id, _)| *id == node_id)
                .map(|(_, n)| n)
                .ok_or_else(|| format!("the test ID `{name}` was set but its node is not in the tree ({want:?})"))?;
            let t = ctx.layer_transform_to_global(entry.layer).unwrap_or_default();
            let (rect, interact_rect) = (t * entry.rect, t * entry.interact_rect);
            let enabled = entry.enabled && !node.is_disabled();
            let hidden = node.is_hidden();
            let click_at = click_point(ctx, name, interact_rect, entry.layer, enabled, hidden, viewport_rect);
            Ok(IdNode {
                name: name.to_owned(),
                role: format!("{:?}", node.role()),
                rect,
                interact_rect,
                layer: entry.layer,
                enabled,
                focused: node_id == update.focus,
                value: node.value().map(String::from),
                hidden,
                click_at,
            })
        })();
        out.insert(name.to_owned(), result);
    }
    out
}

/// The point to press to click the widget, or the first reason it cannot be clicked.
fn click_point(
    ctx: &egui::Context,
    name: &str,
    r: egui::Rect,
    layer: egui::LayerId,
    enabled: bool,
    hidden: bool,
    viewport_rect: egui::Rect,
) -> Result<egui::Pos2, String> {
    if hidden {
        return Err(format!("`{name}` is hidden"));
    }
    if !enabled {
        return Err(format!("`{name}` is disabled"));
    }
    if !(r.min.x.is_finite() && r.min.y.is_finite() && r.max.x.is_finite() && r.max.y.is_finite()) || r.width() <= 0.0 || r.height() <= 0.0
    {
        return Err(format!("`{name}` has no area to click ({r:?})"));
    }
    let c = r.center();
    if !viewport_rect.contains(c) {
        return Err(format!("`{name}` is outside the window (at {c:?}, window {viewport_rect:?})"));
    }
    if !ctx.memory(|m| m.allows_interaction(layer)) {
        return Err(format!("`{name}` is behind a modal window or cannot be clicked ({layer:?})"));
    }
    match ctx.layer_id_at(c) {
        Some(l) if l == layer => Ok(c),
        other => Err(format!("`{name}` is covered by {other:?} at {c:?}")),
    }
}

impl egui::Plugin for Tap {
    fn debug_name(&self) -> &'static str {
        "ito-keyscript"
    }

    fn input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.passes.len() > 64 {
            self.passes.clear();
        }
        self.passes.push(raw.viewport_id);
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        for command in &output.platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                self.copied = Some(text.clone());
                self.copies += 1;
                self.copied_stamp = next_copy_stamp();
            }
        }
        // `Context::viewport_id` is already the parent's here, so the pass's own viewport is the one `input_hook` saw.
        // Nothing on the stack: the plugin was added in the middle of this pass, and whose it is is not known.
        if self.passes.pop() != Some(egui::ViewportId::ROOT) {
            return;
        }
        // The IDs a pass records are taken on every main-window pass, so none is left over for a later one.
        let entries = ito_a11y::take_test_ids(ctx, egui::ViewportId::ROOT);
        let Some(update) = &output.platform_output.accesskit_update else {
            self.items.clear();
            self.tree.clear();
            self.ids.clear();
            self.fresh = false;
            return;
        };
        // egui writes the whole tree each pass, so the last one is the window as it is.
        let mut items: Vec<Item> = Vec::new();
        let nodes: std::collections::HashMap<_, _> = update.nodes.iter().map(|(id, n)| (*id, n)).collect();
        let mut tree = Vec::new();
        if let Some(root) = update.tree.as_ref().map(|t| t.root) {
            dump(&nodes, root, update.focus, 0, &mut tree);
        }
        self.tree = tree;
        for (id, node) in &update.nodes {
            let focused = *id == update.focus && update.focus != update.tree.as_ref().map_or(update.focus, |t| t.root);
            let toggled = toggled(node);
            let Some(b) = node.bounds().filter(|_| !node.is_hidden()) else { continue };
            let rect = egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32));
            for text in [node.label(), node.value()].into_iter().flatten().filter(|t| !t.is_empty()) {
                let item = Item { text: text.to_owned(), rect, disabled: node.is_disabled(), focused, toggled };
                if !items.contains(&item) {
                    items.push(item);
                }
            }
        }
        self.items = items;
        self.generation += 1;
        self.fresh = true;
        self.pass_nr = ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT);
        let (info, rect) = ctx.input_for(egui::ViewportId::ROOT, |i| (i.viewport().clone(), i.viewport_rect()));
        // `cumulative_pass_nr_for` has counted this pass already, so a widget that recorded N is in pass N + 1.
        let pass = self.pass_nr;
        let entries: Vec<_> = entries.into_iter().filter(|e| e.pass + 1 == pass).collect();
        self.ids = join_ids(ctx, entries, update, rect);
        self.viewport_rect = Some(rect);
        self.minimized = info.minimized;
        self.focused = info.focused;
    }
}

/// What [`observe`] saw in the main window's last pass.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct Observation {
    /// Counts up by one for each main-window pass that brought a tree; never goes back.
    pub generation: u64,
    pub items: Vec<Item>,
    /// One line per named node (the body of [`tree_text`], without announcements).
    pub tree: Vec<String>,
    /// `cumulative_pass_nr_for(ROOT)` after the pass that produced it.
    pub pass_nr: u64,
    pub viewport: egui::ViewportId,
    pub viewport_rect: egui::Rect,
    pub minimized: Option<bool>,
    pub focused: Option<bool>,
    /// The widgets with a test ID (`ito_a11y::test_id`), by name; an `Err` is a name with a problem (a duplicate, two
    /// names on one widget). Empty while AccessKit is off.
    pub ids: BTreeMap<String, Result<IdNode, String>>,
}

impl Observation {
    /// The widget with that test ID: `Ok(None)` when no widget has it, `Err` when the name has a problem.
    pub fn id(&self, name: &str) -> Result<Option<&IdNode>, String> {
        match self.ids.get(name) {
            None => Ok(None),
            Some(Ok(node)) => Ok(Some(node)),
            Some(Err(e)) => Err(e.clone()),
        }
    }
}

/// Copies the app gave to [`note_copy`].
#[derive(Default)]
struct Noted {
    text: Option<String>,
    count: u64,
    stamp: u64,
}

/// Set by [`observe`]; until then [`note_copy`] keeps nothing.
static NOTING: AtomicBool = AtomicBool::new(false);
static NOTED: Mutex<Noted> = Mutex::new(Noted { text: None, count: 0, stamp: 0 });
/// Every recorded copy, from the `Tap` or from [`note_copy`], takes the next number, so the two kinds can be ordered.
static COPY_STAMP: AtomicU64 = AtomicU64::new(0);

fn next_copy_stamp() -> u64 {
    COPY_STAMP.fetch_add(1, Ordering::SeqCst) + 1
}

/// Record that the app copied `text` by its own path (not through egui's `CopyText`, e.g. straight to the OS clipboard),
/// so `<Clipboard:>` and `<ClipboardHas:>` see it like any other copy. Pass the text as the app means it, before any
/// change of line endings for the OS. Callable from any thread, with no `egui::Context`. Records only after
/// [`observe`] was called (a `--keys` run, or an app's own loop that calls it); otherwise it does nothing, so a shipped
/// app keeps nothing. The record is process-wide: every window's [`mark`] counts it.
pub fn note_copy(text: &str) {
    note_copy_when(NOTING.load(Ordering::SeqCst), text);
}

/// [`note_copy`] with the switch passed in, so a test does not race other tests that call [`observe`].
fn note_copy_when(noting: bool, text: &str) {
    if !noting {
        return;
    }
    let mut noted = NOTED.lock().unwrap_or_else(|e| e.into_inner());
    noted.text = Some(text.to_owned());
    noted.count += 1;
    noted.stamp = next_copy_stamp();
}

/// Start reading the window's tree and what the app copies, from the next frame on. `Runner` does this
/// itself when the script needs it; an app with its own frame loop calls it once. Costs nothing
/// until called (egui builds no tree otherwise). It also starts recording the copies given to [`note_copy`].
pub fn observe(ctx: &egui::Context) {
    NOTING.store(true, Ordering::SeqCst);
    ctx.enable_accesskit();
    ctx.add_plugin(Tap::default());
}

/// The current generation: 0 before [`observe`] (or before its first tree). Readable at any time; it only goes up.
pub fn generation(ctx: &egui::Context) -> u64 {
    ctx.with_plugin::<Tap, _>(|t| t.generation).unwrap_or(0)
}

/// The main window's last observation, if the last main-window pass brought a tree; `None` otherwise.
pub fn observation(ctx: &egui::Context) -> Option<Observation> {
    ctx.with_plugin::<Tap, _>(|t| {
        t.fresh.then(|| Observation {
            generation: t.generation,
            items: t.items.clone(),
            tree: t.tree.clone(),
            pass_nr: t.pass_nr,
            viewport: egui::ViewportId::ROOT,
            viewport_rect: t.viewport_rect.unwrap_or(egui::Rect::NOTHING),
            minimized: t.minimized,
            focused: t.focused,
            ids: t.ids.clone(),
        })
    })
    .flatten()
}

/// The labelled things as of the last main-window pass; empty before [`observe`] has seen a tree, and empty again
/// after a main-window pass that brought none. Passes of child windows are not observed.
pub fn items(ctx: &egui::Context) -> Vec<Item> {
    ctx.with_plugin::<Tap, _>(|t| t.items.clone()).unwrap_or_default()
}

fn toggled(node: &egui::accesskit::Node) -> Option<bool> {
    use egui::accesskit::Toggled;
    node.toggled().map(|t| t == Toggled::True)
}

/// The role as a word, in the vocabulary of the tree file and of `ito-a11y` (`button`, `switch`, `text`, ...).
fn role_name(role: egui::accesskit::Role) -> String {
    use egui::accesskit::Role;
    match role {
        Role::Button => "button".into(),
        Role::CheckBox => "checkbox".into(),
        Role::Switch => "switch".into(),
        Role::RadioButton => "radio".into(),
        Role::TextInput | Role::MultilineTextInput | Role::SearchInput => "text".into(),
        Role::Label | Role::TextRun => "label".into(),
        Role::Heading => "heading".into(),
        Role::ListItem => "row".into(),
        other => format!("{other:?}").to_lowercase(),
    }
}

fn dump(
    nodes: &std::collections::HashMap<egui::accesskit::NodeId, &egui::accesskit::Node>,
    id: egui::accesskit::NodeId,
    focus: egui::accesskit::NodeId,
    depth: usize,
    out: &mut Vec<String>,
) {
    let Some(node) = nodes.get(&id) else { return };
    if node.is_hidden() {
        return;
    }
    let label = node.label().filter(|t| !t.is_empty());
    let value = node.value().filter(|t| !t.is_empty());
    let toggle = toggled(node);
    let named = label.is_some() || value.is_some() || toggle.is_some();
    let mut child_depth = depth;
    if named {
        let mut line = format!("{}{}", "  ".repeat(depth), role_name(node.role()));
        if let Some(l) = label {
            line.push_str(&format!(" \"{}\"", paste_text_escape(l)));
        }
        if let Some(v) = value {
            line.push_str(&format!(" value=\"{}\"", paste_text_escape(v)));
        }
        if id == focus {
            line.push_str(" focused");
        }
        if node.is_disabled() {
            line.push_str(" disabled");
        }
        match toggle {
            Some(true) => line.push_str(" checked"),
            Some(false) => line.push_str(" unchecked"),
            None => {}
        }
        out.push(line);
        child_depth += 1;
    }
    for child in node.children() {
        dump(nodes, *child, focus, child_depth, out);
    }
}

/// `<Tree:name>`'s file: the window as the AccessKit tree has it, one line per named node, indented under its parent:
/// `button "確かめた" focused`, `switch "…" checked`, `text "Name" value="…"`. Then the messages `announce`d, as
/// `announced: …` lines.
pub fn tree_text(ctx: &egui::Context) -> String {
    let tree = ctx.with_plugin::<Tap, _>(|t| t.tree.clone()).unwrap_or_default();
    tree_file(&tree)
}

/// `lines` of the tree, then the `announced:` lines.
fn tree_file(lines: &[String]) -> String {
    let mut s = lines.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    for m in ito_a11y::announced() {
        s.push_str(&format!("announced: {}\n", paste_text_escape(&m)));
    }
    s
}

/// `<Enabled:text>` and its kin.
pub fn check_is(ctx: &egui::Context, prop: Prop, text: &str) -> Result<(), String> {
    check_is_in(&items(ctx), prop, text)
}

fn check_is_in(all: &[Item], prop: Prop, text: &str) -> Result<(), String> {
    let exact: Vec<&Item> = all.iter().filter(|i| i.text == text).collect();
    let found: Vec<&Item> = if exact.is_empty() { all.iter().filter(|i| i.text.contains(text)).collect() } else { exact };
    let item = match found.as_slice() {
        [one] => *one,
        [] => return Err(format!("no `{text}` in the window; it has: {}", listed(&all.iter().collect::<Vec<_>>()))),
        // A text field and the text run inside it both say `hi`: the outer one holds the state.
        many if many.iter().any(|o| many.iter().all(|i| o.rect.contains_rect(i.rect))) => {
            many.iter().find(|o| many.iter().all(|i| o.rect.contains_rect(i.rect))).copied().unwrap_or_else(|| unreachable!())
        }
        many => return Err(format!("`{text}` is more than one thing: {}", listed(many))),
    };
    let ok = match prop {
        Prop::Enabled => !item.disabled,
        Prop::Disabled => item.disabled,
        Prop::Focused => item.focused,
        Prop::Checked => item.toggled == Some(true),
        Prop::Unchecked => item.toggled == Some(false),
    };
    if ok {
        return Ok(());
    }
    let is = match (prop, item.toggled, item.disabled, item.focused) {
        (Prop::Checked | Prop::Unchecked, Some(true), ..) => "checked",
        (Prop::Checked | Prop::Unchecked, Some(false), ..) => "unchecked",
        (Prop::Checked | Prop::Unchecked, None, ..) => "not a check box or switch",
        (_, _, true, _) => "disabled",
        (_, _, false, true) => "enabled and focused",
        _ => "enabled and not focused",
    };
    Err(format!("`{}` is not {}: it is {is}", item.text, prop.name().to_lowercase()))
}

/// Where the copies and announcements stood when the last action went in. `<Clipboard:>`, `<ClipboardHas:>` and
/// `<Announced:>` look only after it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    /// How many copies had been recorded (seen by `observe`, or given to [`note_copy`]).
    pub copies: u64,
    /// `ito_a11y::announced_count()`.
    pub announced: u64,
}

/// The mark as of now: the copies recorded (seen by `observe` or given to [`note_copy`]; 0 before) and
/// `ito_a11y::announced_count()`.
pub fn mark(ctx: &egui::Context) -> Mark {
    Mark { copies: last_copy(ctx).1, announced: ito_a11y::announced_count() }
}

/// `<Announced:text>`, over the kept announcements (the last 200 since the run began).
pub fn check_announced(text: &str) -> Result<(), String> {
    let heard = ito_a11y::announced();
    if heard.iter().any(|m| m.contains(text)) {
        return Ok(());
    }
    Err(format!("`{text}` was not announced; announced: {}", last_said(&heard)))
}

/// `<Announced:text>`, over what was announced after `after`.
pub fn check_announced_after(after: Mark, text: &str) -> Result<(), String> {
    let heard = ito_a11y::announced_after(after.announced);
    if heard.iter().any(|m| m.contains(text)) {
        return Ok(());
    }
    // With nothing after the mark, everything kept is from before it.
    let before = if heard.is_empty() { ito_a11y::announced() } else { Vec::new() };
    Err(not_announced_after(text, &heard, &before))
}

/// The last 8 of `said`, newest first, or `nothing`.
fn last_said(said: &[String]) -> String {
    let tail: Vec<String> = said.iter().rev().take(8).map(|m| format!("`{m}`")).collect();
    if tail.is_empty() {
        "nothing".into()
    } else {
        tail.join(", ")
    }
}

/// What `check_announced_after` says: `heard` was announced after the mark, `before` before it (looked at only when
/// `heard` is empty).
fn not_announced_after(text: &str, heard: &[String], before: &[String]) -> String {
    let mut msg = format!("`{text}` was not announced after the last action; announced after it: {}", last_said(heard));
    if heard.is_empty() && !before.is_empty() {
        msg.push_str(&format!(" (before it: {})", last_said(before)));
    }
    msg
}

fn listed(found: &[&Item]) -> String {
    let mut names: Vec<String> = found.iter().take(12).map(|i| format!("`{}`", i.text)).collect();
    if found.len() > 12 {
        names.push(format!("and {} more", found.len() - 12));
    }
    names.join(", ")
}

/// Where `<Click:text>` clicks: the centre of the one thing whose text is `text`, or else the one
/// whose text contains it. An error says what was found instead.
pub fn locate(ctx: &egui::Context, text: &str) -> Result<egui::Pos2, String> {
    locate_in(&items(ctx), text)
}

fn locate_in(all: &[Item], text: &str) -> Result<egui::Pos2, String> {
    let exact: Vec<&Item> = all.iter().filter(|i| i.text == text).collect();
    let found = if exact.is_empty() { all.iter().filter(|i| i.text.contains(text)).collect() } else { exact };
    match found.as_slice() {
        [one] => Ok(one.rect.center()),
        [] => {
            let seen: Vec<&Item> = all.iter().collect();
            Err(format!("no `{text}` in the window; it has: {}", listed(&seen)))
        }
        many => Err(format!("`{text}` is more than one thing: {}", listed(many))),
    }
}

/// `<Assert:text>` (`present`) or `<Assert:!text>`.
pub fn check(ctx: &egui::Context, text: &str, present: bool) -> Result<(), String> {
    check_in(&items(ctx), text, present)
}

fn check_in(all: &[Item], text: &str, present: bool) -> Result<(), String> {
    let found: Vec<&Item> = all.iter().filter(|i| i.text.contains(text)).collect();
    match (present, found.is_empty()) {
        (true, false) | (false, true) => Ok(()),
        (true, true) => Err(format!("no `{text}` in the window; it has: {}", listed(&all.iter().collect::<Vec<_>>()))),
        (false, false) => Err(format!("`{text}` is in the window: {}", listed(&found))),
    }
}

/// The last copy and how many copies there were (0 when nothing was copied), over the `Tap` and [`note_copy`].
fn last_copy(ctx: &egui::Context) -> (Option<String>, u64) {
    let (tapped, tap_count, tap_stamp) =
        ctx.with_plugin::<Tap, _>(|t| (t.copied.clone(), t.copies, t.copied_stamp)).unwrap_or((None, 0, 0));
    let noted = NOTED.lock().unwrap_or_else(|e| e.into_inner());
    let count = tap_count + noted.count;
    let text = if noted.text.is_some() && noted.stamp > tap_stamp { noted.text.clone() } else { tapped };
    (text, count)
}

/// What a clipboard check says when there is no newer copy: nothing at all, or only one from before the mark.
fn no_newer_copy(want: &str, before: Option<String>) -> String {
    let want = paste_text_escape(want);
    match before {
        Some(old) => {
            format!("nothing copied after the last action, not `{want}` (the last copy, before it, was `{}`)", paste_text_escape(&old))
        }
        None => format!("nothing copied yet, not `{want}`"),
    }
}

/// `<Clipboard:text>`: the last text the app copied is exactly `expect`.
pub fn check_clipboard(ctx: &egui::Context, expect: &str) -> Result<(), String> {
    check_clipboard_after(ctx, Mark::default(), expect)
}

/// `<Clipboard:text>`, over the copies made after `after`.
pub fn check_clipboard_after(ctx: &egui::Context, after: Mark, expect: &str) -> Result<(), String> {
    match last_copy(ctx) {
        (Some(got), n) if n > after.copies => {
            if got == expect {
                Ok(())
            } else {
                Err(format!("copied `{}`, not `{}`", paste_text_escape(&got), paste_text_escape(expect)))
            }
        }
        (before, _) => Err(no_newer_copy(expect, before)),
    }
}

/// `<ClipboardHas:text>`: the last text the app copied contains `part`.
pub fn check_clipboard_has(ctx: &egui::Context, part: &str) -> Result<(), String> {
    check_clipboard_has_after(ctx, Mark::default(), part)
}

/// `<ClipboardHas:text>`, over the copies made after `after`.
pub fn check_clipboard_has_after(ctx: &egui::Context, after: Mark, part: &str) -> Result<(), String> {
    match last_copy(ctx) {
        (Some(got), n) if n > after.copies => {
            if got.contains(part) {
                Ok(())
            } else {
                Err(format!("copied `{}`, which has no `{}`", paste_text_escape(&got), paste_text_escape(part)))
            }
        }
        (before, _) => Err(no_newer_copy(part, before)),
    }
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
        let report = with_current_run(&panic_report(info));
        let _ = std::fs::write(&done, &report);
        let mut named = done.clone().into_os_string();
        named.push(".panic");
        let _ = std::fs::write(std::path::PathBuf::from(named), &report);
        before(info);
        std::process::exit(101);
    }));
}

static RUN_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The run id of this process: `<prefix>_KEYS_RUN_ID` if set (kura: prefix `KURA`), else `<unix ms>-<pid>`. A test harness tells
/// its own run's done file from a stale one by the `run: <id>` line. Decided once per process: the first `Ok` is kept and
/// later calls return it unchanged. A set but malformed value is an `Err` (nothing is kept) that the app turns into a
/// [`refused_report`] before any window opens.
pub fn run_id(prefix: &str) -> Result<String, String> {
    let name = format!("{prefix}_KEYS_RUN_ID");
    if let Some(id) = RUN_ID.get() {
        return Ok(id.clone());
    }
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let id = decide_run_id(std::env::var_os(&name), &name, ms, std::process::id())?;
    Ok(RUN_ID.get_or_init(|| id).clone())
}

/// The id [`run_id`] kept, once it has been called successfully.
pub fn current_run_id() -> Option<&'static str> {
    RUN_ID.get().map(String::as_str)
}

/// `var` is the environment value of `name`. Valid is 1 to 64 of `A-Z a-z 0-9 _ -`; an empty value is set, so invalid.
fn decide_run_id(var: Option<std::ffi::OsString>, name: &str, now_ms: u128, pid: u32) -> Result<String, String> {
    let Some(var) = var else { return Ok(format!("{now_ms}-{pid}")) };
    let text = var.to_string_lossy();
    let ok =
        var.to_str().is_some_and(|t| (1..=64).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
    if ok {
        return Ok(text.into_owned());
    }
    // Escaped: a newline in the value must not add a line (a `run:` one, say) to the refused report.
    let shown: String = text.chars().take(64).flat_map(char::escape_debug).collect();
    Err(format!("{name}: '{shown}' is not a run id (1 to 64 of A-Z a-z 0-9 _ -)"))
}

/// `report` with exactly one `run: <id>` line: any `run:` line it had is dropped, and the new one goes just before a final
/// `keys: done`, else last. So the first line stays what it was (readers look for `keys: stalled` there) and a finished run
/// still ends with `keys: done`.
pub fn with_run(id: &str, report: &str) -> String {
    let mut lines: Vec<&str> = report.lines().filter(|l| !l.starts_with("run: ")).collect();
    let line = format!("run: {id}");
    match lines.last() {
        Some(&"keys: done") => lines.insert(lines.len() - 1, &line),
        _ => lines.push(&line),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// [`with_run`] with [`current_run_id`], or `(none)` before one was decided. For an app's writers and ito's own.
pub fn with_current_run(report: &str) -> String {
    with_run(current_run_id().unwrap_or("(none)"), report)
}

/// What the done file holds for a script that failed on a step that reads the window
/// (`<Assert:>`, `<Click:text>`, `<Clipboard:>`). The app ends with [`FAILED_EXIT`].
pub fn failed_report(step: &str, why: &str, left: usize) -> String {
    format!("keys: failed\nstep: {step}\nwhy: {}\nleft: {left} not pressed\n", why.replace('\n', " / "))
}

/// The exit code of an app whose script failed ([`failed_report`]).
pub const FAILED_EXIT: i32 = 3;

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
    // One line, as in [`failed_report`]: a reader takes the last line as the outcome.
    format!("keys: refused\nwhy: {}\n", why.replace('\n', " / "))
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

/// The text in `<Paste:text>`, `<Preedit:text>` or `<Commit:text>` (`what`); see [`Step::PasteText`] for the escapes.
fn paste_text(token: &str, what: &str) -> Result<String, String> {
    let noun = match what {
        "Paste" => "a paste",
        "Click" | "RClick" | "DClick" | "Assert" | "Clipboard" | "ClipboardHas" | "Enabled" | "Disabled" | "Focused" | "Checked"
        | "Unchecked" | "Announced" | "ValueId" | "ReadHas" => "a text to look for",
        _ => "an input method step",
    };
    let bad = |why: &str| format!("`{token}` is not {noun}: {why}");
    let inner = token.strip_prefix(&format!("<{what}:")).and_then(|t| t.strip_suffix('>')).unwrap_or("");
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

/// [`paste_text`] for a text to look for, which must not be empty: an empty text matches anything, so a check that
/// uses it passes without looking.
fn search_text(token: &str, what: &str) -> Result<String, String> {
    let text = paste_text(token, what)?;
    if text.is_empty() {
        return Err(format!("`{token}` has nothing to look for; an empty text matches anything"));
    }
    Ok(text)
}

const TEST_ID_HELP: &str = "write lowercase letters, digits, - and _, in parts joined by `.`, as `<Id:settings.shell.env.add>`";

/// The test ID in `<{what}:name>`.
fn test_id_arg<'t>(token: &'t str, what: &str) -> Result<&'t str, String> {
    let name = token.strip_prefix(&format!("<{what}:")).and_then(|t| t.strip_suffix('>')).unwrap_or("");
    if ito_a11y::is_test_id(name) {
        return Ok(name);
    }
    Err(format!("`{token}` is not a test ID; {TEST_ID_HELP}"))
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

/// `<Size:480x600>`, `<Theme:name>` and `<Lang:ja>`: the window and settings steps. `None` for a token that is not one.
fn setting(token: &str) -> Option<Result<Step, String>> {
    let inner = token.strip_prefix('<')?.strip_suffix('>')?;
    let (name, arg) = inner.split_once(':')?;
    // A theme or language is a word the app looks up; keep it a plain one.
    let word = |what: &str| {
        let ok = !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if ok {
            Ok(arg.to_owned())
        } else {
            Err(format!("`{token}` is not a {what}; write it as letters, digits, - _ and ., as `<{name}:x>`"))
        }
    };
    Some(match name {
        "Size" => arg
            .split_once('x')
            .and_then(|(w, h)| {
                let side = |v: &str| v.parse::<u16>().ok().filter(|v| (100..=10_000).contains(v));
                Some(Step::Size { w: side(w)?, h: side(h)? })
            })
            .ok_or_else(|| format!("`{token}` is not a size; write the inner size in points from 100 to 10000, as `<Size:480x600>`")),
        "Theme" => word("theme").map(Step::Theme),
        "Lang" => word("language").map(Step::Lang),
        _ => return None,
    })
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
/// `<Now>` sends the next step (past any `<Wait:N>`) without waiting for settle,
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
            None if token.starts_with("<Paste:") => Step::PasteText(paste_text(token, "Paste")?),
            None if token.starts_with("<Preedit:") => Step::Preedit(paste_text(token, "Preedit")?),
            None if token.starts_with("<Commit:") => Step::Commit(paste_text(token, "Commit")?),
            None if token.starts_with("<Assert:") => {
                // The `!` is read before the escapes, so `!` is a literal one.
                let negated = token.starts_with("<Assert:!");
                let inner = if negated { token.replacen("<Assert:!", "<Assert:", 1) } else { token.to_owned() };
                let text = paste_text(&inner, "Assert")?;
                if text.is_empty() {
                    return Err(format!("`{token}` has nothing to look for; an empty text matches anything"));
                }
                Step::Assert { text, present: !negated }
            }
            None if token.starts_with("<Clipboard:") => Step::Clipboard(search_text(token, "Clipboard")?),
            None if token.starts_with("<ClipboardHas:") => Step::ClipboardHas(search_text(token, "ClipboardHas")?),
            None if token.starts_with("<ClickId:") => Step::ClickId(test_id_arg(token, "ClickId")?.to_owned()),
            None if token.starts_with("<Id:!") => {
                let name = token.strip_prefix("<Id:!").and_then(|t| t.strip_suffix('>')).unwrap_or("");
                if !ito_a11y::is_test_id(name) {
                    return Err(format!("`{token}` is not a test ID; {TEST_ID_HELP}"));
                }
                Step::IdCheck(IdCheck::Absent(name.to_owned()))
            }
            None if token.starts_with("<Id:") => Step::IdCheck(IdCheck::Present(test_id_arg(token, "Id")?.to_owned())),
            None if token.starts_with("<FocusedId:") => Step::IdCheck(IdCheck::Focused(test_id_arg(token, "FocusedId")?.to_owned())),
            None if token.starts_with("<EnabledId:") => Step::IdCheck(IdCheck::Enabled(test_id_arg(token, "EnabledId")?.to_owned())),
            None if token.starts_with("<DisabledId:") => Step::IdCheck(IdCheck::Disabled(test_id_arg(token, "DisabledId")?.to_owned())),
            None if token.starts_with("<ValueId:") => {
                let inner = token.strip_prefix("<ValueId:").and_then(|t| t.strip_suffix('>')).unwrap_or("");
                let Some((name, text)) = inner.split_once('=') else {
                    return Err(format!("`{token}` has no value to compare; write it as `<ValueId:name=text>`"));
                };
                if !ito_a11y::is_test_id(name) {
                    return Err(format!("`{token}` is not a test ID; {TEST_ID_HELP}"));
                }
                let text = paste_text(&format!("<ValueId:{text}>"), "ValueId")?;
                Step::IdCheck(IdCheck::Value { name: name.to_owned(), text })
            }
            None if token.starts_with("<ReadHas:") => {
                let inner = token.strip_prefix("<ReadHas:").and_then(|t| t.strip_suffix('>')).unwrap_or("");
                let Some((tag, text)) = inner.split_once('=') else {
                    return Err(format!("`{token}` has no text to look for; write it as `<ReadHas:NAME=TEXT>`"));
                };
                let Some((tag, _)) = dialect.reads.iter().find(|(t, _)| *t == tag) else {
                    let tags: Vec<&str> = dialect.reads.iter().map(|(t, _)| *t).collect();
                    return Err(if tags.is_empty() {
                        format!("`{token}`: this app reads nothing, so `<ReadHas:>` has nothing to read")
                    } else {
                        format!("`{token}`: `{tag}` is not something this app reads; write one of {}", tags.join(", "))
                    });
                };
                let text = search_text(&format!("<ReadHas:{text}>"), "ReadHas")?;
                Step::ReadHas { tag: (*tag).to_owned(), text }
            }
            None if token.starts_with("<Tree:") => Step::Tree(named(token, "Tree", "tree name")?),
            None if token.starts_with("<Announced:") => Step::Announced(search_text(token, "Announced")?),
            None if Prop::ALL.iter().any(|p| token.starts_with(&format!("<{}:", p.name()))) => {
                let prop =
                    Prop::ALL.iter().copied().find(|p| token.starts_with(&format!("<{}:", p.name()))).unwrap_or_else(|| unreachable!());
                Step::Is { prop, text: search_text(token, prop.name())? }
            }
            None if read.is_some() => {
                let (tag, noun) = read.copied().unwrap_or_else(|| unreachable!());
                Step::Read { tag: tag.to_owned(), name: named(token, tag, noun)? }
            }
            None if pointer(token).is_some() => pointer(token).unwrap_or_else(|| unreachable!())?,
            None if setting(token).is_some() => setting(token).unwrap_or_else(|| unreachable!())?,
            None if raw(token).is_some() => raw(token).unwrap_or_else(|| unreachable!())?,
            None => Step::Key(Key::parse(token).ok_or_else(|| format!("`{token}` is not a key"))?),
        };
        rest = &rest[token.len()..];
        // `<C-+>*45`: the key that many times. Only after a `<…>` key, since a
        // bare `*` is a key of its own.
        let mut times = 1;
        let no_times = matches!(
            step,
            Step::Wait(_)
                | Step::Now
                | Step::Quit
                | Step::Shot(_)
                | Step::State(_)
                | Step::Read { .. }
                | Step::ClickId(_)
                | Step::IdCheck(_)
                | Step::ReadHas { .. }
        );
        if matches!(step, Step::ClickId(_) | Step::IdCheck(_) | Step::ReadHas { .. })
            && rest.strip_prefix('*').is_some_and(|after| after.starts_with(|d: char| d.is_ascii_digit()))
        {
            return Err(format!("`{token}` cannot be repeated with `*N`; write it once"));
        }
        if c == '<' && !no_times {
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
    // `<Now>` is about the step after it (past any `<Wait:N>`), so there has to be one.
    for (i, step) in out.iter().enumerate() {
        if *step != Step::Now {
            continue;
        }
        let next = out[i + 1..].iter().find(|s| !matches!(s, Step::Wait(_)));
        if !matches!(next, Some(s) if !matches!(s, Step::Now | Step::Quit)) {
            return Err("`<Now>` has to come right before a key or a reading (`d<Now>w`, `<A-c><Now><State:mid>`), or before `<Wait:N>` and then one (`<Now><Wait:150><State:b>`)"
                .into());
        }
    }
    // A `<ClickId:x>` proves nothing by itself (see `Step::ClickId`), so a check of its result has to come after it.
    for (i, step) in out.iter().enumerate() {
        let Step::ClickId(name) = step else { continue };
        let next = out[i + 1..].iter().find(|s| !matches!(s, Step::Wait(_) | Step::State(_) | Step::Shot(_) | Step::Read { .. }));
        let checked =
            matches!(next, Some(Step::IdCheck(IdCheck::Focused(_) | IdCheck::Value { .. } | IdCheck::Present(_) | IdCheck::Absent(_))));
        if !checked {
            return Err(format!(
                "`<ClickId:{name}>` has to be followed by a check of what it did \u{2014} `<FocusedId:\u{2026}>`, `<ValueId:\u{2026}=\u{2026}>`, `<Id:\u{2026}>` or `<Id:!\u{2026}>`; only `<Wait:>`, `<State:>`, `<Shot:>` and readings may come between"
            ));
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
    /// `<Theme:name>`: switch to it (the app decides how).
    Theme(String),
    /// `<Lang:code>`: switch to it (the app decides how).
    Lang(String),
    /// A step that reads the window found it other than the script said: write
    /// [`failed_report`] and end with [`FAILED_EXIT`].
    Failed {
        step: String,
        why: String,
    },
    /// The last step has gone in.
    Done,
}

/// A click at `pos` as the frames of pointer events it takes.
fn click_frames(pos: egui::Pos2, right: bool) -> VecDeque<Vec<egui::Event>> {
    let button = if right { egui::PointerButton::Secondary } else { egui::PointerButton::Primary };
    let modifiers = egui::Modifiers::NONE;
    VecDeque::from([
        vec![egui::Event::PointerMoved(pos)],
        vec![egui::Event::PointerButton { pos, button, pressed: true, modifiers }],
        vec![egui::Event::PointerButton { pos, button, pressed: false, modifiers }],
    ])
}

/// A double click at `pos`: the move, then two press-and-release pairs, each pair in one frame so that a slow frame cannot
/// push the second out of egui's double-click window.
fn dclick_frames(pos: egui::Pos2) -> VecDeque<Vec<egui::Event>> {
    let (button, modifiers) = (egui::PointerButton::Primary, egui::Modifiers::NONE);
    let pair = || [true, false].map(|pressed| egui::Event::PointerButton { pos, button, pressed, modifiers }).to_vec();
    VecDeque::from([vec![egui::Event::PointerMoved(pos)], pair(), pair()])
}

/// A click or a drag as the frames of pointer events it takes: egui sees a
/// press and a release in one frame as a click, never as a drag.
fn pointer_frames(press: &Press, rect: egui::Rect) -> VecDeque<Vec<egui::Event>> {
    let events = pointer_events(press, rect);
    match press {
        Press::Click { .. } => events.into_iter().map(|e| vec![e]).collect(),
        Press::DClick { .. } => {
            let mut it = events.into_iter();
            let (moved, rest): (Vec<_>, Vec<_>) = (it.next().into_iter().collect(), it.collect());
            VecDeque::from([moved, rest[..2].to_vec(), rest[2..].to_vec()])
        }
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
                let report = with_current_run(&stalled_report(&labels, p.left, quiet));
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

/// An input the runner put in that no main-window pass has drawn yet.
#[derive(Clone, Debug)]
struct Flight {
    /// `cumulative_pass_nr_for(ROOT)` when it went in.
    pass: u64,
    at: Instant,
    /// The label of the step it came from.
    step: String,
    /// Whether events went into the raw input (`<Size:>`, `<Theme:>` and `<Lang:>` put none in, but still wait for a
    /// main-window pass).
    events: bool,
}

/// A script being played.
///
/// A reading step (`<Assert:>`, `<Assert:!>`, the states, `<Tree:>`, `<Click:text>`, `<DClick:text>`) judges only an
/// observation made after the last input was drawn by the main window. A negative step (`<Assert:!x>`) passes on
/// the first such observation without `x` (waiting up to [`TEXT_WAIT`] for it to go); once it has passed, it does
/// not watch for `x` appearing later. `<Clipboard:>`, `<ClipboardHas:>` and `<Announced:>` look only at copies and
/// announcements made after the last input went in (see [`Mark`]).
pub struct Runner {
    steps: VecDeque<Step>,
    next: Instant,
    /// The picture asked for, and whether the request has gone out.
    shot: Option<(String, bool)>,
    /// The frames of a click or a drag still to go in.
    pointer: VecDeque<Vec<egui::Event>>,
    /// Where `<Shot:>` and `<State:>` write.
    pub dir: PathBuf,
    /// What the app says about its shots (`app`, `version`, `component`, `state`, `zoom`); `<Theme:>` and
    /// `<Lang:>` keep `theme` and `lang`. [`Runner::shot`] writes it, with the size and time, to `<name>.json`.
    pub info: review::Meta,
    /// The app's `<APP>_KEYS_DONE`.
    pub done: Option<PathBuf>,
    /// The clipboard's text for `<Paste>` and `<C-v>`.
    pub clipboard: fn() -> String,
    finished: bool,
    progress: Option<Arc<Mutex<Progress>>>,
    /// Some step reads the window's tree or the last copy.
    reads_window: bool,
    /// Until when a step that reads the window keeps looking.
    deadline: Option<Instant>,
    /// An input put in, not yet seen drawn by a main-window pass.
    flight: Option<Flight>,
    /// The generation as of the pass that drew the last input; a reading step needs a newer one.
    action_gen: u64,
    /// The label of the step that made the frames in `pointer`.
    pointer_step: String,
    /// The frames in `pointer` are a step's first: the next one to go in takes the mark.
    pointer_new: bool,
    /// What the last `<ReadHas:>` read, while it waits.
    last_read: Option<String>,
    /// Where the copies and announcements stood when the last action's first input went in (`None` before the first action).
    since: Option<Mark>,
    /// When the last action's first input went in (`None` before the first action).
    acted: Option<Instant>,
}

/// The `<State:>` line that says when a reading was really taken, ending in a newline:
/// `since last action: 153 ms` (whole milliseconds, rounded down), or `since last action: none` before the first
/// action. A timed read (`<Now><Wait:N>`) is not held to its N; this line says how long it really took.
pub fn since_action_line(since: Option<Duration>) -> String {
    match since {
        Some(d) => format!("since last action: {} ms\n", d.as_millis()),
        None => "since last action: none\n".into(),
    }
}

/// What an app reads for `<ReadHas:NAME=TEXT>` (a terminal pane's text, say). The runner asks for it in each frame
/// the step waits and never keeps the reader.
pub trait Reader {
    /// The text under `name` (a tag of the app's [`Dialect::reads`]): `Ok(None)` when there is nothing yet (the step
    /// waits), `Err` when there never will be (the step fails at once).
    fn read(&mut self, name: &str) -> Result<Option<String>, String>;
}

/// The reader of [`Runner::input`]: an app that reads nothing.
struct NoReader;

impl Reader for NoReader {
    fn read(&mut self, name: &str) -> Result<Option<String>, String> {
        Err(format!("this app does not read {name}"))
    }
}

impl Runner {
    pub fn new(steps: Vec<Step>, done: Option<PathBuf>) -> Self {
        // A script may switch the theme or language, which writes `common.toml`.
        ito_common::scripted_run();
        let dir = done
            .as_ref()
            .and_then(|d| d.parent().map(PathBuf::from))
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        // The first frames lay the window out; a key before them can land
        // before the panes exist.
        let reads_window = steps.iter().any(|s| {
            matches!(s, Step::ClickText { .. } | Step::DClickText(_) | Step::Assert { .. } | Step::Clipboard(_) | Step::ClipboardHas(_))
                || matches!(s, Step::Is { .. } | Step::Tree(_) | Step::Announced(_) | Step::ClickId(_) | Step::IdCheck(_))
        });
        if steps.iter().any(|s| matches!(s, Step::Announced(_) | Step::Tree(_))) {
            ito_a11y::record_announcements(true);
        }
        Self {
            reads_window,
            deadline: None,
            flight: None,
            action_gen: 0,
            pointer_step: String::new(),
            last_read: None,
            since: None,
            acted: None,
            pointer_new: false,
            steps: steps.into(),
            next: Instant::now() + Duration::from_millis(500),
            shot: None,
            pointer: VecDeque::new(),
            dir,
            info: review::Meta { zoom: 1.0, component: "window".into(), ..Default::default() },
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

    /// Put the next step's events into this frame's input when it is due. Only the main window's hook gets
    /// any, and the next one waits until the main window has drawn the last.
    pub fn input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) -> Option<Do> {
        self.input_with(ctx, raw, &mut NoReader)
    }

    /// [`input`](Self::input) for an app that reads something for `<ReadHas:>`: `reader` is asked in the frames that
    /// step takes and is not kept.
    pub fn input_with(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput, reader: &mut dyn Reader) -> Option<Do> {
        if self.finished || self.shot.is_some() {
            return None;
        }
        if self.reads_window {
            observe(ctx);
        }
        let now = Instant::now();
        let root_passes = ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT);
        if raw.viewport_id != egui::ViewportId::ROOT {
            // eframe hands what is left over to whichever window comes next: if the main window was not drawn, the
            // input would land here.
            if let Some(f) = self.flight.clone().filter(|f| f.events && root_passes == f.pass) {
                self.finished = true;
                self.note(now);
                let why = format!("the input went to viewport {:?} while the main window was not drawn", raw.viewport_id);
                return Some(Do::Failed { step: f.step, why });
            }
            return None;
        }
        if let Some(f) = self.flight.clone() {
            if root_passes > f.pass {
                self.flight = None;
                self.action_gen = generation(ctx);
                self.note(now);
            } else if now.saturating_duration_since(f.at) >= DELIVER_WAIT {
                self.finished = true;
                self.note(now);
                let why = "the input was not drawn within 10 s (the window may be hidden, minimized or covered)".to_owned();
                return Some(Do::Failed { step: f.step, why });
            } else {
                return None;
            }
        }
        if now < self.next {
            return None;
        }
        if let Some(frame) = self.pointer.pop_front() {
            raw.events.extend(frame);
            // The step's first frame is the action: a copy made by the press still counts after the release.
            if std::mem::take(&mut self.pointer_new) {
                self.act(ctx, now);
            }
            self.next = now + FRAME;
            self.fly(ctx, now, self.pointer_step.clone(), true);
            self.note(now);
            return None;
        }
        let Some(step) = self.steps.pop_front() else {
            self.finished = true;
            self.note(now);
            return Some(Do::Done);
        };
        self.next = now + GAP;
        let did = self.take(&step, ctx, raw, now, reader);
        self.note(now);
        did
    }

    /// How long ago the last action's first input went in (`None` before the first action). Read it in the frame
    /// that got `Do::State` (when the state is taken), not later when a file is written.
    pub fn since_action(&self) -> Option<Duration> {
        self.acted.map(|t| t.elapsed())
    }

    /// The last action's first input goes in now: remember the copies and announcements, and the time.
    fn act(&mut self, ctx: &egui::Context, now: Instant) {
        self.since = Some(mark(ctx));
        self.acted = Some(now);
    }

    /// An input has gone in (`events`: into the raw input): wait for the main window to draw it.
    fn fly(&mut self, ctx: &egui::Context, now: Instant, step: String, events: bool) {
        self.flight = Some(Flight { pass: ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT), at: now, step, events });
    }

    /// The window as a reading step may judge it: seen after the last input was drawn, in the main window,
    /// and not minimized.
    fn fresh(&self, ctx: &egui::Context) -> Result<Observation, String> {
        match observation(ctx) {
            Some(o) if o.generation > self.action_gen && o.viewport == egui::ViewportId::ROOT => {
                if o.minimized == Some(true) {
                    Err("the window is minimized".into())
                } else {
                    Ok(o)
                }
            }
            _ => Err("the window's tree was not updated after the last action (the window may be hidden, minimized or covered)".into()),
        }
    }

    /// One step into this frame's input.
    fn take(&mut self, step: &Step, ctx: &egui::Context, raw: &mut egui::RawInput, now: Instant, reader: &mut dyn Reader) -> Option<Do> {
        // A key no keyboard has is skipped, as `--keys` refuses it up front.
        match press(step)? {
            Press::Events(events) => {
                raw.events.extend(events);
                self.act(ctx, now);
                self.fly(ctx, now, label(step), true);
            }
            Press::Wait(d) => self.next = now + d,
            // This runner has no settling to skip. Its pacing (`GAP`) is unchanged, so a timed read here
            // (`<Now><Wait:N>`) is about `GAP` later than N (ito#204 shares the pacing).
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
            Press::Size { w, h } => {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w as f32, h as f32)));
                // The window lays out again at the new size before the next step.
                self.next = now + SETTLE;
                self.act(ctx, now);
                self.fly(ctx, now, label(step), false);
            }
            Press::Theme(name) => {
                self.info.theme = name.clone();
                self.act(ctx, now);
                self.fly(ctx, now, label(step), false);
                return Some(Do::Theme(name));
            }
            Press::Lang(code) => {
                self.info.lang = code.clone();
                self.act(ctx, now);
                self.fly(ctx, now, label(step), false);
                return Some(Do::Lang(code));
            }
            Press::ClickText { right, text } => match self.fresh(ctx).and_then(|o| locate_in(&o.items, &text)) {
                Ok(pos) => {
                    self.deadline = None;
                    self.pointer = click_frames(pos, right);
                    self.pointer_step = label(step);
                    self.pointer_new = true;
                    self.next = now;
                }
                Err(why) => return self.retry(step, why, now),
            },
            Press::DClickText(text) => match self.fresh(ctx).and_then(|o| locate_in(&o.items, &text)) {
                Ok(pos) => {
                    self.deadline = None;
                    self.pointer = dclick_frames(pos);
                    self.pointer_step = label(step);
                    self.pointer_new = true;
                    self.next = now;
                }
                Err(why) => return self.retry(step, why, now),
            },
            Press::Assert { text, present } => match self.fresh(ctx).and_then(|o| check_in(&o.items, &text, present)) {
                Ok(()) => self.deadline = None,
                Err(why) => return self.retry(step, why, now),
            },
            Press::Clipboard(text) => match check_clipboard_after(ctx, self.since.unwrap_or_default(), &text) {
                Ok(()) => self.deadline = None,
                Err(why) => return self.retry(step, why, now),
            },
            Press::ClipboardHas(text) => match check_clipboard_has_after(ctx, self.since.unwrap_or_default(), &text) {
                Ok(()) => self.deadline = None,
                Err(why) => return self.retry(step, why, now),
            },
            Press::Is { prop, text } => match self.fresh(ctx).and_then(|o| check_is_in(&o.items, prop, &text)) {
                Ok(()) => self.deadline = None,
                Err(why) => return self.retry(step, why, now),
            },
            Press::Announced(text) => match self.since.map_or_else(|| check_announced(&text), |m| check_announced_after(m, &text)) {
                Ok(()) => self.deadline = None,
                Err(why) => return self.retry(step, why, now),
            },
            Press::ClickId(name) => match self.fresh(ctx).and_then(|o| match o.id(&name)? {
                Some(node) => node.click_at.clone(),
                None => Err(format!("no widget has the test ID `{name}`")),
            }) {
                Ok(pos) => {
                    self.deadline = None;
                    self.pointer = click_frames(pos, false);
                    self.pointer_step = label(step);
                    self.pointer_new = true;
                    self.next = now;
                }
                Err(why) => return self.retry(step, why, now),
            },
            Press::IdCheck(check) => match self.fresh(ctx).and_then(|o| check.check(&o)) {
                Ok(()) => self.deadline = None,
                Err(why) => return self.retry(step, why, now),
            },
            Press::ReadHas { tag, text } => return self.read_has(step, &tag, &text, now, ctx, reader),
            Press::Tree(name) => {
                let obs = match self.fresh(ctx) {
                    Ok(o) if o.tree.is_empty() => Err("the window's tree was empty".to_owned()),
                    other => other,
                };
                let obs = match obs {
                    Ok(o) => o,
                    Err(why) => return self.retry(step, why, now),
                };
                self.deadline = None;
                let path = self.dir.join(format!("{name}.txt"));
                if let Err(e) = std::fs::write(&path, tree_file(&obs.tree)) {
                    self.finished = true;
                    return Some(Do::Failed { step: label(step), why: format!("cannot write {}: {e}", path.display()) });
                }
            }
            Press::Paste => {
                raw.events.push(egui::Event::Paste((self.clipboard)()));
                self.act(ctx, now);
                self.fly(ctx, now, label(step), true);
            }
            Press::PasteText(text) => {
                raw.events.push(paste_text_event(&text));
                self.act(ctx, now);
                self.fly(ctx, now, label(step), true);
            }
            p @ (Press::Click { .. } | Press::DClick { .. } | Press::Hover { .. } | Press::Wheel { .. } | Press::Drag { .. }) => {
                let rect = raw.screen_rect.unwrap_or_else(|| ctx.content_rect());
                self.pointer = pointer_frames(&p, rect);
                self.pointer_step = label(step);
                self.pointer_new = true;
                self.next = now;
            }
        }
        None
    }

    /// `<ReadHas:>`: read, and wait up to [`READ_WAIT`] for the text. Not an operation: no input goes in, so nothing
    /// is in flight and the observation generation stays.
    fn read_has(&mut self, step: &Step, tag: &str, text: &str, now: Instant, ctx: &egui::Context, reader: &mut dyn Reader) -> Option<Do> {
        let read = reader.read(tag);
        let fail = |this: &mut Self, why: String| {
            this.deadline = None;
            this.last_read = None;
            this.finished = true;
            Some(Do::Failed { step: label(step), why })
        };
        match read {
            Err(e) => return fail(self, e),
            Ok(Some(t)) if t.contains(text) => {
                self.deadline = None;
                self.last_read = None;
                return None;
            }
            Ok(Some(t)) => self.last_read = Some(t),
            Ok(None) => self.last_read = None,
        }
        let deadline = *self.deadline.get_or_insert(now + READ_WAIT);
        if now >= deadline {
            let why = match &self.last_read {
                Some(t) => {
                    let n = t.chars().count();
                    if n > 200 {
                        format!("\u{2026}{}", t.chars().skip(n - 200).collect::<String>())
                    } else {
                        t.clone()
                    }
                }
                None => "nothing to read (no pane)".to_owned(),
            };
            return fail(self, why);
        }
        self.steps.push_front(step.clone());
        self.next = now + FRAME;
        ctx.request_repaint();
        None
    }

    /// A step that reads the window did not find what it looked for: look again next frame, until
    /// [`TEXT_WAIT`] has passed, then fail the run.
    fn retry(&mut self, step: &Step, why: String, now: Instant) -> Option<Do> {
        let deadline = *self.deadline.get_or_insert(now + TEXT_WAIT);
        if now >= deadline {
            self.deadline = None;
            self.finished = true;
            return Some(Do::Failed { step: label(step), why });
        }
        self.steps.push_front(step.clone());
        self.next = now + FRAME;
        None
    }

    /// When the next frame is wanted for the script: a repaint asked for in
    /// `raw_input_hook` is forgotten when the frame begins, so the frame asks. While an input waits to be drawn, no
    /// sooner than [`FRAME`], so a window that is not drawn is not asked for frames back to back.
    pub fn due(&self) -> Option<Duration> {
        match () {
            _ if self.finished => None,
            _ if self.shot.is_some() => Some(Duration::ZERO),
            _ if self.flight.is_some() => Some(self.next.saturating_duration_since(Instant::now()).max(FRAME)),
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
        if let Err(e) = save_png(&path, &image) {
            return Some(format!("Shot {name}: {e}"));
        }
        let screen = ctx.content_rect().size();
        let meta = review::Meta {
            name: name.clone(),
            size: [screen.x, screen.y],
            rect: [0.0, 0.0, screen.x, screen.y],
            time: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            pixels_per_point: ctx.pixels_per_point(),
            ..self.info.clone()
        };
        std::fs::write(self.dir.join(format!("{name}.json")), meta.to_json()).err().map(|e| format!("Shot {name}: {e}"))
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

    /// Copies are process-wide state (see [`note_copy`]), so the tests that copy or check the clipboard take turns.
    static COPIES: Mutex<()> = Mutex::new(());

    /// Takes the turn and forgets the copies given to `note_copy` by earlier tests.
    fn lock_copies() -> std::sync::MutexGuard<'static, ()> {
        let guard = COPIES.lock().unwrap_or_else(|e| e.into_inner());
        *NOTED.lock().unwrap_or_else(|e| e.into_inner()) = Noted::default();
        guard
    }

    fn run_lines(r: &str) -> usize {
        r.lines().filter(|l| l.starts_with("run: ")).count()
    }

    #[test]
    fn run_id_parsing() {
        let name = "KURA_KEYS_RUN_ID";
        assert_eq!(decide_run_id(None, name, 1234, 77).unwrap(), "1234-77");
        assert_eq!(decide_run_id(Some("abc_DEF-09".into()), name, 1, 1).unwrap(), "abc_DEF-09");
        let long = "a".repeat(64);
        assert_eq!(decide_run_id(Some(long.clone().into()), name, 1, 1).unwrap(), long);
        for bad in ["a".repeat(65), String::new(), "a b".into(), "a.b".into(), "ä".into()] {
            let e = decide_run_id(Some(bad.clone().into()), name, 1, 1).unwrap_err();
            assert!(e.starts_with(&format!("{name}: '")) && e.contains("is not a run id"), "{e}");
        }
        let e = decide_run_id(Some("a".repeat(100).into()), name, 1, 1).unwrap_err();
        assert!(e.contains(&format!("'{}'", "a".repeat(64))), "{e}");
    }

    #[test]
    fn run_id_is_kept_for_the_process() {
        let a = run_id("ITO_TEST_UNSET_PREFIX").unwrap();
        assert_eq!(run_id("OTHER_PREFIX").unwrap(), a);
        assert_eq!(current_run_id(), Some(a.as_str()));
    }

    #[test]
    fn with_run_puts_one_run_line_in_the_right_place() {
        let stalled = stalled_report(&["j".to_owned(), "k".to_owned()], 1, Duration::from_secs(31));
        let failures = [
            stalled,
            refused_report("bad"),
            failed_report("<Assert:x>", "no", 2),
            "keys: panicked\nthread: t\nat: a\nwhy: w\n".to_owned(),
            quit_report(2),
            closed_report(2),
        ];
        for r in &failures {
            let got = with_run("R", r);
            assert_eq!(run_lines(&got), 1, "{got}");
            assert_eq!(got.lines().next(), r.lines().next(), "{got}");
            assert_eq!(got.lines().last(), Some("run: R"), "{got}");
            assert!(got.ends_with('\n') && !got.ends_with("\n\n"), "{got:?}");
        }
        for r in [quit_report(0), closed_report(0)] {
            let got = with_run("R", &r);
            assert_eq!(run_lines(&got), 1, "{got}");
            assert_eq!(got.lines().next(), r.lines().next(), "{got}");
            assert!(got.ends_with("run: R\nkeys: done\n"), "{got:?}");
        }
        assert_eq!(with_run("R", "keys: done\n"), "run: R\nkeys: done\n");
        assert_eq!(with_run("R", &quit_report(0)), "quit: yes\nrun: R\nkeys: done\n");
        assert_eq!(with_run("R", ""), "run: R\n");
    }

    #[test]
    fn with_run_twice_leaves_the_last_id_only() {
        for x in ["keys: done\n", "keys: failed\nstep: s\n", ""] {
            let got = with_run("B", &with_run("A", x));
            assert_eq!(run_lines(&got), 1, "{got}");
            assert!(got.contains("run: B\n") && !got.contains("run: A"), "{got}");
        }
    }

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

    /// Q41: `<Now>` marks the next step past any waits, and a key or a reading has to follow it.
    #[test]
    fn now_marks_the_next_step() {
        let got = parse("d<Now>w").unwrap();
        assert_eq!(got[1], Step::Now);
        assert!(matches!(got[2], Step::Key(_)));
        assert_eq!(press(&got[1]), Some(Press::Now));
        for ok in [
            "d<Now>w",
            "<A-c><Now><State:mid>",
            "x<Now><Wait:150><State:b>",
            "x<Now><Wait:100><Wait:50>y",
            "<Now><Wait:0><State:a>",
            "<Now><Wait:100>x",
        ] {
            assert!(parse(ok).is_ok(), "{ok}: {:?}", parse(ok));
        }
        for bad in ["d<Now>", "<Now><Now>x", "<Now><Quit>", "<Now><Wait:100>", "<Now><Wait:100><Quit>", "<Now><Wait:100><Now>x"] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("<Now>"), "{bad}: {err}");
        }
        let got = parse("x<Now><Wait:150><State:b>").unwrap();
        assert_eq!(got.len(), 4);
        assert!(matches!(got[0], Step::Key(_)));
        assert_eq!(got[1], Step::Now);
        assert_eq!(got[2], Step::Wait(Duration::from_millis(150)));
        assert_eq!(got[3], Step::State("b".into()));
    }

    #[test]
    fn since_action_line_says_whole_milliseconds() {
        assert_eq!(since_action_line(None), "since last action: none\n");
        assert_eq!(since_action_line(Some(Duration::from_micros(153_900))), "since last action: 153 ms\n");
        assert_eq!(since_action_line(Some(Duration::ZERO)), "since last action: 0 ms\n");
    }

    /// `<Size:WxH>`, `<Theme:x>` and `<Lang:x>` parse, print back as written, and refuse nonsense.
    #[test]
    fn size_theme_and_lang_are_steps() {
        let got = parse("<Size:480x600><Theme:light><Lang:ja>").unwrap();
        assert_eq!(got, [Step::Size { w: 480, h: 600 }, Step::Theme("light".into()), Step::Lang("ja".into())]);
        let labels: Vec<String> = got.iter().map(label).collect();
        assert_eq!(labels, ["<Size:480x600>", "<Theme:light>", "<Lang:ja>"]);
        assert_eq!(press(&got[0]), Some(Press::Size { w: 480, h: 600 }));
        assert_eq!(press(&got[1]), Some(Press::Theme("light".into())));
        for bad in ["<Size:480>", "<Size:50x600>", "<Size:480x99999>", "<Size:axb>", "<Theme:>", "<Theme:a b>", "<Lang:ja/en>"] {
            assert!(parse(bad).is_err(), "{bad}");
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

    /// `<Preedit:>` and `<Commit:>` are the input method's events, with the paste escapes.
    #[test]
    fn preedit_and_commit_are_the_input_methods_events() {
        let got = parse("<Preedit:にほん><Preedit:><Commit:日本\\n>").unwrap();
        assert_eq!(got, [Step::Preedit("にほん".into()), Step::Preedit(String::new()), Step::Commit("日本\n".into())]);
        assert_eq!(press(&got[0]), Some(Press::Events(vec![preedit_event("にほん")])));
        assert_eq!(press(&got[2]), Some(Press::Events(vec![commit_event("日本\n")])));
        assert_eq!(label(&got[2]), "<Commit:日本\\n>");
        assert!(parse("<Commit:\\q>").unwrap_err().contains("not an input method step"));
    }

    /// `<DClick:>` parses by place or by text, round-trips, and makes egui report `double_clicked()` once.
    #[test]
    fn a_double_click_is_one_double_click() {
        let got = parse("<DClick:0.5,0.5><DClick:C-0.1,0.2><DClick:Open>").unwrap();
        assert_eq!(got[0], Step::DClick { mods: 0, at: (500, 500) });
        assert_eq!(got[2], Step::DClickText("Open".into()));
        assert_eq!(got.iter().map(label).collect::<Vec<_>>(), ["<DClick:0.5,0.5>", "<DClick:C-0.1,0.2>", "<DClick:Open>"]);
        assert!(parse("<DClick:2,2>").is_err());
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 200.0));
        let p = press(&got[0]).unwrap();
        let frames = pointer_frames(&p, rect);
        assert_eq!(frames.len(), 3);
        let ctx = egui::Context::default();
        let (mut single, mut double) = (0, 0);
        let mut run = |events: Vec<egui::Event>| {
            let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), events, ..Default::default() }, |ui| {
                let r = ui.interact(
                    egui::Rect::from_min_size(egui::pos2(50.0, 50.0), egui::vec2(100.0, 100.0)),
                    egui::Id::new("b"),
                    egui::Sense::click(),
                );
                single += r.clicked() as u32;
                double += r.double_clicked() as u32;
            });
            out.textures_delta.clear();
        };
        run(vec![]);
        for f in frames {
            run(f);
        }
        run(vec![]);
        assert_eq!(double, 1, "single clicks {single}");
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
        for bad in ["<Click:2,0.5>", "<Click:0.5>", "<Wheel:3>", "<Wheel:x@0.5,0.5>"] {
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
    /// running; before a wait it reaches through to the step after it.
    #[test]
    fn now_may_come_before_a_reading() {
        assert!(parse("<A-c><Now><State:mid>").is_ok());
        assert!(parse("<Now><Shot:a>").is_ok());
        assert!(parse("<Now><Wait:5>x").is_ok());
        assert!(parse("<Now><Wait:5>").is_err());
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
        // The main window draws a pass after each input, as a real frame loop does.
        let step = |r: &mut Runner| {
            r.next = Instant::now();
            let did = r.input(&ctx, &mut egui::RawInput::default());
            ctx.run_ui(egui::RawInput::default(), |_| {}).drop_without_applying_deltas();
            did
        };
        assert_eq!((r.left(), r.running()), (4, true));
        assert_eq!(step(&mut r), None);
        assert_eq!(step(&mut r), Some(Do::Quit));
        assert_eq!((r.left(), r.running(), step(&mut r)), (2, false, None));
        let mut r = Runner::new(parse("j").unwrap(), None);
        assert_eq!((step(&mut r), step(&mut r), r.running()), (None, Some(Do::Done), false));
    }

    #[test]
    fn clipboard_has_parses_and_prints_back() {
        let steps = parse("<ClipboardHas:a/b>").unwrap();
        assert_eq!(steps, vec![Step::ClipboardHas("a/b".into())]);
        assert_eq!(parse(&label(&steps[0])).unwrap(), steps);
        assert!(matches!(press(&steps[0]), Some(Press::ClipboardHas(_))));
    }

    /// #98: the steps that read the window parse and print back.
    #[test]
    fn text_steps_parse() {
        let steps = parse("<Click:Save><RClick:a\\x3eb><Click:0.5,0.4><Assert:Done><Assert:!Oops><Clipboard:x\\ny>").unwrap();
        assert_eq!(steps[0], Step::ClickText { right: false, text: "Save".into() });
        assert_eq!(steps[1], Step::ClickText { right: true, text: "a>b".into() });
        assert!(matches!(steps[2], Step::Click { .. }));
        assert_eq!(steps[3], Step::Assert { text: "Done".into(), present: true });
        assert_eq!(steps[4], Step::Assert { text: "Oops".into(), present: false });
        assert_eq!(steps[5], Step::Clipboard("x\ny".into()));
        for s in &steps[..2] {
            assert_eq!(parse(&label(s)).unwrap(), vec![s.clone()]);
        }
        for s in &steps[3..] {
            assert_eq!(parse(&label(s)).unwrap(), vec![s.clone()]);
        }
        let literal_bang = Step::Assert { text: "!x".into(), present: true };
        assert_eq!(parse(&label(&literal_bang)).unwrap(), vec![literal_bang]);
    }

    /// #98: the runner finds a button by its label, clicks it, and fails the run when the text is
    /// not there after the wait.
    #[test]
    fn the_runner_reads_the_window() {
        let _g = lock_copies();
        let ctx = egui::Context::default();
        let mut clicked = false;
        let pass = |ctx: &egui::Context, raw: egui::RawInput, clicked: &mut bool| {
            ctx.run_ui(raw, |ui| {
                if ui.button("Save").clicked() {
                    *clicked = true;
                }
                if *clicked {
                    ui.label("Saved!");
                    ui.ctx().copy_text("copied".into());
                }
            })
            .drop_without_applying_deltas();
        };
        let mut r = Runner::new(parse("<Assert:Save><Click:Save><Assert:Saved!><Assert:!Nope><Clipboard:copied>").unwrap(), None);
        let mut out = None;
        for _ in 0..200 {
            r.next = Instant::now();
            let mut raw = egui::RawInput::default();
            if let Some(d) = r.input(&ctx, &mut raw) {
                out = Some(d);
                break;
            }
            pass(&ctx, raw, &mut clicked);
        }
        assert_eq!(out, Some(Do::Done));
        assert!(clicked);

        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("<Assert:Missing>").unwrap(), None);
        let mut out = None;
        for _ in 0..4 {
            r.next = Instant::now();
            let mut raw = egui::RawInput::default();
            if let Some(d) = r.input(&ctx, &mut raw) {
                out = Some(d);
                break;
            }
            pass(&ctx, raw, &mut false);
            r.deadline = Some(Instant::now());
        }
        match out {
            Some(Do::Failed { step, why }) => {
                assert_eq!(step, "<Assert:Missing>");
                assert!(why.contains("Missing"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        assert!(failed_report("<Assert:x>", "a\nb", 2).starts_with("keys: failed\nstep: <Assert:x>\nwhy: a / b\n"));
    }

    /// Plays `script` against `app` (called once per pass with the pass number). `quick`: a failing step fails at once.
    fn drive(script: &str, quick: bool, mut app: impl FnMut(&mut egui::Ui, usize)) -> Option<Do> {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse(script).unwrap(), None);
        for i in 0..400 {
            r.next = Instant::now();
            let mut raw = egui::RawInput::default();
            if let Some(d) = r.input(&ctx, &mut raw) {
                return Some(d);
            }
            ctx.run_ui(raw, |ui| app(ui, i)).drop_without_applying_deltas();
            if quick {
                r.deadline = Some(Instant::now());
            }
        }
        None
    }

    fn typed(ui: &egui::Ui, c: &str) -> bool {
        ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == c)))
    }

    #[test]
    fn a_wait_or_state_is_not_an_action_for_the_clipboard() {
        let _g = lock_copies();
        let app = |ui: &mut egui::Ui, _: usize| {
            if typed(ui, "y") {
                ui.ctx().copy_text("x".into());
            }
        };
        assert_eq!(drive("y<Wait:500><Clipboard:x>", true, app), Some(Do::Done));
        let mut states = 0;
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("y<State:s><Clipboard:x>").unwrap(), None);
        let mut out = None;
        for _ in 0..100 {
            r.next = Instant::now();
            let mut raw = egui::RawInput::default();
            match r.input(&ctx, &mut raw) {
                Some(Do::State(_)) => states += 1,
                Some(d) => {
                    out = Some(d);
                    break;
                }
                None => {}
            }
            ctx.run_ui(raw, |ui| app(ui, 0)).drop_without_applying_deltas();
        }
        assert_eq!((states, out), (1, Some(Do::Done)));
    }

    /// Plays `script` one `input` call at a time without hurrying the waits; `trace` sees each call's result and `acted`.
    fn play(script: &str, mut trace: impl FnMut(&Runner, &Option<Do>)) {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse(script).unwrap(), None);
        r.next = Instant::now();
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            let mut raw = egui::RawInput::default();
            let did = r.input(&ctx, &mut raw);
            trace(&r, &did);
            if matches!(did, Some(Do::Done | Do::Failed { .. })) {
                return;
            }
            ctx.run_ui(raw, |_| {}).drop_without_applying_deltas();
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("{script}: did not finish");
    }

    fn state_of(did: &Option<Do>) -> Option<String> {
        match did {
            Some(Do::State(n)) => Some(n.clone()),
            _ => None,
        }
    }

    #[test]
    fn since_action_starts_at_the_first_action_and_only_actions_move_it() {
        let mut trail = Vec::new();
        play("<State:a>x<Now><Wait:10><State:b>", |r, did| trail.push((state_of(did), r.acted)));
        let first = trail.iter().position(|(d, _)| d.as_deref() == Some("a")).unwrap();
        assert_eq!(trail[first].1, None, "no action before the first state");
        assert!(trail.iter().take(first + 1).all(|(_, a)| a.is_none()));
        let acted = trail.last().unwrap().1.expect("the key is an action");
        let set_at = trail.iter().position(|(_, a)| a.is_some()).unwrap();
        assert!(trail[set_at..].iter().all(|(_, a)| *a == Some(acted)), "a wait, state or <Now> never moves it");
        let b = trail.iter().position(|(d, _)| d.as_deref() == Some("b")).unwrap();
        assert!(b > set_at);
    }

    #[test]
    fn a_pointer_step_marks_the_time_at_its_first_frame_only() {
        for script in ["<Click:0.5,0.5>", "<Drag:0.1,0.1-0.9,0.9>"] {
            let mut trail = Vec::new();
            play(script, |r, did| trail.push((state_of(did), r.acted, r.pointer.len())));
            let first = trail.iter().position(|(_, a, _)| a.is_some()).expect(script);
            let acted = trail[first].1;
            // The first frame has gone in; frames of the same step are still to come.
            assert!(trail[first].2 > 0, "{script}: the step has later frames");
            assert!(trail[first..].iter().all(|(_, a, _)| *a == acted), "{script}");
        }
    }

    #[test]
    fn a_timed_read_waits_its_time_between_the_two_readings() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("x<Now><State:a><Now><Wait:150><State:b>").unwrap(), None);
        r.next = Instant::now();
        let (mut at_a, mut at_b, mut seen_due) = (None, None, false);
        let mut order = Vec::new();
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end && at_b.is_none() {
            let mut raw = egui::RawInput::default();
            match r.input(&ctx, &mut raw) {
                Some(Do::State(n)) => {
                    let t = Instant::now();
                    if n == "a" {
                        at_a = Some(t);
                    } else {
                        at_b = Some(t);
                    }
                    order.push(n);
                }
                Some(d) => panic!("{d:?}"),
                None => {}
            }
            // The Wait step gave the watchdog its due: `note` reports `next`.
            if at_a.is_some() && r.next > Instant::now() + Duration::from_millis(100) {
                seen_due = true;
            }
            ctx.run_ui(raw, |_| {}).drop_without_applying_deltas();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(order, ["a", "b"]);
        assert!(seen_due, "next is about 150 ms ahead after the wait");
        assert!(at_b.unwrap() - at_a.unwrap() >= Duration::from_millis(150));
    }

    #[test]
    fn a_copy_before_the_last_action_does_not_count() {
        let _g = lock_copies();
        let app = |ui: &mut egui::Ui, _: usize| {
            if typed(ui, "a") {
                ui.ctx().copy_text("x".into());
            }
        };
        assert_eq!(drive("a<Clipboard:x>", true, app), Some(Do::Done));
        assert_eq!(drive("a<ClipboardHas:x>", true, app), Some(Do::Done));
        for script in ["ab<Clipboard:x>", "ab<ClipboardHas:x>"] {
            match drive(script, true, app) {
                Some(Do::Failed { why, .. }) => {
                    assert!(why.contains("nothing copied after the last action"), "{why}");
                    assert!(why.contains("`x`"), "{why}");
                }
                other => panic!("{script}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_copy_before_any_action_still_counts() {
        let _g = lock_copies();
        let app = |ui: &mut egui::Ui, i: usize| {
            if i == 0 {
                ui.ctx().copy_text("x".into());
            }
        };
        assert_eq!(drive("<Clipboard:x>", true, app), Some(Do::Done));
    }

    #[test]
    fn a_click_marks_once_at_its_first_frame() {
        let _g = lock_copies();
        let app = |ui: &mut egui::Ui, _: usize| {
            let _ = ui.button("Save");
            if ui.input(|i| i.pointer.any_pressed()) {
                ui.ctx().copy_text("pressed".into());
            }
        };
        assert_eq!(drive("<Click:Save><Clipboard:pressed>", false, app), Some(Do::Done));
    }

    #[test]
    fn note_copy_when_off_records_nothing() {
        let _g = lock_copies();
        let before = NOTED.lock().unwrap_or_else(|e| e.into_inner()).count;
        note_copy_when(false, "off");
        let after = NOTED.lock().unwrap_or_else(|e| e.into_inner()).count;
        assert_eq!(before, after);
    }

    #[test]
    fn note_copy_is_seen_by_the_checks_from_any_thread() {
        let _g = lock_copies();
        let ctx = egui::Context::default();
        observe(&ctx);
        let before = mark(&ctx);
        std::thread::spawn(|| note_copy("n1")).join().unwrap();
        let after = mark(&ctx);
        assert_eq!(after.copies, before.copies + 1);
        assert_eq!(check_clipboard_after(&ctx, before, "n1"), Ok(()));
        assert_eq!(check_clipboard_has_after(&ctx, before, "n"), Ok(()));
        let why = check_clipboard_after(&ctx, after, "n1").unwrap_err();
        assert!(why.contains("nothing copied after the last action"), "{why}");
    }

    #[test]
    fn the_later_of_a_tap_copy_and_a_noted_copy_is_the_last() {
        let _g = lock_copies();
        let ctx = egui::Context::default();
        observe(&ctx);
        let base = mark(&ctx).copies;
        ctx.run_ui(egui::RawInput::default(), |ui| ui.ctx().copy_text("y".into())).drop_without_applying_deltas();
        note_copy("z");
        assert_eq!((last_copy(&ctx), mark(&ctx).copies), ((Some("z".to_string()), base + 2), base + 2));
        ctx.run_ui(egui::RawInput::default(), |ui| ui.ctx().copy_text("w".into())).drop_without_applying_deltas();
        assert_eq!((last_copy(&ctx), mark(&ctx).copies), ((Some("w".to_string()), base + 3), base + 3));
    }

    #[test]
    fn a_copy_noted_by_the_app_passes_the_clipboard_steps() {
        let _g = lock_copies();
        let app = |ui: &mut egui::Ui, _: usize| {
            let _ = ui.button("Save");
            if ui.input(|i| i.pointer.any_pressed()) {
                note_copy("noted");
            }
        };
        assert_eq!(drive("<Click:Save><Clipboard:noted>", false, app), Some(Do::Done));
        assert_eq!(drive("<Click:Save><ClipboardHas:note>", false, app), Some(Do::Done));
    }

    #[test]
    fn announcements_count_only_after_the_last_action() {
        ito_a11y::record_announcements(true);
        let app = |ui: &mut egui::Ui, i: usize| {
            if i == 0 {
                ito_a11y::announce(ui, "mark-test first");
            }
            if typed(ui, "a") {
                ito_a11y::announce(ui, "mark-test after a");
            }
        };
        assert_eq!(drive("<Announced:mark-test first>", true, app), Some(Do::Done));
        assert_eq!(drive("a<Wait:20><Announced:mark-test after a>", true, app), Some(Do::Done));
        match drive("ab<Announced:mark-test after a>", true, app) {
            Some(Do::Failed { why, .. }) => assert!(why.contains("not announced after the last action"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_after_checks_use_a_hand_made_mark() {
        let _g = lock_copies();
        ito_a11y::record_announcements(true);
        let ctx = egui::Context::default();
        assert_eq!(mark(&ctx).copies, 0);
        assert_eq!(check_clipboard_after(&ctx, Mark::default(), "x").unwrap_err(), "nothing copied yet, not `x`");
        observe(&ctx);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.ctx().copy_text("y".into());
            ito_a11y::announce(ui, "mark-direct one");
        })
        .drop_without_applying_deltas();
        let m = mark(&ctx);
        assert_eq!(m.copies, 1);
        assert!(m.announced >= 1);
        assert_eq!(check_clipboard_after(&ctx, Mark { copies: 0, announced: 0 }, "y"), Ok(()));
        assert_eq!(check_clipboard_has_after(&ctx, Mark { copies: 0, announced: 0 }, "y"), Ok(()));
        assert_eq!(check_clipboard_after(&ctx, Mark { copies: 0, announced: 0 }, "z").unwrap_err(), "copied `y`, not `z`");
        assert_eq!(check_clipboard_has_after(&ctx, Mark { copies: 0, announced: 0 }, "z").unwrap_err(), "copied `y`, which has no `z`");
        let why = check_clipboard_after(&ctx, m, "x").unwrap_err();
        assert_eq!(why, "nothing copied after the last action, not `x` (the last copy, before it, was `y`)");
        assert!(check_clipboard_has_after(&ctx, m, "y").unwrap_err().contains("nothing copied after the last action"));
        assert_eq!(check_announced_after(Mark { copies: 0, announced: 0 }, "mark-direct one"), Ok(()));
        let why = check_announced_after(m, "mark-direct nope").unwrap_err();
        assert!(why.contains("`mark-direct nope` was not announced after the last action; announced after it: "), "{why}");
        assert!(check_announced("mark-direct nope").unwrap_err().contains("was not announced; announced: "));
    }

    #[test]
    fn the_announced_after_message_names_what_came_before() {
        let s = |v: &[&str]| v.iter().map(|m| m.to_string()).collect::<Vec<_>>();
        assert_eq!(
            not_announced_after("x", &[], &s(&["a", "b"])),
            "`x` was not announced after the last action; announced after it: nothing (before it: `b`, `a`)"
        );
        assert_eq!(not_announced_after("x", &[], &[]), "`x` was not announced after the last action; announced after it: nothing");
        assert_eq!(
            not_announced_after("x", &s(&["c"]), &s(&["a"])),
            "`x` was not announced after the last action; announced after it: `c`"
        );
    }

    /// Before the first action the Runner uses the old message; after one, the new one, even when nothing was announced
    /// before that action.
    #[test]
    fn the_runner_words_announced_by_whether_an_action_went_in() {
        let app = |_: &mut egui::Ui, _: usize| {};
        match drive("<Announced:wording-test never>", true, app) {
            Some(Do::Failed { why, .. }) => assert!(why.contains("`wording-test never` was not announced; announced: "), "{why}"),
            other => panic!("{other:?}"),
        }
        match drive("a<Announced:wording-test never>", true, app) {
            Some(Do::Failed { why, .. }) => assert!(why.contains("`wording-test never` was not announced after the last action"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    /// #145: states, the tree file and announcements are checked from the AccessKit tree.
    #[test]
    fn the_runner_checks_states_the_tree_and_announcements() {
        let steps = parse("<Enabled:Go><Disabled:Stop><Checked:Dark><Unchecked:Beep><Focused:hi><Tree:t1><Announced:Saved now>").unwrap();
        for s in &steps {
            assert_eq!(parse(&label(s)).unwrap(), vec![s.clone()]);
        }
        assert!(parse("<Tree:a b>").is_err());
        let dir = std::env::temp_dir().join(format!("ito-keyscript-tree-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut text = String::from("hi");
        let (mut dark, mut beep) = (true, false);
        let ctx = egui::Context::default();
        let mut r = Runner::new(steps, Some(dir.join("done")));
        let mut out = None;
        for i in 0..400 {
            r.next = Instant::now();
            let mut raw = egui::RawInput::default();
            if let Some(d) = r.input(&ctx, &mut raw) {
                out = Some(d);
                break;
            }
            ctx.run_ui(raw, |ui| {
                let _ = ui.button("Go");
                ui.add_enabled(false, egui::Button::new("Stop"));
                ui.checkbox(&mut dark, "Dark");
                ui.checkbox(&mut beep, "Beep");
                let name = ui.add(egui::TextEdit::singleline(&mut text).id(egui::Id::new("name")));
                if i == 0 {
                    name.request_focus();
                }
                if i == 3 {
                    ito_a11y::announce(ui, "Saved now");
                }
            })
            .drop_without_applying_deltas();
        }
        assert_eq!(out, Some(Do::Done));
        let tree = std::fs::read_to_string(dir.join("t1.txt")).unwrap();
        assert!(tree.contains("button \"Go\""), "{tree}");
        assert!(tree.contains("checkbox \"Dark\" checked") || tree.contains("\"Dark\" checked"), "{tree}");
        assert!(tree.contains("\"Stop\" disabled"), "{tree}");
        let _ = std::fs::remove_dir_all(&dir);

        // A wrong state fails with what the thing is.
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("<Checked:Beep>").unwrap(), None);
        let mut out = None;
        for _ in 0..4 {
            r.next = Instant::now();
            let mut raw = egui::RawInput::default();
            if let Some(d) = r.input(&ctx, &mut raw) {
                out = Some(d);
                break;
            }
            ctx.run_ui(raw, |ui| {
                ui.checkbox(&mut false, "Beep");
            })
            .drop_without_applying_deltas();
            r.deadline = Some(Instant::now());
        }
        match out {
            Some(Do::Failed { step, why }) => {
                assert_eq!(step, "<Checked:Beep>");
                assert!(why.contains("unchecked"), "{why}");
            }
            other => panic!("{other:?}"),
        }
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
        assert_eq!(run_lines(&said), 1, "{said}");
        assert!(said.contains("pressed: 2 of 3 (last: `<Wait:2000>`)\nleft: k\n"), "{said}");
        assert_eq!(watched(Duration::from_secs(60), "waiting.done"), None, "still inside a <Wait:60000>");
        // No done file, nothing to watch.
        let mut r = Runner::new(parse("j").unwrap(), None);
        r.watch(egui::Context::default());
        assert!(r.progress.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// eframe's input carry-over: `prepare` takes what a logic-only tick put back, appends the new input and calls the hook;
    /// `logic_only` runs no pass and puts the prepared input back.
    struct Frames {
        pending: egui::RawInput,
    }

    impl Frames {
        fn new() -> Self {
            Self { pending: egui::RawInput::default() }
        }

        fn prepare(&mut self, r: &mut Runner, ctx: &egui::Context, new: egui::RawInput) -> (egui::RawInput, Option<Do>) {
            let mut raw = std::mem::take(&mut self.pending);
            raw.append(new);
            r.next = Instant::now();
            let did = r.input(ctx, &mut raw);
            (raw, did)
        }

        fn logic_only(&mut self, raw: egui::RawInput) {
            self.pending = raw;
        }
    }

    fn child() -> egui::ViewportId {
        egui::ViewportId::from_hash_of("child")
    }

    fn child_input() -> egui::RawInput {
        let mut raw = egui::RawInput { viewport_id: child(), ..Default::default() };
        raw.viewports.insert(child(), egui::ViewportInfo::default());
        raw
    }

    fn texts_in(raw: &egui::RawInput, what: &str) -> usize {
        raw.events.iter().filter(|e| matches!(e, egui::Event::Text(t) if t == what)).count()
    }

    /// A pass in which the window draws what `ui_fn` says.
    fn draw(ctx: &egui::Context, raw: egui::RawInput, ui_fn: impl FnMut(&mut egui::Ui)) {
        ctx.run_ui(raw, ui_fn).drop_without_applying_deltas();
    }

    /// An input that waits through logic-only ticks (a minimized window) goes in once, and the next one only after
    /// the main window has drawn it.
    #[test]
    fn an_input_goes_in_once_through_logic_only_ticks() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("ab").unwrap(), None);
        let mut f = Frames::new();
        let (raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
        assert_eq!((did, texts_in(&raw, "a"), r.left()), (None, 1, 1));
        f.logic_only(raw);
        for _ in 0..3 {
            let (raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
            assert_eq!(did, None);
            assert_eq!((texts_in(&raw, "a"), texts_in(&raw, "b"), r.left()), (1, 0, 1), "carried over, nothing added");
            f.logic_only(raw);
        }
        let (raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
        assert_eq!((did, texts_in(&raw, "a"), r.left()), (None, 1, 1));
        let mut seen = 0;
        draw(&ctx, raw, |ui| seen = ui.input(|i| i.raw.events.iter().filter(|e| matches!(e, egui::Event::Text(t) if t == "a")).count()));
        assert_eq!(seen, 1, "the pass saw `a` exactly once");
        // The hook after the delivering pass delivers, and only then puts `b` in.
        let (raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
        assert_eq!((did, texts_in(&raw, "a"), texts_in(&raw, "b"), r.left()), (None, 0, 1, 0));
    }

    /// eframe gives what is left over to the next window; the runner puts nothing into a child hook, and fails the
    /// run when the input would have landed there.
    #[test]
    fn a_child_viewport_never_gets_the_input() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("ab").unwrap(), None);
        let mut f = Frames::new();
        let (raw, _) = f.prepare(&mut r, &ctx, egui::RawInput::default());
        f.logic_only(raw);
        let (raw, did) = f.prepare(&mut r, &ctx, child_input());
        match did {
            Some(Do::Failed { step, why }) => {
                assert_eq!(step, "a");
                assert!(why.contains("viewport"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        assert!(!r.running());
        assert_eq!(texts_in(&raw, "a"), 1, "only what was carried over");

        let mut r = Runner::new(parse("ab").unwrap(), None);
        let mut f = Frames::new();
        let (raw, did) = f.prepare(&mut r, &ctx, child_input());
        assert_eq!((did, raw.events.len(), r.left(), r.running()), (None, 0, 2, true));

        // `<Theme:>` puts nothing into the input: a child's hook before the main window draws is not a failure.
        let mut r = Runner::new(parse("<Theme:dark>a").unwrap(), None);
        let mut f = Frames::new();
        let (raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
        assert_eq!(did, Some(Do::Theme("dark".into())));
        f.logic_only(raw);
        let (_, did) = f.prepare(&mut r, &ctx, child_input());
        assert_eq!((did, r.left(), r.running()), (None, 1, true));
    }

    /// An input nobody draws fails the run after `DELIVER_WAIT`.
    #[test]
    fn an_input_not_drawn_in_time_fails_the_run() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("ab").unwrap(), None);
        let mut f = Frames::new();
        let (raw, _) = f.prepare(&mut r, &ctx, egui::RawInput::default());
        f.logic_only(raw);
        let (raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
        assert_eq!(did, None);
        f.logic_only(raw);
        // Waiting for the draw asks for frames no faster than `FRAME`.
        r.next = Instant::now();
        assert_eq!(r.due(), Some(FRAME));
        let flight = r.flight.as_mut().expect("in flight");
        flight.at = flight.at.checked_sub(DELIVER_WAIT).unwrap();
        let (_, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
        match did {
            Some(Do::Failed { why, .. }) => assert!(why.contains("not drawn within"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(!r.running());
    }

    /// The pass that draws the input is not the window the next reading step may judge.
    #[test]
    fn the_delivering_pass_does_not_count_for_a_reading_step() {
        let run = |script: &str, expire: bool| {
            let ctx = egui::Context::default();
            let mut r = Runner::new(parse(script).unwrap(), None);
            let mut f = Frames::new();
            let mut kept = false;
            for i in 0..40 {
                let (raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
                if did.is_some() {
                    return did;
                }
                draw(&ctx, raw, |ui| {
                    if ui.input(|i| i.raw.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "x"))) {
                        ui.label("Flash");
                        kept = true;
                    }
                    if kept {
                        ui.label("Kept");
                    }
                });
                if expire && i >= 6 {
                    r.deadline = Some(Instant::now());
                }
            }
            None
        };
        assert_eq!(run("x<Assert:Kept>", false), Some(Do::Done));
        match run("x<Assert:Flash>", true) {
            Some(Do::Failed { step, why }) => {
                assert_eq!(step, "<Assert:Flash>");
                assert!(why.contains("Flash"), "{why}");
            }
            other => panic!("{other:?}"),
        }
    }

    /// Only the main window is observed; a pass with no tree empties the observation; the generation never goes back.
    #[test]
    fn only_the_main_window_is_observed() {
        let ctx = egui::Context::default();
        assert_eq!(generation(&ctx), 0);
        assert!(observation(&ctx).is_none());
        observe(&ctx);
        assert_eq!(generation(&ctx), 0);
        for _ in 0..2 {
            draw(&ctx, egui::RawInput::default(), |ui| {
                ui.label("Root");
            });
        }
        let seen = observation(&ctx).expect("a main-window pass brought a tree");
        assert!(seen.generation >= 1 && seen.generation == generation(&ctx));
        assert!(seen.items.iter().any(|i| i.text == "Root"));
        assert_eq!(seen.viewport, egui::ViewportId::ROOT);
        assert_eq!(seen.pass_nr, ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT));
        assert!(!seen.tree.is_empty());

        draw(&ctx, child_input(), |ui| {
            ui.label("Child");
        });
        assert_eq!(generation(&ctx), seen.generation);
        assert_eq!(observation(&ctx).map(|o| o.generation), Some(seen.generation));
        assert!(items(&ctx).iter().all(|i| i.text != "Child"));

        ctx.disable_accesskit();
        draw(&ctx, egui::RawInput::default(), |_| {});
        let after = generation(&ctx);
        assert!(after >= seen.generation);
        draw(&ctx, egui::RawInput::default(), |_| {});
        assert!(observation(&ctx).is_none());
        assert!(items(&ctx).is_empty());
        assert_eq!(generation(&ctx), after, "monotonic: a pass without a tree does not count");
    }

    /// A reading step with no newer observation, or in a minimized window, retries and then says why.
    #[test]
    fn a_reading_step_says_why_it_could_not_judge() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("j<Assert:Kept>").unwrap(), None);
        let mut f = Frames::new();
        let mut out = None;
        for _ in 0..10 {
            let (raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
            if did.is_some() {
                out = did;
                break;
            }
            // Only the pass that draws the key happens; the delivery is then confirmed but no newer tree comes.
            if r.flight.is_some() {
                draw(&ctx, raw, |ui| {
                    ui.label("Kept");
                });
            }
            r.deadline = Some(Instant::now());
        }
        match out {
            Some(Do::Failed { why, .. }) => assert!(why.contains("not updated after the last action"), "{why}"),
            other => panic!("{other:?}"),
        }

        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("<Assert:Kept>").unwrap(), None);
        let mut f = Frames::new();
        let mut out = None;
        for _ in 0..10 {
            let (mut raw, did) = f.prepare(&mut r, &ctx, egui::RawInput::default());
            if did.is_some() {
                out = did;
                break;
            }
            raw.viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo { minimized: Some(true), ..Default::default() });
            draw(&ctx, raw, |ui| {
                ui.label("Kept");
            });
            r.deadline = Some(Instant::now());
        }
        match out {
            Some(Do::Failed { why, .. }) => assert!(why.contains("minimized"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    /// Every wait inside the runner starts after the last progress the watch saw, so the longest quiet is a settle plus
    /// one wait, which must stay under `STALL`.
    #[test]
    fn the_waits_stay_under_the_stall() {
        assert!(DELIVER_WAIT < STALL && TEXT_WAIT < STALL && READ_WAIT < STALL);
        assert!(SETTLE + DELIVER_WAIT.max(TEXT_WAIT).max(READ_WAIT) < STALL);
    }

    const PANE: Dialect<'static> = Dialect { reads: &[("PaneText", "pane text")], shot_suffixes: &[] };

    fn window() -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() }
    }

    /// The main window's observation after a few passes of `ui_fn`.
    fn seen(ctx: &egui::Context, mut ui_fn: impl FnMut(&mut egui::Ui)) -> Observation {
        observe(ctx);
        for _ in 0..3 {
            draw(ctx, window(), &mut ui_fn);
        }
        observation(ctx).expect("a tree")
    }

    fn button(ui: &mut egui::Ui, name: &str) -> egui::Response {
        let r = ui.button(name);
        ito_a11y::test_id(&r, name);
        r
    }

    #[test]
    fn id_steps_round_trip_and_refuse_bad_names() {
        for script in [
            "<ClickId:a.b-c_d><FocusedId:a.b-c_d>",
            "<Id:a>",
            "<Id:!a>",
            "<EnabledId:a>",
            "<DisabledId:a>",
            "<ValueId:a.b=x=y>",
            "<ValueId:a=>",
        ] {
            let steps = parse(script).unwrap_or_else(|e| panic!("{script}: {e}"));
            let again: String = steps.iter().map(label).collect();
            assert_eq!(again, script);
            assert_eq!(parse(&again).unwrap(), steps);
        }
        assert_eq!(parse("<ValueId:a.b=x=y>").unwrap(), vec![Step::IdCheck(IdCheck::Value { name: "a.b".into(), text: "x=y".into() })]);
        assert_eq!(parse("<ValueId:a=>").unwrap(), vec![Step::IdCheck(IdCheck::Value { name: "a".into(), text: String::new() })]);
        assert_eq!(parse("<Id:!a>").unwrap(), vec![Step::IdCheck(IdCheck::Absent("a".into()))]);
        for bad in ["<Id:A>", "<Id:a..b>", "<Id:>", "<Id:!>", "<Id:!A>", "<ClickId:>", "<ValueId:a>", "<ValueId:A=x>", "<Id:a>*2"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(parse("<ClickId:a><FocusedId:a>*2").is_err());
    }

    #[test]
    fn a_click_id_has_to_be_followed_by_a_check() {
        for ok in [
            "<ClickId:a><FocusedId:a>",
            "<ClickId:a><ValueId:a=x>",
            "<ClickId:a><Id:b>",
            "<ClickId:a><Id:!a>",
            "<ClickId:a><Wait:100><Id:b>",
        ] {
            parse(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        parse_with("<ClickId:a><PaneText:f><Id:!a>", &PANE).unwrap();
        for bad in [
            "<ClickId:a>",
            "<ClickId:a><Wait:100>",
            "<ClickId:a>j",
            "<ClickId:a><EnabledId:a>",
            "<ClickId:a><DisabledId:a>",
            "<ClickId:a><Assert:x>",
            "<ClickId:a><ClickId:b><FocusedId:b>",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_empty_text_to_look_for_is_refused() {
        for bad in ["<Assert:>", "<Assert:!>", "<Clipboard:>", "<ClipboardHas:>", "<Announced:>", "<Click:>", "<RClick:>", "<DClick:>"] {
            let e = parse(bad).expect_err(bad);
            assert!(e.contains("nothing to look for"), "{bad}: {e}");
        }
        for ok in ["<Paste:>", "<Preedit:>", "<ValueId:a=>"] {
            parse(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
    }

    #[test]
    fn read_has_parses_against_the_dialect() {
        let steps = parse_with("<ReadHas:PaneText=ok\\x3e>", &PANE).unwrap();
        assert_eq!(steps, vec![Step::ReadHas { tag: "PaneText".into(), text: "ok>".into() }]);
        for bad in ["<ReadHas:Nope=a>", "<ReadHas:PaneText>", "<ReadHas:PaneText=>", "<ReadHas:PaneText=a>*2"] {
            assert!(parse_with(bad, &PANE).is_err(), "{bad}");
        }
        assert!(parse("<ReadHas:PaneText=a>").is_err());
    }

    #[test]
    fn a_widget_with_a_test_id_is_observed() {
        let ctx = egui::Context::default();
        let obs = seen(&ctx, |ui| {
            button(ui, "b");
        });
        let node = obs.id("b").unwrap().expect("found");
        assert_eq!(node.name, "b");
        assert!(node.enabled && !node.hidden && !node.focused);
        assert_eq!(node.click_at, Ok(node.interact_rect.center()));
        assert!(obs.id("other").unwrap().is_none());
        assert_eq!(IdCheck::Present("b".into()).check(&obs), Ok(()));
    }

    #[test]
    fn a_name_on_two_widgets_or_two_names_on_one_are_errors() {
        let ctx = egui::Context::default();
        let obs = seen(&ctx, |ui| {
            let a = ui.button("one");
            ito_a11y::test_id(&a, "dup");
            let b = ui.button("two");
            ito_a11y::test_id(&b, "dup");
        });
        let e = obs.id("dup").unwrap_err();
        assert!(e.contains("`dup`") && e.contains("2 widgets"), "{e}");
        assert!(IdCheck::Absent("dup".into()).check(&obs).is_err());
        assert!(IdCheck::Present("dup".into()).check(&obs).is_err());

        let ctx = egui::Context::default();
        let obs = seen(&ctx, |ui| {
            let a = ui.button("one");
            ito_a11y::test_id(&a, "first");
            ito_a11y::test_id(&a, "second");
        });
        for name in ["first", "second"] {
            let e = obs.id(name).unwrap_err();
            assert!(e.contains("first") && e.contains("second") && e.contains("same widget"), "{name}: {e}");
        }
    }

    #[test]
    fn a_gone_widget_is_none_and_accesskit_off_gives_no_ids() {
        let ctx = egui::Context::default();
        observe(&ctx);
        for _ in 0..3 {
            draw(&ctx, window(), |ui| {
                button(ui, "b");
            });
        }
        assert!(observation(&ctx).unwrap().id("b").unwrap().is_some());
        for _ in 0..3 {
            draw(&ctx, window(), |ui| {
                ui.label("none");
            });
        }
        let obs = observation(&ctx).unwrap();
        assert!(obs.id("b").unwrap().is_none());
        assert_eq!(IdCheck::Absent("b".into()).check(&obs), Ok(()));

        // AccessKit off (no `observe`): `test_id` notes nothing, so nothing is left to take.
        let ctx = egui::Context::default();
        for _ in 0..3 {
            draw(&ctx, window(), |ui| {
                button(ui, "b");
            });
        }
        assert!(ito_a11y::take_test_ids(&ctx, egui::ViewportId::ROOT).is_empty());
    }

    #[test]
    fn click_at_says_why_a_widget_cannot_be_clicked() {
        let ctx = egui::Context::default();
        let obs = seen(&ctx, |ui| {
            let r = ui.add_enabled(false, egui::Button::new("d"));
            ito_a11y::test_id(&r, "off");
        });
        let node = obs.id("off").unwrap().expect("found");
        assert!(!node.enabled);
        assert!(node.click_at.clone().unwrap_err().contains("disabled"));

        let ctx = egui::Context::default();
        let obs = seen(&ctx, |ui| {
            let r = ui.put(egui::Rect::from_min_size(egui::pos2(5000.0, 5000.0), egui::vec2(40.0, 20.0)), egui::Button::new("far"));
            ito_a11y::test_id(&r, "far");
        });
        let why = obs.id("far").unwrap().expect("found").click_at.clone().unwrap_err();
        assert!(why.contains("outside the window") || why.contains("no area"), "{why}");

        let ctx = egui::Context::default();
        let obs = seen(&ctx, |ui| {
            button(ui, "under");
            egui::Modal::new(egui::Id::new("m")).show(ui.ctx(), |ui| {
                ui.label("modal");
            });
        });
        let why = obs.id("under").unwrap().expect("found").click_at.clone().unwrap_err();
        assert!(why.contains("modal") || why.contains("covered"), "{why}");
    }

    #[test]
    fn id_checks_say_what_they_found() {
        let ctx = egui::Context::default();
        let (mut full, mut empty) = ("hi".to_owned(), String::new());
        let obs = seen(&ctx, |ui| {
            button(ui, "b");
            let r = ui.add_enabled(false, egui::Button::new("d"));
            ito_a11y::test_id(&r, "d");
            let r = ui.text_edit_singleline(&mut full);
            ito_a11y::test_id(&r, "full");
            let r = ui.text_edit_singleline(&mut empty);
            ito_a11y::test_id(&r, "empty");
        });
        let v = |name: &str, text: &str| IdCheck::Value { name: name.into(), text: text.into() };
        assert_eq!(IdCheck::Present("b".into()).check(&obs), Ok(()));
        assert!(IdCheck::Present("zz".into()).check(&obs).unwrap_err().contains("`zz`"));
        assert!(IdCheck::Absent("b".into()).check(&obs).is_err());
        assert_eq!(IdCheck::Absent("zz".into()).check(&obs), Ok(()));
        assert_eq!(IdCheck::Enabled("b".into()).check(&obs), Ok(()));
        assert!(IdCheck::Enabled("d".into()).check(&obs).is_err());
        assert_eq!(IdCheck::Disabled("d".into()).check(&obs), Ok(()));
        assert!(IdCheck::Disabled("b".into()).check(&obs).is_err());
        assert!(IdCheck::Enabled("zz".into()).check(&obs).is_err());
        assert!(IdCheck::Focused("b".into()).check(&obs).is_err());
        assert_eq!(v("full", "hi").check(&obs), Ok(()));
        assert!(v("full", "ho").check(&obs).unwrap_err().contains("hi"));
        assert_eq!(v("empty", "").check(&obs), Ok(()));
        assert!(v("full", "").check(&obs).is_err());
        assert!(v("zz", "x").check(&obs).is_err());
    }

    #[test]
    fn a_click_id_lands_and_the_check_passes() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("<ClickId:t><FocusedId:t>").unwrap(), None);
        let mut f = Frames::new();
        let mut text = String::new();
        let mut out = None;
        for _ in 0..200 {
            let (raw, did) = f.prepare(&mut r, &ctx, window());
            if did.is_some() {
                out = did;
                break;
            }
            draw(&ctx, raw, |ui| {
                let resp = ui.text_edit_singleline(&mut text);
                ito_a11y::test_id(&resp, "t");
            });
        }
        assert_eq!(out, Some(Do::Done));
    }

    #[test]
    fn a_click_id_on_a_missing_widget_fails_with_why() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse("<ClickId:missing><Id:missing>").unwrap(), None);
        let mut f = Frames::new();
        let mut out = None;
        for _ in 0..200 {
            let (raw, did) = f.prepare(&mut r, &ctx, window());
            if did.is_some() {
                out = did;
                break;
            }
            draw(&ctx, raw, |ui| {
                ui.label("nothing");
            });
            if r.deadline.is_some() {
                r.deadline = Some(Instant::now());
            }
        }
        match out {
            Some(Do::Failed { why, .. }) => assert!(why.contains("no widget has the test ID `missing`"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    struct Replies {
        replies: Vec<Result<Option<String>, String>>,
        at: usize,
    }

    impl Reader for Replies {
        fn read(&mut self, _name: &str) -> Result<Option<String>, String> {
            let reply = self.replies[self.at.min(self.replies.len() - 1)].clone();
            self.at += 1;
            reply
        }
    }

    /// Run `<ReadHas:PaneText=ok>` against `replies`; from frame `expire_at` on the wait counts as over.
    fn read_run(replies: Vec<Result<Option<String>, String>>, expire_at: Option<usize>) -> (Option<Do>, Runner) {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse_with("<ReadHas:PaneText=ok>", &PANE).unwrap(), None);
        let mut reader = Replies { replies, at: 0 };
        let mut out = None;
        for i in 0..40 {
            let mut raw = egui::RawInput::default();
            r.next = Instant::now();
            let did = r.input_with(&ctx, &mut raw, &mut reader);
            if did.is_some() {
                out = did;
                break;
            }
            if expire_at.is_some_and(|k| i >= k) {
                r.deadline = Some(Instant::now());
            }
        }
        (out, r)
    }

    #[test]
    fn read_has_waits_for_the_text() {
        let (out, r) = read_run(vec![Ok(None), Ok(None), Ok(Some("all ok here".into()))], None);
        assert_eq!(out, Some(Do::Done));
        assert_eq!(r.action_gen, 0, "reading is not an operation");
        assert!(r.flight.is_none());

        let (out, _) = read_run(vec![Err("gone".into())], None);
        assert!(matches!(out, Some(Do::Failed { ref why, .. }) if why == "gone"), "{out:?}");

        let long = format!("{}END", "x".repeat(300));
        let (out, _) = read_run(vec![Ok(Some(long))], Some(3));
        match out {
            Some(Do::Failed { why, .. }) => {
                assert_eq!(why.chars().count(), 201);
                assert!(why.starts_with('\u{2026}') && why.ends_with("END"), "{why}");
            }
            other => panic!("{other:?}"),
        }

        let (out, _) = read_run(vec![Ok(None)], Some(3));
        assert!(matches!(out, Some(Do::Failed { ref why, .. }) if why == "nothing to read (no pane)"), "{out:?}");
    }

    #[test]
    fn input_without_a_reader_fails_a_read_has() {
        let ctx = egui::Context::default();
        let mut r = Runner::new(parse_with("<ReadHas:PaneText=ok>", &PANE).unwrap(), None);
        let mut raw = egui::RawInput::default();
        r.next = Instant::now();
        let out = r.input(&ctx, &mut raw);
        assert!(matches!(out, Some(Do::Failed { ref why, .. }) if why == "this app does not read PaneText"), "{out:?}");
    }
}
