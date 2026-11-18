//! The efficiency dashboard's findings, as rules. Each machine posts the output of its findings
//! command (`findings --json` of the dashboard) to `POST /metrics/import` with the source
//! [`SOURCE`]. A row is either one finding or the whole report, whose `groups` hold the
//! findings.
//!
//! - A finding's subject is its rule's name, since a dashboard rule measures one kind of waste
//!   across the whole fleet.
//! - Each machine's latest row for a rule from the last 14 days counts, and the machines' costs
//!   and savings add up, because each machine reads only its own transcripts.
//! - A finding whose saving the dashboard cannot estimate is left out: nothing can be filed
//!   against an unknown amount.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde_json::Value;

use ignite_core::time::Timestamp;

use super::{AGENT_HOURS, CORE_HOURS, DOLLARS, Finding, Rule};

/// The source name under which machines post the dashboard's findings.
pub const SOURCE: &str = "findings";

/// How old a machine's last report of a finding may be and still count.
const FRESH_DAYS: f64 = 14.0;

/// Every rule of the efficiency dashboard, as one Ignite rule. Whether a finding is report-only
/// is decided by its own rule's name in `ignite.toml`.
pub struct Efficiency;

/// One finding of the dashboard, as one machine reported it.
struct Reported {
    unit: &'static str,
    summary: String,
    cost: f64,
    saving: f64,
    evidence: String,
}

impl Rule for Efficiency {
    fn name(&self) -> &'static str {
        "efficiency"
    }

    fn report_only(&self) -> bool {
        false
    }

    fn find(&self, conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Finding>> {
        let mut found = Vec::new();
        for (rule, reports) in reported(conn, now)? {
            let unit = reports[0].unit;
            let same_unit = reports.iter().filter(|r| r.unit == unit);
            let (cost, saving) = same_unit
                .clone()
                .fold((0.0, 0.0), |(c, s), r| (c + r.cost, s + r.saving));
            let summaries: Vec<String> = same_unit
                .map(|r| format!("- {} (`{}`)", r.summary, r.evidence))
                .collect();
            found.push(Finding {
                rule: rule.clone(),
                subject: rule.clone(),
                unit: unit.to_owned(),
                cost,
                saving,
                title: format!("Efficiency finding: {rule}"),
                body: format!(
                    "The efficiency dashboard's rule `{rule}` finds waste costing {cost:.1} \
                     {unit} a week. Estimated saving: {saving:.1} {unit} a week.\n{}\nmeasure: \
                     efficiency finding `{rule}`, now {cost:.1} {unit} a week",
                    summaries.join("\n")
                ),
            });
        }
        Ok(found)
    }

    fn measure(&self, conn: &Connection, subject: &str, now: Timestamp) -> rusqlite::Result<f64> {
        Ok(reported(conn, now)?
            .get(subject)
            .map_or(0.0, |reports| reports.iter().map(|r| r.cost).sum()))
    }
}

/// Each dashboard rule's findings, one per machine that reported it lately.
fn reported(
    conn: &Connection,
    now: Timestamp,
) -> rusqlite::Result<BTreeMap<String, Vec<Reported>>> {
    let since = now.plus_seconds(-FRESH_DAYS * 86_400.0).to_string();
    let mut statement = conn.prepare(
        "SELECT machine, data FROM metrics WHERE source = ?1 AND at >= ?2 AND at <= ?3
         ORDER BY at",
    )?;
    let rows = statement.query_map(params![SOURCE, since, now.to_string()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    // Later rows replace earlier ones for the same machine and rule.
    let mut latest: BTreeMap<(String, String), Reported> = BTreeMap::new();
    for row in rows {
        let (machine, data) = row?;
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        for finding in findings_in(&value) {
            if let Some((rule, reported)) = parse(finding) {
                latest.insert((rule, machine.clone()), reported);
            }
        }
    }
    let mut by_rule: BTreeMap<String, Vec<Reported>> = BTreeMap::new();
    for ((rule, _), reported) in latest {
        by_rule.entry(rule).or_default().push(reported);
    }
    Ok(by_rule)
}

/// The findings in one row: the row itself, or those in a whole report's groups.
fn findings_in(value: &Value) -> Vec<&Value> {
    match value.get("groups").and_then(Value::as_array) {
        Some(groups) => groups
            .iter()
            .filter_map(|group| group.get("findings").and_then(Value::as_array))
            .flatten()
            .collect(),
        None => vec![value],
    }
}

fn parse(finding: &Value) -> Option<(String, Reported)> {
    let rule = finding.get("rule")?.as_str()?;
    let text = finding.get("unit")?.as_str()?;
    let unit = match text {
        "dollars" => DOLLARS,
        "agent_hours" | "agent-hours" => AGENT_HOURS,
        "core_hours" | "core-hours" => CORE_HOURS,
        _ => return None,
    };
    let saving = finding.get("saving_per_week")?.get(text)?.as_f64()?;
    let text_of = |key: &str| {
        finding
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Some((
        rule.to_owned(),
        Reported {
            unit,
            summary: text_of("summary"),
            cost: finding.get("cost_per_week").and_then(Value::as_f64)?,
            saving,
            evidence: text_of("evidence"),
        },
    ))
}
