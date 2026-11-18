//! Tip verification rounds: the costs a round cuts its claims by, frozen per tip and family, and
//! the record of where each finished round's time went (see "How long a round takes" in
//! `docs/ignite.md`).

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::score::BILLIONS_PER_CPU_SECOND;
use ignite_core::time::Timestamp;
use ignite_core::tipround::{TipCosts, TipCostsRequest, TipRound};

use crate::hub::results::parse;
use crate::{Result, StoreError};

/// How long frozen costs are kept. A round verifies the newest tip, so costs for a tip older
/// than this are never asked for again.
pub const COSTS_KEPT_SECONDS: f64 = 2.0 * 86_400.0;

/// The costs and claim length a round at `request.commit` cuts `request.family` by. The first
/// request for a tip and family freezes them, with the claim length it names; every later
/// request gets the same answer, whatever costs were recorded since. Frozen costs older than
/// [`COSTS_KEPT_SECONDS`] are deleted.
pub fn costs(conn: &Connection, request: &TipCostsRequest, now: Timestamp) -> Result<TipCosts> {
    if let Some(frozen) = frozen(conn, request)? {
        return Ok(frozen);
    }
    let fresh = TipCosts {
        claim_seconds: request.claim_seconds,
        costs: recorded(conn, &request.family)?,
    };
    let json =
        serde_json::to_string(&fresh.costs).map_err(|e| StoreError::Invalid(e.to_string()))?;
    conn.execute(
        "DELETE FROM tip_costs WHERE at < ?1",
        params![now.plus_seconds(-COSTS_KEPT_SECONDS).to_string()],
    )?;
    // Two machines may ask at once: the first insert wins, and both read it back.
    conn.execute(
        "INSERT OR IGNORE INTO tip_costs (commit_sha, family, claim_seconds, costs, at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            request.commit.as_str(),
            request.family,
            fresh.claim_seconds,
            json,
            now.to_string()
        ],
    )?;
    Ok(frozen(conn, request)?.unwrap_or(fresh))
}

fn frozen(conn: &Connection, request: &TipCostsRequest) -> Result<Option<TipCosts>> {
    let row: Option<(f64, String)> = conn
        .query_row(
            "SELECT claim_seconds, costs FROM tip_costs WHERE commit_sha = ?1 AND family = ?2",
            params![request.commit.as_str(), request.family],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(claim_seconds, json)| {
        let costs = serde_json::from_str(&json).map_err(|error| {
            StoreError::Invalid(format!("frozen costs of {}: {error}", request.family))
        })?;
        Ok(TipCosts {
            claim_seconds,
            costs,
        })
    })
    .transpose()
}

/// Each recorded check of `family` with the lowest of its kept costs, in CPU seconds. Costs
/// kept in billions of instructions convert at [`BILLIONS_PER_CPU_SECOND`].
fn recorded(conn: &Connection, family: &str) -> Result<BTreeMap<String, f64>> {
    let mut select = conn.prepare(
        "SELECT check_name, costs, unit FROM check_costs
         WHERE substr(check_name, 1, length(?1) + 1) = ?1 || ':'",
    )?;
    let rows = select
        .query_map(params![family], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut costs = BTreeMap::new();
    for (check, json, unit) in rows {
        let kept: Vec<f64> = serde_json::from_str(&json)
            .map_err(|error| StoreError::Invalid(format!("stored costs of {check}: {error}")))?;
        let lowest = kept
            .into_iter()
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
            .min_by(f64::total_cmp);
        if let Some(lowest) = lowest {
            let seconds = if unit == "cpu_s" {
                lowest
            } else {
                lowest / BILLIONS_PER_CPU_SECOND
            };
            costs.insert(check, seconds);
        }
    }
    Ok(costs)
}

/// Stores one finished round.
pub fn record(conn: &Connection, round: &TipRound) -> Result<()> {
    conn.execute(
        "INSERT INTO tip_rounds (commit_sha, machine, at, wall_s, ran, cached, checks_s,
                                 listing_s, building_s, waiting_s)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            round.commit.as_str(),
            round.machine.as_str(),
            round.at.to_string(),
            round.wall_s,
            round.ran,
            round.cached,
            round.checks_s,
            round.listing_s,
            round.building_s,
            round.waiting_s
        ],
    )?;
    Ok(())
}

/// The newest `limit` rounds, newest first.
pub fn recent(conn: &Connection, limit: u32) -> Result<Vec<TipRound>> {
    let mut select = conn.prepare(
        "SELECT commit_sha, machine, at, wall_s, ran, cached, checks_s, listing_s, building_s,
                waiting_s
         FROM tip_rounds ORDER BY at DESC, id DESC LIMIT ?1",
    )?;
    let rows = select
        .query_map(params![limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                [
                    row.get::<_, f64>(3)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ],
                (row.get::<_, u32>(4)?, row.get::<_, u32>(5)?),
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(commit, machine, at, seconds, (ran, cached))| {
            let [wall_s, checks_s, listing_s, building_s, waiting_s] = seconds;
            Ok(TipRound {
                commit: parse("commit", &commit)?,
                machine: parse("machine", &machine)?,
                at: parse("time", &at)?,
                wall_s,
                ran,
                cached,
                checks_s,
                listing_s,
                building_s,
                waiting_s,
            })
        })
        .collect()
}

/// The newest round, if any was recorded.
pub fn last(conn: &Connection) -> Result<Option<TipRound>> {
    Ok(recent(conn, 1)?.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{self, checks};
    use ignite_core::ids::{Commit, MachineId};

    fn request(commit: &str, claim_seconds: f64) -> TipCostsRequest {
        TipCostsRequest {
            commit: Commit::new(commit).unwrap(),
            family: "unit".to_owned(),
            claim_seconds,
        }
    }

    #[test]
    fn costs_are_frozen_for_a_tip_at_the_first_request() {
        let conn = hub::open_in_memory().unwrap();
        checks::add_cost(&conn, "unit:a", 2.0, "cpu_s").unwrap();
        checks::add_cost(&conn, "unit:a", 1.0, "cpu_s").unwrap();
        checks::add_cost(&conn, "unit:b", 34.0, "instructions").unwrap();
        checks::add_cost(&conn, "unitx:c", 5.0, "cpu_s").unwrap();
        let now = Timestamp::EPOCH;

        let first = costs(&conn, &request("abcd", 60.0), now).unwrap();
        // The lowest kept cost, instructions converted, and no check of another family.
        assert_eq!(
            first.costs,
            BTreeMap::from([("unit:a".to_owned(), 1.0), ("unit:b".to_owned(), 2.0)])
        );

        // Costs recorded since, and another machine's claim length, change nothing for the tip.
        checks::add_cost(&conn, "unit:a", 0.5, "cpu_s").unwrap();
        let second = costs(&conn, &request("abcd", 30.0), now.plus_seconds(10.0)).unwrap();
        assert_eq!(second, first);
        // A new tip sees them.
        let newer = costs(&conn, &request("ef01", 30.0), now.plus_seconds(10.0)).unwrap();
        assert!((newer.costs["unit:a"] - 0.5).abs() < f64::EPSILON);
        assert!((newer.claim_seconds - 30.0).abs() < f64::EPSILON);

        // Two days on, the old tips' costs are gone.
        costs(
            &conn,
            &request("2345", 60.0),
            now.plus_seconds(COSTS_KEPT_SECONDS + 20.0),
        )
        .unwrap();
        let left: u32 = conn
            .query_row("SELECT count(*) FROM tip_costs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(left, 1);
    }

    #[test]
    fn the_last_round_is_the_newest() {
        let conn = hub::open_in_memory().unwrap();
        assert_eq!(last(&conn).unwrap(), None);
        let round = |commit: &str, at: f64| TipRound {
            commit: Commit::new(commit).unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            at: Timestamp::EPOCH.plus_seconds(at),
            wall_s: 600.0,
            ran: 3,
            cached: 4,
            checks_s: 400.0,
            listing_s: 180.0,
            building_s: 10.0,
            waiting_s: 5.0,
        };
        record(&conn, &round("abcd", 10.0)).unwrap();
        record(&conn, &round("ef01", 20.0)).unwrap();
        assert_eq!(last(&conn).unwrap(), Some(round("ef01", 20.0)));
        assert_eq!(recent(&conn, 5).unwrap().len(), 2);
    }
}
