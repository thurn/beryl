//! Compare a dispatch candidate's build time here with the fleet's recent capacity reports.

use std::cell::Cell;
use std::collections::BTreeMap;

use rusqlite::Connection;

use ignite_core::capacity::CapacityReport;
use ignite_core::config::Os;
use ignite_core::dispatch::{self, ReadyTicket};
use ignite_core::dispatch_pressure::{DispatchPressure, needs_build_slot};
use ignite_core::fleet::{FleetSettings, MachineState};
use ignite_core::ids::IdError;
use ignite_core::mix::{self, ProviderLoad};
use ignite_core::placement::{self, MachineView, PlacementTicket};
use ignite_core::plan_usage::Hold;
use ignite_core::protocol::hub::{MachineFree, ProviderOffer};
use ignite_core::time::Timestamp;
use ignite_store::hub::{capacity, fleet, placement_deferrals};
use ignite_store::{Result, StoreError};

use super::mix as dispatch_mix;
use super::queue::{self, held};

pub(super) struct Placement<'a> {
    asker: &'a MachineFree,
    current: Option<CapacityReport>,
    peers: Vec<Peer>,
}

struct Peer {
    id: String,
    free_build_handoffs: Cell<Option<u32>>,
    name: String,
    os: Os,
    families: Vec<String>,
    capacity: CapacityReport,
    offers: Vec<ProviderOffer>,
    loads: Vec<ProviderLoad>,
}

fn compares(ticket: &ReadyTicket) -> bool {
    !held::holds_master(ticket) && ticket.pins.machine.is_none()
}

impl<'a> Placement<'a> {
    pub(super) fn load(
        conn: &Connection,
        asker: &'a MachineFree,
        settings: &FleetSettings,
        holds: &BTreeMap<String, Vec<Hold>>,
        ready: &[ReadyTicket],
        now: Timestamp,
    ) -> Result<Self> {
        let mut placement = Self {
            asker,
            current: None,
            peers: Vec::new(),
        };
        if !ready.iter().any(compares) {
            return Ok(placement);
        }
        let mut reports = capacity::latest(conn, now)?;
        let latest = reports.remove(&asker.machine);
        if reports.is_empty() {
            return Ok(placement);
        }
        placement.current = asking_capacity(asker, latest);
        if placement.current.is_none() {
            return Ok(placement);
        }
        for machine in fleet::machines(conn)? {
            if settings.machine(&machine.name).state != MachineState::Active {
                continue;
            }
            let id = machine
                .id
                .parse()
                .map_err(|error: IdError| StoreError::Invalid(error.to_string()))?;
            let Some(mut report) = reports.remove(&id) else {
                continue;
            };
            report.worker_room =
                report
                    .worker_room
                    .saturating_sub(dispatch_mix::pending(conn, &machine.id, now)?);
            if report.worker_room == 0 || report.memory_pressed {
                continue;
            }
            let (os, families) =
                fleet::parse_dispatch_capabilities(&machine.os, &machine.families)?;
            placement.peers.push(Peer::new(
                (machine.id, machine.name),
                os,
                families,
                report,
                settings,
                holds,
            ));
        }
        Ok(placement)
    }

    pub(super) fn defer(
        &self,
        conn: &Connection,
        ready: &ReadyTicket,
        provider: Option<&str>,
        now: Timestamp,
    ) -> Result<Option<String>> {
        let Some(capacity) = &self.current else {
            return Ok(None);
        };
        if self.peers.is_empty() || !compares(ready) {
            return Ok(None);
        }
        let asker = MachineView {
            name: self
                .asker
                .name
                .as_deref()
                .unwrap_or(self.asker.machine.as_str()),
            os: self.asker.os,
            families: &self.asker.families,
            state: MachineState::Active,
            capacity,
            provider,
        };
        let ticket = PlacementTicket {
            ready,
            pins: &ready.pins,
        };
        let mut peers: Vec<_> = self
            .peers
            .iter()
            .filter_map(|peer| peer.view(ready))
            .collect();
        if peers.is_empty() {
            return Ok(None);
        }
        let first = placement_deferrals::first(conn, &ready.id)?;
        let best = loop {
            let Some(best) = placement::preferred_machine(&ticket, &asker, &peers, first, now)
            else {
                return Ok(None);
            };
            let name = best.name;
            if !needs_build_slot(ready.expected_cores, &ready.files)
                || self
                    .peers
                    .iter()
                    .find(|peer| peer.name == name)
                    .expect("placement selected a peer")
                    .free_build_handoffs(conn, now)?
                    > 0
            {
                break best;
            }
            peers.retain(|peer| peer.name != name);
        };
        if first.is_none() {
            placement_deferrals::record(conn, &ready.id, now)?;
        }
        Ok(Some(dispatch::why_placement(&ticket, &asker, best)))
    }
}

/// Use current scheduler and pressure readings from the request. Older requests without
/// pressure can compare using their own fresh heartbeat's outputs when they have one.
fn asking_capacity(asker: &MachineFree, latest: Option<CapacityReport>) -> Option<CapacityReport> {
    if asker.pressure.is_none() && latest.is_none() {
        return None;
    }
    let mut report = latest.unwrap_or_default();
    if let Some(pressure) = &asker.pressure {
        report.queue_seconds = f64::from(pressure.queue_seconds);
        report.outputs.clone_from(&pressure.outputs);
        report.free_trees.clone_from(&pressure.free_trees);
        report.leased_trees.clone_from(&pressure.leased);
    }
    report.free_cores = asker.free_cores;
    report.admit_cores = asker.admit_cores.unwrap_or(report.admit_cores);
    // The candidate passed the asker's worker admission, including its pending reservations.
    report.worker_room = 1;
    report.memory_pressed = false;
    Some(report)
}

impl Peer {
    fn new(
        (id, name): (String, String),
        os: Os,
        families: Vec<String>,
        capacity: CapacityReport,
        settings: &FleetSettings,
        holds: &BTreeMap<String, Vec<Hold>>,
    ) -> Self {
        let offers = dispatch_mix::offers(&capacity);
        let loads = dispatch_mix::peer_loads(&offers, settings, holds);
        Self {
            id,
            free_build_handoffs: Cell::new(None),
            name,
            os,
            families,
            capacity,
            offers,
            loads,
        }
    }

    fn free_build_handoffs(&self, conn: &Connection, now: Timestamp) -> Result<u32> {
        if let Some(free) = self.free_build_handoffs.get() {
            return Ok(free);
        }
        let pressure = DispatchPressure {
            build_slots_free: self.capacity.build_space.map_or(0, |space| space.free),
            slot_holders: self.capacity.slot_holders.clone(),
            ..DispatchPressure::default()
        };
        let free = queue::peer_free_build_handoffs(
            conn,
            &self.id,
            now,
            &pressure,
            self.capacity.admit_cores,
        )?;
        self.free_build_handoffs.set(Some(free));
        Ok(free)
    }

    fn view(&self, ticket: &ReadyTicket) -> Option<MachineView<'_>> {
        if self
            .capacity
            .build_space
            .is_none_or(|space| space.free == 0)
            && needs_build_slot(ticket.expected_cores, &ticket.files)
        {
            return None;
        }
        let tier = dispatch::label(&ticket.labels, "tier:");
        let provider = mix::choose(
            &self.loads,
            &self.offers,
            tier,
            ticket.pins.provider.as_deref(),
        )
        .ok()?;
        Some(MachineView {
            name: &self.name,
            os: self.os,
            families: &self.families,
            state: MachineState::Active,
            capacity: &self.capacity,
            provider: Some(provider),
        })
    }
}
