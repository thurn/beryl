//! The landings freeze: when `master` has fallen [`FREEZE_SECONDS`] behind `staging`, upkeep
//! stops new landings across the fleet until `master` catches up.
//!
//! The two-hour justiciar tickets of [`super::justiciar::master_lag`] did not end long stalls by
//! themselves. In November 2026 `master` fell more than six hours behind five times in eight
//! days, and three of those stalls lasted 15 to 27 hours. While landings continue, every new
//! landing moves the tip, so tip rounds are overtaken before they reach a verdict, culprit
//! searches do not finish, reverts have to be rebuilt on newer tips, and new breakages pile onto
//! the ones already failing. Holding the tip still lets the rounds finish and the fixes settle.
//!
//! The freeze is an ordinary landings-only emergency (`ignite_core::protocol::emergency`), so
//! every daemon already knows how to enforce it. Upkeep writes it directly, as
//! [`FREEZE_SESSION`], because no session declares it:
//!
//! - It files a justiciar ticket under the [`FREEZE_SUBJECT`] signature, whose worker gets
//!   `master` moving again. The emergency is declared on that ticket, so its worker can land
//!   and can end the freeze.
//! - It files a postmortem ticket under the [`POSTMORTEM_SUBJECT`] signature, which waits on the
//!   freeze's ticket. Its worker finds out how `master` fell six hours behind without the earlier
//!   escalations ending the stall, and fixes the workflow or infrastructure that let it happen.
//!   The freeze's own ticket used to carry this work as a second task, so it depended on that
//!   worker choosing to do it after `master` moved.
//! - Every pass renews the emergency, so it never lapses while the stall lasts, and lists every
//!   open regression, justiciar and incident ticket as allowed to land, so the fixes get in.
//!   A tip round's own reverts do not go through `ignite land` and are never refused.
//! - It ends once `master` holds every landing made before the freeze.
//! - Landing a fix under the freeze's ticket does not close it while the freeze's stall lasts
//!   ([`holds_open`]): the hub releases it instead, so the pool sends it to another worker
//!   if `master` still does not move. Its body says to close it once `master` moves, and
//!   `ignite land` closes the ticket it lands under, so in November 2026 a justiciar's last
//!   fix closed the ticket while `master` was still 71 landings behind, and the postmortem
//!   that waits on it started while the stall went on. Once `master` catches up, upkeep
//!   closes the ticket if no worker holds it.
//! - It is not declared while a tip waits on a machine that has stopped reporting, because
//!   stopping landings does not bring that machine back ([`super::stuck::tips_waiting`] files
//!   its ticket), or while another emergency is in force. The postmortem ticket is filed
//!   anyway: a stall of six hours needs one whether or not landings froze.
//! - Once a session ends or replaces the freeze, upkeep does not declare it again until
//!   `master` has caught up with the landings that were waiting when it froze.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::emergency::{Allowance, Emergency, EmergencyScope};
use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, EdgeKind, INCIDENT_LABEL, JUSTICIAR_LABEL, Operation, Status,
    TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch;
use ignite_store::hub::emergency;
use ignite_store::{Result, StoreError};

use super::filing::{self, Filed, Filing, UPKEEP};
use super::justiciar::oldest_unverified_landing;
use super::signature::Signature;
use super::stuck::TIP_WAIT_SECONDS;

/// How long the oldest landing on `staging` that `master` lacks may wait before landings
/// freeze: six hours. Of the five stalls past six hours in November 2026, two cleared by
/// themselves within 20 minutes and three lasted 15 hours or more, so a freeze at six hours
/// costs little when the stall was about to clear and saves most of a long one.
pub const FREEZE_SECONDS: f64 = 6.0 * 3600.0;

/// The session named as the freeze's declarer.
pub const FREEZE_SESSION: &str = "ignite-upkeep";

/// The subject of the signature the freeze's justiciar ticket is filed under.
pub const FREEZE_SUBJECT: &str = "master-freeze";

/// The subject of the signature the postmortem ticket of a six-hour stall is filed under.
pub const POSTMORTEM_SUBJECT: &str = "master-stall-postmortem";

/// The `meta` key holding the freeze upkeep declared, until `master` catches up.
const STATE_KEY: &str = "master_freeze";

/// The label of regression tickets, whose fixes may land during a freeze.
const REGRESSION_LABEL: &str = "regression";

/// The freeze upkeep declared, kept after a session ends it so that upkeep does not declare it
/// again for the same stall.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Freeze {
    /// The justiciar ticket the emergency is declared on.
    ticket: TicketId,
    /// When landings froze. The freeze ends once `master` holds every landing before this.
    since: Timestamp,
}

/// What one pass did about the freeze.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FreezeReport {
    /// The freeze's justiciar ticket and the stall's postmortem ticket, filed or commented on
    /// when `master` reached six hours behind.
    pub filed: Vec<Filed>,
    /// Whether this pass declared the freeze.
    pub frozen: bool,
    /// Whether this pass ended the freeze because `master` caught up.
    pub lifted: bool,
}

/// One pass: declares, renews or ends the landings freeze.
pub fn pass(conn: &mut Connection, now: Timestamp) -> Result<FreezeReport> {
    let mut report = FreezeReport::default();
    let session = session();
    let in_force = emergency::in_force(conn, now)?;
    if let Some(freeze) = state(conn)? {
        let ours = in_force
            .as_ref()
            .filter(|e| e.session == session && e.ticket == freeze.ticket);
        if caught_up(conn, freeze.since, now)? {
            if ours.is_some() {
                emergency::clear(conn)?;
                report.lifted = true;
            }
            caught_up_comment(conn, &freeze, ours.is_some(), now)?;
            set_state(conn, None)?;
        } else if let Some(current) = ours {
            emergency::impose(conn, &renewed(conn, current.clone(), now)?)?;
        }
        return Ok(report);
    }
    let Some((landed_at, landing)) = oldest_unverified_landing(conn, now)? else {
        return Ok(report);
    };
    let waited = now.seconds_since(landed_at);
    if waited < FREEZE_SECONDS {
        return Ok(report);
    }
    let stall = Stall {
        landing: &landing,
        landed_at,
        waited,
    };
    let skipped = if in_force.is_some() {
        Some("another emergency was already in force")
    } else if waiting_on_silent_machine(conn, now)? {
        Some("a tip was waiting on a machine that had stopped reporting")
    } else {
        None
    };
    if let Some(reason) = skipped {
        // Every pass of the same stall files the same evidence, which changes nothing after
        // the first.
        report
            .filed
            .push(file_postmortem(conn, &stall, Err(reason), now)?);
        return Ok(report);
    }
    let filed = file(conn, &stall, now)?;
    let ticket = TicketId::new(filed.ticket.as_str())
        .map_err(|error| StoreError::Invalid(format!("freeze ticket: {error}")))?;
    let declared = Emergency {
        ticket: ticket.clone(),
        session,
        machine: machine(),
        declared_at: now,
        renewed_at: now,
        scope: EmergencyScope::Landings,
        allowed: Vec::new(),
    };
    // The postmortem is filed before the emergency, so that it is among the justiciar tickets
    // the emergency lets land from the start.
    let postmortem = file_postmortem(conn, &stall, Ok(&filed.ticket), now)?;
    emergency::impose(conn, &renewed(conn, declared, now)?)?;
    set_state(conn, Some(&Freeze { ticket, since: now }))?;
    report.filed = vec![filed, postmortem];
    report.frozen = true;
    Ok(report)
}

fn session() -> SessionId {
    SessionId::new(FREEZE_SESSION).expect("the freeze session is a valid session ID")
}

fn machine() -> MachineId {
    MachineId::new(UPKEEP.machine).expect("the upkeep machine is a valid machine ID")
}

/// `emergency` renewed at `now`, allowing every open regression, justiciar and incident ticket.
fn renewed(conn: &Connection, mut emergency: Emergency, now: Timestamp) -> Result<Emergency> {
    emergency.renewed_at = now;
    emergency.allowed = fix_tickets(conn)?
        .into_iter()
        .map(|ticket| Allowance { ticket, verb: None })
        .collect();
    Ok(emergency)
}

/// The open tickets whose landings a freeze lets through: regressions, and the justiciar and
/// incident tickets that repair Ignite and the fleet.
fn fix_tickets(conn: &Connection) -> Result<Vec<TicketId>> {
    let mut select = conn.prepare_cached(
        "SELECT id FROM tickets WHERE status != 'closed' AND EXISTS
           (SELECT 1 FROM json_each(tickets.labels) WHERE value IN (?1, ?2, ?3))
         ORDER BY id",
    )?;
    let ids = select
        .query_map(
            params![REGRESSION_LABEL, JUSTICIAR_LABEL, INCIDENT_LABEL],
            |row| row.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter()
        .map(|id| {
            TicketId::new(id.as_str())
                .map_err(|error| StoreError::Invalid(format!("ticket {id}: {error}")))
        })
        .collect()
}

/// Whether `master` holds every landing made before `since`.
fn caught_up(conn: &Connection, since: Timestamp, now: Timestamp) -> Result<bool> {
    Ok(oldest_unverified_landing(conn, now)?.is_none_or(|(landed_at, _)| landed_at >= since))
}

/// Whether a tip has waited [`TIP_WAIT_SECONDS`] for verification while a machine that is not
/// switched off has not reported since the tip arrived: the case [`super::stuck::tips_waiting`]
/// files a ticket for. Freezing landings would not bring that machine back.
fn waiting_on_silent_machine(conn: &Connection, now: Timestamp) -> Result<bool> {
    let cutoff = now.plus_seconds(-TIP_WAIT_SECONDS).to_string();
    let oldest: Option<String> = conn
        .query_row(
            "SELECT seen FROM tips WHERE state IN ('seen', 'verifying') AND seen <= ?1
             ORDER BY seen LIMIT 1",
            [cutoff],
            |row| row.get(0),
        )
        .optional()?;
    let Some(seen) = oldest else {
        return Ok(false);
    };
    let seen = super::parse_time(&seen)?;
    let settings = ignite_store::hub::fleet_settings::settings(conn)?;
    for machine in ignite_store::hub::fleet::machines(conn)? {
        if settings.machine(&machine.name).state == ignite_core::fleet::MachineState::Off {
            continue;
        }
        if super::parse_time(&machine.last_seen)? < seen {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The landing `master` has waited longest for, which both tickets name.
#[derive(Clone, Copy)]
struct Stall<'a> {
    landing: &'a str,
    landed_at: Timestamp,
    /// Seconds since `landing` reached `staging`.
    waited: f64,
}

fn file(conn: &mut Connection, stall: &Stall<'_>, now: Timestamp) -> Result<Filed> {
    let Stall {
        landing,
        landed_at,
        waited,
    } = *stall;
    let hours = waited / 3600.0;
    let body = format!(
        "Landings are frozen across the fleet. `master` has not moved for {hours:.1} hours: \
         ticket {landing} landed on `staging` at {landed_at} and no tip since has been \
         verified.\n\n\
         While the freeze lasts, `ignite land` refuses every ticket except open regression, \
         justiciar and incident tickets and this one. The freeze ends by itself once `master` \
         holds every landing made before it, and you can end it sooner with `ignite pool resume \
         --all` while you hold this ticket.\n\n\
         Get `master` moving: fix or revert what fails at the tip, and defer a platform whose \
         machine is off. Record each mitigation on this ticket with what it fixed and the \
         evidence that led you to it, because a postmortem ticket waits on this one and starts \
         from that record. Landing a fix under this ticket releases it instead of closing it \
         while `master` is still behind, and upkeep closes it once `master` catches up; close \
         it yourself only when `master` has moved. Finding out why the stall happened is the \
         postmortem's work. Read `ignite guide justiciar` before anything else."
    );
    let filing = Filing {
        signature: Signature::of_subject(FREEZE_SUBJECT, JUSTICIAR_LABEL),
        evidence: format!("{landing}\n{now}"),
        fields: TicketFields {
            title: Some(format!(
                "Landings frozen: master has not moved for {hours:.0} hours"
            )),
            body: Some(body),
            ticket_type: Some(TicketType::Task),
            priority: Some(0),
            labels: Some(vec![JUSTICIAR_LABEL.to_owned(), "tier:high".to_owned()]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: true,
    };
    filing::file(conn, UPKEEP, &filing, now)
}

/// Files the stall's postmortem ticket. `freeze` is the freeze's ticket, which the postmortem
/// waits on, or why landings did not freeze.
fn file_postmortem(
    conn: &mut Connection,
    stall: &Stall<'_>,
    freeze: std::result::Result<&str, &str>,
    now: Timestamp,
) -> Result<Filed> {
    let Stall {
        landing,
        landed_at,
        waited,
    } = *stall;
    let hours = waited / 3600.0;
    let (context, deps) = match freeze {
        Ok(ticket) => (
            format!(
                "Upkeep froze landings and filed {ticket} to get `master` moving. This ticket \
                 waits on it, so read its mitigations and their evidence first."
            ),
            vec![Dependency {
                on: ticket.to_owned(),
                kind: Some(EdgeKind::Blocks),
            }],
        ),
        Err(reason) => (
            format!(
                "Upkeep did not freeze landings because {reason}. Find the ticket that is \
                 handling the stall and read its record first."
            ),
            Vec::new(),
        ),
    };
    let body = format!(
        "`master` fell {hours:.1} hours behind `staging`: ticket {landing} landed on `staging` at \
         {landed_at} and no tip had been verified since. {context}\n\n\
         This is a postmortem. Getting `master` moving is not your job; finding out why it \
         stopped and why nothing ended the stall sooner is. Before landings freeze, upkeep \
         escalates a stall by itself: tip rounds search for the culprit and revert it, \
         regression tickets go to the pool, and a justiciar ticket is filed once a landing has \
         waited two hours. Ignite and its fleet are meant to keep `master` within a few hours of \
         `staging` with no person involved, so a six-hour stall means part of that failed.\n\n\
         1. Write the timeline on this ticket from the tips, tip rounds, regression, justiciar and \
         incident tickets and the daemon logs: what broke `master`, when each escalation fired, \
         who worked it and what each did.\n\
         2. For every hour lost, name the cause: a check that failed or hung, a culprit search \
         or revert that did not finish, a machine that was off or busy, a ticket nobody took or \
         a worker that stalled, or an escalation that never fired.\n\
         3. Fix each workflow or infrastructure cause in Ignite, the checks or the fleet \
         settings, with a test that would have caught it, so the same stall cannot happen again. \
         A test or check that held `master` back is a candidate for deletion: delete it if it \
         is flaky, slow or has not caught a real bug that nothing else would, following `ignite \
         guide flaky-checks`, and record what still covers its behavior. File a ticket only for \
         a cause too large to fix here, and link it.\n\n\
         Close this ticket with the root causes and the guard now in place for each, or one \
         sentence per cause saying why nothing can guard it. Read `ignite guide justiciar` \
         before anything else."
    );
    let filing = Filing {
        signature: Signature::of_subject(POSTMORTEM_SUBJECT, JUSTICIAR_LABEL),
        evidence: landing.to_owned(),
        fields: TicketFields {
            title: Some(format!(
                "Postmortem: master fell {hours:.0} hours behind staging"
            )),
            body: Some(body),
            ticket_type: Some(TicketType::Task),
            priority: Some(1),
            labels: Some(vec![JUSTICIAR_LABEL.to_owned(), "tier:high".to_owned()]),
            ..TicketFields::default()
        },
        deps,
        released: true,
    };
    filing::file(conn, UPKEEP, &filing, now)
}

/// Whether landing `ticket` must leave it open: it is the ticket of a freeze whose stall has
/// not ended, because `master` still lacks a landing made before landings froze. The stored
/// freeze is deleted by the first pass after `master` catches up, so this is true until then.
pub fn holds_open(conn: &Connection, ticket: &str) -> Result<bool> {
    Ok(state(conn)?.is_some_and(|freeze| freeze.ticket.as_str() == ticket))
}

/// Says on the freeze's ticket that `master` caught up, and closes it if it is open and no
/// worker holds it. A worker that holds it closes it after recording its mitigations.
fn caught_up_comment(
    conn: &mut Connection,
    freeze: &Freeze,
    lifted: bool,
    now: Timestamp,
) -> Result<()> {
    let Some(ticket) = ignite_store::hub::tickets::load(conn, freeze.ticket.as_str())? else {
        return Ok(());
    };
    let open = ticket.status != Status::Closed;
    let close = open && ticket.assignee.is_none();
    if !lifted && !close {
        return Ok(());
    }
    let mut text = format!(
        "`master` now holds every landing made before landings froze at {}",
        freeze.since
    );
    text.push_str(if lifted {
        ", so landings resumed."
    } else {
        "."
    });
    if close {
        text.push_str(" No worker holds this ticket, so upkeep closed it.");
    } else if open {
        text.push_str(" Close this ticket once its mitigations are recorded.");
    }
    text.push_str(" The postmortem ticket that waits on it finds the cause of the stall.");
    let id = freeze.ticket.to_string();
    let mut ops = vec![Operation::Comment {
        id: id.clone(),
        text,
    }];
    if close {
        ops.push(Operation::Close {
            id,
            reason: "master caught up with the landings made before the freeze".to_owned(),
            include_ready: false,
        });
    }
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    batch::apply(conn, &request, UPKEEP.actor, UPKEEP.machine, now)?;
    Ok(())
}

fn state(conn: &Connection) -> Result<Option<Freeze>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [STATE_KEY],
            |row| row.get(0),
        )
        .optional()?;
    value
        .map(|json| {
            serde_json::from_str(&json)
                .map_err(|error| StoreError::Invalid(format!("the stored freeze: {error}")))
        })
        .transpose()
}

fn set_state(conn: &Connection, freeze: Option<&Freeze>) -> Result<()> {
    match freeze {
        Some(freeze) => {
            let json = serde_json::to_string(freeze)
                .map_err(|error| StoreError::Invalid(format!("freeze: {error}")))?;
            conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![STATE_KEY, json],
            )?;
        }
        None => {
            conn.execute("DELETE FROM meta WHERE key = ?1", [STATE_KEY])?;
        }
    }
    Ok(())
}
