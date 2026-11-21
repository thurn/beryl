//! Plan usage samples, holds and reporting for CLI workers.

use std::fmt::Write as _;

use serde_json::{Value, json};

use ignite_core::ids::MachineId;
use std::collections::BTreeMap;

use ignite_core::plan_usage::{Hold, parse_resets_at};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::MetricsImport;
use ignite_core::time::Timestamp;
use ignite_core::traits::Hub;
use ignite_store::local::plan_usage;
use ignite_store::local::rows::PlanUsage;

use crate::pool::Pool;

/// The source under which samples are posted to the hub's metrics.
pub const SOURCE: &str = "plan_usage";

fn metrics_row(row: &PlanUsage) -> Value {
    json!({
        "provider": row.provider,
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

/// Posts every sample row the hub has not acknowledged, and marks it posted.
pub fn post_usage(
    pool: &Pool,
    hub: &dyn Hub,
    machine: &MachineId,
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
        rows: waiting.iter().map(metrics_row).collect(),
    })?;
    let ids: Vec<i64> = waiting.iter().map(|row| row.id).collect();
    pool.with_database(|conn| plan_usage::mark_posted(conn, &ids))
        .map_err(store_error)
}

/// The hub's most recently reported holds. A cached hold expires at its reset even if
/// the daemon has lost contact with the hub.
pub fn usage_holds(pool: &Pool, now: Timestamp) -> BTreeMap<String, Vec<Hold>> {
    let mut holds = crate::fleet_notice::current(pool).unwrap_or_default().holds;
    holds.retain(|_, windows| {
        windows.retain(|hold| {
            hold.resets_at
                .as_deref()
                .and_then(parse_resets_at)
                .is_none_or(|at| at > now)
        });
        !windows.is_empty()
    });
    holds
}

/// The lines `ignite pool list`, `ignite pool usage` and `ignite status` print for holds.
pub fn hold_lines(holds: &BTreeMap<String, Vec<Hold>>) -> impl Iterator<Item = String> + '_ {
    holds.iter().flat_map(|(provider, windows)| {
        windows
            .iter()
            .map(move |hold| format!("usage hold: {provider} · {}", hold.describe()))
    })
}

/// `ignite pool usage`: the fleet's holds and the newest samples this machine has seen.
pub fn usage_report(pool: &Pool, now: Timestamp) -> Reply {
    let rows = match pool.with_database(|conn| plan_usage::latest(conn)) {
        Ok(rows) => rows,
        Err(error) => return Reply::text(Exit::Usage, error.to_string()),
    };
    let holds = usage_holds(pool, now);
    if rows.is_empty() && holds.is_empty() {
        return Reply::text(Exit::Success, "no plan usage samples or fleet holds yet");
    }
    let mut lines: Vec<String> = hold_lines(&holds).collect();
    lines.extend(rows.iter().map(|row| {
        let mut line = format!(
            "{} · {:<32} {}",
            row.provider,
            describe(row),
            ago(now, &row.at)
        );
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
