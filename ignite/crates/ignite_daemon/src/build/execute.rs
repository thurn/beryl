//! Join a running build or reserve its outputs before asking for space and running it.

use std::sync::{Arc, PoisonError};

use ignite_core::ids::TreeId;
use ignite_core::traits::BuildResult;

use super::flight::{FlightGuard, STOP_RECHECK};
use super::{BuildManager, Flight, Plan, Usage, lock};

impl BuildManager {
    /// Runs the build, or joins or skips it, holding each output directory for one build.
    pub(super) fn build(&self, plan: &Plan) -> BuildResult {
        let _step = crate::steps::during(format!(
            "building {} in {}",
            plan.request.product, plan.request.tree
        ));
        let flight_key = (plan.request.clone(), plan.hash.clone());
        let dirs: Vec<(TreeId, String)> = plan
            .product
            .outputs
            .iter()
            .map(|dir| (plan.request.tree.clone(), dir.clone()))
            .collect();
        // The work that asked for the build. Once it is asked to stop, a wait for another build
        // gives up instead of lasting until that build ends.
        let activity = crate::steps::current();
        let stop_requested = || activity.as_ref().is_some_and(|a| a.stop_requested());
        let compiled = self.tree_control.get().is_some() && self.tracks_compiled_outputs(plan);
        let mut state = lock(&self.state);
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
                let _waiting = crate::steps::waiting(format!(
                    "the build of {} in {} that another command started",
                    plan.request.product, plan.request.tree
                ));
                let result = flight.wait(&stop_requested);
                lock(&self.state).joined -= 1;
                return result.unwrap_or_else(|| self.stopped_waiting(plan));
            }
            let same_tree_build = compiled
                && state
                    .flights
                    .keys()
                    .any(|(request, _)| request.tree == plan.request.tree);
            if !same_tree_build && dirs.iter().all(|dir| !state.busy.contains(dir)) {
                break;
            }
            if stop_requested() {
                drop(state);
                return self.stopped_waiting(plan);
            }
            state.queued += 1;
            let waiting = crate::steps::waiting(format!(
                "another build in {} to finish before building {}",
                plan.request.tree, plan.request.product
            ));
            // Woken when a build ends, and every `STOP_RECHECK` to see whether the work that
            // asked for this build was stopped, as `ignite stop` does.
            state = self
                .ended
                .wait_timeout(state, STOP_RECHECK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
            drop(waiting);
            state.queued -= 1;
        }
        let flight = Arc::new(Flight::default());
        state
            .flights
            .insert(flight_key.clone(), Arc::clone(&flight));
        state.busy.extend(dirs.iter().cloned());
        drop(state);

        // The guard frees the directories and wakes the joiners even if `run` panics.
        let guard = FlightGuard::new(self, plan.request.clone(), plan.hash.clone(), dirs, flight);
        let (result, remember) = match self.prepare_space(plan, &stop_requested) {
            Ok(_slot) => self.run(plan),
            Err(result) => (result, false),
        };
        guard.finish(&result, remember);
        result
    }
}
