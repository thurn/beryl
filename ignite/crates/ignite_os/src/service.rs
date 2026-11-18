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

    /// Registers the monitor: `executable args` runs once at login and then every
    /// [`MONITOR_INTERVAL_S`] seconds, and is not restarted when it exits. A platform without
    /// one answers [`io::ErrorKind::Unsupported`].
    fn install_monitor(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        let _ = (executable, args);
        Err(unsupported())
    }

    /// Whether the login service has a daemon process running now.
    fn daemon_running(&self) -> io::Result<bool> {
        Err(unsupported())
    }

    /// Kills the daemon process and starts it again through the login service.
    fn kickstart_daemon(&self) -> io::Result<()> {
        Err(unsupported())
    }
}

/// Seconds between two runs of the monitor.
pub const MONITOR_INTERVAL_S: u32 = 120;

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the monitor is not supported on this platform yet",
    )
}
