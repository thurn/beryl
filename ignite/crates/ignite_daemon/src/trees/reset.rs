//! Ending a ticket tree's lease: stop its processes and reset it to a clean checkout.

use std::collections::BTreeSet;
use std::path::Path;

use ignite_core::ids::{Commit, TreeId};
use ignite_core::traits::{Error, Result};
use ignite_git::{refs, worktree};
use ignite_os::Native;
use ignite_os::occupants::Occupants;
use ignite_store::local::{rows, trees as store};

use super::{Ended, GRACE, Trees, park_branch, store_error};

impl Trees {
    pub(super) fn reset_leased(
        &self,
        tree: &TreeId,
        row: &rows::Tree,
        park_at: &Commit,
        caller: Option<u32>,
    ) -> Result<Vec<Ended>> {
        let config = &self.inner.config;
        let path = config.path(tree);
        let head = refs::head(&path)?;
        // Moving from `leased` is a compare-and-set, so of two overlapping releases of one tree
        // only the first proceeds.
        let claimed = store::transition(
            &self.lock().db,
            tree.as_str(),
            store::State::Leased,
            store::State::Resetting,
            &self.now(),
        )
        .map_err(|e| store_error(&e))?;
        if !claimed {
            return Err(Error::Conflict(format!(
                "tree {tree} is no longer leased; another release has it"
            )));
        }
        // A release that fails after the tree is marked `resetting` goes back to `leased`, so
        // the caller can retry it; nothing else would move a `resetting` tree until the daemon
        // restarts.
        let reset = Self::end_processes(&path, caller).and_then(|ended| {
            worktree::reset(&path, &park_branch(tree), park_at, &config.keep)?;
            Ok(ended)
        });
        let ended = match reset {
            Ok(ended) => ended,
            Err(error) => {
                self.back_to_leased(tree);
                return Err(error);
            }
        };
        let built = row.built_commit.is_some().then(|| head.to_string());
        let freed = store::release(
            &self.lock().db,
            tree.as_str(),
            built.as_deref(),
            &self.now(),
        );
        if let Err(error) = freed {
            // The tree is clean but the store would not say so; leave it where a retry finds it.
            self.back_to_leased(tree);
            return Err(store_error(&error));
        }
        self.serve_waiters();
        Ok(ended)
    }

    /// Puts a tree that a failed release left `resetting` back to `leased`.
    fn back_to_leased(&self, tree: &TreeId) {
        let _ = store::transition(
            &self.lock().db,
            tree.as_str(),
            store::State::Resetting,
            store::State::Leased,
            &self.now(),
        );
    }

    pub(super) fn row(&self, tree: &TreeId) -> Result<rows::Tree> {
        store::get(&self.lock().db, tree.as_str())
            .map_err(|e| store_error(&e))?
            .ok_or_else(|| Error::NotFound(format!("tree {tree}")))
    }

    /// Ends every process whose working directory or executable is inside `path`, except this
    /// process, `caller` and the ancestors of both, and every process in `caller`'s process
    /// group. The group is spared because the client usually runs in a pipeline such as
    /// `ignite close -m ... 2>&1 | tail -4` from a shell inside the tree: `tail` is neither the
    /// client nor its ancestor, and ending it ended the command line with exit 143.
    pub(super) fn end_processes(path: &Path, caller: Option<u32>) -> Result<Vec<Ended>> {
        let mut spared = BTreeSet::new();
        for start in [Some(std::process::id()), caller].into_iter().flatten() {
            let mut pid = Some(start);
            while let Some(current) = pid.filter(|&p| p > 1 && spared.insert(p)) {
                pid = Native.parent(current);
            }
        }
        // A caller in the daemon's own group would spare everything the daemon started.
        let caller_group = caller
            .and_then(|pid| Native.group(pid))
            .filter(|&group| group > 1 && Native.group(std::process::id()) != Some(group));
        let occupants: Vec<_> = Native
            .occupants(path)?
            .into_iter()
            .filter(|occupant| !spared.contains(&occupant.pid))
            .filter(|occupant| caller_group.is_none() || Native.group(occupant.pid) != caller_group)
            .collect();
        let pids: Vec<u32> = occupants.iter().map(|o| o.pid).collect();
        let killed = Native.end(&pids, GRACE)?;
        Ok(occupants
            .into_iter()
            .map(|occupant| Ended {
                killed: killed.contains(&occupant.pid),
                pid: occupant.pid,
                executable: occupant.executable,
                cwd: occupant.cwd,
            })
            .collect())
    }
}
