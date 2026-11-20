//! Regression tickets: P0 tickets for failing checks, linked to the landings that caused them.
//!
//! - A check with no regression gets a new ticket, linked to the culprit's ticket or, when the
//!   culprit search ended with a range, to every ticket in the range. It is owned by the
//!   machine that reported it, and records the operating system of a check that is not
//!   portable. A regression holds `master` back, so when that machine goes silent it falls back
//!   to any other machine with that system (see `ignite_core::owner`).
//! - Failures are grouped: a newly failing check is added to an open regression ticket
//!   ([`Outcome::Added`]) when one holds a check with the same first bad commit, or when one is
//!   not yet held by a worker, who then gets every check on it. One bad landing often fails
//!   several checks, and when each had a ticket and a worker of its own, most of those workers
//!   closed theirs as fixed by another.
//! - A check gets a ticket of its own only when it comes from another landing and every open
//!   regression ticket is already being worked, and never when [`OPEN_TICKETS`] are open; then
//!   it is added to the open ticket that needs the same operating system, else the one with the
//!   fewest checks.
//! - A check whose regression ticket is still open keeps it, and a second failure changes
//!   nothing.
//! - When the fix has landed and closed the ticket but the check still fails, the ticket is
//!   reopened with the new output and no assignee, so that dispatch offers it to a worker
//!   again, unless the rules above add the check to an open ticket. The reopen also clears the
//!   session, tree and machine of the old claim, as a release does: a daemon that still saw
//!   them took the ticket for held and refused every other session's `ignite start`.
//! - A regression a landing found, not tip verification, also records why tip verification
//!   missed it (`super::missed`).
//! - An upkeep pass marks a regression fixed once its ticket is closed and the check has passed
//!   since.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use ignite_core::owner::Owner;
use ignite_core::protocol::hub::RegressionReport;
use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, EdgeKind, Operation, Status, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{tickets, tips};
use ignite_store::{Result, StoreError};

use super::filing::{self, Author};
use crate::routes::dispatch::set_columns;

/// The lines of a regression's output kept in its ticket.
pub const OUTPUT_LINES: usize = 40;

/// How many regression tickets may be open at once; further failing checks are added to them.
pub const OPEN_TICKETS: usize = 3;

/// The ticket metadata key that holds when a check was last added to an open regression
/// ticket, so that only tips seen after that can clear it (see `super::clearing`).
pub const ADDED_KEY: &str = "ignite_regression_added_at";

/// What filing a regression did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Reopened,
    /// The check was added to an open regression ticket.
    Added,
    /// The check's regression ticket is still open.
    Unchanged,
}

/// Records the regression and returns its ticket and what changed.
pub fn file(
    conn: &mut Connection,
    author: Author,
    report: &RegressionReport,
    now: Timestamp,
) -> Result<(String, Outcome)> {
    let check = report.check.as_str();
    let existing: Option<String> = conn
        .query_row(
            "SELECT ticket FROM regressions WHERE check_name = ?1",
            params![check],
            |row| row.get(0),
        )
        .optional()?;
    let closed = match &existing {
        None => None,
        Some(ticket) => {
            let loaded = tickets::load(conn, ticket)?;
            if !loaded.as_ref().is_some_and(|t| t.status == Status::Closed) {
                return Ok((ticket.clone(), Outcome::Unchanged));
            }
            Some((ticket.clone(), loaded))
        }
    };
    let open = open_tickets(conn)?;
    let target = pick(conn, &open, report)?;
    let (ops, outcome) = match (&target, closed) {
        (Some(target), _) => (added(conn, target, author, report, now)?, Outcome::Added),
        (None, None) => (
            vec![create(report, filing::origin(conn, author.machine)?)],
            Outcome::Created,
        ),
        (None, Some((ticket, loaded))) => {
            // Upkeep closes a regression whose check passed on a later tip (`clearing`); the
            // check failing again then is not a failed fix.
            let cleared = loaded
                .and_then(|t| t.close_reason)
                .is_some_and(|reason| reason.starts_with(super::clearing::CLOSE_PREFIX));
            let again = if cleared {
                "The check fails again after Ignite closed this ticket because a later tip passed."
            } else {
                "The check still fails after the fix."
            };
            let ops = vec![
                Operation::Reopen { id: ticket.clone() },
                // The session that closed the ticket is still its assignee, and dispatch
                // offers no ticket that has one, so without this nobody works it again.
                Operation::Update {
                    id: ticket.clone(),
                    set: Some(TicketFields {
                        assignee: Some(None),
                        ..TicketFields::default()
                    }),
                    add_labels: Vec::new(),
                    remove_labels: Vec::new(),
                    set_meta: BTreeMap::new(),
                    unset_meta: Vec::new(),
                },
                Operation::Comment {
                    id: ticket,
                    text: format!("{again}\n\n{}", details(report)),
                },
            ];
            (ops, Outcome::Reopened)
        }
    };
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    let ticket = match batch::apply_automatic(conn, &request, author.actor, author.machine, now)? {
        BatchOutcome::Applied(reply) => target.unwrap_or_else(|| reply.results[0].id.to_string()),
        BatchOutcome::Conflict(conflict) => {
            return Err(StoreError::Rejected(format!(
                "filing the regression of {check} conflicted at operation {}",
                conflict.failed
            )));
        }
    };
    if outcome == Outcome::Reopened {
        let cleared =
            ["session", "tree", "machine", "claimed_at"].map(|column| (column, Value::Null));
        set_columns(conn, &ticket, author.actor, &cleared, now)?;
    }
    if outcome == Outcome::Added && existing.is_some() {
        conn.execute(
            "UPDATE regressions SET ticket = ?2 WHERE check_name = ?1",
            params![check, ticket],
        )?;
    }
    if existing.is_some() && outcome != Outcome::Created {
        // The store reopens only a regression marked fixed, with the new commits.
        tips::regression_fixed(conn, check)?;
    }
    tips::regression(conn, report, &ticket)?;
    if outcome != Outcome::Unchanged {
        // A landing found the failure before tip verification did. Recording why must not
        // keep the regression from being filed.
        if let Err(error) = super::missed::record(conn, author, report, &ticket, now) {
            eprintln!("ignite-hub: why tip verification missed {check} was not recorded: {error}");
        }
    }
    Ok((ticket, outcome))
}

/// Marks fixed each regression whose ticket is closed and whose check has passed since the
/// ticket closed. Returns the checks marked.
pub fn mark_fixed(conn: &Connection) -> Result<Vec<String>> {
    let mut select = conn.prepare(
        "SELECT r.check_name FROM regressions r
         JOIN tickets t ON t.id = r.ticket
         WHERE r.state != ?1 AND t.status = 'closed' AND EXISTS (
           SELECT 1 FROM results p
           WHERE p.check_name = r.check_name AND p.outcome = 'pass' AND p.at >= t.closed_at)
         ORDER BY r.check_name",
    )?;
    let fixed = select
        .query_map(params![tips::FIXED], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for check in &fixed {
        tips::regression_fixed(conn, check)?;
    }
    Ok(fixed)
}

/// Each open regression ticket, with the first bad commits of its checks.
fn open_tickets(conn: &Connection) -> Result<BTreeMap<String, Vec<String>>> {
    let mut select = conn.prepare(
        "SELECT r.ticket, r.first_bad FROM regressions r JOIN tickets t ON t.id = r.ticket
         WHERE t.status != 'closed'",
    )?;
    let mut open: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in select.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))? {
        let (ticket, first_bad): (String, String) = row?;
        open.entry(ticket).or_default().push(first_bad);
    }
    Ok(open)
}

/// The open ticket a newly failing check is added to, or `None` for a ticket of its own. In
/// order: one whose checks share its first bad commit, one no worker holds yet, then, when
/// [`OPEN_TICKETS`] are open, one that needs the same operating system, else the one with the
/// fewest checks.
fn pick(
    conn: &Connection,
    open: &BTreeMap<String, Vec<String>>,
    report: &RegressionReport,
) -> Result<Option<String>> {
    // Shares the first bad commit, not held, same system, fewest checks; the first ticket wins a
    // tie.
    type Rank = (bool, bool, bool, std::cmp::Reverse<usize>);
    let mut best: Option<(Rank, &String)> = None;
    for (ticket, commits) in open {
        let Some(loaded) = tickets::load(conn, ticket)? else {
            continue;
        };
        let rank = (
            commits.iter().any(|c| c == report.first_bad.as_str()),
            loaded.status == Status::Open && loaded.assignee.is_none(),
            loaded.needs_os == report.platform,
            std::cmp::Reverse(commits.len()),
        );
        if best.as_ref().is_none_or(|(top, _)| rank > *top) {
            best = Some((rank, ticket));
        }
    }
    let full = open.len() >= OPEN_TICKETS;
    Ok(best
        .filter(|((shared, waiting, _, _), _)| *shared || *waiting || full)
        .map(|(_, ticket)| ticket.clone()))
}

/// The writes that add a check to the open regression ticket `target`. Whoever reports it is
/// recorded among the ticket's filers, so that `super::clearing` does not read the comment as a
/// person's and stop closing the ticket.
fn added(
    conn: &Connection,
    target: &str,
    author: Author,
    report: &RegressionReport,
    now: Timestamp,
) -> Result<Vec<Operation>> {
    let mut filers = Vec::new();
    if let Some(ticket) = tickets::load(conn, target)? {
        filers = filing::strings(&ticket.metadata, filing::FILERS_KEY);
        if filers.is_empty() {
            filers.push(ticket.created_by);
        }
    }
    if !filers.iter().any(|filer| filer == author.actor) {
        filers.push(author.actor.to_owned());
    }
    let text = format!(
        "Another check fails: {check}. Failing checks are grouped on as few regression tickets \
         as can be (at most {OPEN_TICKETS}), so it is added to this one. This ticket is done \
         when every check on it passes.\n\n{details}",
        check = report.check,
        details = details(report),
    );
    Ok(vec![
        Operation::Comment {
            id: target.to_owned(),
            text,
        },
        Operation::Update {
            id: target.to_owned(),
            set: None,
            add_labels: vec![format!("check:{}", report.check.family())],
            remove_labels: Vec::new(),
            set_meta: BTreeMap::from([
                (ADDED_KEY.to_owned(), json!(now.to_string())),
                (filing::FILERS_KEY.to_owned(), json!(filers)),
            ]),
            unset_meta: Vec::new(),
        },
    ])
}

fn create(report: &RegressionReport, origin: Option<Owner>) -> Operation {
    let mut linked: Vec<String> = report.caused_by.iter().map(ToString::to_string).collect();
    for suspect in &report.suspects {
        let suspect = suspect.to_string();
        if !linked.contains(&suspect) {
            linked.push(suspect);
        }
    }
    let deps = linked
        .into_iter()
        .map(|on| Dependency {
            on,
            kind: Some(EdgeKind::DiscoveredFrom),
        })
        .collect();
    let labels = vec![
        "regression".to_owned(),
        format!("check:{}", report.check.family()),
    ];
    Operation::Create {
        fields: TicketFields {
            title: Some(format!("Regression: {}", report.check)),
            // Filed released, unlike every other ticket: a regression holds `master` back, so it
            // must not wait for a person to approve it.
            status: Some(Status::Open),
            body: Some(details(report)),
            ticket_type: Some(TicketType::Bug),
            priority: Some(0),
            labels: Some(labels),
            owner: Some(origin),
            needs_os: Some(report.platform),
            fallback: Some(true),
            ..TicketFields::default()
        },
        parent: None,
        deps,
        request_key: Some(format!("regression:{}:{}", report.check, report.first_bad)),
    }
}

/// The ticket text: who caused it, between which commits, where, and the end of its output.
pub fn details(report: &RegressionReport) -> String {
    let lines: Vec<&str> = report.output.lines().collect();
    let tail = lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n");
    let mut text = String::new();
    if let Some(ticket) = &report.caused_by {
        let _ = writeln!(text, "caused-by  {ticket} (commit {})", report.first_bad);
    }
    if !report.suspects.is_empty() {
        let suspects: Vec<String> = report.suspects.iter().map(ToString::to_string).collect();
        let _ = writeln!(
            text,
            "suspects   {} (the culprit search ended with a range)",
            suspects.join(", ")
        );
    }
    // Without this line a range read as if its newest commit were the culprit.
    if let Some(search) = &report.search {
        let _ = writeln!(text, "culprit: {}", search.words());
    }
    let _ = writeln!(
        text,
        "first bad {}, last good {}",
        report.first_bad, report.last_good
    );
    if let Some(os) = report.platform {
        let _ = writeln!(text, "failed on {}", os.as_str());
    }
    let _ = write!(text, "rerun: ignite test {}\n\n", report.check);
    if tail.trim().is_empty() {
        // An empty "output" heading left readers unable to tell a missing output from a
        // truncated one.
        text.push_str(
            "no output: the failing run reported none, so rerun the check to see what failed.",
        );
    } else {
        let _ = write!(text, "output (last {OUTPUT_LINES} lines):\n{tail}");
    }
    text
}
