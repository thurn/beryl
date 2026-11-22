//! Starting again, when the daemon starts, the seeds a restart cut short (see `seed_swap.rs` for
//! the seed directory that marks one).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use ignite_core::traits::Result;
use ignite_git::refs;
use ignite_os::Native;
use ignite_os::clones::Clones as _;
use ignite_store::local::trees as store;

use super::super::{Kind, Trees, disk::id_of, store_error};
use super::Seeding;
use super::swap::{delete, discarded_prefix, set_aside, staging};

impl Trees {
    /// Starts the seed again of every leased ticket tree whose seed directory is still there,
    /// which means the daemon stopped before that seed ended, and deletes the seed directories
    /// of every other tree and those that ended seeds set aside. Each resumed seed measures
    /// donors against the tree's checked-out commit and runs on a thread of its own, followed by
    /// the prebuild products, as a lease's would. The trees are marked as being seeded before
    /// this returns, so it runs before the daemon serves its socket: a build that a verb asks
    /// for after the restart then waits for the copy. Returns the trees resumed.
    pub fn resume_seeds(&self) -> Result<Vec<ignite_core::ids::TreeId>> {
        let rows = store::all(&self.lock().db).map_err(|e| store_error(&e))?;
        let root = self.root();
        let roots: BTreeSet<_> = rows
            .iter()
            .filter_map(|row| Path::new(&row.path).parent())
            .chain(std::iter::once(root.as_path()))
            .collect();
        for root in roots {
            let Ok(entries) = fs::read_dir(root) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(discarded_prefix())
                {
                    delete(entry.path());
                }
            }
        }
        let mut resumed = Vec::new();
        for row in rows {
            let tree = id_of(&row)?;
            let path = std::path::PathBuf::from(&row.path);
            if fs::symlink_metadata(staging(&path)).is_err() {
                continue;
            }
            let leased = row.state == store::State::Leased.as_str();
            let at = refs::head(&path);
            match at {
                Ok(at)
                    if leased
                        && row.kind == Kind::Ticket.as_str()
                        && Native.copies_are_clones(&path) =>
                {
                    eprintln!("ignite: starting {tree}'s seed again, which a restart cut short");
                    let seeding = Seeding::start(path, tree.clone());
                    self.spawn_builds(Some(seeding), &tree, at.clone(), at, Vec::new());
                    resumed.push(tree);
                }
                _ => {
                    if let Some(left) = set_aside(&staging(&path))? {
                        delete(left);
                    }
                }
            }
        }
        Ok(resumed)
    }
}
