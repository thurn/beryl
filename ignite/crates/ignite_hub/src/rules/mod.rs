//! Improvement rules: queries over the records that find a source of waste, name a subject and
//! estimate a cost and a saving per week (docs/ignite.md, "Improvement tickets").
//!
//! - [`slow_crate`], [`never_caught`], [`repeated_command`] and [`slow_verb`] are Ignite's own
//!   rules over its records.
//! - [`efficiency`] reads the efficiency dashboard's findings from the rows each machine posts
//!   to `POST /metrics/import`.
//! - [`filing`] evaluates every rule and files one ticket per (rule, subject) that clears the
//!   minimum for its unit, and comments on each landed improvement seven days later.
//! - [`backlog`] gives the ratios the dispatcher's improvement share is computed from.
//!
//! A rule measures what the work costs now, never how much a number grew, so code that only
//! gets larger files nothing unless agents pay for it often enough to clear the minimum.

use rusqlite::Connection;

use ignite_core::config;
use ignite_core::time::Timestamp;

pub mod backlog;
pub mod efficiency;
pub mod filing;
pub mod never_caught;
pub mod repeated_command;
pub mod slow_crate;
pub mod slow_verb;

#[cfg(test)]
mod tests;

/// The units a saving is measured in, as tickets and findings name them.
pub const DOLLARS: &str = "dollars";
pub const AGENT_HOURS: &str = "agent-hours";
pub const CORE_HOURS: &str = "core-hours";

pub(crate) const WEEK_SECONDS: f64 = 7.0 * 86_400.0;

/// What one rule found.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub rule: String,
    pub subject: String,
    /// `dollars`, `agent-hours` or `core-hours`.
    pub unit: String,
    /// The subject's cost per week now.
    pub cost: f64,
    /// The estimated saving per week.
    pub saving: f64,
    /// The ticket's title and body.
    pub title: String,
    pub body: String,
}

pub trait Rule: Send + Sync {
    fn name(&self) -> &str;
    /// A report-only rule shows on the dashboard and files nothing.
    fn report_only(&self) -> bool;
    fn find(&self, conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Finding>>;
    /// The subject's cost per week now, in the rule's unit: the measure that the comment seven
    /// days after an improvement lands compares with the cost when its ticket was filed. Zero
    /// when the subject costs nothing any more.
    fn measure(&self, conn: &Connection, subject: &str, now: Timestamp) -> rusqlite::Result<f64>;
}

/// Every rule, with the report-only and protected lists from `ignite.toml`.
pub fn all(rules: &config::Rules) -> Vec<Box<dyn Rule>> {
    let report_only = |name: &str| rules.report_only.iter().any(|r| r == name);
    vec![
        Box::new(slow_crate::SlowCrate {
            report_only: report_only(slow_crate::NAME),
        }),
        Box::new(never_caught::NeverCaught {
            report_only: report_only(never_caught::NAME),
            protected: rules.clone(),
        }),
        Box::new(repeated_command::RepeatedCommand {
            report_only: report_only(repeated_command::NAME),
        }),
        Box::new(slow_verb::SlowVerb {
            report_only: report_only(slow_verb::NAME),
        }),
        Box::new(efficiency::Efficiency),
    ]
}

/// The smallest weekly saving that files a ticket, in the unit's terms: $25, 2 agent-hours or
/// 10 core-hours. `None` for a unit no rule should use, which files nothing.
pub fn minimum(unit: &str) -> Option<f64> {
    match unit {
        DOLLARS => Some(25.0),
        AGENT_HOURS => Some(2.0),
        CORE_HOURS => Some(10.0),
        _ => None,
    }
}

/// The median of `values`, or zero for none.
pub(crate) fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        f64::midpoint(values[middle - 1], values[middle])
    }
}

/// A number with thousands separators and no decimals, as ticket bodies show counts.
pub(crate) fn grouped(value: f64) -> String {
    let digits = format!("{:.0}", value.max(0.0));
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}
