//! For each kind of ticket the hub files by itself, whether the fault it reports still holds,
//! and if not, a sentence saying what shows it cleared. [`super::clearing`] acts on the answer.
//!
//! Kinds whose fault cannot be seen to clear answer [`Verdict::Unwatched`] and stay open until a
//! person or worker closes them:
//!
//! - `flaky`, `flaky-always`, `test-only-regression`: a worker must decide and apply a remedy.
//! - `machine-flaky`, `disagreement`: intermittent by nature, so a quiet spell proves nothing.
//! - `audit`: caching is switched off for the check, so the disagreement cannot be seen again.
//! - `found`: a bug that exploration found stays until it is fixed, and exploration may never
//!   replay that input.
//! - justiciar reports from a session and daemon faults without a recovery observation.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use ignite_core::fleet::{FleetSettings, MachineState};
use ignite_core::protocol::tickets::{Status, Ticket};
use ignite_core::time::Timestamp;
use ignite_core::tipround::{OVERHEAD_SHARE, TipRound};
use ignite_store::Result;
use ignite_store::hub::{fleet, fleet_settings, rows, tickets, tip_rounds};

use super::filing::{CHECKS_KEY, FAULT_KEY, SIGHTED_KEY, strings};
use super::platform_wait::{PlatformWait, WAIT_SCOPE_KEY};
use super::stuck::{self, StuckNow, TIP_WAIT_SECONDS};
use super::{justiciar, parse_time, regression_stalls, resource_stalls};
use crate::routes::tip_rounds as rounds_route;

/// How many tip rounds in a row, all after the last sighting, must be quick before a slow-round
/// ticket counts as cleared.
pub const QUICK_ROUNDS: u32 = 3;

/// How many of a cleared fault's checks its close reason names.
const NAMED_CHECKS: usize = 5;

/// The categories of the faults that hold checks back without a result
/// (`stuck_incomplete.rs`).
const INCOMPLETE: [&str; 6] = [
    "incomplete",
    "cannot-run",
    "hung",
    "build-failed",
    "invalid-entry",
    "incomplete-overflow",
];

/// Whether a ticket's fault still holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing upkeep can observe shows whether this kind of fault cleared.
    Unwatched,
    /// The fault still holds.
    Present,
    /// The fault cleared; the text completes "Ignite closed this ticket because ...".
    Cleared(String),
    /// Snapshot history already proves the fault stayed clear for the full waiting period.
    ConfirmedClear(String),
}

/// The parts of a ticket's signature, from its [`FAULT_KEY`] metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub subject: String,
    pub category: String,
    pub platform: String,
}

impl Fault {
    /// The ticket's fault, when it records one with a category.
    pub fn of(metadata: &BTreeMap<String, Value>) -> Option<Fault> {
        let fault = metadata.get(FAULT_KEY)?;
        let part = |key: &str| {
            fault
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let category = part("category");
        (!category.is_empty()).then(|| Fault {
            subject: part("subject"),
            category,
            platform: part("platform"),
        })
    }
}

/// What one upkeep pass reads once for every ticket it looks at.
pub struct World {
    now: Timestamp,
    stuck: StuckNow,
    machines: Vec<rows::Machine>,
    settings: FleetSettings,
    /// Some tip has waited [`TIP_WAIT_SECONDS`] or more for verification.
    tip_waiting: bool,
    master_lagging: bool,
    stalled_regressions: BTreeSet<String>,
    /// The newest rounds, newest first.
    rounds: Vec<TipRound>,
}

impl World {
    pub fn load(conn: &Connection, now: Timestamp) -> Result<World> {
        let cutoff = now.plus_seconds(-TIP_WAIT_SECONDS).to_string();
        let tip_waiting = conn
            .query_row(
                "SELECT 1 FROM tips WHERE state IN ('seen', 'verifying') AND seen <= ?1 LIMIT 1",
                params![cutoff],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let master_lagging = justiciar::master_lagging(conn, now)?;
        let stalled_regressions = if master_lagging {
            regression_stalls::open(conn, now)?
                .into_iter()
                .filter_map(|(ticket, stall)| stall.map(|_| ticket))
                .collect()
        } else {
            BTreeSet::new()
        };
        Ok(World {
            now,
            stuck: stuck::stuck_now(conn, now)?,
            machines: fleet::machines(conn)?,
            settings: fleet_settings::settings(conn)?,
            tip_waiting,
            master_lagging,
            stalled_regressions,
            rounds: tip_rounds::recent(conn, QUICK_ROUNDS)?,
        })
    }
}

/// When the ticket's newest sighting was recorded: its [`SIGHTED_KEY`], or when it was filed.
pub fn sighted_at(ticket: &Ticket) -> Timestamp {
    ticket
        .metadata
        .get(SIGHTED_KEY)
        .and_then(Value::as_str)
        .and_then(|at| Timestamp::parse(at).ok())
        .unwrap_or(ticket.created_at)
}

/// Whether the fault of `ticket` still holds.
pub fn verdict(
    conn: &Connection,
    world: &World,
    fault: &Fault,
    ticket: &Ticket,
) -> Result<Verdict> {
    let sighted = sighted_at(ticket);
    let subject = fault.subject.as_str();
    let present_unless = |holds: bool, cleared: &dyn Fn() -> Result<String>| {
        Ok(if holds {
            Verdict::Present
        } else {
            Verdict::Cleared(cleared()?)
        })
    };
    match fault.category.as_str() {
        "stuck-epic" => present_unless(world.stuck.epics.contains(subject), &|| {
            Ok(if status_of(conn, subject)? == Some(Status::Closed) {
                format!("epic {subject} is closed.")
            } else {
                format!(
                    "a ticket in epic {subject} can start, is in progress or waits for a person \
                     to release it."
                )
            })
        }),
        "stuck-cycle" => present_unless(world.stuck.cycles.contains(subject), &|| {
            Ok(format!(
                "the tickets {subject} no longer wait on one another in a loop."
            ))
        }),
        "held" => present_unless(world.stuck.held.contains(subject), &|| {
            Ok(match status_of(conn, subject)? {
                Some(Status::Closed) => format!("ticket {subject} is closed."),
                Some(Status::InProgress) => format!(
                    "ticket {subject} was claimed again, or its session is working and it has been \
                     held for less than six hours."
                ),
                _ => format!("ticket {subject} is no longer in progress."),
            })
        }),
        "unreachable" => unreachable(conn, world, subject, ticket, sighted),
        category if category == justiciar::LABEL && subject == justiciar::MASTER_LAG_SUBJECT => {
            present_unless(world.master_lagging, &|| {
                Ok("`master` moved: no landing has waited two hours for it since.".to_owned())
            })
        }
        category if category == justiciar::LABEL => {
            if let Some((invariant, id)) = resource_stalls::Invariant::from_subject(subject) {
                let Some(machine) = world.machines.iter().find(|machine| machine.id == id) else {
                    return Ok(Verdict::Present);
                };
                return Ok(
                    if resource_stalls::recovered(conn, machine, invariant, world.now)? {
                        Verdict::ConfirmedClear(format!(
                            "machine {} reported an hour of recovery.",
                            machine.name
                        ))
                    } else {
                        Verdict::Present
                    },
                );
            }
            if let Some((name, machine)) = subject.rsplit_once('@')
                && justiciar::watches(name)
            {
                return Ok(justiciar::daemon_verdict(name, machine, ticket));
            }
            let Some(regression) = subject.strip_prefix(justiciar::REGRESSION_STALL_SUBJECT) else {
                return Ok(Verdict::Unwatched);
            };
            present_unless(world.stalled_regressions.contains(regression), &|| {
                Ok(format!(
                    "regression ticket {regression} is no longer stalled while `master` waits."
                ))
            })
        }
        category if INCOMPLETE.contains(&category) => checks_reported(conn, ticket, fault, sighted),
        rounds_route::CATEGORY => Ok(quick_rounds(&world.rounds, sighted)),
        super::attachments::CATEGORY => super::attachments::verdict(conn, ticket),
        _ => Ok(Verdict::Unwatched),
    }
}

fn status_of(conn: &Connection, id: &str) -> Result<Option<Status>> {
    Ok(tickets::load(conn, id)?.map(|ticket| ticket.status))
}

/// A platform wait clears when its required work progresses or its tip is verified. For a
/// report of silence only, a new heartbeat and the absence of an old wait clear the fault.
fn unreachable(
    conn: &Connection,
    world: &World,
    subject: &str,
    ticket: &Ticket,
    sighted: Timestamp,
) -> Result<Verdict> {
    let named = world
        .machines
        .iter()
        .filter(|machine| match subject.strip_prefix("os:") {
            Some(os) => machine.os == os,
            None => machine.id == subject,
        });
    let on =
        |machine: &&rows::Machine| world.settings.machine(&machine.name).state != MachineState::Off;
    if named.clone().next().is_some() && !named.clone().any(|machine| on(&machine)) {
        return Ok(Verdict::Cleared(
            "the machines the tip waited on were turned off.".to_owned(),
        ));
    }
    let mut enabled = named.filter(on);
    if let Some(scope) = ticket
        .metadata
        .get(WAIT_SCOPE_KEY)
        .and_then(|value| serde_json::from_value::<PlatformWait>(value.clone()).ok())
    {
        if scope.verified(conn)? {
            return Ok(Verdict::Cleared(format!(
                "tip {} or a later tip verified.",
                scope.commit
            )));
        }
        if let Some(machine) = scope.working_machine(conn, enabled, world.now)? {
            return Ok(Verdict::Cleared(format!(
                "machine {} reported progress on the tip's {} work within the hour.",
                machine.name,
                scope.platform.as_str()
            )));
        }
        return Ok(Verdict::Present);
    }
    if world.tip_waiting {
        return Ok(Verdict::Present);
    }
    let back =
        enabled.find(|machine| parse_time(&machine.last_seen).is_ok_and(|seen| seen > sighted));
    Ok(match back {
        Some(machine) => Verdict::Cleared(format!(
            "machine {} reported at {} and no tip has waited an hour for verification since.",
            machine.name, machine.last_seen
        )),
        None => Verdict::Present,
    })
}

/// Every check the fault held back has a result stored after the last sighting.
fn checks_reported(
    conn: &Connection,
    ticket: &Ticket,
    fault: &Fault,
    sighted: Timestamp,
) -> Result<Verdict> {
    let checks = strings(&ticket.metadata, CHECKS_KEY);
    if checks.is_empty() {
        return Ok(Verdict::Unwatched);
    }
    // A fault filed without a platform (an overflow ticket, or a portable check) is cleared by a
    // result on any platform.
    let mut newest = conn.prepare_cached(
        "SELECT max(at) FROM results WHERE check_name = ?1 AND (?2 = '' OR platform = ?2)",
    )?;
    for check in &checks {
        let at: Option<String> =
            newest.query_row(params![check, fault.platform], |row| row.get(0))?;
        let reported = match at {
            Some(at) => parse_time(&at)? > sighted,
            None => false,
        };
        if !reported {
            return Ok(Verdict::Present);
        }
    }
    let mut named = checks
        .iter()
        .take(NAMED_CHECKS)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if checks.len() > NAMED_CHECKS {
        named = format!("{named} and {} more", checks.len() - NAMED_CHECKS);
    }
    Ok(Verdict::Cleared(format!(
        "every check it held back has a result again: {named}."
    )))
}

/// The last [`QUICK_ROUNDS`] rounds came after the last sighting, and none was slow.
fn quick_rounds(rounds: &[TipRound], sighted: Timestamp) -> Verdict {
    let enough = rounds.len() >= QUICK_ROUNDS as usize;
    if enough
        && rounds
            .iter()
            .all(|round| round.at > sighted && round.overhead().is_none())
    {
        Verdict::Cleared(format!(
            "the last {QUICK_ROUNDS} tip rounds each spent less than {:.0}% of their time on \
             listing families and Ignite's own work.",
            OVERHEAD_SHARE * 100.0
        ))
    } else {
        Verdict::Present
    }
}
