//! Idle work, run in the `idle` class on cores nobody else wants: exploration checks, cache
//! audits and rebuild counts (`docs/ignite.md`, "Exploration checks", "Auditing the cache" and
//! "Rebuild counts").
//!
//! The scheduler, which knows whether the machine is under 70% busy, asks
//! [`Idle::next_job`] for work and runs it with [`Idle::run`]. Every job can be stopped at any
//! moment. A job saves what it has started in the state file (see [`state`]) before it acts, so
//! a job that starts again after a stop gives the result it would have given without one:
//!
//! - exploration saves the checks `next` printed before running them, and runs a saved batch
//!   before asking `next` for more;
//! - an audit marks a hit audited only once the rerun's verdict is saved;
//! - a rebuild saves the edited file's old contents before editing it, and writes them back
//!   before anything else when it finds them saved.
//!
//! Reports for the hub wait in the state file until the hub takes them. A job that fails waits
//! longer before each new attempt, and is reported to the hub after a few failures (see
//! [`backoff`]).
//!
//! The idle loop in the daemon decides whether idle work may run, and each job runs in a verb
//! process of its own as the internal verb `idle-job` ([`IdleJobVerb`], started by [`Idle::drive`]),
//! so a restart of the daemon does not cut it. Several processes then share the state file:
//! every change to it reads the file again under a file lock (see `share`), and a job holds a
//! second file lock for its whole run, so `ignite rebuild`, `ignite audit` and the idle job
//! never edit one tree at once.
//!
//! Holding the trees under their disk limit is not a job: the daemon's disk loop calls
//! [`Idle::hold_floor`] and [`Idle::hold_disk`] every round. The first clears builds whenever the
//! disk is below its floor of free space; the second measures the trees at most every 30
//! minutes, and gives way to the first if the disk falls below the floor during the walk.

mod audit;
mod backoff;
mod disk;
mod explore;
mod launch;
mod rebuild;
mod remote;
mod share;
mod state;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, RecordId, TreeId};
use ignite_core::manifest::{Kind, Platforms};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Error, Result};

use crate::checks::{Checks, RunOptions};
use crate::server::{Context, Verb};
use crate::trees::{DiskReport, Trees};

pub use audit::AuditSummary;
pub use disk::{DISK_EVERY, FLOOR_GB, ROOM_GB};
pub use launch::{IdleJobVerb, JobRunner, JobWait, ServerJobs};
pub use rebuild::RebuildCount;
pub use remote::RemoteIdleHost;
use state::{Report, State};

/// One piece of idle work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdleJob {
    Explore { family: String },
    Audit,
    Rebuild { name: String },
}

impl IdleJob {
    /// The arguments of the `idle-job` request that runs this job.
    pub fn to_args(&self) -> Vec<String> {
        match self {
            IdleJob::Explore { family } => vec!["explore".to_owned(), family.clone()],
            IdleJob::Audit => vec!["audit".to_owned()],
            IdleJob::Rebuild { name } => vec!["rebuild".to_owned(), name.clone()],
        }
    }

    /// The job that [`IdleJob::to_args`] named.
    pub fn from_args(args: &[String]) -> Option<IdleJob> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match args.as_slice() {
            ["explore", family] => Some(IdleJob::Explore {
                family: (*family).to_owned(),
            }),
            ["audit"] => Some(IdleJob::Audit),
            ["rebuild", name] => Some(IdleJob::Rebuild {
                name: (*name).to_owned(),
            }),
            _ => None,
        }
    }
}

/// Where idle work runs, and the repository facts it needs. [`LiveHost`] is the real one.
pub trait IdleHost: Send + Sync {
    /// Leases a landing tree checked out at `commit`, leaving another free for landings and
    /// tip verification; `None` while fewer than two landing trees are free.
    fn lease(&self, commit: &Commit) -> Result<Option<(TreeId, PathBuf)>>;
    /// Frees a tree that [`IdleHost::lease`] gave; `built_at` is the commit its builds match.
    fn release(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()>;
    /// How many commits `to` has that `from` does not.
    fn commits_between(&self, from: &Commit, to: &Commit) -> Result<usize>;
    /// Deletes build directories while the trees are above the measured budget, giving up
    /// partway once `stop` says yes ([`Trees::hold_disk`]).
    fn hold_disk(&self, stop: &dyn Fn() -> bool) -> Result<DiskReport>;
    /// Deletes the build directories of free trees, used longest ago first, until the disk has
    /// `floor_gib` gibibytes free, without measuring the trees ([`Trees::free_disk`]).
    fn free_disk(&self, floor_gib: u64) -> Result<DiskReport>;
    /// Size and available bytes of the disk that holds the trees.
    fn disk_space(&self) -> Option<ignite_os::system::DiskSpace> {
        None
    }
    /// The effective cap, if the hub or legacy configuration sets one.
    fn disk_cap_gb(&self) -> Option<u32> {
        None
    }
}

/// Runs one check again in a tree without asking the cache. [`Checks`] is the real one.
pub trait Rerun: Send + Sync {
    /// The check's outcome, its saved output or failure message if it said anything, and the
    /// hub's ID for the record of the run that gave the outcome, when the hub took it.
    fn rerun(&self, ctx: &Context, tree: &TreeId, check: &CheckName) -> Reran;
}

/// What [`Rerun::rerun`] returns.
pub type Reran = (Outcome, Option<String>, Option<RecordId>);

impl Rerun for Checks {
    fn rerun(&self, ctx: &Context, tree: &TreeId, check: &CheckName) -> Reran {
        let options = RunOptions {
            verb: "audit".to_owned(),
            class: Class::Idle,
            rerun: true,
            ticket: None,
            session: None,
            reserve: None,
        };
        let run = self.run(ctx, tree, std::slice::from_ref(check), &options);
        let outcome = run
            .outcomes
            .iter()
            .find(|(name, _)| name == check)
            .map_or(Outcome::Incomplete, |(_, outcome)| *outcome);
        let said = run
            .failures
            .iter()
            .find(|f| f.check == *check)
            .and_then(|f| f.output.clone().or_else(|| f.message.clone()));
        let record = run
            .records
            .iter()
            .find(|(name, _)| name == check)
            .map(|(_, id)| *id);
        (outcome, said, record)
    }
}

/// The landing trees and the main checkout's history.
pub struct LiveHost {
    pub trees: Arc<Trees>,
    /// The repository's main checkout.
    pub repo: PathBuf,
}

impl IdleHost for LiveHost {
    fn lease(&self, commit: &Commit) -> Result<Option<(TreeId, PathBuf)>> {
        Ok(self
            .trees
            .lease_spare_landing("idle work", commit)?
            .map(|lease| (lease.tree, lease.path)))
    }

    fn release(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()> {
        self.trees.release_landing(tree, built_at)
    }

    fn commits_between(&self, from: &Commit, to: &Commit) -> Result<usize> {
        Ok(ignite_git::refs::commits_between(&self.repo, from, to)?.len())
    }

    fn hold_disk(&self, stop: &dyn Fn() -> bool) -> Result<DiskReport> {
        self.trees.hold_disk(stop)
    }

    fn free_disk(&self, floor_gib: u64) -> Result<DiskReport> {
        let dir = &self.trees.config().dir;
        let floor = floor_gib * ignite_core::capacity::BYTES_PER_GIB;
        self.trees
            .free_disk(floor, || ignite_os::system::disk_free_bytes(dir).ok())
    }

    fn disk_space(&self) -> Option<ignite_os::system::DiskSpace> {
        ignite_os::system::disk_space(&self.trees.config().dir).ok()
    }

    fn disk_cap_gb(&self) -> Option<u32> {
        self.trees.disk_cap_gb()
    }
}

/// What the idle work is configured with.
#[derive(Clone, Debug)]
pub struct IdleSettings {
    /// The manifests, whose exploration families idle work chooses from. The daemon reads them
    /// again when the main checkout moves (see `crate::manifests`).
    pub manifest: crate::manifests::LiveManifest,
    /// This machine's operating system, which decides the families it can run.
    pub os: Os,
    /// The families this machine runs, or `None` for all.
    pub families: Option<Vec<String>>,
    /// The state file.
    pub state: PathBuf,
    /// Where a run's result files are written.
    pub outputs: PathBuf,
    /// The machine's hostname, which exploration reports name.
    pub hostname: String,
    /// Cores an idle command asks for.
    pub cores: u32,
    /// The Ignite commit, for records.
    pub ignite: String,
}

/// The idle work of one machine. See the module documentation.
pub struct Idle {
    settings: IdleSettings,
    clock: Arc<dyn Clock>,
    host: Box<dyn IdleHost>,
    rerun: Arc<dyn Rerun>,
    state: Mutex<State>,
    /// Each family's share of exploration time, as the hub gives them; equal shares until then.
    shares: Mutex<Vec<(String, f64)>>,
    /// Held while a job runs in this process (see [`Idle::work`]).
    work: Mutex<()>,
    /// When the trees' disk use was last measured, which [`Idle::hold_disk`] limits.
    disk_held: Mutex<Option<Timestamp>>,
    /// What the last pass to get the disk above its floor could not delete, which the disk
    /// loop reports until a pass succeeds ([`Idle::hold_floor`]).
    floor_stuck: Mutex<Option<crate::trees::FloorStuck>>,
}

/// The number of commits the verified commit moves before each rebuild edit is measured again.
pub const REBUILD_EVERY: usize = 100;

impl Idle {
    /// The idle work, with the state saved at `settings.state`.
    pub fn new(
        settings: IdleSettings,
        clock: Arc<dyn Clock>,
        host: Box<dyn IdleHost>,
        rerun: Arc<dyn Rerun>,
    ) -> Result<Idle> {
        let state = State::load(&settings.state).unwrap_or_else(|error| {
            // A state file that cannot be read (cut off by a full disk, say) must not keep
            // the daemon from starting: it would exit and be started again by the login
            // service, over and over. The idle work starts over, and the file is kept aside.
            eprintln!("ignite: idle state not read, starting with none: {error}");
            let mut aside = settings.state.as_os_str().to_owned();
            aside.push(".unreadable");
            let _ = std::fs::rename(&settings.state, aside);
            State::default()
        });
        Ok(Idle {
            settings,
            clock,
            host,
            rerun,
            state: Mutex::new(state),
            shares: Mutex::new(Vec::new()),
            work: Mutex::new(()),
            disk_held: Mutex::new(None),
            floor_stuck: Mutex::new(None),
        })
    }

    /// Sets each exploration family's share of idle time.
    pub fn set_shares(&self, shares: Vec<(String, f64)>) {
        *lock(&self.shares) = shares;
    }

    /// The exploration families this machine runs, by name.
    pub fn exploration_families(&self) -> Vec<String> {
        self.settings
            .manifest
            .get()
            .families
            .iter()
            .filter(|(_, declared)| {
                declared.family.kind == Kind::Exploration && declared.family.next.is_some()
            })
            .filter(|(name, _)| {
                self.settings
                    .families
                    .as_ref()
                    .is_none_or(|list| list.contains(name))
            })
            .filter(|(_, declared)| match &declared.family.platforms {
                Platforms::Any(_) => true,
                Platforms::Each(list) => list.contains(&self.settings.os),
            })
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// The next idle job: interrupted work first (an applied rebuild edit, then an unfinished
    /// exploration batch), then a due audit, then a due rebuild count, then exploration in the
    /// families' shares. A job still waiting after a failure is passed over, and while an
    /// applied rebuild edit waits, nothing runs. `None` when there is nothing to do.
    pub fn next_job(&self, ctx: &Context) -> Option<IdleJob> {
        let applied = self
            .state()
            .journal
            .as_ref()
            .map(|journal| IdleJob::Rebuild {
                name: journal.name.clone(),
            });
        if let Some(job) = applied {
            // Other work could lease the edited tree, so it waits until the edit is undone.
            return self.ready(&job).then_some(job);
        }
        let interrupted = {
            let families = self.exploration_families();
            let state = self.state();
            families
                .into_iter()
                .filter(|family| state.batches.contains_key(family))
                .collect::<Vec<_>>()
        };
        if let Some(job) = interrupted
            .into_iter()
            .map(|family| IdleJob::Explore { family })
            .find(|job| self.ready(job))
        {
            return Some(job);
        }
        if self.audits_due() && self.ready(&IdleJob::Audit) {
            return Some(IdleJob::Audit);
        }
        if let Some(name) = self.rebuild_due(ctx) {
            return Some(IdleJob::Rebuild { name });
        }
        self.next_family().map(|family| IdleJob::Explore { family })
    }

    /// Runs one job, after sending any reports the hub has not taken yet. A failure makes the
    /// job wait before it runs again; a job that ran and succeeded is cleared of its failures.
    pub fn run(&self, ctx: &Context, job: &IdleJob) -> Result<()> {
        let _work = self.work();
        self.send_reports(ctx);
        let ran = match job {
            IdleJob::Explore { family } => self.explore(ctx, family),
            IdleJob::Audit => self.audit(ctx, false).map(|_| true),
            IdleJob::Rebuild { name } => self.rebuild(ctx, name).map(|count| count.is_some()),
        };
        match ran {
            Ok(true) => self.note_success(job),
            // No landing tree was free, so the job did not run.
            Ok(false) => Ok(()),
            Err(error) => {
                if let Err(unsaved) = self.note_failure(ctx, job, &error) {
                    eprintln!("ignite: the failure of idle work {job:?} was not saved: {unsaved}");
                }
                Err(error)
            }
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Reads the state file again, because another process (the idle job in its verb process,
    /// `ignite rebuild`, a test run that sampled a cache hit) may have changed it. A file that
    /// cannot be read leaves the state as it was.
    fn refresh(&self) {
        if let Ok(state) = State::load(&self.settings.state) {
            *self.state() = state;
        }
    }

    /// Takes the right to run a job: held for the job's whole run, in this process and in every
    /// other (see the module documentation), so that `ignite rebuild`, `ignite audit` and the
    /// idle job never edit one tree at once. The state is read again once it is held, because
    /// the job that held it before may have changed it.
    fn work(&self) -> Work<'_> {
        let guard = lock(&self.work);
        let file = share::lock(&self.settings.state, "job-lock");
        self.refresh();
        Work {
            _guard: guard,
            _file: file,
        }
    }

    /// Changes the state and saves it before returning.
    fn update<T>(&self, change: impl FnOnce(&mut State) -> T) -> Result<T> {
        let changed = self.update_if(|state| Some(change(state)))?;
        Ok(changed.expect("the change always saves"))
    }

    /// Reads the state file under its file lock, applies `change` and saves the result, unless
    /// `change` returns `None`, which leaves the file alone.
    fn update_if<T>(&self, change: impl FnOnce(&mut State) -> Option<T>) -> Result<Option<T>> {
        let _file = share::lock(&self.settings.state, "state-lock");
        self.refresh();
        let mut state = self.state();
        let value = change(&mut state);
        if value.is_some() {
            state.save(&self.settings.state)?;
        }
        Ok(value)
    }

    /// Sends the waiting reports, oldest first, until the hub cannot be reached or does not
    /// answer. A report that the hub rejects as malformed is dropped.
    fn send_reports(&self, ctx: &Context) {
        loop {
            let Some(report) = self.state().unsent.first().cloned() else {
                return;
            };
            let sent = match &report {
                Report::Audit(audit) => ctx.hub.audit(audit),
                Report::Find(find) => ctx.hub.find(find),
            };
            match sent {
                Ok(_) => {}
                Err(Error::Invalid(reason)) => {
                    // The hub will never take this report as it is, and every later report
                    // waits behind it, so it goes.
                    eprintln!("ignite: a report the hub rejects is dropped: {reason}");
                }
                Err(_) => return,
            }
            let removed = self.update(|state| {
                if state.unsent.first() == Some(&report) {
                    state.unsent.remove(0);
                }
            });
            if removed.is_err() {
                return;
            }
        }
    }

    /// The commit idle work runs at: the verified branch's newest commit.
    fn verified(ctx: &Context) -> Result<Commit> {
        ctx.git.tip(&ctx.config.repository.master)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The right to run a job, from [`Idle::work`].
struct Work<'a> {
    _guard: MutexGuard<'a, ()>,
    _file: Option<share::FileLock>,
}

/// The error for idle work whose build failed with its whole output in `output`; `failed`
/// says which build failed and how, such as "rust-tests failed to build before the edit".
///
/// A build the scheduler stopped to give its cores to a higher class, or that a daemon restart
/// cut, ended by a signal. That says nothing about the source, so it is a
/// [`Error::Conflict`], which [`backoff`] does not count as a failure of the job: the job runs
/// again on the next idle round. Before this, every landing that stopped an idle rebuild
/// counted as a failure, and three in a row filed a justiciar report against a job that had
/// nothing wrong with it.
fn build_failure(failed: &str, output: &Path) -> Error {
    if crate::supervise::ResultFile::ended_from_outside(output) {
        Error::Conflict(format!(
            "{failed}: the build was stopped before it finished; its output is in {}",
            output.display()
        ))
    } else {
        Error::Io(format!("{failed}; its output is in {}", output.display()))
    }
}

/// The exit code for an error from idle work.
fn error_reply(error: &Error) -> Reply {
    let exit = match error {
        Error::Unreachable(_) => Exit::Unreachable,
        Error::Conflict(_) => Exit::Conflict,
        _ => Exit::Usage,
    };
    Reply::text(exit, error.to_string())
}

/// `ignite rebuild <name>` measures a named edit's rebuild count now; `ignite rebuild --list`
/// lists the edits and `ignite rebuild --last` prints the stored counts.
pub struct RebuildVerb {
    pub idle: Arc<Idle>,
}

impl Verb for RebuildVerb {
    fn name(&self) -> &'static str {
        "rebuild"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let flags: Vec<&str> = req.args.iter().map(String::as_str).collect();
        let name = flags.iter().find(|a| !a.starts_with("--")).copied();
        if flags.contains(&"--list") {
            return Reply::text(Exit::Success, rebuild::list(&ctx.config.rebuild));
        }
        if flags.contains(&"--last") {
            return Reply::text(Exit::Success, self.idle.last_counts(name));
        }
        let Some(name) = name else {
            return Reply::text(
                Exit::Usage,
                "usage: ignite rebuild <name> | --list | --last",
            );
        };
        let _work = self.idle.work();
        match self.idle.rebuild(ctx, name) {
            Ok(Some(count)) => {
                // A measurement that works clears the idle job's failures, so it runs as due.
                let job = IdleJob::Rebuild {
                    name: name.to_owned(),
                };
                if let Err(error) = self.idle.note_success(&job) {
                    eprintln!("ignite: idle state not saved: {error}");
                }
                Reply::text(Exit::Success, count.render())
            }
            Ok(None) => Reply::text(
                Exit::Conflict,
                "no landing tree is free beside the one kept for landings; try again",
            ),
            Err(error) => error_reply(&error),
        }
    }
}

/// `ignite audit --now` reruns the sampled cache hits at once.
pub struct AuditVerb {
    pub idle: Arc<Idle>,
}

impl Verb for AuditVerb {
    fn name(&self) -> &'static str {
        "audit"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        if !req.args.iter().any(|a| a == "--now") {
            return Reply::text(Exit::Usage, "usage: ignite audit --now");
        }
        let _work = self.idle.work();
        self.idle.send_reports(ctx);
        match self.idle.audit(ctx, true) {
            Ok(summary) => Reply::text(Exit::Success, summary.render()),
            Err(error) => error_reply(&error),
        }
    }
}
