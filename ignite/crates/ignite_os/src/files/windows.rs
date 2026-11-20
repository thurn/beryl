//! The Windows half of `files`: the C runtime's open-file cap is not a per-process limit that
//! Ignite can lift or that its builds reach, so nothing is raised.

use std::io;

#[allow(clippy::unnecessary_wraps)] // Same signature as the macOS half.
pub fn open_file_limit() -> io::Result<u64> {
    Ok(u64::MAX)
}

pub fn raise_open_file_limit(_wanted: u64) -> io::Result<u64> {
    open_file_limit()
}
