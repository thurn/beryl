//! P1 tickets for work that cannot go on by itself:
//!
//! - a check still `incomplete` after its last attempt, or one that cannot run on a machine;
//! - a cache audit whose rerun disagreed with the cache, which also switches off caching for
//!   that check;
//! - a tip that has waited an hour while a machine has not reported, or for checks that only
//!   machines of another operating system can run;
//! - an epic with open tickets of which none is ready or in progress, nor waits (through any
//!   chain of dependencies and epics) on a ticket that is. A ready ticket that no machine can
//!   take (its `needs:` label names no registered machine or operating system, or is
//!   `needs:person`) does not count as ready;
//! - a loop of tickets that wait on one another and are under no epic;
//! - a ticket held in progress for more than six hours, which is reported for the justiciar to
//!   look at.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use ignite_core::ids::{CheckName, TicketId};
use ignite_core::protocol::hub::{
    AuditReport, CheckState, IncompleteReason, IncompleteReport, TipWaiting,
};
use ignite_core::protocol::tickets::{
    Graph, GraphTicket, Status, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{checks, fleet, tickets};

use super::filing::{self, Author, Filed, Filing, UPKEEP};
use super::regressions::OUTPUT_LINES;
use super::signature::Signature;
use super::{parse_time, tail};

/// How long a tip waits for a machine that has not reported before a ticket names it.
pub const TIP_WAIT_SECONDS: f64 = 3_600.0;

/// Files the ticket for a check that stayed incomplete; the ticket names the machine.
pub fn incomplete(
    conn: &mut Connection,
    author: Author,
    report: &IncompleteReport,
    now: Timestamp,
) -> Result<Filed> {
    let check = report.check.as_str();
    let (category, title, why) = match report.reason {
        IncompleteReason::Unreported => (
            "incomplete",
            format!("Check stays incomplete: {check}"),
            format!(
                "The command ended without reporting {check} in each of {} attempts.",
                report.attempts
            ),
        ),
        IncompleteReason::CannotRun => (
            "cannot-run",
            format!("Check cannot run on {}: {check}", report.host),
            format!(
                "The command said it cannot run on {}, so it is not run there again.",
                report.host
            ),
        ),
        IncompleteReason::Hung => (
            "hung",
            format!("Check hangs: {check}"),
            "The command ran past its hang limit and was ended with everything it started. \
             It is not run again, because a hung command hangs again; the output lists the \
             processes it left."
                .to_owned(),
        ),
    };
    let platform = report.platform.map_or("", |os| os.as_str());
    let mut labels = vec![
        "incomplete".to_owned(),
        format!("check:{}", report.check.family()),
    ];
    if let Some(os) = report.platform {
        labels.push(format!("needs:{}", os.as_str()));
    }
    let body = format!(
        "{why}\nmachine {} ({}), commit {}\n\
         A landing or a tip that needs the check waits for it.\n\n\
         output (last {OUTPUT_LINES} lines):\n{}",
        report.host,
        report.machine,
        report.commit,
        tail(&report.output, OUTPUT_LINES)
    );
    let filing = Filing {
        signature: Signature::of_output(check, category, platform, &report.output, &report.host),
        evidence: format!("{}@{}", report.commit, report.machine),
        fields: TicketFields {
            title: Some(title),
            body: Some(body),
            ticket_type: Some(TicketType::Bug),
            priority: Some(1),
            labels: Some(labels),
            ..TicketFields::default()
        },
        deps: Vec::new(),
    };
    filing::file(conn, author, &filing, now)
}

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
    for machine in fleet::machines(conn)? {
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
                labels: Some(vec!["fleet".to_owned()]),
                ..TicketFields::default()
            },
            deps: Vec::new(),
        };
        filed.push(filing::file(conn, UPKEEP, &filing, now)?);
    }
    Ok(filed)
}

/// How many of a waiting tip's checks its ticket lists by name.
const LISTED_CHECKS: usize = 20;

/// Files one ticket for each machine running the platform a tip has waited an hour for, or one
/// saying no such machine is registered. A machine shares its ticket with [`tips_waiting`], so
/// a machine that is silent and needed has one ticket.
pub fn tip_waiting(
    conn: &mut Connection,
    author: Author,
    report: &TipWaiting,
    now: Timestamp,
) -> Result<Vec<Filed>> {
    let os = report.platform.as_str();
    let mut checks: Vec<&str> = report.checks.iter().map(CheckName::as_str).collect();
    let more = checks.len().saturating_sub(LISTED_CHECKS);
    checks.truncate(LISTED_CHECKS);
    let mut listed = checks.join("\n- ");
    if more > 0 {
        listed = format!("{listed}\n- and {more} more");
    }
    let waited = format!(
        "Tip {} has waited {:.0} minutes for checks that only a {os} machine can run, and \
         `master` stays where it is until they pass. Machine {} is waiting.",
        report.commit,
        report.waited_s / 60.0,
        report.machine
    );
    let labels = vec!["fleet".to_owned(), format!("needs:{os}")];
    let machines: Vec<_> = fleet::machines(conn)?
        .into_iter()
        .filter(|machine| machine.os == os)
        .collect();
    let filings: Vec<Filing> = if machines.is_empty() {
        vec![Filing {
            signature: Signature::of_subject(&format!("os:{os}"), "unreachable"),
            evidence: report.commit.to_string(),
            fields: TicketFields {
                title: Some(format!("No {os} machine is registered to run its checks")),
                body: Some(format!(
                    "{waited}\nNo {os} machine is registered with the hub.\n\nchecks:\n- {listed}"
                )),
                ticket_type: Some(TicketType::Bug),
                priority: Some(1),
                labels: Some(labels),
                ..TicketFields::default()
            },
            deps: Vec::new(),
        }]
    } else {
        machines
            .iter()
            .map(|machine| Filing {
                signature: Signature::of_subject(&machine.id, "unreachable"),
                evidence: format!("{}@{}", report.commit, machine.last_seen),
                fields: TicketFields {
                    title: Some(format!(
                        "Tip verification has waited an hour on machine {}",
                        machine.name
                    )),
                    body: Some(format!(
                        "{waited}\nMachine {} ({}) runs {os} and last reported at {}.\n\n\
                         checks:\n- {listed}",
                        machine.name, machine.id, machine.last_seen
                    )),
                    ticket_type: Some(TicketType::Bug),
                    priority: Some(1),
                    labels: Some(labels.clone()),
                    ..TicketFields::default()
                },
                deps: Vec::new(),
            })
            .collect()
    };
    let mut filed = Vec::with_capacity(filings.len());
    for filing in &filings {
        filed.push(filing::file(conn, author, filing, now)?);
    }
    Ok(filed)
}

/// How long a ticket may stay in progress before a ticket reports it, in seconds.
pub const HELD_SECONDS: f64 = 6.0 * 3_600.0;

/// The label on a ticket for the justiciar to look at.
const JUSTICIAR: &str = ignite_core::dispatch::JUSTICIAR;

/// The names that a `needs:` label can ask for and some registered machine has: each machine's
/// operating system and name.
fn machine_names(conn: &Connection) -> Result<BTreeSet<String>> {
    let mut known = BTreeSet::new();
    for machine in fleet::machines(conn)? {
        known.insert(machine.os);
        known.insert(machine.name);
    }
    Ok(known)
}

/// Files one ticket for each epic whose open tickets are all blocked, one for each loop of
/// tickets under no epic that wait on one another, and one for each ticket held in progress for
/// more than six hours. Of nested stuck epics, only the innermost is reported, since the outer
/// one waits on it.
pub fn stuck_epics(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let graph = tickets::graph(conn, false, now)?;
    let known = machine_names(conn)?;
    let mut filings: Vec<Filing> = find_stuck(&graph, now, &known)
        .into_iter()
        .map(|(epic, lines)| epic_filing(&epic, &lines))
        .collect();
    filings.extend(find_cycles(&graph).iter().map(|cycle| cycle_filing(cycle)));
    filings.extend(find_held(&graph, now).iter().map(held_filing));
    let mut filed = Vec::with_capacity(filings.len());
    for filing in &filings {
        filed.push(filing::file(conn, UPKEEP, filing, now)?);
    }
    Ok(filed)
}

/// Whether some registered machine could take a ticket with these labels: every `needs:` label
/// names a known machine or operating system, and none is `needs:person`.
fn takeable(labels: &[String], known: &BTreeSet<String>) -> bool {
    labels
        .iter()
        .filter_map(|label| label.strip_prefix("needs:"))
        .all(|need| need != "person" && known.contains(need))
}

/// The `needs:` labels of a ticket that no registered machine meets.
fn unmet(labels: &[String], known: &BTreeSet<String>) -> Vec<String> {
    labels
        .iter()
        .filter(|label| {
            label
                .strip_prefix("needs:")
                .is_some_and(|need| need == "person" || !known.contains(need))
        })
        .cloned()
        .collect()
}

/// Each innermost stuck epic, with a line per waiting ticket saying what it waits on.
fn find_stuck(
    graph: &Graph,
    now: Timestamp,
    known: &BTreeSet<String>,
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
            t.ticket.status == Status::InProgress || (t.ready && takeable(&t.ticket.labels, known))
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
    for entry in &graph.tickets {
        let ticket = &entry.ticket;
        if ticket.ticket_type == TicketType::Epic {
            continue;
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
        .filter(|epic| waiting.contains_key(epic) && !moving.contains(epic))
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
                .map(|entry| {
                    let mut by: Vec<String> =
                        entry.blocked_by.iter().map(|b| b.id.to_string()).collect();
                    by.extend(unmet(&entry.ticket.labels, known).into_iter().map(|label| {
                        if label == "needs:person" {
                            "a person (needs:person)".to_owned()
                        } else {
                            format!("{label}, which no registered machine has")
                        }
                    }));
                    let by = if by.is_empty() {
                        "nothing listed".to_owned()
                    } else {
                        by.join(", ")
                    };
                    format!("- {} waits on {by}", entry.ticket.id)
                })
                .collect();
            ((*epic).to_owned(), lines)
        })
        .collect()
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
    }
}

/// A ticket in progress for more than [`HELD_SECONDS`].
struct Held {
    ticket: String,
    title: String,
    hours: u64,
    session: String,
    machine: String,
}

fn find_held(graph: &Graph, now: Timestamp) -> Vec<Held> {
    graph
        .tickets
        .iter()
        .filter(|entry| entry.ticket.status == Status::InProgress)
        .filter_map(|entry| {
            let ticket = &entry.ticket;
            let held = now.seconds_since(ticket.claimed_at?);
            (held > HELD_SECONDS).then(|| Held {
                ticket: ticket.id.to_string(),
                title: ticket.title.clone(),
                hours: (held / 3_600.0) as u64,
                session: ticket
                    .session
                    .as_ref()
                    .map(ToString::to_string)
                    .or_else(|| ticket.assignee.clone())
                    .unwrap_or_else(|| "unknown".to_owned()),
                machine: ticket
                    .machine
                    .as_ref()
                    .map_or_else(|| "unknown".to_owned(), ToString::to_string),
            })
        })
        .collect()
}

fn held_filing(held: &Held) -> Filing {
    let Held {
        ticket,
        title,
        hours,
        session,
        machine,
    } = held;
    Filing {
        signature: Signature::of_subject(ticket, "held"),
        evidence: "held".to_owned(),
        fields: TicketFields {
            title: Some(format!(
                "Ticket {ticket} has been in progress for {hours} hours"
            )),
            body: Some(format!(
                "{ticket} ({title}) is held for {hours} hours by session {session} on machine \
                 {machine}. A session that stopped, a worker that lost the ticket or a ticket \
                 too large for one session leaves it so. Find out which, and release, split or                  finish the ticket."
            )),
            ticket_type: Some(TicketType::Task),
            priority: Some(1),
            labels: Some(vec![JUSTICIAR.to_owned()]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
    }
}
