//! The queue snapshot behind `POST /next`: every ready ticket as `ignite_core::dispatch` sees it,
//! each group's allocation of workers, whether improvements are under their share, and each idle
//! worker's last files. The hub keeps no model of a machine's load; it reads what the machine
//! says it has free and the tickets in the database.
//!
//! What the hub cannot know is left out rather than guessed:
//!
//! - A ticket names its files as backquoted paths in its body or acceptance criteria. Areas are
//!   their first two directories, because workspace package roots are in each machine's
//!   `ignite.toml`, not at the hub.
//! - Dispatch resolves machine and provider pins independently through the ticket's ancestors.
//! - The backlog's ratios for the improvement share come from `crate::rules::backlog`, which
//!   reads last week's use from `meta`, never from `metrics` inside this transaction.
//!
//! Not every ready ticket is offered (see [`Snapshot::offered`]): only a ticket under an active
//! epic, under no epic, or one the hub filed itself, never one set aside for a person. While the
//! machine's hand-offs are paused only a ticket that holds `master` back is offered, and nothing
//! while every machine's are (see `dispatch_held.rs`).

use std::collections::{BTreeSet, HashMap, HashSet};

use rusqlite::Connection;

use ignite_core::admission::{self, Worked};
use ignite_core::config::Improvement;
use ignite_core::dispatch::{self, ReadyTicket, TicketKind};
use ignite_core::dispatch_pressure::{Budget, DispatchPressure, HandoffWait};
use ignite_core::fleet::{Allocations, FleetSettings, MachineState};
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::pins::Pins;
use ignite_core::protocol::hub::{MachineFree, Reservation};
use ignite_core::protocol::tickets::{
    LandedWork, REVERTED_KEY, Saving, Status, Ticket, TicketType,
};
use ignite_core::shares::{self, Group, Source, Unit};
use ignite_core::time::Timestamp;
use ignite_store::hub::{flaky_triage, fleet, fleet_settings, provider_usage, releases, tickets};
use ignite_store::{Result, StoreError};

use super::{families::Requirements, mix, placement, usage};
use crate::rules;
use crate::upkeep::filing::{SOURCE_FIELDS, allocation_source};
use crate::upkeep::regressions;

#[path = "dispatch_allocation.rs"]
mod allocation;
#[path = "dispatch_held.rs"]
pub(super) mod held;
#[path = "dispatch_pins.rs"]
mod pins;
use held::Paused;
#[path = "dispatch_capacity.rs"]
mod capacity;
#[path = "dispatch_shed.rs"]
pub(super) mod shed;

#[path = "dispatch_named_files.rs"]
mod files;
use files::{areas, resolved_files};
pub use files::{named_files, resolve_named_file};

#[path = "dispatch_view.rs"]
pub mod view;

#[path = "dispatch_snapshot.rs"]
pub(super) mod machine_snapshot;

const HOLDS_ONE: &str = "this machine already works a ticket that holds master back";

/// The ticket to reserve for `free`'s machine, already reserved and logged, or why there is none
/// ([`dispatch::why_nothing`]). Call it inside an `IMMEDIATE` transaction after lapsing old
/// reservations, so that two machines asking at once are answered one after the other and never
/// get one ticket.
pub fn next(
    conn: &Connection,
    free: &MachineFree,
    now: Timestamp,
) -> Result<std::result::Result<Reservation, HandoffWait>> {
    let machine = free.machine.as_str();
    let settings = fleet_settings::settings(conn)?;
    let name = free.name.as_deref().unwrap_or(machine);
    let state = settings.machine(name).state;
    let paused = held::paused(conn, free, state, now)?;
    if paused == Paused::Everything {
        let reason = if state == MachineState::Off {
            "this machine is off"
        } else {
            "hand-offs are paused on every machine"
        };
        return Ok(Err(reason.to_owned().into()));
    }
    if free.room.is_none()
        && let Some(reservation) = held::retry_reservation(conn, free, paused)?
    {
        return Ok(Ok(reservation));
    }
    // A checks-only machine works one master blocker at a time; an active one takes them all.
    if state == MachineState::ChecksOnly && held::holds_one(conn, machine)? {
        return Ok(Err(HOLDS_ONE.to_owned().into()));
    }
    let snapshot = Snapshot::load(conn, now)?;
    // A worker holding a reservation for another machine is not idle.
    let waiting: HashSet<String> = snapshot.reserved.values().cloned().collect();
    let mut free = free.clone();
    free.idle_workers
        .retain(|worker| !waiting.contains(worker.session.as_str()));
    let history = history(conn, machine)?;
    let landed = landed_files(conn)?;
    let capacity = capacity::read(
        conn,
        machine,
        &history,
        &landed,
        now,
        &snapshot.requirements,
        Budget::new(free.pressure.as_ref(), free.admit_cores, now),
    )?;
    let fallback = (state == MachineState::ChecksOnly)
        .then(|| held::Fallback::load(conn, &settings, &snapshot.heard, &free, now))
        .transpose()?;
    let mut ready = snapshot.ready_tickets(conn, &history, &landed, now, fallback.as_ref())?;
    let mut waiting_for_queue = Vec::new();
    if paused == Paused::MasterOnly {
        if state == MachineState::Active
            && free
                .pressure
                .as_ref()
                .is_some_and(DispatchPressure::queue_backlogged)
        {
            waiting_for_queue.extend(ready.extract_if(.., |ticket| !held::holds_master(ticket)));
        } else {
            ready.retain(held::holds_master);
        }
    }
    if state == MachineState::ChecksOnly && ready.is_empty() {
        return Ok(Err(
            "no ticket holding master back needs this checks-only machine"
                .to_owned()
                .into(),
        ));
    }
    dispatch_ready(
        conn,
        &free,
        now,
        &settings,
        &snapshot,
        &capacity,
        (paused, ready, waiting_for_queue),
    )
}

fn dispatch_ready(
    conn: &Connection,
    free: &MachineFree,
    now: Timestamp,
    settings: &FleetSettings,
    snapshot: &Snapshot,
    capacity: &capacity::Capacity<'_>,
    (paused, mut ready, mut waiting_for_queue): (Paused, Vec<ReadyTicket>, Vec<ReadyTicket>),
) -> Result<std::result::Result<Reservation, HandoffWait>> {
    let machine = free.machine.as_str();
    let epic_order = dispatch_groups(snapshot, &mut ready);
    let improvements = improvements_due(conn, machine)?;
    let holds = provider_usage::held(conn, settings, now)?;
    let placement = placement::Placement::load(conn, free, settings, &holds, &mut ready, now)?;
    let free = &MachineFree {
        free_cores: dispatch::room(free, &capacity.cores),
        ..free.clone()
    };
    let room = free
        .room
        .as_ref()
        .map(|room| {
            let loads = mix::loads(conn, settings, &holds, room, now)?;
            let pending = mix::pending(conn, machine, now)?;
            Ok::<_, StoreError>((room, loads, pending))
        })
        .transpose()?;
    let last_files = if room.is_none() {
        last_files(conn, machine)?
    } else {
        Vec::new()
    };
    let candidate_for = |ticket: &ReadyTicket| {
        let (session, provider) = if let Some((room, loads, pending)) = &room {
            if !ticket
                .pins
                .admits_machine(free.name.as_deref().unwrap_or(machine))
            {
                return None;
            }
            (
                None,
                mix::choose(loads, room, *pending, ticket).ok()?.to_owned(),
            )
        } else {
            let session = dispatch::pick_worker(free, ticket, &last_files, |worker| {
                usage::legacy_blocker(settings, &holds, worker, ticket).is_none()
            })?;
            let provider = free
                .idle_workers
                .iter()
                .find(|worker| worker.session == session)?;
            (Some(session), provider.app.clone())
        };
        Some(Candidate {
            ticket: ticket.id.clone(),
            kind: ticket.kind,
            session,
            provider: Some(provider),
            master_blocker: held::holds_master(ticket),
        })
    };
    let eligible = || dispatch::eligible_tickets(free, &ready, &epic_order, improvements);
    let candidates = eligible().filter_map(|ticket| Some((ticket, candidate_for(ticket)?)));
    let name = free.name.as_deref().unwrap_or(machine);
    let repair_can_start = |repair: &ReadyTicket| {
        candidate_for(repair).is_some() && capacity.budget.blocked(repair, name).is_none()
    };
    match capacity.choose(name, candidates, |ticket, candidate| {
        if let Some(reason) =
            held::defer_escalation(conn, paused, ticket, now, eligible, repair_can_start)?
        {
            return Ok(Some(reason));
        }
        placement.defer(
            conn,
            ticket,
            candidate.provider.as_deref(),
            now,
            &snapshot.requirements,
        )
    })? {
        Some(Ok(handoff)) => reserve(conn, &free.machine, handoff, now).map(Ok),
        Some(Err(reason)) => Ok(Err(reason)),
        None => {
            // Only master blockers enter admission during a backlog. If none can be offered,
            // explain the pressure on the next ordinary ticket the machine could otherwise take.
            if !waiting_for_queue.is_empty() {
                let order = dispatch_groups(snapshot, &mut waiting_for_queue);
                let wait =
                    dispatch::eligible_tickets(free, &waiting_for_queue, &order, improvements)
                        .filter(|ticket| candidate_for(ticket).is_some())
                        .find_map(|ticket| capacity.budget.blocked(ticket, name));
                if let Some(wait) = wait {
                    return Ok(Err(wait));
                }
            }
            let reason = if let Some((room, loads, pending)) = &room {
                dispatch::why_nothing_with_room(free, &ready, &epic_order, improvements, |ticket| {
                    mix::choose(loads, room, *pending, ticket).err()
                })
            } else {
                dispatch::next_for(free, &ready, &epic_order, improvements, &last_files)
                    .and_then(|handoff| {
                        usage::unavailable_reason(settings, &holds, free, &ready, &handoff)
                    })
                    .unwrap_or_else(|| dispatch::why_nothing(free, &ready, &epic_order))
            };
            Ok(Err(reason.into()))
        }
    }
}

fn dispatch_groups(snapshot: &Snapshot, ready: &mut Vec<ReadyTicket>) -> Vec<Group> {
    let demand = snapshot.demand(ready);
    // Improvements go ahead of the epic order and so skip the share, but not a group's
    // `--max-workers` cap.
    let full: BTreeSet<&Group> = demand
        .iter()
        .filter(|group| group.max_workers.is_some_and(|cap| group.held >= cap))
        .map(|group| &group.epic)
        .collect();
    ready.retain(|ticket| {
        held::holds_master(ticket)
            || ticket.kind != TicketKind::Improvement
            || !full.contains(&ticket.epic)
    });
    shares::epic_order(&demand)
}

/// Check live reservations and claims before deferring to a peer's heartbeat.
pub(super) fn peer_free_build_handoffs(
    conn: &Connection,
    machine: &str,
    now: Timestamp,
    pressure: &DispatchPressure,
    admit_cores: f64,
    requirements: &Requirements,
) -> Result<u32> {
    let worked = history(conn, machine)?;
    let landed = landed_files(conn)?;
    let held = capacity::read(
        conn,
        machine,
        &worked,
        &landed,
        now,
        requirements,
        Budget::new(Some(pressure), Some(admit_cores), now),
    )?;
    Ok(held.budget.free_build_handoffs())
}

/// Lane hand-offs can start ordinary work, so only an Active machine can continue a chain, and
/// no pause of every machine may hold.
pub(super) fn chain_machine(
    conn: &Connection,
    machine: &MachineId,
    now: Timestamp,
) -> Result<Option<String>> {
    let name = fleet::name_of(conn, machine.as_str())?.unwrap_or_else(|| machine.to_string());
    let active = fleet_settings::settings(conn)?.machine(&name).state == MachineState::Active;
    Ok((active && fleet::fleet_pause(conn, now)? == fleet::FleetPause::None).then_some(name))
}

/// The next ready ticket of the `lane:` chain `lane`, reserved for the session that finished
/// the previous one, or `None`. A chain's next ticket skips the dispatch order and the share,
/// but not a pause or the rules of which tickets are offered.
pub fn chain_next(
    conn: &Connection,
    lane: &str,
    machine: &MachineId,
    session: &SessionId,
    provider: Option<&str>,
    now: Timestamp,
    pressure: Option<&DispatchPressure>,
) -> Result<Option<Reservation>> {
    let Some(name) = chain_machine(conn, machine, now)? else {
        return Ok(None);
    };
    let snapshot = Snapshot::load(conn, now)?;
    let history = history(conn, machine.as_str())?;
    let landed = landed_files(conn)?;
    let ready = snapshot.ready_tickets(conn, &history, &landed, now, None)?;
    // A room-based claim records its provider even if the daemon has not yet
    // reported the session. Older claims may still need the heartbeat's worker row.
    let app = match provider {
        Some(provider) => provider.to_owned(),
        None => fleet::workers(conn, machine.as_str())?
            .into_iter()
            .find(|worker| worker.session == session.as_str())
            .map(|worker| worker.app)
            .unwrap_or_default(),
    };
    let Some(next) = dispatch::chain_next(lane, &ready) else {
        return Ok(None);
    };
    if !pins::allows_continuation(conn, next, machine, &name, &app)? {
        return Ok(None);
    }
    if mix::continuation_blocked(conn, machine.as_str(), &app, next, now)? {
        return Ok(None);
    }
    if capacity::read(
        conn,
        machine.as_str(),
        &history,
        &landed,
        now,
        &snapshot.requirements,
        Budget::new(pressure, None, now),
    )?
    .budget
    .blocked(next, &name)
    .is_some()
    {
        return Ok(None);
    }
    let candidate = Candidate {
        ticket: next.id.clone(),
        kind: next.kind,
        session: Some(session.clone()),
        provider: (!app.is_empty()).then_some(app),
        master_blocker: held::holds_master(next),
    };
    reserve(conn, machine, candidate, now).map(Some)
}

struct Candidate {
    ticket: TicketId,
    kind: TicketKind,
    session: Option<SessionId>,
    provider: Option<String>,
    master_blocker: bool,
}

fn reserve(
    conn: &Connection,
    machine: &MachineId,
    candidate: Candidate,
    now: Timestamp,
) -> Result<Reservation> {
    let code = fleet::reservation_code()?;
    let session = match candidate.session {
        Some(session) => session,
        None => format!("reservation-{}", fleet::reservation_code()?)
            .parse()
            .map_err(|error: ignite_core::ids::IdError| StoreError::Invalid(error.to_string()))?,
    };
    let reservation = Reservation {
        provider: candidate.provider,
        master_blocker: candidate.master_blocker,
        ticket: candidate.ticket,
        machine: machine.clone(),
        session,
        expires_at: fleet::reservation_expiry(now),
        code,
    };
    fleet::reserve(conn, &reservation)?;
    let kind = match candidate.kind {
        TicketKind::Regression => "regression",
        TicketKind::Improvement => "improvement",
        TicketKind::Ordinary => "product",
    };
    fleet::record_handoff(
        conn,
        machine.as_str(),
        reservation.ticket.as_str(),
        kind,
        now,
    )?;
    Ok(reservation)
}

/// The parts of a ticket that place it in a group.
struct Node {
    parent: Option<TicketId>,
    workers: Option<u32>,
    source: Option<Source>,
    max_workers: Option<u32>,
    in_progress: bool,
}

/// Every ticket's place in the tree, the reservations and the chains of waiting tickets.
struct Snapshot {
    nodes: HashMap<TicketId, Node>,
    allocations: Allocations,
    /// Reserved tickets and the session each waits for.
    reserved: HashMap<String, String>,
    /// For each ticket, the live tickets that depend on it.
    dependents: HashMap<String, Vec<String>>,
    requirements: Requirements,
    tip_blockers: HashSet<String>,
    ready: Vec<TicketId>,
    /// The names, in lower case, of machines heard from within the fallback interval.
    heard: HashSet<String>,
}

impl Snapshot {
    fn load(conn: &Connection, now: Timestamp) -> Result<Snapshot> {
        // A meta-ticket's schedule is what asks for its copies to be worked, so one with no
        // allocation counts as an epic with an allocation of 1. Without that its copies, which
        // have a parent and so are not loose, would wait for an allocation nobody sets.
        let mut statement = conn.prepare(&format!(
            "SELECT id, parent, coalesce(workers, CASE type WHEN 'meta' THEN 1 END), max_workers,
             status, {SOURCE_FIELDS} FROM tickets"
        ))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<u32>>(2)?,
                row.get::<_, Option<u32>>(3)?,
                row.get::<_, String>(4)?,
                allocation_source(row, 5)?,
            ))
        })?;
        let mut nodes = HashMap::new();
        let mut live = HashSet::new();
        for row in rows {
            let (id, parent, workers, max_workers, status, source) = row?;
            if status != "closed" {
                live.insert(id.clone());
            }
            nodes.insert(
                parse_id(&id)?,
                Node {
                    parent: parent.as_deref().map(parse_id).transpose()?,
                    workers,
                    source,
                    max_workers,
                    in_progress: status == "in_progress",
                },
            );
        }
        let reserved = conn
            .prepare("SELECT ticket, session FROM reservations WHERE expires_at > ?1")?
            .query_map([now.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
        let waits = pairs(
            conn,
            "SELECT dst, src FROM edges WHERE kind IN ('blocks', 'conditional-blocks', 'waits-for')",
        )?;
        for (dst, src) in waits {
            if live.contains(&src) {
                dependents.entry(dst).or_default().push(src);
            }
        }
        let heard = pins::heard_machines(conn, now)?;
        Ok(Snapshot {
            nodes,
            allocations: fleet_settings::settings(conn)?
                .allocations
                .unwrap_or_default(),
            reserved,
            dependents,
            requirements: Requirements::load(conn)?,
            tip_blockers: held::tip_blockers(conn)?,
            // A ticket released by two sessions waits for a justiciar (see `releases`).
            ready: releases::offerable(conn, tickets::ready(conn, now)?)?,
            heard,
        })
    }

    /// The cap on a group's workers: the smallest `max_workers` of the group's own ticket and
    /// all its ancestors, so a cap on an outer epic limits the epics inside it.
    fn cap(&self, id: &TicketId) -> Option<u32> {
        std::iter::once(id.clone())
            .chain(self.ancestors(id))
            .filter_map(|id| self.nodes.get(&id).and_then(|node| node.max_workers))
            .min()
    }

    /// The longest chain of live tickets waiting behind `id`.
    fn chain(&self, id: &str, memo: &mut HashMap<String, u32>, path: &mut HashSet<String>) -> u32 {
        if let Some(&known) = memo.get(id) {
            return known;
        }
        if !path.insert(id.to_owned()) {
            return 0;
        }
        let longest = self.dependents.get(id).map_or(0, |dependents| {
            dependents
                .iter()
                .map(|dependent| 1 + self.chain(dependent, memo, path))
                .max()
                .unwrap_or(0)
        });
        path.remove(id);
        memo.insert(id.to_owned(), longest);
        longest
    }

    /// The ready tickets nobody holds or has reserved that may be offered to some machine,
    /// leaving out epics and milestones, which are containers rather than work. Each carries
    /// its effective pins, which `ignite_core::dispatch` matches against the asking machine.
    fn ready_tickets(
        &self,
        conn: &Connection,
        history: &[Worked],
        landed: &[String],
        now: Timestamp,
        fallback: Option<&held::Fallback<'_>>,
    ) -> Result<Vec<ReadyTicket>> {
        let mut memo = HashMap::new();
        let mut ready = Vec::new();
        let triage_busy = flaky_triage::slot_taken(conn, now)?;
        for id in &self.ready {
            if self.reserved.contains_key(id.as_str()) {
                continue;
            }
            let Some(ticket) = tickets::load(conn, id.as_str())? else {
                continue;
            };
            if ticket.status != Status::Open
                || ticket.assignee.is_some()
                || (triage_busy && flaky_triage::is_batch(&ticket))
                || matches!(ticket.ticket_type, TicketType::Epic | TicketType::Milestone)
            {
                continue;
            }
            if self.requirements.is_regression(id.as_str())
                && regressions::passing(conn, &ticket)?.is_some()
            {
                continue;
            }
            if self.parked(&ticket, &self.group(id)).is_some() {
                continue;
            }
            let pins = pins::effective(conn, &ticket, &self.heard, now)?;
            let candidate = self.ready_ticket(ticket, pins, history, landed, &mut memo, now);
            if fallback.is_some_and(|fallback| !fallback.allows(&candidate)) {
                continue;
            }
            ready.push(candidate);
        }
        Ok(ready)
    }

    /// The ticket's ancestors, nearest first, stopping at a cycle.
    fn ancestors(&self, id: &TicketId) -> impl Iterator<Item = TicketId> + '_ {
        let mut seen = HashSet::new();
        let mut parent = self.nodes.get(id).and_then(|node| node.parent.clone());
        std::iter::from_fn(move || {
            let id = parent.take()?;
            if !seen.insert(id.clone()) {
                return None;
            }
            parent = self.nodes.get(&id).and_then(|node| node.parent.clone());
            Some(id)
        })
    }

    fn ready_ticket(
        &self,
        ticket: Ticket,
        pins: Pins,
        history: &[Worked],
        landed: &[String],
        memo: &mut HashMap<String, u32>,
        now: Timestamp,
    ) -> ReadyTicket {
        let files = named_files(&format!("{}\n{}", ticket.body, ticket.acceptance));
        let resolved = resolved_files(&files, landed);
        let families = self
            .requirements
            .for_ticket(ticket.id.as_str(), ticket.needs_os, &resolved);
        let expected_cores = expected_cores(&ticket, &resolved, history, now);
        let needs_build_slot = self.requirements.needs_build_slot(
            ticket.id.as_str(),
            ticket.ticket_type,
            ticket.needs_os,
            expected_cores,
            &resolved,
        );
        let saving = ticket
            .saving
            .as_ref()
            .and_then(|Saving { unit, amount, .. }| Unit::parse(unit).map(|unit| (unit, *amount)));
        let kind = if self.requirements.is_regression(ticket.id.as_str()) {
            TicketKind::Regression
        } else if ticket.saving.is_some() {
            TicketKind::Improvement
        } else {
            TicketKind::Ordinary
        };
        let has_saved_work = ticket.metadata.contains_key(super::SAVED_BRANCH)
            || ticket.metadata.contains_key(REVERTED_KEY);
        ReadyTicket {
            pins,
            needs_os: ticket.needs_os,
            kind,
            holds_tip: self.tip_blockers.contains(ticket.id.as_str()),
            has_saved_work,
            epic: self.group(&ticket.id),
            allocation: ticket
                .local_allocation()
                .unwrap_or(shares::DEFAULT_TICKET_ALLOCATION),
            chain: self.chain(ticket.id.as_str(), memo, &mut HashSet::new()),
            created_at: ticket.created_at,
            expected_cores,
            needs_build_slot,
            families,
            files,
            saving,
            labels: ticket.labels,
            id: ticket.id,
        }
    }
}

/// A ticket's expected cores ([`admission::expected_cores`]) from the planner's figure, the
/// areas of `files`, the files it names, and the machine's `history`.
fn expected_cores(ticket: &Ticket, files: &[String], history: &[Worked], now: Timestamp) -> f64 {
    admission::expected_cores(ticket.cores, &areas(files), history, now)
}

/// The repository paths that every landed ticket touched, without repeats, standing in for the
/// list of tracked files that the hub cannot read.
fn landed_files(conn: &Connection) -> Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT landed FROM tickets WHERE landed IS NOT NULL")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut files = BTreeSet::new();
    for row in rows {
        if let Ok(work) = serde_json::from_str::<LandedWork>(&row?) {
            files.extend(work.files);
        }
    }
    Ok(files.into_iter().collect())
}

/// The machine's landed tickets, as history for expected cores: the areas of the files they
/// touched, the CPU their records used and the minutes from claim to landing.
fn history(conn: &Connection, machine: &str) -> Result<Vec<Worked>> {
    let mut statement = conn.prepare(
        "SELECT landed, claimed_at, landed_at,
           (SELECT COALESCE(SUM(cpu_s), 0) FROM records WHERE records.ticket = tickets.id)
         FROM tickets
         WHERE machine = ?1 AND landed_at IS NOT NULL AND claimed_at IS NOT NULL",
    )?;
    let rows = statement.query_map([machine], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, f64>(3)?,
        ))
    })?;
    let mut history = Vec::new();
    for row in rows {
        let (landed, claimed_at, landed_at, cpu_s) = row?;
        let work: Option<LandedWork> = landed.and_then(|text| serde_json::from_str(&text).ok());
        let (Ok(claimed_at), Ok(landed_at)) = (
            claimed_at.parse::<Timestamp>(),
            landed_at.parse::<Timestamp>(),
        ) else {
            continue;
        };
        history.push(Worked {
            landed_at,
            areas: areas(&work.map(|w| w.files).unwrap_or_default()),
            core_minutes: cpu_s / 60.0,
            minutes: landed_at.seconds_since(claimed_at) / 60.0,
        });
    }
    Ok(history)
}

/// `None` when improvements have their share of the machine's recent hand-offs, and otherwise
/// the unit whose largest saving goes first: the one after the last improvement's unit.
fn improvements_due(conn: &Connection, machine: &str) -> Result<Option<Unit>> {
    let config = Improvement::default();
    let recent = taken_handoffs(conn, machine, config.window)?;
    let kinds: Vec<TicketKind> = recent
        .iter()
        .rev()
        .map(|(_, kind)| match kind.as_str() {
            "improvement" => TicketKind::Improvement,
            "regression" => TicketKind::Regression,
            _ => TicketKind::Ordinary,
        })
        .collect();
    let share = shares::improvement_share(rules::backlog::ratios(conn)?.fraction, &config);
    if !shares::improvements_due(&kinds, share, &config) {
        return Ok(None);
    }
    let mut last = None;
    if let Some((ticket, _)) = recent.iter().find(|(_, kind)| kind == "improvement") {
        last = tickets::load(conn, ticket)?
            .and_then(|ticket| ticket.saving)
            .and_then(|saving| Unit::parse(&saving.unit));
    }
    Ok(Some(shares::first_unit(last)))
}

/// The last `n` hand-offs to a machine that were not regressions and whose reservation did not
/// lapse, newest first, as `(ticket, kind)`: the improvement share is a share of these. A
/// hand-off counts while its reservation stands, and once the machine has claimed the ticket
/// since; a reservation that lapsed unclaimed was never work, so it does not count.
fn taken_handoffs(conn: &Connection, machine: &str, n: u32) -> Result<Vec<(String, String)>> {
    let mut statement = conn.prepare(
        "SELECT h.ticket, h.kind FROM handoffs h
         WHERE h.machine = ?1 AND h.kind != 'regression'
           AND (EXISTS (SELECT 1 FROM reservations r
                        WHERE r.ticket = h.ticket AND r.machine = h.machine)
                OR EXISTS (SELECT 1 FROM tickets t
                           WHERE t.id = h.ticket AND t.machine = h.machine
                             AND t.claimed_at >= h.at))
         ORDER BY h.seq DESC LIMIT ?2",
    )?;
    let rows = statement
        .query_map(rusqlite::params![machine, i64::from(n)], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

/// Each worker's last ticket's files, as its machine last reported them.
fn last_files(conn: &Connection, machine: &str) -> Result<Vec<(SessionId, Vec<String>)>> {
    let mut last = Vec::new();
    for worker in fleet::workers(conn, machine)? {
        let (Ok(session), Ok(files)) = (
            worker.session.parse::<SessionId>(),
            serde_json::from_str::<Vec<String>>(&worker.files),
        ) else {
            continue;
        };
        last.push((session, files));
    }
    Ok(last)
}

fn pairs(conn: &Connection, sql: &str) -> Result<Vec<(String, String)>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

fn parse_id(id: &str) -> Result<TicketId> {
    id.parse()
        .map_err(|_| StoreError::Invalid(format!("bad ticket ID {id}")))
}
