//! Facts about the machine that macOS gives through commands (`hostname -s`, `sysctl`, `df`),
//! which the daemon's `doctor::NativeHost` runs there. Windows has no such commands outside Git
//! Bash, and a login service started by the Task Scheduler does not have Git Bash's tools on its
//! `PATH`, so on Windows they are read from the system instead.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{disk_free_bytes, total_memory_bytes};
