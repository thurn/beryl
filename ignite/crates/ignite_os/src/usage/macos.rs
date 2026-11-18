//! The macOS half of `usage`.
//!
//! Stub. The ticket Thrh4.8 (macOS process, usage and socket layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;
use std::process::{Child, ExitStatus};

use super::{ProcessUsage, UsageReader};
use crate::Native;

impl UsageReader for Native {
    fn sample(&self, pid: u32) -> io::Result<ProcessUsage> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }

    fn reap(&self, child: &mut Child) -> io::Result<(ExitStatus, ProcessUsage)> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }
}
