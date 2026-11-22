//! Local volume provisioning. The fleet edit uses the same versioned write as `ignite fleet`.

use std::path::Path;

use ignite_core::protocol::client::Reply;

use crate::args::TreesCommand;

#[cfg(target_os = "macos")]
mod macos;

pub fn run(command: &TreesCommand, cwd: &Path) -> Reply {
    let TreesCommand::Volume { state, size_gb } = *command;
    #[cfg(target_os = "macos")]
    return macos::run(cwd, state, size_gb);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (cwd, state, size_gb);
        Reply::text(
            ignite_core::protocol::client::Exit::Success,
            "Set trees_root and tree_space_gb by hand with ignite fleet machine <name>.",
        )
    }
}
