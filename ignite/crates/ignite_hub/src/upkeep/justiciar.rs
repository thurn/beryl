//! Justiciar tickets: P0 tickets for an emergency that stops work and that no ordinary ticket
//! will fix. New justiciar roots get `J` IDs. The dispatch queue offers tickets carrying the
//! `justiciar` label first. They use `tier:opus` because the session has to diagnose the problem
//! and may change anything to end it (`ignite guide justiciar`).
//!
//! Sources:
//!
//! - A report (`POST /justiciar`) from a daemon that saw Ignite crash or one of its loops keep
//!   failing, or from a session that ran `ignite justiciar "<reason>"` ([`report`]).
//! - Upkeep, when `master` has not moved for [`MASTER_LAG_SECONDS`] although commits landed
//!   on `staging` ([`master_lag`]). Tip verification is the only thing that moves `master`, so
//!   when it is stuck, for any reason, nothing reaches `master` and no fix to Ignite itself is
//!   released from it. Upkeep tells regression tickets whose work is progressing how long
//!   `master` has waited, and files a justiciar ticket for each whose work has stalled.
//!   A stuck landing whose justiciar ticket was closed less than [`MASTER_LAG_SECONDS`] ago
//!   gives that worker's fix time to reach the verified branch.
//! - Upkeep, when a machine stays stuck on a resource ([`super::resource_stalls`]).
//! - The dispatch routes, when different sessions have released one ticket
//!   ([`ignite_store::hub::releases::HOLD_AFTER_SESSIONS`] of them) and the hub stops offering
//!   it ([`released_repeatedly`]). Each worker met an obstacle in the ticket itself, so a
//!   justiciar fixes the ticket rather than another worker trying it.
//!
//! Every filing goes through [`filing`] with a signature of its subject, so a repeated sighting
//! adds a comment to the open ticket and never a second ticket. Every justiciar ticket is filed
//! released ([`Filing::released`]): an emergency that stops work cannot wait for a person to
//! release its ticket.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use ignite_core::pins::machine_label;
use ignite_core::protocol::hub::JusticiarReport;
use ignite_core::protocol::tickets::{
    BatchRequest, Operation, Precondition, Status, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::feed;
use ignite_store::hub::releases::Releases;
use ignite_store::hub::{tickets, tips};
use ignite_store::{Result, StoreError};

use super::filing::{self, Author, Change, Filed, Filing, UPKEEP};
use super::signature::Signature;
use super::{
    parse_time,
    regression_stalls::{self, Stall},
};

pub use ignite_core::protocol::tickets::JUSTICIAR_LABEL as LABEL;

/// The subject of the signature [`master_lag`] files under.
pub const MASTER_LAG_SUBJECT: &str = "master-lag";

/// The prefix of subjects identifying regression tickets whose handling has stalled.
pub(super) const REGRESSION_STALL_SUBJECT: &str = "regression-stall:";

/// The model tier a justiciar ticket asks for.
const TIER: &str = "tier:opus";

/// How long the oldest landing on `staging` that `master` does not hold may wait before a
/// justiciar ticket is filed: two hours. A tip round under load takes a few minutes, and a
/// failing check already has a regression ticket, so two hours without `master` moving is a
/// fault that no other ticket covers. At 30 minutes, most tickets filed were closed as "no
/// emergency" by a worker who found ordinary regressions holding `master` back.
pub const MASTER_LAG_SECONDS: f64 = 2.0 * 3600.0;

/// Landings older than this are left out of the lag check, so history imported from before
/// the hub existed never counts as lag.
const LAG_HORIZON_SECONDS: f64 = 7.0 * 86_400.0;

/// The subject of the signature [`released_repeatedly`] files under, followed by the ticket.
pub const RELEASED_SUBJECT: &str = "released-repeatedly";

/// The ticket fields of a justiciar ticket.
fn fields(title: String, body: String) -> TicketFields {
    let all = vec![LABEL.to_owned(), TIER.to_owned()];
    TicketFields {
        title: Some(title),
        body: Some(body),
        ticket_type: Some(TicketType::Task),
        priority: Some(0),
        labels: Some(all),
        ..TicketFields::default()
    }
}

/// Files or comments on the justiciar ticket for `report`.
pub fn report(
    conn: &mut Connection,
    author: Author,
    report: &JusticiarReport,
    now: Timestamp,
) -> Result<Filed> {
    // The ticket is pinned to the machine that reported it (see `filing`), and falls back to
    // another machine when that one goes silent, since it is filed released.
    let subject = if report.on_machine {
        format!("{}@{}", report.subject, report.machine)
    } else {
        report.subject.clone()
    };
    let body = format!(
        "Reported by {} ({}).\n\n{}\n\nRead `ignite guide justiciar` before anything else.",
        report.host, report.machine, report.details
    );
    let mut fields = fields(report.title.clone(), body);
    if report.on_machine {
        fields
            .labels
            .get_or_insert_with(Vec::new)
            .push(machine_label(&report.host));
    }
    let filing = Filing {
        signature: Signature::of_subject(&subject, LABEL),
        evidence: report.evidence.clone(),
        fields,
        deps: Vec::new(),
        released: true,
    };
    filing::file(conn, author, &filing, now)
}

/// Files a machine resource stall or adds the latest sighting to its open ticket.
pub(super) fn file_resource_stall(
    conn: &mut Connection,
    subject: &str,
    evidence: String,
    title: String,
    body: String,
    now: Timestamp,
) -> Result<Filed> {
    let filing = Filing {
        signature: Signature::of_subject(subject, LABEL),
        evidence,
        fields: fields(title, body),
        deps: Vec::new(),
        released: true,
    };
    filing::file(conn, UPKEEP, &filing, now)
}

/// The oldest landing in the last week that came after the last verified tip, as its landing
/// time and ticket, if there is one.
fn oldest_unverified_landing(
    conn: &Connection,
    now: Timestamp,
) -> Result<Option<(Timestamp, String)>> {
    let last_verified: Option<String> = conn
        .query_row(
            "SELECT max(verified) FROM tips WHERE state = 'verified' AND verified IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let horizon = now.plus_seconds(-LAG_HORIZON_SECONDS);
    let after = match &last_verified {
        Some(text) => {
            let verified = parse_time(text)?;
            if verified > horizon {
                verified
            } else {
                horizon
            }
        }
        None => horizon,
    };
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT landed_at, id FROM tickets
             WHERE landed_at IS NOT NULL AND landed_at > ?1
             ORDER BY landed_at LIMIT 1",
            [after.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(at, id)| Ok((parse_time(&at)?, id))).transpose()
}

/// Whether a landing has waited [`MASTER_LAG_SECONDS`] or more for `master` at `now`.
pub(super) fn master_lagging(conn: &Connection, now: Timestamp) -> Result<bool> {
    Ok(oldest_unverified_landing(conn, now)?
        .is_some_and(|(landed_at, _)| now.seconds_since(landed_at) >= MASTER_LAG_SECONDS))
}

/// The ticket metadata key that lists the stuck landings a regression ticket was told about.
pub const TOLD_KEY: &str = "ignite_master_lag_told";

/// The metadata key whose new comment number the holding machine's daemon delivers to the
/// session working the ticket, as `ignite tell` sets it.
const TELL_KEY: &str = "tell";

/// How often a tell is tried when other writes keep coming between its read and its write.
const ATTEMPTS: usize = 8;

/// Tells each regression ticket in `tickets` that `master` has waited for the stuck landing
/// `landing`, once per landing: a comment, with [`TELL_KEY`] set to its number so the session
/// working the ticket gets it as a message. A ticket nobody holds keeps the comment for
/// whoever starts it. Upkeep's own comments do not stop a ticket from closing once its fault
/// clears (see `super::clearing`).
fn tell_regressions(
    conn: &mut Connection,
    tickets: &[String],
    landing: &str,
    text: &str,
    now: Timestamp,
) -> Result<Vec<Filed>> {
    let mut told = Vec::new();
    for ticket in tickets {
        told.push(tell_one(conn, ticket, landing, text, now)?);
    }
    Ok(told)
}

fn tell_one(
    conn: &mut Connection,
    ticket: &str,
    landing: &str,
    text: &str,
    now: Timestamp,
) -> Result<Filed> {
    for _ in 0..ATTEMPTS {
        let seq = feed::current(conn)?;
        let (metadata, last): (Option<String>, i64) = conn.query_row(
            "SELECT t.metadata, (SELECT coalesce(max(n), 0) FROM comments c WHERE c.ticket = t.id)
             FROM tickets t WHERE t.id = ?1",
            [ticket],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let metadata: BTreeMap<String, Value> = match metadata {
            Some(json) => serde_json::from_str(&json)
                .map_err(|error| StoreError::Invalid(format!("metadata of {ticket}: {error}")))?,
            None => BTreeMap::new(),
        };
        let mut landings = filing::strings(&metadata, TOLD_KEY);
        if landings.iter().any(|seen| seen == landing) {
            return Ok(Filed {
                ticket: ticket.to_owned(),
                change: Change::Unchanged,
            });
        }
        landings.push(landing.to_owned());
        let request = BatchRequest {
            preconditions: vec![Precondition::Seq { seq }],
            ops: vec![
                Operation::Comment {
                    id: ticket.to_owned(),
                    text: text.to_owned(),
                },
                Operation::Update {
                    id: ticket.to_owned(),
                    set: None,
                    add_labels: Vec::new(),
                    remove_labels: Vec::new(),
                    set_meta: BTreeMap::from([
                        (TELL_KEY.to_owned(), Value::from(last + 1)),
                        (TOLD_KEY.to_owned(), Value::from(landings)),
                    ]),
                    unset_meta: Vec::new(),
                },
            ],
        };
        if let BatchOutcome::Applied(_) =
            batch::apply(conn, &request, UPKEEP.actor, UPKEEP.machine, now)?
        {
            return Ok(Filed {
                ticket: ticket.to_owned(),
                change: Change::Commented,
            });
        }
    }
    Err(StoreError::Invalid(format!(
        "telling {ticket} about master lag did not commit after {ATTEMPTS} attempts"
    )))
}

/// Whether a master-lag ticket that reported the stuck landing `landing` was closed less than
/// [`MASTER_LAG_SECONDS`] before `now`. Its worker either found nothing for Ignite to fix or
/// landed a fix that has not yet had time to move `master`, so a new ticket for the same
/// landing would send a second worker after the same stall. A stall that outlasts that wait
/// gets a new ticket, in case the fix did not work.
fn recently_closed_for(conn: &Connection, landing: &str, now: Timestamp) -> Result<bool> {
    let key = Signature::of_subject(MASTER_LAG_SUBJECT, LABEL).key();
    let newest: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT closed_at, metadata FROM tickets
             WHERE json_extract(metadata, '$.ignite_signature') = ?1 AND status = 'closed'
             ORDER BY created_at DESC, id DESC LIMIT 1",
            [key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((Some(closed_at), metadata)) = newest else {
        return Ok(false);
    };
    if now.seconds_since(parse_time(&closed_at)?) >= MASTER_LAG_SECONDS {
        return Ok(false);
    }
    let metadata: BTreeMap<String, Value> = match metadata {
        Some(json) => serde_json::from_str(&json)
            .map_err(|error| StoreError::Invalid(format!("master-lag metadata: {error}")))?,
        None => BTreeMap::new(),
    };
    Ok(filing::strings(&metadata, filing::SIGHTINGS_KEY)
        .iter()
        .any(|seen| seen.lines().next() == Some(landing)))
}

fn tip_failure_note(conn: &Connection, landed_at: Timestamp) -> Result<String> {
    let (report, context) = if let Some(report) = tips::candidate_failures(conn)? {
        (report, "Required checks reported failures at candidate")
    } else if let Some(report) = tips::latest_failure(conn, landed_at)? {
        (report, "A completed round recorded failures at")
    } else {
        return Ok(String::new());
    };
    let mut note = format!("{context} `{}`.\n\n", report.commit);
    for key in report.checks.iter().take(10) {
        let platform = key.platform.map_or("portable", |os| os.as_str());
        let _ = writeln!(note, "- `{}` ({platform})", key.check);
    }
    if report.checks.len() > 10 {
        let _ = writeln!(note, "- {} more", report.checks.len() - 10);
    }
    note.push_str("\nAny later candidate needs results for its own check inputs.\n\n");
    Ok(note)
}

/// Files a justiciar ticket when a landing has waited [`MASTER_LAG_SECONDS`] for `master`.
/// When open regression tickets explain the stall, it tells the ones making progress and
/// escalates the ones that stalled. A recently closed master-lag ticket gives its fix time to
/// reach the verified branch.
pub fn master_lag(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let Some((landed_at, ticket)) = oldest_unverified_landing(conn, now)? else {
        return Ok(Vec::new());
    };
    let waited = now.seconds_since(landed_at);
    if waited < MASTER_LAG_SECONDS {
        return Ok(Vec::new());
    }
    let minutes = waited / 60.0;
    let regressions = regression_stalls::open(conn, now)?;
    if !regressions.is_empty() {
        let mut filed = Vec::new();
        let mut progressing = Vec::new();
        for (regression, stall) in regressions {
            if let Some(stall) = stall {
                filed.push(file_regression_stall(conn, &regression, stall, now)?);
            } else {
                progressing.push(regression);
            }
        }
        let text = format!(
            "`master` has not moved for {minutes:.0} minutes: ticket {ticket} landed on \
             `staging` at {landed_at} and no tip since has been verified. This regression \
             ticket is still making progress. Fixing its failing checks will help move `master`."
        );
        filed.extend(tell_regressions(conn, &progressing, &ticket, &text, now)?);
        return Ok(filed);
    }
    if recently_closed_for(conn, &ticket, now)? {
        return Ok(Vec::new());
    }
    let failures = tip_failure_note(conn, landed_at)?;
    let body = format!(
        "`master` has not moved for {minutes:.0} minutes: ticket {ticket} landed at {landed_at} \
         and no tip since has been verified. Tip verification is the only thing that moves \
         `master`, so nothing reaches it, and no fix to Ignite is released from it, until \
         this is fixed.\n\n{failures}Look at the tip loop's state in `ignite status` and the daemon \
         log on each machine, the open regression and fleet tickets, and the dashboard's \
         tips page. Read `ignite guide justiciar` before anything else."
    );
    let filing = Filing {
        signature: Signature::of_subject(MASTER_LAG_SUBJECT, LABEL),
        // New check evidence goes on the existing ticket for this landing.
        evidence: if failures.is_empty() {
            ticket
        } else {
            format!("{ticket}\n{failures}")
        },
        fields: fields(
            format!("master has not moved for {minutes:.0} minutes"),
            body,
        ),
        deps: Vec::new(),
        released: true,
    };
    Ok(vec![filing::file(conn, UPKEEP, &filing, now)?])
}

fn file_regression_stall(
    conn: &mut Connection,
    regression: &str,
    stall: Stall,
    now: Timestamp,
) -> Result<Filed> {
    let reason = stall.reason();
    let body = format!(
        "Regression ticket {regression} is holding back `master`. {reason}\n\n\
         Read `ignite guide justiciar`, then check the regression's history, the worker if one \
         holds it, and the tip verification results."
    );
    let filing = Filing {
        signature: Signature::of_subject(&format!("{REGRESSION_STALL_SUBJECT}{regression}"), LABEL),
        evidence: reason,
        fields: fields(format!("Handling of regression {regression} stalled"), body),
        deps: Vec::new(),
        released: true,
    };
    filing::file(conn, UPKEEP, &filing, now)
}

/// Files the justiciar ticket for `ticket`, which different sessions released and which the hub
/// no longer offers, or comments on the open one. The session of each release is a sighting, so
/// a release by a session the ticket has not yet heard from adds a comment, and a session
/// releasing again adds nothing.
pub fn released_repeatedly(
    conn: &mut Connection,
    ticket: &str,
    title: &str,
    releases: &Releases,
    now: Timestamp,
) -> Result<Option<Filed>> {
    let mut notes = String::new();
    for release in &releases.releases {
        let _ = write!(
            notes,
            "Released by {} at {} ({}):\n\n",
            release.session, release.at, release.cause
        );
        for line in release.note.lines() {
            let _ = writeln!(notes, "> {line}");
        }
        notes.push('\n');
    }
    let body = format!(
        "Ticket {ticket} (\"{title}\") was released {} times by {} different sessions, so the \
         pool is no longer offered it: each worker met an obstacle that the next would meet \
         too.\n\n{notes}Read `ignite guide justiciar`, then fix the ticket so that a worker can \
         complete it: rewrite its acceptance, split it into tickets that can each be done, remove \
         a dependency it cannot have, or label it `needs:person` with an interim path. Changing \
         its body or acceptance, or `ignite ticket update {ticket} --release`, lets the pool take \
         it again.",
        releases.releases.len(),
        releases.sessions().len(),
    );
    let evidence = releases
        .releases
        .last()
        .map(|release| release.session.clone())
        .unwrap_or_default();
    let filing = Filing {
        signature: Signature::of_subject(&format!("{RELEASED_SUBJECT}:{ticket}"), LABEL),
        evidence,
        fields: fields(format!("{ticket} was released repeatedly: {title}"), body),
        deps: Vec::new(),
        released: true,
    };
    // Verification can finish after a release commits, before the justiciar filing.
    filing::file_if(conn, UPKEEP, &filing, now, |tx| {
        let Some(ticket) = tickets::load(tx, ticket)? else {
            return Ok(false);
        };
        Ok(ticket.status != Status::Closed && super::regressions::passing(tx, &ticket)?.is_none())
    })
}
