//! The machine's total CPU use, so the daemon grants only cores that processes outside Ignite
//! are not using.

use std::io;

#[cfg(target_os = "macos")]
mod macos;

pub trait HostCpu {
    /// Logical cores.
    fn cores(&self) -> u32;
    /// The fraction of all cores busy since the previous call, from 0 to 1.
    fn busy(&self) -> io::Result<f64>;
    /// Memory not in use, in megabytes.
    fn free_memory_mb(&self) -> io::Result<u64>;
}
