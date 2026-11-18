//! `repeated-command`: a command that tickets of one kind run far more often than the other
//! tickets do. Its ticket asks why those tickets repeat it and how to need it less.
//!
//! - A command is a record's verb with its check family, such as `test rust`.
//! - A ticket's kind is its `tier:` label, the setting Ignite chooses work by, or `none`.
//! - Over the last week, the runs per ticket of each kind are compared with the runs per ticket
//!   of every other ticket (the cohort). Both groups need at least five tickets, and the kind
//!   must run the command at least twice as often as the cohort.
//! - The cost is the agent time those runs took; the saving is the runs above the cohort's
//!   rate, at the median run's wall time.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use ignite_core::dispatch::label;
use ignite_core::time::Timestamp;

use super::{AGENT_HOURS, Finding, Rule, WEEK_SECONDS, median};

pub const NAME: &str = "repeated-command";

/// Each group of tickets needs this many before their rates are compared.
const MIN_TICKETS: usize = 5;
/// How many times the cohort's rate counts as far more often.
const FACTOR: f64 = 2.0;

pub struct RepeatedCommand {
    pub report_only: bool,
}

/// One command's runs by one kind of ticket.
#[derive(Default)]
struct Runs {
    count: u32,
    wall_s: Vec<f64>,
}

/// The week's agent commands, with the tickets of each kind.
struct Week {
    /// Tickets that ran any agent command, by kind.
    tickets: BTreeMap<String, BTreeSet<String>>,
    /// Runs by (command, kind).
    runs: BTreeMap<(String, String), Runs>,
}

impl Week {
    fn ticket_count(&self, kind: Option<&str>, other_than: Option<&str>) -> usize {
        self.tickets
            .iter()
            .filter(|(k, _)| kind.is_none_or(|kind| *k == kind))
            .filter(|(k, _)| other_than.is_none_or(|other| *k != other))
            .map(|(_, tickets)| tickets.len())
            .sum()
    }

    fn cohort_runs(&self, command: &str, kind: &str) -> u32 {
        self.runs
            .iter()
            .filter(|((c, k), _)| c == command && k != kind)
            .map(|(_, runs)| runs.count)
            .sum()
    }
}

impl Rule for RepeatedCommand {
    fn name(&self) -> &str {
        NAME
    }

    fn report_only(&self) -> bool {
        self.report_only
    }

    fn find(&self, conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Finding>> {
        let mut week = week(conn, now)?;
        let mut found = Vec::new();
        let keys: Vec<(String, String)> = week.runs.keys().cloned().collect();
        for (command, kind) in keys {
            let tickets = week.ticket_count(Some(&kind), None);
            let cohort = week.ticket_count(None, Some(&kind));
            if tickets < MIN_TICKETS || cohort < MIN_TICKETS {
                continue;
            }
            let cohort_rate = f64::from(week.cohort_runs(&command, &kind)) / cohort as f64;
            let runs = week
                .runs
                .get_mut(&(command.clone(), kind.clone()))
                .expect("a key");
            let rate = f64::from(runs.count) / tickets as f64;
            if rate < FACTOR * cohort_rate || rate <= cohort_rate {
                continue;
            }
            let typical = median(&mut runs.wall_s);
            let cost = runs.wall_s.iter().sum::<f64>() / 3600.0;
            let excess = (rate - cohort_rate) * tickets as f64;
            let saving = excess * typical / 3600.0;
            let subject = subject(&command, &kind);
            found.push(Finding {
                rule: NAME.to_owned(),
                subject: subject.clone(),
                unit: AGENT_HOURS.to_owned(),
                cost,
                saving,
                title: format!("Run `{command}` less often in tier:{kind} tickets"),
                body: format!(
                    "tier:{kind} tickets ran `{command}` {rate:.1} times each in the last week, \
                     against {cohort_rate:.1} for other tickets ({tickets} tickets against \
                     {cohort}), costing {cost:.1} agent-hours a week. Estimated saving: \
                     {saving:.1} agent-hours a week if they ran it as often as other tickets \
                     do.\nmeasure: {NAME} agent-hours a week for {subject}, now {cost:.1}"
                ),
            });
        }
        Ok(found)
    }

    fn measure(&self, conn: &Connection, subject: &str, now: Timestamp) -> rusqlite::Result<f64> {
        Ok(week(conn, now)?
            .runs
            .iter()
            .find(|((command, kind), _)| self::subject(command, kind) == subject)
            .map_or(0.0, |(_, runs)| runs.wall_s.iter().sum::<f64>() / 3600.0))
    }
}

fn subject(command: &str, kind: &str) -> String {
    format!("{command} in tier:{kind}")
}

/// The agent commands that tickets ran in the week before `now`.
fn week(conn: &Connection, now: Timestamp) -> rusqlite::Result<Week> {
    let since = now.plus_seconds(-WEEK_SECONDS).to_string();
    let mut statement = conn.prepare(
        "SELECT r.ticket, r.verb, r.family, r.wall_s, t.labels FROM records r
         LEFT JOIN tickets t ON t.id = r.ticket
         WHERE r.class = 'agent' AND r.ticket IS NOT NULL AND r.at >= ?1 AND r.at <= ?2",
    )?;
    let rows = statement.query_map(params![since, now.to_string()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, f64>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut week = Week {
        tickets: BTreeMap::new(),
        runs: BTreeMap::new(),
    };
    for row in rows {
        let (ticket, verb, family, wall_s, labels) = row?;
        let labels: Vec<String> = labels
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let kind = label(&labels, "tier:").unwrap_or("none").to_owned();
        let command = match family {
            Some(family) => format!("{verb} {family}"),
            None => verb,
        };
        week.tickets.entry(kind.clone()).or_default().insert(ticket);
        let runs = week.runs.entry((command, kind)).or_default();
        runs.count += 1;
        runs.wall_s.push(wall_s);
    }
    Ok(week)
}
