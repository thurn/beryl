//! CLI mode's rows (see `schema_20.sql` and `schema_22.sql`): worker slots, their turns, the
//! messages waiting for a turn, each app's CLI hold, the tickets that landed from a slot, and the
//! mode override `ignite pool mode` stores.
//!
//! [`record_turn_start`] is the first phase of starting a turn: it records the turn and marks
//! the messages its prompt carries as delivered in one transaction, before the supervisor
//! starts. A second record of the same turn fails, so a turn is never recorded twice.

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::Result;
use crate::local::rows::{CliHold, CliLanded, CliMessage, CliSlot, CliTurn};

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
        fallback_prompt: row.get("fallback_prompt")?,
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
             (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
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
            slot.fallback_prompt,
        ],
    )?;
    Ok(())
}

fn read_landed(row: &Row<'_>) -> rusqlite::Result<CliLanded> {
    Ok(CliLanded {
        ticket: row.get("ticket")?,
        app: row.get("app")?,
        transcript: row.get("transcript")?,
        model: row.get("model")?,
        landed_at: row.get("landed_at")?,
    })
}

/// Records that `landed.ticket` landed from a CLI slot, replacing an earlier record of the same
/// ticket, and deletes every record that landed before `expired_before`. Times are stored as
/// `Timestamp` writes them (UTC, fixed width), so they compare as text.
pub fn record_landed(conn: &Connection, landed: &CliLanded, expired_before: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM cli_landed WHERE landed_at IS NULL OR landed_at < ?1",
        [expired_before],
    )?;
    conn.execute(
        &format!(
            "INSERT OR REPLACE INTO cli_landed ({}) VALUES (?1, ?2, ?3, ?4, ?5)",
            columns(CliLanded::COLUMNS)
        ),
        params![
            landed.ticket,
            landed.app,
            landed.transcript,
            landed.model,
            landed.landed_at
        ],
    )?;
    Ok(())
}

/// Removes and returns the record of `ticket` landing from a CLI slot, if there is one. A
/// record is used at most once, so two regressions never resume the same transcript at once.
pub fn take_landed(conn: &mut Connection, ticket: &str) -> Result<Option<CliLanded>> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let landed = transaction
        .query_row(
            &format!(
                "SELECT {} FROM cli_landed WHERE ticket = ?1",
                columns(CliLanded::COLUMNS)
            ),
            [ticket],
            read_landed,
        )
        .optional()?;
    transaction.execute("DELETE FROM cli_landed WHERE ticket = ?1", [ticket])?;
    transaction.commit()?;
    Ok(landed)
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

/// Deletes the worker row while keeping its turn and transcript history.
pub fn remove(conn: &Connection, session: &str) -> Result<()> {
    conn.execute("DELETE FROM cli_slots WHERE session = ?1", [session])?;
    Ok(())
}

/// Clears the empty slot rows left by releases that kept a fixed pool.
pub fn clear_empty(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM cli_slots WHERE session IS NULL", [])?;
    Ok(())
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

/// The slot whose session took the hub's reservation of `ticket` with the one-time code `code`,
/// if any. A reservation's code is unique to it, so a slot found here already started that
/// hand-off.
pub fn by_reservation(conn: &Connection, ticket: &str, code: &str) -> Result<Option<CliSlot>> {
    let slot = conn
        .query_row(
            &format!(
                "SELECT {} FROM cli_slots WHERE ticket = ?1 AND reservation = ?2 \
                 AND session IS NOT NULL",
                columns(CliSlot::COLUMNS)
            ),
            params![ticket, code],
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
    record_in(&transaction, turn, deliver)?;
    transaction.commit()?;
    Ok(())
}

/// Gives the slot `slot` names (by app and number) to its session unless another session holds
/// it, and with `first`, records the slot's first turn as [`record_turn_start`] does, all in one
/// transaction. Returns whether the slot was free. Two hand-offs that chose the same free slot
/// cannot both take it, and the `ignite-cli` loop never sees the slot before its turn is
/// recorded, so it cannot record the same turn first.
pub fn claim(
    conn: &mut Connection,
    slot: &CliSlot,
    first: Option<(&CliTurn, &[i64])>,
) -> Result<bool> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let holder: Option<String> = transaction
        .query_row(
            "SELECT session FROM cli_slots WHERE app = ?1 AND slot = ?2",
            params![slot.app, slot.slot],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    if holder.is_some_and(|holder| slot.session.as_deref() != Some(holder.as_str())) {
        return Ok(false);
    }
    put(&transaction, slot)?;
    if let Some((turn, deliver)) = first {
        record_in(&transaction, turn, deliver)?;
    }
    transaction.commit()?;
    Ok(true)
}

fn record_in(transaction: &Connection, turn: &CliTurn, deliver: &[i64]) -> Result<()> {
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
    let mut update = transaction.prepare(
        "UPDATE cli_messages SET delivered_turn = ?2 WHERE id = ?1 AND delivered_turn IS NULL",
    )?;
    for id in deliver {
        update.execute(params![id, turn.turn])?;
    }
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

/// The values `cli_turns.state` takes.
const TURN_STATES: [&str; 3] = ["starting", "running", "ended"];

/// Reads every row of the five CLI tables through the row structs and `ignite_core::cli`'s
/// types, and returns the first row that does not decode, naming its table and row. The release
/// selftest runs it on a copy of the daemon's database, so that a build that could not read the
/// rows an older build wrote is rejected before every running worker is stranded by a restart.
pub fn verify_decodable(conn: &Connection) -> std::result::Result<(), String> {
    verify_table(conn, "cli_slots", CliSlot::COLUMNS, |row| {
        let slot = read_slot(row)?;
        let key = format!("app {}, slot {}", slot.app, slot.slot);
        Ok((key, check_slot(&slot)))
    })?;
    verify_table(conn, "cli_turns", CliTurn::COLUMNS, |row| {
        let turn = read_turn(row)?;
        let key = format!("session {}, turn {}", turn.session, turn.turn);
        Ok((key, check_turn(&turn)))
    })?;
    verify_table(conn, "cli_messages", CliMessage::COLUMNS, |row| {
        let message = read_message(row)?;
        Ok((format!("id {}", message.id), Ok(())))
    })?;
    verify_table(conn, "cli_holds", CliHold::COLUMNS, |row| {
        let hold = read_hold(row)?;
        let outcome = match ignite_core::cli::HoldReason::parse(&hold.reason) {
            Some(_) => Ok(()),
            None => Err(format!("unknown hold reason {:?}", hold.reason)),
        };
        Ok((format!("app {}", hold.app), outcome))
    })?;
    verify_table(conn, "cli_landed", CliLanded::COLUMNS, |row| {
        let landed = read_landed(row)?;
        Ok((format!("ticket {}", landed.ticket), Ok(())))
    })
}

fn check_slot(slot: &CliSlot) -> std::result::Result<(), String> {
    if ignite_core::cli::SlotState::parse(&slot.state).is_none() {
        return Err(format!("unknown slot state {:?}", slot.state));
    }
    Ok(())
}

fn check_turn(turn: &CliTurn) -> std::result::Result<(), String> {
    if !TURN_STATES.contains(&turn.state.as_str()) {
        return Err(format!("unknown turn state {:?}", turn.state));
    }
    if let Some(end) = &turn.end_kind {
        serde_json::from_str::<ignite_core::cli::TurnEnd>(end)
            .map_err(|e| format!("end_kind {end:?} is not a turn end: {e}"))?;
    }
    Ok(())
}

/// Reads each row of `table` on its own, by `rowid`, so one bad row is named and cannot hide
/// behind another. `read` returns the row's key for the message and whether its values mean
/// something to this build.
fn verify_table(
    conn: &Connection,
    table: &str,
    columns: &[&str],
    read: impl Fn(&Row<'_>) -> rusqlite::Result<(String, std::result::Result<(), String>)>,
) -> std::result::Result<(), String> {
    let fail = |row: &str, why: String| format!("{table} row {row} does not decode: {why}");
    let rowids: Vec<i64> = conn
        .prepare(&format!("SELECT rowid FROM {table} ORDER BY rowid"))
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .map_err(|e| format!("{table} cannot be read: {e}"))?;
    let select = format!(
        "SELECT {} FROM {table} WHERE rowid = ?1",
        columns.join(", ")
    );
    for rowid in rowids {
        let (key, outcome) = conn
            .query_row(&select, [rowid], &read)
            .map_err(|e| fail(&format!("rowid {rowid}"), e.to_string()))?;
        outcome.map_err(|why| fail(&key, why))?;
    }
    Ok(())
}

/// Puts `hold.app` on `hold` unless it is already on one, which keeps its first reason and
/// time. Returns whether the hold is new.
pub fn start_hold(conn: &Connection, hold: &CliHold) -> Result<bool> {
    let inserted = conn.execute(
        &format!(
            "INSERT OR IGNORE INTO cli_holds ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            columns(CliHold::COLUMNS)
        ),
        params![
            hold.app,
            hold.reason,
            hold.since,
            hold.cli_version,
            hold.message,
            hold.probed_at,
            hold.reported
        ],
    )?;
    Ok(inserted > 0)
}

/// Ends `app`'s hold. Returns whether it was on one.
pub fn end_hold(conn: &Connection, app: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM cli_holds WHERE app = ?1", [app])? > 0)
}

/// Records when the last probe turn of `app`'s hold ran.
pub fn hold_probed(conn: &Connection, app: &str, at: &str) -> Result<()> {
    conn.execute(
        "UPDATE cli_holds SET probed_at = ?2 WHERE app = ?1",
        params![app, at],
    )?;
    Ok(())
}

/// Marks `app`'s hold as reported to the justiciar.
pub fn hold_reported(conn: &Connection, app: &str) -> Result<()> {
    conn.execute("UPDATE cli_holds SET reported = 1 WHERE app = ?1", [app])?;
    Ok(())
}

/// Replaces every column of a recorded turn except its key, the session and number.
pub fn update_turn(conn: &Connection, turn: &CliTurn) -> Result<()> {
    conn.execute(
        "UPDATE cli_turns SET app = ?3, slot = ?4, ticket = ?5, prompt = ?6, state = ?7, \
         run = ?8, started_at = ?9, ended_at = ?10, exit = ?11, end_kind = ?12, \
         transcript = ?13, model = ?14, cli_version = ?15, cost_usd = ?16, \
         lines_applied = ?17 WHERE session = ?1 AND turn = ?2",
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
    Ok(())
}

/// Puts back every message that turn `turn` of `session` carried, so the next turn carries them
/// again, and returns how many. For a turn that printed no line: its prompt never reached the
/// worker.
pub fn requeue_messages(conn: &Connection, session: &str, turn: i64) -> Result<usize> {
    let count = conn.execute(
        "UPDATE cli_messages SET delivered_turn = NULL WHERE session = ?1 AND delivered_turn = ?2",
        params![session, turn],
    )?;
    Ok(count)
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
            fallback_prompt: None,
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
        let taken = by_reservation(&conn, "T.1", "code").unwrap().unwrap();
        assert_eq!(taken.session.as_deref(), Some(session));
        assert!(by_reservation(&conn, "T.1", "other").unwrap().is_none());
        assert!(by_reservation(&conn, "T.2", "code").unwrap().is_none());
    }

    fn landed(ticket: &str, at: &str) -> CliLanded {
        CliLanded {
            ticket: ticket.to_owned(),
            app: "claude".to_owned(),
            transcript: Some(format!("t-{ticket}")),
            model: Some("opus".to_owned()),
            landed_at: Some(at.to_owned()),
        }
    }

    #[test]
    fn a_landed_ticket_is_taken_once_and_old_records_are_pruned() {
        let mut conn = open_in_memory().unwrap();
        let old = landed("T.1", "2026-11-19T11:00:00.000Z");
        record_landed(&conn, &old, "2026-11-19T00:00:00.000Z").unwrap();
        let new = landed("T.2", "2026-11-20T12:00:00.000Z");
        record_landed(&conn, &new, "2026-11-19T12:00:00.000Z").unwrap();
        assert_eq!(take_landed(&mut conn, "T.1").unwrap(), None);
        assert_eq!(take_landed(&mut conn, "T.2").unwrap(), Some(new));
        assert_eq!(take_landed(&mut conn, "T.2").unwrap(), None);
    }

    #[test]
    fn a_hold_starts_once_and_ends() {
        let conn = open_in_memory().unwrap();
        let hold = CliHold {
            app: "claude".to_owned(),
            reason: "not_logged_in".to_owned(),
            since: Some("2026-11-20T12:00:00.000Z".to_owned()),
            cli_version: Some("2.1.323".to_owned()),
            message: Some("Invalid API key".to_owned()),
            probed_at: None,
            reported: 0,
        };
        assert!(start_hold(&conn, &hold).unwrap());
        let later = CliHold {
            reason: "api_error".to_owned(),
            ..hold.clone()
        };
        assert!(!start_hold(&conn, &later).unwrap());
        hold_probed(&conn, "claude", "2026-11-20T12:05:00.000Z").unwrap();
        hold_reported(&conn, "claude").unwrap();
        let stored = holds(&conn).unwrap();
        assert_eq!(stored[0].reason, "not_logged_in");
        assert_eq!(
            (stored[0].probed_at.as_deref(), stored[0].reported),
            (Some("2026-11-20T12:05:00.000Z"), 1)
        );
        assert!(end_hold(&conn, "claude").unwrap());
        assert!(!end_hold(&conn, "claude").unwrap());
        assert!(holds(&conn).unwrap().is_empty());
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

    /// A column that is neither nullable nor defaulted cannot be added by a later build without
    /// making the rows an older build wrote unreadable, so none of these tables may have one.
    #[test]
    fn every_column_of_the_cli_tables_is_nullable_or_defaulted() {
        let conn = open_in_memory().unwrap();
        for table in [
            "cli_slots",
            "cli_turns",
            "cli_messages",
            "cli_holds",
            "cli_landed",
        ] {
            let mut statement = conn
                .prepare(&format!(
                    "SELECT name, \"notnull\", dflt_value FROM pragma_table_info('{table}')"
                ))
                .unwrap();
            let columns: Vec<(String, i64, Option<String>)> = statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert!(!columns.is_empty(), "{table} has no columns");
            for (name, not_null, default) in columns {
                assert!(
                    not_null == 0 || default.is_some(),
                    "{table}.{name} is NOT NULL with no DEFAULT"
                );
            }
        }
    }

    #[test]
    fn verification_passes_on_good_rows_and_names_the_bad_one() {
        let mut conn = open_in_memory().unwrap();
        verify_decodable(&conn).unwrap();
        put(&conn, &slot("cli-claude-0a1b2c3d")).unwrap();
        record_turn_start(&mut conn, &turn("cli-claude-0a1b2c3d", 1), &[]).unwrap();
        queue_message(&conn, &message("cli-claude-0a1b2c3d", "hi")).unwrap();
        conn.execute_batch(
            "INSERT INTO cli_holds (app, reason) VALUES ('codex', 'not_logged_in');
             UPDATE cli_turns SET end_kind = '{\"kind\":\"hold\"}';",
        )
        .unwrap();
        verify_decodable(&conn).unwrap();

        conn.execute("UPDATE cli_slots SET state = 'dreaming'", [])
            .unwrap();
        let error = verify_decodable(&conn).unwrap_err();
        assert!(
            error.contains("cli_slots") && error.contains("claude") && error.contains("dreaming"),
            "{error}"
        );
        conn.execute("UPDATE cli_slots SET state = 'free'", [])
            .unwrap();

        conn.execute("UPDATE cli_turns SET end_kind = 'not json'", [])
            .unwrap();
        let error = verify_decodable(&conn).unwrap_err();
        assert!(
            error.contains("cli_turns") && error.contains("turn 1"),
            "{error}"
        );
        conn.execute("UPDATE cli_turns SET end_kind = NULL", [])
            .unwrap();

        conn.execute("UPDATE cli_holds SET reason = 'sulking'", [])
            .unwrap();
        let error = verify_decodable(&conn).unwrap_err();
        assert!(
            error.contains("cli_holds") && error.contains("codex"),
            "{error}"
        );
        conn.execute("UPDATE cli_holds SET reason = 'api_error'", [])
            .unwrap();

        // A value of the wrong type is named by its rowid.
        conn.execute("UPDATE cli_messages SET urgent = 'yes'", [])
            .unwrap();
        let error = verify_decodable(&conn).unwrap_err();
        assert!(error.contains("cli_messages row rowid 1"), "{error}");
    }
}
