//! Build trees: the machine's fixed set of ticket trees and landing trees, leased by ticket,
//! kept built near the tip and held under the machine's disk limit.
//!
//! Cargo's incremental builds are only reused at the path they were made at, so each tree keeps
//! its build directories between tickets, and a compiling lease starts from a copy of the build
//! of the tree nearest the tip when that is nearer than its own ([`seed`]). Which directories those are comes from the build
//! products' `outputs` in the check manifests; no directory name is written here. Ignite never
//! builds anywhere but these trees: when every ticket tree is leased, a caller gets a [`Waiter`]
//! that says why, and the next release serves it.
//!
//! A tree's state lives in the daemon's local store (`ignite_store::local::trees`): `free`,
//! `prebuilding`, `leased`, `landing` or `resetting`. Only a free tree is ever leased.
//!
//! A landing tree leased by a verb with a handle records that handle as its owner. When the
//! daemon restarts, a landing tree whose owner is still running ([`owner_alive`]) is kept as it
//! is, with the commands in it, for the verb the new daemon runs again under that handle; every
//! other tree left in the middle of something is reset ([`reset_at_start`]). The daemon marks
//! those trees `resetting` before it serves its socket and resets them on a thread of its own
//! afterwards ([`Trees::mark_unfinished`], [`Trees::finish_in_background`]), so a restart is not
//! held up by `git`; a lease that needs one of them waits for it like any other.
//!
//! Releasing a ticket tree saves its work first ([`ignite_git::save`]) and fails, leaving the
//! tree leased and untouched, if that push fails. It then stops every verb still running from
//! the tree (a landing, a test) and waits for each to end, ends every process inside the tree
//! except the caller and its ancestors, resets the tree to a clean checkout that keeps the build
//! directories, and serves the first waiter. A verb that refuses to stop (a landing whose push
//! began) or does not end in time leaves the tree leased, and the release says why.

mod branch;
mod choice;
mod disk;
pub mod head;
mod landing;
mod pressure;
mod reset;
mod retire;
mod seed;
mod setup;
#[cfg(test)]
mod tests;
mod wait;

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::manifests::Live;
use ignite_core::config::{Config, Machine};
use ignite_core::ids::{Commit, Handle, SessionId, TicketId, TreeId};
use ignite_core::manifest::Manifest;
use ignite_core::traits::{BuildResult, Builds, Clock, Error, Result};
use ignite_git::fastforward::{self, FastForward};
use ignite_git::save::{self, Saved};
use ignite_git::{refs, worktree};
use ignite_store::local::{handles as handle_store, rows, trees as store};
use rusqlite::Connection;
use serde::Serialize;

pub use disk::DiskReport;
pub use seed::{Seeding, wait_for_seed_on_disk, wait_until_seeded};
pub use setup::Unfinished;
use wait::Slot;
pub use wait::Waiter;

/// The trailer on the commit that [`Trees::release`] makes of uncommitted work, so the next
/// start of the ticket can say that its branch ends in unfinished work saved at release.
pub const SAVED_TRAILER: &str = "Ignite-Saved: release";

/// How long a process inside a released tree has to exit after `SIGTERM` before `SIGKILL`.
const GRACE: Duration = Duration::from_secs(3);

/// The local branch a tree sits on while no ticket holds it.
fn park_branch(tree: &TreeId) -> String {
    format!("ignite-tree/{tree}")
}

/// What one machine's trees are, from `ignite.toml` and the check manifests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeConfig {
    /// The repository's main checkout.
    pub repo: PathBuf,
    /// The directory that holds every tree.
    pub dir: PathBuf,
    pub remote: String,
    /// The prefix of the remote ref that holds a released ticket's work (`repository.saved`).
    pub saved: String,
    /// The main checkout's local branches kept fast-forwarded to the remote.
    pub branches: Vec<String>,
    pub ticket_trees: u32,
    pub landing_trees: u32,
    /// The disk the trees may use, in bytes.
    pub disk_limit: Option<u64>,
    /// The build products' output directories, relative to a tree's root: what a reset keeps.
    /// Both lists change when the manifests do ([`TreeConfig::reload`]).
    pub keep: Live<Vec<String>>,
    /// The products built at the lease of a ticket expected to compile, and kept built at the
    /// tip in one free tree: those whose inputs name workspace packages.
    pub prebuild: Live<Vec<String>>,
    /// Each prebuild product's output directories, which order a lease's prebuild by the
    /// ticket's files ([`TreeConfig::prebuild_order`]).
    pub prebuild_outputs: Live<BTreeMap<String, Vec<String>>>,
}

impl TreeConfig {
    /// The remote ref that holds a released ticket's work: the `saved` prefix and the ticket.
    pub fn save_branch(&self, ticket: &TicketId) -> String {
        format!("{}{ticket}", self.saved)
    }

    pub fn new(
        repo: &Path,
        config: &Config,
        machine: &Machine,
        landing_trees: u32,
        manifest: &Manifest,
    ) -> Self {
        let (keep, prebuild, prebuild_outputs) = Self::products(manifest);
        TreeConfig {
            repo: repo.to_path_buf(),
            dir: repo.join(&config.repository.trees),
            remote: config.repository.remote.clone(),
            saved: config.repository.saved_prefix().to_owned(),
            // `green`, when set, is fast-forwarded too, so that work started from it locally
            // sees where it is. A branch missing locally is skipped.
            branches: [
                Some(config.repository.staging.clone()),
                Some(config.repository.master.clone()),
                config.repository.green.clone(),
            ]
            .into_iter()
            .flatten()
            .collect(),
            ticket_trees: machine.ticket_trees,
            landing_trees,
            disk_limit: machine
                .trees_disk_gb
                .map(|gb| u64::from(gb) * 1_000_000_000),
            keep: keep.into(),
            prebuild: prebuild.into(),
            prebuild_outputs: prebuild_outputs.into(),
        }
    }

    /// Every tree this machine has, ticket trees first: `tree-01`… and `landing-1`….
    fn configured(&self) -> Vec<(TreeId, Kind)> {
        let ticket = (1..=self.ticket_trees).map(|n| (format!("tree-{n:02}"), Kind::Ticket));
        let landing = (1..=self.landing_trees).map(|n| (format!("landing-{n}"), Kind::Landing));
        ticket
            .chain(landing)
            .map(|(id, kind)| (TreeId::new(id).expect("a tree ID is a token"), kind))
            .collect()
    }

    fn path(&self, tree: &TreeId) -> PathBuf {
        self.dir.join(tree.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Ticket,
    Landing,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Ticket => "ticket",
            Kind::Landing => "landing",
        }
    }
}

/// A leased tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeLease {
    pub tree: TreeId,
    pub path: PathBuf,
    pub branch: String,
    pub at: Commit,
}

/// What a ticket asks of [`Trees::lease`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseRequest {
    pub ticket: TicketId,
    pub session: Option<SessionId>,
    /// The branch the tree checks out.
    pub branch: String,
    /// The commit the branch starts at: the tip, or the head of the ticket's saved work.
    pub at: Commit,
    /// The integration branch's newest commit, which builds are measured against.
    pub tip: Commit,
    /// Whether the ticket is expected to compile, which picks the tree and starts its builds.
    pub compiles: bool,
    /// The files the ticket names, relative to the checkout's root: a compiling ticket prefers
    /// a tree that holds the build directories beside them ([`choice`]).
    pub files: Vec<String>,
}

/// How [`Trees::lease`] answered.
#[derive(Debug)]
pub enum Lease {
    Ready(TreeLease),
    /// Every ticket tree is leased; the waiter is served by the next release.
    Waiting(Waiter),
}

/// A process that a release ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ended {
    pub pid: u32,
    pub executable: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    /// Whether it ignored `SIGTERM` and needed `SIGKILL`.
    pub killed: bool,
}

/// What [`Trees::release`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Released {
    pub tree: TreeId,
    pub saved: Saved,
    pub ended: Vec<Ended>,
}

/// One tree, for `ignite status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TreeStatus {
    pub tree: TreeId,
    pub kind: Kind,
    pub state: String,
    pub path: PathBuf,
    pub ticket: Option<String>,
    pub session: Option<String>,
    pub branch: Option<String>,
    pub built_commit: Option<String>,
    pub last_used: Option<String>,
}

/// The data behind `ignite status` for trees.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Status {
    pub trees: Vec<TreeStatus>,
    /// The tickets waiting for a tree, first served first.
    pub waiting: Vec<TicketId>,
}

/// The machine's build trees.
#[derive(Clone)]
pub struct Trees {
    inner: Arc<Inner>,
}

struct Inner {
    config: TreeConfig,
    clock: Arc<dyn Clock>,
    builds: Arc<dyn Builds>,
    /// The running verbs, which are stopped before their tree is freed (see `crate::steps`).
    steps: Arc<crate::steps::Steps>,
    /// How long freeing a tree waits for its running verbs to end once asked to stop.
    stop_wait: Duration,
    /// Free disk requested by a blocked handoff, in GB as reported by the disk loop.
    handoff_disk_target: AtomicU64,
    /// The store and the waiters, under one lock so a release cannot slip between a lease
    /// finding no free tree and its waiter joining the queue.
    locked: Mutex<Locked>,
}

struct Locked {
    db: Connection,
    waiters: VecDeque<Arc<Slot>>,
}

impl std::fmt::Debug for Trees {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Trees")
            .field("config", &self.inner.config)
            .finish_non_exhaustive()
    }
}

fn store_error(error: &ignite_store::StoreError) -> Error {
    Error::Io(format!("local store: {error}"))
}

/// Whether the verb that leased a landing tree under the handle `owner` will still use it: its
/// handle is `running` in the local database, and, for a verb that runs in a verb process (see
/// `server/process.rs`), the process still holds `verbs/<handle>/lock` in the Ignite home or the
/// daemon runs the verb again once it finds the process dead. At start that is a verb process
/// that outlived the previous daemon, or a verb the previous daemon ran (or whose process died)
/// that this one runs again under the same handle (see `server/resume.rs`); while the daemon
/// runs, a verb that has not finished.
///
/// This is the only place that decides it. A handle the restarted daemon finds running but
/// cannot run again is ended `incomplete` just after start; its tree is freed by the next
/// landing lease ([`Trees::lease_landing`]).
///
/// A verb process that still holds its lock is alive whatever its handle's row says: a daemon
/// that took a hung process for dead and ended its handle must not give its trees to the next
/// verb while the process, and the commands it runs, still work in them.
pub fn owner_alive(db: &Connection, owner: &str) -> bool {
    owner_alive_in(db, owner, &crate::update::Home::from_env().root)
}

/// [`owner_alive`], with the verb processes' locks in the Ignite home `home`.
pub fn owner_alive_in(db: &Connection, owner: &str, home: &Path) -> bool {
    let handle = Handle::new(owner).ok();
    if handle
        .as_ref()
        .is_some_and(|handle| crate::verbproc::alive(home, handle))
    {
        return true;
    }
    let Ok(Some(row)) = handle_store::get(db, owner) else {
        return false;
    };
    if row.state != "running" {
        return false;
    }
    // A verb run by the daemon itself names no process.
    row.pid.is_none() || crate::server::RUN_AGAIN_AFTER_DEATH.contains(&row.verb.as_str())
}

/// Whether a restarted daemon resets `row`'s tree: it was being prebuilt or reset, or it is a
/// landing tree whose owner is not alive (a tip verification or idle work, which start over, or
/// a verb that finished or will not run again). Leased ticket trees and free trees are not.
fn resets_at_start(db: &Connection, row: &rows::Tree) -> bool {
    let unfinished = [
        store::State::Prebuilding,
        store::State::Resetting,
        store::State::Landing,
    ]
    .iter()
    .any(|state| row.state == state.as_str());
    let kept = row.state == store::State::Landing.as_str()
        && row
            .owner
            .as_deref()
            .is_some_and(|owner| owner_alive(db, owner));
    unfinished && !kept
}

/// The directories of the trees that [`Trees::setup`] resets when a daemon starts, so that the
/// commands still running in them can be ended first (see `crate::adopt`).
pub fn reset_at_start(db: &Connection) -> Result<Vec<PathBuf>> {
    let rows = store::all(db).map_err(|e| store_error(&e))?;
    Ok(rows
        .into_iter()
        .filter(|row| resets_at_start(db, row))
        .map(|row| PathBuf::from(row.path))
        .collect())
}

impl Trees {
    /// How many landing trees this machine has.
    pub fn landing_count(&self) -> usize {
        usize::try_from(self.inner.config.landing_trees).unwrap_or(usize::MAX)
    }

    pub fn new(
        config: TreeConfig,
        db: Connection,
        clock: Arc<dyn Clock>,
        builds: Arc<dyn Builds>,
    ) -> Self {
        Trees {
            inner: Arc::new(Inner {
                config,
                clock,
                builds,
                steps: crate::steps::global(),
                stop_wait: crate::stop::STOP_WAIT,
                handoff_disk_target: AtomicU64::new(0),
                locked: Mutex::new(Locked {
                    db,
                    waiters: VecDeque::new(),
                }),
            }),
        }
    }

    pub fn config(&self) -> &TreeConfig {
        &self.inner.config
    }

    fn lock(&self) -> MutexGuard<'_, Locked> {
        self.inner
            .locked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> String {
        self.inner.clock.now().to_string()
    }

    /// Leases a free ticket tree and checks the request's branch out in it. A ticket expected
    /// to compile gets the free tree whose last build is nearest the tip, and its builds start
    /// at once on a thread of their own; any other ticket gets the free tree with the oldest
    /// build, leaving the prebuilt trees to tickets that need them.
    pub fn lease(&self, request: LeaseRequest) -> Result<Lease> {
        let tree = {
            let mut locked = self.lock();
            if let Some(tree) = self.take_free(&locked, &request)? {
                tree
            } else {
                let slot = Arc::new(Slot::new(request, self.busy_reason(&locked)?));
                locked.waiters.push_back(Arc::clone(&slot));
                return Ok(Lease::Waiting(Waiter::new(slot, self.clone())));
            }
        };
        let lease = self.check_out(&tree, &request)?;
        Ok(Lease::Ready(lease))
    }

    /// Picks a free ticket tree for `request` and marks it leased in the store.
    fn take_free(&self, locked: &Locked, request: &LeaseRequest) -> Result<Option<TreeId>> {
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        let configured = self.inner.config.configured();
        let keep = self.inner.config.keep.get();
        let wanted = choice::wanted(&keep, &request.files);
        let mut free: Vec<(usize, Option<usize>, &rows::Tree)> = rows
            .iter()
            .filter(|row| {
                row.state == store::State::Free.as_str()
                    && configured
                        .iter()
                        .any(|(id, kind)| id.as_str() == row.id && *kind == Kind::Ticket)
            })
            .map(|row| {
                let path = self.inner.config.dir.join(&row.id);
                (
                    choice::missing(&path, &wanted),
                    self.distance(row.built_commit.as_deref(), &request.tip),
                    row,
                )
            })
            .collect();
        // Fewest wanted build directories missing first; `None` (never built) sorts after every
        // distance.
        let key = |(missing, distance, row): &(usize, Option<usize>, &rows::Tree)| {
            (*missing, distance.unwrap_or(usize::MAX), row.id.clone())
        };
        free.sort_by_key(key);
        let chosen = if request.compiles {
            free.first()
        } else {
            free.iter().max_by_key(|entry| {
                (
                    entry.1.unwrap_or(usize::MAX),
                    std::cmp::Reverse(&entry.2.id),
                )
            })
        };
        let Some((_, _, row)) = chosen else {
            return Ok(None);
        };
        let taken = store::lease(
            &locked.db,
            &row.id,
            store::State::Leased,
            Some(request.ticket.as_str()),
            request.session.as_ref().map(SessionId::as_str),
            Some(&request.branch),
            &self.now(),
        )
        .map_err(|e| store_error(&e))?;
        if !taken {
            return Err(Error::Conflict(format!("tree {} was taken", row.id)));
        }
        Ok(Some(
            TreeId::new(row.id.clone()).map_err(|e| Error::Invalid(e.to_string()))?,
        ))
    }

    /// How many commits the tip has that a build at `built` lacks; `None` when the tree was
    /// never built or the commit is unknown.
    fn distance(&self, built: Option<&str>, tip: &Commit) -> Option<usize> {
        let built = Commit::new(built?).ok()?;
        refs::commits_between(&self.inner.config.repo, &built, tip)
            .ok()
            .map(|commits| commits.len())
    }

    /// Why a lease must wait: who holds each ticket tree.
    fn busy_reason(&self, locked: &Locked) -> Result<String> {
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        let holders: Vec<String> = rows
            .iter()
            .filter(|row| row.kind == Kind::Ticket.as_str())
            .map(|row| match &row.ticket {
                Some(ticket) => format!("{} {}", row.id, ticket),
                None => format!("{} {}", row.id, row.state),
            })
            .collect();
        Ok(format!(
            "all {} ticket trees are in use ({}); waiting behind {} other tickets",
            self.inner.config.ticket_trees,
            holders.join(", "),
            locked.waiters.len()
        ))
    }

    /// Checks the request's branch out in a tree just marked leased and starts its builds. On
    /// failure the tree is made free again.
    fn check_out(&self, tree: &TreeId, request: &LeaseRequest) -> Result<TreeLease> {
        let config = &self.inner.config;
        let path = config.path(tree);
        // A tree given back while its last lease's copy ran may be leased again before the copy ends.
        let _ = seed::wait_until_seeded(&path);
        // Saved work that is only an ancestor of the tip has no commits of its own (for example
        // a branch left by an attempt that changed nothing). Checking it out would leave the
        // tree on an old commit while the ticket's brief names the tip, so start at the tip.
        let at = if request.at != request.tip
            && refs::is_ancestor(&config.repo, &request.at, &request.tip).unwrap_or(false)
        {
            request.tip.clone()
        } else {
            request.at.clone()
        };
        if let Err(error) = self.check_out_branch(&path, &request.branch, &at) {
            let _ = store::release(&self.lock().db, tree.as_str(), None, &self.now());
            return Err(error);
        }
        if request.compiles {
            self.start_builds(tree, request);
        }
        Ok(TreeLease {
            tree: tree.clone(),
            path,
            branch: request.branch.clone(),
            at,
        })
    }

    /// Makes every prebuild product current in `tree`, first those beside `files` (see
    /// [`TreeConfig::prebuild_order`]), and, if all succeed, records `at` as the commit its build
    /// directories match.
    fn build(&self, tree: &TreeId, at: &Commit, files: &[String]) -> bool {
        let built = self
            .inner
            .config
            .prebuild_order(files)
            .iter()
            .all(|product| {
                !matches!(
                    self.inner.builds.ensure(tree, product, &[]),
                    BuildResult::Failed { .. }
                )
            });
        if built {
            let _ = self.record_build(tree, at);
        }
        built
    }

    /// Records that `tree`'s build directories now match `commit`.
    pub fn record_build(&self, tree: &TreeId, commit: &Commit) -> Result<()> {
        let locked = self.lock();
        let Some(mut row) = store::get(&locked.db, tree.as_str()).map_err(|e| store_error(&e))?
        else {
            return Err(Error::NotFound(format!("tree {tree}")));
        };
        row.built_commit = Some(commit.to_string());
        store::upsert(&locked.db, &row).map_err(|e| store_error(&e))
    }

    /// Gives a ticket tree back: saves its work to the ticket's branch on the remote, ends the
    /// processes inside it, resets it to `park_at` keeping its build directories, and serves
    /// the first waiter. `caller` is the process asking, which is spared with its ancestors, as
    /// is the daemon itself. If the save fails, nothing else happens and the tree stays leased.
    pub fn release(
        &self,
        tree: &TreeId,
        park_at: &Commit,
        caller: Option<u32>,
    ) -> Result<Released> {
        let config = &self.inner.config;
        let row = self.row(tree)?;
        if row.state != store::State::Leased.as_str() {
            return Err(Error::Conflict(format!(
                "tree {tree} is {}, not leased",
                row.state
            )));
        }
        let ticket = row
            .ticket
            .as_deref()
            .map(TicketId::new)
            .transpose()
            .map_err(|e| Error::Invalid(e.to_string()))?
            .ok_or_else(|| Error::Invalid(format!("tree {tree} is leased to no ticket")))?;
        let path = config.path(tree);
        let message = format!("wip: {ticket} saved at release\n\n{SAVED_TRAILER}");
        let saved = save::save(
            &path,
            &config.remote,
            &config.save_branch(&ticket),
            &message,
        )?;
        let ended = self.reset_leased(tree, &row, park_at, caller)?;
        Ok(Released {
            tree: tree.clone(),
            saved,
            ended,
        })
    }

    /// Gives a ticket tree back without saving its work: ends the processes inside it, resets
    /// it to `park_at` keeping its build directories, and serves the first waiter. For a tree
    /// whose work is already saved or landed, or a claim that was refused.
    pub fn release_unsaved(
        &self,
        tree: &TreeId,
        park_at: &Commit,
        caller: Option<u32>,
    ) -> Result<Vec<Ended>> {
        let row = self.row(tree)?;
        if row.state != store::State::Leased.as_str() {
            return Err(Error::Conflict(format!(
                "tree {tree} is {}, not leased",
                row.state
            )));
        }
        self.reset_leased(tree, &row, park_at, caller)
    }

    /// Leases free trees to waiters, first come first served, while both remain.
    fn serve_waiters(&self) {
        loop {
            let (slot, tree) = {
                let mut locked = self.lock();
                let Some(slot) = locked.waiters.front().cloned() else {
                    return;
                };
                match self.take_free(&locked, slot.request()) {
                    Ok(Some(tree)) => {
                        locked.waiters.pop_front();
                        (slot, tree)
                    }
                    Ok(None) | Err(_) => return,
                }
            };
            let result = self.check_out(&tree, slot.request());
            if !slot.serve(result) {
                // The waiter gave up while its tree was being checked out.
                self.give_back(&tree);
            }
        }
    }

    /// Frees a tree that was leased to a waiter who never came for it, without saving or
    /// resetting it, and serves the next waiter.
    pub fn abandon(&self, lease: &TreeLease) {
        self.give_back(&lease.tree);
    }

    /// Frees a tree that was leased but never used, without saving or resetting it.
    fn give_back(&self, tree: &TreeId) {
        let _ = seed::wait_until_seeded(&self.inner.config.path(tree));
        let _ = store::release(&self.lock().db, tree.as_str(), None, &self.now());
        self.serve_waiters();
    }

    /// Removes a waiter that gives up. Returns false if it was already taken off the queue to
    /// be served.
    fn forget(&self, slot: &Arc<Slot>) -> bool {
        let mut locked = self.lock();
        let before = locked.waiters.len();
        locked.waiters.retain(|s| !Arc::ptr_eq(s, slot));
        locked.waiters.len() != before
    }

    /// While cores are idle, keeps one free ticket tree built at `tip`: if none is, prebuilds
    /// the free tree nearest the tip. Returns the tree it built, if any.
    pub fn keep_one_built(&self, tip: &Commit) -> Result<Option<TreeId>> {
        if self.inner.config.prebuild.get().is_empty() {
            return Ok(None);
        }
        let tree = {
            let locked = self.lock();
            let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
            // A tree `ignite.toml` no longer declares is about to be removed, not built.
            let configured = self.inner.config.configured();
            let free = |row: &&rows::Tree| {
                row.kind == Kind::Ticket.as_str()
                    && row.state == store::State::Free.as_str()
                    && configured.iter().any(|(id, _)| id.as_str() == row.id)
            };
            if rows
                .iter()
                .filter(free)
                .any(|row| row.built_commit.as_deref() == Some(tip.as_str()))
            {
                return Ok(None);
            }
            let Some(row) = rows.iter().filter(free).min_by_key(|row| {
                self.distance(row.built_commit.as_deref(), tip)
                    .unwrap_or(usize::MAX)
            }) else {
                return Ok(None);
            };
            let id = row.id.clone();
            let now = self.now();
            if !store::lease(
                &locked.db,
                &id,
                store::State::Prebuilding,
                None,
                None,
                None,
                &now,
            )
            .map_err(|e| store_error(&e))?
            {
                return Ok(None);
            }
            TreeId::new(id).map_err(|e| Error::Invalid(e.to_string()))?
        };
        crate::steps::step(format!("prebuilding {tree} at {}", &tip.as_str()[..7]));
        let path = self.inner.config.path(&tree);
        let reset = worktree::reset(
            &path,
            &park_branch(&tree),
            tip,
            &self.inner.config.keep.get(),
        );
        let built = reset.is_ok() && self.build(&tree, tip, &[]);
        store::release(&self.lock().db, tree.as_str(), None, &self.now())
            .map_err(|e| store_error(&e))?;
        self.serve_waiters();
        reset?;
        Ok(built.then_some(tree))
    }

    /// The tree that holds `path`, if any: a configured tree, or one that `ignite.toml` no
    /// longer declares but the store still holds, so that a session leased it before the count
    /// was lowered can go on and release it.
    pub fn tree_at(&self, path: &Path) -> Option<TreeId> {
        let real = |p: &Path| ignite_os::paths::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let path = real(path);
        let stored = store::all(&self.lock().db).unwrap_or_default();
        self.inner
            .config
            .configured()
            .into_iter()
            .map(|(id, _)| id)
            .chain(
                stored
                    .into_iter()
                    .filter_map(|row| TreeId::new(row.id).ok()),
            )
            .find(|id| path.starts_with(real(&self.inner.config.path(id))))
    }

    /// The session that holds a ticket tree's lease.
    pub fn holder(&self, tree: &TreeId) -> Result<Option<SessionId>> {
        let row = self.row(tree)?;
        if row.state != store::State::Leased.as_str() {
            return Ok(None);
        }
        row.session
            .map(SessionId::new)
            .transpose()
            .map_err(|e| Error::Invalid(e.to_string()))
    }

    /// The fence: a command run from `cwd` inside a tree may go on only for the session that
    /// holds the tree's lease. Returns the tree, `None` outside every tree, or
    /// [`Error::Conflict`] (exit code 3) for any other session or a tree not leased to one.
    pub fn fence(&self, cwd: &Path, session: Option<&SessionId>) -> Result<Option<TreeId>> {
        let Some(tree) = self.tree_at(cwd) else {
            return Ok(None);
        };
        match (self.holder(&tree)?, session) {
            (Some(holder), Some(session)) if holder == *session => Ok(Some(tree)),
            (Some(holder), _) => Err(Error::Conflict(format!(
                "tree {tree} is leased to session {holder}, not this one"
            ))),
            (None, _) => Err(Error::Conflict(format!(
                "tree {tree} is not leased to any session"
            ))),
        }
    }

    /// The data behind `ignite status` for trees.
    pub fn status(&self) -> Result<Status> {
        let locked = self.lock();
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        let configured = self.inner.config.configured();
        let mut trees = Vec::new();
        for (id, kind) in configured {
            let Some(row) = rows.iter().find(|row| row.id == id.as_str()) else {
                continue;
            };
            trees.push(TreeStatus {
                path: self.inner.config.path(&id),
                tree: id,
                kind,
                state: row.state.clone(),
                ticket: row.ticket.clone(),
                session: row.session.clone(),
                branch: row.branch.clone(),
                built_commit: row.built_commit.clone(),
                last_used: row.last_used.clone(),
            });
        }
        let waiting = locked
            .waiters
            .iter()
            .map(|slot| slot.request().ticket.clone())
            .collect();
        Ok(Status { trees, waiting })
    }

    /// [`TreeConfig::reload`], with manifests read again from the main checkout.
    pub fn reload_products(&self, manifest: &Manifest) {
        self.inner.config.reload(manifest);
    }

    /// Fast-forwards the main checkout's local integration branches to the remote, only while
    /// the checkout has no local changes.
    pub fn fast_forward_main(&self) -> Result<FastForward> {
        let config = &self.inner.config;
        fastforward::main_checkout(&config.repo, &config.remote, &config.branches)
    }
}
