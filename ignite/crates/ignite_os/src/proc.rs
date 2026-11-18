//! Process groups (job objects on Windows): every process a command starts can be ended at once,
//! when its tree is removed, its ticket finishes or its lease expires.

use std::io;
use std::process::{Child, Command};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// A started command and the group that holds its descendants.
#[derive(Debug)]
pub struct Group {
    pub child: Child,
    /// The process group ID (macOS) or job handle value (Windows).
    pub id: u64,
}

pub trait Processes {
    /// Starts `command` as the leader of a new group.
    fn spawn_group(&self, command: &mut Command) -> io::Result<Group>;
    /// Ends every process in the group.
    fn end_group(&self, group: &mut Group) -> io::Result<()>;
}
