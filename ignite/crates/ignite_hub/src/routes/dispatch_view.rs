//! The queue as the dashboard's `available` page shows it: the tickets workers hold or have
//! reserved, then every ready ticket in the order `POST /next` would try them, each with why it
//! is not running yet on each machine, then the ready tickets the queue never offers and why.
//!
//! The page answers with what [`super::next`] would do, built from the same snapshot, shares and
//! [`dispatch::order`]. What a machine has free comes from its latest `POST /next`
//! ([`machine_asks`]), brought up to date where the hub knows better: only the workers its last
//! heartbeat still reports idle count as idle, and its room for expected cores is worked out
//! again from the tickets it holds now. The page then hands the tickets out in order, as
//! successive asks would, so a ticket behind others that fill a machine says so.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::dispatch::{self, ReadyTicket, Stage, TicketKind};
use ignite_core::dispatch_pressure::{BACKLOG_SECONDS, Budget};
use ignite_core::fleet::{FleetSettings, MachineState};
use ignite_core::ids::{MachineId, TicketId};
use ignite_core::mix::ProviderLoad;
use ignite_core::plan_usage::Hold;
use ignite_core::protocol::hub::MachineFree;
use ignite_core::protocol::tickets::{Status, TicketType};
use ignite_core::shares::{self, Group};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, fleet_settings, machine_asks, tickets};

use super::super::{mix, usage};
use super::held::{self, Paused};
use super::{NEEDS_PERSON, Snapshot, capacity, history, improvements_due, landed_files};

/// A ticket a worker holds, or that is reserved for one.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub id: TicketId,
    /// The name of the machine it is reserved for, while nobody has claimed it yet.
    pub reserved_for: Option<String>,
}

/// A ready ticket the queue offers, in its place in the order.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    pub id: TicketId,
    pub stage: Stage,
    /// The epic it shares workers in, `None` for tickets under no epic.
    pub group: Group,
    /// The machine the next ask would hand it to, if any.
    pub next_on: Option<String>,
    /// One line: where it goes next, or the reason that applies on most machines.
    pub summary: String,
    /// Each reporting machine's name and what it says about the ticket.
    pub machines: Vec<(String, String)>,
}

/// A ready ticket the queue does not offer to any machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub id: TicketId,
    pub reason: String,
}

/// What one machine has free, as the page counts it.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineReport {
    pub name: String,
    pub summary: String,
    /// When it last asked for work, and the hub's answer.
    pub asked: Option<(Timestamp, String)>,
}

/// The whole page's data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Available {
    pub running: Vec<Running>,
    pub waiting: Vec<Waiting>,
    pub held: Vec<Held>,
    pub machines: Vec<MachineReport>,
}

/// A machine taking part in the hand-outs, with what it has free as the hand-outs go on.
struct Seat<'a> {
    name: String,
    /// What it had free before the page handed it anything.
    start: MachineFree,
    free: MachineFree,
    ready: HashMap<TicketId, ReadyTicket>,
    start_disk: Option<Budget<'a>>,
    disk: Option<Budget<'a>>,
    /// How many tickets the page has handed it.
    taken: u32,
    /// Live room reservations before projecting any of the ready tickets.
    pending: u32,
    providers: Vec<ProviderLoad>,
    stopped: bool,
    /// What to say in place of [`dispatch::MASTER_ONLY`]: why this machine asked only for
    /// tickets that hold `master` back.
    master_reason: String,
    /// A checks-only machine, which takes a ticket that holds `master` back only when no
    /// active machine can; `ready` then holds only the tickets it may take.
    checks_only: bool,
    settings: &'a FleetSettings,
    /// Each provider's usage holds across the fleet.
    holds: &'a BTreeMap<String, Vec<Hold>>,
}

/// Builds the page's data at `now`.
pub fn available(conn: &Connection, now: Timestamp) -> Result<Available> {
    let snapshot = Snapshot::load(conn, now)?;
    let landed = landed_files(conn)?;
    let settings = fleet_settings::settings(conn)?;
    let holds = usage::holds(conn, now)?;
    let mut page = Available {
        running: running(conn)?,
        ..Available::default()
    };
    let reserved_sessions: HashSet<String> = snapshot.reserved.values().cloned().collect();
    let asks = asks(conn, &snapshot, &reserved_sessions)?;
    let mut seats = Vec::new();
    let mut silent = Vec::new();
    let mut improvements = None;
    for (name, id, ask) in &asks {
        let Some((asked, free)) = ask else {
            let said = silent_reason(conn, &settings, name, id)?;
            page.machines.push(MachineReport {
                name: name.clone(),
                summary: said.clone(),
                asked: None,
            });
            silent.push((name.clone(), said));
            continue;
        };
        let pressure = free.pressure.as_ref();
        let queue_seconds = pressure.map_or(0, |p| p.queue_seconds);
        let worked = history(conn, id)?;
        let mut free = free.clone();
        let held_now = capacity::read(conn, id, &worked, &landed, now, pressure)?;
        free.free_cores = dispatch::room(&free, &held_now.cores);
        let state = settings.machine(name).state;
        let paused = held::paused(conn, &free, state)?;
        let holds_one = paused == Paused::MasterOnly && held::holds_one(conn, id)?;
        if paused == Paused::Everything || holds_one {
            free.idle_workers.clear();
        }
        let mut master_reason = master_reason(paused, state, queue_seconds);
        if holds_one {
            master_reason += ", and it already works one, so its idle workers wait";
        }
        free.master_only |= paused == Paused::MasterOnly;
        if improvements.is_none() {
            improvements = improvements_due(conn, id)?;
        }
        let pending = free
            .room
            .as_ref()
            .map_or(Ok(0), |_| mix::pending(conn, id, now))?;
        page.machines.push(MachineReport {
            name: name.clone(),
            summary: if asked.is_some() {
                machine_summary(&free, &master_reason, held_now.cores.len(), pending)
            } else {
                format!(
                    "every worker is busy · {} held · has not asked for work, so its free trees and \
                     cores are not known",
                    held_now.cores.len()
                )
            },
            asked: asked.clone(),
        });
        let checks_only = state == MachineState::ChecksOnly;
        let fallback = checks_only
            .then(|| held::Fallback::load(conn, &settings, &snapshot.heard, &free, now))
            .transpose()?;
        let ready = snapshot.ready_tickets(conn, &worked, &landed, now, fallback.as_ref())?;
        let providers = free
            .room
            .as_ref()
            .map(|room| mix::loads(conn, &settings, &holds, room, now))
            .transpose()?
            .unwrap_or_default();
        seats.push(Seat {
            name: name.clone(),
            start: free.clone(),
            free,
            ready: ready.into_iter().map(|t| (t.id.clone(), t)).collect(),
            start_disk: held_now.disk.clone(),
            disk: held_now.disk,
            taken: 0,
            pending,
            providers,
            stopped: paused == Paused::Everything || holds_one,
            master_reason,
            checks_only,
            settings: &settings,
            holds: &holds,
        });
    }
    let (waiting, capped) = waiting(
        conn,
        &snapshot,
        &landed,
        now,
        (&mut seats, &silent),
        improvements,
    )?;
    page.waiting = waiting;
    page.held = held_back(conn, &snapshot, &capped, now)?;
    Ok(page)
}

/// Every offered ready ticket in the order `POST /next` tries them, handed out to `seats` as
/// successive asks would hand them, and the ready tickets left out because their epic is at its
/// `--max-workers` cap.
fn waiting(
    conn: &Connection,
    snapshot: &Snapshot,
    landed: &[String],
    now: Timestamp,
    (seats, silent): (&mut [Seat], &[(String, String)]),
    improvements: Option<shares::Unit>,
) -> Result<(Vec<Waiting>, Vec<TicketId>)> {
    // The order does not depend on the machine: expected cores, the only part that does, plays
    // no part in it.
    let mut ready = snapshot.ready_tickets(conn, &[], landed, now, None)?;
    let demand = snapshot.demand(&ready);
    let full: HashSet<&Group> = demand
        .iter()
        .filter(|group| group.max_workers.is_some_and(|cap| group.held >= cap))
        .map(|group| &group.epic)
        .collect();
    let mut capped = Vec::new();
    ready.retain(|ticket| {
        let keep = ticket.kind == TicketKind::Regression || !full.contains(&ticket.epic);
        if !keep {
            capped.push(ticket.id.clone());
        }
        keep
    });
    let epic_order = shares::epic_order(&demand);
    let mut waiting = Vec::new();
    for (ticket, stage) in dispatch::order(&ready, &epic_order, improvements) {
        waiting.push(hand_out(seats, silent, ticket, stage, now));
    }
    let listed: HashSet<&TicketId> = waiting.iter().map(|w| &w.id).collect();
    for ticket in &ready {
        if !listed.contains(&ticket.id) && !capped.contains(&ticket.id) {
            capped.push(ticket.id.clone());
        }
    }
    Ok((waiting, capped))
}

/// Hands `ticket` to the first machine that can take it, and says what every machine makes of
/// it, including the `silent` machines, which report but have not said what they have free,
/// each with what is known about it instead.
fn hand_out(
    seats: &mut [Seat],
    silent: &[(String, String)],
    ticket: &ReadyTicket,
    stage: Stage,
    now: Timestamp,
) -> Waiting {
    let mut next_on: Option<String> = None;
    let mut machines = Vec::new();
    for seat in seats.iter_mut() {
        let mine = seat.ready.get(&ticket.id).unwrap_or(ticket).clone();
        let mine = &mine;
        let deferred = seat.checks_only && !seat.ready.contains_key(&ticket.id);
        let reason = if deferred && held::holds_master(ticket) {
            Some("checks-only machine, and an active machine can take it".to_owned())
        } else if deferred {
            Some(seat.master_reason.clone())
        } else {
            refusal(seat, false, mine)
        };
        let said = match (reason, &next_on) {
            (None, None) => {
                take(seat, mine, now);
                next_on = Some(seat.name.clone());
                "next in line here".to_owned()
            }
            (None, Some(first)) => format!("could go here, but goes to {first} first"),
            // A reason that held before the page handed this machine anything is the one to
            // show; otherwise the machine is only full of the tickets ahead.
            (Some(reason), _) => match refusal(seat, true, mine) {
                Some(first) => first,
                None if seat.taken > 0 => format!(
                    "behind {} earlier {} that fill this machine",
                    seat.taken,
                    if seat.taken == 1 { "ticket" } else { "tickets" }
                ),
                None => reason,
            },
        };
        machines.push((seat.name.clone(), said));
    }
    // The chip names a reason from a machine the ticket may go to, preferring one that said
    // what it has free: a silent machine's line says nothing about this ticket in particular,
    // and "pinned to machine" only points at another machine whose own line is the reason.
    let reported: Vec<(String, String)> = machines.clone();
    let first_silent = machines.len();
    for (name, known) in silent {
        let said = ticket
            .pins
            .machine
            .as_deref()
            .filter(|_| !ticket.pins.admits_machine(name))
            .map_or_else(|| known.clone(), |pin| format!("pinned to machine {pin}"));
        machines.push((name.clone(), said));
    }
    let unreported = machines[first_silent..].to_vec();
    machines.sort_by(|a, b| a.0.cmp(&b.0));
    let summary = next_on.as_ref().map_or_else(
        || {
            summary(&reported)
                .or_else(|| summary(&unreported))
                .or_else(|| most_common(machines.iter().map(|(_, reason)| reason.as_str())))
                .unwrap_or_default()
        },
        |machine| format!("Next on {machine}"),
    );
    Waiting {
        id: ticket.id.clone(),
        stage,
        group: ticket.epic.clone(),
        next_on,
        summary,
        machines,
    }
}

/// Why the machine cannot take the ticket, from dispatch's own tests and the disk budget.
fn refusal(seat: &Seat, start: bool, ticket: &ReadyTicket) -> Option<String> {
    let free = if start { &seat.start } else { &seat.free };
    let disk = if start { &seat.start_disk } else { &seat.disk };
    let blocker = if let Some(room) = &free.room {
        let pending = seat.pending + if start { 0 } else { seat.taken };
        dispatch::blocker_with(free, ticket, || {
            if seat.stopped {
                return Some(seat.master_reason.clone());
            }
            mix::choose(&seat.providers, room, pending, ticket).err()
        })
    } else {
        dispatch::blocker(free, ticket).or_else(|| legacy_refusal(seat, free, ticket))
    };
    blocker
        .map(|reason| {
            if reason == dispatch::MASTER_ONLY {
                seat.master_reason.clone()
            } else {
                reason
            }
        })
        .or_else(|| {
            disk.as_ref()
                .and_then(|budget| budget.blocked(ticket))
                .map(|wait| {
                    let prefix = format!("{} ", ticket.id);
                    wait.reason
                        .strip_prefix(&prefix)
                        .unwrap_or(&wait.reason)
                        .to_owned()
                })
        })
}

/// A provider blocker shared by the suited idle workers, or `None` if one is available.
fn legacy_refusal(seat: &Seat, free: &MachineFree, ticket: &ReadyTicket) -> Option<String> {
    let mut reason = None;
    for worker in free
        .idle_workers
        .iter()
        .filter(|w| dispatch::suits(free, w, ticket))
    {
        let blocked = usage::legacy_blocker(seat.settings, seat.holds, worker, ticket)?;
        reason.get_or_insert(blocked);
    }
    reason
}

/// Takes from the seat what handing it the ticket uses: the worker that would get it, a ticket
/// tree and, unless it is a regression, its expected cores.
fn take(seat: &mut Seat, ticket: &ReadyTicket, now: Timestamp) {
    let free = &seat.free;
    let chosen = free.idle_workers.iter().position(|w| {
        dispatch::suits(free, w, ticket)
            && usage::legacy_blocker(seat.settings, seat.holds, w, ticket).is_none()
    });
    if let Some(index) = chosen {
        seat.free.idle_workers.remove(index);
    }
    seat.free.free_ticket_trees = seat.free.free_ticket_trees.saturating_sub(1);
    if ticket.kind != TicketKind::Regression {
        seat.free.free_cores -= ticket.expected_cores;
    }
    if let Some(budget) = &mut seat.disk {
        budget.hold(&ticket.id, ticket.expected_cores, &ticket.files, Some(now));
    }
    seat.taken += 1;
}

/// The reason to show on a ticket no machine takes next: the one most of `machines` give,
/// leaving out machines its machine pin excludes. `None` when every machine is excluded.
fn summary(machines: &[(String, String)]) -> Option<String> {
    most_common(
        machines
            .iter()
            .map(|(_, reason)| reason.as_str())
            .filter(|reason| !reason.starts_with("pinned to machine ")),
    )
}

/// The reason given most often, capitalized; the first given wins a tie.
fn most_common<'a>(reasons: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for reason in reasons {
        match counts.iter_mut().find(|(seen, _)| *seen == reason) {
            Some((_, n)) => *n += 1,
            None => counts.push((reason, 1)),
        }
    }
    let best = counts.iter().map(|(_, n)| *n).max()?;
    counts
        .into_iter()
        .find(|(_, n)| *n == best)
        .map(|(reason, _)| capitalized(reason))
}

/// What is known about a machine that reports but has not asked for work since the hub began
/// recording asks: that it is off, or has no workers to ask for, before the bare fact.
fn silent_reason(
    conn: &Connection,
    settings: &ignite_core::fleet::FleetSettings,
    name: &str,
    id: &str,
) -> Result<String> {
    let state = settings.machine(name).state;
    let workers = fleet::workers(conn, id)?
        .iter()
        .filter(|w| w.role == "worker")
        .count();
    Ok(match (state, workers) {
        (MachineState::Off, _) => "this machine is off".to_owned(),
        (MachineState::ChecksOnly, 0) => "checks-only machine with no workers".to_owned(),
        (_, 0) => "has no workers".to_owned(),
        _ => "has not asked for work since the hub began recording asks".to_owned(),
    })
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Why a machine asks only for tickets that hold `master` back, as its tickets show it; empty
/// when it asks for every ticket. `state` is the machine's state in the fleet settings. The
/// daemon also sets `master_only` itself: for its own pause, for a build queue whose commands
/// have waited [`BACKLOG_SECONDS`] for cores and for a check failing in every tree. The request
/// says the queue's wait but neither of the others.
fn master_reason(paused: Paused, state: MachineState, queue_seconds: u32) -> String {
    match paused {
        Paused::No => String::new(),
        Paused::Everything if state == MachineState::Off => "this machine is off".to_owned(),
        Paused::Everything => "hand-offs are paused on every machine".to_owned(),
        Paused::MasterOnly if state == MachineState::ChecksOnly => {
            "checks-only machine: only tickets that hold master back start here".to_owned()
        }
        Paused::MasterOnly if queue_seconds >= BACKLOG_SECONDS => format!(
            "commands have waited {queue_seconds} s for cores, so only tickets that hold \
             master back start"
        ),
        Paused::MasterOnly => "only tickets that hold master back start here (a pause or a \
                               check failing in every tree)"
            .to_owned(),
    }
}

fn machine_summary(free: &MachineFree, master_reason: &str, held: usize, pending: u32) -> String {
    let pause = if master_reason.is_empty() {
        String::new()
    } else {
        format!("{master_reason} · ")
    };
    let cores = free.admit_cores.map_or_else(
        || format!("{:.1} cores free", free.free_cores.max(0.0)),
        |admit| {
            format!(
                "{:.1} of {admit:.0} admitted cores free",
                free.free_cores.max(0.0)
            )
        },
    );
    let workers = free.room.as_ref().map_or_else(
        || {
            let idle = free.idle_workers.len();
            let noun = if idle == 1 { "worker" } else { "workers" };
            format!("{idle} idle {noun}")
        },
        |room| {
            if room.memory_pressed {
                return "memory pressure".to_owned();
            }
            let free = room.start.saturating_sub(pending);
            format!("room to start {free}")
        },
    );
    format!(
        "{pause}{workers} · {held} held · {} free {} · {cores}",
        free.free_ticket_trees,
        if free.free_ticket_trees == 1 {
            "tree"
        } else {
            "trees"
        },
    )
}

/// A machine's latest ask as the page counts it: when it asked and the hub's answer, if it has
/// asked, and what it has free.
type Ask = (Option<(Timestamp, String)>, MachineFree);

/// Every reporting machine, by name, with its ID and its latest ask, keeping as idle only the
/// workers its heartbeat still reports idle and holding no reservation. A machine whose workers
/// are all busy never asks, so one that has not asked since the hub began recording asks counts
/// as having no idle worker when its heartbeat shows none; one that shows idle workers but has
/// not asked gets no ask.
fn asks(
    conn: &Connection,
    snapshot: &Snapshot,
    reserved_sessions: &HashSet<String>,
) -> Result<Vec<(String, String, Option<Ask>)>> {
    let mut machines = fleet::machines(conn)?;
    machines.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = Vec::new();
    for machine in machines {
        if !snapshot.heard.contains(&machine.name.to_ascii_lowercase()) {
            continue;
        }
        let workers: Vec<_> = fleet::workers(conn, &machine.id)?
            .into_iter()
            .filter(|w| w.role == "worker")
            .collect();
        let idle: HashSet<String> = workers
            .iter()
            .filter(|w| w.state == "idle" && w.ticket.is_none())
            .map(|w| w.session.clone())
            .filter(|session| !reserved_sessions.contains(session))
            .collect();
        let ask = match machine_asks::latest(conn, &machine.id)? {
            Some(ask) => {
                let mut free = ask.free;
                free.idle_workers
                    .retain(|w| idle.contains(w.session.as_str()));
                Some((Some((ask.at, ask.answer)), free))
            }
            None if idle.is_empty() && !workers.is_empty() => {
                MachineId::new(machine.id.as_str()).ok().map(|id| {
                    let free = MachineFree {
                        machine: id,
                        os: if machine.os == "windows" {
                            Os::Windows
                        } else {
                            Os::Macos
                        },
                        idle_workers: Vec::new(),
                        free_ticket_trees: 0,
                        free_cores: 0.0,
                        admit_cores: None,
                        pressure: None,
                        families: Vec::new(),
                        name: None,
                        justiciar_only: false,
                        master_only: false,
                        explain: false,
                        room: None,
                    };
                    (None, free)
                })
            }
            None => None,
        };
        let ask = ask.map(|(asked, mut free)| {
            if free.name.is_none() {
                free.name = Some(machine.name.clone());
            }
            (asked, free)
        });
        out.push((machine.name, machine.id, ask));
    }
    Ok(out)
}

/// The tickets in progress and the reserved ones, the oldest claim first, then the reservations.
fn running(conn: &Connection) -> Result<Vec<Running>> {
    let mut statement = conn.prepare(
        "SELECT t.id, m.name FROM tickets t
           LEFT JOIN reservations r ON r.ticket = t.id
           LEFT JOIN machines m ON m.id = r.machine
         WHERE (t.status = 'in_progress' OR r.ticket IS NOT NULL)
           AND t.type NOT IN ('epic', 'milestone')
           AND NOT EXISTS (SELECT 1 FROM tickets c WHERE c.parent = t.id)
         ORDER BY r.ticket IS NOT NULL, t.claimed_at, t.id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut running = Vec::new();
    for row in rows {
        let (id, reserved_for) = row?;
        if let Ok(id) = id.parse() {
            running.push(Running { id, reserved_for });
        }
    }
    Ok(running)
}

/// The ready tickets nobody holds that the queue does not offer, with the reason: `capped` are
/// in epics at their `--max-workers` cap; the rest are set aside for a person, under an epic
/// with no allocation, or released by two sessions.
fn held_back(
    conn: &Connection,
    snapshot: &Snapshot,
    capped: &[TicketId],
    now: Timestamp,
) -> Result<Vec<Held>> {
    let offerable: HashSet<&TicketId> = snapshot.ready.iter().collect();
    let mut held = Vec::new();
    for id in tickets::ready(conn, now)? {
        if snapshot.reserved.contains_key(id.as_str()) {
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
        let reason = if capped.contains(&id) {
            "its epic is at its --max-workers cap"
        } else if !offerable.contains(&id) {
            "released by two sessions, so it waits for a justiciar"
        } else if ticket.labels.iter().any(|label| label == NEEDS_PERSON) {
            "set aside for a person (needs:person)"
        } else if snapshot.offered(ticket).is_none() {
            "its epic has no allocation, so the pool is never offered it"
        } else {
            continue;
        };
        held.push(Held {
            id,
            reason: reason.to_owned(),
        });
    }
    Ok(held)
}
