//! The local channel between the client and its daemon: a Unix socket on macOS, a named pipe on
//! Windows. `name` is a path on macOS and a pipe name on Windows.

use std::io::{self, Read, Write};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// One connection.
pub trait Stream: Read + Write + Send {}

/// A listening endpoint.
pub trait Listener: Send {
    fn accept(&self) -> io::Result<Box<dyn Stream>>;
}

pub trait Ipc {
    fn listen(&self, name: &str) -> io::Result<Box<dyn Listener>>;
    fn connect(&self, name: &str) -> io::Result<Box<dyn Stream>>;
}
