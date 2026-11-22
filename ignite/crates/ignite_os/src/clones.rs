//! Filesystem support for cloning build output directories.

use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub trait Clones {
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
