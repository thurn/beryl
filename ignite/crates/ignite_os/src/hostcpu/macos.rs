//! The macOS half of `hostcpu`.
//!
//! Stub. The ticket Thrh4.8 (macOS process, usage and socket layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;

use super::HostCpu;
use crate::Native;

impl HostCpu for Native {
    fn cores(&self) -> u32 {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }

    fn busy(&self) -> io::Result<f64> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        todo!("Thrh4.8 (macOS process, usage and socket layer)")
    }
}
