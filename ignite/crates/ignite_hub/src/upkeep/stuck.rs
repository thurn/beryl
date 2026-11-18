//! P1 tickets for work that cannot go on by itself:
//!
//! - a check still `incomplete` after its last attempt, or one that cannot run on a machine;
//! - a cache audit whose rerun disagreed with the cache, which also switches off caching for
//!   that check;
//! - a tip that has waited an hour while a machine has not reported;
//! - an epic with open tickets of which none is ready or in progress, nor waits directly on a
//!   ticket that is.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use ignite_core::ids::TicketId;
use ignite_core::protocol::hub::{AuditReport, CheckState, IncompleteReason, IncompleteReport};
use ignite_core::protocol::tickets::{Graph, GraphTicket, Status, TicketFields, TicketType};
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
    checks::set_state(conn, check, CheckState::Uncached)?;
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
    filing::file(conn, author, &filing, now)
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

/// Files one ticket for each epic whose open tickets are all blocked. Of nested stuck epics,
/// only the innermost is reported, since the outer one waits on it.
pub fn stuck_epics(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let graph = tickets::graph(conn, false, now)?;
    let filings: Vec<Filing> = find_stuck(&graph, now)
        .into_iter()
        .map(|(epic, lines)| epic_filing(&epic, &lines))
        .collect();
    let mut filed = Vec::with_capacity(filings.len());
    for filing in &filings {
        filed.push(filing::file(conn, UPKEEP, filing, now)?);
    }
    Ok(filed)
}

/// Each innermost stuck epic, with a line per waiting ticket saying what it waits on.
fn find_stuck(graph: &Graph, now: Timestamp) -> Vec<(String, Vec<String>)> {
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
    // A ticket can move when it is ready, in progress, or waits directly on one that is: a
    // ticket outside the epic that someone is working on unblocks it in time.
    let active: BTreeSet<&str> = graph
        .tickets
        .iter()
        .filter(|t| t.ready || t.ticket.status == Status::InProgress)
        .map(|t| t.ticket.id.as_str())
        .collect();
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
        let moves = active.contains(ticket.id.as_str())
            || entry
                .blocked_by
                .iter()
                .any(|b| active.contains(b.id.as_str()));
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
                    let by: Vec<String> =
                        entry.blocked_by.iter().map(|b| b.id.to_string()).collect();
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
