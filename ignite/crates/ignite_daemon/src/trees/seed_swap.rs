//! Swapping a seeded tree's Cargo target directories for the copy made beside it, while no
//! build in the tree runs (see `copy_from` in the parent module).

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use super::super::disk::profile_dirs;
use super::CARGO_LOCKS;

/// Where the copy for the tree at `to` is made before it replaces the tree's target
/// directories: a directory beside the tree, so that the copy is on the same volume (a clone
/// and a rename need that) and outside the tree's worktree, where `git status` would list it.
/// It holds `new/<dir>` for each copied directory and `old/<dir>` for each directory it
/// replaced, and is deleted when the seed ends.
pub(super) fn staging(to: &Path) -> PathBuf {
    let name = to
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    to.with_file_name(format!(".{name}.seeding"))
}

/// Every profile directory the tree at `to` has in `own`, and every one the donor at `from`
/// has in `dirs`, at the same place in the tree: the directories whose Cargo locks a build in
/// the tree would take, now or once the copy is in place.
pub(super) fn tree_profiles(
    from: &Path,
    to: &Path,
    own: &[String],
    dirs: &[String],
) -> Vec<PathBuf> {
    let mut profiles: Vec<PathBuf> = own
        .iter()
        .flat_map(|dir| profile_dirs(&to.join(dir)))
        .collect();
    for dir in dirs {
        for profile in profile_dirs(&from.join(dir)) {
            if let Ok(relative) = profile.strip_prefix(from) {
                profiles.push(to.join(relative));
            }
        }
    }
    profiles.sort();
    profiles.dedup();
    profiles
}

/// Holds Cargo's lock files in each of `profiles` exclusively, creating any that is missing,
/// so that no build in the tree runs while its target directories are swapped. `None` when a
/// build holds one now.
pub(super) fn tree_locks(profiles: &[PathBuf]) -> io::Result<Option<Vec<File>>> {
    let mut held = Vec::new();
    for profile in profiles {
        fs::create_dir_all(profile)?;
        for name in CARGO_LOCKS {
            let file = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(profile.join(name))?;
            match file.try_lock() {
                Ok(()) => held.push(file),
                Err(fs::TryLockError::WouldBlock) => return Ok(None),
                Err(fs::TryLockError::Error(error)) => return Err(error),
            }
        }
    }
    Ok(Some(held))
}

/// Renames the directory `from` to `to`, creating `to`'s parent. A missing `from` is no error.
pub(super) fn move_dir(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(from).is_err() {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(from, to)
}
