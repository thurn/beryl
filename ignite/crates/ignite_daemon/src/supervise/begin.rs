//! Starting a supervised command: the grant, the run directory and the supervisor. A verb that
//! runs in the daemon waits for its command on the same thread ([`SupervisedLauncher::run`]);
//! a verb that runs in a process of its own asks the daemon only to start it
//! ([`SupervisedLauncher::begin`]) and waits for the run's `lock` and `result.json` itself (see
//! `crate::host`).

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, RunResult};
use ignite_os::fifo;

use super::grants::{GrantFile, GrantGuard, watch_for_stop};
use super::jobserver::compiles_only;
use super::{CORES, Run, Started, SupervisedLauncher, peaks};
use crate::sched::{Grant, GrantRequest};

/// How [`SupervisedLauncher::launch`] ended.
pub(super) enum Launch {
    /// The supervisor runs the command under the grant the guard holds.
    Started(Started, GrantGuard),
    /// The caller cancelled while the command waited for its grant.
    Cancelled,
    /// The run directory or the supervisor could not be made.
    Failed(io::Error),
}

impl SupervisedLauncher {
    /// Waits for the command's grant, queued as if first asked for at `since` (now, when
    /// `None`), and starts its supervisor. A cancelled request is withdrawn.
    pub(super) fn launch(
        &self,
        spec: &CommandSpec,
        since: Option<Timestamp>,
        cancelled: &dyn Fn() -> bool,
    ) -> Launch {
        // The run directory comes before the grant, because a grant that changes keeps its size
        // in a file there from the moment it is granted.
        let dir = match self.new_run() {
            Ok(dir) => dir,
            Err(error) => return Launch::Failed(error),
        };
        // Every command gets a jobserver, whatever its first word, so that the Cargo builds a
        // script, `just` or `uv` starts follow the grant as it grows. Only a Cargo command that
        // only compiles shrinks: what other commands run besides Cargo, such as tests, keeps
        // the thread count it started with. Where there are no FIFOs, a command without a
        // jobserver keeps the size it started with, unless it rereads its cores file.
        let shrinks = compiles_only(&spec.args);
        let cores_file = self
            .scheduler
            .as_ref()
            .filter(|_| fifo::AVAILABLE || shrinks || spec.grows)
            .map(|_| dir.join(CORES));
        let Some(grant) = self.grant(spec, shrinks, cores_file.clone(), since, cancelled) else {
            let _ = fs::remove_dir_all(&dir);
            return Launch::Cancelled;
        };
        // Released when the guard is dropped, however the caller ends.
        let held = GrantGuard::new(self.scheduler.clone(), grant);
        let grant = held.grant();
        let follows = cores_file.is_some();
        let jobserver = cores_file
            .filter(|_| fifo::AVAILABLE)
            .zip(self.scheduler.as_ref())
            .map(|(file, scheduler)| (file, scheduler.status().max_per_command));
        let file = GrantFile {
            class: spec.class,
            cores: grant.cores,
            resources: grant.resources.clone(),
            asked: Some(spec.cores),
            follows,
            shrinks,
        };
        let env = grant.env();
        let queued_s = grant.queued_s as f64;
        match self.start_in(dir, spec, env, queued_s, jobserver, Some(file)) {
            Ok(started) => {
                // The scheduler stops `idle` work when a higher class waits or holds what it
                // needs; stopping ends the command. The thread ends when the grant is released.
                if let (Class::Idle, Some(scheduler)) = (spec.class, &self.scheduler) {
                    let id = held.grant().id;
                    watch_for_stop(scheduler, id, started.run().clone(), self.stop_grace_s);
                }
                Launch::Started(started, held)
            }
            Err(error) => Launch::Failed(error),
        }
    }

    /// Starts `spec`'s command and returns its run directory once its supervisor holds the
    /// run's lock, without waiting for the command. The command waits for its grant first,
    /// queued as if first asked for at `since`; `None` when `cancelled` turned true meanwhile.
    ///
    /// The grant stays held until the command has finished: a thread of this daemon waits for
    /// the supervisor and then gives the grant back. If the daemon restarts first, the next one
    /// registers the grant again from the run's `spec.json` (see
    /// [`SupervisedLauncher::adopt_unfinished`]).
    pub fn begin(
        self: &Arc<Self>,
        spec: &CommandSpec,
        since: Timestamp,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<Option<PathBuf>> {
        match self.launch(spec, Some(since), cancelled) {
            Launch::Started(started, held) => {
                let dir = started.run().dir().to_path_buf();
                let launcher = Arc::clone(self);
                let args = spec.args.clone();
                thread::spawn(move || {
                    let result = started.wait();
                    drop(held);
                    launcher.finished(&args, &result);
                });
                Ok(Some(dir))
            }
            Launch::Cancelled => Ok(None),
            Launch::Failed(error) => Err(error),
        }
    }

    /// Records what a finished command used, for the scheduler's memory gate, and deletes old
    /// run directories now and then.
    pub(super) fn finished(&self, args: &[String], result: &RunResult) {
        if result.exit.is_some() && !result.hung {
            self.peaks.record(&peaks::key(args), result.usage.peak_mb);
        }
        self.prune_now_and_then();
    }

    /// Asks the scheduler for the command's cores. A grant with a `cores_file` keeps its size
    /// there and grows as cores free up, which the command's jobserver follows. A Cargo build
    /// that only compiles also shrinks while a higher class waits.
    fn grant(
        &self,
        spec: &CommandSpec,
        shrinks: bool,
        cores_file: Option<PathBuf>,
        since: Option<Timestamp>,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Grant> {
        let Some(scheduler) = &self.scheduler else {
            return Some(Grant {
                id: 0,
                cores: spec.cores.max(1),
                resources: spec.resources.clone(),
                queued_s: 0,
                cores_file: None,
            });
        };
        let request = GrantRequest {
            class: spec.class,
            cores: spec.cores,
            resources: spec.resources.clone(),
            // The median peak of the command's recent runs, so that the scheduler holds a
            // command back while less than that is free.
            memory_mb: self.usual_memory(&spec.args),
            shrinks,
            grows: spec.grows,
            cores_file,
        };
        match since {
            Some(since) => scheduler.acquire_since(&request, since, cancelled),
            None => scheduler.acquire_until(&request, cancelled),
        }
    }
}

impl Run {
    /// A run directory, as `crate::host` names it to a verb process.
    pub fn at(dir: PathBuf) -> Run {
        Run { dir }
    }
}
