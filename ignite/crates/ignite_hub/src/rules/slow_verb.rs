//! `slow-verb`: an `ignite` verb whose own overhead, apart from the work it runs, is over a
//! second at the median. Its ticket asks to make the verb itself faster.
//!
//! A record holds no separate overhead, so the rule measures the verbs that run no work: an
//! agent's command with no check family and no instructions counted, such as `status` or
//! `brief`. Its overhead is the time the agent waited apart from the queue, `wall_s` less
//! `queued_s`. The saving is each run's overhead above one second.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use ignite_core::time::Timestamp;

use super::{AGENT_HOURS, Finding, Rule, WEEK_SECONDS, grouped, median};

pub const NAME: &str = "slow-verb";

/// The median overhead above which a verb is slow, in seconds.
const LIMIT_S: f64 = 1.0;

pub struct SlowVerb {
    pub report_only: bool,
}

impl Rule for SlowVerb {
    fn name(&self) -> &str {
        NAME
    }

    fn report_only(&self) -> bool {
        self.report_only
    }

    fn find(&self, conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Finding>> {
        let mut found = Vec::new();
        for (verb, mut overheads) in overheads(conn, now)? {
            let typical = median(&mut overheads);
            if typical <= LIMIT_S {
                continue;
            }
            let cost = overheads.iter().sum::<f64>() / 3600.0;
            let saving = overheads
                .iter()
                .map(|seconds| (seconds - LIMIT_S).max(0.0))
                .sum::<f64>()
                / 3600.0;
            found.push(Finding {
                rule: NAME.to_owned(),
                subject: verb.clone(),
                unit: AGENT_HOURS.to_owned(),
                cost,
                saving,
                title: format!("Make `ignite {verb}` faster"),
                body: format!(
                    "`ignite {verb}` took {typical:.1} seconds of its own at the median over {} \
                     runs in the last week, costing agents {cost:.1} hours a week. Estimated \
                     saving: {saving:.1} agent-hours a week if every run took a second at \
                     most.\nmeasure: {NAME} agent-hours a week for {verb}, now {cost:.1}",
                    grouped(overheads.len() as f64)
                ),
            });
        }
        Ok(found)
    }

    fn measure(&self, conn: &Connection, subject: &str, now: Timestamp) -> rusqlite::Result<f64> {
        Ok(overheads(conn, now)?
            .get(subject)
            .map_or(0.0, |seconds| seconds.iter().sum::<f64>() / 3600.0))
    }
}

/// Each verb's overhead per run, in seconds, over the week before `now`.
fn overheads(conn: &Connection, now: Timestamp) -> rusqlite::Result<BTreeMap<String, Vec<f64>>> {
    let since = now.plus_seconds(-WEEK_SECONDS).to_string();
    let mut statement = conn.prepare(
        "SELECT verb, MAX(wall_s - queued_s, 0) FROM records
         WHERE class = 'agent' AND family IS NULL AND instructions IS NULL
           AND at >= ?1 AND at <= ?2",
    )?;
    let rows = statement.query_map(params![since, now.to_string()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;
    let mut verbs: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for row in rows {
        let (verb, seconds) = row?;
        verbs.entry(verb).or_default().push(seconds);
    }
    Ok(verbs)
}
