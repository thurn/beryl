//! Starting the daemon at login: a launch agent on macOS, a scheduled task on Windows.

use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub trait Service {
    /// Registers `executable args` to start at login, replacing an earlier registration.
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()>;
    fn uninstall(&self) -> io::Result<()>;
    fn is_installed(&self) -> io::Result<bool>;
}
