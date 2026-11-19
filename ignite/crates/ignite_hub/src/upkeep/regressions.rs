//! Regression tickets: one P0 ticket per failing check, linked to the landing that caused it.
//!
//! - A check with no regression gets a new ticket, linked to the culprit's ticket or, when the
//!   culprit search ended with a range, to every ticket in the range. A check that is not
//!   portable carries a `needs:<os>` label so that only a worker on that system takes it.
//! - A check whose regression ticket is still open keeps it, and a second failure changes
//!   nothing.
//! - When the fix has landed and closed the ticket but the check still fails, the ticket is
//!   reopened with the new output.
//! - An upkeep pass marks a regression fixed once its ticket is closed and the check has passed
//!   since.

use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::protocol::hub::RegressionReport;
use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, EdgeKind, Operation, Status, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{tickets, tips};
use ignite_store::{Result, StoreError};

use super::filing::Author;

/// The lines of a regression's output kept in its ticket.
pub const OUTPUT_LINES: usize = 40;

/// What filing a regression did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Reopened,
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
    let (ops, outcome) = match &existing {
        None => (vec![create(report)], Outcome::Created),
        Some(ticket) => {
            let loaded = tickets::load(conn, ticket)?;
            if !loaded.as_ref().is_some_and(|t| t.status == Status::Closed) {
                return Ok((ticket.clone(), Outcome::Unchanged));
            }
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
                Operation::Comment {
                    id: ticket.clone(),
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
    let ticket = match batch::apply(conn, &request, author.actor, author.machine, now)? {
        BatchOutcome::Applied(reply) => reply.results[0].id.to_string(),
        BatchOutcome::Conflict(conflict) => {
            return Err(StoreError::Rejected(format!(
                "filing the regression of {check} conflicted at operation {}",
                conflict.failed
            )));
        }
    };
    if outcome == Outcome::Reopened {
        // The store reopens only a regression marked fixed, with the new commits.
        tips::regression_fixed(conn, check)?;
    }
    tips::regression(conn, report, &ticket)?;
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

fn create(report: &RegressionReport) -> Operation {
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
    let mut labels = vec![
        "regression".to_owned(),
        format!("check:{}", report.check.family()),
    ];
    if let Some(os) = report.platform {
        labels.push(format!("needs:{}", os.as_str()));
    }
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
    let _ = write!(
        text,
        "rerun: ignite test {}\n\noutput (last {OUTPUT_LINES} lines):\n{tail}",
        report.check
    );
    text
}
