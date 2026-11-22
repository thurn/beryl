//! Bringing the trees to a known state when the daemon starts.
//!
//! Creating a missing tree or resetting one a stopped daemon left half done needs `git`, and the
//! tip to reset to needs a fetch, which together took seconds while the daemon did not answer.
//! So starting is split in two:
//!
//! - [`Trees::mark_unfinished`] looks for unrecorded checkouts and marks trees that need
//!   creation or a reset. A `resetting` tree cannot be leased until it is clean.
//! - [`Trees::finish`] creates or resets one marked tree at the tip and frees it, serving the
//!   first waiter. [`Trees::finish_in_background`] does that for every marked tree on a thread
//!   of its own, trying again while `git` fails. A lease that arrives meanwhile and finds no
//!   free tree and is at its ceiling waits until a tree is freed.
//!
//! A daemon that stops again before it has finished leaves the trees `resetting`, and the next
//! start marks them again ([`super::reset_at_start`]).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ignite_core::ids::{Commit, TreeId};
use ignite_core::traits::{Error, Git, Result};
use ignite_git::worktree;
use ignite_store::local::{rows, trees as store};

use super::{Kind, Trees, park_branch, resets_at_start, root, store_error, ticket_tree_number};

/// The first pause before [`Trees::finish_in_background`] tries again; it doubles up to
/// [`MAX_PAUSE`].
const FIRST_PAUSE: Duration = Duration::from_secs(1);
const MAX_PAUSE: Duration = Duration::from_secs(60);

/// A tree that [`Trees::mark_unfinished`] marked `resetting`, for [`Trees::finish`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unfinished {
    pub tree: TreeId,
    /// The tree has no checkout yet and is created rather than reset.
    pub fresh: bool,
}

impl Trees {
    /// Creates every configured tree that does not exist yet, at `tip`, and finishes any reset
    /// prebuild or landing that a stopped daemon left half done, before it returns. Leased trees
    /// keep their leases, and so does a landing tree whose owner runs again
    /// ([`super::reset_at_start`]).
    pub fn setup(&self, tip: &Commit) -> Result<()> {
        for tree in self.mark_unfinished()? {
            self.finish(&tree, tip)?;
        }
        Ok(())
    }

    /// Marks missing landing trees and unfinished checkouts as resetting. When there are
    /// unrecorded ticket directories, adopts the repository's parked worktrees among them.
    pub fn mark_unfinished(&self) -> Result<Vec<Unfinished>> {
        let config = &self.inner.config;
        let locked = self.lock();
        let root = self.root_in(&locked.db);
        fs::create_dir_all(&root)?;
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        let mut pending: BTreeMap<_, _> = config
            .configured_landings()
            .map(|id| (id, Kind::Landing))
            .collect();
        for row in &rows {
            if row.kind == Kind::Ticket.as_str() {
                pending.insert(super::disk::id_of(row)?, Kind::Ticket);
            }
        }
        for id in unrecorded_tickets(&config.repo, &root, &rows)? {
            pending.insert(id, Kind::Ticket);
        }
        let mut unfinished = Vec::new();
        for (id, kind) in pending {
            let row = rows.iter().find(|row| row.id == id.as_str());
            let path = row.map_or_else(|| root.join(id.as_str()), |row| row.path.clone().into());
            let fresh = !path.join(".git").exists();
            if row.is_none() && kind == Kind::Landing {
                check_landing_destination(&config.repo, &path, &id)?;
            }
            let marked = match row {
                Some(row) if row.state == store::State::Retiring.as_str() => continue,
                Some(row) if !root::holds(&root, row) => {
                    if !resets_at_start(&locked.db, row) {
                        continue;
                    }
                    if fresh {
                        store::set_state(&locked.db, &row.id, store::State::Retiring, &self.now())
                            .map_err(|e| store_error(&e))?;
                        continue;
                    }
                    interrupted(row)
                }
                Some(row) if !fresh => {
                    if !resets_at_start(&locked.db, row) {
                        continue;
                    }
                    interrupted(row)
                }
                // A new checkout, or a tree the store has no row for, holds no build record.
                _ => resetting_tree(&path, &id, kind),
            };
            store::upsert(&locked.db, &marked).map_err(|e| store_error(&e))?;
            unfinished.push(Unfinished { tree: id, fresh });
        }
        Ok(unfinished)
    }

    /// Creates or resets a tree [`Trees::mark_unfinished`] marked, at `tip`, keeping its build
    /// directories, then frees it and serves the first waiter. On failure the tree stays
    /// `resetting` for another try.
    pub fn finish(&self, unfinished: &Unfinished, tip: &Commit) -> Result<()> {
        let config = &self.inner.config;
        let tree = &unfinished.tree;
        let path = std::path::PathBuf::from(self.row(tree)?.path);
        let park = park_branch(tree);
        // A tree created by an earlier try that failed later is reset like any other.
        if unfinished.fresh && !path.join(".git").exists() {
            fs::create_dir_all(path.parent().expect("a tree has a parent"))?;
            worktree::add(&config.repo, &path, &park, tip)?;
        } else {
            if !registered(&config.repo, &path, None)? {
                return Err(Error::Conflict(format!(
                    "{} is outside the repository's registered worktrees",
                    path.display()
                )));
            }
            worktree::reset(&path, &park, tip, &config.keep.get())?;
        }
        let freed = store::transition(
            &self.lock().db,
            tree.as_str(),
            store::State::Resetting,
            store::State::Free,
            &self.now(),
        )
        .map_err(|e| store_error(&e))?;
        if !freed {
            return Err(Error::Conflict(format!(
                "tree {tree} is no longer resetting"
            )));
        }
        self.serve_waiters();
        Ok(())
    }

    /// Reserves missing landing trees during a live root change. Existing IDs are kept
    /// until retirement finishes, so a running landing and its replacement never share an ID.
    pub(crate) fn mark_missing_landings(&self) -> Result<Vec<Unfinished>> {
        let locked = self.lock();
        let root = self.root_in(&locked.db);
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        let missing: Vec<_> = self
            .inner
            .config
            .configured_landings()
            .filter(|id| !rows.iter().any(|row| row.id == id.as_str()))
            .map(|id| (root.join(id.as_str()), id))
            .collect();
        for (path, id) in &missing {
            check_landing_destination(&self.inner.config.repo, path, id)?;
        }
        let mut unfinished = Vec::new();
        for (path, id) in missing {
            store::upsert(&locked.db, &resetting_tree(&path, &id, Kind::Landing))
                .map_err(|e| store_error(&e))?;
            unfinished.push(Unfinished {
                tree: id,
                fresh: true,
            });
        }
        Ok(unfinished)
    }

    /// Runs [`Trees::finish`] for each of `unfinished` on a thread of its own, at the tip of
    /// `branch` as `git` fetches it, trying again after a growing pause while the fetch or a
    /// reset fails. Returns at once.
    pub fn finish_in_background(
        &self,
        git: Arc<dyn Git>,
        branch: String,
        mut unfinished: Vec<Unfinished>,
    ) {
        if unfinished.is_empty() {
            return;
        }
        let trees = self.clone();
        let spawned = std::thread::Builder::new()
            .name("ignite-tree-setup".to_owned())
            .spawn(move || {
                let mut pause = FIRST_PAUSE;
                loop {
                    match git.tip(&branch) {
                        Ok(tip) => unfinished.retain(|tree| match trees.finish(tree, &tip) {
                            Ok(()) => false,
                            Err(error) => {
                                eprintln!("ignite: tree {} not reset yet: {error}", tree.tree);
                                true
                            }
                        }),
                        Err(error) => {
                            eprintln!("ignite: trees not reset yet, no tip of {branch}: {error}");
                        }
                    }
                    if unfinished.is_empty() {
                        return;
                    }
                    std::thread::sleep(pause);
                    pause = (pause * 2).min(MAX_PAUSE);
                }
            });
        if let Err(error) = spawned {
            eprintln!("ignite: cannot start the thread that resets trees: {error}");
        }
    }
}

fn check_landing_destination(repo: &Path, path: &Path, id: &TreeId) -> Result<()> {
    if !path.exists()
        || fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
        || registered(repo, path, Some(&park_branch(id)))?
    {
        return Ok(());
    }
    Err(Error::Conflict(format!(
        "{} already exists and is outside Ignite's parked worktrees",
        path.display()
    )))
}

fn registered(repo: &Path, path: &Path, branch: Option<&str>) -> Result<bool> {
    let path = ignite_os::paths::canonicalize(path)?;
    Ok(worktree::list(repo)?.iter().any(|tree| {
        tree.path == path && branch.is_none_or(|name| tree.branch.as_deref() == Some(name))
    }))
}

fn interrupted(row: &rows::Tree) -> rows::Tree {
    rows::Tree {
        state: store::State::Resetting.as_str().to_owned(),
        ticket: None,
        session: None,
        branch: None,
        owner: None,
        ..row.clone()
    }
}

pub(super) fn resetting_tree(path: &Path, id: &TreeId, kind: Kind) -> rows::Tree {
    rows::Tree {
        id: id.to_string(),
        path: path.display().to_string(),
        kind: kind.as_str().to_owned(),
        state: store::State::Resetting.as_str().to_owned(),
        ticket: None,
        session: None,
        branch: None,
        built_commit: None,
        leased_at: None,
        last_used: None,
        owner: None,
    }
}

fn unrecorded_tickets(repo: &Path, root: &Path, rows: &[rows::Tree]) -> Result<Vec<TreeId>> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if ticket_tree_number(&name).is_some()
            && !rows.iter().any(|row| row.id == name)
            && entry.file_type()?.is_dir()
            && entry.path().join(".git").is_file()
        {
            candidates.push(TreeId::new(name).map_err(|e| Error::Invalid(e.to_string()))?);
        }
    }
    if candidates.is_empty() {
        return Ok(candidates);
    }

    // An interrupted allocation or a checkout left by an older daemon may have no row.
    // Check Git's registration and the parking branch before ever resetting its files.
    let dir = ignite_os::paths::canonicalize(root)?;
    let registered = worktree::list(repo)?;
    candidates.retain(|id| {
        let path = dir.join(id.as_str());
        let park = park_branch(id);
        registered
            .iter()
            .any(|tree| tree.path == path && tree.branch.as_deref() == Some(park.as_str()))
    });
    Ok(candidates)
}
