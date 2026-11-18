//! The result cache: outcomes by check, input hash and platform.
//!
//! A portable check (`platforms = "any"`) has `platform: None` in its key and is stored with an
//! empty platform, so every machine finds it. A platform-specific check is stored under its own
//! operating system and found only by a lookup that names that system.
//!
//! Only `pass` and `fail` are stored. An `incomplete` run says nothing about the check, and a
//! `flaky` one is kept in the `flaky` table (see [`crate::hub::checks::record_flaky`]), not as a
//! cached outcome.

use std::fmt::Display;
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{CachedResult, NewResult, ResultKey};
use ignite_core::time::Timestamp;

use crate::{Result, StoreError};

/// Entries nobody has looked up for this many days are deleted.
pub const KEEP_DAYS: f64 = 30.0;

/// Turns a value that fails validation (a malformed ID in a stored row) into a store error.
pub(super) fn parse<T: FromStr>(what: &str, text: &str) -> Result<T>
where
    T::Err: Display,
{
    text.parse()
        .map_err(|error| StoreError::Invalid(format!("stored {what} {text:?}: {error}")))
}

fn platform_text(key: &ResultKey) -> &'static str {
    key.platform.map_or("", |os| os.as_str())
}

fn outcome_text(outcome: Outcome) -> Option<&'static str> {
    match outcome {
        Outcome::Pass => Some("pass"),
        Outcome::Fail => Some("fail"),
        Outcome::Flaky | Outcome::Incomplete => None,
    }
}

/// One entry per key, in order, marking each hit as looked up at `now`.
pub fn lookup(
    conn: &Connection,
    keys: &[ResultKey],
    now: Timestamp,
) -> Result<Vec<Option<CachedResult>>> {
    let mut select = conn.prepare_cached(
        "SELECT outcome, machine, record, at, output FROM results
         WHERE check_name = ?1 AND hash = ?2 AND platform = ?3",
    )?;
    let mut touch = conn.prepare_cached(
        "UPDATE results SET last_lookup = ?4 WHERE check_name = ?1 AND hash = ?2 AND platform = ?3",
    )?;
    let mut found = Vec::with_capacity(keys.len());
    for key in keys {
        let args = (key.check.as_str(), key.hash.as_str(), platform_text(key));
        let row = select
            .query_row(params![args.0, args.1, args.2], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .optional()?;
        let Some((outcome, machine, record, at, output)) = row else {
            found.push(None);
            continue;
        };
        touch.execute(params![args.0, args.1, args.2, now.to_string()])?;
        let outcome = match outcome.as_str() {
            "pass" => Outcome::Pass,
            "fail" => Outcome::Fail,
            other => return Err(StoreError::Invalid(format!("stored outcome {other:?}"))),
        };
        found.push(Some(CachedResult {
            key: key.clone(),
            outcome,
            machine: parse("machine", &machine)?,
            record: ignite_core::ids::RecordId(u64::try_from(record).unwrap_or_default()),
            at: parse("time", &at)?,
            output,
        }));
    }
    Ok(found)
}

/// Stores each result that is a pass or a failure, replacing an entry with the same key.
pub fn store(conn: &mut Connection, results: &[NewResult], now: Timestamp) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut insert = tx.prepare_cached(
            "INSERT OR REPLACE INTO results
               (check_name, hash, platform, outcome, machine, record, at, last_lookup, output)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
        )?;
        for result in results {
            let Some(outcome) = outcome_text(result.outcome) else {
                continue;
            };
            insert.execute(params![
                result.key.check.as_str(),
                result.key.hash.as_str(),
                platform_text(&result.key),
                outcome,
                result.machine.as_str(),
                i64::try_from(result.record.0).unwrap_or(i64::MAX),
                now.to_string(),
                result.output,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Deletes entries nobody has looked up for 30 days; returns how many.
pub fn prune(conn: &Connection, now: Timestamp) -> Result<u64> {
    let cutoff = now.plus_seconds(-KEEP_DAYS * 86_400.0);
    let deleted = conn.execute(
        "DELETE FROM results WHERE last_lookup < ?1",
        params![cutoff.to_string()],
    )?;
    Ok(deleted as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;
    use ignite_core::config::Os;
    use ignite_core::ids::{CheckName, InputHash, MachineId, RecordId};

    const DAY: f64 = 86_400.0;

    fn start() -> Timestamp {
        "2026-11-01T00:00:00.000Z".parse().unwrap()
    }

    fn key(check: &str, digit: char, platform: Option<Os>) -> ResultKey {
        ResultKey {
            check: CheckName::new(check).unwrap(),
            hash: InputHash::new(digit.to_string().repeat(64)).unwrap(),
            platform,
        }
    }

    fn new_result(key: &ResultKey, outcome: Outcome) -> NewResult {
        NewResult {
            key: key.clone(),
            outcome,
            machine: MachineId::new("m-1").unwrap(),
            record: RecordId(7),
            output: (outcome == Outcome::Fail).then(|| "out/1.log".to_owned()),
        }
    }

    #[test]
    fn a_portable_result_is_found_whichever_machine_asks_and_a_platform_one_only_by_its_own() {
        let mut conn = hub::open_in_memory().unwrap();
        let portable = key("rust:a", '1', None);
        let macos = key("ui:b", '2', Some(Os::Macos));
        store(
            &mut conn,
            &[
                new_result(&portable, Outcome::Pass),
                new_result(&macos, Outcome::Fail),
            ],
            start(),
        )
        .unwrap();

        // The same check and hash looked up as the other kind misses.
        let portable_as_macos = key("rust:a", '1', Some(Os::Macos));
        let macos_as_portable = key("ui:b", '2', None);
        let found = lookup(
            &conn,
            &[
                portable.clone(),
                macos.clone(),
                portable_as_macos,
                macos_as_portable,
                key("rust:a", '3', None),
            ],
            start(),
        )
        .unwrap();
        assert_eq!(found[0].as_ref().unwrap().outcome, Outcome::Pass);
        let failure = found[1].as_ref().unwrap();
        assert_eq!(failure.outcome, Outcome::Fail);
        assert_eq!(failure.output.as_deref(), Some("out/1.log"));
        assert_eq!(failure.record, RecordId(7));
        assert_eq!(failure.at, start());
        assert!(found[2].is_none() && found[3].is_none() && found[4].is_none());
    }

    #[test]
    fn incomplete_and_flaky_outcomes_are_never_stored() {
        let mut conn = hub::open_in_memory().unwrap();
        let incomplete = key("rust:a", '1', None);
        let flaky = key("rust:b", '1', None);
        store(
            &mut conn,
            &[
                new_result(&incomplete, Outcome::Incomplete),
                new_result(&flaky, Outcome::Flaky),
            ],
            start(),
        )
        .unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM results", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn storing_again_replaces_the_entry() {
        let mut conn = hub::open_in_memory().unwrap();
        let k = key("rust:a", '1', None);
        store(&mut conn, &[new_result(&k, Outcome::Fail)], start()).unwrap();
        store(&mut conn, &[new_result(&k, Outcome::Pass)], start()).unwrap();
        let found = lookup(&conn, &[k], start()).unwrap();
        assert_eq!(found[0].as_ref().unwrap().outcome, Outcome::Pass);
    }

    #[test]
    fn pruning_keeps_an_entry_looked_up_yesterday_and_deletes_an_untouched_one() {
        let mut conn = hub::open_in_memory().unwrap();
        let used = key("rust:used", '1', None);
        let idle = key("rust:idle", '1', None);
        store(
            &mut conn,
            &[
                new_result(&used, Outcome::Pass),
                new_result(&idle, Outcome::Pass),
            ],
            start(),
        )
        .unwrap();
        // 29 days on, someone looks `used` up; 31 days after storing, prune.
        let looked_up = start().plus_seconds(29.0 * DAY);
        lookup(&conn, std::slice::from_ref(&used), looked_up).unwrap();
        let now = start().plus_seconds(31.0 * DAY);
        assert_eq!(prune(&conn, now).unwrap(), 1);
        let found = lookup(&conn, &[used, idle], now).unwrap();
        assert!(found[0].is_some());
        assert!(found[1].is_none());
    }

    #[test]
    fn pruning_keeps_every_entry_within_thirty_days() {
        let mut conn = hub::open_in_memory().unwrap();
        let k = key("rust:a", '1', None);
        store(&mut conn, &[new_result(&k, Outcome::Pass)], start()).unwrap();
        assert_eq!(prune(&conn, start().plus_seconds(29.5 * DAY)).unwrap(), 0);
    }
}
