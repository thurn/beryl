//! The build manager's memory of what each tree holds and which builds failed, kept in the local
//! database's `builds` table so that a restarted daemon does not run Cargo again for a product a
//! tree already holds.
//!
//! The rows are read once, when the manager is given the database ([`BuildManager::with_store`]),
//! and written wherever the memory changes: when a build ends, and when a tree's builds are
//! forgotten. A row whose product is no longer declared is dropped when read. So is a `held` row
//! whose product's output directories are missing or empty, since those directories were
//! deleted while no daemon watched, and a `failed` row whose output file is gone.

use std::path::PathBuf;

use ignite_core::ids::{InputHash, TreeId};
use ignite_core::traits::Clock as _;
use ignite_store::local::{builds, rows};
use rusqlite::Connection;

use super::{BuildManager, Request, State, lock};

/// How a finished build changed what the manager knows about its request.
pub(super) enum Change {
    /// The tree holds the product built at the hash.
    Held,
    /// The build at the hash exited with a failing code; the path names its output.
    Failed(PathBuf),
    /// The build failed in a way the same inputs may not repeat, and may have rewritten part of
    /// its outputs: the tree holds nothing, and no failure is remembered.
    NotHeld,
}

impl BuildManager {
    /// The manager with `conn` keeping what each tree holds and which builds failed, and with
    /// what a previous daemon recorded there read back into memory.
    #[must_use]
    pub fn with_store(self, conn: Connection) -> Self {
        match builds::all(&conn) {
            Ok(found) => {
                let mut state = lock(&self.state);
                for row in found {
                    if !self.restore(&mut state, &row) {
                        drop_row(&conn, &row);
                    }
                }
            }
            Err(error) => eprintln!("ignite: cannot read the recorded builds: {error}"),
        }
        *lock(&self.store) = Some(conn);
        self
    }

    /// Puts `row` back into memory; `false` when it no longer says anything true.
    fn restore(&self, state: &mut State, row: &rows::Build) -> bool {
        let (Ok(tree), Ok(hash), Ok(packages)) = (
            TreeId::new(row.tree.as_str()),
            InputHash::new(row.hash.as_str()),
            serde_json::from_str::<Vec<String>>(&row.packages),
        ) else {
            return false;
        };
        if !self
            .settings
            .manifest
            .get()
            .products
            .contains_key(&row.product)
        {
            return false;
        }
        let request = Request {
            tree,
            product: row.product.clone(),
            packages,
        };
        match (row.outcome.as_str(), &row.output) {
            (builds::HELD, _) if self.outputs_present(&request.tree, &request.product) => {
                state.held.insert(request, hash);
                true
            }
            (builds::FAILED, Some(output)) if PathBuf::from(output).is_file() => {
                state.failed.insert(request, (hash, PathBuf::from(output)));
                true
            }
            _ => false,
        }
    }

    /// Records how a finished build changed what is known about `request`.
    pub(super) fn store_change(&self, request: &Request, hash: &InputHash, change: &Change) {
        let store = lock(&self.store);
        let Some(conn) = store.as_ref() else {
            return;
        };
        let packages = serde_json::to_string(&request.packages).unwrap_or_else(|_| "[]".into());
        let row = |outcome: &str, output: Option<String>| rows::Build {
            tree: request.tree.to_string(),
            product: request.product.clone(),
            packages: packages.clone(),
            hash: hash.to_string(),
            outcome: outcome.to_owned(),
            output,
            updated_at: ignite_core::time::SystemClock.now().to_string(),
        };
        let written = match change {
            Change::Held => builds::set(conn, &row(builds::HELD, None)),
            Change::Failed(output) => builds::set(
                conn,
                &row(builds::FAILED, Some(output.to_string_lossy().into_owned())),
            ),
            Change::NotHeld => {
                builds::drop_held(conn, request.tree.as_str(), &request.product, &packages)
            }
        };
        if let Err(error) = written {
            eprintln!(
                "ignite: cannot record the build of {} in {}: {error}",
                request.product, request.tree
            );
        }
    }

    /// Forgets the recorded builds of `tree`.
    pub(super) fn forget_stored(&self, tree: &TreeId) {
        if let Some(conn) = lock(&self.store).as_ref()
            && let Err(error) = builds::clear_tree(conn, tree.as_str())
        {
            eprintln!("ignite: cannot forget the recorded builds of {tree}: {error}");
        }
    }
}

/// Deletes a row that [`BuildManager::restore`] found no longer true.
fn drop_row(conn: &Connection, row: &rows::Build) {
    if let Err(error) = builds::remove(conn, &row.tree, &row.product, &row.packages) {
        eprintln!(
            "ignite: cannot drop the recorded build of {}: {error}",
            row.product
        );
    }
}
