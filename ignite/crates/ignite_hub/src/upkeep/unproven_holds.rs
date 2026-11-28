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
//! - A check holds `master` back when it fails at a tip seen after the newest verified tip,
//!   either in a failed tip's verdict (`failure`) or in `observed_failures` on at least
//!   [`tip_streaks::STREAK`] tips (covering tips that were superseded before finishing or are
//!   still verifying). Its hold starts when the first such tip was `seen`, so slow tip rounds
//!   do not extend the hour.
//! - Checks whose regressions name the same first bad commit share the earliest hold among
//!   them. A landing that breaks many checks of a large family, such as every phone layout of
//!   the interface checks, is found a few checks per tip round, because each round runs only
//!   the checks its budget selects and stops at the first failures. Each new batch then waited
//!   an hour of its own, while the same landing had already held `master` back for hours. A
//!   check switched back on after that earliest hold began keeps its own start, so a fix that
//!   has not landed yet is not switched off again at once.
//! - Its record is `check_fix_outcomes`. Any `product` outcome is evidence of a real defect,
//!   except for a check in one of the `[rules] budgets` families, whose product fixes are
//!   performance work to get back under a limit.
//! - A protected check (`[rules] protected`) is never switched off, and neither is a check on a
//!   platform with no machine to verify it, since that one does not hold `master` back.
//! - The switch is recorded with the regression ticket, which gets a comment saying how to
//!   switch the check back on with its fix.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use ignite_core::config::{Os, Rules};
use ignite_core::protocol::hub::{CheckState, ResultKey, TipFailure};
use ignite_core::protocol::tickets::{BatchRequest, Operation};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::check_state::{self, Switch};
use ignite_store::hub::fleet_settings;
use ignite_store::hub::tip_streaks;
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
    /// The first bad commit its regression names, when its hold starts from an earlier failure
    /// of another check that the same commit broke.
    culprit: Option<String>,
    platform: Option<Os>,
    confirmed: bool,
    tips: usize,
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
        let held = match &hold.culprit {
            Some(culprit) => format!(
                "Its first bad commit {} has held master back since {}, and it failed at tip {}.",
                short(culprit),
                hold.since,
                short(&hold.tip),
            ),
            None => format!(
                "Held master back since {} (it failed at tip {}).",
                hold.since,
                short(&hold.tip),
            ),
        };
        let note = format!(
            "{held} No fix of this check has changed product behavior, so upkeep switched it off \
             instead of waiting for its repair."
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

/// Every check that failed at tips seen after the newest verified tip, with when the first such
/// tip was seen. A check observed only on superseded or still-verifying tips qualifies once it
/// has failed on at least [`tip_streaks::STREAK`] tips, so a one-off flake on a single
/// superseded tip is ignored.
///
/// A failing tip seen at or before the check's latest switch back to `on` is ignored, so a
/// repaired check is not immediately switched back off for the older failure it just fixed. A
/// pass recorded in `regression_passes` on a newer tip clears any earlier hold for that check.
fn holds(conn: &Connection) -> Result<BTreeMap<String, Hold>> {
    let re_enabled = latest_re_enables(conn)?;
    let mut select = conn.prepare(
        "SELECT commit_sha, seen, failure, observed_failures, regression_passes FROM tips
         WHERE (failure IS NOT NULL OR observed_failures IS NOT NULL OR regression_passes IS NOT NULL)
           AND seen > coalesce((SELECT max(seen) FROM tips WHERE state = 'verified'), '')
         ORDER BY seen, commit_sha",
    )?;
    let rows = select
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut holds: BTreeMap<String, Hold> = BTreeMap::new();
    for (tip, seen, failure, observed, passes) in rows {
        let since = parse_time(&seen)?;
        if let Some(passes) = passes {
            let passes: Vec<ResultKey> = serde_json::from_str(&passes)
                .map_err(|error| StoreError::Invalid(format!("a tip's passing checks: {error}")))?;
            for key in &passes {
                let clear = holds.get(key.check.as_str()).is_some_and(|hold| {
                    hold.platform.is_none()
                        || key.platform.is_none()
                        || hold.platform == key.platform
                });
                if clear {
                    holds.remove(key.check.as_str());
                }
            }
        }
        let mut seen_on_tip = BTreeSet::new();
        if let Some(failure) = failure {
            let failure: TipFailure = serde_json::from_str(&failure)
                .map_err(|error| StoreError::Invalid(format!("a tip's failed checks: {error}")))?;
            for key in &failure.checks {
                record_hold(
                    &mut holds,
                    &mut seen_on_tip,
                    &re_enabled,
                    &tip,
                    since,
                    key,
                    true,
                );
            }
        }
        if let Some(observed) = observed {
            let observed: Vec<ResultKey> = serde_json::from_str(&observed)
                .map_err(|error| StoreError::Invalid(format!("observed tip failures: {error}")))?;
            for key in &observed {
                record_hold(
                    &mut holds,
                    &mut seen_on_tip,
                    &re_enabled,
                    &tip,
                    since,
                    key,
                    false,
                );
            }
        }
    }
    holds.retain(|_, hold| hold.confirmed || hold.tips >= tip_streaks::STREAK);
    share_culprit_holds(conn, &mut holds, &re_enabled)?;
    Ok(holds)
}

/// Starts each hold at the earliest hold among the checks whose regressions name the same first
/// bad commit, unless the check was switched back on after that earlier start.
fn share_culprit_holds(
    conn: &Connection,
    holds: &mut BTreeMap<String, Hold>,
    re_enabled: &BTreeMap<String, Timestamp>,
) -> Result<()> {
    let mut first_bad: BTreeMap<String, String> = BTreeMap::new();
    let mut select = conn.prepare("SELECT check_name, first_bad FROM regressions")?;
    let rows = select.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (check, commit) = row?;
        if holds.contains_key(&check) {
            first_bad.insert(check, commit);
        }
    }
    let mut earliest: BTreeMap<&str, Timestamp> = BTreeMap::new();
    for (check, commit) in &first_bad {
        let since = holds[check].since;
        earliest
            .entry(commit.as_str())
            .and_modify(|at| *at = (*at).min(since))
            .or_insert(since);
    }
    for (check, commit) in &first_bad {
        let start = earliest[commit.as_str()];
        let hold = holds.get_mut(check).expect("listed from the holds");
        if start >= hold.since || re_enabled.get(check).is_some_and(|at| start <= *at) {
            continue;
        }
        hold.since = start;
        hold.culprit = Some(commit.clone());
    }
    Ok(())
}

fn latest_re_enables(conn: &Connection) -> Result<BTreeMap<String, Timestamp>> {
    let mut select = conn.prepare(
        "SELECT check_name, max(at) FROM check_state_changes WHERE state = 'on' GROUP BY check_name",
    )?;
    let rows = select
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut map = BTreeMap::new();
    for (check, at) in rows {
        map.insert(check, parse_time(&at)?);
    }
    Ok(map)
}

fn record_hold(
    holds: &mut BTreeMap<String, Hold>,
    seen_on_tip: &mut BTreeSet<String>,
    re_enabled: &BTreeMap<String, Timestamp>,
    tip: &str,
    since: Timestamp,
    key: &ResultKey,
    confirmed: bool,
) {
    if re_enabled
        .get(key.check.as_str())
        .is_some_and(|at| since <= *at)
    {
        return;
    }
    let name = key.check.to_string();
    let first_on_tip = seen_on_tip.insert(name.clone());
    let entry = holds.entry(name).or_insert_with(|| Hold {
        since,
        tip: tip.to_owned(),
        culprit: None,
        platform: key.platform,
        confirmed: false,
        tips: 0,
    });
    if confirmed {
        entry.confirmed = true;
    }
    if first_on_tip {
        entry.tips += 1;
    }
}

/// Whether a fix of `check` ever changed product behavior, counting only a fix outside the
/// budget families. A close that stamped checks from more than one family at once is ignored,
/// because a fix for one check on a multi-family or reopened ticket otherwise stamps every
/// unrelated co-filed check as having caught a product defect.
fn caught_a_defect(conn: &Connection, rules: &Rules, check: &str) -> Result<bool> {
    if rules.is_budget(check) {
        return Ok(false);
    }
    Ok(conn.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM check_fix_outcomes o
             WHERE o.check_name = ?1 AND o.outcome = 'product'
               AND NOT EXISTS (
                   SELECT 1 FROM check_fix_outcomes peer
                   WHERE peer.ticket = o.ticket AND peer.close_seq = o.close_seq
                     AND CASE WHEN instr(peer.check_name, ':') > 0
                              THEN substr(peer.check_name, 1, instr(peer.check_name, ':') - 1)
                              ELSE peer.check_name END
                      != CASE WHEN instr(o.check_name, ':') > 0
                              THEN substr(o.check_name, 1, instr(o.check_name, ':') - 1)
                              ELSE o.check_name END
               )
         )",
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
