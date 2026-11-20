//! The tables of `ignite.toml` that say how each app's workers run on a machine (see "CLI mode"
//! in the design document).
//!
//! ```toml
//! # How each app's workers run on this machine: "desktop" (the default) or "cli".
//! [machine.jarvis.pool.claude]
//! mode = "cli"
//! slots = 8
//!
//! # The program and the arguments every turn of the app gets.
//! [pool.cli.claude]
//! program = "claude"
//! args = ["--dangerously-skip-permissions"]
//! ```
//!
//! An app with no `[machine.<host>.pool.<app>]` table runs in desktop mode with no CLI slots, so
//! a file written before CLI mode existed means what it always meant.

use serde::{Deserialize, Serialize};

/// How one app's workers run on one machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PoolMode {
    /// A dispatcher session in the desktop app makes the tool calls the daemon asks for.
    #[default]
    Desktop,
    /// The daemon runs each worker itself as a headless CLI process, one process per turn.
    Cli,
}

impl PoolMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PoolMode::Desktop => "desktop",
            PoolMode::Cli => "cli",
        }
    }
}

impl std::fmt::Display for PoolMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `[machine.<host>.pool.<app>]`: the mode an app runs in on the machine and, in CLI mode, how
/// many workers run at once. One slot above `slots` takes only tickets that hold the master
/// branch back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachinePool {
    #[serde(default)]
    pub mode: PoolMode,
    /// CLI workers that run at once; ignored in desktop mode.
    #[serde(default)]
    pub slots: u32,
}

/// `[pool.cli.<app>]`: the program every CLI turn of the app runs and the arguments it always
/// gets, before the ones each turn adds (model, effort, transcript, prompt).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliProgram {
    /// The executable, looked up on `PATH` when it has no directory.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl super::Config {
    /// How `app` runs on the machine with this hostname: its `[machine.<host>.pool.<app>]`
    /// table, or desktop mode with no slots when the machine or the table is missing.
    pub fn machine_pool(&self, hostname: &str, app: &str) -> MachinePool {
        self.machine(hostname)
            .and_then(|machine| machine.pool.get(app))
            .copied()
            .unwrap_or_default()
    }

    /// The program and arguments of `app`'s CLI turns, if `[pool.cli.<app>]` declares them.
    pub fn cli_program(&self, app: &str) -> Option<&CliProgram> {
        self.pool.cli.get(app)
    }

    /// `[pool.cli]` and every machine's `pool` tables: a declared program is not empty, and an
    /// app in CLI mode has slots and a program.
    pub(super) fn validate_cli(&self) -> Result<(), (String, String)> {
        let fail = |key: String, message: &str| Err((key, message.to_owned()));
        for (app, program) in &self.pool.cli {
            if program.program.trim().is_empty() {
                return fail(format!("pool.cli.{app}.program"), "must not be empty");
            }
        }
        for (name, machine) in &self.machine {
            for (app, pool) in &machine.pool {
                if pool.mode != PoolMode::Cli {
                    continue;
                }
                if pool.slots == 0 {
                    return fail(
                        format!("machine.{name}.pool.{app}.slots"),
                        "must be above zero in cli mode",
                    );
                }
                if !self.pool.cli.contains_key(app) {
                    return fail(
                        format!("machine.{name}.pool.{app}.mode"),
                        "cli mode needs a [pool.cli.<app>] table",
                    );
                }
            }
        }
        Ok(())
    }
}
