//! The Windows half of `ipc`.
//!
//! Stub. The ticket Thrh4.9 (Windows process, usage and pipe layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;

use super::{Ipc, Listener, Stream};
use crate::Native;

impl Ipc for Native {
    fn listen(&self, name: &str) -> io::Result<Box<dyn Listener>> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }

    fn connect(&self, name: &str) -> io::Result<Box<dyn Stream>> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }
}
