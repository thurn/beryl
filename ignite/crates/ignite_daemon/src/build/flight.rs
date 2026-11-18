//! The record of a running build: the result that requests joined to it wait for, and the
//! guard that frees its output directories however the build ends.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use ignite_core::ids::{InputHash, TreeId};
use ignite_core::traits::BuildResult;

use super::store::Change;
use super::{BuildManager, Request, State, lock};

/// How often a request waiting on another build checks whether its own work was asked to stop.
pub(super) const STOP_RECHECK: Duration = Duration::from_millis(250);

/// One running build that other requests can wait for.
#[derive(Default)]
pub(super) struct Flight {
    result: Mutex<Option<BuildResult>>,
    done: Condvar,
}

impl Flight {
    /// Waits for the build's result. Returns `None` once `stopped` is true, which is asked every
    /// [`STOP_RECHECK`], so a request whose work is stopped does not wait for the whole build.
    pub(super) fn wait(&self, stopped: &dyn Fn() -> bool) -> Option<BuildResult> {
        let mut result = lock(&self.result);
        loop {
            if let Some(result) = result.as_ref() {
                return Some(result.clone());
            }
            if stopped() {
                return None;
            }
            result = self
                .done
                .wait_timeout(result, STOP_RECHECK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn finish(&self, result: BuildResult) {
        *lock(&self.result) = Some(result);
        self.done.notify_all();
    }
}

/// A build in progress: the output directories it holds and the flight that joiners wait on.
/// [`FlightGuard::finish`] ends it with the build's result. Dropped without that call, as when
/// the build panics, it frees the directories and ends the flight with a failure, so that no
/// later request waits on a build that no longer runs.
pub(super) struct FlightGuard<'a> {
    manager: &'a BuildManager,
    request: Request,
    hash: InputHash,
    dirs: Vec<(TreeId, String)>,
    flight: Arc<Flight>,
    finished: bool,
}

impl<'a> FlightGuard<'a> {
    pub(super) fn new(
        manager: &'a BuildManager,
        request: Request,
        hash: InputHash,
        dirs: Vec<(TreeId, String)>,
        flight: Arc<Flight>,
    ) -> Self {
        Self {
            manager,
            request,
            hash,
            dirs,
            flight,
            finished: false,
        }
    }

    /// Frees the output directories and removes the flight, in `state`.
    fn release(&self, state: &mut State) {
        for dir in &self.dirs {
            state.busy.remove(dir);
        }
        state
            .flights
            .remove(&(self.request.clone(), self.hash.clone()));
    }

    /// Ends the build with `result`. `remember` says whether a failure is one the same inputs
    /// would repeat, so that later requests get it without building. What the tree now holds is
    /// stored before any joined request is woken.
    pub(super) fn finish(mut self, result: &BuildResult, remember: bool) {
        self.finished = true;
        let mut state = lock(&self.manager.state);
        self.release(&mut state);
        let request = &self.request;
        let change = match result {
            BuildResult::Built { .. } => {
                state.failed.remove(request);
                state.held.insert(request.clone(), self.hash.clone());
                Some(Change::Held)
            }
            BuildResult::Failed { output, .. } => {
                // A failed build may have rewritten part of its outputs, so the tree no longer
                // holds the last state that was built.
                state.held.remove(request);
                if remember {
                    let failure = (self.hash.clone(), output.clone());
                    state.failed.insert(request.clone(), failure);
                    Some(Change::Failed(output.clone()))
                } else {
                    Some(Change::NotHeld)
                }
            }
            BuildResult::Current => None,
        };
        drop(state);
        if let Some(change) = change {
            self.manager.store_change(request, &self.hash, &change);
        }
        self.flight.finish(result.clone());
        self.manager.ended.notify_all();
    }
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.release(&mut lock(&self.manager.state));
        let failed = self
            .manager
            .failure(&self.request.product, "the build stopped without a result");
        self.flight.finish(failed);
        self.manager.ended.notify_all();
    }
}
