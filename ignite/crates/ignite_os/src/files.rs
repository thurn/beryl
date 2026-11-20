//! The number of files a process may hold open at once.
//!
//! A daemon that launchd starts at login inherits launchd's soft limit of 256 descriptors,
//! where a shell allows about a million. A daemon that starts several builds at once, each with
//! pipes, log files and a lock, passes 256, and every later open fails with `EMFILE` ("Too many
//! open files"). [`raise_open_file_limit`] lifts the soft limit toward the hard one when the
//! process starts, so that the processes it spawns inherit the higher limit too.
//!
//! Windows has no such per-process limit, so there the call does nothing.

use std::io;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// The soft limit the daemon asks for when the hard limit allows it. It is far above what a
/// burst of builds needs, and macOS refuses a soft limit above `kern.maxfilesperproc`, so
/// [`raise_open_file_limit`] falls back to lower values when this one is refused.
pub const WANTED_OPEN_FILES: u64 = 65_536;

/// Raises this process's soft open-file limit to `wanted`, or to the hard limit when that is
/// lower, and returns the soft limit in force afterwards. A soft limit that is already higher
/// is left alone.
///
/// # Errors
/// The limits cannot be read, or none of the values tried could be set.
pub fn raise_open_file_limit(wanted: u64) -> io::Result<u64> {
    #[cfg(target_os = "macos")]
    return macos::raise_open_file_limit(wanted);
    #[cfg(windows)]
    return windows::raise_open_file_limit(wanted);
}

/// This process's soft open-file limit right now.
///
/// # Errors
/// The limit cannot be read.
pub fn open_file_limit() -> io::Result<u64> {
    #[cfg(target_os = "macos")]
    return macos::open_file_limit();
    #[cfg(windows)]
    return windows::open_file_limit();
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// A daemon started with launchd's soft limit has at least the wanted limit after the call
    /// (or the hard limit, when that is lower).
    #[test]
    fn the_soft_limit_is_raised_to_at_least_what_was_wanted() {
        let before = open_file_limit().unwrap();
        let after = raise_open_file_limit(WANTED_OPEN_FILES).unwrap();
        assert!(after >= before);
        assert_eq!(after, open_file_limit().unwrap());
        assert!(after >= WANTED_OPEN_FILES.min(10_240), "{after}");
    }
}
