//! Samples of the Claude subscription plan's usage. Without them nothing in Ignite knew how much
//! of the plan was used: the pool kept handing out tickets until workers failed at the limit,
//! and nobody could say what a ticket cost as a share of the plan.
//!
//! The desktop app tells any of its sessions through the `get_usage` tool, which the daemon
//! cannot call, so the daemon asks the dispatcher with a `usage` instruction. The dispatcher
//! calls the tool and reports what it returned, as `ignite dispatch ack <id> raw '<json>'`.
//!
//! - A `usage` is queued ahead of a waiting hand-off when the last one was queued more than
//!   [`USAGE_BEFORE_HANDOFF_S`] ago, and on its own every [`USAGE_EVERY_S`], so the series has
//!   no long gaps while no hand-off happens. Only a Claude dispatcher is asked: Codex has no
//!   such tool.
//! - The report is `get_usage`'s result or its `plan` object. Each of `plan.windows` (`label`,
//!   `percentUsed`, `resetsAt`) becomes one row of the local `plan_usage` table. A `plan.status`
//!   other than `ok` becomes one row with no window and the plan's note, and a report that is not
//!   `get_usage`'s JSON one row with the status `unreadable` and the start of the report.
//! - Every row the hub has not yet acknowledged is posted to `POST /metrics/import` under the
//!   source [`SOURCE`], one metrics row per window, so a row the hub missed goes with the next
//!   sample.
//! - `ignite pool usage` prints the newest row of each window ([`usage_report`]).
//! - A window whose newest row is at or above `[pool] usage_hold_percent` puts the machine on a
//!   usage hold ([`usage_holds`], and `ignite_core::plan_usage` for the rule). Each metrics row
//!   says whether it holds (`holds`), so the dashboard can show the hold without the threshold.
//!   While held, samples are still taken every [`USAGE_EVERY_S`], so the hold ends promptly.

use std::fmt::Write as _;

use serde_json::{Value, json};

use ignite_core::plan_usage::{self as hold_rule, Hold};
use ignite_core::protocol::client::{DispatchAction, Exit, Reply};
use ignite_core::protocol::hub::{MetricsImport, WorkerRole};
use ignite_core::time::Timestamp;
use ignite_store::local::plan_usage;
use ignite_store::local::rows::PlanUsage;

use super::{Dispatcher, Purpose, State, raw};
use crate::pool::{Member, Pool};
use crate::server::Context;

/// Seconds since the last `usage` after which a hand-off is preceded by another.
pub const USAGE_BEFORE_HANDOFF_S: f64 = 300.0;
/// Seconds since the last `usage` after which one is queued even with no hand-off waiting.
pub const USAGE_EVERY_S: f64 = 900.0;
/// The source under which samples are posted to the hub's metrics.
pub const SOURCE: &str = "plan_usage";

/// The text of a `usage` instruction: which tool to load and call, and how to report it.
pub const USAGE_PROMPT: &str = "Load `mcp__ccd_session_mgmt__get_usage` with ToolSearch \
                                (\"select:mcp__ccd_session_mgmt__get_usage\"), call it with no \
                                arguments, and report its result as it came: `ignite dispatch \
                                ack <id> raw '<the JSON>'`.";

/// Whether a `usage` is queued or given and not yet acknowledged.
fn outstanding(state: &State) -> bool {
    state
        .pending
        .iter()
        .chain(state.given.values().map(|g| &g.queued))
        .any(|q| q.purpose == Purpose::Usage)
}

/// The rows of one sample from the dispatcher's report `raw`.
fn sample_rows(raw: &str, machine: &str, now: Timestamp) -> Vec<PlanUsage> {
    let row = |status: &str, note: Option<String>| PlanUsage {
        id: 0,
        at: now.to_string(),
        machine: machine.to_owned(),
        status: status.to_owned(),
        label: None,
        percent_used: None,
        resets_at: None,
        note,
        posted: 0,
    };
    let parsed = serde_json::from_str::<Value>(raw.trim()).ok();
    let plan = parsed
        .as_ref()
        .map(|value| value.get("plan").unwrap_or(value));
    let Some((plan, status)) = plan.and_then(|plan| Some((plan, plan.get("status")?.as_str()?)))
    else {
        return vec![row("unreadable", Some(raw::short(raw)))];
    };
    if status != "ok" {
        let note = ["note", "message", "reason", "error"]
            .iter()
            .find_map(|field| plan.get(*field)?.as_str())
            .map(str::to_owned);
        return vec![row(status, note)];
    }
    let windows: Vec<PlanUsage> = plan
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|window| {
            let label = window.get("label")?.as_str()?;
            let resets_at = match window.get("resetsAt") {
                Some(Value::String(text)) => Some(text.clone()),
                Some(Value::Null) | None => None,
                Some(other) => Some(other.to_string()),
            };
            Some(PlanUsage {
                label: Some(label.to_owned()),
                percent_used: window.get("percentUsed").and_then(Value::as_f64),
                resets_at,
                ..row("ok", None)
            })
        })
        .collect();
    if windows.is_empty() {
        return vec![row("ok", Some("get_usage reported no windows".to_owned()))];
    }
    windows
}

/// One sample row as the hub's metrics row. `holds` says whether the row's percentage is at or
/// above `threshold`; the dashboard checks `resets_at` itself.
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

impl Dispatcher {
    /// Queues a `usage` when one is due (see the module's header).
    pub(super) fn plan_usage(&self, state: &mut State, members: &[Member], now: Timestamp) {
        if !self.setup.plan_usage {
            return;
        }
        let claude_dispatcher = members
            .iter()
            .any(|m| m.role == WorkerRole::Dispatcher && m.app == "claude" && !m.down);
        if !claude_dispatcher || outstanding(state) {
            return;
        }
        if state.usage_asked.is_none() {
            // After a restart, the newest stored sample says when the last one was taken.
            state.usage_asked = self
                .pool
                .with_database(|conn| plan_usage::newest_at(conn))
                .ok()
                .flatten()
                .and_then(|at| Timestamp::parse(&at).ok());
        }
        let age = state
            .usage_asked
            .map_or(f64::INFINITY, |at| now.seconds_since(at));
        let handoff_waiting = state
            .pending
            .iter()
            .any(|q| matches!(&q.purpose, Purpose::Handoff(reserved) if reserved.app == "claude"));
        let before_handoff = handoff_waiting && age >= USAGE_BEFORE_HANDOFF_S;
        if !before_handoff && age < USAGE_EVERY_S {
            return;
        }
        Self::queue(
            state,
            None,
            DispatchAction::Usage,
            USAGE_PROMPT,
            Purpose::Usage,
        );
        if before_handoff && let Some(usage) = state.pending.pop_back() {
            state.pending.push_front(usage);
        }
        state.usage_asked = Some(now);
    }

    /// Acts on `ignite dispatch ack <id> raw <result>` for a `usage`: acknowledges it, stores
    /// the sample and posts what the hub has not yet acknowledged.
    pub(super) fn ack_usage(&self, ctx: &Context, id: u64, raw: &str) -> Reply {
        let now = ctx.clock.now();
        let sample = sample_rows(raw, ctx.machine.as_str(), now);
        let mut reply = self.ack(ctx, id, "ok", "");
        if reply.exit != Exit::Success {
            return reply;
        }
        self.state().usage_asked = Some(now);
        let summary: Vec<String> = sample.iter().map(describe).collect();
        let _ = write!(reply.text, " raw: plan usage {}", summary.join(", "));
        if let Err(error) = self
            .pool
            .with_database(|conn| plan_usage::insert(conn, &sample, now))
        {
            let _ = write!(reply.text, "; not stored: {error}");
            return reply;
        }
        if let Err(error) = self.post_usage(ctx) {
            let _ = write!(
                reply.text,
                "; not yet at the hub ({error}), sent with the next sample"
            );
        }
        reply
    }

    /// Posts every sample row the hub has not acknowledged, and marks them posted.
    fn post_usage(&self, ctx: &Context) -> ignite_core::traits::Result<()> {
        let store_error =
            |e: ignite_store::StoreError| ignite_core::traits::Error::Io(e.to_string());
        let waiting = self
            .pool
            .with_database(|conn| plan_usage::unposted(conn))
            .map_err(store_error)?;
        if waiting.is_empty() {
            return Ok(());
        }
        ctx.hub.metrics(&MetricsImport {
            machine: ctx.machine.clone(),
            source: SOURCE.to_owned(),
            rows: waiting
                .iter()
                .map(|row| metrics_row(row, ctx.config.pool.usage_hold_percent))
                .collect(),
        })?;
        let ids: Vec<i64> = waiting.iter().map(|row| row.id).collect();
        self.pool
            .with_database(|conn| plan_usage::mark_posted(conn, &ids))
            .map_err(store_error)
    }
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
             15 minutes",
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
