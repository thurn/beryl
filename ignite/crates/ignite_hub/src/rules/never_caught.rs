//! `never-caught`: a check that used cores at the tip for 30 days and caught nothing. Its
//! ticket asks to delete the check or replace it with a cheaper test.
//!
//! - A check qualifies once its earliest run at the tip is at least 30 days old, so a new check
//!   is never named for the quiet weeks it has not yet lived through.
//! - Catching means any failed result of the check in the last 30 days, at the gate or the
//!   tip, or an open regression ticket for it.
//! - A run's cost is the CPU time of the record that ran it, divided among the results that
//!   record produced.
//! - A check on the protected list in `ignite.toml`, by its own name or its family's, is never
//!   named.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use ignite_core::config;
use ignite_core::time::Timestamp;

use super::{CORE_HOURS, Finding, Rule};

pub const NAME: &str = "never-caught";

/// How long a check must have run at the tip without catching anything.
const WINDOW_DAYS: f64 = 30.0;

pub struct NeverCaught {
    pub report_only: bool,
    /// The protected list, from `ignite.toml`.
    pub protected: config::Rules,
}

/// One check's runs at the tip over the window.
struct TipRuns {
    runs: u32,
    cpu_s: f64,
    first: String,
    caught: bool,
}

impl Rule for NeverCaught {
    fn name(&self) -> &str {
        NAME
    }

    fn report_only(&self) -> bool {
        self.report_only
    }

    fn find(&self, conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Finding>> {
        let since = now.plus_seconds(-WINDOW_DAYS * 86_400.0).to_string();
        let mut found = Vec::new();
        for (check, tip) in tip_runs(conn, now)? {
            if tip.caught || tip.first > since || self.protected.is_protected(&check) {
                continue;
            }
            let cost = weekly_core_hours(tip.cpu_s);
            found.push(Finding {
                rule: NAME.to_owned(),
                subject: check.clone(),
                unit: CORE_HOURS.to_owned(),
                cost,
                saving: cost,
                title: format!("Delete or replace the check {check}"),
                body: format!(
                    "{check} ran {} times at the tip in the last {WINDOW_DAYS:.0} days and \
                     caught nothing, costing {cost:.1} core-hours a week. Estimated saving: \
                     {cost:.1} core-hours a week if it is deleted, or less if it is replaced by \
                     a cheaper test that guards the same behavior.\nmeasure: {NAME} \
                     core-hours a week for {check}, now {cost:.1}",
                    tip.runs
                ),
            });
        }
        Ok(found)
    }

    fn measure(&self, conn: &Connection, subject: &str, now: Timestamp) -> rusqlite::Result<f64> {
        Ok(tip_runs(conn, now)?
            .get(subject)
            .map_or(0.0, |tip| weekly_core_hours(tip.cpu_s)))
    }
}

/// CPU seconds spent over the window, as core-hours a week.
fn weekly_core_hours(cpu_s: f64) -> f64 {
    cpu_s / 3600.0 * 7.0 / WINDOW_DAYS
}

/// Every check that ran at the tip in the window before `now`, with its runs and cost there,
/// its earliest run at the tip ever, and whether it caught anything in the window.
fn tip_runs(conn: &Connection, now: Timestamp) -> rusqlite::Result<BTreeMap<String, TipRuns>> {
    let since = now.plus_seconds(-WINDOW_DAYS * 86_400.0).to_string();
    let until = now.to_string();
    let mut statement = conn.prepare(
        "SELECT s.check_name, COUNT(*), SUM(r.cpu_s / shared.results), first.at,
           EXISTS (SELECT 1 FROM results x WHERE x.check_name = s.check_name
                   AND x.outcome = 'fail' AND x.at >= ?1 AND x.at <= ?2)
           OR EXISTS (SELECT 1 FROM regressions g WHERE g.check_name = s.check_name
                      AND g.state = 'open')
         FROM results s JOIN records r ON r.id = s.record
         JOIN (SELECT record, COUNT(*) AS results FROM results GROUP BY record) shared
           ON shared.record = s.record
         JOIN (SELECT f.check_name, MIN(f.at) AS at FROM results f
               JOIN records fr ON fr.id = f.record WHERE fr.class = 'tip'
               GROUP BY f.check_name) first
           ON first.check_name = s.check_name
         WHERE r.class = 'tip' AND s.at >= ?1 AND s.at <= ?2
         GROUP BY s.check_name",
    )?;
    let rows = statement.query_map(params![since, until], |row| {
        Ok((
            row.get::<_, String>(0)?,
            TipRuns {
                runs: row.get(1)?,
                cpu_s: row.get(2)?,
                first: row.get(3)?,
                caught: row.get(4)?,
            },
        ))
    })?;
    rows.collect()
}
