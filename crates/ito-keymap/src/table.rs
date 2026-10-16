//! The keymap as the settings screen lists it: a row per operation (a
//! layer's `run`), its keys beside it, and which of them a press reaches.

use std::sync::Arc;

use ito_keys::{render_seq, Key};

use crate::{resolve, Binding, Match, BUILT_IN};

/// One operation of a layer: a `run` as written, with every key it is on.
#[derive(Clone, Debug)]
pub struct Row {
    pub layer: String,
    /// The command text as written ([`Binding::raw`]).
    pub run: String,
    /// The built-in description, else the first one a file gives.
    pub desc: String,
    pub keys: Vec<Cell>,
    /// The keys the built-in defaults give it, in their order.
    pub defaults: Vec<Vec<Key>>,
}

impl Row {
    /// Something in a file of the user's changed it: a key added, a default
    /// one taken away or given to something else.
    pub fn changed(&self) -> bool {
        self.keys.iter().any(|k| &*k.from != BUILT_IN || matches!(&k.state, State::Shadowed { from, .. } if &**from != BUILT_IN))
            || self.defaults.iter().any(|d| !self.keys.iter().any(|k| &k.on == d))
    }
}

/// One key of a row.
#[derive(Clone, Debug)]
pub struct Cell {
    pub on: Vec<Key>,
    /// Where it was written: a file's path, or [`BUILT_IN`].
    pub from: Arc<str>,
    pub state: State,
    /// A default key taken away (`noop` in front of it).
    pub removed: bool,
}

/// What a press of a cell's keys does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Runs the row.
    Live,
    /// Runs something else first: the `run`, `desc` and `from` of what does.
    /// For a default key taken away, the `noop` in front of it.
    Shadowed { run: String, desc: String, from: Arc<str> },
}

/// The rows of each layer of `now` (the keymap in force), against
/// `built_in` (the defaults alone). `noop` is the app's command that does
/// nothing: a key bound to it is a default key taken away, not a row.
///
/// The rows come in the defaults' order, an operation only a user's file
/// binds after them; a key bound twice to the same run is shown once.
pub fn rows<A>(now: &[(String, Vec<Binding<A>>)], built_in: &[(String, Vec<Binding<A>>)], noop: &str) -> Vec<Row> {
    let mut out = Vec::new();
    for (layer, bindings) in now {
        let defaults: &[Binding<A>] = built_in.iter().find(|(n, _)| n == layer).map_or(&[], |(_, b)| b);
        let mut runs: Vec<&str> = Vec::new();
        for b in defaults.iter().chain(bindings) {
            if b.raw != noop && !runs.contains(&b.raw.as_str()) {
                runs.push(&b.raw);
            }
        }
        for run in runs {
            let desc =
                defaults.iter().chain(bindings).find(|b| b.raw == run && !b.desc.is_empty()).map(|b| b.desc.clone()).unwrap_or_default();
            let mut keys: Vec<Cell> = Vec::new();
            for b in bindings.iter().filter(|b| b.raw == run) {
                if keys.iter().any(|k| k.on == b.on) {
                    continue;
                }
                // Live only when a press of its keys reaches this very key: a
                // shorter key of the same run in front makes it a dead one.
                let state = match reach(bindings, &b.on) {
                    Some(hit) if hit.raw != run || hit.on != b.on => {
                        State::Shadowed { run: hit.raw.clone(), desc: hit.desc.clone(), from: hit.from.clone() }
                    }
                    _ => State::Live,
                };
                // A `noop` on exactly its keys in front of it took it away
                // (whatever a press now reaches).
                let removed =
                    &*b.from == BUILT_IN && bindings.iter().take_while(|x| !std::ptr::eq(*x, b)).any(|x| x.raw == noop && x.on == b.on);
                keys.push(Cell { on: b.on.clone(), from: b.from.clone(), state, removed });
            }
            let defaults: Vec<Vec<Key>> = defaults.iter().filter(|b| b.raw == run).map(|b| b.on.clone()).collect();
            out.push(Row { layer: layer.clone(), run: run.to_owned(), desc, keys, defaults });
        }
    }
    out
}

/// The bindings of a layer that a key can reach: not `noop` (a key taken from
/// a built-in operation) and not shadowed by an earlier binding with the same
/// keys or a shorter one they start with. What the help and the hints name, so
/// a key taken away is not offered.
pub fn live<'a, A>(layer: &'a [Binding<A>], noop: &'a str) -> impl Iterator<Item = &'a Binding<A>> {
    layer.iter().filter(move |b| {
        b.raw != noop
            && (1..=b.on.len()).all(|i| match resolve(layer, &b.on[..i]) {
                Match::Exact(x) => i == b.on.len() && std::ptr::eq(x, *b),
                Match::Pending(_) => i < b.on.len(),
                Match::None => false,
            })
    })
}

/// One line of a help: the keys that do the same thing, and what it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpLine<'a> {
    /// The keys as the keymap writes them, in the order the layer lists them,
    /// joined by ` / `.
    pub keys: String,
    /// The binding's `desc` as written (the app translates it).
    pub desc: &'a str,
    /// The command text as written.
    pub run: &'a str,
}

/// The [`live`] bindings of a layer, one line per description and command, so
/// the keys that do the same thing sit on one line.
pub fn help_lines<'a, A>(layer: &'a [Binding<A>], noop: &'a str) -> Vec<HelpLine<'a>> {
    let mut out: Vec<HelpLine<'a>> = Vec::new();
    for b in live(layer, noop) {
        let keys = render_seq(&b.on);
        match out.iter_mut().find(|l| l.desc == b.desc && l.run == b.raw) {
            Some(l) => {
                l.keys.push_str(" / ");
                l.keys.push_str(&keys);
            }
            None => out.push(HelpLine { keys, desc: &b.desc, run: &b.raw }),
        }
    }
    out
}

/// What a key given to an operation would take from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clash {
    /// The same keys run another operation now: that one loses them.
    Same { run: String, desc: String },
    /// Keys that start with the new ones, or that the new ones start with,
    /// run another operation: put in front, the new ones win and that one
    /// is never reached. `on` is that one's keys.
    Prefix { run: String, desc: String, on: Vec<Key> },
}

/// What a press of `on`, key by key, comes to: the binding that runs (one
/// on fewer keys may run first), or the first of those a press waits on
/// for more keys. `None` when nothing is on them.
pub fn reach<'a, A>(bindings: &'a [Binding<A>], on: &[Key]) -> Option<&'a Binding<A>> {
    for n in 1..=on.len() {
        match resolve(bindings, &on[..n]) {
            Match::Exact(hit) => return Some(hit),
            Match::Pending(c) if n == on.len() => return c.first().copied(),
            Match::Pending(_) => {}
            Match::None => return None,
        }
    }
    None
}

/// What giving `on` to `run` in a layer whose bindings are `bindings` would
/// take: the bindings reached now that the new keys would shut out. `noop`
/// ones are nothing to lose.
pub fn clashes<A>(bindings: &[Binding<A>], on: &[Key], run: &str, noop: &str) -> Vec<Clash> {
    let mut out = Vec::new();
    for b in bindings {
        if b.raw == run || b.raw == noop {
            continue;
        }
        // Only what a press reaches now can be lost.
        let live = reach(bindings, &b.on).is_some_and(|hit| std::ptr::eq(hit, b));
        if !live {
            continue;
        }
        let n = on.len().min(b.on.len());
        if b.on == on {
            out.push(Clash::Same { run: b.raw.clone(), desc: b.desc.clone() });
        } else if b.on[..n] == on[..n] {
            out.push(Clash::Prefix { run: b.raw.clone(), desc: b.desc.clone(), on: b.on.clone() });
        }
    }
    out
}

/// The keys every uchmk app keeps for itself (`docs/common-spec.md`, the
/// shared keys): their notation, and what they do, in English. A layer key
/// on one of them is reached only where the app lets it through.
pub const COMMON: [(&str, &str); 9] = [
    ("<C-,>", "the settings"),
    ("<F1>", "the help"),
    ("<C-S-p>", "the command palette"),
    ("<C-q>", "quitting"),
    ("<C-=>", "zooming in"),
    ("<C-+>", "zooming in"),
    ("<C-;>", "zooming in"),
    ("<C-->", "zooming out"),
    ("<C-0>", "the zoom back to 100%"),
];

/// Which of [`COMMON`] the first of `on` is (its index there).
pub fn common(on: &[Key]) -> Option<usize> {
    let first = on.first()?;
    COMMON.iter().position(|(k, _)| Key::parse(k).as_ref() == Some(first))
}
