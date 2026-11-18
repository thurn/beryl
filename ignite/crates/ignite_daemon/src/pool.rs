//! The pool: the machine's worker sessions, their state and what each holds.
//!
//! Stub. The ticket Thrh4.33 (Dispatcher endpoint and the pool) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

/// `ignite pool`: lists, adds or removes pool members.
pub struct PoolVerb;

impl Verb for PoolVerb {
    fn name(&self) -> &'static str {
        "pool"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.33 (Dispatcher endpoint and the pool)")
    }
}
