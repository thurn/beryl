//! Disk space for the volume that holds a path, and system facts that Windows reports through
//! system calls.

use std::io;
use std::path::Path;

#[cfg(not(windows))]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
pub use macos::disk_space;
#[cfg(target_os = "macos")]
pub use macos::{ApfsVolume, NativeVolumeSystem, VolumeSystem, mount_points};
#[cfg(windows)]
pub use windows::{disk_space, mount_points, total_memory_bytes};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskSpace {
    pub total: u64,
    /// Bytes the current user can still write.
    pub available: u64,
}

pub fn disk_free_bytes(path: &Path) -> io::Result<u64> {
    disk_space(path).map(|space| space.available)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn mount_points() -> Vec<std::path::PathBuf> {
    Vec::new()
}
