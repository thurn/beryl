//! Build trees: the machine's fixed set of ticket trees and landing trees, leased by ticket,
//! kept built near the tip and held under the machine's disk limit.
//!
//! Cargo's incremental builds are only reused at the path they were made at, so each tree keeps
//! its build directories between tickets. Which directories those are comes from the build
//! products' `outputs` in the check manifests; no directory name is written here. Ignite never
//! builds anywhere but these trees: when every ticket tree is leased, a caller gets a [`Waiter`]
//! that says why, and the next release serves it.
//!
//! A tree's state lives in the daemon's local store (`ignite_store::local::trees`): `free`,
//! `prebuilding`, `leased`, `landing` or `resetting`. Only a free tree is ever leased.
//!
//! Releasing a ticket tree saves its work first ([`ignite_git::save`]) and fails, leaving the
//! tree leased and untouched, if that push fails. It then ends every process inside the tree
//! except the caller and its ancestors, resets the tree to a clean checkout that keeps the build
//! directories, and serves the first waiter.

mod disk;
mod landing;
mod reset;
#[cfg(test)]
mod tests;
mod wait;

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ignite_core::config::Config;
use ignite_core::ids::{Commit, SessionId, TicketId, TreeId};
use ignite_core::manifest::{InputPattern, Manifest};
use ignite_core::traits::{BuildResult, Builds, Clock, Error, Result};
use ignite_git::fastforward::{self, FastForward};
use ignite_git::save::{self, Saved};
use ignite_git::{refs, worktree};
use ignite_store::local::{rows, trees as store};
use rusqlite::Connection;
use serde::Serialize;

pub use disk::DiskReport;
use wait::Slot;
pub use wait::Waiter;

/// The trailer on the commit that [`Trees::release`] makes of uncommitted work, so the next
/// start of the ticket can say that its branch ends in unfinished work saved at release.
pub const SAVED_TRAILER: &str = "Ignite-Saved: release";

/// How long a process inside a released tree has to exit after `SIGTERM` before `SIGKILL`.
const GRACE: Duration = Duration::from_secs(3);

/// The remote branch that holds a released ticket's work.
pub fn save_branch(ticket: &TicketId) -> String {
    format!("ignite/{ticket}")
}

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
    /// The main checkout's local branches kept fast-forwarded to the remote.
    pub branches: Vec<String>,
    pub ticket_trees: u32,
    pub landing_trees: u32,
    /// The disk the trees may use, in bytes.
    pub disk_limit: Option<u64>,
    /// The build products' output directories, relative to a tree's root: what a reset keeps.
    pub keep: Vec<String>,
    /// The products built at the lease of a ticket expected to compile, and kept built at the
    /// tip in one free tree: those whose inputs name workspace packages.
    pub prebuild: Vec<String>,
}

impl TreeConfig {
    pub fn new(repo: &Path, config: &Config, hostname: &str, manifest: &Manifest) -> Result<Self> {
        let machine = config
            .machine(hostname)
            .ok_or_else(|| Error::Invalid(format!("ignite.toml declares no machine {hostname}")))?;
        let keep: BTreeSet<String> = manifest
            .products
            .values()
            .flat_map(|product| product.outputs.iter())
            .map(|dir| dir.trim_matches('/').to_owned())
            .collect();
        let prebuild = manifest
            .products
            .iter()
            .filter(|(_, product)| {
                product.inputs.iter().any(|input| {
                    matches!(input, InputPattern::Crate(_) | InputPattern::CrateClosure)
                })
            })
            .map(|(name, _)| name.clone())
            .collect();
        Ok(TreeConfig {
            repo: repo.to_path_buf(),
            dir: repo.join(&config.repository.trees),
            remote: config.repository.remote.clone(),
            branches: vec![
                config.repository.staging.clone(),
                config.repository.master.clone(),
            ],
            ticket_trees: machine.ticket_trees,
            landing_trees: machine.landing_trees,
            disk_limit: machine
                .trees_disk_gb
                .map(|gb| u64::from(gb) * 1_000_000_000),
            keep: keep.into_iter().collect(),
            prebuild,
        })
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

impl Trees {
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

    /// Creates every configured tree that does not exist yet, at `tip`, and finishes any reset
    /// prebuild or landing that a stopped daemon left half done. Leased trees keep their leases.
    pub fn setup(&self, tip: &Commit) -> Result<()> {
        let config = &self.inner.config;
        fs::create_dir_all(&config.dir)?;
        for (id, kind) in config.configured() {
            let path = config.path(&id);
            let park = park_branch(&id);
            let row = store::get(&self.lock().db, id.as_str()).map_err(|e| store_error(&e))?;
            let fresh = !path.join(".git").exists();
            if fresh {
                worktree::add(&config.repo, &path, &park, tip)?;
            }
            let row = match row {
                Some(row) if !fresh => row,
                _ => {
                    if !fresh {
                        worktree::reset(&path, &park, tip, &config.keep)?;
                    }
                    rows::Tree {
                        id: id.to_string(),
                        path: path.display().to_string(),
                        kind: kind.as_str().to_owned(),
                        state: store::State::Free.as_str().to_owned(),
                        ticket: None,
                        session: None,
                        branch: None,
                        built_commit: None,
                        leased_at: None,
                        last_used: None,
                    }
                }
            };
            // A `landing` row's holder is a landing or tip verification of the daemon that
            // stopped, so after a restart nothing can free it but this reset.
            let unfinished = [
                store::State::Prebuilding,
                store::State::Resetting,
                store::State::Landing,
            ]
            .iter()
            .any(|state| row.state == state.as_str());
            let row = if unfinished {
                worktree::reset(&path, &park, tip, &config.keep)?;
                rows::Tree {
                    state: store::State::Free.as_str().to_owned(),
                    ticket: None,
                    session: None,
                    branch: None,
                    ..row
                }
            } else {
                row
            };
            store::upsert(&self.lock().db, &row).map_err(|e| store_error(&e))?;
        }
        Ok(())
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
        let mut free: Vec<(Option<usize>, &rows::Tree)> = rows
            .iter()
            .filter(|row| {
                row.state == store::State::Free.as_str()
                    && configured
                        .iter()
                        .any(|(id, kind)| id.as_str() == row.id && *kind == Kind::Ticket)
            })
            .map(|row| {
                (
                    self.distance(row.built_commit.as_deref(), &request.tip),
                    row,
                )
            })
            .collect();
        // `None` (never built) sorts after every distance.
        let key = |(distance, row): &(Option<usize>, &rows::Tree)| {
            (distance.unwrap_or(usize::MAX), row.id.clone())
        };
        free.sort_by_key(key);
        let chosen = if request.compiles {
            free.first()
        } else {
            free.iter().max_by_key(|entry| {
                (
                    entry.0.unwrap_or(usize::MAX),
                    std::cmp::Reverse(&entry.1.id),
                )
            })
        };
        let Some((_, row)) = chosen else {
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
        if let Err(error) = worktree::reset(&path, &request.branch, &request.at, &config.keep) {
            let _ = store::release(&self.lock().db, tree.as_str(), None, &self.now());
            return Err(error);
        }
        if request.compiles && !config.prebuild.is_empty() {
            let trees = self.clone();
            let (tree, at) = (tree.clone(), request.at.clone());
            std::thread::spawn(move || trees.build(&tree, &at));
        }
        Ok(TreeLease {
            tree: tree.clone(),
            path,
            branch: request.branch.clone(),
            at: request.at.clone(),
        })
    }

    /// Makes every prebuild product current in `tree` and, if all succeed, records `at` as the
    /// commit its build directories match.
    fn build(&self, tree: &TreeId, at: &Commit) -> bool {
        let built = self.inner.config.prebuild.iter().all(|product| {
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
        let saved = save::save(&path, &config.remote, &save_branch(&ticket), &message)?;
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

    /// Frees a tree that was leased but never used, without saving or resetting it.
    fn give_back(&self, tree: &TreeId) {
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
        if self.inner.config.prebuild.is_empty() {
            return Ok(None);
        }
        let tree = {
            let locked = self.lock();
            let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
            let free = |row: &&rows::Tree| {
                row.kind == Kind::Ticket.as_str() && row.state == store::State::Free.as_str()
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
        let path = self.inner.config.path(&tree);
        let reset = worktree::reset(&path, &park_branch(&tree), tip, &self.inner.config.keep);
        let built = reset.is_ok() && self.build(&tree, tip);
        store::release(&self.lock().db, tree.as_str(), None, &self.now())
            .map_err(|e| store_error(&e))?;
        self.serve_waiters();
        reset?;
        Ok(built.then_some(tree))
    }

    /// The configured tree that holds `path`, if any.
    pub fn tree_at(&self, path: &Path) -> Option<TreeId> {
        let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let path = real(path);
        self.inner
            .config
            .configured()
            .into_iter()
            .map(|(id, _)| id)
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

    /// Fast-forwards the main checkout's local integration branches to the remote, only while
    /// the checkout has no local changes.
    pub fn fast_forward_main(&self) -> Result<FastForward> {
        let config = &self.inner.config;
        fastforward::main_checkout(&config.repo, &config.remote, &config.branches)
    }
}
