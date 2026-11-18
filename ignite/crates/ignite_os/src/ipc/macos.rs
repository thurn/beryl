//! The macOS half of `ipc`.
//!
//! Stub. The ticket Thrh4.8 (macOS process, usage and socket layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;

use super::{Ipc, Listener, Stream};
use crate::Native;

impl Ipc for Native {
    fn listen(&self, name: &str) -> io::Result<Box<dyn Listener>> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }

    fn connect(&self, name: &str) -> io::Result<Box<dyn Stream>> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }
}
