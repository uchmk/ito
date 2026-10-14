//! A yazi-style `keymap.toml`, shared by uchmk's apps that let their keys be
//! changed (kura's `config/keymap.rs` is where it started).
//!
//! A file has a section per layer (`[mgr]`, `[input]` …, as the app names
//! them), each with up to three lists of bindings: `keymap` replaces every
//! binding before it, `prepend_keymap` goes in front, `append_keymap` behind.
//! The built-in defaults are folded first, then each of the user's files in
//! turn, so a later file builds on an earlier one. The first binding that
//! matches wins, which is what makes a prepended single key shadow a default
//! chord that starts with it.
//!
//! ```toml
//! [[mgr.prepend_keymap]]
//! on = "<C-e>"              # or "g g", "gg", ["g", "g"]
//! on_mac = "<C-A-e>"        # what `on` is on macOS, when it differs
//! for = "windows"           # yazi's: only on windows | macos | linux | unix
//! run = "shell 'nvim'"      # or a list: each runs in turn
//! desc = "Edit in nvim"
//! ```
//!
//! What a command *is* stays in each app: [`load`] hands every `run` string
//! to the app's parser. The keys' spelling is [`ito_keys`]'s.
//!
//! Besides the bindings, a load says what in the files can never work: keys
//! that cannot be written or pressed, a short `keymap` that throws the
//! defaults away, a key bound twice, a single key in front of the chords that
//! start with it. A user's key that takes the place of a default one is meant,
//! so it is not a warning but an *override*, kept for the help to show.

use std::collections::BTreeMap;
use std::sync::Arc;

use ito_keys::{render_seq, Code, Key};
use serde::Deserialize;

/// One binding: the keys, the commands they run, and where it was written.
#[derive(Clone, Debug)]
pub struct Binding<A> {
    pub on: Vec<Key>,
    pub run: Vec<A>,
    pub desc: String,
    /// The command text as written (`;` between commands), for the help.
    pub raw: String,
    /// Where it was written: a keymap file's path, or [`BUILT_IN`]. A key
    /// bound twice is warned about by naming both, so the warning says which
    /// file to open.
    pub from: Arc<str>,
}

/// What [`Binding::from`] says for a binding from the built-in defaults.
pub const BUILT_IN: &str = "the built-in defaults";

/// A layer of an app's keymap: the section it is written under, and other
/// names it answers to (kura's `[mgr]` is also yazi's older `[manager]`).
#[derive(Clone, Copy, Debug)]
pub struct Layer<'a> {
    pub name: &'a str,
    pub aliases: &'a [&'a str],
}

impl<'a> Layer<'a> {
    pub const fn new(name: &'a str) -> Self {
        Self { name, aliases: &[] }
    }
}

/// What an app's keymap is made of.
#[derive(Clone, Copy, Debug)]
pub struct Spec<'a> {
    pub layers: &'a [Layer<'a>],
    /// Sections read and left alone without a word (yazi's `[cmp]` in kura).
    pub ignored: &'a [&'a str],
    /// The built-in keymap, in the same form as the user's files.
    pub defaults: &'a str,
}

/// A loaded keymap: the bindings of each layer of the [`Spec`], in its order.
#[derive(Debug)]
pub struct Keymap<A> {
    pub layers: Vec<(String, Vec<Binding<A>>)>,
    /// Everything in the files that does not work as written. Parse errors
    /// come first, one per file that does not parse, in file order.
    pub warnings: Vec<String>,
    /// A key of the built-in defaults that a user's file binds to something
    /// else: meant, so not a warning, but where "what did my `T` displace?"
    /// can be looked up.
    pub overrides: Vec<String>,
}

impl<A> Keymap<A> {
    /// The bindings of the layer `name` (empty when the spec has none).
    pub fn layer(&self, name: &str) -> &[Binding<A>] {
        self.layers.iter().find(|(n, _)| n == name).map_or(&[], |(_, b)| b)
    }

    /// The bindings of the layer `name`, taken out, for the app's own struct.
    pub fn take(&mut self, name: &str) -> Vec<Binding<A>> {
        self.layers.iter_mut().find(|(n, _)| n == name).map(|(_, b)| std::mem::take(b)).unwrap_or_default()
    }
}

/// What a key sequence pressed so far comes to in a layer.
#[derive(Debug)]
pub enum Match<'a, A> {
    /// Run this binding.
    Exact(&'a Binding<A>),
    /// More keys are needed; these are the candidates, for a panel that shows
    /// what may follow.
    Pending(Vec<&'a Binding<A>>),
    None,
}

/// The first binding whose keys are `pending`, or the ones that start with it.
pub fn resolve<'a, A>(bindings: &'a [Binding<A>], pending: &[Key]) -> Match<'a, A> {
    let candidates: Vec<&Binding<A>> =
        bindings.iter().filter(|b| b.on.len() >= pending.len() && b.on[..pending.len()] == *pending).collect();
    match candidates.first() {
        None => Match::None,
        Some(first) if first.on.len() == pending.len() => Match::Exact(first),
        Some(_) => Match::Pending(candidates),
    }
}

// ---------------------------------------------------------------- the file

type File = BTreeMap<String, Section>;

#[derive(Deserialize, Debug, Default)]
struct Section {
    #[serde(default)]
    keymap: Option<Vec<Entry>>,
    #[serde(default)]
    prepend_keymap: Vec<Entry>,
    #[serde(default)]
    append_keymap: Vec<Entry>,
    #[serde(flatten)]
    other: BTreeMap<String, toml::Value>,
}

#[derive(Deserialize, Debug)]
struct Entry {
    on: StrOrVec,
    #[serde(default)]
    on_mac: Option<StrOrVec>,
    #[serde(default, rename = "for")]
    only: Option<String>,
    #[serde(default)]
    run: StrOrVec,
    #[serde(default)]
    desc: String,
    #[serde(flatten)]
    other: BTreeMap<String, toml::Value>,
}

/// A string, or a list of them.
#[derive(Debug, Default)]
struct StrOrVec(Vec<String>);

impl<'de> Deserialize<'de> for StrOrVec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            One(String),
            Many(Vec<String>),
        }
        Ok(match Helper::deserialize(d)? {
            Helper::One(s) => StrOrVec(vec![s]),
            Helper::Many(v) => StrOrVec(v),
        })
    }
}

/// Split a key string into tokens: `"gg"` and `"g g"` both become `[g, g]`,
/// while `"<C-a>"` stays a single key.
fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '<' {
            let mut tok = String::from('<');
            for c in it.by_ref() {
                tok.push(c);
                if c == '>' {
                    break;
                }
            }
            out.push(tok);
        } else if !c.is_whitespace() {
            out.push(c.to_string());
        }
    }
    out
}

/// Whether yazi's `for` lets a binding in on this machine.
fn for_here(only: &str) -> Option<bool> {
    Some(match only {
        "windows" => cfg!(windows),
        "macos" => cfg!(target_os = "macos"),
        "linux" => cfg!(target_os = "linux"),
        "unix" => cfg!(unix),
        _ => return None,
    })
}

/// How the app reads one `run` string: `None` leaves the binding out (the
/// parser says why in the warnings it is handed).
pub type Command<'f, A> = dyn FnMut(&str, &mut Vec<String>) -> Option<A> + 'f;

struct Fold<'c, 'f, A> {
    command: &'c mut Command<'f, A>,
    warnings: Vec<String>,
}

impl<A> Fold<'_, '_, A> {
    fn build(&mut self, layer: &str, e: &Entry, from: &Arc<str>) -> Option<Binding<A>> {
        for key in e.other.keys() {
            self.warnings
                .push(format!("{from}: [{layer}] a binding has `{key}`, which a binding does not take (on, on_mac, for, run, desc)"));
        }
        if let Some(only) = &e.only {
            match for_here(only) {
                Some(true) => {}
                Some(false) => return None,
                None => {
                    self.warnings.push(format!("{from}: [{layer}] `for = \"{only}\"` is none of windows, macos, linux, unix"));
                    return None;
                }
            }
        }
        let on = match (&e.on_mac, cfg!(target_os = "macos")) {
            (Some(mac), true) => mac,
            _ => &e.on,
        };
        let tokens: Vec<String> = if on.0.len() == 1 { tokenize(&on.0[0]) } else { on.0.clone() };
        let mut keys = Vec::with_capacity(tokens.len());
        for t in &tokens {
            match Key::parse(t) {
                // Shift with a printable key and nothing else arrives as the
                // character it types, so this binding can never be pressed:
                // `<S-t>` parses, and then nothing ever matches it. Kept, but said.
                Some(k) if k.shift && !k.ctrl && !k.alt && !k.sup && matches!(k.code, Code::Char(c) if c != ' ') => {
                    let hint = match k.code {
                        Code::Char(c) if c.is_ascii_alphabetic() => format!("write `{}`", c.to_ascii_uppercase()),
                        _ => "write the character Shift types".into(),
                    };
                    self.warnings.push(format!(
                        "`{t}` can never be pressed: Shift with a printable key arrives as the character it types -- {hint}"
                    ));
                    keys.push(k);
                }
                Some(k) => keys.push(k),
                None => {
                    self.warnings.push(format!("unknown key `{t}`"));
                    return None;
                }
            }
        }
        if keys.is_empty() {
            self.warnings.push(format!("{from}: [{layer}] a binding with no key"));
            return None;
        }
        let mut run = Vec::with_capacity(e.run.0.len());
        for s in &e.run.0 {
            run.push((self.command)(s, &mut self.warnings)?);
        }
        Some(Binding { on: keys, run, desc: e.desc.clone(), raw: e.run.0.join("; "), from: from.clone() })
    }

    /// Lay one file's section over what the files before it made of the layer.
    fn fold(&mut self, layer: &str, base: Vec<Binding<A>>, s: &Section, from: &Arc<str>) -> Vec<Binding<A>> {
        for key in s.other.keys() {
            self.warnings
                .push(format!("{from}: [{layer}] has `{key}`, which a section does not take (keymap, prepend_keymap, append_keymap)"));
        }
        let mut out = Vec::new();
        for e in &s.prepend_keymap {
            out.extend(self.build(layer, e, from));
        }
        match &s.keymap {
            Some(list) => {
                // `keymap` replaces every key before it, as in yazi. A copy
                // of the whole keymap does that on purpose; one or two entries
                // almost never do -- one `[[mgr.keymap]]` line left a window
                // no key could even close. Under a quarter of what it replaces
                // is the line between the two.
                if list.len() * 4 < base.len() {
                    self.warnings.push(format!(
                        "{from}: `[[{layer}.keymap]]` replaces all {} keys of [{layer}] with {} -- did you mean `[[{layer}.prepend_keymap]]`?",
                        base.len(),
                        list.len()
                    ));
                }
                for e in list {
                    out.extend(self.build(layer, e, from));
                }
            }
            None => out.extend(base),
        }
        for e in &s.append_keymap {
            out.extend(self.build(layer, e, from));
        }
        out
    }
}

/// Whether `text` reads as a keymap file at all. One that does not is left
/// out by [`load`], with its parse error first among the warnings.
pub fn parses(text: &str) -> bool {
    toml::from_str::<File>(text).is_ok()
}

/// A TOML parse error as a warning shows it, with a hint when the error is a
/// backslash inside a "double-quoted" string.
///
/// `run = "C:\Users\me\nvim.exe"` is the commonest way to break a config on
/// Windows, and the parser's own message -- "too few unicode value digits" or
/// "missing escaped value" -- names the escape rules, not the fix.
pub fn parse_error(text: &str, e: &toml::de::Error) -> String {
    let mut said = e.to_string().trim_end().to_string();
    // The span lands on the backslash, just after it, or -- for `\U` with too
    // few hex digits -- up to nine characters on, still inside the escape.
    let at_escape = e.span().is_some_and(|s| {
        let Some(before) = text.get(..s.start) else { return false };
        let escape = before.rfind('\\').map(|i| &before[i + 1..]);
        escape.is_some_and(|tail| tail.len() <= 9 && tail.chars().all(|c| c.is_ascii_alphanumeric()))
            || text.get(s.start..).is_some_and(|a| a.starts_with('\\'))
    });
    if at_escape {
        said += "\n(a backslash in \"double quotes\" starts an escape: write a Windows path in 'single quotes')";
    }
    said
}

/// The built-in defaults of `spec` with each of the user's files (its path to
/// name in a warning, and its text) laid over them in turn. `command` reads
/// one `run` string.
///
/// # Panics
///
/// When the built-in defaults do not parse: that is the app's own mistake,
/// and its tests load them.
pub fn load<A>(spec: &Spec, files: &[(&str, &str)], command: &mut Command<'_, A>) -> Keymap<A> {
    let mut warnings = Vec::new();
    let mut parsed = vec![(Arc::<str>::from(BUILT_IN), toml::from_str::<File>(spec.defaults).expect("the built-in keymap must parse"))];
    for (path, text) in files {
        match toml::from_str::<File>(text) {
            Ok(f) => parsed.push((Arc::from(*path), f)),
            Err(e) => warnings.push(format!("{path}: {}", parse_error(text, &e))),
        }
    }
    let mut fold = Fold { command, warnings };
    let mut layers: Vec<(String, Vec<Binding<A>>)> = spec.layers.iter().map(|l| (l.name.to_owned(), Vec::new())).collect();
    for (from, file) in &parsed {
        for (section, s) in file {
            let at = spec.layers.iter().position(|l| l.name == section || l.aliases.contains(&section.as_str()));
            match at {
                Some(i) => {
                    let base = std::mem::take(&mut layers[i].1);
                    layers[i].1 = fold.fold(spec.layers[i].name, base, s, from);
                }
                None if spec.ignored.contains(&section.as_str()) => {}
                None => {
                    let known: Vec<String> = spec.layers.iter().map(|l| format!("[{}]", l.name)).collect();
                    fold.warnings.push(format!("{from}: no layer is called [{section}] (there are {})", known.join(", ")));
                }
            }
        }
    }
    let mut warnings = fold.warnings;
    let mut overrides = Vec::new();
    for (name, bindings) in &layers {
        let (warned, replaced) = unreachable(name, bindings);
        warnings.extend(warned);
        overrides.extend(replaced);
    }
    Keymap { layers, warnings, overrides }
}

/// Bindings in `bindings` that no key press can ever reach.
///
/// [`resolve`] takes the first binding that matches, so order decides: a key
/// bound on its own, before a chord that starts with it, ends the sequence
/// where it stands and the chord never runs. A user file's `prepend_keymap`
/// goes in front of everything, which is exactly how one line can silently
/// retire a whole prefix.
///
/// One warning per key that shadows, not per binding shadowed: a prefix with
/// five chords under it is one mistake, and five lines would push everything
/// else out of the panel that shows them.
///
/// A key bound twice is split by where the bindings came from. The same
/// command on both sides loses nothing and is not mentioned. A user's file
/// taking a key from the built-in defaults is what `prepend_keymap` is for, so
/// it goes in the second list, the overrides, not among the warnings. Two of
/// the user's own bindings, or one of theirs that a default shuts out, stay
/// warnings.
fn unreachable<A>(layer: &str, bindings: &[Binding<A>]) -> (Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    let mut replaced = Vec::new();
    let mut lost = vec![false; bindings.len()];
    for i in 0..bindings.len() {
        // Something earlier already swallows this one, and said so.
        if lost[i] {
            continue;
        }
        let on = &bindings[i].on;
        let mut hidden: Vec<String> = Vec::new();
        let mut losers: Vec<&Binding<A>> = Vec::new();
        for j in i + 1..bindings.len() {
            let other = &bindings[j].on;
            if other == on {
                losers.push(&bindings[j]);
            } else if other.len() > on.len() && other[..on.len()] == on[..] {
                hidden.push(render_seq(other));
            } else {
                continue;
            }
            lost[j] = true;
        }
        let key = render_seq(on);
        let winner = &bindings[i];
        let run = &winner.raw;
        losers.retain(|b| b.raw != winner.raw);
        if &*winner.from != BUILT_IN {
            for b in losers.iter().filter(|b| &*b.from == BUILT_IN) {
                replaced.push(format!("[{layer}] `{key}`: `{run}` ({}) instead of the default `{}`", winner.from, b.raw));
            }
            losers.retain(|b| &*b.from != BUILT_IN);
        }
        if !losers.is_empty() {
            // Each side with the file it came from, the winner first: the
            // line alone says which file to open and what to take out of it.
            // The run that loses is named too, since a duplicate is usually
            // noticed as a key that stopped doing what it did.
            let n = losers.len();
            let not: Vec<String> = losers.iter().take(3).map(|b| format!("`{}` ({})", b.raw, b.from)).collect();
            let more = if n > 3 { format!(", and {} more", n - 3) } else { String::new() };
            out.push(format!(
                "[{layer}] `{key}` is bound more than once; only `{run}` ({}) runs, not {}{more}",
                winner.from,
                not.join(", "),
            ));
        }
        if !hidden.is_empty() {
            let n = hidden.len();
            // Three is as many as the line can carry and still be read.
            hidden.truncate(3);
            let shown = hidden.join("`, `");
            let more = if n > 3 { format!(", and {} more", n - 3) } else { String::new() };
            out.push(format!("[{layer}] `{key}` runs `{run}` on its own, so the {n} key(s) starting with it never run: `{shown}`{more}",));
        }
    }
    (out, replaced)
}

#[cfg(test)]
mod tests;
