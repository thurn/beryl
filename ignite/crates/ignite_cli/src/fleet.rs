//! Operator commands for the fleet settings. They talk to the hub directly.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};

use ignite_core::fleet::{FleetSettings, FleetUpdate, FleetView, MachineState, TreeSpace, summary};
use ignite_core::protocol::client::{Exit, Reply};

use crate::client::session_from_env;
use crate::ticket::http::{Failure, Hub, Result, usage};

#[derive(Debug, Subcommand)]
pub enum FleetCommand {
    /// Change a machine's state and build tree settings.
    Machine {
        name: String,
        #[arg(long, value_enum)]
        state: Option<State>,
        /// Absolute tree directory, or "default" to use the repository setting.
        #[arg(long)]
        trees_root: Option<String>,
        /// Space in decimal gigabytes, or "auto" on a dedicated volume.
        #[arg(long, value_name = "GB|auto")]
        tree_space_gb: Option<TreeSpace>,
        /// Maximum size of one tree's build directories in decimal gigabytes.
        #[arg(long)]
        tree_ceiling_gb: Option<u32>,
    },
    /// Change a provider's settings.
    Provider {
        name: String,
        #[arg(long, conflicts_with = "off")]
        on: bool,
        #[arg(long)]
        off: bool,
        /// Relative weight, from 0 to 1000. Zero turns the provider off.
        #[arg(long)]
        share: Option<u32>,
        /// Hold at this percentage of a weekly plan window, from 1 to 100.
        #[arg(long)]
        stop_percent: Option<u32>,
        /// Hold at this percentage of the 5-hour plan window, from 1 to 100.
        #[arg(long)]
        five_hour_stop_percent: Option<u32>,
        /// Continue past the usage threshold and let the provider enforce its limits.
        #[arg(long, action = clap::ArgAction::Set)]
        ignore_usage_threshold: Option<bool>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum State {
    Active,
    ChecksOnly,
    Off,
}

impl From<State> for MachineState {
    fn from(state: State) -> Self {
        match state {
            State::Active => Self::Active,
            State::ChecksOnly => Self::ChecksOnly,
            State::Off => Self::Off,
        }
    }
}

impl FleetCommand {
    fn apply(&self, settings: &mut FleetSettings) {
        match self {
            Self::Machine {
                name,
                state,
                trees_root,
                tree_space_gb,
                tree_ceiling_gb,
            } => {
                let mut machine = settings.machine(name);
                if let Some(state) = state {
                    machine.state = (*state).into();
                }
                if let Some(root) = trees_root {
                    machine.trees.trees_root = (root != "default").then(|| root.clone());
                }
                if let Some(space) = tree_space_gb {
                    machine.trees.tree_space_gb = Some(*space);
                }
                if let Some(ceiling) = tree_ceiling_gb {
                    machine.trees.tree_ceiling_gb = *ceiling;
                }
                settings.machines.insert(name.clone(), machine);
            }
            Self::Provider {
                name,
                on,
                off,
                share,
                stop_percent,
                five_hour_stop_percent,
                ignore_usage_threshold,
            } => {
                if !*on
                    && !*off
                    && share.is_none()
                    && stop_percent.is_none()
                    && five_hour_stop_percent.is_none()
                    && ignore_usage_threshold.is_none()
                {
                    return;
                }
                let mut provider = settings.provider(name);
                if *on || *off {
                    provider.enabled = *on;
                }
                if let Some(share) = share {
                    provider.share = *share;
                }
                if let Some(stop_percent) = stop_percent {
                    provider.stop_percent = *stop_percent;
                }
                if let Some(five_hour) = five_hour_stop_percent {
                    provider.five_hour_stop_percent = Some(*five_hour);
                }
                if let Some(ignore) = ignore_usage_threshold {
                    provider.ignore_usage_threshold = *ignore;
                }
                settings.providers.insert(name.clone(), provider);
            }
        }
    }
}

pub fn run(command: Option<&FleetCommand>, cwd: &Path) -> Reply {
    let env = |name: &str| std::env::var(name).ok();
    let actor = session_from_env(env).map_or_else(|| "ignite".to_owned(), |id| id.to_string());
    Hub::load(cwd, actor, env)
        .and_then(|hub| execute(&hub, command))
        .unwrap_or_else(Failure::into_reply)
}

pub(crate) fn execute(hub: &Hub, command: Option<&FleetCommand>) -> Result<Reply> {
    let view: FleetView = serde_json::from_str(&hub.get(&["fleet"], &[])?.body).map_err(usage)?;
    let Some(command) = command else {
        return Ok(reply(render(&view), json!(view)));
    };
    edit(command, view.settings, |update| {
        serde_json::from_str(&hub.put_versioned(&["fleet"], &json!(update))?.body).map_err(usage)
    })
}

fn edit(
    command: &FleetCommand,
    mut before: FleetSettings,
    mut put: impl FnMut(&FleetUpdate) -> Result<FleetSettings>,
) -> Result<Reply> {
    let mut attempt = |before: &FleetSettings| {
        let mut settings = before.clone();
        command.apply(&mut settings);
        settings.validate().map_err(usage)?;
        put(&FleetUpdate {
            version: before.version,
            settings,
        })
    };
    let saved = match attempt(&before) {
        Ok(saved) => saved,
        Err(error) if error.exit == Exit::Conflict => {
            before = error
                .body
                .clone()
                .and_then(|body| serde_json::from_value(body).ok())
                .ok_or(error)?;
            attempt(&before)?
        }
        Err(error) => return Err(error),
    };
    Ok(reply(summary(&before, &saved), json!(saved)))
}

fn reply(text: String, json: Value) -> Reply {
    Reply {
        exit: Exit::Success,
        text,
        json: Some(json),
        handle: None,
    }
}

fn render(view: &FleetView) -> String {
    let mut text = format!("Fleet settings version {}\n", view.settings.version);
    let machines: BTreeSet<_> = view
        .settings
        .machines
        .keys()
        .chain(view.machines.iter().map(|machine| &machine.name))
        .collect();
    for name in machines {
        let setting = view.settings.machine(name);
        let trees = &setting.trees;
        let root = trees.trees_root.as_deref().unwrap_or("repository default");
        let space = trees
            .tree_space_gb
            .map_or_else(|| "unset".to_owned(), |space| space.to_string());
        let _ = write!(
            text,
            "  machine {name}: {}, trees root {root}, tree space (GB) {space}, tree ceiling {} GB",
            setting.state, trees.tree_ceiling_gb,
        );
        for machine in view.machines.iter().filter(|machine| &machine.name == name) {
            let _ = write!(text, " | {}, last seen {}", machine.os, machine.last_seen);
            if let Some(capacity) = &machine.capacity {
                let _ = write!(
                    text,
                    ", {:.1} free cores, room for {} workers",
                    capacity.free_cores, capacity.worker_room
                );
                if let Some(build_space) = capacity.build_space {
                    let _ = write!(text, ", {build_space}");
                }
            }
        }
        text.push('\n');
    }
    let providers: BTreeSet<_> = view
        .settings
        .providers
        .keys()
        .chain(view.usage.iter().map(|sample| &sample.provider))
        .chain(view.holds.keys())
        .collect();
    for name in providers {
        let setting = view.settings.provider(name);
        let _ = writeln!(
            text,
            "  provider {name}: {}, share {}, {}",
            setting.state(),
            setting.share,
            setting.usage_policy(),
        );
        for sample in view.usage.iter().filter(|sample| &sample.provider == name) {
            let label = sample.label.as_deref().unwrap_or("Usage");
            let _ = write!(text, "    {label}: {}", sample.status);
            if let Some(percent) = sample.percent_used {
                let _ = write!(text, ", {percent}% used");
            }
            if let Some(reset) = &sample.resets_at {
                let _ = write!(text, ", resets {reset}");
            }
            let _ = writeln!(text, " ({}, {})", sample.machine, sample.at);
        }
        for hold in view.holds.get(name).into_iter().flatten() {
            let _ = writeln!(text, "    Held: {}", hold.describe());
        }
    }
    if !view.changes.is_empty() {
        text.push_str("Recent changes\n");
        for change in &view.changes {
            let _ = writeln!(text, "  {} {}: {}", change.at, change.actor, change.summary);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conflict_reapplies_only_the_requested_fields_to_the_current_version() {
        let command = FleetCommand::Provider {
            name: "agent".to_owned(),
            on: false,
            off: false,
            share: Some(50),
            stop_percent: None,
            five_hour_stop_percent: None,
            ignore_usage_threshold: None,
        };
        let mut current = FleetSettings {
            version: 2,
            ..FleetSettings::default()
        };
        current
            .providers
            .entry("agent".to_owned())
            .or_default()
            .enabled = false;

        let mut calls = Vec::new();
        let result = edit(&command, FleetSettings::default(), |update| {
            calls.push(update.clone());
            if calls.len() == 1 {
                return Err(Failure {
                    exit: Exit::Conflict,
                    message: "first conflict".to_owned(),
                    body: Some(json!(current)),
                });
            }
            Ok(update.settings.clone())
        })
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].version, 0);
        assert!(calls[0].settings.provider("agent").enabled);
        assert_eq!(calls[1].version, 2);
        assert!(!calls[1].settings.provider("agent").enabled);
        assert_eq!(calls[1].settings.provider("agent").share, 50);
        assert!(result.text.contains("off, share 50"));
    }
}
