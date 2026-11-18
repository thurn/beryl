//! Handles: an operation that would outlive the client's wait returns a short name, and
//! `ignite wait <handle>` blocks until it finishes.
//!
//! Stub. The ticket Thrh4.23 (Daemon core: requests, handles and the hub client) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

use ignite_core::ids::Handle;

/// Operations still running or finished but not yet collected.
#[derive(Debug, Default)]
pub struct Handles;

impl Handles {
    /// Names a running operation.
    pub fn create(&self, verb: &str) -> Handle {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }

    /// The reply, once the operation has finished, waiting at most `wait_s`.
    pub fn wait(&self, handle: &Handle, wait_s: u32) -> Option<Reply> {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }
}

/// `ignite wait`: blocks until a handle's operation finishes.
pub struct WaitVerb;

impl Verb for WaitVerb {
    fn name(&self) -> &'static str {
        "wait"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.23 (Daemon core: requests, handles and the hub client)")
    }
}
