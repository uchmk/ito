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
mod matcher;

pub use matcher::Matcher;
