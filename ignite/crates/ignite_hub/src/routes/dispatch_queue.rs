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
//! - The hub holds no check manifests, so a ticket's families are empty and every machine can
//!   run them.
//! - Dispatch resolves machine and provider pins independently through the ticket's ancestors.
//! - The backlog's ratios for the improvement share come from `crate::rules::backlog`, which
//!   reads last week's use from `meta`, never from `metrics` inside this transaction.
//!
//! Not every ready ticket is offered (see [`Snapshot::offered`]): only a ticket under an active
//! epic, under no epic, or one the hub filed itself, never one set aside for a person. While the
//! machine's hand-offs are paused only a ticket that holds `master` back is offered, and nothing
//! while every machine's are (see `dispatch_held.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use rusqlite::Connection;

use ignite_core::admission::{self, Worked};
use ignite_core::config::Improvement;
use ignite_core::dispatch::{self, ReadyTicket, TicketKind};
use ignite_core::dispatch_pressure::{DispatchPressure, HandoffWait};
use ignite_core::fleet::{FleetSettings, MachineState};
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::pins::Pins;
use ignite_core::protocol::hub::{MachineFree, Reservation};
use ignite_core::protocol::tickets::{
    LandedWork, REVERTED_KEY, Saving, Status, Ticket, TicketType,
};
use ignite_core::shares::{self, EpicDemand, Group, Unit};
use ignite_core::time::Timestamp;
use ignite_store::hub::{fleet, fleet_settings, provider_usage, releases, tickets};
use ignite_store::{Result, StoreError};

use super::{mix, usage};
use crate::{rules, upkeep::regressions};

/// The labels of tickets the hub files itself, which are offered whether or not an epic is
/// active. `hub` is on everything the hub files today (`upkeep::filing::HUB_LABEL`); the other
/// labels are the ones tickets filed before it carry, and `justiciar` marks the judge's
/// reports of faults in the system itself.
const HUB_FILED: [&str; 10] = [
    crate::upkeep::filing::HUB_LABEL,
    "regression",
    "improvement",
    "machine",
    "incomplete",
    "cache-audit",
    "graph-repair",
    "fleet",
    "flaky",
    dispatch::JUSTICIAR,
];

/// The label of a ticket set aside for a person, which no machine is offered.
const NEEDS_PERSON: &str = "needs:person";

#[path = "dispatch_pins.rs"]
mod pins;

#[path = "dispatch_held.rs"]
pub(super) mod held;
use held::Paused;

#[path = "dispatch_capacity.rs"]
mod capacity;

#[path = "dispatch_named_files.rs"]
mod files;
use files::areas;
pub use files::{named_files, resolve_named_file};

#[path = "dispatch_view.rs"]
pub mod view;

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
    let paused = held::paused(conn, free, state)?;
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
    if paused == Paused::MasterOnly && held::holds_one(conn, machine)? {
        return Ok(Err(
            "this machine already works a ticket that holds master back"
                .to_owned()
                .into(),
        ));
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
        free.pressure.as_ref(),
    )?;
    free.free_cores = dispatch::room(&free, &capacity.cores);
    let fallback = (state == MachineState::ChecksOnly)
        .then(|| held::Fallback::load(conn, &settings, &snapshot.heard, &free))
        .transpose()?;
    let mut ready = snapshot.ready_tickets(conn, &history, &landed, now, fallback.as_ref())?;
    if paused == Paused::MasterOnly {
        ready.retain(held::holds_master);
    }
    if state == MachineState::ChecksOnly && ready.is_empty() {
        return Ok(Err(
            "no ticket holding master back needs this checks-only machine"
                .to_owned()
                .into(),
        ));
    }
    dispatch_ready(conn, &free, now, &settings, &snapshot, &capacity, ready)
}

fn dispatch_ready(
    conn: &Connection,
    free: &MachineFree,
    now: Timestamp,
    settings: &FleetSettings,
    snapshot: &Snapshot,
    capacity: &capacity::Capacity<'_>,
    mut ready: Vec<ReadyTicket>,
) -> Result<std::result::Result<Reservation, HandoffWait>> {
    let machine = free.machine.as_str();
    let demand = snapshot.demand(&ready);
    // Improvements go ahead of the epic order and so skip the share, but not a group's
    // `--max-workers` cap.
    let full: BTreeSet<&Group> = demand
        .iter()
        .filter(|group| group.max_workers.is_some_and(|cap| group.held >= cap))
        .map(|group| &group.epic)
        .collect();
    ready.retain(|ticket| ticket.kind != TicketKind::Improvement || !full.contains(&ticket.epic));
    let epic_order = shares::epic_order(&demand);
    let improvements = improvements_due(conn, machine)?;
    let holds = provider_usage::held(conn, settings, now)?;
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
    let pick = |ready: &[ReadyTicket]| {
        if let Some((room, loads, pending)) = &room {
            return dispatch::next_ticket_with(free, ready, &epic_order, improvements, |ticket| {
                if !ticket
                    .pins
                    .admits_machine(free.name.as_deref().unwrap_or(machine))
                {
                    return None;
                }
                let provider = mix::choose(loads, room, *pending, ticket).ok()?;
                Some(Candidate {
                    ticket: ticket.id.clone(),
                    kind: ticket.kind,
                    session: None,
                    provider: Some(provider.to_owned()),
                })
            });
        }
        let handoff = dispatch::next_for_with(
            free,
            ready,
            &epic_order,
            improvements,
            &last_files,
            |worker, ticket| usage::blocking_hold(&holds, worker, ticket).is_none(),
        )?;
        let kind = ready
            .iter()
            .find(|ticket| ticket.id == handoff.ticket)
            .map_or(TicketKind::Ordinary, |ticket| ticket.kind);
        let provider = free
            .idle_workers
            .iter()
            .find(|worker| worker.session == handoff.session)
            .map(|worker| worker.app.clone());
        Some(Candidate {
            ticket: handoff.ticket,
            kind,
            session: Some(handoff.session),
            provider,
        })
    };
    match capacity.choose(&mut ready, pick) {
        Some(Ok(handoff)) => reserve(conn, &free.machine, handoff, now).map(Ok),
        Some(Err(reason)) => Ok(Err(reason)),
        None => {
            let reason = if let Some((room, loads, pending)) = &room {
                dispatch::why_nothing_with_room(free, &ready, &epic_order, improvements, |ticket| {
                    mix::choose(loads, room, *pending, ticket).err()
                })
            } else {
                dispatch::next_for(free, &ready, &epic_order, improvements, &last_files)
                    .and_then(|handoff| usage::hold_reason(&holds, free, &ready, &handoff))
                    .unwrap_or_else(|| dispatch::why_nothing(free, &ready, &epic_order))
            };
            Ok(Err(reason.into()))
        }
    }
}

/// Lane hand-offs can start ordinary work, so only an Active machine can continue a chain.
pub(super) fn chain_machine(conn: &Connection, machine: &MachineId) -> Result<Option<String>> {
    let name = fleet::name_of(conn, machine.as_str())?.unwrap_or_else(|| machine.to_string());
    let active = fleet_settings::settings(conn)?.machine(&name).state == MachineState::Active;
    Ok((active && !fleet::paused_everywhere(conn)?).then_some(name))
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
    let Some(name) = chain_machine(conn, machine)? else {
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
    if let Some(pressure) = pressure
        && let Some(budget) = capacity::read(
            conn,
            machine.as_str(),
            &history,
            &landed,
            now,
            Some(pressure),
        )?
        .disk
        && budget.blocked(next).is_some()
    {
        return Ok(None);
    }
    let candidate = Candidate {
        ticket: next.id.clone(),
        kind: next.kind,
        session: Some(session.clone()),
        provider: (!app.is_empty()).then_some(app),
    };
    reserve(conn, machine, candidate, now).map(Some)
}

struct Candidate {
    ticket: TicketId,
    kind: TicketKind,
    session: Option<SessionId>,
    provider: Option<String>,
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
    max_workers: Option<u32>,
    in_progress: bool,
}

/// Every ticket's place in the tree, the reservations and the chains of waiting tickets.
struct Snapshot {
    nodes: HashMap<TicketId, Node>,
    /// Reserved tickets and the session each waits for.
    reserved: HashMap<String, String>,
    /// For each ticket, the live tickets that depend on it.
    dependents: HashMap<String, Vec<String>>,
    regressions: HashSet<String>,
    ready: Vec<TicketId>,
    /// The names, in lower case, of machines heard from within the fallback interval.
    heard: HashSet<String>,
}

impl Snapshot {
    fn load(conn: &Connection, now: Timestamp) -> Result<Snapshot> {
        // A meta-ticket's schedule is what asks for its copies to be worked, so one with no
        // allocation counts as an epic with an allocation of 1. Without that its copies, which
        // have a parent and so are not loose, would wait for an allocation nobody sets.
        let mut statement = conn.prepare(
            "SELECT id, parent, coalesce(workers, CASE type WHEN 'meta' THEN 1 END), max_workers, \
             status FROM tickets",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<u32>>(2)?,
                row.get::<_, Option<u32>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut nodes = HashMap::new();
        let mut live = HashSet::new();
        for row in rows {
            let (id, parent, workers, max_workers, status) = row?;
            if status != "closed" {
                live.insert(id.clone());
            }
            nodes.insert(
                parse_id(&id)?,
                Node {
                    parent: parent.as_deref().map(parse_id).transpose()?,
                    workers,
                    max_workers,
                    in_progress: status == "in_progress",
                },
            );
        }
        let reserved = pairs(conn, "SELECT ticket, session FROM reservations")?
            .into_iter()
            .collect();
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
        let mut statement =
            conn.prepare("SELECT ticket FROM regressions WHERE state IN (?1, ?2)")?;
        let regressions = statement
            .query_map(held::FAILING, |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(Snapshot {
            nodes,
            reserved,
            dependents,
            regressions,
            // A ticket released by two sessions waits for a justiciar (see `releases`).
            ready: releases::offerable(conn, tickets::ready(conn, now)?)?,
            heard,
        })
    }

    /// The group a ticket shares workers in: its ancestors, nearest first, through
    /// [`shares::group_of`].
    fn group(&self, id: &TicketId) -> Group {
        let ancestors: Vec<(TicketId, Option<u32>)> = self
            .ancestors(id)
            .map(|id| {
                let workers = self.nodes.get(&id).and_then(|n| n.workers);
                (id, workers)
            })
            .collect();
        shares::group_of(&ancestors)
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
        for id in &self.ready {
            if self.reserved.contains_key(id.as_str()) {
                continue;
            }
            let Some(ticket) = tickets::load(conn, id.as_str())? else {
                continue;
            };
            if ticket.status != Status::Open
                || ticket.assignee.is_some()
                || matches!(ticket.ticket_type, TicketType::Epic | TicketType::Milestone)
            {
                continue;
            }
            if self.regressions.contains(id.as_str())
                && regressions::passing(conn, &ticket)?.is_some()
            {
                continue;
            }
            let Some(ticket) = self.offered(ticket) else {
                continue;
            };
            let pins = pins::effective(conn, &ticket, &self.heard)?;
            let candidate = self.ready_ticket(ticket, pins, history, landed, &mut memo, now);
            if fallback.is_some_and(|fallback| !fallback.allows(&candidate)) {
                continue;
            }
            ready.push(candidate);
        }
        Ok(ready)
    }

    /// The ticket if it may be offered: it is under an active epic, under no epic at all, or
    /// the hub filed it itself, and it is not set aside for a person (`needs:person`).
    ///
    /// An epic is active when it has an allocation (the `workers` column) greater than zero: the
    /// nearest ancestor with an allocation decides, so an epic set to 0 stops its milestones however their
    /// parent is set. A meta-ticket always has one (see [`Snapshot::load`]), so its copies are
    /// offered. A ticket with no parent has no epic a person could activate, so a person
    /// releasing it (only an `open` ticket gets here) is what asks for it to be worked. A ticket
    /// started by name skips these rules, because `/claims` does not ask the queue.
    fn offered(&self, ticket: Ticket) -> Option<Ticket> {
        let filed_by_hub = self.regressions.contains(ticket.id.as_str())
            || ticket.metadata.contains_key(REVERTED_KEY)
            || ticket
                .labels
                .iter()
                .any(|label| HUB_FILED.contains(&label.as_str()));
        let mut ancestors = self.ancestors(&ticket.id).peekable();
        let loose = ancestors.peek().is_none();
        let active = loose
            || ancestors
                .find_map(|id| self.nodes.get(&id).and_then(|node| node.workers))
                .is_some_and(|workers| workers > 0);
        let for_person = ticket.labels.iter().any(|label| label == NEEDS_PERSON);
        ((filed_by_hub || active) && !for_person).then_some(ticket)
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
        let expected_cores = expected_cores(&ticket, &files, history, landed, now);
        let saving = ticket
            .saving
            .as_ref()
            .and_then(|Saving { unit, amount, .. }| Unit::parse(unit).map(|unit| (unit, *amount)));
        let kind = if self.regressions.contains(ticket.id.as_str()) {
            TicketKind::Regression
        } else if ticket.saving.is_some() {
            TicketKind::Improvement
        } else {
            TicketKind::Ordinary
        };
        ReadyTicket {
            pins,
            needs_os: ticket.needs_os,
            kind,
            epic: self.group(&ticket.id),
            priority: ticket.priority,
            chain: self.chain(ticket.id.as_str(), memo, &mut HashSet::new()),
            created_at: ticket.created_at,
            expected_cores,
            families: Vec::new(),
            files,
            saving,
            labels: ticket.labels,
            id: ticket.id,
        }
    }

    /// Each group's share, cap, ready tickets and tickets in progress or reserved, across every
    /// machine, for the groups that have a ready ticket. Sorted by group, so the input order that
    /// breaks ties is the same on every request.
    fn demand(&self, ready: &[ReadyTicket]) -> Vec<EpicDemand> {
        let mut groups: BTreeMap<Group, EpicDemand> = BTreeMap::new();
        for ticket in ready {
            let group = ticket.epic.clone();
            let node = group.as_ref().and_then(|id| self.nodes.get(id));
            let cap = group.as_ref().and_then(|id| self.cap(id));
            groups
                .entry(group.clone())
                .or_insert_with(|| EpicDemand {
                    epic: group,
                    allocation: node.and_then(|n| n.workers),
                    max_workers: cap,
                    ready: 0,
                    held: 0,
                })
                .ready += 1;
        }
        for (id, node) in &self.nodes {
            if (node.in_progress || self.reserved.contains_key(id.as_str()))
                && let Some(demand) = groups.get_mut(&self.group(id))
            {
                demand.held += 1;
            }
        }
        groups.into_values().collect()
    }
}

/// A ticket's expected cores ([`admission::expected_cores`]) from the planner's figure, the
/// areas of `files`, the files it names, and the machine's `history`.
fn expected_cores(
    ticket: &Ticket,
    files: &[String],
    history: &[Worked],
    landed: &[String],
    now: Timestamp,
) -> f64 {
    admission::expected_cores(ticket.cores, &areas(files, landed), history, now)
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
            areas: areas(&work.map(|w| w.files).unwrap_or_default(), &[]),
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
