//! Who works a ticket: its owner, a machine and the desktop app whose workers on that machine may
//! take it, such as `jarvis/claude` or `cygnus/codex`.
//!
//! Owners are a ticket field of their own, not labels:
//!
//! - A ticket filed with no owner and no ancestor that has one is owned by whoever filed it: the
//!   machine and app of the session that ran `ignite ticket create` (see
//!   `ignite_store::hub::owners::add_creator_owners`). A ticket under an owned ancestor takes
//!   the nearest ancestor's owner, so changing an epic's owner moves its tickets.
//! - Dispatch offers a ticket only to workers of its owner. An idle worker elsewhere is never
//!   offered it. A ticket with no owner at all, such as one filed before owners existed, goes to
//!   any worker.
//! - An owner may name a machine without an app (`cygnus`), meaning any app's workers on that
//!   machine. A ticket the hub files itself is owned this way by the machine that saw the fault,
//!   and so is one filed from a session whose app the client could not tell.
//! - A ticket the hub files with `fallback` set holds `master` back. When its owner's machine
//!   has not been heard from for [`FALLBACK_SECONDS`], it may also go to any machine that runs
//!   the operating system the ticket needs.

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
    /// `claude` or `codex`; `None` means any app on the machine.
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
    pub fn admits(&self, machine: &str, app: &str) -> bool {
        self.machine.eq_ignore_ascii_case(machine)
            && self
                .app
                .as_deref()
                .is_none_or(|owner_app| owner_app.eq_ignore_ascii_case(app))
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

/// What the hub knows about routing from `ignite.toml`: each machine's operating system and the
/// apps the pool runs. The hub records it in its database at start, so routes and upkeep read it
/// without the configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routing {
    pub machines: Vec<RoutedMachine>,
    /// The apps the pool runs: those with a `[pool.models.<app>]` table.
    pub apps: Vec<String>,
}

/// One declared machine, as routing sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedMachine {
    pub name: String,
    pub os: Option<Os>,
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
                })
                .collect(),
            apps: config.pool.models.keys().cloned().collect(),
        }
    }

    pub fn machine(&self, name: &str) -> Option<&RoutedMachine> {
        self.machines
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
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
    /// - `own` is the ticket's owner or, failing that, its nearest ancestor's.
    /// - With `fallback`, when `silent` says the owner's machine has not been heard from for
    ///   [`FALLBACK_SECONDS`], every other machine that runs `needs_os` (or any machine, when the
    ///   ticket needs no system) is added, with any app.
    ///
    /// Empty only when there is no owner, which leaves the ticket to any worker.
    pub fn destinations(
        &self,
        own: Option<&Owner>,
        fallback: bool,
        needs_os: Option<Os>,
        silent: impl Fn(&str) -> bool,
    ) -> Vec<Owner> {
        let Some(owner) = own else {
            return Vec::new();
        };
        let mut owners = vec![owner.clone()];
        if !(fallback && silent(&owner.machine)) {
            return owners;
        }
        let runs = |machine: &str| {
            needs_os.is_none_or(|os| self.machine(machine).and_then(|m| m.os) == Some(os))
        };
        owners.extend(
            self.machines
                .iter()
                .filter(|m| runs(&m.name) && !silent(&m.name))
                .map(|m| Owner::new(&m.name, None)),
        );
        owners.dedup();
        owners
    }
}

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
