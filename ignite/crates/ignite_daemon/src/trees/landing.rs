//! Landing trees: they stay at the tip, and a landing or a tip verification leases one so that
//! it builds only what one change added.

use ignite_core::ids::{Commit, Handle, TicketId, TreeId};
use ignite_core::traits::{Error, Result};
use ignite_git::worktree;
use ignite_store::local::trees as store;

use super::{Kind, Locked, TreeLease, Trees, head, owner_alive, park_branch, store_error};

impl Trees {
    /// Leases a free landing tree at `tip` to a holder without a handle, such as tip
    /// verification, the one built nearest the tip; `None` when all are busy.
    pub fn lease_landing(&self, holder: &str, tip: &Commit) -> Result<Option<TreeLease>> {
        self.lease_landing_keeping(holder, None, tip, 0)
    }

    /// Leases a landing tree at `tip` to the verb with the handle `owner`, such as a landing,
    /// and records the handle with the lease. A verb run again after a restart gets back the
    /// tree its handle held, untouched when it is already at `tip` (the commands still running
    /// in it are taken over, see `crate::adopt`) and reset to `tip` otherwise. Any other verb
    /// gets the free tree built nearest the tip; `None` when all are busy.
    pub fn lease_landing_for(
        &self,
        holder: &str,
        owner: &Handle,
        tip: &Commit,
    ) -> Result<Option<TreeLease>> {
        self.lease_landing_keeping(holder, Some(owner), tip, 0)
    }

    /// Leases a free landing tree at `tip` to idle work, but only while another landing tree
    /// stays free, so that a landing or tip verification that arrives while the idle work runs
    /// still finds one. Idle work can hold a tree for a long build, and nothing stops it for
    /// a tree. `None` when fewer than two landing trees are free.
    pub fn lease_spare_landing(&self, holder: &str, tip: &Commit) -> Result<Option<TreeLease>> {
        self.lease_landing_keeping(holder, None, tip, 1)
    }

    /// Leases the landing tree `owner` already holds, or else the free landing tree built
    /// nearest `tip` if more than `keep_free` are free.
    pub(crate) fn lease_landing_keeping(
        &self,
        holder: &str,
        owner: Option<&Handle>,
        tip: &Commit,
        keep_free: usize,
    ) -> Result<Option<TreeLease>> {
        let kept = {
            let locked = self.lock();
            self.free_dead_owners(&locked)?;
            match owner {
                Some(owner) => Self::owned(&locked, owner)?,
                None => None,
            }
        };
        let held = kept.is_some();
        let tree = match kept {
            Some(tree) => tree,
            None => match self.take_free_landing(holder, owner, tip, keep_free)? {
                Some(tree) => tree,
                None => return Ok(None),
            },
        };
        let path = self.inner.config.path(&tree);
        let branch = park_branch(&tree);
        if held && head::commit_at(&path).as_ref() == Some(tip) {
            return Ok(Some(TreeLease {
                tree,
                path,
                branch,
                at: tip.clone(),
            }));
        }
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

    /// The landing tree `owner` holds, if any: one a previous daemon leased to the verb that
    /// this daemon runs again under the same handle.
    fn owned(locked: &Locked, owner: &Handle) -> Result<Option<TreeId>> {
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        rows.into_iter()
            .find(|row| {
                row.state == store::State::Landing.as_str()
                    && row.owner.as_deref() == Some(owner.as_str())
            })
            .map(|row| TreeId::new(row.id).map_err(|e| Error::Invalid(e.to_string())))
            .transpose()
    }

    /// Picks the free landing tree built nearest `tip`, if more than `keep_free` are free, and
    /// marks it landing for `holder` and `owner`.
    fn take_free_landing(
        &self,
        holder: &str,
        owner: Option<&Handle>,
        tip: &Commit,
        keep_free: usize,
    ) -> Result<Option<TreeId>> {
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
        let leased = store::lease(
            &locked.db,
            &row.id,
            store::State::Landing,
            Some(holder),
            None,
            None,
            &now,
        )
        .map_err(|e| store_error(&e))?;
        if !leased {
            return Ok(None);
        }
        if let Some(owner) = owner {
            store::set_owner(&locked.db, &row.id, owner.as_str()).map_err(|e| store_error(&e))?;
        }
        Ok(Some(
            TreeId::new(row.id.clone()).map_err(|e| Error::Invalid(e.to_string()))?,
        ))
    }

    /// Frees every landing tree whose owner finished without freeing it, such as a verb that a
    /// restarted daemon kept a tree for but then ended `incomplete`, or one run again that
    /// finished without asking for a landing tree. The next lease resets the tree.
    fn free_dead_owners(&self, locked: &Locked) -> Result<()> {
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        for row in rows {
            let Some(owner) = row.owner.as_deref() else {
                continue;
            };
            if row.state == store::State::Landing.as_str() && !owner_alive(&locked.db, owner) {
                eprintln!(
                    "ignite: freeing landing tree {}: {owner} finished without freeing it",
                    row.id
                );
                store::release(&locked.db, &row.id, None, &self.now())
                    .map_err(|e| store_error(&e))?;
            }
        }
        Ok(())
    }

    /// Names the ticket a leased ticket tree holds, keeping its branch: the hub numbers an ad
    /// hoc ticket only when it files it, after the tree was leased under a placeholder.
    pub fn set_ticket(&self, tree: &TreeId, ticket: &TicketId) -> Result<()> {
        store::set_ticket(&self.lock().db, tree.as_str(), ticket.as_str())
            .map_err(|e| store_error(&e))?;
        Ok(())
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
