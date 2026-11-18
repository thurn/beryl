//! The operating system, behind one trait per module. [`Native`] implements every trait: the
//! implementation of each module is in `<module>/macos.rs`.
//!
//! This is the only crate where `unsafe` is allowed.

pub mod hostcpu;
pub mod ipc;
pub mod occupants;
pub mod proc;
pub mod service;
pub mod usage;

/// The running operating system's implementation of every trait in this crate.
#[derive(Clone, Copy, Debug, Default)]
pub struct Native;
