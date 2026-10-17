//! A lane's checklist from `TESTING.md`: the manual half of it, one tickable
//! line per check, with a gloss per row and the commands each section needs.
//!
//! `TESTING.md` is the source of truth and stays so. It is written in
//! English, from the code, as tables of "Do / Expect" -- excellent for
//! auditing against the source, and awkward to *work through*: there is
//! nowhere to record that a row was done, no way to see how much is left, and
//! the setup a section needs is buried in its prose. The checklist is the
//! other shape of the same content: one tickable line per check, the gloss
//! first, and each section's preparation lifted out into a block to paste.
//!
//! **Generated, because a hand-kept second copy drifts** from the first
//! within a week, and the direction it drifts is the dangerous one -- rows
//! that quietly vanish look like rows that were done. So the ids, the section
//! list and the English all come from `TESTING.md` on every run; only the
//! gloss comes from the annotations file ([`Config::notes`]), and the English
//! is printed beside it so the two can be compared without leaving the line.
//!
//! **Rows `cargo test` already covers are not listed as work.** A test names
//! the check it stands in for in its doc comment (`TESTING.md 45.2`, or
//! `/// 13.4:` at the start of one), and this walks the sources for those. So
//! the count at the top is what is actually left for a person.
//!
//! **Ticks survive regeneration**, carried over by check id. Each lane has a
//! file of its own, since a tick says which machine it was seen on: `[x]`
//! always, and the extra marks the lane keeps ([`Lane::marks`]):
//!
//! - `[-]`: the row does not apply on this lane (a UNC share or ConPTY on
//!   Linux). Carried over like a tick and counted as done, apart from one.
//! - `[~]`: an appearance row an agent judged from a screenshot it took. Carried
//!   over like a tick and **not** done: the owner turns it into `[x]` after
//!   looking at the same picture.
//!
//! The command line is the app's example's: `--lane NAME` picks the lane (the
//! first one by default), `--check` writes nothing and exits 1 when the file
//! is out of date, `--stats` prints the progress per section. A run that
//! cannot find a crate it must scan exits 2; a malformed `TESTING.md` panics.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::Report;

/// What an app's checklist reads and writes. Paths are relative to the
/// working directory, the repository's root when run through `cargo run`.
pub struct Config<'a> {
    /// `TESTING.md`.
    pub source: &'a str,
    /// The annotations: the gloss per row, a title, setup and note per
    /// section, and the ids to keep on the list although a test names them
    /// ([`Notes`]). A missing file is not an error.
    pub notes: &'a str,
    /// The folders whose `.rs` files are searched for tests naming a check.
    pub src_dirs: &'a [&'a str],
    /// Dependencies whose tests name this app's checks too (`ito-pane` for
    /// kura), found through `cargo metadata`. Not finding one exits 2: its
    /// rows would quietly come back onto the human's list.
    pub dep_crates: &'a [&'a str],
    /// A hand-kept list of ids (`- **id** text` lines) that `--check` on a lane
    /// with [`Lane::check_release`] makes sure `TESTING.md` still has: the rows
    /// a release asks another machine to press.
    pub release_list: Option<&'a str>,
    /// How the app runs this, without arguments, for the message that says to
    /// regenerate (`cargo run --example make-testcheck`).
    pub command: &'a str,
    /// The lanes, the default first.
    pub lanes: &'a [Lane<'a>],
}

/// One machine's checklist.
pub struct Lane<'a> {
    /// What `--lane` takes.
    pub name: &'a str,
    /// The file it writes.
    pub out: &'a str,
    /// The marks it keeps besides `x`: `-` (not applicable, done), `~` (judged
    /// from a picture, not done). A mark not listed is dropped on the next
    /// run: a `[-]` on a lane where every row applies would only hide one.
    pub marks: &'a [char],
    /// Whether the sections' setup blocks are printed. They are PowerShell
    /// against the Windows fixtures, instructions another lane cannot follow.
    pub setup: bool,
    /// Whether `--check` also checks [`Config::release_list`].
    pub check_release: bool,
    /// The text before the first section. Only numbers a tick cannot move go
    /// in it, so that a pull request that only ticks never touches it.
    pub preamble: fn(&Counts) -> String,
}

/// The numbers a preamble may print.
pub struct Counts {
    /// Every check in `TESTING.md`.
    pub all: usize,
    /// The ones left for a person: `all` less those a test covers.
    pub manual: usize,
    /// Manual rows with no gloss.
    pub untranslated: usize,
}

/// One row of one of TESTING.md's tables.
#[derive(Debug, PartialEq)]
struct Check {
    section: u32,
    id: String,
    /// The cells after the id, in order. Two normally (`Do`, `Expect`); three
    /// where the table qualifies them first.
    cells: Vec<String>,
}

#[derive(Debug)]
struct Section {
    n: u32,
    /// The heading as TESTING.md writes it, version tag and all.
    title: String,
    checks: Vec<Check>,
}

/// What the annotations file adds, and the only hand-written input here.
/// A key in the wrong place is silent otherwise: TOML reads a bare key written
/// after the first `[table]` as belonging to that table, so `manual` placed at
/// the foot of kura's file became `section.46.manual` and did nothing at all.
#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Notes {
    /// The checklist has no second language: a row with no gloss is printed as
    /// `- [ ] **id** cells`, without the `〔未訳〕` marker, the untranslated
    /// count is 0, and the fixed lines are English. Written before the first
    /// `[table]`, like every bare key here.
    #[serde(default)]
    pub plain: bool,
    /// Report faults in `TESTING.md` (a duplicate id in a section, a control
    /// character, a row whose cell count differs from its table's header, a line
    /// that follows a table row without a blank line) as errors with
    /// `file:line`, exit 1, before anything is written.
    #[serde(default)]
    pub strict: bool,
    #[serde(default)]
    pub section: BTreeMap<String, SectionNote>,
    /// The gloss for one check, by id.
    #[serde(default)]
    pub row: BTreeMap<String, String>,
    /// Ids to keep on the human's list even though a test names them.
    ///
    /// For a test that covers the row on one platform only: kura's
    /// `spot_link_section` is `#[cfg(unix)]`, so on the machine the checklist
    /// is *for* its rows are not covered at all. Deriving coverage from the
    /// tests cannot see that, and the error is in the direction that matters
    /// -- a row taken off the list is a row nobody runs.
    #[serde(default)]
    pub manual: Vec<String>,
}

#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SectionNote {
    /// The heading in the gloss's language. Falls back to TESTING.md's.
    #[serde(default)]
    pub title: String,
    /// What to set up first, as a block to paste. Usually PowerShell.
    #[serde(default)]
    pub setup: String,
    /// A sentence of context, where the section needs one.
    #[serde(default)]
    pub note: String,
}

/// Run with the process's arguments, print, and exit.
pub fn run(cfg: &Config) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut automated = BTreeSet::new();
    let mut dirs: Vec<PathBuf> = cfg.src_dirs.iter().map(PathBuf::from).collect();
    for name in cfg.dep_crates {
        match crate_src(name) {
            Ok(dir) => dirs.push(dir),
            Err(report) => return report.exit(),
        }
    }
    for dir in dirs {
        let mut files = Vec::new();
        collect_rs(&dir, &mut files);
        for f in files {
            if let Ok(text) = std::fs::read_to_string(&f) {
                automated.append(&mut claimed_ids(&text));
            }
        }
    }
    run_with(cfg, Path::new("."), &args, automated).exit();
}

/// [`run`] with the arguments, the folder the paths are relative to, and the
/// ids the tests claim already gathered.
pub fn run_with(cfg: &Config, root: &Path, args: &[String], mut automated: BTreeSet<String>) -> Report {
    let mut r = Report::default();
    let check = args.iter().any(|a| a == "--check");
    let stats = args.iter().any(|a| a == "--stats");
    let first = &cfg.lanes[0];
    let lane = match args.iter().position(|a| a == "--lane").and_then(|i| args.get(i + 1)).map(String::as_str) {
        None => first,
        Some(name) => cfg.lanes.iter().find(|l| l.name == name).unwrap_or_else(|| {
            let names: Vec<String> = cfg.lanes.iter().map(|l| format!("`{}`", l.name)).collect();
            panic!("--lane takes {}, not `{name}`", names.join(" or "))
        }),
    };
    let is_default = std::ptr::eq(lane, first);
    let src = cfg.source;
    let out_path = lane.out;
    let text = std::fs::read_to_string(root.join(src)).unwrap_or_else(|e| panic!("read {src}: {e}"));
    let sections = parse(&text, src);
    let notes: Notes = match std::fs::read_to_string(root.join(cfg.notes)) {
        Ok(t) => toml::from_str(&t).unwrap_or_else(|e| panic!("parse {}: {e}", cfg.notes)),
        // Not an error: without the file this is TESTING.md rearranged, which is
        // still a working checklist. The count of untranslated rows says so.
        Err(_) => Notes::default(),
    };
    if notes.strict {
        let faults = lint(&text, src);
        if !faults.is_empty() {
            writeln!(r.stderr, "{}", faults.join("\n")).unwrap();
            r.code = 1;
            return r;
        }
    }
    let plain = notes.plain;
    for id in &notes.manual {
        automated.remove(id);
    }
    let marks = previous_marks(&std::fs::read_to_string(root.join(out_path)).unwrap_or_default(), lane.marks);
    // `done` is what counts as finished: ticked, or not applicable. A `[~]` is
    // a judgement from a picture, waiting on the owner, so it is counted on
    // its own and not as done.
    let done: BTreeSet<String> = marks.iter().filter(|(_, m)| **m != '~').map(|(id, _)| id.clone()).collect();

    let all: Vec<&Check> = sections.iter().flat_map(|s| s.checks.iter()).collect();
    let manual: Vec<&&Check> = all.iter().filter(|c| !automated.contains(&c.id)).collect();
    let ticked = manual.iter().filter(|c| marks.get(&c.id) == Some(&'x')).count();
    let skipped = manual.iter().filter(|c| marks.get(&c.id) == Some(&'-')).count();
    let looked = manual.iter().filter(|c| marks.get(&c.id) == Some(&'~')).count();
    let untranslated = if plain { 0 } else { manual.iter().filter(|c| !notes.row.contains_key(&c.id)).count() };
    // The fixed lines, in the gloss's language or in English.
    let t = |ja: String, en: String| if plain { en } else { ja };

    let mut out = (lane.preamble)(&Counts { all: all.len(), manual: manual.len(), untranslated });
    // The progress, for `--stats`. Not written into the file: every pull
    // request that ticked a box also rewrote the total and its section's
    // heading, so any two open at once conflicted on lines no tick was on,
    // and each conflict held a lane up for an hour.
    let mut progress = vec![format!(
        "{ticked} / {} checked, {skipped} not applicable, {looked} looked at ([~]), {untranslated} untranslated",
        manual.len()
    )];

    for s in &sections {
        let mine: Vec<&Check> = s.checks.iter().filter(|c| !automated.contains(&c.id)).collect();
        let auto: Vec<&str> = s.checks.iter().filter(|c| automated.contains(&c.id)).map(|c| c.id.as_str()).collect();
        let n = notes.section.get(&s.n.to_string());
        let title = match n.map(|n| n.title.as_str()).unwrap_or_default() {
            "" => s.title.clone(),
            ja => ja.to_owned(),
        };
        let d = mine.iter().filter(|c| done.contains(&c.id)).count();
        let seen = mine.iter().filter(|c| marks.get(&c.id) == Some(&'~')).count();
        let seen = if seen > 0 { format!(", {seen} looked at") } else { String::new() };
        if !mine.is_empty() {
            progress.push(format!("{:>3}. {d} / {}{seen}  {}", s.n, mine.len(), s.title));
        }

        if mine.is_empty() {
            // Every row automated. Say so and move on: a heading with nothing
            // under it reads like a section someone forgot to fill in.
            let n = s.checks.len();
            writeln!(out, "\n## {}. {title} — {}\n", s.n, t(format!("全 {n} 件が自動"), format!("all {n} automated"))).unwrap();
            writeln!(
                out,
                "{}",
                t(
                    "`cargo test` が全部見ているので、押すものはありません。".into(),
                    "`cargo test` covers them all; nothing to press.".into()
                )
            )
            .unwrap();
            continue;
        }
        writeln!(out, "\n## {}. {title}\n", s.n).unwrap();
        if let Some(note) = n.map(|n| n.note.trim()).filter(|s| !s.is_empty()) {
            writeln!(out, "{note}\n").unwrap();
        }
        if !auto.is_empty() {
            let ids = auto.join(", ");
            writeln!(
                out,
                "{}\n",
                t(format!("自動テスト済みなので下には出していない: {ids}"), format!("Covered by tests, not listed below: {ids}"))
            )
            .unwrap();
        }
        if let Some(setup) = n.map(|n| n.setup.trim()).filter(|s| !s.is_empty() && lane.setup) {
            writeln!(out, "{}\n\n{setup}\n", t("準備:".into(), "Setup:".into())).unwrap();
        }
        for c in mine {
            let mark = marks.get(&c.id).copied().unwrap_or(' ');
            let en = c.cells.join(" → ");
            match notes.row.get(&c.id) {
                Some(ja) => writeln!(out, "- [{mark}] **{}** {ja} — *{en}*", c.id).unwrap(),
                None if plain => writeln!(out, "- [{mark}] **{}** {en}", c.id).unwrap(),
                None => writeln!(out, "- [{mark}] **{}** {en} 〔未訳〕", c.id).unwrap(),
            }
        }
    }

    // A tick against an id TESTING.md no longer has is worth seeing: the row may
    // have been removed on purpose, or renumbered, and renumbering silently
    // moves a tick onto a different check -- which is the one failure this file
    // must not have.
    let live: BTreeSet<&str> = all.iter().map(|c| c.id.as_str()).collect();
    let orphans: Vec<&String> = marks.keys().filter(|id| !live.contains(id.as_str())).collect();
    if !orphans.is_empty() {
        writeln!(out, "\n## {}\n", t(format!("済みだが {src} に無い項目"), format!("Checked, but not in {src}"))).unwrap();
        let why = t(
            "消えたか、番号が振り直されたか。**番号が動いた場合、上のチェックは別の項目に\n付いている**ので、その節はもう一度通すこと。"
                .into(),
            "Removed, or renumbered. **If the numbers moved, the ticks above sit on different\nchecks**, so go through that section again."
                .into(),
        );
        writeln!(out, "{why}\n").unwrap();
        for o in orphans {
            writeln!(out, "- `{o}`").unwrap();
        }
    }

    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    // The separator between the gloss and the English is how a later
    // `--check` tells them apart, so a gloss containing it would make that row
    // read as drifted forever.
    for (id, ja) in &notes.row {
        assert!(!ja.contains(" — *"), "the gloss for {id} contains ` — *`, which separates it from the English");
    }
    // The newest version a row names, in its English and in its gloss (a
    // gloss that names none is skipped). A fix edits one and forgets the
    // other, and `--check` compared neither.
    let mut skewed = Vec::new();
    for c in &all {
        let Some(ja) = notes.row.get(&c.id) else { continue };
        let (en, ja_v) = (last_version(&c.cells.join(" ")), last_version(ja));
        if ja_v.is_some() && en != ja_v {
            skewed.push(format!("{}: {src} ends at {en:?}, the Japanese at {ja_v:?}", c.id));
        }
    }
    if !skewed.is_empty() {
        writeln!(r.stderr, "{}: the version a row ends at differs from {src}:\n  {}", cfg.notes, skewed.join("\n  ")).unwrap();
        r.code = 1;
        return r;
    }
    // An empty cell reads as an ordinary row with a short description, so
    // nothing about the file would say the parse lost one. Refuse to write it.
    for c in &all {
        assert!(
            c.cells.iter().all(|cell| !cell.trim().is_empty()),
            "check {} came out with an empty cell: the table parse dropped one",
            c.id,
        );
    }

    if stats {
        writeln!(r.stdout, "{out_path}:").unwrap();
        for line in &progress {
            writeln!(r.stdout, "  {line}").unwrap();
        }
        return r;
    }
    if check {
        // A renumbered or removed row would otherwise leave the release issue
        // asking for a check that no longer exists, or a different one.
        if let (Some(list), true) = (cfg.release_list, lane.check_release) {
            let text = std::fs::read_to_string(root.join(list)).unwrap_or_default();
            let gone: Vec<&str> = release_ids(&text).into_iter().filter(|id| !live.contains(id)).collect();
            if !gone.is_empty() {
                writeln!(r.stderr, "{list} names ids {src} does not have: {}", gone.join(", ")).unwrap();
                r.code = 1;
                return r;
            }
        }
        // Byte comparison, and tick-insensitive for free: `out` carried the
        // existing ticks forward, so a tick can never be what differs -- and
        // with no counts in the file, neither can a number a tick moved.
        let old = std::fs::read_to_string(root.join(out_path)).unwrap_or_default();
        if old == out {
            writeln!(
                r.stdout,
                "{out_path}: in sync with {src} ({ticked} / {} checked, {looked} looked at, {untranslated} untranslated)",
                manual.len(),
            )
            .unwrap();
            return r;
        }
        r.stdout = report_drift(&all, &automated, &old, src, plain);
        let flag = if is_default { String::new() } else { format!(" -- --lane {}", lane.name) };
        writeln!(r.stderr, "\n{out_path} is out of date. Regenerate it, read the diff, and commit it:\n\n    {}{flag}\n", cfg.command)
            .unwrap();
        r.code = 1;
        return r;
    }
    std::fs::write(root.join(out_path), out).unwrap_or_else(|e| panic!("write {out_path}: {e}"));
    writeln!(
        r.stdout,
        "{out_path}: {ticked} / {} checked, {looked} looked at ({} automated, {untranslated} untranslated)",
        manual.len(),
        all.len() - manual.len(),
    )
    .unwrap();
    r
}

/// TESTING.md's sections and the rows of their tables.
///
/// Only two shapes matter: `## N. Title` starts a section, and a line whose
/// first cell is `N.M` is a check. Everything else is prose, which this steps
/// over -- the prose is where the section's context lives, and it is carried
/// into the output by hand, through the annotations, because it is the part
/// that needs judgement rather than transcription.
fn parse(text: &str, src: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    // Under a `##` heading that is not `## N. Title` (a "Covered by tests"
    // table, say) the rows are not checks: they are for the reader.
    let mut skip = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            skip = true;
            if let Some((num, title)) = rest.split_once(". ") {
                if let Ok(n) = num.parse::<u32>() {
                    out.push(Section { n, title: title.trim().to_owned(), checks: Vec::new() });
                    skip = false;
                }
            }
            continue;
        }
        if skip {
            continue;
        }
        let Some(row) = line.strip_prefix("| ") else { continue };
        let mut cells = row.trim_end().trim_end_matches('|').split(" | ").map(str::trim);
        let Some(id) = cells.next() else { continue };
        if !is_check_id(id) {
            continue; // the `| # | Do | Expect |` header, or the `| --- |` rule
        }
        let mut cells: Vec<String> = cells.map(str::to_owned).collect();
        // A row's own tick (`[ ]`, `[x]`, `[~]`, `[-]`) is not a cell of the check.
        if cells.last().is_some_and(|c| matches!(c.as_str(), "[ ]" | "[x]" | "[~]" | "[-]")) {
            cells.pop();
        }
        // A section heading always precedes a table; a row without one would
        // mean the heading did not parse, and silently dropping it is how a
        // whole section goes missing.
        let s = out.last_mut().unwrap_or_else(|| panic!("check {id} sits before any `## N.` heading"));
        s.checks.push(Check { section: s.n, id: id.to_owned(), cells });
    }
    // The id must name its own section: a row numbered `13.2` inside section
    // 12 would land here as a tick on the wrong line.
    for s in &out {
        for c in &s.checks {
            let (sec, _) = c.id.split_once('.').expect("an id has a dot");
            assert_eq!(
                sec.parse::<u32>().ok(),
                Some(c.section),
                "check {} is inside section {} ({}): one of the two is wrong in {src}",
                c.id,
                s.n,
                s.title,
            );
        }
    }
    out
}

/// Faults in `TESTING.md` that the parse would read past: each as
/// `file:line: what`.
fn lint(text: &str, src: &str) -> Vec<String> {
    let mut faults = Vec::new();
    let mut seen = BTreeSet::new();
    let mut width = 0;
    let mut in_table = false;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if let Some(c) = line.chars().find(|c| c.is_control() && *c != '\t') {
            faults.push(format!("{src}:{n}: control character U+{:04X}", c as u32));
        }
        if line.starts_with("## ") {
            seen.clear();
        }
        if !line.starts_with('|') {
            if in_table && !line.trim().is_empty() {
                faults.push(format!("{src}:{n}: a line follows a table row without a blank line, which breaks the row"));
            }
            in_table = false;
            continue;
        }
        let cells = line.replace("\\|", "");
        let cells = cells.trim().trim_start_matches('|').trim_end_matches('|');
        let count = cells.split('|').count();
        if !in_table {
            width = count;
            in_table = true;
        } else if count != width {
            faults.push(format!("{src}:{n}: {count} cells in a table of {width}"));
        }
        let id = cells.split('|').next().unwrap_or_default().trim();
        if is_check_id(id) && !seen.insert(id.to_owned()) {
            faults.push(format!("{src}:{n}: check {id} appears twice in its section"));
        }
    }
    faults
}

/// `13.4`, or `11.9b` -- a section number, a dot, a count, and an optional
/// letter for a row inserted after the fact.
fn is_check_id(s: &str) -> bool {
    let Some((sec, rest)) = s.split_once('.') else { return false };
    if sec.is_empty() || !sec.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let digits = rest.trim_end_matches(|c: char| c.is_ascii_lowercase());
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// The check ids the tests in one source file claim to stand in for.
///
/// Read from the tests rather than from TESTING.md's own prose, which says
/// things like "all eight are automated" -- true, and not something to parse.
/// A test naming its id is a fact about the code, so this can only be wrong in
/// the safe direction: a test that forgot to say leaves its row on the human's
/// list, which costs one redundant check. The reverse -- a row marked done
/// because prose said so -- is the one that certifies something nobody ran.
///
/// Two spellings are accepted: `TESTING.md 45.2` inside a sentence, and
/// `/// 13.4:` opening the comment.
///
/// **Only a doc comment that actually precedes a `#[test]` counts.** Production
/// code cites TESTING.md too -- kura's `src/spot.rs` explains a `read_link`
/// branch by naming a row that is emphatically *not* automated -- and counting
/// that as coverage takes a row off the human's list on the strength of a
/// comment about why the code is shaped the way it is.
fn claimed_ids(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut pending = BTreeSet::new();
    let mut is_test = false;
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(comment) = t.strip_prefix("///") {
            if let Some(at) = comment.find("TESTING.md ") {
                ids_in(&comment[at + "TESTING.md ".len()..], &mut pending);
            }
            // `/// 13.4: …` and `/// 6.1, 6.2, …`: ids up to the first colon
            // or dash, which also has to be *present*. Without that a doc
            // comment that merely wraps onto an id reads as a claim about it --
            // and the sentence it wrapped from said "12.9 to 12.12 stay with
            // the machine", so the rows were taken off the human's list by the
            // words putting them on it.
            let head = comment.trim_start();
            if head.starts_with(|c: char| c.is_ascii_digit()) {
                if let Some(end) = head.find([':', '-', '—']) {
                    ids_in(&head[..end], &mut pending);
                }
            }
            continue;
        }
        if t.starts_with("#[") {
            // `#[cfg(test)]` counts as well as `#[test]`, on purpose: a
            // module's own doc comment is where a claim over several rows
            // belongs ("TESTING.md 45.1, 45.5 and 45.7"), which no single
            // test inside it could make.
            is_test |= t.contains("test]") || t.contains("test)");
            continue;
        }
        if t.is_empty() || t.starts_with("//") {
            continue; // a blank line or a `//` aside inside the block
        }
        // Anything else ends the run of comments and attributes. What they
        // were attached to is now known.
        if is_test {
            out.append(&mut pending);
        }
        pending.clear();
        is_test = false;
    }
    out
}

/// Every check id in `s`, stopping at the first word that is not one, a
/// separator, or a word joining two ("and", "to", "through").
fn ids_in(s: &str, out: &mut BTreeSet<String>) {
    for word in s.split([' ', ',', '/', '(', ')', '–', '—']).map(|w| w.trim_end_matches(['.', ':'])) {
        if word.is_empty() || matches!(word, "and" | "to" | "through" | "&") {
            continue;
        }
        if is_check_id(word) {
            out.insert(word.to_owned());
        } else {
            return; // the prose has started
        }
    }
}

/// The source folder of a dependency, found through `cargo metadata`, since a
/// git dependency lives in Cargo's own checkout folder.
fn crate_src(name: &str) -> Result<PathBuf, Report> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo).args(["metadata", "--format-version", "1"]).output().expect("cargo metadata runs");
    let json = String::from_utf8_lossy(&out.stdout);
    // `"manifest_path":"…/crates/ito-pane/Cargo.toml"`, read without a JSON
    // parser: the path is the one string that ends that way.
    let end = json.find(&format!("{name}/Cargo.toml\"")).or_else(|| json.find(&format!("{name}\\\\Cargo.toml\"")));
    let Some(end) = end else {
        return Err(Report {
            stderr: format!("{name} is not in `cargo metadata`; run `cargo build` first\n"),
            code: 2,
            ..Report::default()
        });
    };
    let start = json[..end].rfind('"').map_or(0, |i| i + 1);
    let manifest = json[start..end].replace("\\\\", "\\");
    Ok(Path::new(&manifest).join(name).join("src"))
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The marks already in the file, by id: `x`, and those of `kept`.
fn previous_marks(text: &str, kept: &[char]) -> BTreeMap<String, char> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("- [") else { continue };
        let mut chars = rest.chars();
        let Some(mark) = chars.next() else { continue };
        if mark != 'x' && !kept.contains(&mark) {
            continue;
        }
        let Some(rest) = chars.as_str().strip_prefix("] **") else { continue };
        if let Some(end) = rest.find("**") {
            out.insert(rest[..end].to_owned(), mark);
        }
    }
    out
}

/// The ids a release list names, one per `- **id** text` line.
fn release_ids(text: &str) -> Vec<&str> {
    text.lines().filter_map(|l| l.strip_prefix("- **")).filter_map(|rest| rest.find("**").map(|end| &rest[..end])).collect()
}

/// Name what makes the file stale: which checks arrived, left, changed wording,
/// or became automated. The exit code alone says "regenerate", which anyone can
/// guess; what is worth printing is which rows moved, because a row that
/// vanished may have been removed on purpose or lost in an edit, and from here
/// the two look identical.
fn report_drift(all: &[&Check], automated: &BTreeSet<String>, old: &str, src: &str, plain: bool) -> String {
    let now: BTreeMap<&str, String> = all.iter().map(|c| (c.id.as_str(), c.cells.join(" → "))).collect();
    let was = rows_of_file(old, plain);
    let mut out = String::new();

    for (id, en) in &now {
        let auto = automated.contains(*id);
        match was.get(*id) {
            None if auto => {} // automated rows are not listed, so absence is right
            None => writeln!(out, "+ {id} — {en}").unwrap(),
            Some(_) if auto => writeln!(out, "* {id} — 自動テストが覆ったので一覧から外れる").unwrap(),
            Some(before) if before != en => writeln!(out, "~ {id}\n    was: {before}\n    now: {en}").unwrap(),
            Some(_) => {}
        }
    }
    for id in was.keys().filter(|id| !now.contains_key(**id)) {
        writeln!(out, "- {id} (gone from {src})").unwrap();
    }
    if out.is_empty() {
        // Every row matches, so what differs is the prose, a count in a
        // heading, or a gloss -- the generated file or the annotations, not
        // TESTING.md.
        out.push_str("The checks all match; the difference is in the surrounding text or the Japanese.\n");
    }
    out
}

/// The rows read back out of the generated file: id to the English that follows
/// it, which is the part [`report_drift`] can compare against TESTING.md.
fn rows_of_file(text: &str, plain: bool) -> BTreeMap<&str, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some(rest) = ["x", " ", "-", "~"].iter().find_map(|m| line.strip_prefix(&format!("- [{m}] **"))) else {
            continue;
        };
        let Some((id, body)) = rest.split_once("** ") else { continue };
        // The marker is checked first, and the split is from the *front*.
        // Splitting from the back found ` — *` inside an untranslated row's own
        // English -- kura's 1.5 reads "list — **and the shell is still
        // there**" -- and reported a drift on a row nobody had touched. `run`
        // refuses to write a gloss containing the separator, which is what
        // makes the front split safe.
        if plain {
            // No marker, or a gloss: the body is the English.
            let en = match body.strip_suffix(" 〔未訳〕") {
                Some(en) => en,
                None => body.split_once(" — *").map_or(body, |(_, en)| en.strip_suffix('*').unwrap_or(en)),
            };
            out.insert(id, en.to_owned());
            continue;
        }
        let en = match body.strip_suffix(" 〔未訳〕") {
            Some(en) => en.to_owned(),
            None => match body.split_once(" — *") {
                // Exactly one `*`: the italic's closer. An English that itself
                // ends in bold ends the line in `***`, and trimming them all
                // took the bold's closer too -- every such row then read as
                // drifted in any failing `--check`, burying the real change.
                Some((_, en)) => en.strip_suffix('*').unwrap_or(en).to_owned(),
                None => continue, // not a row this generator wrote
            },
        };
        out.insert(id, en);
    }
    out
}

/// The newest `vX.Y.Z` in `s`, in either `(v1.2.3)` or `（v1.2.3）` brackets.
fn last_version(s: &str) -> Option<String> {
    let mut last: Option<(Vec<u32>, String)> = None;
    for (i, _) in s.match_indices('v') {
        let rest = &s[i + 1..];
        let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
        let v = rest[..end].trim_end_matches('.');
        let before = s[..i].chars().next_back();
        if v.split('.').count() == 3 && v.split('.').all(|p| !p.is_empty()) && !before.is_some_and(|c| c.is_alphanumeric()) {
            let key: Vec<u32> = v.split('.').map(|p| p.parse().unwrap_or(0)).collect();
            if last.as_ref().is_none_or(|(k, _)| key > *k) {
                last = Some((key, v.to_owned()));
            }
        }
    }
    last.map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TESTING: &str = "# Testing\n\nProse.\n\n## 1. First (v0.1.0)\n\n| # | Do | Expect |\n| --- | --- | --- |\n\
        | 1.1 | Press `a` | It opens |\n| 1.2 | Press `b` | It closes (v0.2.0) |\n| 1.3 | Covered | By a test |\n\n\
        ## 2. Second\n\n| # | Where | Do | Expect |\n| --- | --- | --- | --- |\n| 2.1 | Linux | Run | It runs — **and stays** |\n";

    fn preamble(c: &Counts) -> String {
        format!("# List\n\n{} of {}, {} untranslated.\n", c.manual, c.all, c.untranslated)
    }

    const LANES: [Lane; 2] = [
        Lane { name: "windows", out: "CHECKS.md", marks: &['~'], setup: true, check_release: true, preamble },
        Lane { name: "linux", out: "LINUX.md", marks: &['-'], setup: false, check_release: false, preamble },
    ];

    fn config() -> Config<'static> {
        Config {
            source: "TESTING.md",
            notes: "ja.toml",
            src_dirs: &[],
            dep_crates: &[],
            release_list: Some("ARM.md"),
            command: "cargo run --example make-testcheck",
            lanes: &LANES,
        }
    }

    fn dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ito-testcheck-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("TESTING.md"), TESTING).unwrap();
        std::fs::write(d.join("ja.toml"), "[section.1]\ntitle = \"一つめ\"\nsetup = \"mkdir x\"\n\n[row]\n\"1.1\" = \"`a` を押す\"\n")
            .unwrap();
        d
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn auto() -> BTreeSet<String> {
        BTreeSet::from(["1.3".to_owned()])
    }

    #[test]
    fn parses_sections_and_rows() {
        let s = parse(TESTING, "TESTING.md");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].title, "First (v0.1.0)");
        assert_eq!(s[0].checks.len(), 3);
        assert_eq!(s[1].checks[0].cells, ["Linux", "Run", "It runs — **and stays**"]);
    }

    #[test]
    fn rows_under_an_unnumbered_heading_are_not_checks() {
        let text =
            "## 1. One\n\n| 1.1 | Do | Expect |\n\n## Covered by tests\n\n| 1.2 | a | test |\n\n## 2. Two\n\n| 2.1 | Do | Expect |\n";
        let s = parse(text, "T.md");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].checks.len(), 1);
        assert_eq!(s[1].checks[0].id, "2.1");
    }

    #[test]
    fn plain_prints_a_row_without_a_gloss_as_it_is() {
        let d = dir("plain");
        std::fs::write(d.join("ja.toml"), "plain = true\n").unwrap();
        let cfg = config();
        let r = run_with(&cfg, &d, &args(&[]), auto());
        assert_eq!(r.code, 0, "{}", r.stderr);
        let text = std::fs::read_to_string(d.join("CHECKS.md")).unwrap();
        assert!(text.contains("- [ ] **1.2** Press `b` → It closes (v0.2.0)\n"), "{text}");
        assert!(!text.contains("〔未訳〕"));
        assert!(text.contains("Covered by tests, not listed below: 1.3"), "{text}");
        let again = run_with(&cfg, &d, &args(&["--check"]), auto());
        assert_eq!(again.code, 0, "{}{}", again.stdout, again.stderr);
    }

    #[test]
    fn strict_reports_faults_with_their_lines() {
        let d = dir("strict");
        std::fs::write(d.join("ja.toml"), "strict = true\n").unwrap();
        std::fs::write(
            d.join("TESTING.md"),
            "## 1. One\n\n| # | Do | Expect |\n| --- | --- | --- |\n| 1.1 | a | b |\n| 1.1 | c | d |\n| 1.2 | only |\n| 1.3 | bell \u{7} | x |\nprose\n",
        )
        .unwrap();
        let r = run_with(&config(), &d, &args(&[]), BTreeSet::new());
        assert_eq!(r.code, 1);
        for want in [
            "TESTING.md:6: check 1.1 appears twice",
            "TESTING.md:7: 2 cells in a table of 3",
            "TESTING.md:8: control character U+0007",
            "TESTING.md:9: a line follows",
        ] {
            assert!(r.stderr.contains(want), "{want} not in {}", r.stderr);
        }
    }

    #[test]
    fn a_trailing_tick_cell_is_not_a_cell_of_the_check() {
        let s = parse(
            "## 1. One

| 1.1 | Do | Expect | [ ] |
| 1.2 | Do | Expect | [~] |
| 1.3 | Do | Where | Expect |
",
            "TESTING.md",
        );
        assert_eq!(s[0].checks[0].cells, ["Do", "Expect"]);
        assert_eq!(s[0].checks[1].cells, ["Do", "Expect"]);
        assert_eq!(s[0].checks[2].cells, ["Do", "Where", "Expect"]);
    }

    #[test]
    #[should_panic(expected = "check 2.1 is inside section 1")]
    fn a_row_in_the_wrong_section_stops_the_run() {
        parse("## 1. One\n\n| 2.1 | a | b |\n", "TESTING.md");
    }

    #[test]
    fn check_ids() {
        assert!(is_check_id("13.4"));
        assert!(is_check_id("11.9b"));
        assert!(!is_check_id("13."));
        assert!(!is_check_id("v1.2"));
        assert!(!is_check_id("#"));
    }

    #[test]
    fn only_a_test_doc_comment_claims_a_row() {
        let src = "/// TESTING.md 4.1 and 4.2: the list.\n#[test]\nfn a() {}\n\n\
                   /// Explains 5.1: not a test.\nfn b() {}\n\n\
                   /// 6.1, 6.2: both.\n#[cfg(test)]\nmod t {}\n\n\
                   /// 12.9 to 12.12 stay with the machine\n#[test]\nfn c() {}\n";
        let ids: Vec<String> = claimed_ids(src).into_iter().collect();
        assert_eq!(ids, ["4.1", "4.2", "6.1", "6.2"]);
    }

    #[test]
    fn writes_keeps_ticks_and_checks() {
        let d = dir("ticks");
        let cfg = config();
        let r = run_with(&cfg, &d, &[], auto());
        assert_eq!(r.code, 0, "{r:?}");
        let text = std::fs::read_to_string(d.join("CHECKS.md")).unwrap();
        assert!(text.starts_with("# List\n\n3 of 4, 2 untranslated.\n"), "{text}");
        assert!(text.contains("## 1. 一つめ\n\n自動テスト済みなので下には出していない: 1.3\n\n準備:\n\nmkdir x\n"), "{text}");
        assert!(text.contains("- [ ] **1.1** `a` を押す — *Press `a` → It opens*\n"), "{text}");
        assert!(text.contains("- [ ] **1.2** Press `b` → It closes (v0.2.0) 〔未訳〕\n"), "{text}");

        let ticked = text.replace("- [ ] **1.1**", "- [x] **1.1**").replace("- [ ] **1.2**", "- [~] **1.2**");
        std::fs::write(d.join("CHECKS.md"), &ticked).unwrap();
        let r = run_with(&cfg, &d, &args(&["--check"]), auto());
        assert_eq!(r.stdout, "CHECKS.md: in sync with TESTING.md (1 / 3 checked, 1 looked at, 2 untranslated)\n");
        let r = run_with(&cfg, &d, &args(&["--stats"]), auto());
        assert!(r.stdout.contains("1 / 3 checked, 0 not applicable, 1 looked at ([~]), 2 untranslated"), "{}", r.stdout);
        assert!(r.stdout.contains("  1. 1 / 2, 1 looked at  First (v0.1.0)"), "{}", r.stdout);
        assert_eq!(run_with(&cfg, &d, &[], auto()).code, 0);
        assert_eq!(std::fs::read_to_string(d.join("CHECKS.md")).unwrap(), ticked);
    }

    #[test]
    fn a_lane_drops_the_marks_it_does_not_keep() {
        let d = dir("lanes");
        let cfg = config();
        run_with(&cfg, &d, &args(&["--lane", "linux"]), auto());
        let text = std::fs::read_to_string(d.join("LINUX.md")).unwrap();
        assert!(!text.contains("準備"), "the setup is for the first lane only");
        std::fs::write(d.join("LINUX.md"), text.replace("- [ ] **1.1**", "- [~] **1.1**").replace("- [ ] **1.2**", "- [-] **1.2**"))
            .unwrap();
        let r = run_with(&cfg, &d, &args(&["--lane", "linux", "--check"]), auto());
        assert_eq!(r.code, 1);
        assert!(r.stdout.contains("surrounding text"), "{}", r.stdout);
        assert!(r.stderr.contains("cargo run --example make-testcheck -- --lane linux\n"), "{}", r.stderr);
        run_with(&cfg, &d, &args(&["--lane", "linux"]), auto());
        let text = std::fs::read_to_string(d.join("LINUX.md")).unwrap();
        assert!(text.contains("- [ ] **1.1**") && text.contains("- [-] **1.2**"), "{text}");
    }

    #[test]
    #[should_panic(expected = "--lane takes `windows` or `linux`, not `mac`")]
    fn an_unknown_lane_is_named() {
        run_with(&config(), &dir("unknown"), &args(&["--lane", "mac"]), auto());
    }

    #[test]
    fn drift_names_the_rows() {
        let d = dir("drift");
        let cfg = config();
        run_with(&cfg, &d, &[], auto());
        let text = TESTING.replace("It opens", "It opens wide").replace("| 2.1 |", "| 2.2 |");
        std::fs::write(d.join("TESTING.md"), text).unwrap();
        let mut covered = auto();
        covered.insert("1.2".into());
        let r = run_with(&cfg, &d, &args(&["--check"]), covered);
        assert_eq!(r.code, 1);
        assert_eq!(
            r.stdout,
            "~ 1.1\n    was: Press `a` → It opens\n    now: Press `a` → It opens wide\n\
             * 1.2 — 自動テストが覆ったので一覧から外れる\n+ 2.2 — Linux → Run → It runs — **and stays**\n- 2.1 (gone from TESTING.md)\n"
        );
        assert_eq!(
            r.stderr,
            "\nCHECKS.md is out of date. Regenerate it, read the diff, and commit it:\n\n    cargo run --example make-testcheck\n\n"
        );
    }

    #[test]
    fn a_gloss_on_an_older_version_fails() {
        let d = dir("skew");
        std::fs::write(d.join("ja.toml"), "[row]\n\"1.2\" = \"`b` で閉じる（v0.1.0）\"\n").unwrap();
        let r = run_with(&config(), &d, &args(&["--stats"]), auto());
        assert_eq!(r.code, 1);
        assert_eq!(
            r.stderr,
            "ja.toml: the version a row ends at differs from TESTING.md:\n  1.2: TESTING.md ends at Some(\"0.2.0\"), the Japanese at Some(\"0.1.0\")\n"
        );
    }

    #[test]
    fn the_release_list_names_live_ids() {
        let d = dir("release");
        let cfg = config();
        run_with(&cfg, &d, &[], auto());
        std::fs::write(d.join("ARM.md"), "# ARM\n\n- **1.1** a\n- **9.9** gone\n").unwrap();
        let r = run_with(&cfg, &d, &args(&["--check"]), auto());
        assert_eq!((r.code, r.stderr.as_str()), (1, "ARM.md names ids TESTING.md does not have: 9.9\n"));
        // The other lane does not check it.
        run_with(&cfg, &d, &args(&["--lane", "linux"]), auto());
        assert_eq!(run_with(&cfg, &d, &args(&["--lane", "linux", "--check"]), auto()).code, 0);
    }

    #[test]
    fn orphaned_ticks_are_listed() {
        let d = dir("orphans");
        let cfg = config();
        std::fs::write(d.join("CHECKS.md"), "- [x] **7.1** gone\n").unwrap();
        run_with(&cfg, &d, &[], auto());
        let text = std::fs::read_to_string(d.join("CHECKS.md")).unwrap();
        assert!(text.contains("## 済みだが TESTING.md に無い項目\n") && text.ends_with("- `7.1`\n"), "{text}");
    }

    #[test]
    fn rows_read_back_from_the_front() {
        let file = "- [x] **1.5** list — **and the shell is still there** 〔未訳〕\n- [ ] **2.1** 訳 — *Run → It runs — **and stays***\n";
        let rows = rows_of_file(file, false);
        assert_eq!(rows["1.5"], "list — **and the shell is still there**");
        assert_eq!(rows["2.1"], "Run → It runs — **and stays**");
    }

    #[test]
    fn the_last_version() {
        assert_eq!(last_version("x (v0.9.1) then （v0.10.0）"), Some("0.10.0".into()));
        assert_eq!(last_version("nav0.1.2 v1.2"), None);
    }
}
