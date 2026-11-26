//! What a restarted daemon does with the verbs the previous daemon was running.
//!
//! A daemon restarts into a ready build at once, whatever runs (see `main/selfupdate.rs`). Each
//! command a verb started runs under its own supervisor process, which outlives the daemon, but
//! the verb's own thread does not. Every verb that does work got its handle, with the request
//! that started it, before it began, so the new daemon finds those handles still `running` in
//! the local database and, for each:
//!
//! - runs the verb again under the same handle if it is [resumable](super::Registry::register_resumable),
//!   in a new verb process when the verb runs in one, so the run again outlives the next restart
//!   too. On 2026-11-20 a landing run again on a thread of the daemon was cut off by each of the
//!   four restarts that followed within 13 minutes, since a restart waits only for landings whose
//!   code its release changes, and each run started its gate over.
//!   Its checks that finished come back from the result cache, and each command that was still
//!   running is taken over from its supervisor instead of being started again (see
//!   [`crate::adopt`]), so the run finishes as the old daemon's would have: results stored once,
//!   records sent, trees, cores and resources given back, and the handle's reply set. A client
//!   that was cut off waits on the handle again and prints that reply.
//! - otherwise ends the handle `incomplete`, with a reply that says why.
//!
//! A verb that runs in a verb process (see `server/process.rs`) is not run again while its
//! process lives: the new daemon finds `verbs/<handle>/lock` held, lists the verb and watches
//! the process, which goes on with the new daemon through the host protocol. A process that
//! wrote its reply while no daemon ran has the reply handed on. Only a verb process that died
//! (its lock is free and its handle still `running`) has its leftover commands ended and its
//! verb run again here, under the rules above. The exception is a landing whose live verb
//! process runs an older build than this daemon, whose landing code differs from this daemon's,
//! and has not begun its push: it is stopped and run again here under the same handle, so that it
//! runs the new build's code (see `server/outdated.rs`).

use std::sync::Arc;

use ignite_core::ids::Handle;
use ignite_core::protocol::client::Request;

use super::process::Shared;
use super::{Launch, Server, Verb, begin_activity, spawn_on_thread};
use crate::handles::{Orphan, incomplete};

impl Server {
    /// Goes on serving the verb processes the previous daemon started that still run, and runs
    /// again, or ends `incomplete`, every other handle the previous daemon left running.
    pub(super) fn resume_orphans(&self) {
        let shared = self.shared();
        for orphan in self.handles.take_orphans() {
            let Orphan {
                handle,
                verb,
                request,
            } = orphan;
            if self.runs_in_process(&verb) {
                if self.adopt_process(&handle, &verb, request.as_ref()) {
                    continue;
                }
                // Its verb process died, or was never started: only then is it run again here.
                // The commands it left are kept for the run again to take over (see
                // `crate::adopt`).
                eprintln!("ignite: the verb process of `ignite {verb}` ({handle}) is gone");
                shared.forget_process(&handle);
            }
            let resumable = self.registry.resumable(&verb);
            let runs = match (request, resumable) {
                (Some(request), Some(runs)) => Ok((runs, request)),
                // An internal verb, such as a tip round, is in no registry and is never run again.
                _ if self.registry.get(&verb).is_none() && !self.runs_in_process(&verb) => {
                    Err("this daemon has no such verb")
                }
                (_, None) => Err("it cannot be run again safely"),
                (None, Some(_)) => Err("the request that started it was not recorded"),
            };
            run_again(&shared, &handle, &verb, runs, true);
        }
    }
}

/// Runs `verb` again under `handle`, with the request that started it, as for a verb the
/// previous daemon was cut off in, or ends the handle `incomplete` for the reason given instead.
/// With `in_process`, a verb that runs in a verb process runs again in a new one, which outlives
/// the daemon's next restart; otherwise, or when no verb process can be started, it runs on a
/// thread of the daemon.
pub(super) fn run_again(
    shared: &Shared,
    handle: &Handle,
    verb: &str,
    runs: Result<(Arc<dyn Verb>, Request), &str>,
    in_process: bool,
) {
    let (runs, request) = match runs {
        Ok(found) => found,
        Err(reason) => {
            shared
                .handles
                .finish_incomplete(handle, incomplete(verb, handle, reason));
            return;
        }
    };
    eprintln!("ignite: running `ignite {verb}` ({handle}) again after the restart");
    if in_process && shared.runs_in_process(verb) {
        // The host serves the new process, though an earlier one of the handle was handed back.
        if let Some(host) = &shared.host {
            host.take_on(handle);
        }
        match shared.start_process(handle) {
            Some(Ok(child)) => {
                if let Some(host) = &shared.host {
                    let mut ctx = shared.ctx.clone();
                    ctx.session.clone_from(&request.session);
                    ctx.tree = ctx.tree_of_in(
                        &request.cwd,
                        shared.db.as_ref().map(|db| super::lock(db)).as_deref(),
                    );
                    host.track(&ctx, handle);
                }
                super::process::watch(shared.clone(), handle, Some(child), Some(runs), None);
                return;
            }
            Some(Err(error)) => {
                eprintln!(
                    "ignite: cannot start the verb process of {handle}, so `ignite {verb}` runs \
                     in the daemon: {error}"
                );
                if let Some(host) = &shared.host {
                    host.hand_back(handle);
                }
            }
            None => {}
        }
    }
    run_on_daemon(shared, handle, verb, runs, request);
}

/// Runs `runs`, the verb `verb`, on a thread of the daemon under `handle` with `request`, or
/// ends the handle `incomplete` when no thread can be started.
pub(super) fn run_on_daemon(
    shared: &Shared,
    handle: &Handle,
    verb: &str,
    runs: Arc<dyn Verb>,
    request: Request,
) {
    let request = Request {
        // Nobody waits on this run but `ignite wait`.
        wait_s: 0,
        ..request
    };
    let mut ctx = shared.ctx.clone();
    ctx.session.clone_from(&request.session);
    ctx.tree = ctx.tree_of_in(
        &request.cwd,
        shared.db.as_ref().map(|db| super::lock(db)).as_deref(),
    );
    let activity = begin_activity(&shared.steps, shared.db.as_ref(), &ctx, &request);
    activity.attach_handle(handle);
    let launch = Launch {
        verb: runs,
        ctx,
        req: request,
        activity,
        handle: Some(handle.clone()),
        probe: None,
    };
    if let Err(reply) = spawn_on_thread(&shared.handles, &shared.running, launch) {
        let reason = format!("it could not be started again: {}", reply.text);
        shared
            .handles
            .finish_incomplete(handle, incomplete(verb, handle, &reason));
    }
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
