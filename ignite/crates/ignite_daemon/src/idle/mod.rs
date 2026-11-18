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
//! Reports for the hub wait in the state file until the hub takes them.

mod audit;
mod explore;
mod rebuild;
mod state;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, TreeId};
use ignite_core::manifest::{Kind, Manifest, Platforms};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::traits::{Clock, Error, Result};

use crate::checks::{Checks, RunOptions};
use crate::server::{Context, Verb};
use crate::trees::Trees;

pub use audit::AuditSummary;
pub use rebuild::RebuildCount;
use state::{Report, State};

/// One piece of idle work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdleJob {
    Explore { family: String },
    Audit,
    Rebuild { name: String },
}

/// Where idle work runs, and the repository facts it needs. [`LiveHost`] is the real one.
pub trait IdleHost: Send + Sync {
    /// Leases a landing tree checked out at `commit`; `None` while every landing tree is busy.
    fn lease(&self, commit: &Commit) -> Result<Option<(TreeId, PathBuf)>>;
    /// Frees a tree that [`IdleHost::lease`] gave; `built_at` is the commit its builds match.
    fn release(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()>;
    /// How many commits `to` has that `from` does not.
    fn commits_between(&self, from: &Commit, to: &Commit) -> Result<usize>;
}

/// Runs one check again in a tree without asking the cache. [`Checks`] is the real one.
pub trait Rerun: Send + Sync {
    /// The check's outcome, and its saved output or failure message if it said anything.
    fn rerun(&self, ctx: &Context, tree: &TreeId, check: &CheckName) -> (Outcome, Option<String>);
}

impl Rerun for Checks {
    fn rerun(&self, ctx: &Context, tree: &TreeId, check: &CheckName) -> (Outcome, Option<String>) {
        let options = RunOptions {
            verb: "audit".to_owned(),
            class: Class::Idle,
            rerun: true,
            ticket: None,
            session: None,
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
        (outcome, said)
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
            .lease_landing("idle work", commit)?
            .map(|lease| (lease.tree, lease.path)))
    }

    fn release(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()> {
        self.trees.release_landing(tree, built_at)
    }

    fn commits_between(&self, from: &Commit, to: &Commit) -> Result<usize> {
        Ok(ignite_git::refs::commits_between(&self.repo, from, to)?.len())
    }
}

/// What the idle work is configured with.
#[derive(Clone, Debug)]
pub struct IdleSettings {
    pub manifest: Manifest,
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
    /// Held while a job runs, so that `ignite rebuild` and the idle loop never edit one tree
    /// at once.
    work: Mutex<()>,
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
    /// families' shares. `None` when there is nothing to do.
    pub fn next_job(&self, ctx: &Context) -> Option<IdleJob> {
        if let Some(journal) = &self.state().journal {
            return Some(IdleJob::Rebuild {
                name: journal.name.clone(),
            });
        }
        let interrupted = {
            let state = self.state();
            self.exploration_families()
                .into_iter()
                .find(|family| state.batches.contains_key(family))
        };
        if let Some(family) = interrupted {
            return Some(IdleJob::Explore { family });
        }
        if self.audits_due() {
            return Some(IdleJob::Audit);
        }
        if let Some(name) = self.rebuild_due(ctx) {
            return Some(IdleJob::Rebuild { name });
        }
        self.next_family().map(|family| IdleJob::Explore { family })
    }

    /// Runs one job, after sending any reports the hub has not taken yet.
    pub fn run(&self, ctx: &Context, job: &IdleJob) -> Result<()> {
        let _work = lock(&self.work);
        self.send_reports(ctx);
        match job {
            IdleJob::Explore { family } => self.explore(ctx, family),
            IdleJob::Audit => self.audit(ctx, false).map(|_| ()),
            IdleJob::Rebuild { name } => self.rebuild(ctx, name).map(|_| ()),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// Changes the state and saves it before returning.
    fn update<T>(&self, change: impl FnOnce(&mut State) -> T) -> Result<T> {
        let mut state = self.state();
        let value = change(&mut state);
        state.save(&self.settings.state)?;
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
        let _work = lock(&self.idle.work);
        match self.idle.rebuild(ctx, name) {
            Ok(Some(count)) => Reply::text(Exit::Success, count.render()),
            Ok(None) => Reply::text(Exit::Conflict, "every landing tree is busy; try again"),
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
        let _work = lock(&self.idle.work);
        self.idle.send_reports(ctx);
        match self.idle.audit(ctx, true) {
            Ok(summary) => Reply::text(Exit::Success, summary.render()),
            Err(error) => error_reply(&error),
        }
    }
}
