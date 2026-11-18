//! Landing trees: they stay at the tip, and a landing or a tip verification leases one so that
//! it builds only what one change added.

use ignite_core::ids::{Commit, TreeId};
use ignite_core::traits::{Error, Result};
use ignite_git::worktree;
use ignite_store::local::trees as store;

use super::{Kind, TreeLease, Trees, park_branch, store_error};

impl Trees {
    /// Leases a free landing tree at `tip` to a landing or tip verification, the one built
    /// nearest the tip; `None` when all are busy.
    pub fn lease_landing(&self, holder: &str, tip: &Commit) -> Result<Option<TreeLease>> {
        self.lease_landing_keeping(holder, tip, 0)
    }

    /// Leases a free landing tree at `tip` to idle work, but only while another landing tree
    /// stays free, so that a landing or tip verification that arrives while the idle work runs
    /// still finds one. Idle work can hold a tree for a long build, and nothing stops it for
    /// a tree. `None` when fewer than two landing trees are free.
    pub fn lease_spare_landing(&self, holder: &str, tip: &Commit) -> Result<Option<TreeLease>> {
        self.lease_landing_keeping(holder, tip, 1)
    }

    /// Leases the free landing tree built nearest `tip`, if more than `keep_free` are free.
    fn lease_landing_keeping(
        &self,
        holder: &str,
        tip: &Commit,
        keep_free: usize,
    ) -> Result<Option<TreeLease>> {
        let tree = {
            let locked = self.lock();
            let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
            let free: Vec<_> = rows
                .iter()
                .filter(|row| {
                    row.kind == Kind::Landing.as_str() && row.state == store::State::Free.as_str()
                })
                .collect();
            if free.len() <= keep_free {
                return Ok(None);
            }
            let Some(row) = free.into_iter().min_by_key(|row| {
                self.distance(row.built_commit.as_deref(), tip)
                    .unwrap_or(usize::MAX)
            }) else {
                return Ok(None);
            };
            let now = self.now();
            if !store::lease(
                &locked.db,
                &row.id,
                store::State::Landing,
                Some(holder),
                None,
                None,
                &now,
            )
            .map_err(|e| store_error(&e))?
            {
                return Ok(None);
            }
            TreeId::new(row.id.clone()).map_err(|e| Error::Invalid(e.to_string()))?
        };
        let path = self.inner.config.path(&tree);
        let branch = park_branch(&tree);
        if let Err(error) = worktree::reset(&path, &branch, tip, &self.inner.config.keep) {
            let _ = store::release(&self.lock().db, tree.as_str(), None, &self.now());
            return Err(error);
        }
        Ok(Some(TreeLease {
            tree,
            path,
            branch,
            at: tip.clone(),
        }))
    }

    /// Frees a landing tree; `built_at` is the commit its builds now match, if it built.
    pub fn release_landing(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()> {
        let row = self.row(tree)?;
        if row.state != store::State::Landing.as_str() {
            return Err(Error::Conflict(format!(
                "tree {tree} is {}, not landing",
                row.state
            )));
        }
        store::release(
            &self.lock().db,
            tree.as_str(),
            built_at.map(Commit::as_str),
            &self.now(),
        )
        .map_err(|e| store_error(&e))?;
        Ok(())
    }
}
