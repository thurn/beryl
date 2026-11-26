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
//! - It files a justiciar ticket under the [`FREEZE_SUBJECT`] signature, whose worker finds and
//!   fixes what let `master` fall so far behind. The emergency is declared on that ticket, so
//!   its worker can land and can end the freeze.
//! - Every pass renews the emergency, so it never lapses while the stall lasts, and lists every
//!   open regression, justiciar and incident ticket as allowed to land, so the fixes get in.
//!   A tip round's own reverts do not go through `ignite land` and are never refused.
//! - It ends once `master` holds every landing made before the freeze. The ticket stays open
//!   for its root-cause work.
//! - It is not declared while a tip waits on a machine that has stopped reporting, because
//!   stopping landings does not bring that machine back ([`super::stuck::tips_waiting`] files
//!   its ticket), or while another emergency is in force.
//! - Once a session ends or replaces the freeze, upkeep does not declare it again until
//!   `master` has caught up with the landings that were waiting when it froze.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::emergency::{Allowance, Emergency, EmergencyScope};
use ignite_core::protocol::tickets::{
    BatchRequest, INCIDENT_LABEL, JUSTICIAR_LABEL, Operation, TicketFields, TicketType,
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
    /// The justiciar ticket filed or commented on when landings froze.
    pub filed: Vec<Filed>,
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
                comment(conn, &freeze, now)?;
                report.lifted = true;
            }
            set_state(conn, None)?;
        } else if let Some(current) = ours {
            emergency::impose(conn, &renewed(conn, current.clone(), now)?)?;
        }
        return Ok(report);
    }
    if in_force.is_some() {
        return Ok(report);
    }
    let Some((landed_at, landing)) = oldest_unverified_landing(conn, now)? else {
        return Ok(report);
    };
    let waited = now.seconds_since(landed_at);
    if waited < FREEZE_SECONDS || waiting_on_silent_machine(conn, now)? {
        return Ok(report);
    }
    let filed = file(conn, &landing, landed_at, waited, now)?;
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
    emergency::impose(conn, &renewed(conn, declared, now)?)?;
    set_state(conn, Some(&Freeze { ticket, since: now }))?;
    report.filed.push(filed);
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

fn file(
    conn: &mut Connection,
    landing: &str,
    landed_at: Timestamp,
    waited: f64,
    now: Timestamp,
) -> Result<Filed> {
    let hours = waited / 3600.0;
    let body = format!(
        "Landings are frozen across the fleet. `master` has not moved for {hours:.1} hours: \
         ticket {landing} landed on `staging` at {landed_at} and no tip since has been \
         verified.\n\n\
         While the freeze lasts, `ignite land` refuses every ticket except open regression, \
         justiciar and incident tickets and this one. The freeze ends by itself once `master` \
         holds every landing made before it, and you can end it sooner with `ignite pool resume \
         --all` while you hold this ticket.\n\n\
         Do two things. First, get `master` moving: fix or revert what fails at the tip, and \
         defer a platform whose machine is off. Second, find out why `master` fell this far \
         behind without the earlier escalations ending it, and fix that in Ignite or file a \
         ticket for it. Record each mitigation with its root cause on this ticket. Read \
         `ignite guide justiciar` before anything else."
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

/// Says on the freeze's ticket that landings resumed.
fn comment(conn: &mut Connection, freeze: &Freeze, now: Timestamp) -> Result<()> {
    let text = format!(
        "`master` now holds every landing made before landings froze at {}, so landings resumed \
         at {now}. This ticket stays open until the cause of the stall is fixed or filed.",
        freeze.since
    );
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Comment {
            id: freeze.ticket.to_string(),
            text,
        }],
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
