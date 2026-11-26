//! Switches off a check that has held `master` back for an hour when it has no record of
//! catching a real defect.
//!
//! A check that fails at the tip stops `master` from moving until a fix lands, however much
//! product work waits behind it. The record says whether that wait is worth it: a check whose
//! fixes have only ever changed the test, a threshold, a budget or a fixture has never caught a
//! fault a player or caller could see. Upkeep switches such a check off once it has held
//! `master` back for [`HOLD_SECONDS`], so the waiting work promotes while its regression ticket
//! stays open. The stall revert (`[revert] stall_minutes`) comes later and would revert a product
//! commit instead, so for these checks deferring is the cheaper remedy.
//!
//! - A check holds `master` back when a tip that failed it was seen after the newest verified
//!   tip. Its hold starts when the first such tip finished verification.
//! - Its record is `check_fix_outcomes`. Any `product` outcome is evidence of a real defect,
//!   except for a check in one of the `[rules] budgets` families, whose product fixes are
//!   performance work to get back under a limit.
//! - A protected check (`[rules] protected`) is never switched off, and neither is a check on a
//!   platform with no machine to verify it, since that one does not hold `master` back.
//! - The switch is recorded with the regression ticket, which gets a comment saying how to
//!   switch the check back on with its fix.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use ignite_core::config::{Os, Rules};
use ignite_core::protocol::hub::{CheckState, TipFailure};
use ignite_core::protocol::tickets::{BatchRequest, Operation};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::check_state::{self, Switch};
use ignite_store::hub::fleet_settings;
use ignite_store::{Result, StoreError};

use super::filing::UPKEEP;
use super::parse_time;

/// How long a check with no record of a real defect may hold `master` back. An hour is about
/// two tip rounds under load, so a fix already on its way usually lands first, and it is half
/// the stall revert's 120 minutes, so the check is switched off before its culprit is reverted.
pub(super) const HOLD_SECONDS: f64 = 3600.0;

/// One check failing at tips seen since the newest verified tip.
struct Hold {
    since: Timestamp,
    tip: String,
    platform: Option<Os>,
}

/// Switches off each qualifying check and returns their names.
pub(super) fn switch_off(
    conn: &mut Connection,
    rules: &Rules,
    now: Timestamp,
) -> Result<Vec<String>> {
    let holds = holds(conn)?;
    if holds.is_empty() {
        return Ok(Vec::new());
    }
    let settings = fleet_settings::settings(conn)?;
    let unavailable = fleet_settings::unavailable_platforms(conn, &settings, now)?;
    let mut switched = Vec::new();
    for (check, hold) in holds {
        if now.seconds_since(hold.since) < HOLD_SECONDS
            || rules.is_protected(&check)
            || hold.platform.is_some_and(|os| unavailable.contains(&os))
        {
            continue;
        }
        let mut tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if check_state::state(&tx, &check)? != Some(CheckState::On)
            || caught_a_defect(&tx, rules, &check)?
        {
            continue;
        }
        let ticket = regression_ticket(&tx, &check)?;
        let note = format!(
            "Held master back since {} (it failed at tip {}). No fix of this check has changed \
             product behavior, so upkeep switched it off instead of waiting for its repair.",
            hold.since,
            short(&hold.tip),
        );
        check_state::switch(
            &tx,
            &Switch {
                check: &check,
                state: CheckState::Off,
                actor: UPKEEP.actor,
                machine: UPKEEP.machine,
                note: &note,
                ticket: ticket.as_deref(),
                at: now,
            },
        )?;
        if let Some(ticket) = &ticket {
            comment(&mut tx, ticket, &check, &note, now)?;
        }
        tx.commit()?;
        switched.push(check);
    }
    Ok(switched)
}

/// Every check that failed at a tip seen after the newest verified tip, with when it first did.
fn holds(conn: &Connection) -> Result<BTreeMap<String, Hold>> {
    let mut select = conn.prepare(
        "SELECT commit_sha, verified, failure FROM tips
         WHERE state = 'failed' AND failure IS NOT NULL AND verified IS NOT NULL
           AND seen > coalesce((SELECT max(seen) FROM tips WHERE state = 'verified'), '')
         ORDER BY verified, commit_sha",
    )?;
    let rows = select
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut holds = BTreeMap::new();
    for (tip, verified, failure) in rows {
        let failure: TipFailure = serde_json::from_str(&failure)
            .map_err(|error| StoreError::Invalid(format!("a tip's failed checks: {error}")))?;
        let since = parse_time(&verified)?;
        for key in failure.checks {
            holds.entry(key.check.to_string()).or_insert_with(|| Hold {
                since,
                tip: tip.clone(),
                platform: key.platform,
            });
        }
    }
    Ok(holds)
}

/// Whether a fix of `check` ever changed product behavior, counting only a fix outside the
/// budget families.
fn caught_a_defect(conn: &Connection, rules: &Rules, check: &str) -> Result<bool> {
    if rules.is_budget(check) {
        return Ok(false);
    }
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM check_fix_outcomes
                        WHERE check_name = ?1 AND outcome = 'product')",
        [check],
        |row| row.get(0),
    )?)
}

/// The open regression ticket for `check`, if one was filed.
fn regression_ticket(conn: &Connection, check: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT r.ticket FROM regressions r JOIN tickets t ON t.id = r.ticket
             WHERE r.check_name = ?1 AND t.status != 'closed'",
            [check],
            |row| row.get(0),
        )
        .optional()?)
}

fn comment(
    tx: &mut Transaction<'_>,
    ticket: &str,
    check: &str,
    note: &str,
    now: Timestamp,
) -> Result<()> {
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Comment {
            id: ticket.to_owned(),
            text: format!(
                "{check}: {note} Fix it as this ticket asks, or delete it, and switch it back on \
                 with `ignite flaky on {check} -m \"<what you changed>\"` in the same session."
            ),
        }],
    };
    match batch::apply_in(tx, &request, UPKEEP.actor, UPKEEP.machine, now)? {
        BatchOutcome::Applied(_) => Ok(()),
        BatchOutcome::Conflict(conflict) => Err(StoreError::Rejected(format!(
            "commenting on {ticket} conflicted at operation {}",
            conflict.failed
        ))),
    }
}

fn short(commit: &str) -> &str {
    commit.get(..9).unwrap_or(commit)
}
