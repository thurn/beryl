//! Facts about the running machine that `ignite doctor` and `ignite status` read from the
//! operating system.

use std::path::{Path, PathBuf};
use std::process::Command;

use ignite_os::hostcpu::HostCpu;

/// Facts about the machine that need the operating system.
pub trait Host {
    fn hostname(&self) -> String;
    fn cores(&self) -> u32;
    fn physical_cores(&self) -> u32 {
        self.cores()
    }
    fn memory_mb(&self) -> u64;
    /// Gigabytes free on the disk that holds `path`.
    fn disk_free_gb(&self, path: &Path) -> Option<u64>;
    /// Size of the volume and bytes available to the daemon.
    fn disk_space(&self, _path: &Path) -> Option<ignite_os::system::DiskSpace> {
        None
    }
    /// Where the program called `name` is found on the search path.
    fn tool_path(&self, name: &str) -> Option<PathBuf>;
    fn path_exists(&self, path: &Path) -> bool;
}

/// The running machine.
pub struct NativeHost;

pub(crate) fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl Host for NativeHost {
    /// The short hostname in lower case, the way `ignite.toml` names machines: macOS reports
    /// the computer name's case, such as `JARVIS`.
    ///
    /// Windows's `hostname` takes no `-s`, and the computer name is in `COMPUTERNAME` in upper
    /// case, such as `CYGNUS`.
    fn hostname(&self) -> String {
        if cfg!(windows) {
            return std::env::var("COMPUTERNAME")
                .unwrap_or_default()
                .to_lowercase();
        }
        command_output("hostname", &["-s"])
            .unwrap_or_default()
            .to_lowercase()
    }

    fn cores(&self) -> u32 {
        ignite_os::Native.cores()
    }

    fn physical_cores(&self) -> u32 {
        ignite_os::Native.physical_cores()
    }

    fn memory_mb(&self) -> u64 {
        #[cfg(windows)]
        let bytes = ignite_os::system::total_memory_bytes().ok();
        #[cfg(not(windows))]
        let bytes = command_output("sysctl", &["-n", "hw.memsize"])
            .and_then(|text| text.parse::<u64>().ok());
        bytes.map_or(0, |bytes| bytes / (1024 * 1024))
    }

    fn disk_free_gb(&self, path: &Path) -> Option<u64> {
        ignite_os::system::disk_free_bytes(path)
            .ok()
            .map(|bytes| bytes / ignite_core::capacity::BYTES_PER_GIB)
    }

    fn disk_space(&self, path: &Path) -> Option<ignite_os::system::DiskSpace> {
        ignite_os::system::disk_space(path).ok()
    }

    fn tool_path(&self, name: &str) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH")?).collect();
        // A login service starts with a short `PATH`, so also look where the usual installers put
        // tools.
        if let Some(home) = ignite_core::config::home_dir(|name| std::env::var_os(name)) {
            dirs.push(home.join(".cargo").join("bin"));
        }
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        // A Windows program's file ends in `.exe`, which its name on a command line leaves out.
        let file = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        };
        dirs.into_iter()
            .map(|dir| dir.join(&file))
            .find(|p| p.is_file())
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }
}
