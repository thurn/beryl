//! Starting and releasing tickets: claiming a reservation or filing an ad hoc ticket, leasing a
//! tree and starting its builds, and giving a ticket back.
//!
//! Stub. The ticket Thrh4.32 (Starting and releasing tickets) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

/// `ignite start`: claims a ticket, leases a build tree at the tip and prints the ticket.
pub struct StartVerb;

impl Verb for StartVerb {
    fn name(&self) -> &'static str {
        "start"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.32 (Starting and releasing tickets)")
    }
}

/// `ignite release`: gives the ticket back without landing it and frees its tree.
pub struct ReleaseVerb;

impl Verb for ReleaseVerb {
    fn name(&self) -> &'static str {
        "release"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.32 (Starting and releasing tickets)")
    }
}
