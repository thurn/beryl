//! The monitor's real-machine implementation: the login service, the daemon's socket, the
//! hub's HTTP endpoint and the desktop notification helper.

use std::fs;
use std::path::{Path, PathBuf};

use ignite_core::protocol::client::Request;
use ignite_core::time::Timestamp;
use ignite_os::Native;
use ignite_os::service::Service;

use super::storage::{self, RebootPolicy, StuckStorage};
use super::{Machine, PROBE_TIMEOUT, justiciar, with_deadline};
#[cfg(windows)]
use super::{daemons_to_end, start_log};
use crate::update::Home;

/// Whether the daemon listening on `socket` answers an `alive` request within [`PROBE_TIMEOUT`]
/// ([`crate::doctor::AliveVerb`]), which asks nothing of the hub, git or the disk, so a slow hub
/// or locked database never makes a working daemon look hung. Any reply counts: a daemon built
/// before `alive` existed answers that the verb is unknown, which shows it is serving as well.
pub fn probe_daemon(socket: &Path) -> bool {
    let request = Request {
        verb: crate::doctor::ALIVE_VERB.to_owned(),
        args: Vec::new(),
        cwd: socket.parent().map(PathBuf::from).unwrap_or_default(),
        session: None,
        wait_s: PROBE_TIMEOUT.as_secs() as u32,
        pid: Some(std::process::id()),
    };
    let socket = socket.to_string_lossy().into_owned();
    // A daemon that accepts the connection and never answers leaves the thread behind.
    with_deadline(PROBE_TIMEOUT, move || {
        crate::server::send(&socket, &request).is_ok()
    })
    .unwrap_or(false)
}

/// The real machine: the login service for the process and the kickstart, the daemon's socket
/// for the probe and the desktop for notifications.
pub struct NativeMachine {
    pub socket: PathBuf,
    /// The Ignite home, which records the main checkout.
    pub home: PathBuf,
}

impl Machine for NativeMachine {
    fn daemon_running(&self) -> bool {
        let home = Home {
            root: self.home.clone(),
        };
        // On Windows a self-update starts the new daemon detached and exits the scheduled task,
        // so Task Scheduler reports the task as Ready while the daemon holds `daemon.lock`.
        if cfg!(windows) && crate::machine_lock::held(&home) {
            return true;
        }
        // When the login service cannot say, the probe decides.
        Native.daemon_running().unwrap_or(true)
    }

    fn probe(&self) -> bool {
        probe_daemon(&self.socket)
    }

    fn kickstart(&self) -> std::io::Result<()> {
        #[cfg(windows)]
        {
            let home = Home {
                root: self.home.clone(),
            };
            // A daemon restarted by a Windows self-update runs detached from the scheduled task,
            // so `schtasks /End` cannot stop it if it hangs. Stop every detached daemon before
            // starting the task again so the new daemon does not hit `daemon.lock`.
            if crate::machine_lock::held(&home) {
                for pid in daemons_to_end(&start_log(&home).read()) {
                    let _ = Native.terminate_under(pid, home.store().root());
                }
            }
        }
        Native.kickstart_daemon()
    }

    fn notify(&self, title: &str, text: &str) {
        ignite_os::notify::post(title, text);
    }

    fn hub_here(&self) -> bool {
        Native.hub_installed().unwrap_or(false)
    }

    fn probe_hub(&self) -> bool {
        // The address the hub's service listens on; a service that names none is not probed.
        let Ok(Some(listen)) = Native.recorded_variable(ignite_os::service::HUB_LISTEN_VARIABLE)
        else {
            return true;
        };
        hub_answers(&listen)
    }

    fn kickstart_hub(&self) -> std::io::Result<()> {
        Native.kickstart_hub()
    }

    fn refresh_daemon(&self) -> std::io::Result<bool> {
        Native.refresh_daemon()
    }

    fn justiciar_app(&self) -> Option<justiciar::JusticiarApp> {
        let (checkout, config) = self.config()?;
        justiciar::first_cli_app(&config, &checkout)
    }

    fn locked(&self, lock: &Path) -> bool {
        justiciar::lock_held(lock)
    }

    fn start_justiciar(&self, turn: &justiciar::JusticiarTurn) -> std::io::Result<()> {
        justiciar::spawn_detached(turn)
    }

    fn daemon_log_tail(&self, lines: usize) -> String {
        let bytes = if cfg!(windows) {
            crate::daemon_log::read(&self.home)
        } else {
            // The log file launchd writes on macOS (`ignite_os::service`).
            let Some(user_home) = ignite_core::config::home_dir(|name| std::env::var_os(name))
            else {
                return String::new();
            };
            fs::read(
                user_home
                    .join("Library")
                    .join("Logs")
                    .join("ignite.daemon.log"),
            )
        };
        let Ok(bytes) = bytes else {
            return String::new();
        };
        let text = String::from_utf8_lossy(&bytes);
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    fn trees_roots(&self) -> Vec<PathBuf> {
        storage::roots(&self.home())
    }

    fn probe_storage(&self, root: &Path) -> bool {
        storage::probe_detached(&self.home(), root)
    }

    fn reboot_policy(&self) -> RebootPolicy {
        self.config()
            .map_or_else(RebootPolicy::default, |(_, config)| {
                RebootPolicy::of(&config, &hostname())
            })
    }

    fn booted_at(&self) -> Option<Timestamp> {
        ignite_os::system::booted_at_millis()
            .ok()
            .map(Timestamp::from_millis)
    }

    fn report_storage(&self, stuck: &StuckStorage) -> Result<(), String> {
        let (_, config) = self
            .config()
            .ok_or("cannot read the main checkout's configuration")?;
        let identity = self
            .home()
            .identity()
            .load()
            .map_err(|error| error.to_string())?
            .ok_or("this machine has no identity")?;
        let token = identity.token.ok_or("this machine is not registered")?;
        let client =
            crate::hubclient::HubClient::new(&config.hub, &token, identity.machine.as_str());
        let report = storage::hub_report(stuck, &hostname());
        // The hub client retries an unreachable hub; the monitor waits for it no longer than for
        // a probe, and tries again on its next run.
        match with_deadline(storage::PROBE_DEADLINE, move || {
            client
                .storage_stuck(&report)
                .map_err(|error| error.to_string())
        }) {
            Some(Ok(_)) => Ok(()),
            Some(Err(error)) => Err(error),
            None => Err("the hub did not answer in time".to_owned()),
        }
    }

    fn restart(&self) -> std::io::Result<()> {
        Native.restart_machine()
    }

    fn run_command(&self, command: &[String]) -> std::io::Result<()> {
        let (program, args) = command
            .split_first()
            .ok_or_else(|| std::io::Error::other("the command is empty"))?;
        let status = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .status()?;
        if status.success() {
            return Ok(());
        }
        Err(std::io::Error::other(format!("it exited with {status}")))
    }
}

impl NativeMachine {
    fn home(&self) -> Home {
        Home {
            root: self.home.clone(),
        }
    }

    /// The main checkout the Ignite home records, and its `ignite.toml`.
    fn config(&self) -> Option<(PathBuf, ignite_core::config::Config)> {
        let checkout = PathBuf::from(fs::read_to_string(self.home().checkout_file()).ok()?.trim());
        let config = crate::main::read_config(&checkout).ok()?;
        Some((checkout, config))
    }
}

fn hostname() -> String {
    crate::doctor::Host::hostname(&crate::doctor::NativeHost)
}

/// Whether the hub listening at `listen` answers an HTTP request within [`PROBE_TIMEOUT`]. Any
/// answer counts, whatever its status: it shows the hub is serving.
pub fn hub_answers(listen: &str) -> bool {
    // An address that listens everywhere is reached through the loopback address.
    let address = listen
        .replace("0.0.0.0:", "127.0.0.1:")
        .replace("[::]:", "[::1]:");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(PROBE_TIMEOUT))
        .proxy(None)
        .build()
        .into();
    agent.get(&format!("http://{address}/")).call().is_ok()
}
