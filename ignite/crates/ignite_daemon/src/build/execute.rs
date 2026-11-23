//! Join a running build or reserve its outputs before asking for space and running it.

use std::sync::{Arc, PoisonError};

use ignite_core::traits::BuildResult;

use super::flight::{FlightGuard, STOP_RECHECK};
use super::{BuildManager, Flight, Plan, Usage, lock};
use crate::steps::{Activity, Kind, Waiting};

impl BuildManager {
    /// Runs the build, or joins or skips it, holding each output directory for one build.
    pub(super) fn build(&self, plan: &Plan) -> BuildResult {
        let _step = crate::steps::during(format!(
            "building {} in {}",
            plan.request.product, plan.request.tree
        ));
        let flight_key = (plan.request.clone(), plan.hash.clone());
        // The owner can finish before a host-build thread has dropped its reservation.
        let activity = crate::steps::current();
        let cancelled = || {
            activity
                .as_ref()
                .is_some_and(|activity| activity.cancelled())
        };
        let compiled = self.tree_control.get().is_some() && self.tracks_compiled_outputs(plan);
        let mut state = lock(&self.state);
        let mut waiting: Option<Waiting> = None;
        loop {
            if state.held.get(&plan.request) == Some(&plan.hash) {
                return BuildResult::Current;
            }
            if let Some((hash, output)) = state.failed.get(&plan.request)
                && *hash == plan.hash
            {
                // Nothing ran for this request, so it reports no usage.
                return BuildResult::Failed {
                    output: output.clone(),
                    usage: Usage::default(),
                };
            }
            if let Some(flight) = state.flights.get(&flight_key).cloned() {
                state.joined += 1;
                drop(state);
                drop(waiting.take());
                let blocker = flight.describe(&plan.request);
                let _waiting = crate::steps::waiting(&blocker);
                let result = flight.wait(&cancelled);
                lock(&self.state).joined -= 1;
                return result.unwrap_or_else(|| self.stopped_waiting(plan, Some(&blocker)));
            }
            // Compiled products also reserve the tree for disk cleanup, even if they write
            // separate directories. Both kinds of reservation belong to the same flight.
            let blocker = state.flights.iter().find(|((request, _), flight)| {
                request.tree == plan.request.tree
                    && (compiled
                        || flight
                            .outputs
                            .iter()
                            .any(|dir| plan.product.outputs.contains(dir)))
            });
            let blocker = blocker.map(|((request, _), flight)| flight.describe(request));
            if cancelled() {
                drop(state);
                return self.stopped_waiting(plan, blocker.as_deref());
            }
            let Some(blocker) = blocker else { break };
            let reason = format!(
                "{blocker} to finish before building {}",
                plan.request.product
            );
            if let Some(waiting) = &waiting {
                waiting.update(&reason);
            } else {
                waiting = Some(crate::steps::waiting(reason));
            }
            state.queued += 1;
            // Woken when a build ends, and every `STOP_RECHECK` to check for cancellation.
            state = self
                .ended
                .wait_timeout(state, STOP_RECHECK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            state.queued -= 1;
        }
        drop(waiting);
        let owner = activity.as_deref().map(describe_owner);
        let flight = Arc::new(Flight::new(plan.product.outputs.clone(), owner));
        state.flights.insert(flight_key, Arc::clone(&flight));
        drop(state);

        // The guard frees the directories and wakes the joiners even if `run` panics.
        let guard = FlightGuard::new(self, plan.request.clone(), plan.hash.clone(), flight);
        let (result, remember) = match self.prepare_space(plan, &cancelled) {
            Ok(_slot) if !cancelled() => self.run(plan),
            Ok(_) => (self.stopped_waiting(plan, None), false),
            Err(result) => (result, false),
        };
        guard.finish(&result, remember);
        result
    }
}

fn describe_owner(activity: &Activity) -> String {
    let mut owner = match activity.kind() {
        Kind::Verb(verb) => format!("ignite {verb}"),
        Kind::Loop(name) => name.clone(),
    };
    if let Some(handle) = activity.handle() {
        use std::fmt::Write as _;
        let _ = write!(owner, " {handle}");
    }
    owner
}
