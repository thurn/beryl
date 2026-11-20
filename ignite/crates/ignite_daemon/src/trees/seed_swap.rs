//! Swapping a seeded tree's Cargo target directories for the copy made beside it, while no
//! build in the tree runs (see `copy_from` in the parent module), and the directory beside the
//! tree that holds the copy until then.
//!
//! That directory also marks a seed that has not ended. It is made when the lease starts the
//! seed and set aside when the seed ends, so one that a daemon restart cut short is found by the
//! next daemon, which starts that seed again ([`super::Trees::resume_seeds`]). On 2026-11-20 a
//! release restarted the daemon seconds after `ignite start` leased a tree whose build the disk
//! loop had deleted. Its seed died with the old daemon, and the ticket's first `ignite test`
//! compiled about 320 crates from nothing, for 1,377 s.

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

/// The name a seed's directory is renamed to when the seed ends, so that deleting what it
/// holds (the tree's replaced build, which can be hundreds of thousands of files) neither
/// holds up the tree's builds nor collides with the tree's next seed. Every name starting with
/// [`discarded_prefix`] is deleted when the daemon starts.
fn discarded(staging: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = staging
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    staging.with_file_name(format!("{}{name}-{nanos}", discarded_prefix()))
}

/// The start of the name of every set-aside seed directory.
pub(super) fn discarded_prefix() -> &'static str {
    ".discarded"
}

/// Renames the seed directory `staging` aside, if it is there, and returns where it went, for
/// deletion with [`delete`] once nothing waits on the seed.
pub(super) fn set_aside(staging: &Path) -> io::Result<Option<PathBuf>> {
    if fs::symlink_metadata(staging).is_err() {
        return Ok(None);
    }
    let to = discarded(staging);
    fs::rename(staging, &to)?;
    Ok(Some(to))
}

/// Deletes `dir` on a thread of its own.
pub(super) fn delete(dir: PathBuf) {
    std::thread::spawn(move || {
        if let Err(error) = super::remove(&dir) {
            eprintln!("ignite: {} not deleted: {error}", dir.display());
        }
    });
}

/// Empties the seed directory `staging` before a copy is made in it: what an earlier seed of
/// the tree left there is set aside and deleted on another thread.
pub(super) fn clear(staging: &Path) -> io::Result<()> {
    if let Some(left) = set_aside(staging)? {
        delete(left);
    }
    fs::create_dir_all(staging)
}

/// Moves the copied directory `new` to `to`. Cargo creates a target directory and its
/// `CACHEDIR.TAG` before it waits for its lock, so a build that starts while the tree's own
/// directory is moved away can make `to` again before the copy is moved in, and the rename
/// then failed with "Directory not empty" (os error 66). Whatever is at `to` is moved under
/// `old` first, each time, until the rename succeeds or has failed [`SWAP_TRIES`] times.
pub(super) fn swap_in(new: &Path, to: &Path, old: &Path) -> io::Result<()> {
    let mut tries = 0;
    loop {
        tries += 1;
        if fs::symlink_metadata(to).is_ok() {
            move_dir(to, &old.join(format!("made-during-seed-{tries}")))?;
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::rename(new, to) {
            Err(error) if tries < SWAP_TRIES && taken(&error) => {}
            result => return result,
        }
    }
}

/// How often [`swap_in`] tries to move the copy in.
const SWAP_TRIES: u32 = 5;

/// Whether a rename failed because something is at the target: "Directory not empty" on macOS,
/// and "Access is denied" or "Cannot create a file when that file already exists" on Windows.
fn taken(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::DirectoryNotEmpty
            | io::ErrorKind::AlreadyExists
            | io::ErrorKind::PermissionDenied
    )
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
