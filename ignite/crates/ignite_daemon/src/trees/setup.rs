//! Bringing the trees to a known state when the daemon starts.
//!
//! Creating a missing tree or resetting one a stopped daemon left half done needs `git`, and the
//! tip to reset to needs a fetch, which together took seconds while the daemon did not answer.
//! So starting is split in two:
//!
//! - [`Trees::mark_unfinished`] runs before the daemon serves its socket and touches only the
//!   local store: every tree that must be created or reset is marked `resetting`, with its lease
//!   cleared. A `resetting` tree is never leased, so nothing uses it until it is clean.
//! - [`Trees::finish`] creates or resets one marked tree at the tip and frees it, serving the
//!   first waiter. [`Trees::finish_in_background`] does that for every marked tree on a thread
//!   of its own, trying again while `git` fails. A lease that arrives meanwhile and finds no
//!   free tree waits, and is served as each tree is freed.
//!
//! A daemon that stops again before it has finished leaves the trees `resetting`, and the next
//! start marks them again ([`super::reset_at_start`]).

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use ignite_core::ids::{Commit, TreeId};
use ignite_core::traits::{Error, Git, Result};
use ignite_git::worktree;
use ignite_store::local::{rows, trees as store};

use super::{Trees, park_branch, resets_at_start, store_error};

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

    /// Marks `resetting`, in the local store only, every configured tree that is missing, has no
    /// row, or was left in the middle of something ([`super::reset_at_start`]), and returns them
    /// for [`Trees::finish`]. Runs no `git`.
    pub fn mark_unfinished(&self) -> Result<Vec<Unfinished>> {
        let config = &self.inner.config;
        fs::create_dir_all(&config.dir)?;
        let mut unfinished = Vec::new();
        for (id, kind) in config.configured() {
            let path = config.path(&id);
            let locked = self.lock();
            let row = store::get(&locked.db, id.as_str()).map_err(|e| store_error(&e))?;
            let fresh = !path.join(".git").exists();
            let marked = match row {
                Some(row) if !fresh => {
                    if !resets_at_start(&locked.db, &row) {
                        continue;
                    }
                    rows::Tree {
                        state: store::State::Resetting.as_str().to_owned(),
                        ticket: None,
                        session: None,
                        branch: None,
                        owner: None,
                        ..row
                    }
                }
                // A new checkout, or a tree the store has no row for, holds no build.
                _ => rows::Tree {
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
                },
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
        let path = config.path(tree);
        let park = park_branch(tree);
        // A tree created by an earlier try that failed later is reset like any other.
        if unfinished.fresh && !path.join(".git").exists() {
            worktree::add(&config.repo, &path, &park, tip)?;
        } else {
            worktree::reset(&path, &park, tip, &config.keep)?;
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
