//! Operator commands for the fleet settings. They talk to the hub directly.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};

use ignite_core::fleet::{
    FleetSettings, FleetUpdate, FleetView, MachineState, SESSION_STATE_HOURS, TreeSpace, summary,
};
use ignite_core::machine_speed::Speed;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::time::Timestamp;

use crate::client::session_from_env;
use crate::ticket::http::{Failure, Hub, Result, usage};

#[derive(Debug, Subcommand)]
pub enum FleetCommand {
    /// Change a machine's state and build tree settings.
    Machine {
        name: String,
        #[arg(long, value_enum)]
        state: Option<State>,
        /// Return the machine to active after this many hours. A session that sets
        /// checks-only or off gets 4 hours unless it passes --indefinitely.
        #[arg(
            long,
            value_name = "HOURS",
            requires = "state",
            conflicts_with = "indefinitely"
        )]
        for_hours: Option<f64>,
        /// Keep a checks-only or off state until someone changes it.
        #[arg(long, requires = "state")]
        indefinitely: bool,
        /// Absolute tree directory, or "default" to use the repository setting.
        #[arg(long)]
        trees_root: Option<String>,
        /// Space in decimal gigabytes, or "auto" on a dedicated volume.
        #[arg(long, value_name = "GB|auto")]
        tree_space_gb: Option<TreeSpace>,
        /// Maximum size of one tree's build directories in decimal gigabytes.
        #[arg(long)]
        tree_ceiling_gb: Option<u32>,
        /// The machine's speed relative to the fastest (1.0), replacing the hub's measurement,
        /// or "measured" to use the measurement again. Work that holds master back goes to the
        /// fastest machine that can take it.
        #[arg(long, value_name = "N|measured", value_parser = parse_speed)]
        speed: Option<SpeedSetting>,
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

/// A `--speed` value: the operator's speed, or `None` to use the measurement.
#[derive(Clone, Copy, Debug)]
pub struct SpeedSetting(Option<Speed>);

fn parse_speed(text: &str) -> std::result::Result<SpeedSetting, String> {
    if text == "measured" {
        return Ok(SpeedSetting(None));
    }
    text.parse().map(|speed| SpeedSetting(Some(speed)))
}

/// When the edit is made and whether a session (rather than the operator) makes it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Editor {
    pub now: Timestamp,
    pub session: bool,
}

impl FleetCommand {
    fn apply(&self, settings: &mut FleetSettings, editor: Editor) {
        match self {
            Self::Machine {
                name,
                state,
                for_hours,
                indefinitely,
                trees_root,
                tree_space_gb,
                tree_ceiling_gb,
                speed,
            } => {
                let mut machine = settings.machine(name);
                if let Some(state) = state {
                    machine.state = (*state).into();
                    let hours = for_hours.or_else(|| {
                        (editor.session && !*indefinitely).then_some(SESSION_STATE_HOURS)
                    });
                    machine.state_until = hours
                        .filter(|_| machine.state != MachineState::Active)
                        .map(|hours| editor.now.plus_seconds(hours * 3600.0));
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
                if let Some(SpeedSetting(speed)) = speed {
                    machine.speed = *speed;
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
    let session = session_from_env(env);
    let editor = Editor {
        now: ignite_core::traits::Clock::now(&ignite_core::time::SystemClock),
        session: session.is_some(),
    };
    let actor = session.map_or_else(|| "ignite".to_owned(), |id| id.to_string());
    Hub::load(cwd, actor, env)
        .and_then(|hub| execute(&hub, command, editor))
        .unwrap_or_else(Failure::into_reply)
}

pub(crate) fn execute(hub: &Hub, command: Option<&FleetCommand>, editor: Editor) -> Result<Reply> {
    let view: FleetView = serde_json::from_str(&hub.get(&["fleet"], &[])?.body).map_err(usage)?;
    let Some(command) = command else {
        return Ok(reply(render(&view), json!(view)));
    };
    edit(command, view.settings, editor, |update| {
        serde_json::from_str(&hub.put_versioned(&["fleet"], &json!(update))?.body).map_err(usage)
    })
}

fn edit(
    command: &FleetCommand,
    mut before: FleetSettings,
    editor: Editor,
    mut put: impl FnMut(&FleetUpdate) -> Result<FleetSettings>,
) -> Result<Reply> {
    let mut attempt = |before: &FleetSettings| {
        let mut settings = before.clone();
        command.apply(&mut settings, editor);
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
        let _ = write!(text, "  machine {name}: {}", setting.state);
        if let Some(until) = setting.state_until {
            let _ = write!(text, " until {until}");
        }
        let _ = write!(
            text,
            ", trees root {root}, tree space (GB) {space}, tree ceiling {} GB",
            trees.tree_ceiling_gb,
        );
        for machine in view.machines.iter().filter(|machine| &machine.name == name) {
            let _ = write!(text, " | {}, last seen {}", machine.os, machine.last_seen);
            if let Some(speed) = machine.speed {
                let source = if setting.speed.is_some() {
                    "set"
                } else {
                    "measured"
                };
                let _ = write!(text, ", speed {speed} ({source})");
            }
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

    fn operator() -> Editor {
        Editor {
            now: Timestamp::EPOCH,
            session: false,
        }
    }

    fn machine(state: State, for_hours: Option<f64>, indefinitely: bool) -> FleetCommand {
        FleetCommand::Machine {
            name: "cygnus".to_owned(),
            state: Some(state),
            for_hours,
            indefinitely,
            trees_root: None,
            tree_space_gb: None,
            tree_ceiling_gb: None,
            speed: None,
        }
    }

    /// A session's checks-only or off state ends after four hours unless it says otherwise;
    /// the operator's lasts until changed.
    #[test]
    fn a_session_state_gets_an_end_unless_kept_indefinitely() {
        let now = Timestamp::parse("2026-11-26T12:42:00Z").unwrap();
        let until = |command: &FleetCommand, session: bool| {
            let mut settings = FleetSettings::default();
            command.apply(&mut settings, Editor { now, session });
            settings.machine("cygnus").state_until
        };
        let four = Some(now.plus_seconds(4.0 * 3600.0));
        assert_eq!(until(&machine(State::ChecksOnly, None, false), true), four);
        assert_eq!(until(&machine(State::Off, None, true), true), None);
        assert_eq!(until(&machine(State::ChecksOnly, None, false), false), None);
        assert_eq!(
            until(&machine(State::Off, Some(1.0), false), false),
            Some(now.plus_seconds(3600.0))
        );
        assert_eq!(until(&machine(State::Active, Some(1.0), false), true), None);
    }

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
        let result = edit(&command, FleetSettings::default(), operator(), |update| {
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
