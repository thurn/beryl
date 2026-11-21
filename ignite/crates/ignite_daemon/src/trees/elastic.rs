//! Ticket trees are added on demand up to a ceiling based on the machine's worker capacity.

use ignite_core::ids::{Commit, TreeId};
use ignite_core::traits::{Error, Result};
use ignite_git::worktree;
use ignite_store::local::{rows, trees as store};

use super::{Kind, Locked, Trees, park_branch, store_error};

/// Parses only the canonical name of an Ignite ticket tree.
pub(crate) fn ticket_tree_number(name: &str) -> Option<u32> {
    let number: u32 = name.strip_prefix("tree-")?.parse().ok()?;
    (number > 0 && name == format!("tree-{number:02}")).then_some(number)
}

fn ticket_rows(rows: &[rows::Tree]) -> impl Iterator<Item = &rows::Tree> {
    rows.iter().filter(|row| row.kind == Kind::Ticket.as_str())
}

impl Trees {
    fn ticket_tree_room(&self, rows: &[rows::Tree]) -> u32 {
        let used = u32::try_from(ticket_rows(rows).count()).unwrap_or(u32::MAX);
        self.inner.config.ticket_tree_ceiling.saturating_sub(used)
    }

    /// The number of leases the machine can take immediately, including trees it can create.
    pub fn free_ticket_trees(&self) -> Result<u32> {
        let rows = store::all(&self.lock().db).map_err(|e| store_error(&e))?;
        let free = ticket_rows(&rows)
            .filter(|row| row.state == store::State::Free.as_str())
            .count();
        Ok(u32::try_from(free)
            .unwrap_or(u32::MAX)
            .saturating_add(self.ticket_tree_room(&rows)))
    }

    /// Reserves a name in the store before calling Git, so a restart can finish an interrupted
    /// checkout. The lease lock covers name selection through the first lease of the new tree.
    pub(super) fn create_ticket_tree(
        &self,
        locked: &Locked,
        rows: &[rows::Tree],
        tip: &Commit,
    ) -> Result<Option<TreeId>> {
        if self.ticket_tree_room(rows) == 0 {
            return Ok(None);
        }
        let config = &self.inner.config;
        let id = (1..=u32::MAX)
            .map(|n| TreeId::new(format!("tree-{n:02}")).expect("a tree ID is a token"))
            .find(|id| !rows.iter().any(|row| row.id == id.as_str()) && !config.path(id).exists())
            .ok_or_else(|| Error::Conflict("no ticket tree name is available".into()))?;
        let path = config.path(&id);
        let row = config.resetting_tree(&id, Kind::Ticket);
        store::upsert(&locked.db, &row).map_err(|e| store_error(&e))?;
        if let Err(error) = worktree::add(&config.repo, &path, &park_branch(&id), tip) {
            store::set_state(&locked.db, id.as_str(), store::State::Retiring, &self.now())
                .map_err(|e| store_error(&e))?;
            return Err(error);
        }
        store::transition(
            &locked.db,
            id.as_str(),
            store::State::Resetting,
            store::State::Free,
            &self.now(),
        )
        .map_err(|e| store_error(&e))?;
        Ok(Some(id))
    }
}
