//! P1 tickets for work that cannot go on by itself:
//!
//! - checks still `incomplete` after their last attempt, one ticket per cause
//!   ([`incomplete`], in `stuck_incomplete.rs`);
//! - a cache audit whose rerun disagreed with the cache, which also switches off caching for
//!   that check;
//! - a tip that has waited an hour while a machine has not reported, or for checks that only
//!   machines of another operating system can run. These hold `master` back, so their tickets
//!   are filed released;
//! - an epic with open tickets of which none is ready or in progress, nor waits (through any
//!   chain of dependencies and epics) on a ticket that is. A ready ticket that no machine can
//!   take (it has an unknown pin or is labeled `needs:person`)
//!   does not count as ready. An epic with a ticket still `pending_approval` is
//!   waiting for a person, not stuck;
//! - a loop of tickets that wait on one another and are under no epic;
//! - a ticket held in progress too long, which is reported for the justiciar to look at
//!   ([`held`], in `stuck_held.rs`).

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use ignite_core::fleet::MachineState;
use ignite_core::ids::{CheckName, TicketId};
use ignite_core::pins::machine_label;
use ignite_core::protocol::hub::{AuditReport, CheckState, TipWaiting};
use ignite_core::protocol::tickets::{
    Graph, GraphTicket, Status, Ticket, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{checks, fleet, fleet_settings, tickets, tips};

use super::filing::{self, Author, Filed, Filing, UPKEEP};
use super::platform_wait::PlatformWait;
use super::regressions::OUTPUT_LINES;
use super::signature::Signature;
use super::{parse_time, tail};

#[path = "stuck_held.rs"]
mod held;
#[path = "stuck_incomplete.rs"]
mod incomplete;

pub use held::{ACTIVE_HELD_SECONDS, HELD_REFILE_COOLDOWN_SECONDS, HELD_SECONDS, STOPPED_SECONDS};
use held::{find_held, held_filing, held_recently_closed};

pub use incomplete::{ROUND_LIMIT, incomplete};

/// How long a tip waits for a machine that has not reported before a ticket names it.
pub const TIP_WAIT_SECONDS: f64 = 3_600.0;

/// How long a tip may wait on another platform while its machines are still storing results
/// before a ticket escalates the slow progress.
pub const TIP_SLOW_WAIT_SECONDS: f64 = 2.0 * TIP_WAIT_SECONDS;

/// Switches off caching for the audited check and files the ticket naming both runs.
pub fn audit(
    conn: &mut Connection,
    author: Author,
    report: &AuditReport,
    now: Timestamp,
) -> Result<Filed> {
    let check = report.key.check.as_str();
    let platform = report.key.platform.map_or("any platform", |os| os.as_str());
    let body = format!(
        "A rerun of a cache hit disagreed with the cache, so caching is switched off for \
         {check}. The cause is an input the check reads but does not declare, or a check that \
         is not deterministic.\n\n\
         - cached: {:?} from record {} on machine {}\n\
         - rerun:  {:?} in record {} on machine {}, at commit {}\n\
         input hash {} on {platform}\n\n\
         rerun output (last {OUTPUT_LINES} lines):\n{}",
        report.cached,
        report.cached_record.0,
        report.cached_machine,
        report.rerun,
        report.rerun_record.0,
        report.rerun_machine,
        report.commit,
        report.key.hash,
        tail(&report.output, OUTPUT_LINES)
    );
    let filing = Filing {
        signature: Signature::of_subject(check, "audit"),
        evidence: format!("{}#{}", report.key.hash, report.rerun_record.0),
        fields: TicketFields {
            title: Some(format!("Cache audit disagreed: {check}")),
            body: Some(body),
            ticket_type: Some(TicketType::Bug),
            priority: Some(1),
            labels: Some(vec![
                "cache-audit".to_owned(),
                format!("check:{}", report.key.check.family()),
            ]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    };
    // The ticket comes first, so a failure to file never leaves caching off with nobody told.
    let filed = filing::file(conn, author, &filing, now)?;
    checks::set_state(conn, check, CheckState::Uncached)?;
    Ok(filed)
}

/// Files one ticket for each machine that has not reported since a tip that has now waited an
/// hour was first seen. A machine stays on one ticket for as long as it stays silent.
pub fn tips_waiting(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let cutoff = now.plus_seconds(-TIP_WAIT_SECONDS).to_string();
    let oldest: Option<(String, String)> = conn
        .query_row(
            "SELECT commit_sha, seen FROM tips
             WHERE state IN ('seen', 'verifying') AND seen <= ?1
             ORDER BY seen LIMIT 1",
            params![cutoff],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    let Some((commit, seen)) = oldest else {
        return Ok(Vec::new());
    };
    let seen = parse_time(&seen)?;
    let mut filed = Vec::new();
    let settings = fleet_settings::settings(conn)?;
    for machine in fleet::machines(conn)? {
        if settings.machine(&machine.name).state == MachineState::Off {
            continue;
        }
        let last_seen = parse_time(&machine.last_seen)?;
        if last_seen >= seen {
            continue;
        }
        let body = format!(
            "Tip {commit} has waited {:.0} minutes for verification, and machine {} ({}) has \
             not reported since {last_seen}. Checks that only it can run hold `master` until \
             it returns.",
            now.seconds_since(seen) / 60.0,
            machine.name,
            machine.id
        );
        let filing = Filing {
            signature: Signature::of_subject(&machine.id, "unreachable"),
            evidence: machine.last_seen.clone(),
            fields: TicketFields {
                title: Some(format!(
                    "Machine {} has not reported for an hour",
                    machine.name
                )),
                body: Some(body),
                ticket_type: Some(TicketType::Bug),
                priority: Some(1),
                labels: Some(vec!["fleet".to_owned(), machine_label(&machine.name)]),
                ..TicketFields::default()
            },
            deps: Vec::new(),
            released: false,
        };
        filed.push(filing::file(
            conn,
            UPKEEP,
            &filing.holding_tip(&commit),
            now,
        )?);
    }
    Ok(filed)
}

/// How many of a waiting tip's checks its ticket lists by name.
const LISTED_CHECKS: usize = 20;

fn listed_checks(checks: &[CheckName]) -> String {
    let mut checks: Vec<&str> = checks.iter().map(CheckName::as_str).collect();
    let more = checks.len().saturating_sub(LISTED_CHECKS);
    checks.truncate(LISTED_CHECKS);
    let mut listed = checks.join("\n- ");
    if more > 0 {
        listed = format!("{listed}\n- and {more} more");
    }
    listed
}

/// Files one ticket for each machine running the platform a tip has waited an hour for, or one
/// saying no such machine is registered, or, when the report names families no machine of the
/// platform runs, one naming those families. A machine shares its ticket with [`tips_waiting`], so
/// a machine that is silent and needed has one ticket. Each of these tickets is filed released,
/// since the tip holds `master` back. Repairs for checks the tip waits on also become blockers
/// and are released if they still wait for approval ([`filing::release_holding`]).
pub fn tip_waiting(
    conn: &mut Connection,
    author: Author,
    report: &TipWaiting,
    now: Timestamp,
) -> Result<Vec<Filed>> {
    let settings = fleet_settings::settings(conn)?;
    if fleet_settings::off_machines(conn, &settings)?.contains_key(&report.platform) {
        return Ok(Vec::new());
    }
    let tip = report.commit.as_str();
    tips::seen(conn, tip, now)?;
    let os = report.platform.as_str();
    let listed = listed_checks(&report.checks);
    let waited = format!(
        "Tip {} has waited {:.0} minutes for checks that only a {os} machine can run, and \
         `master` stays where it is until they pass. Machine {} is waiting.",
        report.commit,
        report.waited_s / 60.0,
        report.machine
    );
    let labels = vec!["fleet".to_owned()];
    let machines: Vec<_> = fleet::machines(conn)?
        .into_iter()
        .filter(|machine| {
            machine.os == os && settings.machine(&machine.name).state != MachineState::Off
        })
        .collect();
    let scope = PlatformWait::from(report);
    let working = scope.working_machine(conn, machines.iter(), now)?;
    let slow = working.is_some() && report.waited_s >= TIP_SLOW_WAIT_SECONDS;
    let scope = scope.with_slow(slow);
    let filings: Vec<Filing> = if !report.unrunnable.is_empty() {
        vec![unrunnable_filing(report, &listed)]
    } else if working.is_some() && !slow {
        // Recent progress within the slow-wait bound is evidence that the round is still moving.
        // A silent machine can still be named by `tips_waiting`.
        Vec::new()
    } else if machines.is_empty() {
        vec![Filing {
            signature: Signature::of_subject(&format!("os:{os}"), "unreachable"),
            evidence: report.commit.to_string(),
            fields: TicketFields {
                title: Some(format!("No {os} machine is registered to run its checks")),
                body: Some(format!(
                    "{waited}\nNo {os} machine is registered with the hub. {}.\n\n\
                     checks:\n- {listed}",
                    scope.remaining_summary()
                )),
                ticket_type: Some(TicketType::Bug),
                priority: Some(1),
                labels: Some(labels),
                ..TicketFields::default()
            },
            deps: Vec::new(),
            released: false,
        }]
    } else {
        machines
            .iter()
            .map(|machine| scope.machine_filing(conn, report, machine, &waited, &listed, now))
            .collect::<Result<Vec<_>>>()?
    };
    let mut filed = Vec::with_capacity(filings.len());
    for filing in filings {
        filed.push(filing::file(
            conn,
            author,
            &filing.holding_platform_wait(&scope),
            now,
        )?);
    }
    let names: Vec<&str> = report.checks.iter().map(CheckName::as_str).collect();
    filing::release_holding(conn, &names, report.platform, tip, now)?;
    Ok(filed)
}

/// Reports families that no machine on the required platform is configured to run.
fn unrunnable_filing(report: &TipWaiting, listed: &str) -> Filing {
    let os = report.platform.as_str();
    // No machine of this system is declared to run these families, so the tip can never
    // verify, however many machines are working. Only a change to `ignite.toml` fixes it.
    let families = report.unrunnable.join(", ");
    Filing {
        signature: Signature::of_subject(&format!("os:{os}:{families}"), "unrunnable"),
        evidence: report.commit.to_string(),
        fields: TicketFields {
            title: Some(format!("No {os} machine runs the families {families}")),
            body: Some(format!(
                "Tip {} waits for checks of the families {families}, which must pass on \
                 {os}, but no `[machine.*]` with `os = \"{os}\"` in `ignite.toml` lists \
                 them in its `families`. `master` cannot move until a {os} machine runs \
                 them: add each family to such a machine's `families`, or drop {os} from \
                 the family's `platforms`. Machine {} is waiting.\n\nchecks:\n- {listed}",
                report.commit, report.machine
            )),
            ticket_type: Some(TicketType::Bug),
            priority: Some(1),
            labels: Some(vec!["fleet".to_owned()]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    }
}

/// Files one ticket for each epic whose open tickets are all blocked, one for each loop of
/// tickets under no epic that wait on one another, and one for each ticket held in progress too
/// long (see [`held`]). Of nested stuck epics, only the innermost is reported, since the outer
/// one waits on it.
pub fn stuck_epics(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let graph = tickets::graph(conn, false, now)?;
    let stranded = super::machines::unassignable(conn)?.into_iter().collect();
    let mut filings: Vec<Filing> = find_stuck(&graph, now, &stranded)
        .into_iter()
        .map(|(epic, lines)| epic_filing(&epic, &lines))
        .collect();
    filings.extend(find_cycles(&graph).iter().map(|cycle| cycle_filing(cycle)));
    for held in find_held(conn, &graph, now)? {
        if !held_recently_closed(conn, &held.ticket, now)? {
            filings.push(held_filing(&held));
        }
    }
    let mut filed = Vec::with_capacity(filings.len());
    for filing in &filings {
        filed.push(filing::file(conn, UPKEEP, filing, now)?);
    }
    Ok(filed)
}

/// What the ticket graph shows stuck now, named as the subjects of the signatures
/// [`stuck_epics`] files under, so [`super::clearing`] can tell which of those faults cleared.
pub(super) struct StuckNow {
    pub epics: BTreeSet<String>,
    pub cycles: BTreeSet<String>,
    pub held: BTreeSet<String>,
}

/// The epics, loops and held tickets [`stuck_epics`] would file for at `now`.
pub(super) fn stuck_now(conn: &Connection, now: Timestamp) -> Result<StuckNow> {
    let graph = tickets::graph(conn, false, now)?;
    let stranded = super::machines::unassignable(conn)?.into_iter().collect();
    Ok(StuckNow {
        epics: find_stuck(&graph, now, &stranded)
            .into_iter()
            .map(|(epic, _)| epic)
            .collect(),
        cycles: find_cycles(&graph)
            .iter()
            .map(|cycle| cycle.join(", "))
            .collect(),
        held: find_held(conn, &graph, now)?
            .into_iter()
            .map(|held| held.ticket)
            .collect(),
    })
}

/// What a waiting ticket requires that the fleet cannot provide.
fn unmet(ticket: &Ticket, stranded: &BTreeMap<String, String>) -> Vec<String> {
    let mut needs = Vec::new();
    if ticket.labels.iter().any(|label| label == "needs:person") {
        needs.push("a person (needs:person)".to_owned());
    }
    needs.extend(stranded.get(ticket.id.as_str()).cloned());
    needs
}

/// Each innermost stuck epic, with a line per waiting ticket saying what it waits on.
fn find_stuck(
    graph: &Graph,
    now: Timestamp,
    stranded: &BTreeMap<String, String>,
) -> Vec<(String, Vec<String>)> {
    let parents: BTreeMap<&str, Option<&str>> = graph
        .tickets
        .iter()
        .map(|t| {
            (
                t.ticket.id.as_str(),
                t.ticket.parent.as_ref().map(TicketId::as_str),
            )
        })
        .collect();
    let ancestors = |id: &str| {
        std::iter::successors(parents.get(id).copied().flatten(), |up| {
            parents.get(up).copied().flatten()
        })
    };
    // A ticket can move when it is ready or in progress. A ready ticket that no machine can
    // take, or that waits for a person, does not move.
    let mut moves: BTreeSet<&str> = BTreeSet::new();
    let mut queue: Vec<&str> = graph
        .tickets
        .iter()
        .filter(|t| {
            t.ticket.status == Status::InProgress
                || (t.ready && unmet(&t.ticket, stranded).is_empty())
        })
        .map(|t| t.ticket.id.as_str())
        .collect();
    // It also moves when it waits on a ticket that moves, since that ticket unblocks it in
    // time, and an epic (or any parent) moves when a ticket somewhere under it moves. So a phase
    // epic that waits on an earlier milestone epic, which in turn waits on one whose children
    // are being worked on, is not stuck.
    let mut waiters: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for entry in &graph.tickets {
        for blocker in &entry.blocked_by {
            waiters
                .entry(blocker.id.as_str())
                .or_default()
                .push(entry.ticket.id.as_str());
        }
    }
    while let Some(id) = queue.pop() {
        if !moves.insert(id) {
            continue;
        }
        queue.extend(ancestors(id));
        queue.extend(waiters.get(id).into_iter().flatten().copied());
    }
    // For each ancestor: its waiting tickets, and whether any of its tickets can move.
    let mut waiting: BTreeMap<&str, Vec<&GraphTicket>> = BTreeMap::new();
    let mut moving: BTreeSet<&str> = BTreeSet::new();
    // Epics with a ticket that waits for a person to release it. That is not stuck: the
    // release is the next step, so nothing is reported until it happens.
    let mut awaiting_approval: BTreeSet<&str> = BTreeSet::new();
    for entry in &graph.tickets {
        let ticket = &entry.ticket;
        if ticket.ticket_type == TicketType::Epic {
            continue;
        }
        if ticket.status == Status::PendingApproval {
            awaiting_approval.extend(ancestors(ticket.id.as_str()));
        }
        let deferred =
            ticket.status == Status::Deferred || ticket.defer_until.is_some_and(|t| t > now);
        let moves = moves.contains(ticket.id.as_str());
        for id in ancestors(ticket.id.as_str()) {
            if moves {
                moving.insert(id);
            } else if !deferred {
                waiting.entry(id).or_default().push(entry);
            }
        }
    }
    let stuck: BTreeSet<&str> = graph
        .tickets
        .iter()
        .filter(|t| t.ticket.ticket_type == TicketType::Epic)
        .map(|t| t.ticket.id.as_str())
        .filter(|epic| {
            waiting.contains_key(epic)
                && !moving.contains(epic)
                && !awaiting_approval.contains(epic)
        })
        .collect();
    stuck
        .iter()
        .filter(|epic| {
            !stuck
                .iter()
                .any(|other| ancestors(other).any(|up| up == **epic))
        })
        .map(|epic| {
            let lines = waiting[epic]
                .iter()
                .map(|entry| waiting_line(entry, stranded))
                .collect();
            ((*epic).to_owned(), lines)
        })
        .collect()
}

/// One line saying what a waiting ticket waits on.
fn waiting_line(entry: &GraphTicket, stranded: &BTreeMap<String, String>) -> String {
    let mut by: Vec<String> = entry.blocked_by.iter().map(|b| b.id.to_string()).collect();
    by.extend(unmet(&entry.ticket, stranded));
    let by = if by.is_empty() {
        "nothing listed".to_owned()
    } else {
        by.join(", ")
    };
    format!("- {} waits on {by}", entry.ticket.id)
}

fn epic_filing(epic: &str, lines: &[String]) -> Filing {
    Filing {
        signature: Signature::of_subject(epic, "stuck-epic"),
        evidence: "stuck".to_owned(),
        fields: TicketFields {
            title: Some(format!(
                "Repair the graph of {epic}: nothing in it can start"
            )),
            body: Some(format!(
                "Epic {epic} has open tickets, but none is ready or in progress, so no worker \
                 will take any of them. Remove the edges that need not hold, or close or split \
                 the tickets they wait on.\n\n{}",
                lines.join("\n")
            )),
            ticket_type: Some(TicketType::Task),
            priority: Some(1),
            labels: Some(vec!["graph-repair".to_owned()]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    }
}

/// Each loop of live tickets that wait on one another, among tickets none of which has an epic
/// above it (an epic's own graph is reported with the epic). A ticket waits on what the
/// readiness rules say holds it back, so a loop can run through `blocks`, `conditional-blocks`
/// or `waits-for` edges, through a parent's dependencies and through a parent's live children.
/// Each loop is its tickets in order of ID.
fn find_cycles(graph: &Graph) -> Vec<Vec<String>> {
    let parents: BTreeMap<&str, Option<&str>> = graph
        .tickets
        .iter()
        .map(|t| {
            (
                t.ticket.id.as_str(),
                t.ticket.parent.as_ref().map(TicketId::as_str),
            )
        })
        .collect();
    let types: BTreeMap<&str, TicketType> = graph
        .tickets
        .iter()
        .map(|t| (t.ticket.id.as_str(), t.ticket.ticket_type))
        .collect();
    let under_epic = |id: &str| {
        std::iter::successors(parents.get(id).copied().flatten(), |up| {
            parents.get(up).copied().flatten()
        })
        .any(|up| types.get(up) == Some(&TicketType::Epic))
    };
    // A ticket waits on each of its blockers but itself (rule 1, a ticket that is not open):
    // its own dependencies, its ancestors' dependencies, the siblings a `last` ticket waits
    // for and its live children. So a loop that runs through a parent link is a loop here.
    let mut forward: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut backward: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for entry in &graph.tickets {
        let src = entry.ticket.id.as_str();
        for blocker in &entry.blocked_by {
            let dst = blocker.id.as_str();
            if dst != src && parents.contains_key(dst) {
                forward.entry(src).or_default().push(dst);
                backward.entry(dst).or_default().push(src);
            }
        }
    }
    // Kosaraju's algorithm, with explicit stacks so that a long chain cannot overflow.
    let mut order: Vec<&str> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for &start in forward.keys() {
        if !seen.insert(start) {
            continue;
        }
        let mut stack = vec![(start, 0_usize)];
        while let Some((node, next)) = stack.pop() {
            let nexts = forward.get(node).map_or(&[][..], Vec::as_slice);
            if let Some(&child) = nexts.get(next) {
                stack.push((node, next + 1));
                if seen.insert(child) {
                    stack.push((child, 0));
                }
            } else {
                order.push(node);
            }
        }
    }
    let mut assigned: BTreeSet<&str> = BTreeSet::new();
    let mut loops: Vec<Vec<String>> = Vec::new();
    for &root in order.iter().rev() {
        if !assigned.insert(root) {
            continue;
        }
        let mut component = vec![root];
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            for &before in backward.get(node).into_iter().flatten() {
                if assigned.insert(before) {
                    component.push(before);
                    stack.push(before);
                }
            }
        }
        let alone =
            component.len() == 1 && !forward.get(root).is_some_and(|nexts| nexts.contains(&root));
        if !alone && !component.iter().any(|id| under_epic(id)) {
            let mut ids: Vec<String> = component.into_iter().map(str::to_owned).collect();
            ids.sort();
            loops.push(ids);
        }
    }
    loops.sort();
    loops
}

fn cycle_filing(cycle: &[String]) -> Filing {
    let names = cycle.join(", ");
    Filing {
        signature: Signature::of_subject(&names, "stuck-cycle"),
        evidence: "cycle".to_owned(),
        fields: TicketFields {
            title: Some(format!("Repair the graph: {names} wait on one another")),
            body: Some(format!(
                "These tickets are under no epic and wait on one another in a loop, so none of \
                 them can ever become ready: {names}. Remove an edge that need not hold, or \
                 close or split a ticket in the loop."
            )),
            ticket_type: Some(TicketType::Task),
            priority: Some(1),
            labels: Some(vec!["graph-repair".to_owned()]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    }
}
