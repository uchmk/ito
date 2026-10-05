//! Everything that differs by OS, behind one set of names. The rest of the
//! crate calls `sys::children` and the like and never says `cfg` itself.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

/// Neither: no process table to read.
#[cfg(not(any(windows, unix)))]
pub fn children(_pid: u32) -> Vec<u32> {
    Vec::new()
}
