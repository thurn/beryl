//! The macOS half of `proc`.
//!
//! Stub. The ticket Thrh4.8 (macOS process, usage and socket layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;
use std::process::Command;

use super::{Group, Processes};
use crate::Native;

impl Processes for Native {
    fn spawn_group(&self, command: &mut Command) -> io::Result<Group> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }

    fn end_group(&self, group: &mut Group) -> io::Result<()> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }
}
