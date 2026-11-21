//! Fleet policy stored by the hub and the messages that carry it to operators and daemons.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::capacity::CapacityReport;
use crate::config::Os;
use crate::ids::MachineId;
use crate::plan_usage::{DEFAULT_HOLD_PERCENT, Hold};
use crate::time::Timestamp;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetSettings {
    pub version: u64,
    pub machines: BTreeMap<String, MachineSetting>,
    pub providers: BTreeMap<String, ProviderSetting>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineSetting {
    pub state: MachineState,
    pub disk_cap_gb: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MachineState {
    #[default]
    Active,
    ChecksOnly,
    Off,
}

impl fmt::Display for MachineState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Active => "active",
            Self::ChecksOnly => "checks-only",
            Self::Off => "off",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSetting {
    pub enabled: bool,
    pub share: u32,
    pub stop_percent: u32,
}

impl Default for ProviderSetting {
    fn default() -> Self {
        Self {
            enabled: true,
            share: 0,
            stop_percent: DEFAULT_HOLD_PERCENT,
        }
    }
}

impl FleetSettings {
    pub fn machine(&self, name: &str) -> MachineSetting {
        self.machines.get(name).copied().unwrap_or_default()
    }

    pub fn provider(&self, name: &str) -> ProviderSetting {
        self.providers.get(name).copied().unwrap_or_default()
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, provider) in &self.providers {
            if provider.share > 1_000 {
                return Err(format!("provider {name}: share must be at most 1,000"));
            }
            if !(1..=100).contains(&provider.stop_percent) {
                return Err(format!(
                    "provider {name}: stop percentage must be from 1 to 100"
                ));
            }
        }
        Ok(())
    }
}

/// One line describing the entries changed in a fleet settings write.
pub fn summary(before: &FleetSettings, after: &FleetSettings) -> String {
    let mut changes = Vec::new();
    for name in before
        .machines
        .keys()
        .chain(after.machines.keys())
        .collect::<BTreeSet<_>>()
    {
        match after.machines.get(name) {
            Some(setting) if before.machines.get(name) != Some(setting) => {
                let cap = setting
                    .disk_cap_gb
                    .map_or_else(|| "auto".to_owned(), |gb| format!("{gb} GB"));
                changes.push(format!("machine {name}: {}, disk cap {cap}", setting.state));
            }
            None => changes.push(format!("machine {name}: defaults restored")),
            Some(_) => {}
        }
    }
    for name in before
        .providers
        .keys()
        .chain(after.providers.keys())
        .collect::<BTreeSet<_>>()
    {
        match after.providers.get(name) {
            Some(setting) if before.providers.get(name) != Some(setting) => {
                let state = if setting.enabled { "on" } else { "off" };
                changes.push(format!(
                    "provider {name}: {state}, share {}, stop at {}%",
                    setting.share, setting.stop_percent
                ));
            }
            None => changes.push(format!("provider {name}: defaults restored")),
            Some(_) => {}
        }
    }
    if changes.is_empty() {
        "No fleet settings changed".to_owned()
    } else {
        changes.join(" | ")
    }
}

/// The fleet policy a daemon needs after a heartbeat or a restart.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FleetNotice {
    pub version: u64,
    pub state: MachineState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_actor: Option<String>,
    pub disk_cap_gb: Option<u32>,
    pub off_platforms: BTreeSet<Os>,
    pub holds: BTreeMap<String, Vec<Hold>>,
}

/// A proposed write and the version the operator edited.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetUpdate {
    pub version: u64,
    pub settings: FleetSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FleetView {
    pub settings: FleetSettings,
    pub changes: Vec<FleetChange>,
    pub machines: Vec<FleetMachine>,
    pub usage: Vec<ProviderSample>,
    pub holds: BTreeMap<String, Vec<Hold>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetChange {
    pub seq: i64,
    pub at: Timestamp,
    pub actor: String,
    pub summary: String,
    pub settings: FleetSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FleetMachine {
    pub id: MachineId,
    pub name: String,
    pub os: Os,
    pub last_seen: Timestamp,
    pub capacity: Option<CapacityReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<crate::protocol::machine_state::MachineSnapshot>,
}

/// One usage window, or a sample with no windows if the CLI had no usage figures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderSample {
    pub provider: String,
    pub at: Timestamp,
    pub machine: MachineId,
    pub status: String,
    pub label: Option<String>,
    pub percent_used: Option<f64>,
    pub resets_at: Option<String>,
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_entries_use_fleet_defaults() {
        let settings = FleetSettings::default();
        assert_eq!(settings.version, 0);
        assert_eq!(settings.machine("host").state, MachineState::Active);
        assert_eq!(settings.machine("host").disk_cap_gb, None);
        assert!(settings.provider("agent").enabled);
        assert_eq!(settings.provider("agent").share, 0);
        assert_eq!(settings.provider("agent").stop_percent, 99);
    }

    #[test]
    fn provider_limits_include_their_boundaries() {
        for (share, stop_percent, valid) in [
            (0, 1, true),
            (1_000, 100, true),
            (1_001, 99, false),
            (0, 0, false),
            (0, 101, false),
        ] {
            let settings = FleetSettings {
                providers: BTreeMap::from([(
                    "agent".to_owned(),
                    ProviderSetting {
                        enabled: false,
                        share,
                        stop_percent,
                    },
                )]),
                ..FleetSettings::default()
            };
            assert_eq!(settings.validate().is_ok(), valid, "{settings:?}");
        }
    }

    #[test]
    fn change_summary_names_edits_and_removed_entries() {
        let before = FleetSettings {
            machines: BTreeMap::from([("host".to_owned(), MachineSetting::default())]),
            providers: BTreeMap::from([("agent".to_owned(), ProviderSetting::default())]),
            ..FleetSettings::default()
        };
        let mut after = before.clone();
        after.version += 1;
        after.machines.clear();
        after.providers.get_mut("agent").unwrap().share = 50;
        assert_eq!(
            summary(&before, &after),
            "machine host: defaults restored | provider agent: on, share 50, stop at 99%"
        );
    }
}
