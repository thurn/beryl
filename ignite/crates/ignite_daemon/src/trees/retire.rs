//! Removing the trees that `ignite.toml` no longer declares.
//!
//! Lowering `ticket_trees` or `landing_trees` leaves the trees above the new count on disk with
//! their build directories, which can be tens of gigabytes each, and in the store. Nothing
//! leases them again, since a lease picks only among the configured trees, but a session that
//! held one when the daemon restarted still works in it. So a tree above the count is removed
//! once it is free: its worktree and build directories are deleted, and then its row.

use std::fs;

use ignite_core::ids::TreeId;
use ignite_core::traits::Result;
use ignite_git::worktree;
use ignite_store::local::trees as store;

use super::disk::id_of;
use super::{Trees, store_error};

impl Trees {
    /// The free trees that `ignite.toml` does not declare, removed from disk and from the store.
    /// A leased tree is left until it is released. Returns the trees removed.
    pub fn retire_undeclared(&self) -> Result<Vec<TreeId>> {
        let configured = self.inner.config.configured();
        let undeclared: Vec<_> = store::all(&self.lock().db)
            .map_err(|e| store_error(&e))?
            .into_iter()
            .filter(|row| !configured.iter().any(|(id, _)| id.as_str() == row.id))
            .collect();
        let mut retired = Vec::new();
        for row in undeclared {
            let Some(row) = self.take_for_sweep(&row.id)? else {
                continue;
            };
            let id = id_of(&row)?;
            let path = self.inner.config.path(&id);
            if path.join(".git").exists()
                && let Err(error) = worktree::remove(&self.inner.config.repo, &path)
            {
                self.put_back(&row)?;
                return Err(error);
            }
            // `git worktree remove` leaves the ignored build directories behind.
            if path.exists()
                && let Err(error) = fs::remove_dir_all(&path)
            {
                self.put_back(&row)?;
                return Err(error.into());
            }
            self.inner.builds.forget(&id);
            store::delete(&self.lock().db, &row.id, store::State::Resetting)
                .map_err(|e| store_error(&e))?;
            retired.push(id);
        }
        Ok(retired)
    }
}
