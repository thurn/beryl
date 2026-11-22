//! Which machines can take a ticket that holds `master` back.

use std::collections::{BTreeMap, HashSet};

use rusqlite::{Connection, params};

use ignite_core::dispatch::{JUSTICIAR, ReadyTicket, TicketKind, label};
use ignite_core::fleet::{FleetSettings, MachineState};
use ignite_core::ids::MachineId;
use ignite_core::mix::{self, ProviderLoad};
use ignite_core::plan_usage::{Hold, model_for_tier};
use ignite_core::protocol::hub::{MachineFree, ProviderOffer, Reservation};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{capacity, fleet, provider_usage, tickets};

use crate::routes::dispatch::mix as dispatch_mix;
use crate::upkeep::regressions;

/// The states of a regression whose check still fails, so that its ticket holds `master` back.
pub const FAILING: [&str; 2] = ["open", ignite_store::hub::tips::REOPENED];

/// How far a pause limits what a machine is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paused {
    /// Every ready ticket may be offered.
    No,
    /// Only a ticket that holds `master` back, while the machine holds none already.
    MasterOnly,
    /// Nothing at all.
    Everything,
}

/// A daemon can ask for master blockers only even on an Active machine, for example when
/// its commands are waiting for cores. Only `ChecksOnly` machines defer to `Active` machines.
pub fn paused(conn: &Connection, free: &MachineFree, state: MachineState) -> Result<Paused> {
    Ok(
        if state == MachineState::Off || fleet::paused_everywhere(conn)? {
            Paused::Everything
        } else if state == MachineState::ChecksOnly || free.master_only {
            Paused::MasterOnly
        } else {
            Paused::No
        },
    )
}

pub(super) struct Fallback<'a> {
    free: &'a MachineFree,
    settings: &'a FleetSettings,
    holds: BTreeMap<String, Vec<Hold>>,
    active: Vec<ActiveMachine>,
}

struct ActiveMachine {
    name: String,
    os: String,
    providers: Providers,
}

enum Providers {
    Reported {
        offers: Vec<ProviderOffer>,
        loads: Vec<ProviderLoad>,
    },
    Legacy(Vec<String>),
}

impl<'a> Fallback<'a> {
    pub(super) fn load(
        conn: &Connection,
        settings: &'a FleetSettings,
        heard: &HashSet<String>,
        free: &'a MachineFree,
        now: Timestamp,
    ) -> Result<Self> {
        let reports = capacity::latest(conn, now)?;
        let holds = provider_usage::held(conn, settings, now)?;
        let active = fleet::machines(conn)?
            .into_iter()
            .filter(|machine| {
                machine.id != free.machine.as_str()
                    && settings.machine(&machine.name).state == MachineState::Active
                    && heard.contains(&machine.name.to_ascii_lowercase())
            })
            .map(|machine| {
                let report = machine
                    .id
                    .parse::<MachineId>()
                    .ok()
                    .and_then(|id| reports.get(&id));
                let providers = if let Some(report) = report {
                    let offers = dispatch_mix::offers(report);
                    let loads = dispatch_mix::peer_loads(&offers, settings, &holds);
                    Providers::Reported { offers, loads }
                } else {
                    let mut names: Vec<String> =
                        serde_json::from_str(&machine.cli_apps).unwrap_or_default();
                    if names.is_empty() {
                        // Older daemons did not list their CLIs, but identified each worker.
                        names.extend(
                            fleet::workers(conn, &machine.id)?
                                .into_iter()
                                .map(|worker| worker.app),
                        );
                    }
                    Providers::Legacy(names)
                };
                Ok(ActiveMachine {
                    name: machine.name,
                    os: machine.os,
                    providers,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            free,
            settings,
            holds,
            active,
        })
    }

    /// Defer to an active machine that can run the ticket's OS, pins and model. Worker room and
    /// busy cores do not affect precedence because this decision is only for master blockers.
    pub(super) fn allows(&self, ticket: &ReadyTicket) -> bool {
        let os_fits = |os: &str| ticket.needs_os.is_none_or(|needed| needed.as_str() == os);
        let name = self
            .free
            .name
            .as_deref()
            .unwrap_or(self.free.machine.as_str());
        holds_master(ticket)
            && os_fits(self.free.os.as_str())
            && ticket.pins.admits_machine(name)
            && !self.active.iter().any(|machine| {
                ticket.pins.admits_machine(&machine.name)
                    && os_fits(&machine.os)
                    && self.provider_fits(&machine.providers, ticket)
            })
    }

    fn provider_fits(&self, providers: &Providers, ticket: &ReadyTicket) -> bool {
        let tier = label(&ticket.labels, "tier:");
        match providers {
            Providers::Reported { offers, loads } => {
                mix::choose(loads, offers, tier, ticket.pins.provider.as_deref()).is_ok()
            }
            Providers::Legacy(names) => names.iter().any(|name| {
                ticket.pins.admits_provider(name)
                    && self.settings.provider(name).enabled
                    && self.holds.get(name).is_none_or(|holds| {
                        let model_ids = BTreeMap::new();
                        let model = model_for_tier(name, tier, &model_ids, None);
                        holds.iter().all(|hold| !hold.holds_model(model))
                    })
            }),
        }
    }
}

/// Whether the ticket holds `master` back: a justiciar ticket, or the ticket of a failing
/// regression.
pub fn holds_master(ticket: &ReadyTicket) -> bool {
    ticket.kind == TicketKind::Regression || ticket.labels.iter().any(|l| l == JUSTICIAR)
}

/// Replays a reservation whose reply was lost. If the requester is limited to master blockers,
/// it can retry an offer that still holds master back.
pub(super) fn retry_reservation(
    conn: &Connection,
    free: &MachineFree,
    paused: Paused,
) -> Result<Option<Reservation>> {
    let machine = free.machine.as_str();
    for worker in &free.idle_workers {
        let Some(reservation) =
            fleet::reservation_for_worker(conn, machine, worker.session.as_str())?
        else {
            continue;
        };
        if let Some(ticket) = tickets::load(conn, reservation.ticket.as_str())?
            && regressions::passing(conn, &ticket)?.is_some()
        {
            fleet::release_reservation(conn, reservation.ticket.as_str())?;
            continue;
        }
        if paused == Paused::No
            || conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM tickets t WHERE t.id = ?1
             AND (t.id IN (SELECT ticket FROM regressions WHERE state IN (?2, ?3))
                  OR EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = ?4)))",
                params![
                    reservation.ticket.as_str(),
                    FAILING[0],
                    FAILING[1],
                    JUSTICIAR
                ],
                |row| row.get(0),
            )?
        {
            return Ok(Some(reservation));
        }
    }
    Ok(None)
}

/// Whether `machine` already works or has reserved a ticket that holds `master` back, so that
/// a paused machine gives such tickets to one worker at a time.
pub fn holds_one(conn: &Connection, machine: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM tickets t
           WHERE ((t.status = 'in_progress' AND t.machine = ?1)
                  OR t.id IN (SELECT ticket FROM reservations WHERE machine = ?1))
             AND (t.id IN (SELECT ticket FROM regressions WHERE state IN (?2, ?3))
                  OR EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = ?4)))",
        params![machine, FAILING[0], FAILING[1], JUSTICIAR],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
#[path = "dispatch_held_tests.rs"]
mod tests;
