//! The scheduler's grant around one supervised command: released when its holder goes out of
//! scope, stopped when the scheduler asks, and re-registered when a restarted daemon finds the
//! command still running.

use std::fs;
use std::sync::Arc;
use std::thread;

use ignite_core::protocol::Class;
use serde::{Deserialize, Serialize};

use super::jobserver::read_cores;
use super::{CORES, Run, SpecFile, SupervisedLauncher};
use crate::sched::{Grant, Scheduler};

/// Seconds an `idle` command has to exit after it is asked to, before its group is killed.
pub const STOP_GRACE_S: f64 = 10.0;

/// What a run's grant was, kept in its `spec.json` so a daemon that restarts while the command
/// runs can register the same cores and resources again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantFile {
    pub class: Class,
    /// The size when granted. A grant that changed size keeps its current one in the run's
    /// `cores` file, which is read first.
    pub cores: u32,
    pub resources: Vec<String>,
}

/// A grant that is released when this value is dropped, so that a panic between asking for
/// cores and handing them back (in the supervisor's start or in waiting for its result) cannot
/// keep them forever.
pub(super) struct GrantGuard {
    scheduler: Option<Arc<Scheduler>>,
    grant: Grant,
}

impl GrantGuard {
    pub(super) fn new(scheduler: Option<Arc<Scheduler>>, grant: Grant) -> Self {
        Self { scheduler, grant }
    }

    pub(super) fn grant(&self) -> &Grant {
        &self.grant
    }
}

impl Drop for GrantGuard {
    fn drop(&mut self) {
        if let Some(scheduler) = &self.scheduler {
            scheduler.release(&self.grant);
        }
    }
}

/// Starts the thread that ends `run`'s command when the scheduler asks grant `id` to stop: first
/// `SIGTERM` (or the job object's end), then, if the grant is still held [`STOP_GRACE_S`] later
/// by the scheduler's clock, `SIGKILL`. The thread ends when the grant is released.
pub(super) fn watch_for_stop(scheduler: &Arc<Scheduler>, id: u64, run: Run) {
    let scheduler = Arc::clone(scheduler);
    thread::spawn(move || {
        if scheduler.wait_stop(id) {
            let _ = run.end();
            if !scheduler.wait_released(id, STOP_GRACE_S) {
                let _ = run.kill();
            }
        }
    });
}

impl SupervisedLauncher {
    /// Takes over the runs a previous daemon started and nobody collected: each is registered
    /// with the scheduler under the cores and resources it was granted, so they are not handed
    /// out again while the command runs, and released when its supervisor has exited. Each run
    /// waits on a thread of its own.
    pub fn adopt_unfinished(&self) {
        for run in self.unfinished() {
            let held = self.scheduler.as_ref().and_then(|scheduler| {
                let spec = run.spec()?;
                let file = spec.grant?;
                let cores = read_cores(&run.dir().join(CORES)).unwrap_or(file.cores);
                let grant = scheduler.adopt(file.class, cores, file.resources, None);
                if file.class == Class::Idle {
                    watch_for_stop(scheduler, grant.id, run.clone());
                }
                Some(GrantGuard::new(Some(Arc::clone(scheduler)), grant))
            });
            thread::spawn(move || {
                run.wait();
                drop(held);
            });
        }
    }
}

impl Run {
    /// The spec this run was started with, if its file can still be read.
    pub(super) fn spec(&self) -> Option<SpecFile> {
        serde_json::from_slice(&fs::read(self.dir().join(super::SPEC)).ok()?).ok()
    }
}
