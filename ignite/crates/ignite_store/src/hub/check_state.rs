//! Switching a check on or off, the record of who did it, and the list that `ignite flaky`
//! reports.
//!
//! The hub switches an ordinary check off after its first flake with a known command or a
//! test-only regression close. Once upkeep files the remedy, that sighting is consumed.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use ignite_core::ids::CheckName;
use ignite_core::protocol::hub::{CheckState, FlakyCheck, HandSwitch};
use ignite_core::time::Timestamp;

use crate::Result;
use crate::hub::checks::{not_machine_fault, state_from, state_text};
use crate::hub::flaky_triage;
use crate::hub::meta;
use crate::hub::results::parse;

/// One recorded state change.
#[derive(Clone, Copy, Debug)]
pub struct Switch<'a> {
    pub check: &'a str,
    pub state: CheckState,
    pub actor: &'a str,
    pub machine: &'a str,
    pub note: &'a str,
    /// The flaky ticket the change is also recorded on.
    pub ticket: Option<&'a str>,
    pub at: Timestamp,
}

/// The check's state, or `None` if no family lists a check by that name.
pub fn state(conn: &Connection, check: &str) -> Result<Option<CheckState>> {
    let text: Option<String> = conn
        .query_row(
            "SELECT state FROM checks WHERE name = ?1",
            params![check],
            |row| row.get(0),
        )
        .optional()?;
    text.as_deref().map(state_from).transpose()
}

/// Sets the check's state and records who did it. Returns the state it had before, or `None`
/// (writing nothing) if there is no such check. A check already in that state is left alone and
/// nothing is recorded. Run it in a transaction when something else, such as a comment on the
/// ticket, must be written with it.
pub fn switch(conn: &Connection, switch: &Switch) -> Result<Option<CheckState>> {
    let Some(previous) = state(conn, switch.check)? else {
        return Ok(None);
    };
    if previous == switch.state {
        return Ok(Some(previous));
    }
    conn.execute(
        "UPDATE checks SET state = ?2 WHERE name = ?1",
        params![switch.check, state_text(switch.state)],
    )?;
    conn.execute(
        "INSERT INTO check_state_changes
           (check_name, state, previous, actor, machine, note, ticket, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            switch.check,
            state_text(switch.state),
            state_text(previous),
            switch.actor,
            switch.machine,
            switch.note,
            switch.ticket,
            switch.at.to_string(),
        ],
    )?;
    Ok(Some(previous))
}

/// Restores checks disabled by a filed imported verdict with no independent evidence.
/// Upkeep used to accept verdicts that had neither a command ID nor failure output. The old
/// importer stored a Git tree ID (or an empty hash), while native reports have a 64-digit
/// input hash, even if they have no command ID. An automatic flaky remedy must name the check.
/// Any native report, queued or filed test-only remedy, or recorded switch leaves it alone.
pub fn recover_imported_flakes(
    conn: &mut Connection,
    actor: &str,
    machine: &str,
    now: Timestamp,
) -> Result<Vec<String>> {
    const RECOVERY: &str = "recovered_imported_flakes";
    if meta::get::<bool>(conn, RECOVERY)? == Some(true) {
        return Ok(Vec::new());
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if meta::get::<bool>(&tx, RECOVERY)? == Some(true) {
        return Ok(Vec::new());
    }
    let candidates: Vec<(String, String)> = {
        let mut select = tx.prepare(
            "SELECT f.check_name, min(f.filed) FROM flaky f
             JOIN checks c ON c.name = f.check_name
             JOIN tickets t ON t.id = f.filed
             WHERE c.state = 'off' AND t.created_by = ?1
               AND t.metadata -> 'ignite_fault' ->> 'category' IN ('flaky', ?2)
               AND EXISTS (SELECT 1 FROM json_each(t.metadata, '$.ignite_checks')
                           WHERE value = f.check_name)
               AND NOT EXISTS (SELECT 1 FROM flaky other WHERE other.check_name = f.check_name
                   AND (length(other.hash) = 64 OR other.command != ''
                        OR other.output != '' OR other.always != 0))
               AND NOT EXISTS (SELECT 1 FROM check_fix_outcomes o WHERE o.check_name = f.check_name
                   AND o.remedy_ticket IS NOT NULL)
               AND NOT EXISTS (SELECT 1 FROM check_state_changes s WHERE s.check_name = f.check_name)
             GROUP BY f.check_name ORDER BY f.check_name",
        )?;
        select
            .query_map(params![actor, flaky_triage::CATEGORY], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?
    };
    for (check, ticket) in &candidates {
        switch(
            &tx,
            &Switch {
                check,
                state: CheckState::On,
                actor,
                machine,
                note: "Restored after an imported flake verdict without command provenance.",
                ticket: Some(ticket),
                at: now,
            },
        )?;
    }
    meta::put(&tx, RECOVERY, &true)?;
    tx.commit()?;
    Ok(candidates.into_iter().map(|(check, _)| check).collect())
}

/// The newest remedy ticket for the check's own failure, excluding machine faults.
pub fn flaky_ticket(conn: &Connection, check: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT ticket FROM (
                   SELECT filed AS ticket, at FROM flaky
                   WHERE check_name = ?1 AND filed IS NOT NULL AND {}
                   UNION ALL
                   SELECT remedy_ticket, at FROM check_fix_outcomes
                   WHERE check_name = ?1 AND remedy_ticket != ''
                 ) ORDER BY at DESC LIMIT 1",
                not_machine_fault()
            ),
            params![check],
            |row| row.get(0),
        )
        .optional()?)
}

/// The last recorded switch of `check`.
fn last_switch(conn: &Connection, check: &str) -> Result<Option<HandSwitch>> {
    let row: Option<(String, String, String, String, String)> = conn
        .prepare_cached(
            "SELECT state, actor, machine, at, note FROM check_state_changes
             WHERE check_name = ?1 ORDER BY id DESC LIMIT 1",
        )?
        .query_row(params![check], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .optional()?;
    row.map(|(state, actor, machine, at, note)| {
        Ok(HandSwitch {
            state: state_from(&state)?,
            actor,
            machine,
            at: parse("time", &at)?,
            note,
        })
    })
    .transpose()
}

/// Every check that is switched off, has a flaky record about itself or has a recorded switch,
/// by name.
pub fn flaky_checks(conn: &Connection) -> Result<Vec<FlakyCheck>> {
    let rows: Vec<(String, String, String, u32, Option<String>)> = {
        let mut select = conn.prepare(&format!(
            "SELECT c.name, c.family, c.state,
                    (SELECT COUNT(*) FROM flaky f WHERE f.check_name = c.name AND {fault}),
                    (SELECT MAX(at) FROM flaky f WHERE f.check_name = c.name AND {fault})
             FROM checks c
             WHERE c.state = 'off'
                OR EXISTS (SELECT 1 FROM flaky f WHERE f.check_name = c.name AND {fault})
                OR EXISTS (SELECT 1 FROM check_state_changes s WHERE s.check_name = c.name)
                OR EXISTS (SELECT 1 FROM check_fix_outcomes o
                           WHERE o.check_name = c.name AND o.remedy_ticket != '')
             ORDER BY c.name",
            fault = not_machine_fault()
        ))?;
        select
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?
    };
    rows.into_iter()
        .map(|(name, family, state, flaky_records, last_flaky)| {
            Ok(FlakyCheck {
                check: parse::<CheckName>("check", &name)?,
                family,
                state: state_from(&state)?,
                flaky_records,
                last_flaky: last_flaky
                    .as_deref()
                    .map(|at| parse("time", at))
                    .transpose()?,
                ticket: flaky_ticket(conn, &name)?,
                switched: last_switch(conn, &name)?,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "check_state_tests.rs"]
mod tests;
