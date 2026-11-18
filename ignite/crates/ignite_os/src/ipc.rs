//! The local channel between the client and its daemon: a Unix socket on macOS, a named pipe on
//! Windows. `name` is a path on macOS and a pipe name on Windows.

use std::io::{self, Read, Write};
use std::time::Duration;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// One connection.
pub trait Stream: Read + Write + Send {
    /// Makes a read that waits longer than `limit` fail with `WouldBlock` or `TimedOut`, or
    /// removes the limit with `None`. A stream that cannot time out does nothing.
    fn set_read_timeout(&self, limit: Option<Duration>) -> io::Result<()> {
        let _ = limit;
        Ok(())
    }

    /// A check, usable from any thread, of whether the other end has closed the connection
    /// without a byte of its request being left unread. `None` where the platform cannot tell.
    fn closed_probe(&self) -> Option<Box<dyn Fn() -> bool + Send + Sync>> {
        None
    }
}

/// A listening endpoint.
pub trait Listener: Send {
    fn accept(&self) -> io::Result<Box<dyn Stream>>;
}

pub trait Ipc {
    fn listen(&self, name: &str) -> io::Result<Box<dyn Listener>>;
    fn connect(&self, name: &str) -> io::Result<Box<dyn Stream>>;
}
