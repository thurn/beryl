//! Tree ownership and status reported by the daemon.

use std::path::Path;

use ignite_core::ids::{SessionId, TreeId};
use ignite_core::traits::{Error, Result};
use ignite_store::local::trees as store;

use super::{Kind, Status, TreeStatus, Trees, store_error};

impl Trees {
    /// The tree that holds `path`, if any. This includes a surplus landing tree whose
    /// holder can still use it until release.
    pub fn tree_at(&self, path: &Path) -> Option<TreeId> {
        super::root::tree_at(&self.lock().db, path)
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
        let root = self.root_in(&locked.db);
        let mut trees: Vec<_> = rows
            .into_iter()
            .filter(|row| {
                self.inner.config.uses(&root, row) || row.state != store::State::Free.as_str()
            })
            .map(|row| {
                let tree = TreeId::new(row.id).map_err(|e| Error::Invalid(e.to_string()))?;
                let kind = if row.kind == Kind::Ticket.as_str() {
                    Kind::Ticket
                } else {
                    Kind::Landing
                };
                Ok(TreeStatus {
                    path: row.path.into(),
                    tree,
                    kind,
                    state: row.state,
                    ticket: row.ticket,
                    session: row.session,
                    branch: row.branch,
                    built_commit: row.built_commit,
                    last_used: row.last_used,
                })
            })
            .collect::<Result<_>>()?;
        trees.sort_by_key(|tree| {
            let number = tree
                .tree
                .as_str()
                .rsplit_once('-')
                .and_then(|(_, n)| n.parse::<u32>().ok())
                .unwrap_or(u32::MAX);
            (tree.kind != Kind::Ticket, number)
        });
        let waiting = locked
            .waiters
            .iter()
            .map(|slot| slot.request().ticket.clone())
            .collect();
        Ok(Status { trees, waiting })
    }
}
