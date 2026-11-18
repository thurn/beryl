//! The local channel between the client and its daemon: a Unix socket. `name` is its path.

use std::io::{self, Read, Write};

#[cfg(target_os = "macos")]
mod macos;

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
