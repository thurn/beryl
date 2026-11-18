//! Idle work, run only while the machine is under 70% busy: exploration checks, cache audits and
//! rebuild counts.
//!
//! Stub. The ticket Thrh4.31 (Idle work: exploration, cache audits and rebuild counts) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

/// One piece of idle work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdleJob {
    Explore { family: String },
    Audit,
    Rebuild { name: String },
}

/// The next idle job, if the machine has room for one.
pub fn next_job(ctx: &Context) -> Option<IdleJob> {
    todo!("Thrh4.31 (Idle work: exploration, cache audits and rebuild counts)")
}

/// `ignite rebuild`: measures the rebuild cost of a named edit.
pub struct RebuildVerb;

impl Verb for RebuildVerb {
    fn name(&self) -> &'static str {
        "rebuild"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.31 (Idle work: exploration, cache audits and rebuild counts)")
    }
}

/// `ignite audit`: reruns a sample of cache hits to check the cache.
pub struct AuditVerb;

impl Verb for AuditVerb {
    fn name(&self) -> &'static str {
        "audit"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.31 (Idle work: exploration, cache audits and rebuild counts)")
    }
}
