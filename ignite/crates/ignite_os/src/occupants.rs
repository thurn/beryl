//! The processes inside a directory: those whose working directory or executable is under it.
//! Releasing a build tree ends them, whether Ignite started them or an agent did by hand and
//! forgot, such as a development server left running.
//!
//! It also answers what still runs under a process ([`Occupants::live_descendants`]), which is
//! how the end of a worker's turn tells a worker waiting on a background command from one that
//! has stopped: the agent app runs each background command as a child of its own process.

use std::collections::BTreeSet;
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

    /// The live processes whose parent is `pid`, leaving out those waiting to be reaped. On
    /// Windows, where a process ID can be reused while a child still names its parent's old
    /// one, only children started after `pid` count.
    fn children(&self, pid: u32) -> io::Result<Vec<u32>>;

    /// Every live process under `root` (its children, theirs and so on), leaving out the branch
    /// that holds `exclude`: the child of `root` that `exclude` descends from, and all under it.
    /// A worker's `Stop` hook passes its own process as `exclude`, since the hook also runs
    /// under the session's process. A process that exits during the walk is skipped.
    fn live_descendants(&self, root: u32, exclude: Option<u32>) -> io::Result<Vec<u32>> {
        let mut skipped = BTreeSet::new();
        let mut at = exclude;
        // The ancestors of `exclude` up to `root`, `exclude` included. A chain that never
        // reaches `root` is not under it and leaves nothing out.
        while let Some(pid) = at.filter(|&pid| pid != root && pid > 1) {
            if !skipped.insert(pid) {
                break;
            }
            at = self.parent(pid);
        }
        if at != Some(root) {
            skipped.clear();
        }
        let mut found = Vec::new();
        let mut next = self.children(root)?;
        let mut seen = BTreeSet::from([root]);
        while let Some(pid) = next.pop() {
            if skipped.contains(&pid) || !seen.insert(pid) {
                continue;
            }
            found.push(pid);
            next.extend(self.children(pid).unwrap_or_default());
        }
        found.sort_unstable();
        Ok(found)
    }

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
