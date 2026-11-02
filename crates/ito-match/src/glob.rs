//! Wildcards for bare file names: `*` (any run, also empty) and `?` (one character).

/// Whether `name` (a bare file name) has a wildcard.
pub fn has_wildcard(name: &str) -> bool {
    name.contains(['*', '?'])
}

/// `*` (any run, also empty) and `?` (one character) against a bare file name.
pub fn glob_match(pattern: &str, name: &str, ignore_case: bool) -> bool {
    let fold = |s: &str| -> Vec<char> {
        s.chars().flat_map(|c| if ignore_case { c.to_lowercase().collect::<Vec<_>>() } else { vec![c] }).collect()
    };
    let (p, n) = (fold(pattern), fold(name));
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(glob_match("*.log", "a.log", false));
        assert!(glob_match("*.log", ".log", false));
        assert!(!glob_match("*.log", "a.log.part", false));
        assert!(glob_match("a?c", "abc", false));
        assert!(!glob_match("a?c", "ac", false));
        assert!(glob_match("*a*b*", "xxaybz", false));
        assert!(!glob_match("A*", "abc", false));
        assert!(glob_match("A*", "abc", true));
        assert!(glob_match("*", "", false));
        assert!(has_wildcard("a?") && !has_wildcard("a.txt"));
    }
}
