//! Landing: rebase onto the tip in a landing tree, run the gate, push to the integration branch,
//! record the note, close the ticket and release the tree.
//!
//! Stub. The ticket Thrh4.29 (Landing) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

/// `ignite land`: rebases, runs the gate and lands the commits.
pub struct LandVerb;

impl Verb for LandVerb {
    fn name(&self) -> &'static str {
        "land"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.29 (Landing)")
    }
}

/// `ignite sync`: rebases the tree's branch onto the tip.
pub struct SyncVerb;

impl Verb for SyncVerb {
    fn name(&self) -> &'static str {
        "sync"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.29 (Landing)")
    }
}
