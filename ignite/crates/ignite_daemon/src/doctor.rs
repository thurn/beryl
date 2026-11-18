//! The machine's state and health.
//!
//! Stub. The ticket Thrh4.34 (Install, self-update, status and doctor) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

/// `ignite status`: prints the machine's state.
pub struct StatusVerb;

impl Verb for StatusVerb {
    fn name(&self) -> &'static str {
        "status"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.34 (Install, self-update, status and doctor)")
    }
}

/// `ignite doctor`: checks the machine's health and says what to fix.
pub struct DoctorVerb;

impl Verb for DoctorVerb {
    fn name(&self) -> &'static str {
        "doctor"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.34 (Install, self-update, status and doctor)")
    }
}
