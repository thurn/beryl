//! Process groups (job objects on Windows): every process a command starts can be ended at once,
//! when its tree is removed, its ticket finishes or its lease expires.

use std::io;
use std::process::{Child, Command};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{become_program, replace_image};
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(windows)]
pub use windows::become_program;

/// The exit code of every process that Ignite ends on Windows, where a process ended from
/// outside still exits with a code: `STATUS_CONTROL_C_EXIT`, the code of a console process
/// ended by Ctrl+C. Ignite reads it as macOS's "ended by a signal", with no exit code.
#[cfg(windows)]
pub const ENDED_EXIT_CODE: u32 = 0xC000_013A;

/// A started command and the group that holds its descendants.
#[derive(Debug)]
pub struct Group {
    pub child: Child,
    /// The process group ID (macOS) or the number in the job object's name (Windows). Both are
    /// the leader's process ID.
    pub id: u64,
}

pub trait Processes {
    /// Starts `command` as the leader of a new group.
    fn spawn_group(&self, command: &mut Command) -> io::Result<Group>;
    /// Ends every process in the group.
    fn end_group(&self, group: &mut Group) -> io::Result<()>;
    /// Makes `command` start apart from this process, so that ending this process with its
    /// process group (macOS) or its console (Windows) leaves the command running.
    fn detach(&self, command: &mut Command);
}
