//! `GET /machines/versions`: the Ignite commit the hub and each machine runs, which
//! `ignite status` and `ignite wait deploy-<commit>` compare with the release branch.

use serde::{Deserialize, Serialize};

use crate::ids::MachineId;
use crate::time::Timestamp;

/// The hub's answer to `GET /machines/versions`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetVersions {
    /// The Ignite commit the hub runs, as its heartbeat replies report it. Empty when the hub
    /// never recorded one, as a hub run from an unstored executable does.
    pub hub_version: String,
    /// Every registered machine, by name.
    pub machines: Vec<MachineVersion>,
}

/// One machine, as its last heartbeat reported it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineVersion {
    pub machine: MachineId,
    /// The hostname.
    pub name: String,
    /// The Ignite commit the machine's daemon runs.
    pub version: String,
    /// When the hub last heard from the machine.
    pub last_seen: Timestamp,
}
