//! How much of a file's disk space it shares with clones of it. On APFS a copy made with
//! `clonefile` shares every block with its source until either is written, so the block count
//! that `stat` reports counts the shared blocks once in each copy.
//! On Windows only a volume that counts block references (`ReFS`, as on a Dev Drive) clones a
//! copy; see `clones/windows.rs` for how such a file's sharing is reported.

use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// A file's private bytes and the data stream it may share with clones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sharing {
    /// The bytes on disk that no other file or snapshot shares, which deleting the file frees.
    pub private: u64,
    /// The data stream's ID. Clones that neither copy has written since have the same one.
    /// `None` where the file system has no clones.
    pub clone_id: Option<u64>,
}

pub trait Clones {
    /// What of the file at `path` is its own, not following a symbolic link. `None` when the
    /// file system cannot say, and the caller counts every allocated byte as the file's own.
    fn sharing(&self, path: &Path) -> io::Result<Option<Sharing>>;

    /// Whether `std::fs::copy` to a file beside `path` clones the source's blocks instead of
    /// writing them again, so that copying a tree's build costs seconds and almost no disk.
    fn copies_are_clones(&self, path: &Path) -> bool;

    /// Clones the directory `from`, with everything under it, to `to`, which must not exist,
    /// in one call: each file keeps its modification time and mode and shares its blocks with
    /// its source. A symbolic link is cloned as a link. Copying a 72,000-file Cargo target
    /// directory file by file took minutes under load; this took 7 s on 2026-11-19. Fails with
    /// [`io::ErrorKind::Unsupported`] where there is no such call, and with whatever the file
    /// system says when an entry changes under it, after which `to` may hold part of the copy.
    fn clone_tree(&self, from: &Path, to: &Path) -> io::Result<()>;
}
