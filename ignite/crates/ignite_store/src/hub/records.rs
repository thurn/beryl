//! Records: one row per command from every machine, with per-crate compile costs.
//!
//! A daemon keeps a record in its outbox until the hub acknowledges the batch, so a batch can
//! arrive twice. A record is identified by the machine and everything the supervisor measured
//! about the command, which is fixed once the record is made, so an identical record is stored
//! once and keeps its first ID.

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use ignite_core::ids::RecordId;
use ignite_core::protocol::hub::{CompileUnit, Record};
use ignite_core::time::Timestamp;

use crate::{Result, StoreError};

/// Inserts the records and their compile units; returns their IDs in order. A record the hub
/// already holds is not stored again, and its existing ID is returned.
pub fn insert(conn: &mut Connection, records: &[Record]) -> Result<Vec<RecordId>> {
    let tx = conn.transaction()?;
    let mut ids = Vec::with_capacity(records.len());
    for record in records {
        ids.push(insert_one(&tx, record)?);
    }
    tx.commit()?;
    Ok(ids)
}

fn insert_one(tx: &Transaction, record: &Record) -> Result<RecordId> {
    let at = record.at.to_string();
    let class = text(&record.class)?;
    let outcome = record.outcome.as_ref().map(text).transpose()?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM records
             WHERE machine = ?1 AND at = ?2 AND verb = ?3 AND class = ?4
               AND ticket IS ?5 AND session IS ?6 AND tree IS ?7 AND family IS ?8
               AND queued_s = ?9 AND wall_s = ?10 AND cpu_s = ?11 AND instructions IS ?12
               AND peak_mb = ?13 AND outcome IS ?14 AND commit_sha IS ?15 AND ignite = ?16",
            params![
                record.machine.as_str(),
                at,
                record.verb,
                class,
                record.ticket.as_ref().map(ToString::to_string),
                record.session.as_ref().map(ToString::to_string),
                record.tree.as_ref().map(ToString::to_string),
                record.family,
                record.queued_s,
                record.wall_s,
                record.cpu_s,
                record.instructions,
                record.peak_mb as i64,
                outcome,
                record.commit.as_ref().map(ToString::to_string),
                record.ignite,
            ],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(RecordId(id as u64));
    }
    tx.execute(
        "INSERT INTO records (machine, ticket, session, tree, verb, class, family, queued_s,
                              wall_s, cpu_s, instructions, peak_mb, outcome, commit_sha,
                              ignite, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            record.machine.as_str(),
            record.ticket.as_ref().map(ToString::to_string),
            record.session.as_ref().map(ToString::to_string),
            record.tree.as_ref().map(ToString::to_string),
            record.verb,
            class,
            record.family,
            record.queued_s,
            record.wall_s,
            record.cpu_s,
            record.instructions,
            record.peak_mb as i64,
            outcome,
            record.commit.as_ref().map(ToString::to_string),
            record.ignite,
            at,
        ],
    )?;
    let id = tx.last_insert_rowid();
    for unit in &record.compile_units {
        // A compiler process that ran twice for one crate and mode adds to the same row.
        tx.execute(
            "INSERT INTO compile_units (record, crate_name, mode, instructions, cpu_s, rebuilt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(record, crate_name, mode) DO UPDATE SET
               instructions = CASE WHEN instructions IS NULL AND excluded.instructions IS NULL
                                   THEN NULL
                                   ELSE COALESCE(instructions, 0) + COALESCE(excluded.instructions, 0)
                              END,
               cpu_s = cpu_s + excluded.cpu_s,
               rebuilt = MAX(rebuilt, excluded.rebuilt)",
            params![
                id,
                unit.crate_name,
                unit.mode,
                unit.instructions,
                unit.cpu_s,
                i64::from(unit.rebuilt)
            ],
        )?;
    }
    Ok(RecordId(id as u64))
}

/// The records of one ticket, oldest first.
pub fn of_ticket(conn: &Connection, ticket: &str) -> Result<Vec<Record>> {
    let mut statement = conn.prepare(
        "SELECT id, machine, ticket, session, tree, verb, class, family, queued_s, wall_s, cpu_s,
                instructions, peak_mb, outcome, commit_sha, ignite, at
         FROM records WHERE ticket = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([ticket], |row| {
            Ok((row.get::<_, i64>(0)?, RawRecord::read(row)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut units = conn.prepare(
        "SELECT crate_name, mode, instructions, cpu_s, rebuilt FROM compile_units
         WHERE record = ?1 ORDER BY crate_name, mode",
    )?;
    rows.into_iter()
        .map(|(id, raw)| {
            let compile_units = units
                .query_map([id], |row| {
                    Ok(CompileUnit {
                        crate_name: row.get(0)?,
                        mode: row.get(1)?,
                        instructions: row.get(2)?,
                        cpu_s: row.get(3)?,
                        rebuilt: row.get::<_, i64>(4)? != 0,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            raw.into_record(compile_units)
        })
        .collect()
}

/// A row of `records`, before its text columns are parsed.
struct RawRecord {
    machine: String,
    ticket: Option<String>,
    session: Option<String>,
    tree: Option<String>,
    verb: String,
    class: String,
    family: Option<String>,
    queued_s: f64,
    wall_s: f64,
    cpu_s: f64,
    instructions: Option<f64>,
    peak_mb: i64,
    outcome: Option<String>,
    commit: Option<String>,
    ignite: String,
    at: String,
}

impl RawRecord {
    fn read(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            machine: row.get(1)?,
            ticket: row.get(2)?,
            session: row.get(3)?,
            tree: row.get(4)?,
            verb: row.get(5)?,
            class: row.get(6)?,
            family: row.get(7)?,
            queued_s: row.get(8)?,
            wall_s: row.get(9)?,
            cpu_s: row.get(10)?,
            instructions: row.get(11)?,
            peak_mb: row.get(12)?,
            outcome: row.get(13)?,
            commit: row.get(14)?,
            ignite: row.get(15)?,
            at: row.get(16)?,
        })
    }

    fn into_record(self, compile_units: Vec<CompileUnit>) -> Result<Record> {
        Ok(Record {
            machine: id(&self.machine)?,
            ticket: self.ticket.as_deref().map(id).transpose()?,
            session: self.session.as_deref().map(id).transpose()?,
            tree: self.tree.as_deref().map(id).transpose()?,
            verb: self.verb,
            class: parse(&self.class)?,
            family: self.family,
            queued_s: self.queued_s,
            wall_s: self.wall_s,
            cpu_s: self.cpu_s,
            instructions: self.instructions,
            peak_mb: self.peak_mb as u64,
            outcome: self.outcome.as_deref().map(parse).transpose()?,
            commit: self.commit.as_deref().map(id).transpose()?,
            ignite: self.ignite,
            at: Timestamp::parse(&self.at).map_err(|e| StoreError::Invalid(e.to_string()))?,
            compile_units,
        })
    }
}

fn id<T: std::str::FromStr<Err = ignite_core::ids::IdError>>(value: &str) -> Result<T> {
    value
        .parse()
        .map_err(|e: ignite_core::ids::IdError| StoreError::Invalid(e.to_string()))
}

/// The lowercase text a protocol enum is stored as.
fn text<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => Ok(text),
        _ => Err(StoreError::Invalid(
            "an enum did not serialize as text".into(),
        )),
    }
}

fn parse<T: DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|e| StoreError::Invalid(format!("stored value {value:?}: {e}")))
}

#[cfg(test)]
mod tests {
    use ignite_core::protocol::Class;
    use ignite_core::protocol::Outcome;

    use super::*;
    use crate::hub;

    fn record(wall_s: f64) -> Record {
        Record {
            machine: "m-7f3a".parse().unwrap(),
            ticket: Some("Tq44s.9.48".parse().unwrap()),
            session: Some("local_c8470108".parse().unwrap()),
            tree: Some("tree-07".parse().unwrap()),
            verb: "test".into(),
            class: Class::Agent,
            family: Some("b3d-test".into()),
            queued_s: 4.1,
            wall_s,
            cpu_s: 301.0,
            instructions: Some(1.21e12),
            peak_mb: 2210,
            outcome: Some(Outcome::Fail),
            commit: Some("8c1f2ab".parse().unwrap()),
            ignite: "d97ed0e".into(),
            at: "2026-11-17T14:12:00.000Z".parse().unwrap(),
            compile_units: vec![CompileUnit {
                crate_name: "crate_b".into(),
                mode: "check".into(),
                instructions: Some(5.0e9),
                cpu_s: 3.5,
                rebuilt: true,
            }],
        }
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn the_same_batch_sent_twice_is_stored_once() {
        let mut conn = hub::open_in_memory().unwrap();
        let batch = [record(58.2), record(59.0)];
        let first = insert(&mut conn, &batch).unwrap();
        let second = insert(&mut conn, &batch).unwrap();
        assert_eq!(first, second);
        assert_ne!(first[0], first[1]);
        assert_eq!(count(&conn, "records"), 2);
        assert_eq!(count(&conn, "compile_units"), 2);
    }

    #[test]
    fn a_duplicate_inside_one_batch_is_stored_once() {
        let mut conn = hub::open_in_memory().unwrap();
        let ids = insert(&mut conn, &[record(58.2), record(58.2)]).unwrap();
        assert_eq!(ids[0], ids[1]);
        assert_eq!(count(&conn, "records"), 1);
    }

    #[test]
    fn records_keep_their_fields_and_compile_units() {
        let mut conn = hub::open_in_memory().unwrap();
        let mut without_ticket = record(10.0);
        without_ticket.ticket = None;
        without_ticket.outcome = None;
        without_ticket.compile_units.clear();
        insert(&mut conn, &[record(58.2), without_ticket]).unwrap();
        assert_eq!(of_ticket(&conn, "Tq44s.9.48").unwrap(), vec![record(58.2)]);
        assert!(of_ticket(&conn, "Tq44s.9.49").unwrap().is_empty());
    }

    #[test]
    fn a_crate_compiled_twice_in_one_mode_adds_up() {
        let mut conn = hub::open_in_memory().unwrap();
        let mut twice = record(58.2);
        twice.compile_units.push(twice.compile_units[0].clone());
        insert(&mut conn, &[twice]).unwrap();
        let stored = &of_ticket(&conn, "Tq44s.9.48").unwrap()[0];
        assert_eq!(stored.compile_units.len(), 1);
        assert_eq!(stored.compile_units[0].instructions, Some(1.0e10));
        assert!((stored.compile_units[0].cpu_s - 7.0).abs() < 1e-9);
    }
}
