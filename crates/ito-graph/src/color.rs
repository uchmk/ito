//! A colour for each branch, as an index into the terminal's sixteen so it
//! follows the theme. A branch keeps its colour: the trunk's is pinned, the
//! branches that matter most (a worktree's) are given different ones first,
//! and any other takes one from a hash of its name, moved on the first time
//! only if the lane beside it has the same.

use std::collections::HashMap;

/// The colours a branch can take: none of red (1, 9), which means a
/// conflict, or the greys (0, 7, 8, 15), which mean nothing.
pub const ALLOWED: [u8; 10] = [2, 3, 4, 5, 6, 10, 11, 12, 13, 14];

/// The colours handed out so far, by branch name.
#[derive(Clone, Debug)]
pub struct Colors {
    known: HashMap<String, u8>,
    /// What `pin` and `reserve` gave out: `reserve` skips them.
    taken: Vec<u8>,
    /// Where a name's hash points.
    pool: Vec<u8>,
    /// What `reserve` gives out, in order.
    first: Vec<u8>,
}

impl Default for Colors {
    /// The blues, magentas and greens to hash into, and yellow, magenta and
    /// green first for the branches that matter, so the first few never
    /// share a colour with a hashed one.
    fn default() -> Self {
        Self::new(&[4, 12, 13, 10, 11], &[3, 5, 2, 13, 4, 10, 11, 12])
    }
}

impl Colors {
    /// Colours from `pool` by a name's hash, and from `first` for `reserve`.
    /// `pool` must not be empty.
    pub fn new(pool: &[u8], first: &[u8]) -> Self {
        assert!(!pool.is_empty(), "a colour pool must have a colour");
        Self { known: HashMap::new(), taken: Vec::new(), pool: pool.to_vec(), first: first.to_vec() }
    }

    /// `name` always takes `color` (main: cyan, 6).
    pub fn pin(&mut self, name: &str, color: u8) {
        self.known.insert(name.to_string(), color);
        self.taken.push(color);
    }

    /// Gives each of `names` a colour that no pinned or reserved name has,
    /// in `first`'s order (a worktree's branches); a name with a colour keeps
    /// it, and when `first` runs out the rest hash like any other.
    pub fn reserve<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        for name in names {
            if self.known.contains_key(name) {
                continue;
            }
            let c = self.first.iter().copied().find(|c| !self.taken.contains(c)).unwrap_or_else(|| self.hashed(name));
            self.known.insert(name.to_string(), c);
            self.taken.push(c);
        }
    }

    /// The colour `name` has, or would take, without keeping it.
    pub fn of(&self, name: &str) -> u8 {
        self.known.get(name).copied().unwrap_or_else(|| self.hashed(name))
    }

    /// The colour of a row on `name`: the one the name has, or, the first
    /// time it is seen, its hash's, moved on along the pool past any of
    /// `avoid` (the lanes beside it), and kept from then on. A row with no
    /// name takes the pool's first colour not in `avoid`, and nothing is kept.
    pub fn place(&mut self, name: Option<&str>, avoid: &[u8]) -> u8 {
        let Some(name) = name else {
            return self.pool.iter().copied().find(|c| !avoid.contains(c)).unwrap_or(self.pool[0]);
        };
        if let Some(&c) = self.known.get(name) {
            return c;
        }
        let n = self.pool.len();
        let at = hash(name) % n;
        let c = (0..n).map(|k| self.pool[(at + k) % n]).find(|c| !avoid.contains(c)).unwrap_or(self.pool[at]);
        self.known.insert(name.to_string(), c);
        c
    }

    fn hashed(&self, name: &str) -> u8 {
        self.pool[hash(name) % self.pool.len()]
    }
}

/// The hash michi's design mock used (`h * 31 + c` over UTF-16), so a
/// branch takes the colour it had there.
fn hash(name: &str) -> usize {
    let mut h: u32 = 0;
    for u in name.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as u32);
    }
    h as usize
}
