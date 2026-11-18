//! The check runner: lists a family's checks, looks each up in the result cache, runs the rest
//! with the family's adapter and reports outcomes.
//!
//! Stub. The ticket Thrh4.28 (Check runner and the result cache client) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use ignite_core::protocol::client::{Reply, Request};

use crate::server::{Context, Verb};

use ignite_core::ids::{CheckName, TreeId};
use ignite_core::protocol::Outcome;

/// The result of running a set of checks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CheckRun {
    pub outcomes: Vec<(CheckName, Outcome)>,
    pub from_cache: u32,
    pub ran: u32,
}

/// Runs `checks` in `tree`, reusing cached results unless `rerun`.
pub fn run_checks(ctx: &Context, tree: &TreeId, checks: &[CheckName], rerun: bool) -> CheckRun {
    todo!("Thrh4.28 (Check runner and the result cache client)")
}

/// `ignite test`: runs the checks the agent names, in its tree.
pub struct TestVerb;

impl Verb for TestVerb {
    fn name(&self) -> &'static str {
        "test"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.28 (Check runner and the result cache client)")
    }
}

/// `ignite check`: runs what the landing gate would run on the tree as it is.
pub struct CheckVerb;

impl Verb for CheckVerb {
    fn name(&self) -> &'static str {
        "check"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.28 (Check runner and the result cache client)")
    }
}

/// `ignite why`: prints a check's record: its cost, its failures, where it runs and why.
pub struct WhyVerb;

impl Verb for WhyVerb {
    fn name(&self) -> &'static str {
        "why"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.28 (Check runner and the result cache client)")
    }
}
