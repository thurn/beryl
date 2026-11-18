//! What a finished command cost: instructions retired, CPU seconds and peak memory of a child
//! and its descendants.

use std::io;
use std::process::{Child, ExitStatus};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// One process tree's resource use.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProcessUsage {
    pub cpu_s: f64,
    /// `None` where the operating system does not count instructions (Windows).
    pub instructions: Option<u64>,
    pub peak_bytes: u64,
}

pub trait UsageReader {
    /// The current use of a running process and its descendants, sampled about once a second
    /// while the command runs, because the operating system totals only a direct child exactly.
    fn sample(&self, pid: u32) -> io::Result<ProcessUsage>;

    /// Waits for `child` and returns its exit status with its final use.
    fn reap(&self, child: &mut Child) -> io::Result<(ExitStatus, ProcessUsage)>;
}
