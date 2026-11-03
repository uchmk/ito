use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

use crate::{change_from_edits, line_of, lines, Change, Dir};

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

/// Indent the list item on the line of `byte` with its nested lines by one level (2 spaces under a bullet, the width
/// of the marker under a number). The numbers are not touched: see [`renumber`].
pub fn indent_item(text: &str, byte: usize) -> Option<Change> {
    shift_item(text, byte, true)
}

/// Take the list item on the line of `byte` and its nested lines out by one level. `None` at the left edge.
pub fn outdent_item(text: &str, byte: usize) -> Option<Change> {
    shift_item(text, byte, false)
}

fn shift_item(text: &str, byte: usize, deeper: bool) -> Option<Change> {
    let byte = byte.min(text.len());
    let ls = lines(text);
    let li = line_of(&ls, byte)?;
    if code_lines(text, &ls)[li] {
        return None;
    }
    let (it, lead_n) = item_ws(&text[ls[li].clone()])?;
    let w = it.indent;
    if !deeper && w == 0 {
        return None;
    }
    let tabbed = text[ls[li].start..ls[li].start + lead_n].contains('\t');
    let unit = if deeper || !tabbed {
        if it.number.is_some() {
            it.marker
        } else {
            2
        }
    } else {
        // An item indented with a tab goes out to the previous tab stop.
        w - (w - 1) / 4 * 4
    };
    let n = if deeper { unit } else { unit.min(w) };
    let last = extent(text, &ls, li, w);
    let mut new = String::new();
    let mut cursor = byte;
    for k in li..=last {
        let line = &text[ls[k].clone()];
        let (lw, ln) = lead(line);
        let nw = if blank(line) {
            lw
        } else if deeper {
            lw + n
        } else {
            lw - n.min(lw)
        };
        // The blanks are written as spaces, so a tab-indented line has the width it is read with.
        let blanks = if blank(line) { line[..ln].to_string() } else { " ".repeat(nw) };
        new.push_str(&blanks);
        new.push_str(&line[ln..]);
        if k == li {
            let col = byte - ls[li].start;
            cursor = ls[li].start + if col >= ln { blanks.len() + col - ln } else { col.min(blanks.len()) };
        }
        if k < last {
            new.push_str(&text[ls[k].end..ls[k + 1].start]);
        }
    }
    let shifted = Change { range: ls[li].start..ls[last].end, text: new, cursor };
    Some(fix_numbers(text, shifted, li, last, w))
}

/// After an item moved to another indent, number the ordered lists it touches again: the list it joined (a new
/// sublist starts at 1: a nested list that starts at another number would not be a list) and the one it left.
fn fix_numbers(text: &str, shifted: Change, li: usize, last: usize, old_w: usize) -> Change {
    let mut cur = shifted.apply(text);
    let mut total = shifted;
    let ls = lines(&cur);
    let ordered_at = |k: usize| -> bool { item_ws(&cur[ls[k].clone()]).is_some_and(|(i, _)| i.indent == old_w && i.number.is_some()) };
    let before = (0..li).rev().find(|&k| !blank(&cur[ls[k].clone()]) && lead(&cur[ls[k].clone()]).0 <= old_w).filter(|&k| ordered_at(k));
    let after = (last + 1..ls.len()).find(|&k| !blank(&cur[ls[k].clone()])).filter(|&k| ordered_at(k));
    for target in [Some(li), before, after].into_iter().flatten() {
        let ls = lines(&cur);
        let code = code_lines(&cur, &ls);
        let edits = renumber_edits(&cur, &ls, &code, target, true);
        let Some(b) = change_from_edits(&cur, &edits, total.cursor) else { continue };
        let next = b.apply(&cur);
        total = compose(text, total, &cur, b);
        cur = next;
    }
    total
}

/// One change from the original `text` that does what `a` (on `text`) and then `b` (on the text `a` makes, `mid`) do.
fn compose(text: &str, a: Change, mid: &str, b: Change) -> Change {
    let a_new_end = a.range.start + a.text.len();
    let map_end = |x: usize| {
        if x <= a.range.start {
            x
        } else if x >= a_new_end {
            x - a.text.len() + a.range.len()
        } else {
            a.range.end
        }
    };
    let start = a.range.start.min(b.range.start);
    let old_end = a.range.end.max(map_end(b.range.end));
    let done = b.apply(mid);
    let new_end = done.len() - (text.len() - old_end);
    Change { range: start..old_end, text: done[start..new_end].to_string(), cursor: b.cursor }
}

/// For each line, whether it is inside a code block (fenced or indented). The line of an item that holds a fence
/// after its marker is not code.
pub(crate) fn code_lines(text: &str, ls: &[Range<usize>]) -> Vec<bool> {
    let blocks: Vec<Range<usize>> =
        Parser::new(text).into_offset_iter().filter_map(|(e, r)| matches!(e, Event::Start(Tag::CodeBlock(_))).then_some(r)).collect();
    ls.iter()
        .map(|l| {
            let at = l.start + lead(&text[l.clone()]).1;
            blocks.iter().any(|b| b.start <= at && at < b.end)
        })
        .collect()
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
    let edits = renumber_edits(text, &ls, &code, li, false);
    change_from_edits(text, &edits, byte)
}

/// The edits (the digits of each number that is wrong) that number the list holding the ordered item on line `li`.
/// With `nested_from_one`, a list that is indented and follows a line of text starts at 1.
fn renumber_edits(text: &str, ls: &[Range<usize>], code: &[bool], li: usize, nested_from_one: bool) -> Vec<(Range<usize>, String)> {
    let Some((it, _)) = item_ws(&text[ls[li].clone()]) else { return Vec::new() };
    if code[li] || it.number.is_none() {
        return Vec::new();
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
    edits
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
}
