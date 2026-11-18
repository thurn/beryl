//! What a restarted daemon does with the verbs the previous daemon was running.
//!
//! A daemon restarts into a ready build at once, whatever runs (see `main/selfupdate.rs`). Each
//! command a verb started runs under its own supervisor process, which outlives the daemon, but
//! the verb's own thread does not. Every verb that does work got its handle, with the request
//! that started it, before it began, so the new daemon finds those handles still `running` in
//! the local database and, for each:
//!
//! - runs the verb again under the same handle if it is [resumable](super::Registry::register_resumable).
//!   Its checks that finished come back from the result cache, and each command that was still
//!   running is taken over from its supervisor instead of being started again (see
//!   [`crate::adopt`]), so the run finishes as the old daemon's would have: results stored once,
//!   records sent, trees, cores and resources given back, and the handle's reply set. A client
//!   that was cut off waits on the handle again and prints that reply.
//! - otherwise ends the handle `incomplete`, with a reply that says why.

use std::sync::Arc;

use ignite_core::protocol::client::Request;

use super::{Launch, Server};
use crate::handles::{Orphan, incomplete};

impl Server {
    /// Runs again, or ends `incomplete`, every handle the previous daemon left running.
    pub(super) fn resume_orphans(&self) {
        for orphan in self.handles.take_orphans() {
            let Orphan {
                handle,
                verb,
                request,
            } = orphan;
            let resumable = self.registry.resumable(&verb);
            let (Some(request), Some(runs)) = (request, resumable) else {
                let reason = if self.registry.get(&verb).is_none() {
                    "this daemon has no such verb"
                } else if self.registry.resumable(&verb).is_none() {
                    "it cannot be run again safely"
                } else {
                    "the request that started it was not recorded"
                };
                self.handles
                    .finish_incomplete(&handle, incomplete(&verb, &handle, reason));
                continue;
            };
            eprintln!("ignite: running `ignite {verb}` ({handle}) again after the restart");
            let request = Request {
                // Nobody waits on this run but `ignite wait`.
                wait_s: 0,
                ..request
            };
            let mut ctx = self.ctx.clone();
            ctx.session.clone_from(&request.session);
            ctx.tree = ctx.tree_of(&request.cwd);
            let activity = self.begin(&ctx, &request);
            activity.attach_handle(&handle);
            let launch = Launch {
                verb: Arc::clone(&runs),
                ctx,
                req: request,
                activity,
                handle: Some(handle.clone()),
                probe: None,
            };
            if let Err(reply) = self.spawn_verb(launch) {
                let reason = format!("it could not be started again: {}", reply.text);
                self.handles
                    .finish_incomplete(&handle, incomplete(&verb, &handle, &reason));
            }
        }
    }
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
