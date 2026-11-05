use std::ops::Range;

/// The front matter block at the very top of `text`: from byte 0 (or after a BOM) through the line break after the
/// closing `---` or `...` line. A first line of `---` (trailing spaces allowed) opens it; without a closing line there
/// is no front matter (`None`), so a lone rule at the top stays a rule. The range includes the BOM.
pub fn front_matter(text: &str) -> Option<Range<usize>> {
    let bom = if text.starts_with('\u{feff}') { '\u{feff}'.len_utf8() } else { 0 };
    let mut at = bom;
    let mut first = true;
    for line in text[bom..].split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        if first {
            if body.trim_end() != "---" {
                return None;
            }
            first = false;
        } else if matches!(body.trim_end(), "---" | "...") {
            return Some(0..at + line.len());
        }
        at += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_block_at_the_top_is_front_matter() {
        let t = "---\ntitle: x\n# not a heading\n---\n# Real\n";
        assert_eq!(front_matter(t), Some(0..t.find("# Real").unwrap()));
        let t = "---\r\ntags: [a]\r\n...\r\nbody";
        assert_eq!(&t[front_matter(t).unwrap()], "---\r\ntags: [a]\r\n...\r\n");
        let t = "\u{feff}---\na: 1\n---";
        assert_eq!(front_matter(t), Some(0..t.len()));
    }

    #[test]
    fn otherwise_there_is_none() {
        assert_eq!(front_matter("---\nno close\n"), None);
        assert_eq!(front_matter("\n---\na\n---\n"), None);
        assert_eq!(front_matter("# T\n---\na\n---\n"), None);
        assert_eq!(front_matter("----\na\n---\n"), None);
        assert_eq!(front_matter(""), None);
    }

    #[test]
    fn list_commands_do_not_read_items_in_it() {
        let t = "---
steps: |
  1. a
  3. b
---
1. x
3. y
";
        assert!(crate::renumber(t, 18).is_none(), "an item-looking line in the front matter is not a list");
        let c = crate::renumber(t, t.find("1. x").unwrap()).unwrap();
        assert!(
            c.apply(t).ends_with(
                "1. x
2. y
"
            ) && c.apply(t).starts_with(
                "---
steps: |
  1. a
  3. b
---
"
            )
        );
    }
}
