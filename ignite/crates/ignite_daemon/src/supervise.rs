//! Supervisors: one small process per command, its parent, which waits for it, measures it and
//! writes the result to a file, so the daemon can restart while commands run.
//!
//! Stub. The ticket Thrh4.25 (Supervisor, `ignite run` and the compiler wrapper) owns this file
//! and replaces the `todo!()` bodies.
#![allow(unused_variables)]

use std::path::Path;

use ignite_core::protocol::client::{Reply, Request};
use ignite_core::traits::{CommandSpec, Launcher, RunResult};

use crate::server::{Context, Verb};

/// Runs commands by starting a supervisor for each.
#[derive(Debug, Default)]
pub struct SupervisedLauncher;

impl Launcher for SupervisedLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        todo!("Thrh4.25 (Supervisor, `ignite run` and the compiler wrapper)")
    }
}

/// The hidden `ignite supervise <spec file>` subcommand; returns the process's exit code.
pub fn supervise(spec_file: &Path) -> i32 {
    todo!("Thrh4.25 (Supervisor, `ignite run` and the compiler wrapper)")
}

/// `ignite run`: runs any heavy command with cores from the daemon, and records it.
pub struct RunVerb;

impl Verb for RunVerb {
    fn name(&self) -> &'static str {
        "run"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        todo!("Thrh4.25 (Supervisor, `ignite run` and the compiler wrapper)")
    }
}
