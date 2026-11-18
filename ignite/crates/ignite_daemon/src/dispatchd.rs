//! The dispatcher endpoint: hands each idle worker its next ticket through the dispatcher session,
//! re-sends hand-offs not acknowledged in 2 minutes, and marks a dispatcher down after three.
//!
//! Stub. The ticket Thrh4.33 (Dispatcher endpoint and the pool) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

/// `ignite dispatch`: blocks until the dispatcher has something to do, then prints it.
pub struct DispatchVerb;

impl Verb for DispatchVerb {
    fn name(&self) -> &'static str {
        "dispatch"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.33 (Dispatcher endpoint and the pool)")
    }
}
