//! The root from the last fleet notice and the persisted paths of trees that already exist.

use std::path::{Path, PathBuf};

use ignite_core::ids::TreeId;
use ignite_store::local::{rows, trees as store};
use rusqlite::Connection;

use super::{TreeConfig, Trees};

impl Trees {
    pub fn root(&self) -> PathBuf {
        self.root_in(&self.lock().db)
    }

    pub(super) fn root_in(&self, db: &Connection) -> PathBuf {
        configured(db, &self.inner.config.dir)
    }

    /// Every folder that holds trees: the configured root, and the folder of each tree this
    /// machine still has, such as one under a root the fleet settings no longer name. The
    /// monitor probes each one ([`crate::monitor::storage`]).
    pub fn roots_in_use(&self) -> Vec<PathBuf> {
        let locked = self.lock();
        let mut roots = vec![self.root_in(&locked.db)];
        for row in store::all(&locked.db).unwrap_or_default() {
            if let Some(parent) = Path::new(&row.path).parent() {
                roots.push(parent.to_path_buf());
            }
        }
        roots.sort();
        roots.dedup();
        roots
    }

    pub(crate) fn path(&self, tree: &TreeId) -> Option<PathBuf> {
        self.row(tree).ok().map(|row| row.path.into())
    }
}

pub(crate) fn configured(db: &Connection, fallback: &Path) -> PathBuf {
    let root = crate::fleet_notice::read(db)
        .map_or_else(|| fallback.to_owned(), |notice| notice.trees.root(fallback));
    ignite_os::paths::canonicalize(&root).unwrap_or(root)
}

pub(crate) fn recorded_path(db: &Connection, tree: &TreeId) -> Option<PathBuf> {
    store::get(db, tree.as_str())
        .ok()
        .flatten()
        .map(|row| row.path.into())
}

pub(crate) fn tree_at(db: &Connection, cwd: &Path) -> Option<TreeId> {
    let rows = store::all(db).ok()?;
    rows.iter()
        .find(|row| cwd.starts_with(&row.path))
        .or_else(|| {
            let cwd = ignite_os::paths::canonicalize(cwd).ok()?;
            rows.iter().find(|row| {
                ignite_os::paths::canonicalize(&row.path).is_ok_and(|path| cwd.starts_with(path))
            })
        })
        .and_then(|row| TreeId::new(row.id.clone()).ok())
}

pub(super) fn holds(root: &Path, row: &rows::Tree) -> bool {
    Path::new(&row.path).parent().is_some_and(|parent| {
        parent == root || ignite_os::paths::canonicalize(parent).is_ok_and(|p| p == root)
    })
}

impl TreeConfig {
    #[cfg(test)]
    pub(super) fn path(&self, tree: &TreeId) -> PathBuf {
        self.dir.join(tree.as_str())
    }

    pub(super) fn uses(&self, root: &Path, row: &rows::Tree) -> bool {
        holds(root, row)
            && (row.kind == super::Kind::Ticket.as_str()
                || self.configured_landings().any(|id| id.as_str() == row.id))
    }
}
