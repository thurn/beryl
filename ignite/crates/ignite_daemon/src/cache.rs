//! The result cache client: the local copy first, then one hub request per command for the rest.
//!
//! Stub. The ticket Thrh4.28 (Check runner and the result cache client) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::hub::{CachedResult, NewResult, ResultKey};

#[derive(Debug, Default)]
pub struct ResultCache;

impl ResultCache {
    pub fn lookup(&self, keys: &[ResultKey]) -> Vec<Option<CachedResult>> {
        todo!("Thrh4.28 (Check runner and the result cache client)")
    }

    /// Stores locally and queues for the hub.
    pub fn store(&self, results: &[NewResult]) {
        todo!("Thrh4.28 (Check runner and the result cache client)")
    }
}
