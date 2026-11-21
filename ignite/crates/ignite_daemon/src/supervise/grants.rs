//! The scheduler's grant around one supervised command: released when its holder goes out of
//! scope, stopped when the scheduler asks, and re-registered when a restarted daemon finds the
//! command still running.

use std::fs;
use std::sync::Arc;
use std::thread;

use ignite_core::ids::TicketId;
use ignite_core::protocol::Class;
use serde::{Deserialize, Serialize};

use super::jobserver::read_cores;
use super::{CORES, Run, SpecFile, SupervisedLauncher};
use crate::sched::{Adoption, Follow, Grant, Scheduler};

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
    /// The cores the command asked for, which a grant that follows its cores file grows to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<u32>,
    /// The grant kept its size in the run's `cores` file, which the command's jobserver
    /// follows, so a restarted daemon must keep resizing it.
    #[serde(default)]
    pub follows: bool,
    /// The command gives cores back while a higher class waits.
    #[serde(default)]
    pub shrinks: bool,
    /// Cores set aside for this kind of command, which a restarted daemon will grant from the
    /// same reserve. Left out when unset, so that a `spec.json` written by an older daemon
    /// still parses and one written by this daemon parses in an older one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve: Option<ignite_core::traits::Reserve>,
    /// Agent queue priority, if the command's ticket held master back when it started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_ticket: Option<TicketId>,
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
/// `SIGTERM` (or the job object's end), then, if the grant is still held `grace_s` later (normally
/// [`STOP_GRACE_S`]) by the scheduler's clock, `SIGKILL`. The thread ends when the grant is released.
pub(super) fn watch_for_stop(scheduler: &Arc<Scheduler>, id: u64, run: Run, grace_s: f64) {
    let scheduler = Arc::clone(scheduler);
    thread::spawn(move || {
        if scheduler.wait_stop(id) {
            let _ = run.end();
            if !scheduler.wait_released(id, grace_s) {
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
        let runs = self.unfinished();
        let Some(scheduler) = &self.scheduler else {
            for run in runs {
                thread::spawn(move || run.wait());
            }
            return;
        };
        // Every grant is recorded before the scheduler takes its next step, so no grant grows
        // into cores that a run not yet recorded still holds (`Scheduler::adopt_all`).
        let mut adopted = Vec::new();
        let mut adoptions = Vec::new();
        let mut others = Vec::new();
        for run in runs {
            match run.spec().and_then(|spec| spec.grant) {
                Some(file) => {
                    let cores_file = run.dir().join(CORES);
                    let cores = read_cores(&cores_file).unwrap_or(file.cores);
                    // A grant that followed its cores file keeps growing and shrinking after
                    // the restart; without the file the command's jobserver would stay at this
                    // size.
                    let follow = file.follows.then(|| Follow {
                        cores_file,
                        asked: file.asked.unwrap_or(cores),
                        shrinks: file.shrinks,
                    });
                    let class = file.class;
                    adoptions.push(Adoption {
                        class,
                        cores,
                        resources: file.resources,
                        memory_mb: None,
                        follow,
                        reserve: file.reserve,
                        priority_ticket: file.priority_ticket,
                    });
                    adopted.push((run, class));
                }
                None => others.push(run),
            }
        }
        let grants = scheduler.adopt_all(adoptions);
        let cores_held: u32 = grants.iter().map(|g| g.cores).sum();
        if !grants.is_empty() {
            eprintln!(
                "ignite: took over {} runs a previous daemon started, holding {cores_held} cores",
                grants.len()
            );
        }
        for ((run, class), grant) in adopted.into_iter().zip(grants) {
            if class == Class::Idle {
                watch_for_stop(scheduler, grant.id, run.clone(), self.stop_grace_s);
            }
            scheduler.watch(grant.id, run.dir().join(super::GROUP));
            let held = GrantGuard::new(Some(Arc::clone(scheduler)), grant);
            thread::spawn(move || {
                run.wait();
                drop(held);
            });
        }
        for run in others {
            thread::spawn(move || run.wait());
        }
    }
}

impl Run {
    /// The spec this run was started with, if its file can still be read.
    pub(super) fn spec(&self) -> Option<SpecFile> {
        serde_json::from_slice(&fs::read(self.dir().join(super::SPEC)).ok()?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{GrantFile, SpecFile};
    use ignite_core::protocol::Class;
    use ignite_core::traits::Reserve;

    /// A `spec.json` as a daemon wrote it before grants had a reserve. A restarted daemon takes
    /// over commands an older release started, so it must still read their files.
    const OLDER_SPEC: &str = r#"{
        "args": ["cargo", "build"],
        "cwd": "/trees/tree-01",
        "env": [["IGNITE_CORES", "4"]],
        "queued_s": 1.5,
        "jobserver": null,
        "build_jobs": null,
        "hang_limit_s": 1800,
        "grant": {"class": "tip", "cores": 4, "resources": [], "asked": 8, "follows": true,
                  "shrinks": false}
    }"#;

    #[test]
    fn a_spec_written_without_a_reserve_parses_and_one_without_writes_none() {
        let spec: SpecFile = serde_json::from_str(OLDER_SPEC).unwrap();
        let grant = spec.grant.clone().unwrap();
        assert_eq!(grant.class, Class::Tip);
        assert_eq!(grant.reserve, None);
        let written = serde_json::to_value(&spec).unwrap();
        assert!(written["grant"].get("reserve").is_none(), "{written}");

        let reserved = GrantFile {
            reserve: Some(Reserve::TipBuild),
            priority_ticket: None,
            ..grant
        };
        let text = serde_json::to_string(&reserved).unwrap();
        assert!(text.contains(r#""reserve":"tip-build""#), "{text}");
        assert_eq!(serde_json::from_str::<GrantFile>(&text).unwrap(), reserved);
    }
}
