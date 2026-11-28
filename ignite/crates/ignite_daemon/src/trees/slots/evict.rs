//! `ignite trees evict <tree> [--force]`: deletes a tree's build output so that its build slot is
//! free again, the way a build that needs a slot clears an idle holder (`Trees::take_slot`).
//!
//! A build only clears trees nobody leases. In November 2026 every build slot on one machine was
//! held by a leased tree with build output, the tip round waited 17 minutes for one, and the
//! incident commander deleted the output directories of seven trees by hand. One of those trees
//! was running a test, which broke. This verb deletes the output under the tree's command lock
//! instead, so it never deletes from under a running command. The lease stays with its holder,
//! whose next build starts from nothing.

use std::path::Path;

use ignite_core::ids::TreeId;
use ignite_core::traits::{Error, Result};
use ignite_store::local::trees as store;

use super::super::{Seeding, Trees, disk, store_error};
use super::{describe_occupant, invalidate_artifacts, try_exclusive};

/// What an eviction did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Evicted {
    /// The ticket that leases the tree, whose session is told.
    pub ticket: Option<String>,
    /// The session that leases the tree.
    pub owner: Option<String>,
    /// The commands `--force` stopped first.
    pub stopped: usize,
}

impl Trees {
    /// Deletes `tree`'s build output. While a command runs in the tree it refuses, unless
    /// `force`, which first stops the verbs running there as `ignite release` does.
    pub(crate) fn evict(&self, tree: &TreeId, force: bool) -> Result<Evicted> {
        let row = self.row(tree)?;
        let path = Path::new(&row.path).to_path_buf();
        // The caller's own verb counts only when it was asked from inside the tree.
        let own = crate::steps::current();
        let running = self
            .inner
            .steps
            .in_tree(tree, &path)
            .iter()
            .filter(|activity| {
                own.as_ref()
                    .is_none_or(|own| !std::sync::Arc::ptr_eq(own, activity))
            })
            .count();
        if running > 0 {
            if !force {
                return Err(Error::Conflict(format!(
                    "{tree} has {running} command(s) running; `--force` stops them first"
                )));
            }
            self.stop_work(tree)?;
        }
        let refused = |why: String| Error::Conflict(format!("{tree} was not evicted: {why}"));
        // A supervisor holds the lock shared until its command exits, so holding it exclusively
        // keeps every command out of the tree while its output is deleted.
        let Some(mut guard) = try_exclusive(&path)? else {
            return Err(refused(
                "a command still holds the tree's command lock".to_owned(),
            ));
        };
        let occupants = self.tree_occupants(tree, &path)?;
        if !occupants.is_empty() {
            let names: Vec<String> = occupants.iter().map(describe_occupant).collect();
            return Err(refused(format!(
                "processes run in it: {}",
                names.join(", ")
            )));
        }
        let directories = {
            let mut locked = self.lock();
            if Self::has_build_slot(&locked, tree) {
                return Err(refused("a build there holds its slot".to_owned()));
            }
            if Seeding::involves(&path) || Self::legacy_command_in(&locked, &path) {
                return Err(refused(
                    "a seed or a supervised run of an older daemon uses it".to_owned(),
                ));
            }
            locked.build_space.measuring.remove(tree);
            self.directories(&locked, &row)
        };
        // Clear cached successes before deleting anything, in case this stops partway.
        self.inner.builds.forget(tree);
        invalidate_artifacts(&mut guard)?;
        {
            let locked = self.lock();
            if let Some(mut current) =
                store::get(&locked.db, &row.id).map_err(|error| store_error(&error))?
            {
                current.built_commit = None;
                store::upsert(&locked.db, &current).map_err(|error| store_error(&error))?;
            }
        }
        for dir in &directories {
            disk::remove_output(dir)?;
        }
        drop(guard);
        self.report_cleanup(format!("evicted {tree}'s build directories by request"));
        Ok(Evicted {
            ticket: row.ticket,
            owner: row.owner,
            stopped: running,
        })
    }
}
