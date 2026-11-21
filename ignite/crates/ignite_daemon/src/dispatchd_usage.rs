//! Plan usage samples, holds and reporting for CLI workers.

use std::fmt::Write as _;

use serde_json::{Value, json};

use ignite_core::ids::MachineId;
use ignite_core::plan_usage::{self as hold_rule, Hold};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::MetricsImport;
use ignite_core::time::Timestamp;
use ignite_core::traits::Hub;
use ignite_store::local::plan_usage;
use ignite_store::local::rows::PlanUsage;

use crate::pool::Pool;

/// The source under which samples are posted to the hub's metrics.
pub const SOURCE: &str = "plan_usage";

fn metrics_row(row: &PlanUsage, threshold: u32) -> Value {
    json!({
        "holds": row.percent_used.is_some_and(|p| p >= f64::from(threshold)),
        "at": row.at,
        "status": row.status,
        "window": row.label,
        "percent_used": row.percent_used,
        "resets_at": row.resets_at,
        "note": row.note,
    })
}

/// A row in a few words, as an acknowledgement and `ignite pool usage` print it.
fn describe(row: &PlanUsage) -> String {
    match (&row.label, row.percent_used) {
        (Some(label), Some(percent)) => format!("{label} {percent:.0}%"),
        (Some(label), None) => format!("{label} (no percentage)"),
        (None, _) => match &row.note {
            Some(note) => format!("{} ({note})", row.status),
            None => row.status.clone(),
        },
    }
}

/// How long ago `at` was, from `now`.
fn ago(now: Timestamp, at: &str) -> String {
    let Ok(at) = Timestamp::parse(at) else {
        return "at an unknown time".to_owned();
    };
    let seconds = now.seconds_since(at).max(0.0);
    if seconds < 90.0 {
        format!("{seconds:.0} s ago")
    } else if seconds < 5400.0 {
        format!("{:.0} min ago", seconds / 60.0)
    } else {
        format!("{:.1} h ago", seconds / 3600.0)
    }
}

/// Posts every sample row the hub has not acknowledged, and marks them posted. `threshold` is
/// `[pool] usage_hold_percent`, which says whether each row holds.
pub fn post_usage(
    pool: &Pool,
    hub: &dyn Hub,
    machine: &MachineId,
    threshold: u32,
) -> ignite_core::traits::Result<()> {
    let store_error = |e: ignite_store::StoreError| ignite_core::traits::Error::Io(e.to_string());
    let waiting = pool
        .with_database(|conn| plan_usage::unposted(conn))
        .map_err(store_error)?;
    if waiting.is_empty() {
        return Ok(());
    }
    hub.metrics(&MetricsImport {
        machine: machine.clone(),
        source: SOURCE.to_owned(),
        rows: waiting
            .iter()
            .map(|row| metrics_row(row, threshold))
            .collect(),
    })?;
    let ids: Vec<i64> = waiting.iter().map(|row| row.id).collect();
    pool.with_database(|conn| plan_usage::mark_posted(conn, &ids))
        .map_err(store_error)
}

/// The windows of the plan whose newest sample holds Claude hand-offs at `now`, given the
/// threshold `[pool] usage_hold_percent`. A database that cannot be read holds nothing: the
/// samples are a guard against running out, not a gate that a fault should close.
pub fn usage_holds(pool: &Pool, threshold: u32, now: Timestamp) -> Vec<Hold> {
    pool.with_database(|conn| plan_usage::latest(conn))
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            hold_rule::hold(
                row.label.as_deref()?,
                row.percent_used,
                row.resets_at.as_deref(),
                threshold,
                now,
            )
        })
        .collect()
}

/// The line `ignite pool list`, `ignite pool usage` and `ignite status` print for a hold.
pub fn hold_line(hold: &Hold) -> String {
    let which = match &hold.family {
        Some(family) => format!("Claude hand-offs on {family} models"),
        None => "Claude hand-offs".to_owned(),
    };
    format!(
        "usage hold: {}; {which} wait, except tickets that hold master back, one at a time, \
         until a sample is lower or the window resets",
        hold.describe()
    )
}

/// `ignite pool usage`: the usage hold, if any, then the newest sample of each window of the
/// plan, its age and when the window resets, and the newest sample that had no windows if it is
/// newer still.
pub fn usage_report(pool: &Pool, now: Timestamp, threshold: u32) -> Reply {
    let rows = match pool.with_database(|conn| plan_usage::latest(conn)) {
        Ok(rows) => rows,
        Err(error) => return Reply::text(Exit::Usage, error.to_string()),
    };
    if rows.is_empty() {
        return Reply::text(
            Exit::Success,
            "no plan usage sample yet: a Claude dispatcher takes one before hand-offs and every \
             15 minutes, and each Claude turn in CLI mode reports one",
        );
    }
    let holds = usage_holds(pool, threshold, now);
    let mut lines: Vec<String> = holds.iter().map(hold_line).collect();
    lines.extend(rows.iter().map(|row| {
        let mut line = format!("{:<32} {}", describe(row), ago(now, &row.at));
        if let Some(resets) = &row.resets_at {
            let _ = write!(line, ", resets {resets}");
        }
        line
    }));
    Reply {
        exit: Exit::Success,
        text: lines.join("\n"),
        json: Some(json!({ "samples": rows, "holds": holds })),
        handle: None,
    }
}
