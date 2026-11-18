//! The Windows half of `usage`.
//!
//! Stub. The ticket Thrh4.9 (Windows process, usage and pipe layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;
use std::process::{Child, ExitStatus};

use super::{ProcessUsage, UsageReader};
use crate::Native;

impl UsageReader for Native {
    fn sample(&self, pid: u32) -> io::Result<ProcessUsage> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }

    fn reap(&self, child: &mut Child) -> io::Result<(ExitStatus, ProcessUsage)> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }
}
