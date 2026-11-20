//! Starting the daemon at login: a launch agent on macOS, a scheduled task on Windows.
//!
//! The hub's machine has a third login service beside the daemon and the monitor: the hub
//! itself, `ignite hub`, so that a daemon restart or crash loop on that machine does not take
//! the hub away from every machine.

use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::{LAUNCH_VERB, LAUNCHER, launch, launcher_for};

/// The variables that tell `ignite hub` where to listen: the writable listener, and the
/// read-only one that the public address proxies. The hub's login service starts with them,
/// and the daemon's does not.
pub const HUB_VARIABLES: &[&str] = &[HUB_LISTEN_VARIABLE, "IGNITE_HUB_PUBLIC_LISTEN"];

/// The variable that says this is the hub's machine, set there to the address the hub's
/// writable listener listens on, such as `127.0.0.1:7345`.
pub const HUB_LISTEN_VARIABLE: &str = "IGNITE_HUB_LISTEN";

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

    /// Rewrites the daemon's login service as this build would write it, keeping the program,
    /// environment and log file it records, and reloads it, which restarts the daemon. Returns
    /// whether it changed anything. Only `install` writes the service, and an update replaces
    /// the daemon's executable without it, so a setting the service carries (such as the macOS
    /// process type, which decides which cores the daemon's commands run on) otherwise waits
    /// for someone to run `ignite install` again. A platform whose service carries no such
    /// setting, or a service this cannot read, answers `false`.
    fn refresh_daemon(&self) -> io::Result<bool> {
        Ok(false)
    }

    /// Registers the hub: `executable args` starts at login with `environment` (the
    /// [`HUB_VARIABLES`] that are set) and again whenever it exits, replacing an earlier
    /// registration. A platform without one answers [`io::ErrorKind::Unsupported`].
    fn install_hub(
        &self,
        executable: &Path,
        args: &[String],
        environment: &[(String, String)],
    ) -> io::Result<()> {
        let _ = (executable, args, environment);
        Err(unsupported())
    }

    /// Whether this machine has the hub's login service registered.
    fn hub_installed(&self) -> io::Result<bool> {
        Ok(false)
    }

    /// Kills the hub process and starts it again through its login service.
    fn kickstart_hub(&self) -> io::Result<()> {
        Err(unsupported())
    }

    /// The value of the variable `name` that an installed login service starts with: the hub's
    /// when it is registered, else the daemon's. Before the hub had a service of its own, the
    /// daemon's service carried the hub's [`HUB_VARIABLES`], and this is how an install moves
    /// them over. `None` when neither service sets it, or the platform's services carry no
    /// variables.
    fn recorded_variable(&self, name: &str) -> io::Result<Option<String>> {
        let _ = name;
        Ok(None)
    }
}

/// Seconds between two runs of the monitor.
pub const MONITOR_INTERVAL_S: u32 = 120;

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "this login service is not supported on this platform yet",
    )
}
