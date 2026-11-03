use std::ops::Range;

use crate::{line_of, lines, Change, Dir};

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

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
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
    let it = item(&text[ls[li].clone()])?;
    if !deeper && it.indent == 0 {
        return None;
    }
    let unit = if it.number.is_some() { it.marker } else { 2 };
    let n = if deeper { unit } else { unit.min(it.indent) };
    let mut last = li;
    for (k, l) in ls.iter().enumerate().skip(li + 1) {
        let line = &text[l.clone()];
        if blank(line) {
            continue;
        }
        if indent_of(line) > it.indent {
            last = k;
        } else {
            break;
        }
    }
    let mut new = String::new();
    let mut cursor = byte;
    for k in li..=last {
        let line = &text[ls[k].clone()];
        let (add, cut) = if blank(line) {
            (0, 0)
        } else if deeper {
            (n, 0)
        } else {
            (0, n.min(indent_of(line)))
        };
        new.push_str(&" ".repeat(add));
        new.push_str(&line[cut..]);
        if k == li {
            let col = byte - ls[li].start;
            cursor = ls[li].start + (col + add).saturating_sub(cut);
        }
        if k < last {
            new.push_str(&text[ls[k].end..ls[k + 1].start]);
        }
    }
    Some(Change { range: ls[li].start..ls[last].end, text: new, cursor })
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
fn owner(text: &str, ls: &[Range<usize>], li: usize) -> Option<usize> {
    let line = &text[ls[li].clone()];
    if item_ws(line).is_some() {
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
            if item_ws(l).is_some() {
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
    let li = owner(text, &ls, line_of(&ls, byte)?)?;
    let (it, _) = item_ws(&text[ls[li].clone()])?;
    let n = it.indent;
    let ordered = it.number.is_some();
    let me = li..=extent(text, &ls, li, n);
    let same = |k: usize| item_ws(&text[ls[k].clone()]).filter(|(i, _)| i.indent == n && i.number.is_some() == ordered);
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
    let it = item(&text[ls[li].clone()])?;
    let at = ls[li].start + it.indent + it.marker;
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
    let it = item(&text[ls[li].clone()])?;
    it.number?;
    let n = it.indent;
    let belongs = |k: usize| -> bool {
        let line = &text[ls[k].clone()];
        if blank(line) {
            return true;
        }
        let ind = indent_of(line);
        if ind < n {
            return false;
        }
        if ind == n {
            return item(line).is_some_and(|i| i.number.is_some());
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
    let mut counter: Option<u64> = None;
    let mut new = String::new();
    let mut cursor = byte;
    let mut shift: isize = 0;
    for k in s..=e {
        let line = &text[ls[k].clone()];
        match item(line).filter(|i| i.indent == n && i.number.is_some()) {
            Some(i) => {
                let c = *counter.get_or_insert(i.number.unwrap_or(1));
                counter = Some(c + 1);
                let after = n + i.digits;
                let fresh = format!("{}{}{}", &line[..n], c, &line[after..]);
                let diff = fresh.len() as isize - line.len() as isize;
                if k == li {
                    let past = byte - ls[k].start >= after;
                    cursor = (byte as isize + shift + if past { diff } else { 0 }) as usize;
                } else if k < li {
                    shift += diff;
                }
                new.push_str(&fresh);
            }
            None => new.push_str(line),
        }
        if k < e {
            new.push_str(&text[ls[k].end..ls[k + 1].start]);
        }
    }
    let range = ls[s].start..ls[e].end;
    if new == text[range.clone()] {
        return None;
    }
    Some(Change { range, text: new, cursor })
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
        assert_eq!(indent_item(t, 5).unwrap().apply(t), "1. a\n   2. b\n");
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
        let t = "\t- a\n    - b\n";
        assert_eq!(mv(t, 0, Dir::Down).unwrap().0, "    - b\n\t- a\n");
    }
}
