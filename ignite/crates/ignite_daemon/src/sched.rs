//! The scheduler: the only place that decides which heavy command runs now. It grants cores by
//! class, holds shared resources to their capacity, keeps the `tip` and `batch` floors, and
//! counts load from outside Ignite.
//!
//! Stub. The ticket Thrh4.24 (Core scheduler) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::Class;

/// What a command asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantRequest {
    pub class: Class,
    pub cores: u32,
    pub resources: Vec<String>,
    /// The family's usual peak memory, if known.
    pub memory_mb: Option<u64>,
}

/// Cores and resources held until dropped or released.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub id: u64,
    pub cores: u32,
    pub resources: Vec<String>,
    /// Seconds spent waiting for the grant.
    pub queued_s: u64,
}

#[derive(Debug, Default)]
pub struct Scheduler;

impl Scheduler {
    /// Blocks until the request can be granted.
    pub fn acquire(&self, request: &GrantRequest) -> Grant {
        todo!("Thrh4.24 (Core scheduler)")
    }

    pub fn release(&self, grant: &Grant) {
        todo!("Thrh4.24 (Core scheduler)")
    }
}
