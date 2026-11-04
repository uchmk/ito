//! How a search query matches text, for filer's `f` `s` `S` `/` `?` `F` and
//! anything else in the family that searches: one rule, so a query means the
//! same thing wherever it is typed.
//!
//! - [`Matcher::new`]: a regular expression, smart case (no capital in the
//!   query ignores case).
//! - [`Matcher::fuzzy`]: the letters in order, scored (`fuzzy::match_str`).
//!
//! Both say where they matched ([`Matcher::ranges`], [`Matcher::positions`]),
//! which is what a view draws highlighted.

pub mod fuzzy;
mod glob;
mod matcher;
mod nav;
mod subst;

pub use glob::{glob_match, has_wildcard};
pub use matcher::Matcher;
pub use nav::{nth_after, nth_before};
pub use subst::{Query, Subst};
