//! Switching a check on or off by hand (`ignite flaky on|off`), the record of who did it, and the
//! list of checks that are off, have flaked or were switched by hand.
//!
//! The hub switches a check off by itself after its second flaky record (see the hub's flaky
//! upkeep). Switching it back on by hand does not bring those records back: each one already has
//! its ticket and no longer counts, so the check is switched off again only after two new ones.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::ids::CheckName;
use ignite_core::protocol::hub::{CheckState, FlakyCheck, HandSwitch};
use ignite_core::time::Timestamp;

use crate::Result;
use crate::hub::checks::{not_machine_fault, state_from, state_text};
use crate::hub::results::parse;

/// One switch by hand.
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

/// The newest ticket filed for the check's own flaky records: not a ticket for a machine on
/// which many checks flaked at once.
pub fn flaky_ticket(conn: &Connection, check: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT filed FROM flaky
                 WHERE check_name = ?1 AND filed IS NOT NULL AND {}
                 ORDER BY at DESC LIMIT 1",
                not_machine_fault()
            ),
            params![check],
            |row| row.get(0),
        )
        .optional()?)
}

/// The last switch of `check` by hand.
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

/// Every check that is switched off, has a flaky record about itself or was switched by hand,
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
