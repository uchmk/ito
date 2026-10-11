//! A tickable line per key binding of a yazi-style `keymap.toml`.
//!
//! An app's default keymap holds a couple of hundred bindings across a dozen
//! layers, which is more than anyone can hold in their head while working
//! through them one at a time -- the checklist exists because losing your
//! place is the normal outcome, not a lapse.
//!
//! Generated rather than written because a hand-kept list drifts from the
//! keymap within a week, and a checklist that is quietly wrong is worse than
//! none: it certifies keys nobody tried.
//!
//! **Ticks survive regeneration.** The old file is read first and every `[x]`
//! is carried over by layer and key, so adding a binding does not cost the
//! afternoon already spent. Keys that have gone from the keymap are listed at
//! the end rather than dropped silently, since a key that vanished is worth
//! noticing.
//!
//! `--check` writes nothing and instead reports whether the file still matches
//! the keymap, exiting 1 if it does not: what CI runs. **No counts in the
//! file**: every pull request that ticked a key rewrote the total and its
//! layer's heading, so two open at once always conflicted on lines no tick was
//! on. `--stats` prints them instead.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::Report;

/// What an app's key checklist reads and writes. Paths are relative to the
/// working directory, the repository's root when run through `cargo run`.
pub struct Config<'a> {
    /// The default `keymap.toml`. Bindings are `[[layer.keymap]]` blocks with
    /// `on`, `run` and `desc`; a `# --- Name ---` comment starts a section.
    pub keymap: &'a str,
    /// The checklist it writes.
    pub out: &'a str,
    /// How the app runs this, without arguments, for the message that says to
    /// regenerate (`cargo run --example make-keycheck`).
    pub command: &'a str,
    /// The text before the first layer, given the number of keys. Only
    /// numbers a tick cannot move go in it.
    pub preamble: fn(usize) -> String,
    /// A line under each layer's heading saying what the layer is, since "why
    /// is `j` listed nine times" is the first question the file raises.
    pub layer_notes: &'a [(&'a str, &'a str)],
    /// The marks carried over besides `x`, which alone counts as checked. A
    /// mark not listed is dropped on the next run.
    pub marks: &'a [char],
}

struct Binding {
    layer: String,
    section: String,
    on: String,
    run: String,
    desc: String,
}

/// Run with the process's arguments, print, and exit.
pub fn run(cfg: &Config) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    run_with(cfg, Path::new("."), &args).exit();
}

/// [`run`] with the arguments and the folder the paths are relative to.
pub fn run_with(cfg: &Config, root: &Path, args: &[String]) -> Report {
    let mut r = Report::default();
    let check = args.iter().any(|a| a == "--check");
    let stats = args.iter().any(|a| a == "--stats");
    let (keymap, out_path) = (cfg.keymap, cfg.out);
    let text = std::fs::read_to_string(root.join(keymap)).unwrap_or_else(|e| panic!("read {keymap}: {e}"));
    let bindings = parse(&text);
    let old = std::fs::read_to_string(root.join(out_path)).ok();
    let done = previous_marks(old.as_deref().unwrap_or_default(), cfg.marks);
    let ticked_in = |b: &Binding| done.get(&key_of(b)) == Some(&'x');

    let mut out = String::new();
    let ticked = bindings.iter().filter(|b| ticked_in(b)).count();
    out.push_str(&(cfg.preamble)(bindings.len()));

    let mut layer = String::new();
    let mut section = String::new();
    let mut progress = vec![format!("{ticked} / {} checked", bindings.len())];
    for m in cfg.marks {
        let n = bindings.iter().filter(|b| done.get(&key_of(b)) == Some(m)).count();
        if n > 0 {
            progress[0].push_str(&format!(", {n} [{m}]"));
        }
    }
    for b in &bindings {
        if b.layer != layer {
            layer = b.layer.clone();
            section.clear();
            let n = bindings.iter().filter(|x| x.layer == layer).count();
            let d = bindings.iter().filter(|x| x.layer == layer && ticked_in(x)).count();
            progress.push(format!("[{layer}] {d} / {n}"));
            writeln!(out, "\n## `[{layer}]`\n").unwrap();
            let note = cfg.layer_notes.iter().find(|(l, _)| *l == layer).map_or("", |(_, n)| n);
            writeln!(out, "{note}\n").unwrap();
        }
        if b.section != section {
            section = b.section.clone();
            if !section.is_empty() {
                writeln!(out, "\n### {section}\n").unwrap();
            }
        }
        let mark = done.get(&key_of(b)).copied().unwrap_or(' ');
        writeln!(out, "- [{mark}] `{}` — {} · `{}`", b.on, b.desc, b.run).unwrap();
    }

    // A tick for a key that is no longer bound is worth seeing rather than
    // discarding: either the key was removed on purpose, or the keymap lost
    // something it should not have.
    let live: HashSet<String> = bindings.iter().map(key_of).collect();
    let orphans: Vec<&String> = done.keys().filter(|k| !live.contains(*k)).collect();
    if !orphans.is_empty() {
        writeln!(out, "\n## Checked, but no longer in the keymap\n").unwrap();
        writeln!(out, "Removed on purpose, or removed by accident. Worth a look either way.\n").unwrap();
        for o in orphans {
            writeln!(out, "- `{o}`").unwrap();
        }
    }

    // Written the way a Markdown formatter leaves it, so the two do not undo
    // each other: the ticks are edited by hand in an editor that reformats on
    // save, and a generator whose output differs cosmetically turns every
    // regeneration into a diff nobody can read.
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    // A row with no key on it is the parser having lost one, not the keymap
    // having a nameless binding -- and it reads as an ordinary checklist line,
    // so nothing about the file says anything went wrong. Refuse to write it.
    for b in &bindings {
        assert!(!b.on.trim().is_empty(), "`{}` in layer `{}` came out with no key: the `on = ` parse dropped it", b.run, b.layer);
    }
    if stats {
        writeln!(r.stdout, "{out_path}:").unwrap();
        for line in &progress {
            writeln!(r.stdout, "  {line}").unwrap();
        }
        return r;
    }
    if check {
        // A byte comparison is the whole test, and it is tick-insensitive for
        // free: `out` carried the existing ticks forward, so a tick can never
        // be what differs. Anything that does differ came from the keymap.
        let old = old.unwrap_or_default();
        if old == out {
            writeln!(r.stdout, "{out_path}: in sync with {keymap} ({ticked} / {} checked)", bindings.len()).unwrap();
            return r;
        }
        r.stdout = report_drift(&bindings, &old);
        r.stderr = format!("\n{out_path} is out of date. Regenerate it, read the diff, and commit it:\n\n    {}\n\n", cfg.command);
        r.code = 1;
        return r;
    }
    std::fs::write(root.join(out_path), out).unwrap_or_else(|e| panic!("write {out_path}: {e}"));
    writeln!(r.stdout, "{out_path}: {ticked} / {} checked", bindings.len()).unwrap();
    r
}

/// Name the keys behind an out-of-date checklist: added, gone, or re-described.
///
/// The exit code alone would say "regenerate", which anyone can guess. What is
/// worth printing is *which* bindings moved, because that is the review: a key
/// that vanished may have been removed on purpose or lost by accident, and the
/// two look identical from here.
fn report_drift(bindings: &[Binding], old: &str) -> String {
    let mut out = String::new();
    let now: BTreeMap<String, (String, String)> = bindings.iter().map(|b| (key_of(b), (b.desc.clone(), b.run.clone()))).collect();
    let was = rows_of_file(old);

    let mut quiet = true;
    for (key, (desc, run)) in &now {
        match was.get(key) {
            None => {
                writeln!(out, "+ {key} — {desc} · {run}").unwrap();
                quiet = false;
            }
            Some((d, r)) if (d, r) != (desc, run) => {
                writeln!(out, "~ {key}\n    was: {d} · {r}\n    now: {desc} · {run}").unwrap();
                quiet = false;
            }
            Some(_) => {}
        }
    }
    for key in was.keys().filter(|k| !now.contains_key(*k)) {
        writeln!(out, "- {key} (gone from the keymap)").unwrap();
        quiet = false;
    }
    if quiet {
        // Every binding matches, so what differs is the prose -- a wording
        // change in the file, not a keymap change.
        writeln!(out, "The bindings all match; the difference is in the surrounding text.").unwrap();
    }
    out
}

/// The rows read back out of the file, keyed the way [`key_of`] keys them.
fn rows_of_file(text: &str) -> BTreeMap<String, (String, String)> {
    let mut out = BTreeMap::new();
    let mut layer = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## `[") {
            if let Some(end) = rest.find("]`") {
                layer = rest[..end].to_owned();
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("- [").and_then(|r| r.get(1..)).and_then(|r| r.strip_prefix("] `")) else {
            continue; // prose, a heading, or the orphan list
        };
        let Some((on, rest)) = rest.split_once("` \u{2014} ") else { continue };
        // The command is the last backquoted word, so a description holding a
        // ` \u{b7} ` of its own does not steal the split.
        let Some((desc, run)) = rest.rsplit_once(" \u{b7} `") else { continue };
        out.insert(format!("{layer}/{on}"), (desc.to_owned(), run.trim_end_matches('`').to_owned()));
    }
    out
}

fn key_of(b: &Binding) -> String {
    format!("{}/{}", b.layer, b.on)
}

/// The marks in the file as it stands, by layer and key: `x`, and those in
/// `kept`. The orphan list's keys come back as `x`, so that they are not lost
/// on the next run either.
fn previous_marks(text: &str, kept: &[char]) -> BTreeMap<String, char> {
    let mut out = BTreeMap::new();
    let mut layer = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## `[") {
            if let Some(end) = rest.find("]`") {
                layer = rest[..end].to_owned();
            }
        } else if let Some(rest) = line.strip_prefix("- [") {
            let mut chars = rest.chars();
            let (Some(mark), Some(rest)) = (chars.next(), chars.as_str().strip_prefix("] `")) else { continue };
            if mark != 'x' && !kept.contains(&mark) {
                continue;
            }
            if let Some(end) = rest.find('`') {
                out.insert(format!("{layer}/{}", &rest[..end]), mark);
            }
        } else if let Some(rest) = line.strip_prefix("- `") {
            if let Some(end) = rest.find('`') {
                out.insert(rest[..end].to_owned(), 'x');
            }
        }
    }
    out
}

/// A line parser rather than a TOML one, because the `# --- Section ---`
/// comments are the grouping and a TOML parse throws comments away.
fn parse(text: &str) -> Vec<Binding> {
    let mut out: Vec<Binding> = Vec::new();
    let mut section = String::new();
    let mut cur: Option<Binding> = None;

    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# --- ") {
            section = rest.trim_end_matches(['-', ' ']).trim().to_owned();
        } else if let Some(rest) = t.strip_prefix("[[") {
            if let Some(end) = rest.find(".keymap]]") {
                if let Some(b) = cur.take() {
                    out.push(b);
                }
                cur = Some(Binding {
                    layer: rest[..end].to_owned(),
                    // Sections belong to the layer they were written in.
                    section: section.clone(),
                    on: String::new(),
                    run: String::new(),
                    desc: String::new(),
                });
            }
        } else if let Some(b) = cur.as_mut() {
            if let Some(v) = t.strip_prefix("on = ") {
                b.on = keys(v);
            } else if let Some(v) = t.strip_prefix("run = ") {
                b.run = unquote(v);
            } else if let Some(v) = t.strip_prefix("desc = ") {
                b.desc = unquote(v);
            }
        }
    }
    if let Some(b) = cur.take() {
        out.push(b);
    }
    out
}

/// `"k"` or `[ "g", "c" ]` -- a chord is written the way it is pressed.
///
/// The split has to honour the quotes. `[ ",", "b" ]` is a prefix and then
/// `b`, and cutting the array text at every comma slices that first element
/// in half: the two halves unquote to nothing, and the chord comes out as two
/// spaces with a key after them.
fn keys(v: &str) -> String {
    let v = v.trim();
    let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return unquote(v);
    };
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for c in inner.chars() {
        match c {
            _ if escaped => {
                cur.push(c);
                escaped = false;
            }
            '\\' if quoted => {
                cur.push(c);
                escaped = true;
            }
            '"' => {
                cur.push(c);
                quoted = !quoted;
            }
            ',' if !quoted => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    parts.push(cur);
    // A trailing comma leaves an empty tail; a key itself is never blank.
    parts.iter().filter(|p| !p.trim().is_empty()).map(|p| unquote(p)).collect::<Vec<_>>().join(" ")
}

/// Strip the quotes and undo TOML's escapes.
///
/// Trimming the quotes alone is not enough: a `desc` containing a Windows path
/// separator is written `\\` in the file and would reach the checklist with
/// both backslashes still on it.
fn unquote(v: &str) -> String {
    let inner = v.trim().trim_matches('"');
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            // `\\` and `\"` stand for themselves; anything else is left as it
            // was rather than guessed at.
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYMAP: &str = "[mgr]\n\n# --- Moving ---\n\n[[mgr.keymap]]\non = \"j\"\nrun = \"arrow 1\"\ndesc = \"Down\"\n\n\
        [[mgr.keymap]]\non = [ \",\", \"b\" ]\nrun = \"sort btime\"\ndesc = \"Sort by birth \\\\ time\"\n\n\
        [input]\n\n# --- Prompt ---\n\n[[input.keymap]]\non = \"<Esc>\"\nrun = \"close\"\ndesc = \"Close\"\n";

    fn preamble(n: usize) -> String {
        format!("# Keys\n\n{n} keys.\n")
    }

    fn config(marks: &'static [char]) -> Config<'static> {
        Config {
            keymap: "keymap.toml",
            out: "KEYS.md",
            command: "cargo run --example make-keycheck",
            preamble,
            layer_notes: &[("mgr", "The list.")],
            marks,
        }
    }

    fn dir(label: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ito-keycheck-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("keymap.toml"), KEYMAP).unwrap();
        d
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_quoted_comma_is_a_key() {
        assert_eq!(keys("[ \",\", \"b\" ]"), ", b");
        assert_eq!(keys("[ \"g\", \"c\", ]"), "g c");
        assert_eq!(keys("\"<C-a>\""), "<C-a>");
    }

    #[test]
    fn unquote_undoes_the_escapes() {
        assert_eq!(unquote("\"a\\\\b\""), "a\\b");
        assert_eq!(unquote("\"a\\nb\\tc\""), "a\nb\tc");
    }

    #[test]
    fn writes_and_keeps_ticks() {
        let d = dir("ticks");
        let cfg = config(&[]);
        let r = run_with(&cfg, &d, &[]);
        assert_eq!(r.stdout, "KEYS.md: 0 / 3 checked\n");
        let text = std::fs::read_to_string(d.join("KEYS.md")).unwrap();
        assert_eq!(
            text,
            "# Keys\n\n3 keys.\n\n## `[mgr]`\n\nThe list.\n\n### Moving\n\n- [ ] `j` — Down · `arrow 1`\n\
             - [ ] `, b` — Sort by birth \\ time · `sort btime`\n\n## `[input]`\n\n### Prompt\n\n- [ ] `<Esc>` — Close · `close`\n"
        );
        let ticked = text.replace("- [ ] `j`", "- [x] `j`").replace("- [ ] `<Esc>`", "- [~] `<Esc>`");
        std::fs::write(d.join("KEYS.md"), &ticked).unwrap();
        // `~` is not kept here, so the file is out of date: it will drop it.
        let r = run_with(&cfg, &d, &args(&["--check"]));
        assert_eq!(r.code, 1);
        assert_eq!(r.stdout, "The bindings all match; the difference is in the surrounding text.\n");
        run_with(&cfg, &d, &[]);
        let text = std::fs::read_to_string(d.join("KEYS.md")).unwrap();
        assert!(text.contains("- [x] `j`") && text.contains("- [ ] `<Esc>`"), "{text}");
        let r = run_with(&cfg, &d, &args(&["--stats"]));
        assert_eq!(r.stdout, "KEYS.md:\n  1 / 3 checked\n  [mgr] 1 / 2\n  [input] 0 / 1\n");
    }

    #[test]
    fn a_kept_mark_is_carried_and_not_counted() {
        let d = dir("kept");
        let cfg = config(&['~']);
        run_with(&cfg, &d, &[]);
        let text = std::fs::read_to_string(d.join("KEYS.md")).unwrap().replace("- [ ] `<Esc>`", "- [~] `<Esc>`");
        std::fs::write(d.join("KEYS.md"), &text).unwrap();
        let r = run_with(&cfg, &d, &args(&["--check"]));
        assert_eq!(r.stdout, "KEYS.md: in sync with keymap.toml (0 / 3 checked)\n");
        let r = run_with(&cfg, &d, &args(&["--stats"]));
        assert!(r.stdout.starts_with("KEYS.md:\n  0 / 3 checked, 1 [~]\n"), "{}", r.stdout);
    }

    #[test]
    fn drift_and_orphans() {
        let d = dir("drift");
        let cfg = config(&[]);
        run_with(&cfg, &d, &[]);
        let text = std::fs::read_to_string(d.join("KEYS.md")).unwrap().replace("- [ ] `<Esc>`", "- [x] `<Esc>`");
        std::fs::write(d.join("KEYS.md"), text).unwrap();
        std::fs::write(d.join("keymap.toml"), KEYMAP.replace("\"Down\"", "\"Down one\"").replace("<Esc>", "q")).unwrap();
        let r = run_with(&cfg, &d, &args(&["--check"]));
        assert_eq!(r.code, 1);
        assert_eq!(
            r.stdout,
            "+ input/q — Close · close\n~ mgr/j\n    was: Down · arrow 1\n    now: Down one · arrow 1\n- input/<Esc> (gone from the keymap)\n"
        );
        assert_eq!(
            r.stderr,
            "\nKEYS.md is out of date. Regenerate it, read the diff, and commit it:\n\n    cargo run --example make-keycheck\n\n"
        );
        run_with(&cfg, &d, &[]);
        let text = std::fs::read_to_string(d.join("KEYS.md")).unwrap();
        assert!(text.ends_with("## Checked, but no longer in the keymap\n\nRemoved on purpose, or removed by accident. Worth a look either way.\n\n- `input/<Esc>`\n"), "{text}");
        // And it stays on the next run.
        assert_eq!(run_with(&cfg, &d, &args(&["--check"])).code, 0);
    }
}
