//! Who works a ticket: its owner, a machine and the desktop app whose workers on that machine may
//! take it, such as `jarvis/claude` or `cygnus/codex`.
//!
//! Owners are a ticket field of their own, not labels:
//!
//! - A ticket with no owner takes its nearest ancestor's, and one whose ancestors have none
//!   either follows the **default owner**, a hub setting that a person changes on the dashboard.
//!   Changing the default moves every ticket that follows it.
//! - Dispatch offers a ticket only to workers of its owner. An idle worker elsewhere is never
//!   offered it.
//! - An owner may name a machine without an app (`cygnus`), meaning the machine's home app
//!   (`home_app` in its `[machine.<name>]` table), or any app on it when none is declared. A
//!   ticket the hub files itself is owned this way by the machine that saw the fault.
//! - A ticket the hub files with `fallback` set holds `master` back. When its owner's machine
//!   has not been heard from for [`FALLBACK_SECONDS`], it may also go to the default owner, if
//!   that runs the operating system the ticket needs, or else to any machine that does.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::{Config, Os};

/// How long the machine owning a ticket that holds `master` back may stay silent before the
/// ticket may go elsewhere.
pub const FALLBACK_SECONDS: f64 = 30.0 * 60.0;

/// A ticket's owner: a machine's name and, optionally, an app on it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Owner {
    /// The machine's name as `ignite.toml` declares it, compared without regard to case.
    pub machine: String,
    /// `claude` or `codex`; `None` means the machine's home app.
    pub app: Option<String>,
}

impl Owner {
    #[must_use]
    pub fn new(machine: &str, app: Option<&str>) -> Owner {
        Owner {
            machine: machine.to_ascii_lowercase(),
            app: app.map(str::to_ascii_lowercase),
        }
    }

    /// Whether a worker of `app` on the machine named `machine` may take a ticket of this owner.
    /// `home` is the machine's home app from `ignite.toml`, if it declares one.
    pub fn admits(&self, machine: &str, app: &str, home: Option<&str>) -> bool {
        self.machine.eq_ignore_ascii_case(machine)
            && match self.app.as_deref().or(home) {
                Some(owner_app) => owner_app.eq_ignore_ascii_case(app),
                None => true,
            }
    }

    /// This owner with its app filled in from the machine's home app where it names none.
    #[must_use]
    pub fn resolved(&self, config: &Routing) -> Owner {
        Owner {
            machine: self.machine.clone(),
            app: self
                .app
                .clone()
                .or_else(|| config.home_app(&self.machine).map(str::to_owned)),
        }
    }
}

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.app {
            Some(app) => write!(f, "{}/{app}", self.machine),
            None => f.write_str(&self.machine),
        }
    }
}

impl FromStr for Owner {
    type Err = String;

    /// `<machine>` or `<machine>/<app>`.
    fn from_str(text: &str) -> Result<Owner, String> {
        let text = text.trim();
        let (machine, app) = match text.split_once('/') {
            Some((machine, app)) => (machine, Some(app)),
            None => (text, None),
        };
        let word = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        if !word(machine) || app.is_some_and(|app| !word(app)) {
            return Err(format!(
                "{text:?} is not an owner; write <machine> or <machine>/<app>, such as jarvis/claude"
            ));
        }
        Ok(Owner::new(machine, app))
    }
}

impl Serialize for Owner {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Owner {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Owner, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// What the hub knows about routing from `ignite.toml`: each machine's operating system and home
/// app, the apps the pool runs and the default owner a new hub starts with. The hub records it in
/// its database at start, so routes and upkeep read it without the configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routing {
    pub machines: Vec<RoutedMachine>,
    /// The apps the pool runs: those with a `[pool.models.<app>]` table.
    pub apps: Vec<String>,
    /// `[pool] default_owner`, the default owner until a person picks another.
    pub default_owner: Option<Owner>,
}

/// One declared machine, as routing sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedMachine {
    pub name: String,
    pub os: Option<Os>,
    pub home_app: Option<String>,
}

impl Routing {
    #[must_use]
    pub fn from_config(config: &Config) -> Routing {
        Routing {
            machines: config
                .machine
                .iter()
                .map(|(name, machine)| RoutedMachine {
                    name: name.to_ascii_lowercase(),
                    os: machine.os,
                    home_app: machine.home_app.clone(),
                })
                .collect(),
            apps: config.pool.models.keys().cloned().collect(),
            default_owner: config.pool.default_owner.clone(),
        }
    }

    pub fn machine(&self, name: &str) -> Option<&RoutedMachine> {
        self.machines
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }

    pub fn home_app(&self, machine: &str) -> Option<&str> {
        self.machine(machine).and_then(|m| m.home_app.as_deref())
    }

    /// Every owner a person may pick: each declared machine with each app, in order.
    pub fn choices(&self) -> Vec<Owner> {
        self.machines
            .iter()
            .flat_map(|m| self.apps.iter().map(|app| Owner::new(&m.name, Some(app))))
            .collect()
    }

    /// The owners a ticket may go to.
    ///
    /// - `own` is the ticket's owner or, failing that, its nearest ancestor's; `default` is the
    ///   default owner, which applies when both are missing.
    /// - With `fallback`, when `silent` says the owner's machine has not been heard from for
    ///   [`FALLBACK_SECONDS`], the default owner is added if its machine runs `needs_os` (or the
    ///   ticket needs none), and otherwise every machine that runs it.
    ///
    /// Empty only when there is no owner and no default, which leaves the ticket to any worker.
    pub fn destinations(
        &self,
        own: Option<&Owner>,
        default: Option<&Owner>,
        fallback: bool,
        needs_os: Option<Os>,
        silent: impl Fn(&str) -> bool,
    ) -> Vec<Owner> {
        let Some(owner) = own.or(default) else {
            return Vec::new();
        };
        let mut owners = vec![owner.clone()];
        if !(fallback && silent(&owner.machine)) {
            return owners;
        }
        let runs = |machine: &str| {
            needs_os.is_none_or(|os| self.machine(machine).and_then(|m| m.os) == Some(os))
        };
        match default {
            Some(default) if runs(&default.machine) && !silent(&default.machine) => {
                owners.push(default.clone());
            }
            _ => owners.extend(
                self.machines
                    .iter()
                    .filter(|m| runs(&m.name) && !silent(&m.name))
                    .map(|m| Owner::new(&m.name, None)),
            ),
        }
        owners.dedup();
        owners
    }
}

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
