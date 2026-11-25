//! Regression tickets: P0 tickets for failing checks, linked to the landings that caused them.
//!
//! - A check with no regression gets a new ticket, linked to the culprit's ticket or, when the
//!   culprit search ended with a range, to every ticket in the range. It is pinned to the
//!   machine that reported it, and records the operating system of a check that is not
//!   portable. A regression holds `master` back, so when that machine goes silent it falls back
//!   to any other machine with that system.
//! - Failures are grouped: a newly failing check is added to an open regression ticket
//!   ([`Outcome::Added`]) when one holds a check with the same first bad commit, or when one is
//!   not yet held by a worker, who then gets every check on it. One bad landing often fails
//!   several checks, and when each had a ticket and a worker of its own, most of those workers
//!   closed theirs as fixed by another.
//! - A check gets a ticket of its own only when it comes from another landing and every open
//!   regression ticket is already being worked, and never when [`OPEN_TICKETS`] are open; then
//!   it is added to the open ticket that needs the same operating system, else the one with the
//!   fewest checks.
//! - A check whose regression ticket is still open keeps it. A later report can set its
//!   confirmed culprit and tell the current worker.
//! - When the fix has landed and closed the ticket but the check still fails, the ticket is
//!   reopened with the new output and no assignee, so that dispatch offers it to a worker
//!   again, unless the rules above add the check to an open ticket. The reopen also clears the
//!   session, tree and machine of the old claim, as a release does: a daemon that still saw
//!   them took the ticket for held and refused every other session's `ignite start`.
//! - A regression a landing found, not tip verification, also records why tip verification
//!   missed it (`super::missed`).
//! - A verified tip can settle an open regression as soon as every check passes. See
//!   [`resolution`] for how the hub closes an unheld ticket or tells its holder.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use ignite_core::protocol::hub::{LandingRegression, RegressionReport};
use ignite_core::protocol::tickets::{
    BatchRequest, CAUSED_BY_KEY, CULPRIT_AFFINITY_KEY, CulpritAffinity, Dependency, EdgeKind,
    Operation, Status, Ticket, TicketFields, TicketType, regression_culprit,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{edges, fleet, tickets, tips};
use ignite_store::{Result, StoreError};

use super::culprit_affinity;
use super::filing::{self, Author};
use crate::routes::dispatch::set_columns;

pub(crate) mod landing;
mod resolution;
pub use resolution::{passing, settle};

/// Settle regressions after verification or a check switch. The evidence has already
/// committed, so upkeep can retry a failure without making the original request fail.
pub(crate) fn settle_after_change(conn: &mut Connection, now: Timestamp) {
    if let Err(error) = settle(conn, now) {
        eprintln!("ignite-hub: could not settle passing regressions: {error}");
    }
}

/// The lines of a regression's output kept in its ticket.
pub const OUTPUT_LINES: usize = 40;

/// How many regression tickets may be open at once; further failing checks are added to them.
pub const OPEN_TICKETS: usize = 3;

/// The ticket metadata key that holds when a check was last added to an open regression
/// ticket, so that only tips seen after that can clear it (see [`resolution`]).
pub const ADDED_KEY: &str = "ignite_regression_added_at";

/// The check whose confirmed culprit supplies the transcript when several checks share a ticket.
const CULPRIT_CHECK_KEY: &str = "culprit_check";

/// What filing a regression did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Reopened,
    /// The check was added to an open regression ticket.
    Added,
    /// A later report recorded the culprit on the open ticket.
    Attributed,
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
    file_report(conn, author, report, None, now)
}

fn file_report(
    conn: &mut Connection,
    author: Author,
    report: &RegressionReport,
    baseline: Option<&LandingRegression>,
    now: Timestamp,
) -> Result<(String, Outcome)> {
    let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let check = report.check.as_str();
    let existing: Option<String> = tx
        .query_row(
            "SELECT ticket FROM regressions WHERE check_name = ?1",
            params![check],
            |row| row.get(0),
        )
        .optional()?;
    let closed = match &existing {
        None => None,
        Some(ticket) => {
            let Some(loaded) = tickets::load(&tx, ticket)? else {
                return Ok((ticket.clone(), Outcome::Unchanged));
            };
            if loaded.status != Status::Closed {
                let outcome = update_open(&mut tx, &loaded, author, report, now)?;
                tx.commit()?;
                return Ok((ticket.clone(), outcome));
            }
            Some(loaded)
        }
    };
    let open = open_tickets(&tx)?;
    let target = pick(&tx, &open, report)?;
    let mut write_at = now;
    let (ops, outcome) = match (&target, closed) {
        (Some(target), _) => {
            let mut ops = added(&tx, target, author, report, now)?;
            if let Some(ticket) = tickets::load(&tx, target)?
                && regression_culprit(&ticket).is_none()
            {
                write_at = ticket.claimed_at.map_or(now, |claimed| now.max(claimed));
                // Attribution goes first so its notification names its own comment number.
                let mut with_attribution = attribution(&tx, &ticket, author, report, false)?;
                with_attribution.append(&mut ops);
                ops = with_attribution;
            }
            (ops, Outcome::Added)
        }
        (None, None) => {
            let pin = affinity(&tx, report, None)?;
            let origin = filing::origin(&tx, author.machine)?;
            (vec![create(report, origin, pin)], Outcome::Created)
        }
        (None, Some(ticket)) => (reopen(&tx, &ticket, author, report)?, Outcome::Reopened),
    };
    let created = apply(&mut tx, author, check, ops, write_at)?;
    let ticket = target.unwrap_or_else(|| created.expect("filing had an operation"));
    if outcome == Outcome::Reopened {
        let cleared =
            ["session", "tree", "machine", "claimed_at"].map(|column| (column, Value::Null));
        set_columns(&tx, &ticket, author.actor, &cleared, now)?;
    }
    if outcome == Outcome::Added && existing.is_some() {
        tx.execute(
            "UPDATE regressions SET ticket = ?2 WHERE check_name = ?1",
            params![check, ticket],
        )?;
    }
    if existing.is_some() && outcome != Outcome::Created {
        // The store reopens only a regression marked fixed, with the new commits.
        tips::regression_fixed(&tx, check)?;
    }
    tips::regression(&tx, report, &ticket)?;
    if let Some(baseline) = baseline {
        landing::queue(&tx, &ticket, author.machine, baseline)?;
    }
    tx.commit()?;
    // A landing found the failure before tip verification did. Recording why must not
    // keep the regression from being filed.
    if let Err(error) = super::missed::record(conn, author, report, &ticket, now) {
        eprintln!("ignite-hub: why tip verification missed {check} was not recorded: {error}");
    }
    Ok((ticket, outcome))
}

fn update_open(
    tx: &mut rusqlite::Transaction<'_>,
    ticket: &Ticket,
    author: Author,
    report: &RegressionReport,
    now: Timestamp,
) -> Result<Outcome> {
    let ops = attribution(tx, ticket, author, report, false)?;
    let check = report.check.as_str();
    if let Some(culprit) = &report.caused_by {
        // A grouped check can get a confirmed culprit without changing the ticket's routing.
        tx.execute(
            "UPDATE regressions SET first_bad = ?2, last_good = ?3,
                culprit_ticket = ?4, failure_output = COALESCE(NULLIF(?5, ''), failure_output),
                landing_attribution = NULL
             WHERE check_name = ?1",
            params![
                check,
                report.first_bad.as_str(),
                report.last_good.as_str(),
                culprit.as_str(),
                report.output,
            ],
        )?;
    }
    if ops.is_empty() {
        return Ok(Outcome::Unchanged);
    }
    // The report may have waited for the transaction that claimed the ticket.
    let now = ticket.claimed_at.map_or(now, |claimed| now.max(claimed));
    apply(tx, author, check, ops, now)?;
    Ok(Outcome::Attributed)
}

fn reopen(
    conn: &Connection,
    ticket_data: &Ticket,
    author: Author,
    report: &RegressionReport,
) -> Result<Vec<Operation>> {
    let ticket = ticket_data.id.to_string();
    // Upkeep closes a regression whose check passed on a later tip (`clearing`); the
    // check failing again then is not a failed fix.
    let cleared = ticket_data
        .close_reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with(super::clearing::CLOSE_PREFIX));
    let again = if cleared {
        "The check fails again after Ignite closed this ticket because a later tip passed."
    } else {
        "The check still fails after the fix."
    };
    let mut ops = vec![
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
    ops.extend(attribution(conn, ticket_data, author, report, cleared)?);
    Ok(ops)
}

fn apply(
    tx: &mut rusqlite::Transaction<'_>,
    author: Author,
    check: &str,
    ops: Vec<Operation>,
    now: Timestamp,
) -> Result<Option<String>> {
    if ops.is_empty() {
        return Ok(None);
    }
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    match batch::apply_automatic_in(tx, &request, author.actor, author.machine, now)? {
        BatchOutcome::Applied(reply) => Ok(Some(reply.results[0].id.to_string())),
        BatchOutcome::Conflict(conflict) => Err(StoreError::Rejected(format!(
            "filing the regression of {check} conflicted at operation {}",
            conflict.failed
        ))),
    }
}

fn affinity(
    conn: &Connection,
    report: &RegressionReport,
    repair: Option<&Ticket>,
) -> Result<Option<CulpritAffinity>> {
    let Some(id) = &report.caused_by else {
        return Ok(None);
    };
    let Some(culprit) = tickets::load(conn, id.as_str())? else {
        return Ok(None);
    };
    let needed_os = repair.map_or(report.platform, |ticket| ticket.needs_os);
    culprit_affinity::for_repair(conn, &culprit, repair, needed_os)
}

/// Updates an existing ticket when a later report confirms the culprit. Returns any open
/// reservation to the queue so the dispatcher starts from the updated attribution.
fn attribution(
    conn: &Connection,
    ticket: &Ticket,
    author: Author,
    report: &RegressionReport,
    cleared: bool,
) -> Result<Vec<Operation>> {
    if report.caused_by.is_none() && !cleared {
        return Ok(Vec::new());
    }
    // Several failing checks can share the ticket. Once a check supplies the transcript, other
    // checks' reports must not repeatedly change its routing or invalidate its reservations.
    if ticket.status != Status::Closed
        && let Some(check) = ticket
            .metadata
            .get(CULPRIT_CHECK_KEY)
            .and_then(Value::as_str)
        && check != report.check.as_str()
    {
        return Ok(Vec::new());
    }
    let cause_changed = regression_culprit(ticket) != report.caused_by;
    let pin = affinity(conn, report, Some(ticket))?;
    let pin_changed = culprit_affinity::affinity(ticket) != pin;
    let cause = json!(report.caused_by);
    if ticket.metadata.get(CAUSED_BY_KEY) == Some(&cause) && !pin_changed {
        return Ok(Vec::new());
    }
    let id = ticket.id.to_string();
    if (cause_changed || pin_changed) && ticket.status == Status::Open {
        fleet::release_reservation(conn, &id)?;
    }
    let mut ops = Vec::new();
    let mut set_meta = BTreeMap::from([
        (CAUSED_BY_KEY.to_owned(), cause),
        (
            CULPRIT_CHECK_KEY.to_owned(),
            json!(report.caused_by.as_ref().map(|_| &report.check)),
        ),
        (filing::FILERS_KEY.to_owned(), json!(filers(ticket, author))),
    ]);
    if let Some(culprit) = &report.caused_by
        && cause_changed
    {
        ops.push(Operation::Comment {
            id: id.clone(),
            text: format!(
                "The culprit search for {} finished.\n\n{}",
                report.check,
                details(report)
            ),
        });
        if ticket.id != *culprit && edges::kind(conn, &id, culprit.as_str())?.is_none() {
            ops.push(Operation::DepAdd {
                id: id.clone(),
                on: culprit.to_string(),
                kind: Some(EdgeKind::DiscoveredFrom),
                external: false,
            });
        }
        if ticket.status == Status::InProgress && ticket.assignee.is_some() {
            let n: u32 = conn.query_row(
                "SELECT COALESCE(MAX(n), 0) + 1 FROM comments WHERE ticket = ?1",
                [&id],
                |row| row.get(0),
            )?;
            set_meta.insert("tell".to_owned(), json!(n));
        }
    }
    let (add_labels, remove_labels, unset_meta) =
        culprit_affinity::update(ticket, pin, &mut set_meta);
    let set = ((cause_changed || cleared)
        && ticket.title == format!("Regression: {}", report.check))
    .then(|| TicketFields {
        body: Some(details(report)),
        ..TicketFields::default()
    });
    ops.push(Operation::Update {
        id,
        set,
        add_labels,
        remove_labels,
        set_meta,
        unset_meta,
    });
    Ok(ops)
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
    let filers =
        tickets::load(conn, target)?.map_or_else(Vec::new, |ticket| filers(&ticket, author));
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

fn filers(ticket: &Ticket, author: Author) -> Vec<String> {
    let mut filers = filing::strings(&ticket.metadata, filing::FILERS_KEY);
    if filers.is_empty() {
        filers.push(ticket.created_by.clone());
    }
    if !filers.iter().any(|filer| filer == author.actor) {
        filers.push(author.actor.to_owned());
    }
    filers
}

fn create(
    report: &RegressionReport,
    origin: Option<String>,
    pin: Option<CulpritAffinity>,
) -> Operation {
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
    labels
        .extend(origin.filter(|label| pin.as_ref().is_none_or(|pin| pin.machine_label != *label)));
    let mut metadata = BTreeMap::from([(CAUSED_BY_KEY.to_owned(), json!(report.caused_by))]);
    if report.caused_by.is_some() {
        metadata.insert(CULPRIT_CHECK_KEY.to_owned(), json!(report.check));
    }
    if let Some(pin) = pin {
        labels.push(pin.machine_label.clone());
        metadata.insert(CULPRIT_AFFINITY_KEY.to_owned(), json!(pin));
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
            metadata: Some(metadata),
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
    } else if report.found_by == ignite_core::protocol::hub::FoundBy::Landing
        && report.detected_at.as_ref() == Some(&report.first_bad)
    {
        let _ = writeln!(text, "culprit: historical search runs separately");
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
