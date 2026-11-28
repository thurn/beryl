//! The fake hub's answer to `GET /machines/versions`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{FakeHub, lock};
use crate::fleet::{FleetNotice, MachineState};
use crate::ids::MachineId;
use crate::protocol::fleet::FleetVersions;
use crate::protocol::hub::shed::{ShedDone, ShedOrder};
use crate::traits::Result;

/// The versions [`FakeHub::set_fleet_versions`] set; none until then.
#[derive(Debug, Default)]
pub(super) struct FakeFleet {
    versions: Mutex<Option<FleetVersions>>,
    notices: Mutex<BTreeMap<MachineId, FleetNotice>>,
    sheds: Mutex<Vec<ShedOrder>>,
    shed_dones: Mutex<Vec<ShedDone>>,
}

impl FakeHub {
    /// Every `ignite pool shed --machine` order sent so far, in order.
    pub fn sheds(&self) -> Vec<ShedOrder> {
        lock(&self.fleet.sheds).clone()
    }

    /// Every shed a machine recorded in the fleet's recent changes, in order.
    pub fn shed_dones(&self) -> Vec<ShedDone> {
        lock(&self.fleet.shed_dones).clone()
    }

    pub(super) fn fake_shed(&self, order: &ShedOrder) -> Result<()> {
        self.reachable()?;
        lock(&self.fleet.sheds).push(order.clone());
        Ok(())
    }

    pub(super) fn fake_shed_done(&self, done: &ShedDone) -> Result<()> {
        self.reachable()?;
        lock(&self.fleet.shed_dones).push(done.clone());
        Ok(())
    }

    pub fn set_fleet_notice(&self, machine: MachineId, notice: FleetNotice) {
        lock(&self.fleet.notices).insert(machine, notice);
    }

    pub fn fleet_notice(&self, machine: &MachineId) -> Option<FleetNotice> {
        lock(&self.fleet.notices).get(machine).cloned()
    }

    pub(super) fn fake_set_machine_state(
        &self,
        machine: &MachineId,
        state: MachineState,
        actor: Option<&str>,
    ) -> Result<()> {
        self.reachable()?;
        let mut notices = lock(&self.fleet.notices);
        let notice = notices.entry(machine.clone()).or_default();
        if notice.state != state {
            notice.version += 1;
            notice.state = state;
            notice.state_actor = Some(actor.unwrap_or(machine.as_str()).to_owned());
        }
        Ok(())
    }

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
