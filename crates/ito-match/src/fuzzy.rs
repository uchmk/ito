//! A compact fzf-style scorer. Returns both a score and the matched character
//! positions so the UI can highlight them.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub score: i32,
    /// Character (not byte) offsets into the haystack.
    pub positions: Vec<usize>,
}

// Weights follow fzf's shape, with the consecutive bonus raised so that a
// tight run beats the same characters scattered across word boundaries
// (`main` should rank `main.rs` above `m_a_i_n.rs`).
const BONUS_BOUNDARY: i32 = 10;
const BONUS_CAMEL: i32 = 8;
const BONUS_CONSECUTIVE: i32 = 14;
const BONUS_FIRST: i32 = 20;
const BONUS_EXACT_CASE: i32 = 2;
const SCORE_MATCH: i32 = 16;
const PENALTY_GAP_START: i32 = -5;
const PENALTY_GAP_EXTEND: i32 = -2;

fn is_sep(c: char) -> bool {
    matches!(c, '/' | '\\' | '_' | '-' | '.' | ' ' | '(' | ')' | '[' | ']' | '@' | '#')
}

/// `smart` means: a pattern with any uppercase letter becomes case-sensitive.
pub fn is_case_sensitive(pattern: &str, smart: bool, forced_insensitive: bool) -> bool {
    if forced_insensitive {
        return false;
    }
    smart && pattern.chars().any(char::is_uppercase)
}

/// The name nearest to a mistyped `name`, by edit distance with a swap of two
/// neighbours costing one (`tpyo` is one step from `typo`). Case is ignored.
/// Nothing is returned when even the best is more than a third of the length
/// away (at least one step is always allowed), and ties go to the first.
pub fn closest<'a>(name: &str, names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let want: Vec<char> = name.to_lowercase().chars().collect();
    let limit = (want.len() / 3).max(1);
    names
        .filter_map(|n| {
            let have: Vec<char> = n.to_lowercase().chars().collect();
            let d = edit_distance(&want, &have);
            (d <= limit).then_some((d, n))
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n)
}

/// Damerau-Levenshtein distance (adjacent swaps count as one edit).
fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, v) in d[0].iter_mut().enumerate() {
        *v = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[a.len()][b.len()]
}

pub fn match_str(pattern: &str, text: &str, case_sensitive: bool) -> Option<Hit> {
    if pattern.is_empty() {
        return Some(Hit { score: 0, positions: Vec::new() });
    }
    let hay: Vec<char> = text.chars().collect();
    let pat: Vec<char> = pattern.chars().collect();
    if pat.len() > hay.len() {
        return None;
    }

    // Forward greedy pass: earliest possible match for each pattern char.
    let mut first = Vec::with_capacity(pat.len());
    let mut hi = 0;
    for &pc in &pat {
        let mut found = None;
        while hi < hay.len() {
            if eq(hay[hi], pc, case_sensitive) {
                found = Some(hi);
                hi += 1;
                break;
            }
            hi += 1;
        }
        first.push(found?);
    }

    // Backward pass: pull the match as late (and therefore as tight) as
    // possible, which is what makes trailing-word matches score well.
    let mut pos = first.clone();
    let mut limit = hay.len();
    for pi in (0..pat.len()).rev() {
        let mut j = limit.min(hay.len());
        let mut chosen = pos[pi];
        while j > pos[pi] {
            j -= 1;
            if eq(hay[j], pat[pi], case_sensitive) {
                chosen = j;
                break;
            }
        }
        pos[pi] = chosen;
        limit = chosen;
    }

    Some(Hit { score: score(&hay, &pat, &pos, case_sensitive), positions: pos })
}

fn eq(a: char, b: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        a == b
    } else {
        a == b || a.to_lowercase().eq(b.to_lowercase())
    }
}

fn score(hay: &[char], pat: &[char], pos: &[usize], case_sensitive: bool) -> i32 {
    let mut total = 0;
    let mut prev: Option<usize> = None;
    for (pi, &p) in pos.iter().enumerate() {
        total += SCORE_MATCH;
        let ch = hay[p];
        if p == 0 {
            total += BONUS_FIRST + BONUS_BOUNDARY;
        } else {
            let before = hay[p - 1];
            if is_sep(before) {
                total += BONUS_BOUNDARY;
            } else if before.is_lowercase() && ch.is_uppercase() {
                total += BONUS_CAMEL;
            }
        }
        if !case_sensitive && ch == pat[pi] {
            total += BONUS_EXACT_CASE;
        }
        match prev {
            Some(q) if p == q + 1 => total += BONUS_CONSECUTIVE,
            Some(q) => {
                let gap = (p - q - 1) as i32;
                total += PENALTY_GAP_START + PENALTY_GAP_EXTEND * (gap - 1).max(0);
            }
            None => {
                // Prefer matches that start early.
                total -= (p as i32).min(20);
            }
        }
        prev = Some(p);
    }
    // Shorter haystacks win ties, so exact names beat long ones.
    total - (hay.len() as i32) / 8
}

/// Plain substring search, used by `find` and by the filter when the query
/// looks literal. Returns the char positions of the first occurrence.
pub fn find_substring(needle: &str, text: &str, case_sensitive: bool) -> Option<Hit> {
    if needle.is_empty() {
        return Some(Hit { score: 0, positions: Vec::new() });
    }
    let hay: Vec<char> = text.chars().collect();
    let pat: Vec<char> = needle.chars().collect();
    if pat.len() > hay.len() {
        return None;
    }
    for start in 0..=(hay.len() - pat.len()) {
        if (0..pat.len()).all(|i| eq(hay[start + i], pat[i], case_sensitive)) {
            return Some(Hit {
                score: 1000 - start as i32,
                positions: (start..start + pat.len()).collect(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_name_forgives_a_swap_and_a_typo() {
        let names = ["alpha", "Typo", "zeta"];
        assert_eq!(closest("tpyo", names.into_iter()), Some("Typo"));
        assert_eq!(closest("alpa", names.into_iter()), Some("alpha"));
        assert_eq!(closest("qqqq", names.into_iter()), None);
    }

    #[test]
    fn matches_and_ranks() {
        let a = match_str("mn", "main.rs", false).unwrap();
        assert_eq!(a.positions.len(), 2);
        assert!(match_str("zzz", "main.rs", false).is_none());

        let exact = match_str("main", "main.rs", false).unwrap().score;
        let scattered = match_str("main", "m_a_i_n.rs", false).unwrap().score;
        assert!(exact > scattered, "{exact} !> {scattered}");
    }

    #[test]
    fn smart_case() {
        assert!(is_case_sensitive("Main", true, false));
        assert!(!is_case_sensitive("main", true, false));
        assert!(match_str("Main", "main.rs", true).is_none());
    }

    #[test]
    fn substring() {
        let h = find_substring("ain", "main.rs", false).unwrap();
        assert_eq!(h.positions, vec![1, 2, 3]);
    }
}
