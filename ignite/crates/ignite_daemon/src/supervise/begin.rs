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

/// A command whose last runs each used less CPU than this, in milliseconds, is quick: it starts
/// at once on one core instead of waiting for a grant. `uv sync`, which builds the Python
/// environment that most check families list their checks with, used at most 1.2 CPU-seconds
/// and 5 s in 82 runs but waited up to 726 s for a core behind the machine's compiles, and every
/// check family listed in a new tree waited with it.
pub(super) const QUICK_CPU_MS: u64 = 2_000;
/// The recorded runs a command needs before it counts as quick.
const QUICK_RUNS: usize = 5;

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
        if self.quick(spec) {
            return self.start_quick(dir, spec);
        }
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
        // The scheduler reads the CPU the command uses, from the group its supervisor records
        // once it starts, so cores the command leaves idle can go to waiting requests.
        if let Some(scheduler) = &self.scheduler {
            scheduler.watch(grant.id, dir.join(super::GROUP));
        }
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

    /// Whether `spec`'s command starts without a grant ([`is_quick`]); never without a
    /// scheduler, where every command starts at once anyway.
    fn quick(&self, spec: &CommandSpec) -> bool {
        self.scheduler.is_some() && is_quick(&self.cpu, spec)
    }

    /// Starts a quick command at once on one core, without asking the scheduler. Its
    /// environment says one core (`CARGO_BUILD_JOBS=1` among it), so a quick command that
    /// turns out to compile after all uses one core. No grant is recorded for a restarted
    /// daemon to take over.
    fn start_quick(&self, dir: PathBuf, spec: &CommandSpec) -> Launch {
        let grant = Grant {
            id: 0,
            cores: 1,
            resources: Vec::new(),
            queued_s: 0,
            cores_file: None,
        };
        let env = grant.env();
        let held = GrantGuard::new(None, grant);
        match self.start_in(dir, spec, env, 0.0, None, None) {
            Ok(started) => Launch::Started(started, held),
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
            let key = peaks::key(args);
            self.peaks.record(&key, result.usage.peak_mb);
            // At least 1, since a run that measured nothing is not recorded.
            let cpu_ms = (result.usage.cpu_s * 1000.0).round() as u64;
            self.cpu.record(&key, cpu_ms.max(1));
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

/// Whether `spec`'s command is quick ([`QUICK_CPU_MS`]) by the CPU its last runs recorded in
/// `cpu`: each of at least [`QUICK_RUNS`] runs used little CPU, it needs no shared resource,
/// and it is not `idle` work, which must stay stoppable. A command that names Cargo is never
/// quick: a build that found nothing to compile in its last runs can still have everything to
/// compile in its next, and on one core.
fn is_quick(cpu: &peaks::Peaks, spec: &CommandSpec) -> bool {
    spec.resources.is_empty()
        && spec.class != Class::Idle
        && !spec.args.iter().any(|arg| arg.contains("cargo"))
        && cpu.all_below(&peaks::key(&spec.args), QUICK_CPU_MS, QUICK_RUNS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(args: &[&str]) -> CommandSpec {
        CommandSpec {
            args: args.iter().map(|&a| a.to_owned()).collect(),
            cwd: PathBuf::from("."),
            env: Vec::new(),
            class: Class::Agent,
            cores: 12,
            resources: Vec::new(),
            grows: false,
            tree: None,
            ticket: None,
            hang_limit_s: None,
        }
    }

    #[test]
    fn a_command_whose_last_runs_were_quick_starts_without_a_grant() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-quick-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cpu = peaks::Peaks::load(dir.join("cpu.json"));
        let sync = spec(&["uv", "sync", "--quiet"]);
        let key = peaks::key(&sync.args);
        for _ in 0..QUICK_RUNS - 1 {
            cpu.record(&key, 900);
        }
        assert!(!is_quick(&cpu, &sync), "too few runs recorded");
        cpu.record(&key, 1_200);
        assert!(is_quick(&cpu, &sync));

        // Idle work, a shared resource or one slow run keeps the queue.
        assert!(!is_quick(
            &cpu,
            &CommandSpec {
                class: Class::Idle,
                ..sync.clone()
            }
        ));
        let pages = CommandSpec {
            resources: vec!["pages".to_owned()],
            ..sync.clone()
        };
        assert!(!is_quick(&cpu, &pages));
        cpu.record(&key, 40_000);
        assert!(!is_quick(&cpu, &sync));

        // A Cargo build that compiled nothing in its last runs still waits for cores.
        let build = spec(&["cargo", "build", "--quiet"]);
        for _ in 0..QUICK_RUNS {
            cpu.record(&peaks::key(&build.args), 300);
        }
        assert!(!is_quick(&cpu, &build));
        fs::remove_dir_all(&dir).unwrap();
    }
}
