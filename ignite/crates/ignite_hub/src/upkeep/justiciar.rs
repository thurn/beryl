//! Justiciar tickets: P0 tickets for an emergency that stops work and that no ordinary ticket
//! will fix. They carry the `justiciar` label, which the dispatch queue offers before any other
//! ticket, and `tier:opus`, because the session that works one has to judge what is wrong and
//! may change anything to end it (`ignite guide justiciar`).
//!
//! Two sources:
//!
//! - A report (`POST /justiciar`) from a daemon that saw Ignite crash or one of its loops keep
//!   failing, or from a session that ran `ignite justiciar "<reason>"` ([`report`]).
//! - Upkeep, when `master` has not moved for [`MASTER_LAG_SECONDS`] although commits landed
//!   ([`master_lag`]). Tip verification is the only thing that moves `master`, so when it is
//!   stuck, for any reason, nothing reaches `master` and no fix to Ignite itself is released
//!   from it.
//!
//! Every filing goes through [`filing`] with a signature of its subject, so a repeated sighting
//! adds a comment to the open ticket and never a second ticket. Every justiciar ticket is filed
//! released ([`Filing::released`]): an emergency that stops work cannot wait for a person to
//! release its ticket.

use rusqlite::{Connection, OptionalExtension};

use ignite_core::owner::Owner;
use ignite_core::protocol::hub::JusticiarReport;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;

use super::filing::{self, Author, Filed, Filing, UPKEEP};
use super::parse_time;
use super::signature::Signature;

/// The label of a justiciar ticket.
pub const LABEL: &str = "justiciar";

/// The subject of the signature [`master_lag`] files under.
pub const MASTER_LAG_SUBJECT: &str = "master-lag";

/// The model tier a justiciar ticket asks for.
const TIER: &str = "tier:opus";

/// How long the oldest landing that `master` does not hold may wait before a justiciar ticket
/// is filed: 30 minutes. A tip round under load takes a few minutes, so half an hour without
/// `master` moving while landings wait is a fault, not a slow round.
pub const MASTER_LAG_SECONDS: f64 = 30.0 * 60.0;

/// Landings older than this are left out of the lag check, so history imported from before
/// the hub existed never counts as lag.
const LAG_HORIZON_SECONDS: f64 = 7.0 * 86_400.0;

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
    // The ticket is owned by the machine that reported it (see `filing`), and falls back to
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
        fields.owner = Some(Some(Owner::new(&report.host, None)));
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

/// Files a justiciar ticket when a landing has waited [`MASTER_LAG_SECONDS`] for `master`.
pub fn master_lag(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let Some((landed_at, ticket)) = oldest_unverified_landing(conn, now)? else {
        return Ok(Vec::new());
    };
    let waited = now.seconds_since(landed_at);
    if waited < MASTER_LAG_SECONDS {
        return Ok(Vec::new());
    }
    let minutes = waited / 60.0;
    let body = format!(
        "`master` has not moved for {minutes:.0} minutes: ticket {ticket} landed at {landed_at} \
         and no tip since has been verified. Tip verification is the only thing that moves \
         `master`, so nothing reaches it, and no fix to Ignite is released from it, until \
         this is fixed.\n\nLook at the tip loop's state in `ignite status` and the daemon \
         log on each machine, the open regression and fleet tickets, and the dashboard's \
         tips page. Read `ignite guide justiciar` before anything else."
    );
    let filing = Filing {
        signature: Signature::of_subject(MASTER_LAG_SUBJECT, LABEL),
        // One sighting per stuck landing: a new comment only when a later landing is the
        // oldest one waiting, which means `master` moved in between.
        evidence: ticket.clone(),
        fields: fields(
            format!("master has not moved for {minutes:.0} minutes"),
            body,
        ),
        deps: Vec::new(),
        released: true,
    };
    Ok(vec![filing::file(conn, UPKEEP, &filing, now)?])
}
