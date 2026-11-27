//! Remove surplus ticket trees and landing trees that are no longer configured.

use std::fs::{self, File};
use std::path::Path;

use ignite_core::ids::TreeId;
use ignite_core::traits::Result;
use ignite_git::worktree;
use ignite_store::local::{rows, trees as store};

use super::disk::id_of;
use super::slots::try_exclusive;
use super::{Kind, Seeding, Trees, store_error};

/// Keep warm donors available when work arrives after a quiet period.
const FREE_TREE_KEEP: usize = 2;
/// Keep an unused tree through a day of normal changes in demand.
const FREE_TREE_RETIRE_HOURS: u32 = 24;

impl Trees {
    /// Frees any ticket tree whose row is `leased` with neither a ticket nor a session, so a
    /// leaked row with no holder heals itself even when no build slot eviction needs its disk
    /// space.
    pub fn free_unheld_leases(&self) -> Result<Vec<TreeId>> {
        let freed = {
            let locked = self.lock();
            let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
            let now = self.now();
            let mut freed = Vec::new();
            for row in rows.iter().filter(|row| {
                super::reset::is_unheld_lease(row) && Self::reclaimable(&locked, row)
            }) {
                let id = id_of(row)?;
                let path = Path::new(&row.path);
                let _guard = if path.exists() {
                    let Some(guard) = try_exclusive(path)? else {
                        continue;
                    };
                    if !self.tree_processes_clear(&id, path)? {
                        continue;
                    }
                    Some(guard)
                } else {
                    None
                };
                if store::release(&locked.db, id.as_str(), None, &now)
                    .map_err(|e| store_error(&e))?
                {
                    freed.push(id);
                }
            }
            freed
        };
        if !freed.is_empty() {
            self.serve_waiters();
        }
        Ok(freed)
    }

    /// Retires free ticket trees older than a day, keeping the newest two free. Retires
    /// unconfigured landing trees as soon as they are free.
    pub fn retire_surplus(&self) -> Result<Vec<TreeId>> {
        let mut retired = Vec::new();
        while let Some((tree, _guard)) = self.next_to_retire()? {
            self.remove_retiring(&tree)?;
            retired.push(tree);
        }
        if !retired.is_empty() {
            self.serve_waiters();
        }
        Ok(retired)
    }

    fn next_to_retire(&self) -> Result<Option<(TreeId, Option<File>)>> {
        let locked = self.lock();
        let rows = store::all(&locked.db).map_err(|e| store_error(&e))?;
        if let Some(row) = rows
            .iter()
            .find(|row| row.state == store::State::Retiring.as_str())
        {
            let path = Path::new(&row.path);
            return if path.exists() {
                let tree = id_of(row)?;
                Ok(try_exclusive(path)?.map(|guard| (tree, Some(guard))))
            } else {
                id_of(row).map(|id| Some((id, None)))
            };
        }
        let root = self.root_in(&locked.db);
        let mut free: Vec<&rows::Tree> = rows
            .iter()
            .filter(|row| {
                row.state == store::State::Free.as_str()
                    && !Seeding::involves(Path::new(&row.path))
                    && !Self::legacy_command_in(&locked, Path::new(&row.path))
            })
            .collect();
        let surplus_tree = free
            .iter()
            .copied()
            .find(|row| !self.inner.config.uses(&root, row));
        let now = self.inner.clock.now();
        let cutoff = now
            .plus_seconds(-f64::from(FREE_TREE_RETIRE_HOURS * 3600))
            .to_string();
        free.retain(|row| row.kind == Kind::Ticket.as_str());
        // Rank and mark under the lease lock so a recent lease cannot be mistaken for disuse.
        free.sort_by_key(|row| (&row.last_used, &row.id));
        let old_ticket = free
            .iter()
            .copied()
            .take(free.len().saturating_sub(FREE_TREE_KEEP))
            .find(|row| row.last_used.as_ref().is_some_and(|last| last <= &cutoff));
        let Some(row) = surplus_tree.or(old_ticket) else {
            return Ok(None);
        };
        let path = Path::new(&row.path);
        let guard = if path.exists() {
            let Some(guard) = try_exclusive(path)? else {
                return Ok(None);
            };
            Some(guard)
        } else {
            None
        };
        let id = id_of(row)?;
        if !self.tree_processes_clear(&id, path)? {
            return Ok(None);
        }
        store::transition(
            &locked.db,
            id.as_str(),
            store::State::Free,
            store::State::Retiring,
            &now.to_string(),
        )
        .map_err(|e| store_error(&e))?;
        Ok(Some((id, guard)))
    }

    fn remove_retiring(&self, id: &TreeId) -> Result<()> {
        let config = &self.inner.config;
        let row = self.row(id)?;
        let path = Path::new(&row.path);
        self.inner.builds.forget(id);
        if path.join(".git").exists() {
            worktree::remove(&config.repo, path)?;
        }
        // Git can leave ignored build directories behind. A restart may also have interrupted
        // this deletion, so the persisted retirement is retried even with no checkout left.
        if path.exists() {
            fs::remove_dir_all(path)?;
        }
        store::delete(&self.lock().db, id.as_str(), store::State::Retiring)
            .map_err(|e| store_error(&e))?;
        Ok(())
    }
}
