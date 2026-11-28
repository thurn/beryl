//! Sends a ticket that holds `master` back to the fastest machine that can take it now
//! (`ignite_core::machine_speed`).
//!
//! Seen on 2026-11-27: `master` stalled for more than eight hours behind a regression pinned to
//! the machine that held its culprit's transcript, which was much slower than the fleet's
//! fastest and was swapping, while the fastest had idle workers. Culprit affinity for such a
//! ticket is now only a preference, recorded in its metadata: the culprit's machine keeps the
//! ticket when it is nearly as fast as the fastest machine with a free slot, and otherwise the
//! fastest takes it. A machine pin that a person or worker added still binds.

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::dispatch::ReadyTicket;
use ignite_core::fleet::{FleetSettings, MachineState};
use ignite_core::ids::TicketId;
use ignite_core::machine_speed::{self, Candidate, Speed};
use ignite_core::placement::PATIENCE_S;
use ignite_core::protocol::hub::MachineFree;
use ignite_core::protocol::tickets::CulpritAffinity;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{capacity, fleet, machine_speeds, placement_deferrals, tickets};

use super::mix as dispatch_mix;
use super::queue::held;

/// A machine other than the asker that has room for a worker now.
struct Peer {
    name: String,
    os: Os,
    families: Vec<String>,
    build_slot_free: bool,
}

pub(super) struct Fastest {
    asker: String,
    speeds: BTreeMap<String, Speed>,
    peers: Vec<Peer>,
    /// The culprit affinity of each ready ticket that holds `master` back and has one.
    affinities: HashMap<TicketId, CulpritAffinity>,
}

impl Fastest {
    /// Also asks for the culprit's provider on the culprit's machine ([`Self::prefer_provider`]).
    pub(super) fn load(
        conn: &Connection,
        asker: &MachineFree,
        settings: &FleetSettings,
        ready: &mut [ReadyTicket],
        now: Timestamp,
    ) -> Result<Self> {
        let mut fastest = Self {
            asker: asker
                .name
                .clone()
                .unwrap_or_else(|| asker.machine.to_string()),
            speeds: BTreeMap::new(),
            peers: Vec::new(),
            affinities: HashMap::new(),
        };
        if !ready.iter().any(held::holds_master) {
            return Ok(fastest);
        }
        for ticket in ready.iter().filter(|ticket| held::holds_master(ticket)) {
            if let Some(affinity) = tickets::load(conn, ticket.id.as_str())?
                .as_ref()
                .and_then(crate::upkeep::culprit_affinity::affinity)
            {
                fastest.affinities.insert(ticket.id.clone(), affinity);
            }
        }
        fastest.speeds = machine_speeds::speeds(conn, settings)?;
        let reports = capacity::latest(conn, now)?;
        for machine in fleet::machines(conn)? {
            if machine.id == asker.machine.as_str() {
                // An asker's request may leave out its registered name.
                fastest.asker = machine.name;
                continue;
            }
            let state = settings.machine(&machine.name).state;
            if state == MachineState::Off
                || (state == MachineState::ChecksOnly && held::holds_one(conn, &machine.id)?)
            {
                continue;
            }
            let Some(report) = machine
                .id
                .parse()
                .ok()
                .and_then(|id| reports.get(&id).cloned())
            else {
                continue;
            };
            let room =
                report
                    .worker_room
                    .saturating_sub(dispatch_mix::pending(conn, &machine.id, now)?);
            if room == 0 || report.memory_pressed {
                continue;
            }
            let (os, families) =
                fleet::parse_dispatch_capabilities(&machine.os, &machine.families)?;
            fastest.peers.push(Peer {
                name: machine.name,
                os,
                families,
                build_slot_free: report.build_space.is_some_and(|space| space.free > 0),
            });
        }
        fastest.prefer_provider(ready, asker);
        Ok(fastest)
    }

    /// On the culprit's machine, asks for the culprit's provider when this machine offers it,
    /// so the worker can resume the culprit's transcript.
    fn prefer_provider(&self, ready: &mut [ReadyTicket], asker: &MachineFree) {
        let offered: Vec<&str> = match &asker.room {
            Some(room) => room.providers.iter().map(|p| p.name.as_str()).collect(),
            None => asker.idle_workers.iter().map(|w| w.app.as_str()).collect(),
        };
        for ticket in ready.iter_mut() {
            let Some(affinity) = self.affinities.get(&ticket.id) else {
                continue;
            };
            let provider = &affinity.landing.provider;
            if ticket.pins.provider.is_none()
                && self.is_affinity_machine(affinity)
                && offered.iter().any(|app| app.eq_ignore_ascii_case(provider))
            {
                ticket.pins.provider = Some(provider.clone());
            }
        }
    }

    fn is_affinity_machine(&self, affinity: &CulpritAffinity) -> bool {
        affinity_machine(affinity).is_some_and(|name| name.eq_ignore_ascii_case(&self.asker))
    }

    /// Why the asker should leave `ticket` to a faster machine, or `None` when it takes it. A
    /// ticket passed over for [`PATIENCE_S`] goes to whichever eligible machine asks, in case
    /// the preferred machine does not ask after all.
    pub(super) fn defer(
        &self,
        conn: &Connection,
        ticket: &ReadyTicket,
        now: Timestamp,
    ) -> Result<Option<String>> {
        if !held::holds_master(ticket) || self.peers.is_empty() {
            return Ok(None);
        }
        let speed = |name: &str| machine_speeds::of(&self.speeds, name);
        let mut candidates = vec![Candidate {
            name: &self.asker,
            speed: speed(&self.asker),
        }];
        candidates.extend(
            self.peers
                .iter()
                .filter(|peer| {
                    ticket.pins.admits_machine(&peer.name)
                        && ticket.platform_fits(peer.os, &peer.families)
                        && (peer.build_slot_free || !ticket.needs_build_slot)
                })
                .map(|peer| Candidate {
                    name: &peer.name,
                    speed: speed(&peer.name),
                }),
        );
        let affinity = self.affinities.get(&ticket.id).and_then(affinity_machine);
        let Some(best) = machine_speed::preferred(&candidates, &self.asker, affinity) else {
            return Ok(None);
        };
        if best.name.eq_ignore_ascii_case(&self.asker) {
            return Ok(None);
        }
        let first = placement_deferrals::first(conn, &ticket.id)?;
        if first.is_some_and(|first| now.seconds_since(first) >= PATIENCE_S) {
            return Ok(None);
        }
        if first.is_none() {
            placement_deferrals::record(conn, &ticket.id, now)?;
        }
        let why = if affinity.is_some_and(|name| name.eq_ignore_ascii_case(best.name)) {
            "it holds the culprit's transcript and is nearly as fast"
        } else {
            "it is the fastest machine that can take it"
        };
        Ok(Some(format!(
            "{} holds master back and is better placed on {} (speed {}, here {}): {why}",
            ticket.id,
            best.name,
            best.speed,
            speed(&self.asker),
        )))
    }
}

fn affinity_machine(affinity: &CulpritAffinity) -> Option<&str> {
    affinity.machine_label.strip_prefix("machine:")
}
