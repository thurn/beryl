//! `slow-crate`: the crate whose rebuilds cost agents the most core-hours in the last week,
//! counting every rebuild in a command an agent ran. Its ticket asks to split the crate or cut
//! what depends on it.
//!
//! The rule names only the most expensive crate, so one evaluation files at most one ticket. A
//! crate that grows but is seldom rebuilt costs little and files nothing; a crate that many
//! tickets rebuild files even if it has not grown.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::time::Timestamp;

use super::{CORE_HOURS, Finding, Rule, WEEK_SECONDS, grouped, median};

pub const NAME: &str = "slow-crate";

/// The share of a crate's rebuild cost that splitting it, or cutting what depends on it, is
/// estimated to save: the document's example, 15.3 of 38.2 core-hours.
const SAVED_SHARE: f64 = 0.4;

pub struct SlowCrate {
    pub report_only: bool,
}

/// One crate's rebuilds by agents over the week.
struct Rebuilds {
    count: u32,
    cpu_s: f64,
    instructions: Vec<f64>,
}

impl Rule for SlowCrate {
    fn name(&self) -> &str {
        NAME
    }

    fn report_only(&self) -> bool {
        self.report_only
    }

    fn find(&self, conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Finding>> {
        let crates = rebuilds(conn, now)?;
        let Some((name, mut found)) = crates
            .into_iter()
            .max_by(|(_, a), (_, b)| a.cpu_s.total_cmp(&b.cpu_s))
        else {
            return Ok(Vec::new());
        };
        let cost = found.cpu_s / 3600.0;
        let saving = cost * SAVED_SHARE;
        let billions = median(&mut found.instructions) / 1e9;
        let measure = match rebuild_count(conn, &name)? {
            Some((count, instructions)) => format!(
                "measure: rebuild count `{count}`, now {} billion",
                grouped(instructions / 1e9)
            ),
            None => format!("measure: {NAME} core-hours a week for {name}, now {cost:.1}"),
        };
        Ok(vec![Finding {
            rule: NAME.to_owned(),
            subject: name.clone(),
            unit: CORE_HOURS.to_owned(),
            cost,
            saving,
            title: format!("Cut the rebuild cost of {name}"),
            body: format!(
                "{name} costs {cost:.1} core-hours a week in rebuilds ({} rebuilds, median {} \
                 billion instructions). Estimated saving: {saving:.1} core-hours a week if it \
                 is split, or if what depends on it is cut, so that fewer changes rebuild \
                 it.\n{measure}",
                grouped(f64::from(found.count)),
                grouped(billions),
            ),
        }])
    }

    fn measure(&self, conn: &Connection, subject: &str, now: Timestamp) -> rusqlite::Result<f64> {
        Ok(rebuilds(conn, now)?
            .get(subject)
            .map_or(0.0, |found| found.cpu_s / 3600.0))
    }
}

/// Each crate's rebuilds in commands agents ran in the week before `now`.
fn rebuilds(conn: &Connection, now: Timestamp) -> rusqlite::Result<BTreeMap<String, Rebuilds>> {
    let since = now.plus_seconds(-WEEK_SECONDS).to_string();
    let mut statement = conn.prepare(
        "SELECT u.crate_name, u.cpu_s, u.instructions FROM compile_units u
         JOIN records r ON r.id = u.record
         WHERE u.rebuilt = 1 AND r.class = 'agent' AND r.at >= ?1 AND r.at <= ?2",
    )?;
    let rows = statement.query_map(params![since, now.to_string()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, Option<f64>>(2)?,
        ))
    })?;
    let mut crates: BTreeMap<String, Rebuilds> = BTreeMap::new();
    for row in rows {
        let (name, cpu_s, instructions) = row?;
        let entry = crates.entry(name).or_insert(Rebuilds {
            count: 0,
            cpu_s: 0.0,
            instructions: Vec::new(),
        });
        entry.count += 1;
        entry.cpu_s += cpu_s;
        entry.instructions.extend(instructions);
    }
    Ok(crates)
}

/// The rebuild count whose measurement recompiles the most of `crate_name`, with that
/// measurement's total instructions: the load-independent measure the ticket names. The
/// table keeps no time, so the measurement is the one with the most of the crate, not the
/// latest; the agent runs `ignite rebuild` before and after the change in any case.
fn rebuild_count(conn: &Connection, crate_name: &str) -> rusqlite::Result<Option<(String, f64)>> {
    conn.query_row(
        "SELECT name, instructions FROM rebuilds
         WHERE json_extract(crates, '$.' || json_quote(?1)) IS NOT NULL
         ORDER BY json_extract(crates, '$.' || json_quote(?1)) DESC, name LIMIT 1",
        params![crate_name],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}
