//! Small text helpers the apps each had a copy of: natural ordering for names,
//! byte counts and waiting times as the panels show them.

use std::cmp::Ordering;
use std::time::Duration;

/// Natural ("version") comparison: `a2` < `a10`, and digits never sort as text.
/// `01` and `1` compare by value first, then by written length, so the order
/// stays stable.
pub fn natural_cmp(a: &str, b: &str, case_sensitive: bool) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    let an = take_number(&mut ai);
                    let bn = take_number(&mut bi);
                    let (at, bt) = (an.trim_start_matches('0'), bn.trim_start_matches('0'));
                    let o = at.len().cmp(&bt.len()).then_with(|| at.cmp(bt)).then_with(|| an.len().cmp(&bn.len()));
                    if o != Ordering::Equal {
                        return o;
                    }
                } else {
                    ai.next();
                    bi.next();
                    let (x, y) = if case_sensitive { (ac, bc) } else { (lower(ac), lower(bc)) };
                    if x != y {
                        return x.cmp(&y);
                    }
                }
            }
        }
    }
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn take_number(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.next_if(char::is_ascii_digit) {
        s.push(c);
    }
    s
}

/// Case-insensitive-first alphabetical comparison, falling back to the
/// case-sensitive form so distinct names never compare equal.
pub fn alpha_cmp(a: &str, b: &str, case_sensitive: bool) -> Ordering {
    if case_sensitive {
        return a.cmp(b);
    }
    a.chars().map(lower).cmp(b.chars().map(lower)).then_with(|| a.cmp(b))
}

/// Bytes with IEC units, as a panel shows them: `512 B`, `3.4 KiB`, `15.2 GiB`
/// (a value of 100 or more drops the decimal).
pub fn bytes_iec(b: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if b < 1024 {
        return format!("{b} B");
    }
    let mut v = b as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[unit])
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// Bytes in the file manager's short form: `999 B`, `1.0 K`, `20 M` (units of
/// 1024, one letter, a decimal only under 10).
pub fn bytes_short(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "K", "M", "G", "T", "P", "E"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    if v < 10.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.0} {}", UNITS[i])
    }
}

/// A waiting time in the shortest form that still reads: `45s`, `3m10s`,
/// `2h05m`; fit for a status bar.
pub fn duration_short(d: Duration) -> String {
    let secs = d.as_secs();
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m{:02}s", secs / 60, secs % 60),
        _ => format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// An uptime in seconds as `3d 4h`, `4h 12m`, `12m 5s`.
pub fn uptime(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m {s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["a10", "a2", "a1", "b1", "A3"];
        v.sort_by(|a, b| natural_cmp(a, b, false));
        assert_eq!(v, ["a1", "a2", "A3", "a10", "b1"]);
        assert_eq!(natural_cmp("a01", "a1", false), Ordering::Greater);
        assert_eq!(natural_cmp("a", "A", true), Ordering::Greater);
    }

    #[test]
    fn alpha_never_calls_distinct_names_equal() {
        assert_eq!(alpha_cmp("a", "A", false), Ordering::Greater);
        assert_eq!(alpha_cmp("a", "b", false), Ordering::Less);
        assert_eq!(alpha_cmp("ab", "a", false), Ordering::Greater);
        assert_eq!(alpha_cmp("a", "a", false), Ordering::Equal);
    }

    #[test]
    fn sizes() {
        assert_eq!(bytes_iec(512), "512 B");
        assert_eq!(bytes_iec(3 * 1024 + 410), "3.4 KiB");
        assert_eq!(bytes_iec(150 * 1024 * 1024), "150 MiB");
        assert_eq!(bytes_short(999), "999 B");
        assert_eq!(bytes_short(1024), "1.0 K");
        assert_eq!(bytes_short(1024 * 1024 * 20), "20 M");
    }

    #[test]
    fn durations_read_at_a_glance() {
        assert_eq!(duration_short(Duration::from_secs(45)), "45s");
        assert_eq!(duration_short(Duration::from_secs(60)), "1m00s");
        assert_eq!(duration_short(Duration::from_secs(3599)), "59m59s");
        assert_eq!(duration_short(Duration::from_secs(7_500)), "2h05m");
        assert_eq!(uptime(90_000), "1d 1h");
        assert_eq!(uptime(4_320), "1h 12m");
        assert_eq!(uptime(125), "2m 5s");
    }
}
