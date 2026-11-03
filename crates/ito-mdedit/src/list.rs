use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};

use crate::{change_from_edits, line_of, lines, Change, Dir, SelChange};

struct Item {
    indent: usize,
    /// Bytes from the marker's start to the item's text (`- ` is 2, `10. ` is 4).
    marker: usize,
    /// Digits of the number of an ordered item.
    digits: usize,
    number: Option<u64>,
}

fn item(line: &str) -> Option<Item> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let b = rest.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return Some(Item { indent, marker: 2, digits: 0, number: None });
    }
    let d = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&d) && b.len() > d + 1 && matches!(b[d], b'.' | b')') && b[d + 1] == b' ' {
        return Some(Item { indent, marker: d + 2, digits: d, number: rest[..d].parse().ok() });
    }
    None
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// Indent the list item on the line of `byte` with its nested lines. It moves to the content column of the item above
/// it, its new parent (3 under `9. `, 4 under `10. `); with no item above, by 2 under a bullet or the width of its own
/// marker under a number. The ordered lists the item leaves and joins are numbered again (a new sublist from 1).
pub fn indent_item(text: &str, byte: usize) -> Option<Change> {
    shift_item(text, byte, true)
}

/// Take the list item on the line of `byte` and its nested lines out by one level: back to the column where its
/// parent's marker starts, so an indent and an outdent undo each other, numbers included. `None` at the left edge.
pub fn outdent_item(text: &str, byte: usize) -> Option<Change> {
    shift_item(text, byte, false)
}

/// How many columns the item on line `k` moves: in, to the content column of the item above it; out, to the column of
/// its parent's marker. Without such an item it moves by its own marker (2 for a bullet). `0` when it cannot move.
fn amount(text: &str, ls: &[Range<usize>], code: &[bool], k: usize, it: &Item, tabbed: bool, deeper: bool) -> usize {
    let w = it.indent;
    let own = if it.number.is_some() { it.marker } else { 2 };
    let item_at = |j: usize| if code[j] { None } else { item_ws(&text[ls[j].clone()]).map(|(i, _)| i) };
    let nearest = |less: bool| {
        (0..k).rev().find(|&j| {
            let l = &text[ls[j].clone()];
            let lw = lead(l).0;
            !blank(l) && if less { lw < w } else { lw <= w }
        })
    };
    if deeper {
        if let Some(p) = nearest(false).and_then(item_at) {
            let to = p.indent + p.marker;
            if to > w {
                return to - w;
            }
        }
        // With no item to go under, the item moves by its marker, but from column 0 not past column 3 (4 is a code block).
        return if w == 0 { own.min(3) } else { own };
    }
    if w == 0 {
        return 0;
    }
    if tabbed {
        // An item indented with a tab goes out to the previous tab stop.
        return w - (w - 1) / 4 * 4;
    }
    if let Some(p) = nearest(true).and_then(item_at) {
        // Out to the parent's marker; an item deeper than the parent's content first comes to that content.
        let content = p.indent + p.marker;
        return w - if w > content { content } else { p.indent };
    }
    own.min(w)
}

/// How the leading blanks of a line change.
#[derive(Clone, Copy)]
enum How {
    /// By this many columns; the blanks are written as spaces.
    Width(i64),
    /// One more tab.
    TabIn,
    /// One tab less (or 4 columns of spaces).
    TabOut,
}

/// Push the edit that changes the leading blanks of line `k`, when they change.
fn shift_edit(text: &str, ls: &[Range<usize>], k: usize, how: How, edits: &mut Vec<(Range<usize>, String)>) {
    let line = &text[ls[k].clone()];
    let (lw, ln) = lead(line);
    let old = &line[..ln];
    let new = match how {
        How::Width(d) => " ".repeat((lw as i64 + d).max(0) as usize),
        How::TabIn => format!("\t{old}"),
        How::TabOut => match old.strip_prefix('\t') {
            Some(rest) => rest.to_string(),
            None => " ".repeat(lw - lw.min(4)),
        },
    };
    if new != old {
        edits.push((ls[k].start..ls[k].start + ln, new));
    }
}

/// An item (with its nested lines) that moved, for [`finish`].
struct Moved {
    li: usize,
    last: usize,
    old_w: usize,
}

fn shift_item(text: &str, byte: usize, deeper: bool) -> Option<Change> {
    let byte = byte.min(text.len());
    let ls = lines(text);
    let li = line_of(&ls, byte)?;
    let code = analyze(text, &ls).code;
    if code[li] {
        return None;
    }
    let (it, lead_n) = item_ws(&text[ls[li].clone()])?;
    let w = it.indent;
    let tabbed = text[ls[li].start..ls[li].start + lead_n].contains('\t');
    let n = amount(text, &ls, &code, li, &it, tabbed, deeper);
    if n == 0 {
        return None;
    }
    let last = extent(text, &ls, li, w);
    let d = if deeper { n as i64 } else { -(n as i64) };
    let mut edits = Vec::new();
    for k in li..=last {
        if !blank(&text[ls[k].clone()]) {
            shift_edit(text, &ls, k, How::Width(d), &mut edits);
        }
    }
    let (mut change, at) = finish(text, &ls, &code, edits, &[Moved { li, last, old_w: w }], &[(byte, true)])?;
    change.cursor = at[0];
    Some(change)
}

/// What [`shift_lines`] adds to or takes from the indent of a line that is not in a list item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// This many spaces.
    Spaces(u8),
    /// One tab (out: one tab, or 4 spaces).
    Tab,
}

impl Unit {
    fn how(self, deeper: bool) -> How {
        match self {
            Unit::Spaces(n) => How::Width(if deeper { n as i64 } else { -(n as i64) }),
            Unit::Tab if deeper => How::TabIn,
            Unit::Tab => How::TabOut,
        }
    }
}

/// Tab (`deeper`) or Shift+Tab over a selection of `sel` (a range; `start > end` is a selection made backwards, and
/// the selection that comes back is backwards too, with the caret at its end). One parse, one [`Change`].
///
/// - A list item on a selected line moves with its nested lines, by what [`indent_item`] / [`outdent_item`] move it;
///   a run of items at one indent moves by one amount, so `9.` and `10.` stay in line. Nested items that are selected
///   too move with their parent, not once more. Numbers are counted again as [`indent_item`] does.
/// - A line that is not in an item gets `unit`, except after a moved item in the selection: it moves by the amount of
///   the last item that moved before it (even when that amount is 0), so it is neither pulled into the list nor left
///   out of it, and Tab then Shift+Tab gives the text back. Lines before the first selected item get `unit`.
/// - Blank lines are not touched (nor do they count as the end of an item). A table row moves only with an item that
///   holds it. Lines of a fenced code block are text, but a fence that the selection covers only in part moves whole,
///   from its opening to its closing line.
/// - A non-empty selection that ends at the start of a line does not touch that line.
/// - Leading tabs are read with a width of 4; a line that changes is written with spaces (`Unit::Tab` keeps its tabs).
///
/// `None` when nothing moves: blank lines only, the left edge, a caret in no line.
pub fn shift_lines(text: &str, sel: Range<usize>, deeper: bool, unit: Unit) -> Option<SelChange> {
    let (from, to) = (sel.start.min(text.len()), sel.end.min(text.len()));
    let (lo, hi) = (from.min(to), from.max(to));
    let ls = lines(text);
    if lo == hi && lo > ls.last()?.end {
        // On the empty line after the last line break.
        return None;
    }
    let mut a = line_of(&ls, lo)?;
    let mut b = line_of(&ls, hi)?;
    if hi > lo && b > a && ls[b].start == hi {
        b -= 1;
    }
    // Cheap answers before the parse.
    let mut any = false;
    let mut indented = false;
    for l in &ls[a..=b] {
        let line = &text[l.clone()];
        if !blank(line) {
            any = true;
            indented |= lead(line).1 > 0;
        }
    }
    if !any || (!deeper && !indented && !text.contains("```") && !text.contains("~~~")) {
        return None;
    }
    let bl = analyze(text, &ls);
    // A fence the selection touches moves whole.
    let first = bl.fences.partition_point(|f| f.1 < a);
    let end = b;
    for f in bl.fences[first..].iter().take_while(|f| f.0 <= end) {
        a = a.min(f.0);
        b = b.max(f.1);
    }
    let sign = |n: usize| if deeper { n as i64 } else { -(n as i64) };
    let mut edits = Vec::new();
    let mut moved = Vec::new();
    // The amounts of the items of the selection by indent: a run of siblings moves alike.
    let mut stack: Vec<(usize, usize)> = Vec::new();
    // What the last moved item moved by.
    let mut after_item: Option<usize> = None;
    let mut k = a;
    while k <= b {
        let line = &text[ls[k].clone()];
        if blank(line) {
            k += 1;
            continue;
        }
        let lw = lead(line).0;
        let it = if bl.code[k] { None } else { item_ws(line) };
        if let Some((it, lead_n)) = it {
            while stack.last().is_some_and(|&(w, _)| w > lw) {
                stack.pop();
            }
            let n = match stack.last() {
                Some(&(w, n)) if w == lw => n,
                _ => {
                    let tabbed = line[..lead_n].contains('\t');
                    let n = amount(text, &ls, &bl.code, k, &it, tabbed, deeper);
                    stack.push((lw, n));
                    n
                }
            };
            let last = extent(text, &ls, k, lw);
            after_item = Some(n);
            if n > 0 {
                for j in k..=last {
                    if !blank(&text[ls[j].clone()]) {
                        shift_edit(text, &ls, j, How::Width(sign(n)), &mut edits);
                    }
                }
                moved.push(Moved { li: k, last, old_w: lw });
            }
            k = last + 1;
            continue;
        }
        while stack.last().is_some_and(|&(w, _)| w >= lw) {
            stack.pop();
        }
        if !bl.table[k] {
            let how = after_item.map_or_else(|| unit.how(deeper), |n| How::Width(sign(n)));
            shift_edit(text, &ls, k, how, &mut edits);
        }
        k += 1;
    }
    if edits.is_empty() {
        return None;
    }
    let (mut change, at) = finish(text, &ls, &bl.code, edits, &moved, &[(lo, lo == hi), (hi, true)])?;
    let selection = if from <= to { at[0]..at[1] } else { at[1]..at[0] };
    change.cursor = selection.end;
    Some(SelChange { change, selection, unflanked: false })
}

/// Where `pos` of the old text is after `edits` (sorted, in old bytes). A position inside an edit goes into its new
/// text, at most to its end. One at the start of an edit stays before it, unless it is an insertion and `follow`.
fn map_pos(pos: usize, edits: &[(Range<usize>, String)], follow: bool) -> usize {
    let mut delta = 0isize;
    for (r, s) in edits {
        if pos < r.start || (pos == r.start && !(follow && r.is_empty())) {
            break;
        }
        if pos >= r.end {
            delta += s.len() as isize - r.len() as isize;
        } else {
            return (r.start as isize + delta) as usize + (pos - r.start).min(s.len());
        }
    }
    (pos as isize + delta) as usize
}

fn apply_edits(text: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut at = 0;
    for (r, s) in edits {
        out.push_str(&text[at..r.start]);
        out.push_str(s);
        at = r.end;
    }
    out.push_str(&text[at..]);
    out
}

/// After the items in `moved` changed their indent (`edits`: the new blanks of their lines, sorted), number the
/// ordered lists they touch again: the list an item joined (a new sublist starts at 1: a nested list that starts at
/// another number would not be a list) and the one it left. Gives one [`Change`] (its `cursor` is not set) and where
/// each of `pos` (a byte, and whether it follows an insertion at it) goes. The edits keep the lines and their code
/// flags as they are, so nothing is parsed again.
fn finish(
    text: &str,
    ls: &[Range<usize>],
    code: &[bool],
    edits: Vec<(Range<usize>, String)>,
    moved: &[Moved],
    pos: &[(usize, bool)],
) -> Option<(Change, Vec<usize>)> {
    let mid = apply_edits(text, &edits);
    let ls2 = lines(&mid);
    debug_assert_eq!(ls.len(), ls2.len());
    let mut targets = Vec::new();
    let mut floor = 0;
    for m in moved {
        let line = |k: usize| &mid[ls2[k].clone()];
        let ordered_at = |k: &usize| item_ws(line(*k)).is_some_and(|(i, _)| i.indent == m.old_w && i.number.is_some());
        targets.push(m.li);
        targets.extend((floor..m.li).rev().find(|&k| !blank(line(k)) && lead(line(k)).0 <= m.old_w).filter(ordered_at));
        targets.extend((m.last + 1..ls2.len()).find(|&k| !blank(line(k))).filter(ordered_at));
        floor = m.last + 1;
    }
    targets.sort_unstable();
    targets.dedup();
    let mut digits: Vec<(Range<usize>, String)> = Vec::new();
    // (indent, last line) of the lists numbered already: a target inside one needs no second look.
    let mut done: Vec<(usize, usize)> = Vec::new();
    for t in targets {
        let Some((it, _)) = item_ws(&mid[ls2[t].clone()]) else { continue };
        if done.iter().any(|&(w, e)| w == it.indent && t <= e) {
            continue;
        }
        let (fresh, end) = renumber_edits(&mid, &ls2, code, t, true);
        done.push((it.indent, end));
        digits.extend(fresh);
    }
    digits.sort_by_key(|(r, _)| r.start);
    digits.dedup();
    let last = if digits.is_empty() { mid.clone() } else { apply_edits(&mid, &digits) };
    let (range, new) = diff(text, &last)?;
    let at = pos.iter().map(|&(p, follow)| map_pos(map_pos(p, &edits, follow), &digits, follow)).collect();
    Some((Change { range, text: new, cursor: 0 }, at))
}

/// The smallest replacement that turns `old` into `new`; `None` when they are the same.
fn diff(old: &str, new: &str) -> Option<(Range<usize>, String)> {
    if old == new {
        return None;
    }
    let (ob, nb) = (old.as_bytes(), new.as_bytes());
    let mut p = ob.iter().zip(nb).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(p) || !new.is_char_boundary(p) {
        p -= 1;
    }
    let mut s = ob[p..].iter().rev().zip(nb[p..].iter().rev()).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(old.len() - s) || !new.is_char_boundary(new.len() - s) {
        s -= 1;
    }
    Some((p..old.len() - s, new[p..new.len() - s].to_string()))
}

/// What one parse of a text says about its lines.
pub(crate) struct Analysis {
    /// For each line, whether it is inside a code block (fenced or indented). The line of an item that holds a fence
    /// after its marker is not code.
    pub code: Vec<bool>,
    /// For each line, whether it is inside a table.
    pub table: Vec<bool>,
    /// The first and the last line of each fenced code block.
    pub fences: Vec<(usize, usize)>,
}

pub(crate) fn analyze(text: &str, ls: &[Range<usize>]) -> Analysis {
    let (mut code, mut table, mut fenced) = (Vec::new(), Vec::new(), Vec::new());
    for (e, r) in Parser::new_ext(text, Options::ENABLE_TABLES).into_offset_iter() {
        match e {
            Event::Start(Tag::CodeBlock(kind)) => {
                if matches!(kind, CodeBlockKind::Fenced(_)) {
                    fenced.push(r.clone());
                }
                code.push(r);
            }
            Event::Start(Tag::Table(_)) => table.push(r),
            _ => {}
        }
    }
    // The blocks come in the order they start and do not overlap.
    let flags = |v: &[Range<usize>]| -> Vec<bool> {
        if v.is_empty() {
            return vec![false; ls.len()];
        }
        ls.iter()
            .map(|l| {
                let at = l.start + lead(&text[l.clone()]).1;
                let i = v.partition_point(|b| b.start <= at);
                i > 0 && at < v[i - 1].end
            })
            .collect()
    };
    let fences = fenced.iter().filter_map(|r| Some((line_of(ls, r.start)?, line_of(ls, r.end.saturating_sub(1).max(r.start))?))).collect();
    Analysis { code: flags(&code), table: flags(&table), fences }
}

/// For each line, whether it is inside a code block (fenced or indented). The line of an item that holds a fence
/// after its marker is not code.
pub(crate) fn code_lines(text: &str, ls: &[Range<usize>]) -> Vec<bool> {
    analyze(text, ls).code
}

/// The width of the leading blanks of `line` (a tab goes to the next multiple of 4) and their byte length.
fn lead(line: &str) -> (usize, usize) {
    let mut w = 0;
    let mut n = 0;
    for c in line.bytes() {
        match c {
            b' ' => w += 1,
            b'\t' => w = (w / 4 + 1) * 4,
            _ => break,
        }
        n += 1;
    }
    (w, n)
}

/// An item whose indent may hold tabs: the item (its `indent` is the width) and the byte length of its leading blanks.
fn item_ws(line: &str) -> Option<(Item, usize)> {
    let (w, n) = lead(line);
    let mut it = item(&line[n..])?;
    it.indent = w;
    Some((it, n))
}

/// The last line of the item starting on line `k` with the indent `w`: the lines deeper than it (blank lines inside).
fn extent(text: &str, ls: &[Range<usize>], k: usize, w: usize) -> usize {
    let mut last = k;
    for (j, l) in ls.iter().enumerate().skip(k + 1) {
        let line = &text[l.clone()];
        if blank(line) {
            continue;
        }
        if lead(line).0 > w {
            last = j;
        } else {
            break;
        }
    }
    last
}

/// The line of the item that owns line `li`: the line itself when it starts an item, else the nearest item above that
/// is less indented (so the caret may sit on a continuation line).
fn owner(text: &str, ls: &[Range<usize>], code: &[bool], li: usize) -> Option<usize> {
    let line = &text[ls[li].clone()];
    if item_ws(line).is_some() && !code[li] {
        return Some(li);
    }
    if blank(line) {
        return None;
    }
    let mut w = lead(line).0;
    for k in (0..li).rev() {
        let l = &text[ls[k].clone()];
        if blank(l) {
            continue;
        }
        let wk = lead(l).0;
        if wk < w {
            if item_ws(l).is_some() && !code[k] {
                return Some(k);
            }
            w = wk;
        }
    }
    None
}

/// Move the list item under the caret (with its nested items and continuation lines) up or down past its sibling in
/// the same list, and the caret goes with it. `None` when the caret is in no list item or the item is the first
/// (going up) or the last (going down) of its list, so an app can fall back to [`move_block`](crate::move_block).
/// Items of an ordered list swap their text but keep their numbers in place, so the numbering stays as it was.
pub fn move_item(text: &str, byte: usize, dir: Dir) -> Option<Change> {
    let byte = byte.min(text.len());
    let ls = lines(text);
    let code = code_lines(text, &ls);
    let at = line_of(&ls, byte)?;
    if code[at] {
        return None;
    }
    let li = owner(text, &ls, &code, at)?;
    let (it, _) = item_ws(&text[ls[li].clone()])?;
    let n = it.indent;
    let ordered = it.number.is_some();
    let me = li..=extent(text, &ls, li, n);
    let same = |k: usize| item_ws(&text[ls[k].clone()]).filter(|(i, _)| i.indent == n && i.number.is_some() == ordered && !code[k]);
    // (a, b) are the two neighbours as inclusive line ranges; the caret's item is b going up, a going down.
    let (a, b) = match dir {
        Dir::Up => {
            let mut s = None;
            for k in (0..li).rev() {
                let l = &text[ls[k].clone()];
                if blank(l) || lead(l).0 > n {
                    continue;
                }
                if same(k).is_some() {
                    s = Some(k);
                }
                break;
            }
            let s = s?;
            (s..=extent(text, &ls, s, n), me)
        }
        Dir::Down => {
            let k = (*me.end() + 1..ls.len()).find(|&k| !blank(&text[ls[k].clone()]))?;
            same(k)?;
            (me, k..=extent(text, &ls, k, n))
        }
    };
    let up = dir == Dir::Up;
    let (ar, br) = (ls[*a.start()].start..ls[*a.end()].end, ls[*b.start()].start..ls[*b.end()].end);
    let gap = &text[ar.end..br.start];
    // Where the digits of an ordered item's number are: (start byte, length).
    let number_of = |r: &Range<usize>| -> Option<(usize, usize)> {
        let (i, w) = item_ws(text[r.start..].lines().next().unwrap_or(""))?;
        (i.number.is_some()).then_some((r.start + w, i.digits))
    };
    let (na, nb) = (number_of(&ar), number_of(&br));
    let digits = |p: Option<(usize, usize)>| p.map(|(s, d)| &text[s..s + d]);
    // The item `r` with its number replaced by `num`.
    let put = |r: &Range<usize>, own: Option<(usize, usize)>, num: Option<&str>| -> String {
        match (own, num) {
            (Some((s, d)), Some(num)) => format!("{}{}{}", &text[r.start..s], num, &text[s + d..r.end]),
            _ => text[r.clone()].to_string(),
        }
    };
    // Each item takes the number of the place it moves to.
    let first = put(&br, nb, digits(na));
    let second = put(&ar, na, digits(nb));
    let mut new = String::with_capacity(br.end - ar.start);
    new.push_str(&first);
    new.push_str(gap);
    new.push_str(&second);
    let (mine, own, new_start, new_digits) = if up {
        (&br, nb, ar.start, digits(na).map_or(0, str::len))
    } else {
        (&ar, na, ar.start + first.len() + gap.len(), digits(nb).map_or(0, str::len))
    };
    let mut off = byte.clamp(mine.start, mine.end) - mine.start;
    if let Some((s, d)) = own {
        if off >= s + d - mine.start {
            off = off + new_digits - d;
        }
    }
    Some(Change { range: ar.start..br.end, text: new, cursor: new_start + off })
}

/// Tick or untick the task on the line of `byte` (`[ ]` and `[x]`). A list item that is not a task becomes one
/// (`- [ ] `).
pub fn toggle_task(text: &str, byte: usize) -> Option<Change> {
    let byte = byte.min(text.len());
    let ls = lines(text);
    let li = line_of(&ls, byte)?;
    if code_lines(text, &ls)[li] {
        return None;
    }
    let (it, lead_n) = item_ws(&text[ls[li].clone()])?;
    let at = ls[li].start + lead_n + it.marker;
    let rest = &text[at..ls[li].end];
    let rb = rest.as_bytes();
    if rb.len() >= 3 && rb[0] == b'[' && rb[2] == b']' && matches!(rb[1], b' ' | b'x' | b'X') && (rb.len() == 3 || rb[3] == b' ') {
        let flipped = if rb[1] == b' ' { "x" } else { " " };
        return Some(Change { range: at + 1..at + 2, text: flipped.to_string(), cursor: byte });
    }
    let cursor = if byte >= at { byte + 4 } else { byte };
    Some(Change { range: at..at, text: "[ ] ".to_string(), cursor })
}

/// Number the ordered items of the list holding `byte` (the ones at the same indent as the item on its line) one,
/// two, three… from the number of the first. `None` when the numbers are already right or the line is no ordered item.
pub fn renumber(text: &str, byte: usize) -> Option<Change> {
    let byte = byte.min(text.len());
    let ls = lines(text);
    let li = line_of(&ls, byte)?;
    let code = code_lines(text, &ls);
    let (edits, _) = renumber_edits(text, &ls, &code, li, false);
    change_from_edits(text, &edits, byte)
}

/// The edits (the digits of each number that is wrong) that number the list holding the ordered item on line `li`.
/// With `nested_from_one`, a list that is indented and follows a line of text starts at 1.
fn renumber_edits(
    text: &str,
    ls: &[Range<usize>],
    code: &[bool],
    li: usize,
    nested_from_one: bool,
) -> (Vec<(Range<usize>, String)>, usize) {
    let Some((it, _)) = item_ws(&text[ls[li].clone()]) else { return (Vec::new(), li) };
    if code[li] || it.number.is_none() {
        return (Vec::new(), li);
    }
    let n = it.indent;
    let belongs = |k: usize| -> bool {
        let line = &text[ls[k].clone()];
        if blank(line) {
            return true;
        }
        let ind = lead(line).0;
        if ind < n {
            return false;
        }
        if ind == n {
            return !code[k] && item_ws(line).is_some_and(|(i, _)| i.number.is_some());
        }
        true
    };
    let (mut s, mut e) = (li, li);
    while s > 0 && belongs(s - 1) {
        s -= 1;
    }
    while e + 1 < ls.len() && belongs(e + 1) {
        e += 1;
    }
    while s < li && blank(&text[ls[s].clone()]) {
        s += 1;
    }
    while e > li && blank(&text[ls[e].clone()]) {
        e -= 1;
    }
    let from_one = nested_from_one && n > 0 && s > 0 && !blank(&text[ls[s - 1].clone()]);
    let mut counter: Option<u64> = None;
    let mut edits = Vec::new();
    for k in s..=e {
        let Some((i, bytes)) = item_ws(&text[ls[k].clone()]).filter(|(i, _)| i.indent == n && i.number.is_some() && !code[k]) else {
            continue;
        };
        let c = *counter.get_or_insert(if from_one { 1 } else { i.number.unwrap_or(1) });
        counter = Some(c + 1);
        let digits = ls[k].start + bytes..ls[k].start + bytes + i.digits;
        let fresh = c.to_string();
        if text[digits.clone()] != fresh {
            edits.push((digits, fresh));
        }
    }
    (edits, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_indent_with_their_children() {
        let t = "- a\n- b\n  - c\n- d\n";
        let at = t.find("- b").unwrap() + 3;
        let c = indent_item(t, at).unwrap();
        assert_eq!(c.apply(t), "- a\n  - b\n    - c\n- d\n");
        assert_eq!(c.cursor, at + 2);
        let t2 = c.apply(t);
        let c = outdent_item(&t2, c.cursor).unwrap();
        assert_eq!(c.apply(&t2), t);
        assert_eq!(c.cursor, at);
        assert!(outdent_item(t, 0).is_none());
        assert!(indent_item("text\n", 0).is_none());
    }

    #[test]
    fn numbered_items_indent_by_their_marker() {
        let t = "1. a\n2. b\n";
        assert_eq!(indent_item(t, 5).unwrap().apply(t), "1. a\n   1. b\n");
    }

    #[test]
    fn code_is_not_a_list() {
        let fenced = "```\n- in fence\n1. one\n2. two\n```\n";
        let at = fenced.find("- in").unwrap() + 3;
        assert!(indent_item(fenced, at).is_none());
        assert!(outdent_item(fenced, at).is_none());
        assert!(toggle_task(fenced, at).is_none());
        assert!(move_item(fenced, at, Dir::Down).is_none());
        assert!(move_item(fenced, at, Dir::Up).is_none());
        let two = fenced.find("2. two").unwrap();
        assert!(renumber(fenced, two).is_none());
        assert!(move_item(fenced, two, Dir::Up).is_none());
        // An indented code block, after a blank line.
        let indented = "text\n\n    - code\n    - more\n";
        let at = indented.find("- code").unwrap();
        assert!(indent_item(indented, at).is_none());
        assert!(toggle_task(indented, at).is_none());
        assert!(move_item(indented, at, Dir::Down).is_none());
        // A real item whose own text opens a fence is still an item.
        let t = "- ```\n  x\n  ```\n";
        assert!(indent_item(t, 0).is_some());
        // The list around a fence is still a list.
        let t = "- a\n  ```\n  - b\n  ```\n- c\n";
        assert!(indent_item(t, t.rfind("- c").unwrap()).is_some());
        assert!(indent_item(t, t.find("- b").unwrap()).is_none());
    }

    #[test]
    fn numbered_lists_stay_numbered_when_indented_or_outdented() {
        let t = "1. a\n2. b\n3. c\n";
        let c = indent_item(t, 8).unwrap();
        assert_eq!(c.apply(t), "1. a\n   1. b\n2. c\n");
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'b');
        // The next item joins the sublist and goes on from 1.
        let t2 = "1. a\n   1. b\n2. c\n";
        let c = indent_item(t2, t2.find("2. c").unwrap()).unwrap();
        assert_eq!(c.apply(t2), "1. a\n   1. b\n   2. c\n");
        // Out again: both lists are numbered from their start.
        let t3 = "1. a\n   1. b\n   2. c\n   3. d\n2. e\n";
        let c = outdent_item(t3, t3.find("2. c").unwrap()).unwrap();
        assert_eq!(c.apply(t3), "1. a\n   1. b\n2. c\n   1. d\n3. e\n");
        let c = outdent_item(t3, t3.find("1. b").unwrap()).unwrap();
        assert_eq!(c.apply(t3), "1. a\n2. b\n   1. c\n   2. d\n3. e\n");
        // The change is one edit from the old text; the caret stays on its item.
        let c = outdent_item(t3, t3.find("3. d").unwrap() + 3).unwrap();
        assert_eq!(c.apply(t3), "1. a\n   1. b\n   2. c\n2. d\n3. e\n");
        assert_eq!(&c.apply(t3)[c.cursor..c.cursor + 1], "d");
        // A bullet under a number, and a number under a bullet, are left as they are.
        let t4 = "- a\n- b\n";
        assert_eq!(indent_item(t4, 4).unwrap().apply(t4), "- a\n  - b\n");
        let t5 = "- a\n- 1. b\n";
        assert!(indent_item(t5, 4).is_some());
    }

    #[test]
    fn tab_indented_items_are_items() {
        let t = "- top\n\t- tabbed\n\t\t- deep\n";
        let at = t.find("- tabbed").unwrap() + 2;
        let c = indent_item(t, at).unwrap();
        assert_eq!(c.apply(t), "- top\n      - tabbed\n          - deep\n");
        let c = outdent_item(t, at).unwrap();
        assert_eq!(c.apply(t), "- top\n- tabbed\n    - deep\n");
        assert_eq!(&c.apply(t)[c.cursor..c.cursor + 6], "tabbed");
        assert!(outdent_item("- top\n\t- t\n", 7).is_some());
        let t = "- top\n\t- a\n";
        assert!(toggle_task(t, 8).is_some());
        // Numbers of a tab-indented list are counted again too.
        let t = "- top\n\t1. a\n\t1. b\n";
        assert_eq!(renumber(t, t.find("1. b").unwrap()).unwrap().apply(t), "- top\n\t1. a\n\t2. b\n");
    }

    #[test]
    fn tasks_toggle_and_plain_items_become_tasks() {
        let t = "- [ ] a\n* [x] b\n- c\n";
        assert_eq!(toggle_task(t, 0).unwrap().apply(t), "- [x] a\n* [x] b\n- c\n");
        assert_eq!(toggle_task(t, 8).unwrap().apply(t), "- [ ] a\n* [ ] b\n- c\n");
        let c = toggle_task(t, t.len() - 2).unwrap();
        assert_eq!(c.apply(t), "- [ ] a\n* [x] b\n- [ ] c\n");
        assert_eq!(c.cursor, t.len() - 2 + 4);
        assert!(toggle_task("plain\n", 0).is_none());
    }

    #[test]
    fn numbers_are_counted_again() {
        let t = "intro\n\n1. a\n1. b\n   - x\n7. c\n\ntail\n";
        let c = renumber(t, t.find('b').unwrap()).unwrap();
        assert_eq!(c.apply(t), "intro\n\n1. a\n2. b\n   - x\n3. c\n\ntail\n");
        assert!(renumber(&c.apply(t), 8).is_none());
        let t = "9. a\n9. b\n";
        let c = renumber(t, 6).unwrap();
        assert_eq!(c.apply(t), "9. a\n10. b\n");
        assert_eq!(c.apply(t).as_bytes()[c.cursor], b'.');
        assert!(renumber("- a\n- b\n", 0).is_none());
    }

    fn mv(t: &str, at: usize, dir: Dir) -> Option<(String, usize)> {
        move_item(t, at, dir).map(|c| (c.apply(t), c.cursor))
    }

    #[test]
    fn an_item_moves_with_its_children_and_the_caret_follows() {
        let t = "- a\n- b\n  more\n  - c\n- d\n\nz\n";
        let at = t.find("- b").unwrap() + 3;
        let (n, cur) = mv(t, at, Dir::Down).unwrap();
        assert_eq!(n, "- a\n- d\n- b\n  more\n  - c\n\nz\n");
        assert_eq!(cur, n.find("- b").unwrap() + 3);
        let (back, cur) = mv(&n, cur, Dir::Up).unwrap();
        assert_eq!(back, t);
        assert_eq!(cur, at);
        // The caret on a continuation line moves the owning item; on a child it moves the child only.
        let (n, _) = mv(t, t.find("more").unwrap(), Dir::Up).unwrap();
        assert_eq!(n, "- b\n  more\n  - c\n- a\n- d\n\nz\n");
        let t2 = "- p\n  - x\n  - y\n- q\n";
        assert_eq!(mv(t2, t2.find("- y").unwrap(), Dir::Up).unwrap().0, "- p\n  - y\n  - x\n- q\n");
    }

    #[test]
    fn edges_and_non_lists_do_nothing() {
        let t = "- a\n- b\n\nc\n";
        assert!(mv(t, 0, Dir::Up).is_none());
        assert!(mv(t, 4, Dir::Down).is_none());
        assert!(mv("- only\n", 2, Dir::Down).is_none());
        assert!(mv("- only\n", 2, Dir::Up).is_none());
        assert!(mv(t, t.find('c').unwrap(), Dir::Up).is_none());
        // A child does not leave its parent, and a bullet does not swap with a number.
        assert!(mv("- p\n  - x\n- q\n", 6, Dir::Down).is_none());
        assert!(mv("- a\n1. b\n", 0, Dir::Down).is_none());
        assert!(mv("", 0, Dir::Down).is_none());
    }

    #[test]
    fn ordered_numbers_stay_in_place() {
        let t = "intro\n\n5. a\n   - x\n6. b\n7. c\n";
        let (n, cur) = mv(t, t.find("5. a").unwrap() + 4, Dir::Down).unwrap();
        assert_eq!(n, "intro\n\n5. b\n6. a\n   - x\n7. c\n");
        assert_eq!(cur, n.find("6. a").unwrap() + 4);
        let t = "9. a\n10. b\n";
        let (n, cur) = mv(t, 5 + 4, Dir::Up).unwrap();
        assert_eq!(n, "9. b\n10. a\n");
        assert_eq!(&n[cur..cur + 1], "b");
        let (n2, cur2) = mv(&n, n.find('a').unwrap(), Dir::Up).unwrap();
        assert_eq!(n2, "9. a\n10. b\n");
        assert_eq!(&n2[cur2..cur2 + 1], "a");
    }

    #[test]
    fn tasks_cjk_crlf_and_tabs() {
        let t = "- [x] あ\r\n- [ ] い\r\n\t- 子\r\n";
        let (n, cur) = mv(t, t.find("い").unwrap(), Dir::Up).unwrap();
        assert_eq!(n, "- [ ] い\r\n\t- 子\r\n- [x] あ\r\n");
        assert_eq!(cur, n.find("い").unwrap());
        let (n, _) = mv(t, 0, Dir::Down).unwrap();
        assert_eq!(n, "- [ ] い\r\n\t- 子\r\n- [x] あ\r\n");
        let t = "- p\n\t- a\n    - b\n";
        assert_eq!(mv(t, 4, Dir::Down).unwrap().0, "- p\n    - b\n\t- a\n");
    }

    #[test]
    fn indent_then_outdent_gives_the_text_back() {
        for nums in [1..=11u32, 95..=101] {
            let t: String = nums.map(|n| format!("{n}. x{n}\n")).collect();
            for l in lines(&t) {
                let c = indent_item(&t, l.start).unwrap();
                let moved = c.apply(&t);
                let back = outdent_item(&moved, c.cursor).unwrap_or_else(|| panic!("{moved:?} cursor {}", c.cursor));
                assert_eq!(back.apply(&moved), t, "item at byte {} of {t:?}", l.start);
            }
        }
        // The item goes to the content column of the item above, and the list it joins starts at 1.
        let t = "9. x9\n10. x10\n";
        assert_eq!(indent_item(t, 6).unwrap().apply(t), "9. x9\n   1. x10\n");
        // A bullet under `10. ` goes to column 4 and comes back to column 0.
        let t = "10. a\n- b\n";
        let c = indent_item(t, 7).unwrap();
        let moved = c.apply(t);
        assert_eq!(moved, "10. a\n    - b\n");
        assert_eq!(outdent_item(&moved, c.cursor).unwrap().apply(&moved), t);
        // An item deeper than its parent's content comes to that content first.
        let t = "10. a\n        - b\n";
        assert_eq!(outdent_item(t, 10).unwrap().apply(t), "10. a\n    - b\n");
    }

    fn sl(t: &str, sel: Range<usize>, deeper: bool) -> Option<(String, Range<usize>)> {
        shift_lines(t, sel, deeper, Unit::Spaces(2)).map(|c| (c.apply(t), c.selection))
    }

    fn all(t: &str, deeper: bool) -> Option<String> {
        sl(t, 0..t.len(), deeper).map(|r| r.0)
    }

    #[test]
    fn tab_and_shift_tab_over_a_mixed_selection_give_the_text_back() {
        let t = "para\n- p\n  - c\n```\ncode\n```\n";
        let (once, sel) = sl(t, 0..t.len(), true).unwrap();
        assert_eq!(once, "  para\n  - p\n    - c\n  ```\n  code\n  ```\n");
        assert_eq!(sel, 0..once.len());
        let (twice, sel) = sl(&once, sel, true).unwrap();
        let (back, sel) = sl(&twice, sel, false).unwrap();
        assert_eq!(back, once);
        let (back, _) = sl(&back, sel, false).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn a_line_after_a_moved_item_moves_like_the_item() {
        assert_eq!(all("- a\n- b\n\npara\n", true).unwrap(), "  - a\n  - b\n\n  para\n");
        assert_eq!(all("- a\n- b\npara\n", true).unwrap(), "  - a\n  - b\n  para\n");
        let t = "para\n- a\n";
        assert_eq!(all(t, true).unwrap(), "  para\n  - a\n");
        assert_eq!(all(&all(t, true).unwrap(), false).unwrap(), t);
        // Nothing moves a line that is at the left edge out, item or not.
        assert!(all("- a\npara\n", false).is_none());
    }

    #[test]
    fn numbered_items_move_alike_and_are_counted_again() {
        let t: String = (1..=11).map(|n| format!("{n}. x\n")).collect();
        let moved = all(&t, true).unwrap();
        assert_eq!(moved, t.lines().map(|l| format!("   {l}\n")).collect::<String>());
        assert_eq!(all(&moved, false).unwrap(), t);
        // Two items join the sublist of the item above; the list they left goes on without them.
        let t = "1. p\n2. a\n3. b\n4. q\n";
        let (moved, _) = sl(t, 5..14, true).unwrap();
        assert_eq!(moved, "1. p\n   1. a\n   2. b\n2. q\n");
        let (back, _) = sl(&moved, 5..moved.len() - 5, false).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn tables_move_only_with_an_item() {
        let t = "- a\n  | h | i |\n  |---|---|\n  | 1 | 2 |\n- b\n";
        assert_eq!(all(t, true).unwrap(), "  - a\n    | h | i |\n    |---|---|\n    | 1 | 2 |\n  - b\n");
        let t = "text\n\n| h | i |\n|---|---|\n| 1 | 2 |\n";
        assert_eq!(all(t, true).unwrap(), "  text\n\n| h | i |\n|---|---|\n| 1 | 2 |\n");
        assert!(all("| h | i |\n|---|---|\n| 1 | 2 |\n", true).is_none());
    }

    #[test]
    fn crlf_blank_lines_and_tabs_are_kept() {
        assert_eq!(all("a\r\n- b\r\n  c\r\n", true).unwrap(), "  a\r\n  - b\r\n    c\r\n");
        assert_eq!(all("a\n\n   \nb\n", true).unwrap(), "  a\n\n   \n  b\n");
        // A tab counts 4 columns: the item goes out to column 0 and its child keeps its place under it.
        let t = "- p\n\t- c\n\t\t- d\n";
        assert_eq!(sl(t, 4..t.len(), false).unwrap().0, "- p\n- c\n    - d\n");
        let u = |t: &str, deeper| shift_lines(t, 0..t.len(), deeper, Unit::Tab).map(|c| c.apply(t));
        assert_eq!(u("a\n  b\n", true).unwrap(), "\ta\n\t  b\n");
        assert_eq!(u("\ta\n    b\nc\n", false).unwrap(), "a\nb\nc\n");
    }

    #[test]
    fn the_selection_decides_which_lines_move() {
        // A selection that ends at the start of a line leaves that line alone.
        let (n, sel) = sl("a\nb\nc\n", 0..4, true).unwrap();
        assert_eq!((n.as_str(), sel), ("  a\n  b\nc\n", 0..8));
        // A caret moves the line it is on, and stays at the same place in its text.
        let t = "ab\ncd\n";
        let c = shift_lines(t, 4..4, true, Unit::Spaces(2)).unwrap();
        assert_eq!(c.apply(t), "ab\n  cd\n");
        assert_eq!(c.selection, 6..6);
        assert_eq!(c.change.cursor, 6);
        // At the start of the line the caret goes after the new blanks.
        assert_eq!(shift_lines(t, 3..3, true, Unit::Spaces(2)).unwrap().selection, 5..5);
        // A selection made backwards stays backwards, with the caret at its end.
        let t = "a\nb\n";
        let back = std::ops::Range { start: 3, end: 0 };
        let c = shift_lines(t, back, true, Unit::Spaces(2)).unwrap();
        assert_eq!(c.selection, std::ops::Range { start: 7, end: 0 });
        assert_eq!(c.change.cursor, 0);
        // A fence the selection covers in part moves whole.
        let t = "x\n```\nfoo\nbar\n```\ny\n";
        let at = t.find("foo").unwrap() + 1;
        let (n, _) = sl(t, at..at, true).unwrap();
        assert_eq!(n, "x\n  ```\n  foo\n  bar\n  ```\ny\n");
        let at = t.find("bar").unwrap();
        let (n, _) = sl(t, at - 3..at, true).unwrap();
        assert_eq!(n, "x\n  ```\n  foo\n  bar\n  ```\ny\n");
    }

    #[test]
    fn nothing_to_move_gives_none() {
        assert!(sl("", 0..0, true).is_none());
        assert!(sl("\n\n  \n", 0..5, true).is_none());
        assert!(sl("a\n", 2..2, true).is_none());
        assert!(all("a\n- b\n", false).is_none());
        let nested = "- a\n  - b\n";
        assert!(all(nested, false).is_none());
        assert!(sl(nested, 4..11, false).is_some());
        assert!(shift_lines("a\n", 0..2, true, Unit::Spaces(0)).is_none());
    }

    #[test]
    fn a_big_selection_is_one_quick_call() {
        let t: String = (0..1000).map(|i| format!("- item {i} {}\n", "lorem ipsum ".repeat(85))).collect();
        assert!(t.len() > 1_000_000);
        let best = (0..3)
            .map(|_| {
                let start = std::time::Instant::now();
                let c = shift_lines(&t, 0..t.len(), true, Unit::Spaces(2)).unwrap();
                let took = start.elapsed();
                assert_eq!(c.apply(&t).len(), t.len() + 2000);
                took
            })
            .min()
            .unwrap();
        let limit = if cfg!(debug_assertions) { 3000 } else { 50 };
        assert!(best.as_millis() < limit, "took {best:?}");
    }
}
