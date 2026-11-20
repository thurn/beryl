//! CLI mode's rows (see `schema_20.sql`): worker slots, their turns, the messages waiting for a
//! turn, each app's CLI hold, and the mode override `ignite pool mode` stores.
//!
//! [`record_turn_start`] is the first phase of starting a turn: it records the turn and marks
//! the messages its prompt carries as delivered in one transaction, before the supervisor
//! starts. A second record of the same turn fails, so a turn is never recorded twice.

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::Result;
use crate::local::rows::{CliHold, CliMessage, CliSlot, CliTurn};

/// The `pool_settings` key of an app's mode override.
fn mode_key(app: &str) -> String {
    format!("mode.{app}")
}

fn columns(names: &[&str]) -> String {
    names.join(", ")
}

fn read_slot(row: &Row<'_>) -> rusqlite::Result<CliSlot> {
    Ok(CliSlot {
        app: row.get("app")?,
        slot: row.get("slot")?,
        session: row.get("session")?,
        ticket: row.get("ticket")?,
        state: row.get("state")?,
        turn: row.get("turn")?,
        transcript: row.get("transcript")?,
        model: row.get("model")?,
        effort: row.get("effort")?,
        since: row.get("since")?,
        reservation: row.get("reservation")?,
        next_prompt: row.get("next_prompt")?,
        wait_handle: row.get("wait_handle")?,
        resume_at: row.get("resume_at")?,
        park_reason: row.get("park_reason")?,
        interrupted_in_row: row.get("interrupted_in_row")?,
        block_sent: row.get("block_sent")?,
        backed_off: row.get("backed_off")?,
    })
}

fn read_turn(row: &Row<'_>) -> rusqlite::Result<CliTurn> {
    Ok(CliTurn {
        session: row.get("session")?,
        turn: row.get("turn")?,
        app: row.get("app")?,
        slot: row.get("slot")?,
        ticket: row.get("ticket")?,
        prompt: row.get("prompt")?,
        state: row.get("state")?,
        run: row.get("run")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        exit: row.get("exit")?,
        end_kind: row.get("end_kind")?,
        transcript: row.get("transcript")?,
        model: row.get("model")?,
        cli_version: row.get("cli_version")?,
        cost_usd: row.get("cost_usd")?,
        lines_applied: row.get("lines_applied")?,
    })
}

fn read_message(row: &Row<'_>) -> rusqlite::Result<CliMessage> {
    Ok(CliMessage {
        id: row.get("id")?,
        session: row.get("session")?,
        ticket: row.get("ticket")?,
        text: row.get("text")?,
        urgent: row.get("urgent")?,
        queued_at: row.get("queued_at")?,
        delivered_turn: row.get("delivered_turn")?,
    })
}

fn read_hold(row: &Row<'_>) -> rusqlite::Result<CliHold> {
    Ok(CliHold {
        app: row.get("app")?,
        reason: row.get("reason")?,
        since: row.get("since")?,
        cli_version: row.get("cli_version")?,
        message: row.get("message")?,
        probed_at: row.get("probed_at")?,
        reported: row.get("reported")?,
    })
}

/// Inserts the slot or replaces every column of the existing row for its app and number.
pub fn put(conn: &Connection, slot: &CliSlot) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO cli_slots ({}) VALUES \
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            columns(CliSlot::COLUMNS)
        ),
        params![
            slot.app,
            slot.slot,
            slot.session,
            slot.ticket,
            slot.state,
            slot.turn,
            slot.transcript,
            slot.model,
            slot.effort,
            slot.since,
            slot.reservation,
            slot.next_prompt,
            slot.wait_handle,
            slot.resume_at,
            slot.park_reason,
            slot.interrupted_in_row,
            slot.block_sent,
            slot.backed_off,
        ],
    )?;
    Ok(())
}

/// Every slot, ordered by app and number.
pub fn list(conn: &Connection) -> Result<Vec<CliSlot>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM cli_slots ORDER BY app, slot",
        columns(CliSlot::COLUMNS)
    ))?;
    let slots = statement
        .query_map([], read_slot)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(slots)
}

/// The slot working `session`, if any.
pub fn by_session(conn: &Connection, session: &str) -> Result<Option<CliSlot>> {
    let slot = conn
        .query_row(
            &format!(
                "SELECT {} FROM cli_slots WHERE session = ?1",
                columns(CliSlot::COLUMNS)
            ),
            [session],
            read_slot,
        )
        .optional()?;
    Ok(slot)
}

/// Records `turn` as starting and marks the messages `deliver` as carried by it, in one
/// transaction: the turn's row is inserted, the slot of its session takes its number with the
/// state `starting` and no pending prompt, and each message not yet delivered gets the turn's
/// number. Fails, changing nothing, when the session already has a row for that turn.
pub fn record_turn_start(conn: &mut Connection, turn: &CliTurn, deliver: &[i64]) -> Result<()> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        &format!(
            "INSERT INTO cli_turns ({}) VALUES \
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            columns(CliTurn::COLUMNS)
        ),
        params![
            turn.session,
            turn.turn,
            turn.app,
            turn.slot,
            turn.ticket,
            turn.prompt,
            turn.state,
            turn.run,
            turn.started_at,
            turn.ended_at,
            turn.exit,
            turn.end_kind,
            turn.transcript,
            turn.model,
            turn.cli_version,
            turn.cost_usd,
            turn.lines_applied,
        ],
    )?;
    transaction.execute(
        "UPDATE cli_slots SET turn = ?2, state = 'starting', since = ?3, next_prompt = NULL \
         WHERE session = ?1",
        params![turn.session, turn.turn, turn.started_at],
    )?;
    {
        let mut update = transaction.prepare(
            "UPDATE cli_messages SET delivered_turn = ?2 \
             WHERE id = ?1 AND delivered_turn IS NULL",
        )?;
        for id in deliver {
            update.execute(params![id, turn.turn])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// Every turn of `session`, oldest first.
pub fn turns(conn: &Connection, session: &str) -> Result<Vec<CliTurn>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM cli_turns WHERE session = ?1 ORDER BY turn",
        columns(CliTurn::COLUMNS)
    ))?;
    let turns = statement
        .query_map([session], read_turn)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(turns)
}

/// Queues a message for a session's next turn and returns its `id` (the given `id` and
/// `delivered_turn` are ignored).
pub fn queue_message(conn: &Connection, message: &CliMessage) -> Result<i64> {
    conn.execute(
        "INSERT INTO cli_messages (session, ticket, text, urgent, queued_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            message.session,
            message.ticket,
            message.text,
            message.urgent,
            message.queued_at
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// The messages of `session` no turn has carried yet, in the order they were queued.
pub fn undelivered(conn: &Connection, session: &str) -> Result<Vec<CliMessage>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM cli_messages WHERE session = ?1 AND delivered_turn IS NULL ORDER BY id",
        columns(CliMessage::COLUMNS)
    ))?;
    let messages = statement
        .query_map([session], read_message)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(messages)
}

/// Every app's CLI hold, ordered by app.
pub fn holds(conn: &Connection) -> Result<Vec<CliHold>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM cli_holds ORDER BY app",
        columns(CliHold::COLUMNS)
    ))?;
    let holds = statement
        .query_map([], read_hold)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(holds)
}

/// The mode `ignite pool mode` forced on `app` (`cli` or `desktop`), or `None` when
/// `ignite.toml` decides.
pub fn mode_override(conn: &Connection, app: &str) -> Result<Option<String>> {
    let value = conn
        .query_row(
            "SELECT value FROM pool_settings WHERE key = ?1",
            [mode_key(app)],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value)
}

/// Forces `app`'s mode, or with `None` removes the override.
pub fn set_mode_override(conn: &Connection, app: &str, mode: Option<&str>) -> Result<()> {
    match mode {
        Some(mode) => conn.execute(
            "INSERT OR REPLACE INTO pool_settings (key, value) VALUES (?1, ?2)",
            params![mode_key(app), mode],
        )?,
        None => conn.execute("DELETE FROM pool_settings WHERE key = ?1", [mode_key(app)])?,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    fn slot(session: &str) -> CliSlot {
        CliSlot {
            app: "claude".to_owned(),
            slot: 1,
            session: Some(session.to_owned()),
            ticket: Some("T.1".to_owned()),
            state: "free".to_owned(),
            turn: 0,
            transcript: None,
            model: Some("opus".to_owned()),
            effort: Some("high".to_owned()),
            since: None,
            reservation: Some("code".to_owned()),
            next_prompt: Some("Run `ignite start T.1`".to_owned()),
            wait_handle: None,
            resume_at: None,
            park_reason: None,
            interrupted_in_row: 0,
            block_sent: 0,
            backed_off: 0,
        }
    }

    fn turn(session: &str, n: i64) -> CliTurn {
        CliTurn {
            session: session.to_owned(),
            turn: n,
            app: "claude".to_owned(),
            slot: 1,
            ticket: Some("T.1".to_owned()),
            prompt: "go".to_owned(),
            state: "starting".to_owned(),
            run: None,
            started_at: Some("2026-11-20T12:00:00.000Z".to_owned()),
            ended_at: None,
            exit: None,
            end_kind: None,
            transcript: None,
            model: None,
            cli_version: None,
            cost_usd: None,
            lines_applied: 0,
        }
    }

    fn message(session: &str, text: &str) -> CliMessage {
        CliMessage {
            id: 0,
            session: session.to_owned(),
            ticket: Some("T.1".to_owned()),
            text: text.to_owned(),
            urgent: 0,
            queued_at: None,
            delivered_turn: None,
        }
    }

    #[test]
    fn a_turn_is_recorded_once_with_the_messages_it_delivers() {
        let mut conn = open_in_memory().unwrap();
        let session = "cli-claude-0a1b2c3d";
        put(&conn, &slot(session)).unwrap();
        let first = queue_message(&conn, &message(session, "one")).unwrap();
        let second = queue_message(&conn, &message(session, "two")).unwrap();
        assert_eq!(undelivered(&conn, session).unwrap().len(), 2);

        record_turn_start(&mut conn, &turn(session, 1), &[first]).unwrap();
        let stored = by_session(&conn, session).unwrap().unwrap();
        assert_eq!(
            (stored.turn, stored.state.as_str(), stored.next_prompt),
            (1, "starting", None)
        );
        let waiting = undelivered(&conn, session).unwrap();
        assert_eq!(waiting.iter().map(|m| m.id).collect::<Vec<_>>(), [second]);

        // Recording the same turn again fails and changes nothing.
        assert!(record_turn_start(&mut conn, &turn(session, 1), &[second]).is_err());
        assert_eq!(undelivered(&conn, session).unwrap().len(), 1);
        assert_eq!(turns(&conn, session).unwrap(), [turn(session, 1)]);
        assert_eq!(list(&conn).unwrap().len(), 1);
        assert!(by_session(&conn, "cli-claude-ffffffff").unwrap().is_none());
    }

    #[test]
    fn a_mode_override_is_stored_and_removed() {
        let conn = open_in_memory().unwrap();
        assert_eq!(mode_override(&conn, "claude").unwrap(), None);
        set_mode_override(&conn, "claude", Some("cli")).unwrap();
        assert_eq!(
            mode_override(&conn, "claude").unwrap().as_deref(),
            Some("cli")
        );
        assert_eq!(mode_override(&conn, "codex").unwrap(), None);
        set_mode_override(&conn, "claude", None).unwrap();
        assert_eq!(mode_override(&conn, "claude").unwrap(), None);
    }

    /// A row written with only the key columns, as an older build that knew fewer columns
    /// would leave it, reads back with every other column's default.
    #[test]
    fn rows_with_only_their_keys_read_back_with_defaults() {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO cli_slots (app, slot) VALUES ('claude', 2);
             INSERT INTO cli_holds (app) VALUES ('codex');",
        )
        .unwrap();
        let slots = list(&conn).unwrap();
        assert_eq!(
            (
                slots[0].state.as_str(),
                slots[0].turn,
                slots[0].session.clone()
            ),
            ("free", 0, None)
        );
        assert_eq!(holds(&conn).unwrap()[0].reported, 0);
    }
}
