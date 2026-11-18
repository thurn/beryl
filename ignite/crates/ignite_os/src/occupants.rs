//! The processes inside a directory: those whose working directory or executable is under it.
//! Releasing a build tree ends them, whether Ignite started them or an agent did by hand and
//! forgot, such as a development server left running.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// A process found inside a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occupant {
    pub pid: u32,
    /// The executable's path, when the operating system reports it.
    pub executable: Option<PathBuf>,
    /// The working directory, when the operating system reports it. Windows never does: a
    /// process's working directory is readable only from inside its memory.
    pub cwd: Option<PathBuf>,
}

pub trait Occupants {
    /// Every live process whose working directory or executable is `dir` or under it. `dir` is
    /// compared after resolving symbolic links, as the kernel reports real paths. Processes this
    /// user may not inspect are left out. On Windows only the executable is compared, without
    /// regard to case.
    fn occupants(&self, dir: &Path) -> io::Result<Vec<Occupant>>;

    /// The parent of `pid`, or `None` once it has exited.
    fn parent(&self, pid: u32) -> Option<u32>;

    /// The process group of `pid`, or `None` once it has exited. A shell puts every process of
    /// one command line in one group, such as both sides of `ignite close | tail`. Windows has
    /// no process groups and always answers `None`.
    fn group(&self, pid: u32) -> Option<u32>;

    /// Sends `SIGTERM` to each process, waits up to `grace` for them to exit and sends `SIGKILL`
    /// to those still running. Returns the processes that needed `SIGKILL`.
    ///
    /// Windows has no request to exit that every process answers, so there each process is
    /// ended at once with `TerminateProcess`, the call waits up to `grace` for them to be gone,
    /// and every process it ended is returned.
    fn end(&self, pids: &[u32], grace: Duration) -> io::Result<Vec<u32>>;
}
