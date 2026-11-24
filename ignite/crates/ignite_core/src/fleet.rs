//! Fleet policy stored by the hub and the messages that carry it to operators and daemons.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};

use serde::{Deserialize, Serialize};

use crate::capacity::CapacityReport;
use crate::config::Os;
use crate::ids::MachineId;
use crate::plan_usage::{DEFAULT_HOLD_PERCENT, Hold};
use crate::shares::Source;
use crate::time::Timestamp;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetSettings {
    pub version: u64,
    pub machines: BTreeMap<String, MachineSetting>,
    pub providers: BTreeMap<String, ProviderSetting>,
    /// Omitted by older fleet clients. Their writes leave these settings in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocations: Option<Allocations>,
}

/// Local worker-share weights. A source without its own weight shares the general pool.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allocations {
    pub general: Option<u32>,
    #[serde(default)]
    pub sources: BTreeMap<Source, u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MachineSetting {
    pub state: MachineState,
    #[serde(flatten)]
    pub trees: TreeSettings,
}

pub const DEFAULT_TREE_CEILING_GB: u32 = 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TreeSettings {
    pub trees_root: Option<String>,
    pub tree_space_gb: Option<TreeSpace>,
    pub tree_ceiling_gb: u32,
}

impl Default for TreeSettings {
    fn default() -> Self {
        Self {
            trees_root: None,
            tree_space_gb: None,
            tree_ceiling_gb: DEFAULT_TREE_CEILING_GB,
        }
    }
}

impl TreeSettings {
    pub fn root(&self, fallback: &std::path::Path) -> std::path::PathBuf {
        self.trees_root
            .as_deref()
            .map_or_else(|| fallback.to_owned(), Into::into)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TreeSpace {
    Gigabytes(u32),
    Automatic(Auto),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Auto {
    Auto,
}

impl fmt::Display for TreeSpace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gigabytes(gb) => write!(formatter, "{gb}"),
            Self::Automatic(Auto::Auto) => formatter.write_str("auto"),
        }
    }
}

impl std::str::FromStr for TreeSpace {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw == "auto" {
            return Ok(Self::Automatic(Auto::Auto));
        }
        raw.parse::<u32>()
            .ok()
            .filter(|gb| *gb > 0)
            .map(Self::Gigabytes)
            .ok_or_else(|| "expected a positive number of gigabytes or auto".to_owned())
    }
}

/// A volume observed by a daemon. An empty volume can hold trees directly or in one
/// top-level directory. Once populated, only the reported root is eligible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeVolume {
    pub mount: String,
    pub root: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeVolumes {
    pub default_root: String,
    pub dedicated: Vec<TreeVolume>,
}

impl TreeVolumes {
    pub fn allows_auto(&self, root: Option<&str>, os: Os) -> bool {
        let normalize = |path: &str| match os {
            Os::Macos => path.trim_end_matches('/').to_owned(),
            Os::Windows => path.replace('\\', "/").trim_end_matches('/').to_lowercase(),
        };
        let root = normalize(root.unwrap_or(&self.default_root));
        self.dedicated.iter().any(|volume| {
            if let Some(known) = &volume.root {
                return root == normalize(known);
            }
            let mount = normalize(&volume.mount);
            root == mount
                || root
                    .strip_prefix(&format!("{mount}/"))
                    .is_some_and(|relative| !relative.is_empty() && !relative.contains('/'))
        })
    }
}

/// Fleet edits can target a machine on a different operating system.
pub fn absolute_trees_root(path: &str, os: Option<Os>) -> bool {
    let drive_root = |path: &str| {
        let bytes = path.as_bytes();
        bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\')
    };
    let mac = path.starts_with('/');
    let windows = drive_root(path) || path.strip_prefix(r"\\?\").is_some_and(drive_root);
    !path.chars().any(char::is_control)
        && match os {
            Some(Os::Macos) => mac,
            Some(Os::Windows) => windows,
            None => mac || windows,
        }
        && !path
            .split(['/', '\\'])
            .any(|part| part == "." || part == "..")
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
    /// The provider's weight in the mix of new work. Zero turns the provider off, like
    /// `enabled: false`, so a provider without a setting takes no work.
    pub share: u32,
    pub stop_percent: u32,
    /// Keep offering work when a usage sample reaches the stop percentage.
    #[serde(default)]
    pub ignore_usage_threshold: bool,
}

impl Default for ProviderSetting {
    fn default() -> Self {
        Self {
            enabled: true,
            share: 0,
            stop_percent: DEFAULT_HOLD_PERCENT,
            ignore_usage_threshold: false,
        }
    }
}

impl ProviderSetting {
    /// Whether new work may go to this provider: it is on and has a positive share.
    pub fn takes_work(self) -> bool {
        self.enabled && self.share > 0
    }

    /// `on`, or `off` with the reason when the switch is on but the share is zero.
    pub fn state(self) -> &'static str {
        match (self.enabled, self.share) {
            (false, _) => "off",
            (true, 0) => "off (share 0)",
            (true, _) => "on",
        }
    }

    pub fn usage_policy(self) -> String {
        if self.ignore_usage_threshold {
            "continue past usage threshold".to_owned()
        } else {
            format!("stop at {}%", self.stop_percent)
        }
    }
}

impl FleetSettings {
    pub fn machine(&self, name: &str) -> MachineSetting {
        self.machines.get(name).cloned().unwrap_or_default()
    }

    pub fn provider(&self, name: &str) -> ProviderSetting {
        self.providers.get(name).copied().unwrap_or_default()
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, machine) in &self.machines {
            let trees = &machine.trees;
            if trees
                .trees_root
                .as_deref()
                .is_some_and(|root| !absolute_trees_root(root, None))
            {
                return Err(format!(
                    "machine {name}: trees root must be an absolute path"
                ));
            }
            if trees.tree_ceiling_gb == 0 || trees.tree_space_gb == Some(TreeSpace::Gigabytes(0)) {
                return Err(format!(
                    "machine {name}: tree space and ceiling must be positive"
                ));
            }
        }
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
                let mut change = format!("machine {name}: {}", setting.state);
                let previous = before.machine(name).trees;
                let trees = &setting.trees;
                if previous.trees_root != trees.trees_root {
                    let _ = write!(
                        change,
                        ", trees root {}",
                        trees.trees_root.as_deref().unwrap_or("repository default")
                    );
                }
                if previous.tree_space_gb != trees.tree_space_gb {
                    let _ = write!(
                        change,
                        ", tree space {}",
                        trees
                            .tree_space_gb
                            .map_or_else(|| "unset".to_owned(), |space| space.to_string())
                    );
                }
                if previous.tree_ceiling_gb != trees.tree_ceiling_gb {
                    let _ = write!(change, ", tree ceiling {} GB", trees.tree_ceiling_gb);
                }
                changes.push(change);
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
                changes.push(format!(
                    "provider {name}: {}, share {}, {}",
                    setting.state(),
                    setting.share,
                    setting.usage_policy()
                ));
            }
            None => changes.push(format!("provider {name}: defaults restored")),
            Some(_) => {}
        }
    }
    let default = Allocations::default();
    let old = before.allocations.as_ref().unwrap_or(&default);
    let new = after.allocations.as_ref().unwrap_or(&default);
    let describe =
        |weight: Option<u32>| weight.map_or_else(|| "inherited".to_owned(), |n| n.to_string());
    if old.general != new.general {
        changes.push(format!("General pool allocation {}", describe(new.general)));
    }
    for source in Source::ALL {
        if old.sources.get(&source) != new.sources.get(&source) {
            changes.push(format!(
                "{} allocation {}",
                source.label(),
                describe(new.sources.get(&source).copied())
            ));
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
    #[serde(default, flatten)]
    pub trees: TreeSettings,
    pub off_platforms: BTreeSet<Os>,
    /// Platforms with a live checks-only machine and no live active machine.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub checks_only_platforms: BTreeSet<Os>,
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
                        ..ProviderSetting::default()
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
