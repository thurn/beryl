//! The operating system, behind one trait per module. [`Native`] implements every trait: the
//! macOS half of each module is in `<module>/macos.rs` and the Windows half in
//! `<module>/windows.rs`, so the two can be written apart.
//!
//! This is the only crate where `unsafe` is allowed.

pub mod clones;
pub mod fifo;
pub mod files;
pub mod hostcpu;
pub mod ipc;
pub mod notify;
pub mod occupants;
pub mod proc;
pub mod service;
pub mod system;
pub mod terminate;
pub mod usage;

/// The running operating system's implementation of every trait in this crate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Native;
