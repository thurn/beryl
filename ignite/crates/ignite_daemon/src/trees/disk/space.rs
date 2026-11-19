//! Measuring the trees' build directories when their files may be clones of each other's.
//!
//! A tree seeded from another (see `trees/seed.rs`) holds APFS clones of the donor's files, which
//! share their blocks until either copy is written. Summing each file's allocated blocks counted
//! those blocks once in every tree that held them, so on 2026-11-18 the disk loop measured
//! hundreds of gigabytes the disk did not hold and deleted the build directories of every free
//! tree, which are the donors the next lease would have copied from.
//!
//! So each file counts its private bytes (which deleting it frees) in its own tree, and the bytes
//! it shares under its clone ID once for the trees together. A clone that either copy has written
//! since gets a new clone ID, and its shared blocks then count once under each ID, which
//! overcounts only those clones.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ignite_core::ids::TreeId;
use ignite_os::Native;
use ignite_os::clones::Clones as _;

/// The bytes the trees' build directories hold, measured together.
#[derive(Debug, Default)]
pub(super) struct Space {
    trees: BTreeMap<TreeId, Held>,
}

/// What one tree's build directories hold.
#[derive(Debug, Default)]
struct Held {
    /// Bytes no other file shares.
    private: u64,
    /// Bytes shared with clones, by clone ID.
    shared: HashMap<u64, u64>,
}

impl Space {
    /// Measures each tree's directories, not following symbolic links. A directory that does not
    /// exist holds nothing.
    pub(super) fn measure(trees: impl IntoIterator<Item = (TreeId, Vec<PathBuf>)>) -> Space {
        let mut space = Space::default();
        for (id, dirs) in trees {
            let held = space.trees.entry(id).or_default();
            for dir in dirs {
                add(held, &dir);
            }
        }
        space
    }

    /// Bytes on disk in every tree's directories, each clone's shared bytes counted once.
    pub(super) fn total(&self) -> u64 {
        let mut shared: HashMap<u64, u64> = HashMap::new();
        let mut private = 0;
        for held in self.trees.values() {
            private += held.private;
            for (&id, &bytes) in &held.shared {
                let most = shared.entry(id).or_default();
                *most = (*most).max(bytes);
            }
        }
        private + shared.values().sum::<u64>()
    }

    /// Whether the tree's directories hold any bytes at all.
    pub(super) fn holds_any(&self, tree: &TreeId) -> bool {
        self.trees
            .get(tree)
            .is_some_and(|held| held.private > 0 || !held.shared.is_empty())
    }

    /// Bytes that deleting `tree`'s directories would free if the trees in `also` were deleted
    /// too: its private bytes, and the bytes it shares only with trees in `also`.
    pub(super) fn freed_with(&self, tree: &TreeId, also: &HashSet<TreeId>) -> u64 {
        let Some(held) = self.trees.get(tree) else {
            return 0;
        };
        let shared: u64 = held
            .shared
            .iter()
            .filter(|(clone, _)| {
                self.trees.iter().all(|(other, theirs)| {
                    other == tree || also.contains(other) || !theirs.shared.contains_key(clone)
                })
            })
            .map(|(_, bytes)| bytes)
            .sum();
        held.private + shared
    }

    /// Forgets `tree`'s directories, as after they are deleted, and returns the bytes that frees.
    pub(super) fn clear(&mut self, tree: &TreeId) -> u64 {
        let freed = self.freed_with(tree, &HashSet::new());
        self.trees.remove(tree);
        freed
    }

    /// Each tree's bytes that deleting its directories alone would free.
    pub(super) fn own(&self) -> Vec<(TreeId, u64)> {
        self.trees
            .keys()
            .map(|id| (id.clone(), self.freed_with(id, &HashSet::new())))
            .collect()
    }
}

/// Adds what is under `path` to `held`.
fn add(held: &mut Held, path: &Path) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    let allocated = allocated(&meta);
    if meta.is_file() {
        match Native.sharing(path) {
            Ok(Some(sharing)) => {
                let private = sharing.private.min(allocated);
                held.private += private;
                if let Some(clone) = sharing.clone_id.filter(|_| private < allocated) {
                    let bytes = held.shared.entry(clone).or_default();
                    *bytes = (*bytes).max(allocated - private);
                }
            }
            _ => held.private += allocated,
        }
        return;
    }
    held.private += allocated;
    if !meta.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.filter_map(io::Result::ok) {
        add(held, &entry.path());
    }
}

/// The bytes a file's blocks take on disk.
#[cfg(unix)]
fn allocated(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

/// The file's length. Windows reports no block count without opening the file, so a sparse or
/// compressed file counts at its full length.
#[cfg(windows)]
fn allocated(meta: &fs::Metadata) -> u64 {
    meta.len()
}

#[cfg(test)]
#[path = "space_tests.rs"]
mod tests;
