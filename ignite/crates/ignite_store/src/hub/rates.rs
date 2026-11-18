//! The rate each class's check commands achieve, which converts the two budgets from seconds to
//! instructions (see [`ignite_core::budget`] and "Where regression checks run" in
//! `docs/ignite.md`).
//!
//! A class's rate for a day is the median, over the [`RATE_DAYS`] whole days before it, of each
//! day's instructions retired per second of wall time by the class's check commands: records
//! that name a family and counted instructions. It is computed the first time it is asked for
//! on a day and kept for the rest of that day, so the gate and tip verification choose the same
//! checks all day, whatever the load of the moment. Days older than [`KEPT_DAYS`] are deleted.

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::budget::{ClassRates, RATE_DAYS};
use ignite_core::time::Timestamp;

use crate::Result;

/// How many days of computed rates are kept.
pub const KEPT_DAYS: u32 = 30;

const SECONDS_PER_DAY: f64 = 86_400.0;

/// The classes whose rates are computed, as records name them.
const CLASSES: [&str; 2] = ["land", "tip"];

/// Each class's rate for the day of `now`, computed and kept on the first call that day.
pub fn rates(conn: &Connection, now: Timestamp) -> Result<ClassRates> {
    let day = now.day();
    let midnight: Timestamp = format!("{day}T00:00:00Z").parse().unwrap_or(now);
    let mut rates = ClassRates {
        day: day.clone(),
        land: None,
        tip: None,
    };
    for class in CLASSES {
        let kept: Option<Option<f64>> = conn
            .query_row(
                "SELECT rate FROM class_rates WHERE day = ?1 AND class = ?2",
                params![day, class],
                |row| row.get(0),
            )
            .optional()?;
        let rate = if let Some(rate) = kept {
            rate
        } else {
            let rate = median_rate(conn, class, midnight)?;
            // Two requests may compute it at once: the first insert wins, and both read it.
            conn.execute(
                "INSERT OR IGNORE INTO class_rates (day, class, rate) VALUES (?1, ?2, ?3)",
                params![day, class, rate],
            )?;
            conn.query_row(
                "SELECT rate FROM class_rates WHERE day = ?1 AND class = ?2",
                params![day, class],
                |row| row.get(0),
            )?
        };
        match class {
            "land" => rates.land = rate,
            _ => rates.tip = rate,
        }
    }
    let oldest = midnight.plus_seconds(-f64::from(KEPT_DAYS) * SECONDS_PER_DAY);
    conn.execute(
        "DELETE FROM class_rates WHERE day < ?1",
        params![oldest.day()],
    )?;
    Ok(rates)
}

/// The median of the daily rates of `class`'s check commands over the whole days before
/// `midnight`, or `None` when those days hold no such record.
fn median_rate(conn: &Connection, class: &str, midnight: Timestamp) -> Result<Option<f64>> {
    let from = midnight.plus_seconds(-f64::from(RATE_DAYS) * SECONDS_PER_DAY);
    let mut select = conn.prepare(
        "SELECT sum(instructions) / 1e9 / sum(wall_s) FROM records
         WHERE class = ?1 AND family IS NOT NULL AND instructions IS NOT NULL AND wall_s > 0
           AND at >= ?2 AND at < ?3
         GROUP BY substr(at, 1, 10)",
    )?;
    let mut daily: Vec<f64> = select
        .query_map(
            params![class, from.to_string(), midnight.to_string()],
            |row| row.get::<_, Option<f64>>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .collect();
    if daily.is_empty() {
        return Ok(None);
    }
    daily.sort_by(f64::total_cmp);
    let middle = daily.len() / 2;
    Ok(Some(if daily.len().is_multiple_of(2) {
        f64::midpoint(daily[middle - 1], daily[middle])
    } else {
        daily[middle]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;

    fn record(conn: &Connection, class: &str, at: &str, billions: f64, wall_s: f64) {
        conn.execute(
            "INSERT INTO records (machine, verb, class, family, queued_s, wall_s, cpu_s,
                                  instructions, peak_mb, ignite, at)
             VALUES ('m-1', ?1, ?1, 'rust', 0, ?2, ?2, ?3, 0, 'x', ?4)",
            params![class, wall_s, billions * 1e9, at],
        )
        .unwrap();
    }

    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    #[test]
    fn a_rate_is_the_median_of_the_week_and_kept_for_the_day() {
        let conn = hub::open_in_memory().unwrap();
        // Three days of landings at 10, 20 and 40 billion a second (the second day's two
        // commands add up), one too old to count, and today's, which waits for tomorrow.
        record(&conn, "land", "2026-11-15T10:00:00.000Z", 100.0, 10.0);
        record(&conn, "land", "2026-11-16T10:00:00.000Z", 300.0, 10.0);
        record(&conn, "land", "2026-11-16T11:00:00.000Z", 100.0, 10.0);
        record(&conn, "land", "2026-11-17T10:00:00.000Z", 400.0, 10.0);
        record(&conn, "land", "2026-11-01T10:00:00.000Z", 9_000.0, 1.0);
        record(&conn, "land", "2026-11-18T01:00:00.000Z", 9_000.0, 1.0);
        // A build or other command that names no family is not a check command.
        conn.execute(
            "INSERT INTO records (machine, verb, class, queued_s, wall_s, cpu_s, instructions,
                                  peak_mb, ignite, at)
             VALUES ('m-1', 'land', 'land', 0, 1, 1, 9e12, 0, 'x', '2026-11-17T12:00:00.000Z')",
            [],
        )
        .unwrap();

        let rates = rates(&conn, at("2026-11-18T09:00:00Z")).unwrap();
        assert_eq!(rates.day, "2026-11-18");
        assert_eq!(rates.land, Some(20.0));
        assert_eq!(rates.tip, None);

        // Records added later that day change nothing until the next day.
        record(&conn, "land", "2026-11-17T20:00:00.000Z", 10_000.0, 1.0);
        let later = super::rates(&conn, at("2026-11-18T23:00:00Z")).unwrap();
        assert_eq!(later, rates);
        let tomorrow = super::rates(&conn, at("2026-11-19T00:30:00Z")).unwrap();
        assert!(tomorrow.land.unwrap() > 20.0, "{tomorrow:?}");
    }
}
