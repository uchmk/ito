use crate::{line_of, lines, Change};

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
    move_item(text, byte, true)
}

/// Take the list item on the line of `byte` and its nested lines out by one level. `None` at the left edge.
pub fn outdent_item(text: &str, byte: usize) -> Option<Change> {
    move_item(text, byte, false)
}

fn move_item(text: &str, byte: usize, deeper: bool) -> Option<Change> {
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
}
