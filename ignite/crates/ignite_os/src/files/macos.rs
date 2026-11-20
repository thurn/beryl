//! The macOS half of `files`: `getrlimit` and `setrlimit` on `RLIMIT_NOFILE`.

use std::io;

use libc::{RLIM_INFINITY, RLIMIT_NOFILE, getrlimit, rlim_t, rlimit, setrlimit};

/// `OPEN_MAX`, the highest soft limit macOS accepts whatever `kern.maxfilesperproc` says on a
/// machine with a smaller setting; it is the last value tried.
const OPEN_MAX: u64 = 10_240;

fn limits() -> io::Result<rlimit> {
    let mut current = rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `current` is a valid, writable `rlimit` for the duration of the call.
    if unsafe { getrlimit(RLIMIT_NOFILE, &raw mut current) } == 0 {
        Ok(current)
    } else {
        Err(io::Error::last_os_error())
    }
}

pub fn open_file_limit() -> io::Result<u64> {
    Ok(limits()?.rlim_cur)
}

pub fn raise_open_file_limit(wanted: u64) -> io::Result<u64> {
    let current = limits()?;
    let ceiling = if current.rlim_max == RLIM_INFINITY {
        wanted
    } else {
        wanted.min(current.rlim_max)
    };
    if current.rlim_cur >= ceiling {
        return Ok(current.rlim_cur);
    }
    let mut last = io::Error::other("no limit tried");
    for target in [ceiling, ceiling.min(OPEN_MAX)] {
        let request = rlimit {
            rlim_cur: target as rlim_t,
            rlim_max: current.rlim_max,
        };
        // SAFETY: `request` is a valid `rlimit`, and the call only reads it.
        if unsafe { setrlimit(RLIMIT_NOFILE, &raw const request) } == 0 {
            return open_file_limit();
        }
        last = io::Error::last_os_error();
    }
    Err(last)
}
