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
    /// The provider's weight in the mix of new work. Zero keeps the provider out of the mix, so
    /// a provider without a setting takes no unpinned work; with the switch on it still takes
    /// tickets pinned to it with a `provider:` label (see [`Self::takes_ticket`]).
    pub share: u32,
    /// Hold new work when a weekly window reaches this percentage.
    pub stop_percent: u32,
    /// Hold new work when the 5-hour window reaches this percentage. `None` uses
    /// `stop_percent`, which is what settings saved before the two windows had separate
    /// percentages mean. Hitting the 5-hour limit costs a few hours at most while hitting a
    /// weekly one can cost days, so this one is usually higher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour_stop_percent: Option<u32>,
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
            five_hour_stop_percent: None,
            ignore_usage_threshold: false,
        }
    }
}

impl ProviderSetting {
    /// Whether unpinned new work may go to this provider: it is on and has a positive share.
    pub fn takes_work(self) -> bool {
        self.enabled && self.share > 0
    }

    /// Whether a ticket may go to this provider. `pinned` says the ticket's `provider:` label
    /// names this provider. A provider that is on with a share of zero takes only such tickets,
    /// so an operator can route chosen tickets to it (a model trial's arm, say) without giving it
    /// a place in the mix. The off switch still refuses every ticket.
    pub fn takes_ticket(self, pinned: bool) -> bool {
        self.enabled && (self.share > 0 || pinned)
    }

    /// `on`, `off`, or `pinned only (share 0)` when the switch is on but the share is zero.
    pub fn state(self) -> &'static str {
        match (self.enabled, self.share) {
            (false, _) => "off",
            (true, 0) => "pinned only (share 0)",
            (true, _) => "on",
        }
    }

    /// The 5-hour window's stop percentage.
    pub fn five_hour_stop(self) -> u32 {
        self.five_hour_stop_percent.unwrap_or(self.stop_percent)
    }

    /// The stop percentage of the window labelled `label`.
    pub fn stop_percent_for(self, label: &str) -> u32 {
        if crate::plan_usage::is_five_hour(label) {
            self.five_hour_stop()
        } else {
            self.stop_percent
        }
    }

    pub fn usage_policy(self) -> String {
        if self.ignore_usage_threshold {
            "continue past usage threshold".to_owned()
        } else if self.five_hour_stop() == self.stop_percent {
            format!("stop at {}%", self.stop_percent)
        } else {
            format!(
                "stop at {}% of 5-hour, {}% of weekly",
                self.five_hour_stop(),
                self.stop_percent
            )
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
            if !(1..=100).contains(&provider.stop_percent)
                || !(1..=100).contains(&provider.five_hour_stop())
            {
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
        for (share, stop_percent, five_hour_stop_percent, valid) in [
            (0, 1, None, true),
            (1_000, 100, Some(1), true),
            (0, 99, Some(100), true),
            (1_001, 99, None, false),
            (0, 0, None, false),
            (0, 101, None, false),
            (0, 99, Some(0), false),
            (0, 99, Some(101), false),
        ] {
            let settings = FleetSettings {
                providers: BTreeMap::from([(
                    "agent".to_owned(),
                    ProviderSetting {
                        enabled: false,
                        share,
                        stop_percent,
                        five_hour_stop_percent,
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
        after.providers.get_mut("agent").unwrap().five_hour_stop_percent = Some(85);
        assert!(
            summary(&before, &after)
                .ends_with("provider agent: on, share 50, stop at 85% of 5-hour, 99% of weekly")
        );
    }
}
