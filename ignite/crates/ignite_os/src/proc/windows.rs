//! The Windows half of `proc`.
//!
//! Stub. The ticket Thrh4.9 (Windows process, usage and pipe layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;
use std::process::Command;

use super::{Group, Processes};
use crate::Native;

impl Processes for Native {
    fn spawn_group(&self, command: &mut Command) -> io::Result<Group> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }

    fn end_group(&self, group: &mut Group) -> io::Result<()> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }
}
