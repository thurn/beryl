//! The machine's total CPU use, so the daemon grants only cores that processes outside Ignite
//! are not using.

use std::io;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::HostMeter;
#[cfg(windows)]
pub use windows::HostMeter;

/// How long [`Native`](crate::Native) waits between its two readings in [`HostCpu::busy`],
/// which keeps no earlier reading to compare against.
#[cfg(any(target_os = "macos", windows))]
const NATIVE_WINDOW: std::time::Duration = std::time::Duration::from_millis(100);

/// The error for a reading with nothing to compare against: the first one of a [`HostMeter`],
/// or one taken before the operating system's counters moved. Callers keep their previous
/// reading, as they do for any failed one.
#[cfg(any(target_os = "macos", windows))]
fn no_reading() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "no CPU time has been counted since the previous reading",
    )
}

pub trait HostCpu {
    /// Logical cores.
    fn cores(&self) -> u32;
    /// Physical cores, falling back to the logical count when topology is unavailable.
    fn physical_cores(&self) -> u32 {
        self.cores()
    }
    /// The fraction of all cores busy since the previous call, from 0 to 1. An error means
    /// there is no reading: it is the first call, or the counters did not move since the
    /// previous one, so an answer of "0" would be a guess. Only one thread may sample a
    /// meter, because two samples microseconds apart each see almost no ticks.
    fn busy(&self) -> io::Result<f64>;
    /// Memory not in use, in megabytes.
    fn free_memory_mb(&self) -> io::Result<u64>;
    /// Whether the OS currently reports warning or critical memory pressure.
    fn active_memory_pressure(&self) -> io::Result<bool> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    /// Swap in use, in megabytes. Unsupported unless an implementation reads it.
    fn swap_used_mb(&self) -> io::Result<u64> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    /// The CPU seconds used so far by the process group `group` and every process its members
    /// started, including processes that left the group, and the children they reaped. A
    /// process that exits unreaped by the tree takes its time with it, so the total can fall.
    /// Unsupported unless an implementation reads it.
    fn tree_cpu_seconds(&self, group: u64) -> io::Result<f64> {
        let _ = group;
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    /// The CPU seconds used so far by process `pid` and every process it started, through any
    /// number of generations, each counted with the children it reaped. As with
    /// [`Self::tree_cpu_seconds`], a process that exits unreaped by the tree takes its time with
    /// it. Unsupported unless an implementation reads it.
    fn process_tree_cpu_seconds(&self, pid: u32) -> io::Result<f64> {
        let _ = pid;
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    /// [`Self::process_tree_cpu_seconds`] for each of `roots`, as `(root, CPU seconds)`, with
    /// every process counted once, under the first root whose tree holds it: a root inside an
    /// earlier root's tree reads as 0. A root that no longer runs is left out. An error means the
    /// first root could not be read.
    ///
    /// The default reads each root on its own, so it counts a process twice when trees overlap;
    /// an implementation whose roots can overlap overrides it.
    fn process_trees_cpu_seconds(&self, roots: &[u32]) -> io::Result<Vec<(u32, f64)>> {
        let mut readings = Vec::with_capacity(roots.len());
        for (index, &root) in roots.iter().enumerate() {
            match self.process_tree_cpu_seconds(root) {
                Ok(cpu_s) => readings.push((root, cpu_s)),
                Err(error) if index == 0 => return Err(error),
                Err(_) => {}
            }
        }
        Ok(readings)
    }
}
