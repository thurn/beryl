//! The Windows half of `hostcpu`.
//!
//! Stub. The ticket Thrh4.9 (Windows process, usage and pipe layer) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::io;

use super::HostCpu;
use crate::Native;

impl HostCpu for Native {
    fn cores(&self) -> u32 {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }

    fn busy(&self) -> io::Result<f64> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        todo!("Thrh4.9 (Windows process, usage and pipe layer)")
    }
}
