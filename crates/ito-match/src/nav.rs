//! Stepping through a list of match ranges (`Matcher::ranges`, or the ranges of
//! `Subst::edits`) from the caret: the next / previous match, wrapping round.

use std::ops::Range;

/// Index of the first range starting at or after `byte`; wraps to 0 past the last.
/// `None` only when `ranges` is empty.
pub fn nth_after(ranges: &[Range<usize>], byte: usize) -> Option<usize> {
    if ranges.is_empty() {
        return None;
    }
    Some(ranges.partition_point(|r| r.start < byte) % ranges.len())
}

/// Index of the last range starting before `byte`; wraps to the last before the first.
pub fn nth_before(ranges: &[Range<usize>], byte: usize) -> Option<usize> {
    if ranges.is_empty() {
        return None;
    }
    let n = ranges.partition_point(|r| r.start < byte);
    Some(if n == 0 { ranges.len() - 1 } else { n - 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_wraps() {
        let r = [2..3, 5..6, 9..10];
        assert_eq!(nth_after(&r, 0), Some(0));
        assert_eq!(nth_after(&r, 5), Some(1));
        assert_eq!(nth_after(&r, 6), Some(2));
        assert_eq!(nth_after(&r, 10), Some(0));
        assert_eq!(nth_before(&r, 5), Some(0));
        assert_eq!(nth_before(&r, 2), Some(2));
        assert_eq!(nth_before(&r, 99), Some(2));
        assert_eq!(nth_after(&[], 0), None);
        assert_eq!(nth_before(&[], 0), None);
    }
}
