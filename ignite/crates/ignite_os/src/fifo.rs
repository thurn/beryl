//! Named pipes (FIFOs), which a jobserver in the GNU make protocol is made of: the supervisor of
//! a command keeps one byte in the pipe for each job slot its Cargo builds may use beyond the
//! first.
//! Windows has no FIFOs, so there a build keeps the size it started with.

use std::fs::File;
use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// Whether this system has FIFOs, so that a supervisor can keep a jobserver.
pub const AVAILABLE: bool = !cfg!(windows);

pub trait Fifos {
    /// Makes a FIFO at `path`, readable and writable by this user only, and opens it for both
    /// reading and writing without blocking: a write never waits for a reader, and a read of an
    /// empty pipe fails with [`io::ErrorKind::WouldBlock`] instead of waiting.
    fn open_fifo(&self, path: &Path) -> io::Result<File>;
}
