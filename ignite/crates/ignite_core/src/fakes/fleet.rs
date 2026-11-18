//! The fake hub's answer to `GET /machines/versions`.

use std::sync::Mutex;

use super::{FakeHub, lock};
use crate::protocol::fleet::FleetVersions;
use crate::traits::Result;

/// The versions [`FakeHub::set_fleet_versions`] set; none until then.
#[derive(Debug, Default)]
pub(super) struct FakeFleet {
    versions: Mutex<Option<FleetVersions>>,
}

impl FakeHub {
    /// Sets what `fleet_versions` answers.
    pub fn set_fleet_versions(&self, versions: FleetVersions) {
        *lock(&self.fleet.versions) = Some(versions);
    }

    pub(super) fn fake_fleet_versions(&self) -> Result<FleetVersions> {
        self.reachable()?;
        Ok(lock(&self.fleet.versions).clone().unwrap_or(FleetVersions {
            hub_version: String::new(),
            machines: Vec::new(),
        }))
    }
}
