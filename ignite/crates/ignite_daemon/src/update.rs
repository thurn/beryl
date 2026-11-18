//! Installing Ignite on a machine and keeping it current: building the executable from each
//! release, restarting into it, and registering with the hub.
//!
//! Stub. The ticket Thrh4.34 (Install, self-update, status and doctor) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

/// `ignite install`: builds the executable, registers the machine and starts the daemon at login.
pub struct InstallVerb;

impl Verb for InstallVerb {
    fn name(&self) -> &'static str {
        "install"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.34 (Install, self-update, status and doctor)")
    }
}
