//! A change made on a settings screen, written into the user's keymap.toml.
//!
//! Only what differs from the built-in defaults is written, all of it in the
//! layer's `prepend_keymap`, so the file reads as one written by hand: a key
//! given to an operation is `{ on, run, desc }`, a default key taken away is
//! `{ on, run = "noop" }` (the app's command that does nothing). A change
//! that brings an operation back to its defaults takes its lines out again;
//! an operation nothing changed keeps whatever later defaults give it.

use ito_keys::Key;

use crate::{parses, tokenize, Spec};

/// One change to a layer of the keymap. `run` is the operation's command as
/// written ([`crate::Binding::raw`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Put `on` on `run`, in front of everything else on those keys.
    Add { layer: String, on: Vec<Key>, run: String, desc: String },
    /// Take `on` off `run`.
    Remove { layer: String, on: Vec<Key>, run: String },
    /// Take the file's lines about `run` out: its defaults again.
    Reset { layer: String, run: String },
}

const LISTS: [&str; 2] = ["prepend_keymap", "append_keymap"];

/// One section of the file that holds a layer (its name, or an alias).
struct Lists {
    section: String,
    /// `prepend_keymap`, `append_keymap`.
    lists: [Vec<toml::Table>; 2],
    changed: bool,
}

/// `text` (the user's keymap.toml, empty when there is none) with `edits`
/// made, in turn. `noop` is the app's command that does nothing. The same
/// text back when nothing needs writing: the caller then writes nothing,
/// and makes no file.
///
/// An error, and nothing to write, when the file does not read, when a
/// layer is a `keymap` of its own (it replaces the defaults, so a change
/// there is the person's to make by hand), or when a section is written in
/// a way that cannot be written back (an inline table).
pub fn edit(text: &str, spec: &Spec, edits: &[Edit], noop: &str) -> Result<String, String> {
    if !text.trim().is_empty() && !parses(text) {
        return Err("keymap.toml does not read: fix it, or take it away, before a key is changed here".into());
    }
    let file: toml::Table = toml::from_str(text).map_err(|e| format!("keymap.toml: {}", e.message()))?;
    let defaults: toml::Table = toml::from_str(spec.defaults).map_err(|e| format!("the built-in keymap: {}", e.message()))?;
    let mut layers: Vec<(String, Vec<Lists>)> = Vec::new();
    for e in edits {
        let name = match e {
            Edit::Add { layer, .. } | Edit::Remove { layer, .. } | Edit::Reset { layer, .. } => layer,
        };
        let layer = spec.layers.iter().find(|l| l.name == name).ok_or_else(|| format!("no layer is called [{name}]"))?;
        if !layers.iter().any(|(n, _)| n == name) {
            layers.push((name.clone(), sections(&file, layer.name, layer.aliases)?));
        }
        let at = layers.iter().position(|(n, _)| n == name).expect("just put in");
        let d = default_keys(&defaults, layer.name, layer.aliases, run_of_edit(e));
        apply(&mut layers[at].1, e, &d, noop);
    }
    let mut out = text.to_owned();
    for (_, secs) in &layers {
        for s in secs.iter().filter(|s| s.changed) {
            for (l, list) in LISTS.iter().zip(&s.lists) {
                let blocks: Vec<Vec<(&str, String)>> = list.iter().map(block).collect();
                let path = format!("{}.{l}", s.section);
                // A list that was not there and stays empty is not written:
                // no `[layer]` header for nothing.
                if blocks.is_empty() && !has(&file, &s.section, l) {
                    continue;
                }
                out = ito_common::set_tables(&out, &path, &blocks)?;
            }
        }
    }
    Ok(out)
}

/// An entry's keys and values as TOML, in the order a person writes them.
fn block(t: &toml::Table) -> Vec<(&str, String)> {
    const ORDER: [&str; 5] = ["on", "on_mac", "for", "run", "desc"];
    let mut out: Vec<(&str, String)> = t.iter().map(|(k, v)| (k.as_str(), v.to_string())).collect();
    out.sort_by_key(|(k, _)| ORDER.iter().position(|o| o == k).unwrap_or(ORDER.len()));
    out
}

fn run_of_edit(e: &Edit) -> &str {
    match e {
        Edit::Add { run, .. } | Edit::Remove { run, .. } | Edit::Reset { run, .. } => run,
    }
}

fn has(file: &toml::Table, section: &str, list: &str) -> bool {
    file.get(section).and_then(toml::Value::as_table).is_some_and(|t| t.contains_key(list))
}

/// The sections of `file` that hold the layer, in the file's order; one
/// called `name`, empty, when none does.
fn sections(file: &toml::Table, name: &str, aliases: &[&str]) -> Result<Vec<Lists>, String> {
    let mut out = Vec::new();
    for (section, v) in file {
        if section != name && !aliases.contains(&section.as_str()) {
            continue;
        }
        let t = v.as_table().ok_or_else(|| format!("keymap.toml: [{section}] is not a table"))?;
        if t.contains_key("keymap") {
            return Err(format!(
                "keymap.toml: [{section}] has a `keymap` of its own, which replaces every default key: change it in the file"
            ));
        }
        let list = |l: &str| -> Vec<toml::Table> {
            t.get(l).and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|e| e.as_table().cloned()).collect()).unwrap_or_default()
        };
        out.push(Lists { section: section.clone(), lists: [list(LISTS[0]), list(LISTS[1])], changed: false });
    }
    // The layer's own name first: a new line goes there.
    out.sort_by_key(|s| s.section != name);
    if out.is_empty() {
        out.push(Lists { section: name.to_owned(), lists: [Vec::new(), Vec::new()], changed: false });
    }
    Ok(out)
}

/// The keys of an entry, as a binding reads them on this machine; `None`
/// when they do not read.
fn keys_of(t: &toml::Table) -> Option<Vec<Key>> {
    let on = match (t.get("on_mac"), cfg!(target_os = "macos")) {
        (Some(mac), true) => mac,
        _ => t.get("on")?,
    };
    let tokens: Vec<String> = match on {
        toml::Value::String(s) => tokenize(s),
        toml::Value::Array(a) if a.len() == 1 => tokenize(a[0].as_str()?),
        toml::Value::Array(a) => a.iter().map(|v| v.as_str().map(str::to_owned)).collect::<Option<_>>()?,
        _ => return None,
    };
    let keys: Option<Vec<Key>> = tokens.iter().map(|t| Key::parse(t)).collect();
    keys.filter(|k| !k.is_empty())
}

/// The command an entry runs, as [`crate::Binding::raw`] has it.
fn run_of(t: &toml::Table) -> Option<String> {
    match t.get("run")? {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Array(a) => a.iter().map(|v| v.as_str()).collect::<Option<Vec<_>>>().map(|v| v.join("; ")),
        _ => None,
    }
}

fn runs(t: &toml::Table, run: &str) -> bool {
    run_of(t).is_some_and(|r| r.trim() == run.trim())
}

/// An entry this screen leaves as written: one for some machines only.
fn kept(t: &toml::Table) -> bool {
    t.contains_key("for")
}

/// The keys the built-in defaults give `run` in the layer.
fn default_keys(defaults: &toml::Table, name: &str, aliases: &[&str], run: &str) -> Vec<Vec<Key>> {
    let mut out = Vec::new();
    for (section, v) in defaults {
        if section != name && !aliases.contains(&section.as_str()) {
            continue;
        }
        for l in ["prepend_keymap", "keymap", "append_keymap"] {
            for e in v.get(l).and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_table) {
                if runs(e, run) && !kept(e) {
                    out.extend(keys_of(e));
                }
            }
        }
    }
    out
}

/// A new entry: `on` as one string for a single key, a list for keys
/// pressed in turn.
fn entry(on: &[Key], run: &str, desc: &str) -> toml::Table {
    let mut t = toml::Table::new();
    let on: toml::Value = match on {
        [one] => one.to_string().into(),
        many => toml::Value::Array(many.iter().map(|k| k.to_string().into()).collect()),
    };
    t.insert("on".into(), on);
    t.insert("run".into(), run.into());
    if !desc.is_empty() {
        t.insert("desc".into(), desc.into());
    }
    t
}

/// Whether one of `a` and `b` starts with the other (or they are the same).
fn overlap(a: &[Key], b: &[Key]) -> bool {
    let n = a.len().min(b.len());
    a[..n] == b[..n]
}

fn apply(secs: &mut [Lists], e: &Edit, defaults: &[Vec<Key>], noop: &str) {
    // Take out every entry `gone` says, marking the sections it changed.
    let retain = |secs: &mut [Lists], gone: &dyn Fn(&toml::Table, &[Key]) -> bool| {
        for s in secs.iter_mut() {
            for list in &mut s.lists {
                let before = list.len();
                list.retain(|t| kept(t) || keys_of(t).is_none_or(|k| !gone(t, &k)));
                s.changed |= list.len() != before;
            }
        }
    };
    match e {
        Edit::Add { on, run, desc, .. } => {
            // What else the file put on these keys (or on keys one of them
            // starts with) loses them; so do lines of this run on them, put
            // back in front below when they are still needed.
            retain(secs, &|t, k| overlap(k, on) && (!runs(t, run) || k == on.as_slice()));
            let default = defaults.iter().any(|d| d == on);
            if !default {
                secs[0].lists[0].insert(0, entry(on, run, desc));
                secs[0].changed = true;
            }
        }
        Edit::Remove { on, run, .. } => {
            retain(secs, &|t, k| k == on.as_slice() && runs(t, run));
            let default = defaults.iter().any(|d| d == on);
            // A default key goes behind a `noop`, unless something in front
            // already has it.
            let taken = secs.iter().any(|s| s.lists[0].iter().any(|t| keys_of(t).is_some_and(|k| k == *on)));
            if default && !taken {
                secs[0].lists[0].insert(0, entry(on, noop, ""));
                secs[0].changed = true;
            }
        }
        Edit::Reset { run, .. } => {
            retain(secs, &|t, k| runs(t, run) || (runs(t, noop) && defaults.iter().any(|d| d == k)));
        }
    }
}
