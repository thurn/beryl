//! Cloning a donor tree's Cargo target directories for a seed (see the parent module).
//!
//! A seed of three target directories (about 560,000 files) copied each file on its own,
//! opened it again to set its modification time, and took 134 to 680 s under load on
//! 2026-11-19. The session's builds waited for all of it. Each directory is now cloned in one
//! call where the system has one ([`Clones::clone_tree`]), which kept every file's time and
//! took 7 s for a 72,000-file target directory under the same load. The file-by-file copy is
//! the fallback: on Windows, and wherever the one call fails.
//!
//! Only what Cargo's lock files protect is copied: the profile directories and the files at
//! the target directory's root. A leased donor's tests write Cargo's `tmp` directory and
//! nextest's directory after the build has released its locks, and seeds failed with "No such
//! file or directory" and "Permission denied" while copying such files.
//!
//! [`Clones::clone_tree`]: ignite_os::clones::Clones::clone_tree

use std::fs;
use std::io;
use std::path::Path;

use ignite_os::Native;
use ignite_os::clones::Clones as _;

use super::super::disk::FINGERPRINTS;
use super::{remove, set_modified};

/// Copies the Cargo target directory `from` to `to`, which must not exist: every file at its
/// root and every directory that is a profile directory or holds one (a target triple's), each
/// file keeping its modification time. Other directories, written by tests outside Cargo's
/// lock, are left out, and so is an entry that disappears while it is copied.
pub(super) fn clone_target(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let Some(kind) = gone_is_none(entry.file_type())? else {
            continue;
        };
        if !kind.is_dir() {
            copy_entry(&source, &target, kind)?;
        } else if holds_profiles(&source) {
            clone_dir(&source, &target)?;
        }
    }
    Ok(())
}

/// Whether the directory `dir` is a Cargo profile directory or holds one.
fn holds_profiles(dir: &Path) -> bool {
    dir.join(FINGERPRINTS).is_dir()
        || fs::read_dir(dir).is_ok_and(|entries| {
            entries
                .filter_map(Result::ok)
                .any(|entry| entry.path().join(FINGERPRINTS).is_dir())
        })
}

/// Copies the directory `from` to `to`, which must not exist, keeping each file's modification
/// time: in one call where the system clones directories, and otherwise file by file. Each file
/// is a clone on APFS and on a block-cloning `ReFS` volume.
pub(in crate::trees) fn clone_dir(from: &Path, to: &Path) -> io::Result<()> {
    if Native.clone_tree(from, to).is_ok() {
        return Ok(());
    }
    // The call fails as a whole when an entry changes under it, and may leave part of the copy.
    remove(to)?;
    walk(from, to, &|_| {})
}

/// [`clone_dir`] file by file. `before` is called with each entry's path before it is copied.
/// An entry that disappears before it is copied is left out: a test or a tool deleted it, and
/// Cargo rebuilds a unit whose output is missing.
pub(super) fn walk(from: &Path, to: &Path, before: &dyn Fn(&Path)) -> io::Result<()> {
    let Some(entries) = gone_is_none(fs::read_dir(from))? else {
        return Ok(());
    };
    fs::create_dir_all(to)?;
    for entry in entries {
        let entry = entry?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        before(&source);
        let Some(kind) = gone_is_none(entry.file_type())? else {
            continue;
        };
        if kind.is_dir() {
            walk(&source, &target, before)?;
        } else {
            copy_entry(&source, &target, kind)?;
        }
    }
    Ok(())
}

/// Copies the file or link `from` to `to`; a file keeps its modification time. Nothing is
/// copied when `from` is gone.
fn copy_entry(from: &Path, to: &Path, kind: fs::FileType) -> io::Result<()> {
    if kind.is_symlink() {
        return gone_is_none(copy_link(from, to)).map(drop);
    }
    let Some(modified) = gone_is_none(fs::symlink_metadata(from).and_then(|m| m.modified()))?
    else {
        return Ok(());
    };
    if gone_is_none(fs::copy(from, to))?.is_none() {
        return Ok(());
    }
    set_modified(to, modified)
}

/// `Ok(None)` for an error that says the file is gone.
fn gone_is_none<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(from)?, to)
}

/// Windows makes a symbolic link only with Developer Mode or an administrator's rights, and
/// Cargo writes none there, so a link is copied as the file it points at.
#[cfg(not(unix))]
fn copy_link(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to).map(drop)
}
