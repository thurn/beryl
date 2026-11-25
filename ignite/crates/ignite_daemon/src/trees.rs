//! Build trees: the machine's ticket trees and landing trees, leased by ticket,
//! kept built near the tip within the machine's build slots.
//!
//! Cargo's incremental builds are only reused at the path they were made at, so each tree keeps
//! its build directories between tickets, and a compiling lease starts from a copy of the build
//! of the tree nearest the tip when that is nearer than its own ([`seed`]). The check manifests
//! name the build output directories. A lease creates a ticket tree when none is free, up to
//! the worker ceiling plus four. At the ceiling a caller gets a [`Waiter`]. The tree maintenance loop
//! retires surplus trees after a day of disuse.
//!
//! A tree's state lives in the daemon's local store (`ignite_store::local::trees`): `free`,
//! `prebuilding`, `leased`, `landing`, `resetting` or `retiring`. Only a free tree is ever leased.
//!
//! A landing tree leased by a verb with a handle records that handle as its owner. When the
//! daemon restarts, a landing tree whose owner is still running ([`owner_alive`]) is kept as it
//! is. An idle lease whose owner stopped is reclaimed when a landing tree is next requested,
//! after commands using that tree finish. Other unfinished trees are marked `resetting` and
//! reset in the background ([`reset_at_start`], [`Trees::finish_in_background`]).
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
mod elastic;
pub mod head;
mod landing;
mod main_checkout;
mod pressure;
pub(crate) use pressure::LargestTree;
mod reset;
mod retire;
pub(crate) mod root;
mod seed;
mod setup;
mod slots;
mod status;
#[cfg(test)]
pub(crate) mod tests;
pub(crate) mod volumes;
mod wait;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::manifests::Live;
use ignite_core::capacity::Derived;
use ignite_core::config::Config;
use ignite_core::ids::{Commit, Handle, SessionId, TicketId, TreeId};
use ignite_core::manifest::Manifest;
use ignite_core::traits::{BuildResult, Builds, Clock, Error, Result};
use ignite_git::save::{self, Saved};
use ignite_git::{refs, worktree};
use ignite_store::local::{handles as handle_store, rows, trees as store};
use rusqlite::Connection;
use serde::Serialize;

pub(crate) use elastic::ticket_tree_number;
pub use seed::{Seeding, wait_for_seed_on_disk, wait_until_seeded, wait_until_seeded_or};
pub use setup::Unfinished;
pub(crate) use slots::{
    BuildSlot, BuildSlots, TreeBuildControl, caller_processes, ceiling_bytes, command_lock,
    slot_count, volume_space,
};
use wait::Slot;
pub use wait::Waiter;

/// The trailer on the commit that [`Trees::release`] makes of uncommitted work, so the next
/// start of the ticket can say that its branch ends in unfinished work saved at release.
pub const SAVED_TRAILER: &str = "Ignite-Saved: release";

/// How long a process inside a released tree has to exit after `SIGTERM` before `SIGKILL`.
const GRACE: Duration = Duration::from_secs(3);

/// Leave room for interactive sessions as well as the headless worker pool.
const EXTRA_TICKET_TREES: u32 = 4;

/// The local branch a tree sits on while no ticket holds it.
fn park_branch(tree: &TreeId) -> String {
    format!("ignite-tree/{tree}")
}

/// What one machine's trees are, from `ignite.toml` and the check manifests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeConfig {
    /// The repository's main checkout.
    pub repo: PathBuf,
    /// The default directory for new trees, from the repository configuration.
    pub dir: PathBuf,
    pub remote: String,
    /// The prefix of the remote ref that holds a released ticket's work (`repository.saved`).
    pub saved: String,
    /// The main checkout's local branches kept fast-forwarded to the remote.
    pub branches: Vec<String>,
    pub ticket_tree_ceiling: u32,
    pub landing_trees: u32,
    /// The build products' output directories, relative to a tree's root: what a reset keeps.
    /// Both lists change when the manifests do ([`TreeConfig::reload`]).
    pub keep: Live<Vec<String>>,
    /// The products built at the lease of a ticket expected to compile, and kept built at the
    /// tip in one free tree: those whose inputs name workspace packages.
    pub prebuild: Live<Vec<String>>,
    /// Each prebuild product's output directories, which order a lease's prebuild by the
    /// ticket's files ([`TreeConfig::prebuild_order`]).
    pub prebuild_outputs: Live<BTreeMap<String, Vec<String>>>,
    pub compiled_outputs: Live<BTreeSet<String>>,
}

impl TreeConfig {
    /// The remote ref that holds a released ticket's work: the `saved` prefix and the ticket.
    pub fn save_branch(&self, ticket: &TicketId) -> String {
        format!("{}{ticket}", self.saved)
    }

    pub fn new(repo: &Path, config: &Config, derived: &Derived, manifest: &Manifest) -> Self {
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
            ticket_tree_ceiling: derived.worker_ceiling.saturating_add(EXTRA_TICKET_TREES),
            landing_trees: derived.landing_trees,
            keep: keep.into(),
            prebuild: prebuild.into(),
            prebuild_outputs: prebuild_outputs.into(),
            compiled_outputs: Self::compiled_outputs(manifest).into(),
        }
    }

    /// Landing trees are created at startup; ticket trees are created by leases.
    fn configured_landings(&self) -> impl Iterator<Item = TreeId> + '_ {
        (1..=self.landing_trees)
            .map(|n| TreeId::new(format!("landing-{n}")).expect("a tree ID is a token"))
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
    /// The machine is at its tree ceiling and every ticket tree is busy.
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
    /// The store and the waiters, under one lock so a release cannot slip between a lease
    /// finding no free tree and its waiter joining the queue.
    locked: Mutex<Locked>,
    ticket_tree_work_done: Condvar,
}

struct Locked {
    db: Connection,
    waiters: VecDeque<Arc<Slot>>,
    /// Leased trees being checked out or swept. Neither operation may overlap the other or
    /// let the tree become free while it still accesses the checkout.
    ticket_tree_work: BTreeSet<TreeId>,
    build_space: slots::BuildSpace,
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
/// landing tree whose owner is gone. Existing idle checkouts are reclaimed on the next lease.
fn resets_at_start(db: &Connection, row: &rows::Tree) -> bool {
    // Idle leases are reclaimed by the next landing lease. A dead verb can have
    // been waiting for another process's build, which still owns the command lock.
    if row.state == store::State::Landing.as_str()
        && row.ticket.as_deref() == Some(crate::idle::LANDING_HOLDER)
        && row.owner.is_some()
        && Path::new(&row.path).join(".git").exists()
    {
        return false;
    }
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
                locked: Mutex::new(Locked {
                    db,
                    waiters: VecDeque::new(),
                    ticket_tree_work: BTreeSet::new(),
                    build_space: slots::BuildSpace::default(),
                }),
                ticket_tree_work_done: Condvar::new(),
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
            if let Some(tree) = self.take_ticket_tree(&mut locked, &request)? {
                tree
            } else {
                let slot = Arc::new(Slot::new(request, Self::busy_reason(&locked)?));
                locked.waiters.push_back(Arc::clone(&slot));
                return Ok(Lease::Waiting(Waiter::new(slot, self.clone())));
            }
        };
        let lease = self.check_out(&tree, &request)?;
        Ok(Lease::Ready(lease))
    }

    /// Picks or creates a ticket tree for `request` and reserves it through checkout.
    fn take_ticket_tree(
        &self,
        locked: &mut Locked,
        request: &LeaseRequest,
    ) -> Result<Option<TreeId>> {
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        let root = self.root_in(&locked.db);
        let keep = self.inner.config.keep.get();
        let wanted = choice::wanted(&keep, &request.files);
        let mut free: Vec<(usize, Option<usize>, &rows::Tree)> = rows
            .iter()
            .filter(|row| {
                row.state == store::State::Free.as_str()
                    && row.kind == Kind::Ticket.as_str()
                    && root::holds(&root, row)
            })
            .map(|row| {
                let path = Path::new(&row.path);
                (
                    choice::missing(path, &wanted),
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
        let id = if let Some((_, _, row)) = chosen {
            TreeId::new(row.id.clone()).map_err(|e| Error::Invalid(e.to_string()))?
        } else {
            let Some(id) = self.create_ticket_tree(locked, &root, &rows, &request.tip)? else {
                return Ok(None);
            };
            id
        };
        let taken = store::lease(
            &locked.db,
            id.as_str(),
            store::State::Leased,
            Some(request.ticket.as_str()),
            request.session.as_ref().map(SessionId::as_str),
            Some(&request.branch),
            &self.now(),
        )
        .map_err(|e| store_error(&e))?;
        if !taken {
            return Err(Error::Conflict(format!("tree {id} was taken")));
        }
        locked.ticket_tree_work.insert(id.clone());
        Ok(Some(id))
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
    fn busy_reason(locked: &Locked) -> Result<String> {
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
            holders.len(),
            holders.join(", "),
            locked.waiters.len()
        ))
    }

    /// Checks the request's branch out in a tree just marked leased and starts its builds. On
    /// failure the tree is made free again.
    fn check_out(&self, tree: &TreeId, request: &LeaseRequest) -> Result<TreeLease> {
        let config = &self.inner.config;
        let path = PathBuf::from(self.row(tree)?.path);
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
        let checkout = self.check_out_branch(&path, &request.branch, &at);
        if checkout.is_ok() && request.compiles {
            self.start_builds(tree, request);
        }
        {
            let mut locked = self.lock();
            self.finish_ticket_tree_work(&mut locked, tree);
            if checkout.is_err() {
                let _ = store::release(&locked.db, tree.as_str(), None, &self.now());
            }
        }
        checkout?;
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
                !crate::steps::stop_requested()
                    && !matches!(
                        self.inner.builds.ensure(tree, product, &[]),
                        BuildResult::Failed { .. }
                    )
            });
        if built && !crate::steps::stop_requested() {
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
        let path = Path::new(&row.path);
        let message = format!("wip: {ticket} saved at release\n\n{SAVED_TRAILER}");
        let saved = save::save(path, &config.remote, &config.save_branch(&ticket), &message)?;
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

    /// Serves queued leases in order when a tree becomes available or can be created.
    fn serve_waiters(&self) {
        loop {
            let (slot, tree) = {
                let mut locked = self.lock();
                let Some(slot) = locked.waiters.front().cloned() else {
                    return;
                };
                match self.take_ticket_tree(&mut locked, slot.request()) {
                    Ok(Some(tree)) => {
                        locked.waiters.pop_front();
                        (slot, tree)
                    }
                    Ok(None) => return,
                    Err(error) => {
                        locked.waiters.pop_front();
                        slot.serve(Err(error));
                        continue;
                    }
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
        if let Ok(row) = self.row(tree) {
            let _ = seed::wait_until_seeded(Path::new(&row.path));
        }
        if let Err(error) = self.stop_work(tree) {
            eprintln!("ignite: cannot return unused {tree}: {error}");
            return;
        }
        {
            let locked = self.wait_for_ticket_tree_work(self.lock(), tree);
            let _ = store::release(&locked.db, tree.as_str(), None, &self.now());
        }
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

    /// Starts a prebuild of the free ticket tree nearest `tip` if none is already built there.
    /// A build can wait for disk space or cores, so the idle loop polls the thread without waiting.
    pub fn start_idle_prebuild(&self, tip: &Commit) -> Result<Option<JoinHandle<Result<()>>>> {
        if self.inner.config.prebuild.get().is_empty() {
            return Ok(None);
        }
        let tree = {
            let locked = self.lock();
            let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
            let root = self.root_in(&locked.db);
            let free = |row: &&rows::Tree| {
                row.kind == Kind::Ticket.as_str()
                    && row.state == store::State::Free.as_str()
                    && root::holds(&root, row)
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
        let (trees, target, tip) = (self.clone(), tree.clone(), tip.clone());
        let started = thread::Builder::new()
            .name("ignite-idle-prebuild".to_owned())
            .spawn(move || trees.prebuild(&target, &tip));
        match started {
            Ok(build) => Ok(Some(build)),
            Err(error) => {
                self.give_back(&tree);
                Err(error.into())
            }
        }
    }

    fn prebuild(&self, tree: &TreeId, tip: &Commit) -> Result<()> {
        let path = PathBuf::from(self.row(tree)?.path);
        let activity = self.inner.steps.begin(
            crate::steps::Kind::Loop("ignite-idle-prebuild".to_owned()),
            crate::steps::Work {
                class: Some(ignite_core::protocol::Class::Idle),
                tree: Some(tree.clone()),
                tree_path: Some(path.clone()),
                ..crate::steps::Work::default()
            },
        );
        let _entered = activity.enter();
        crate::steps::step(format!("prebuilding {tree} at {}", &tip.as_str()[..7]));
        let reset = worktree::reset(
            &path,
            &park_branch(tree),
            tip,
            &self.inner.config.keep.get(),
        );
        if reset.is_ok() {
            self.build(tree, tip, &[]);
        }
        store::release(&self.lock().db, tree.as_str(), None, &self.now())
            .map_err(|e| store_error(&e))?;
        self.serve_waiters();
        reset
    }

    /// [`TreeConfig::reload`], with manifests read again from the main checkout.
    pub fn reload_products(&self, manifest: &Manifest) {
        self.inner.config.reload(manifest);
    }
}
