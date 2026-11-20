//! Evaluating the rules and filing their tickets (docs/ignite.md, "Improvement tickets").
//!
//! - The hub evaluates the rules when new records or efficiency rows arrive, at most once an
//!   hour ([`evaluate_if_due`]), since each evaluation reads a month of records. Each
//!   evaluation also stores last week's use for the improvement share
//!   ([`super::backlog::refresh`]).
//! - A finding files a ticket when no ticket for its (rule, subject) is open, none closed in the
//!   last 30 days ([`findings::may_file`]), its rule is not report-only, and its saving reaches
//!   the minimum for its unit ([`super::minimum`]).
//! - Every other finding is stored without a ticket, so the dashboard shows it. Those rows are
//!   rebuilt on each evaluation; a row with a ticket keeps it, and only its cost and saving
//!   follow the latest finding.
//! - Seven days after an improvement lands, its ticket gets a comment with the rule's measure
//!   for the subject before and after.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};

use ignite_core::config;
use ignite_core::protocol::tickets::{
    BatchRequest, Operation, Saving, Status, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{findings, rows};
use ignite_store::{Result, StoreError};

use super::{Finding, Rule, minimum};
use crate::routes::dispatch::set_columns;

/// The author of the tickets and comments the rules write.
pub const ACTOR: &str = "ignite-hub";

/// The shortest time between two evaluations, in seconds.
const EVERY_SECONDS: f64 = 3600.0;
/// Where the time of the last evaluation is kept, in `meta`.
const LAST_KEY: &str = "rules_evaluated_at";
/// How long after landing an improvement's measure is taken again.
const FOLLOW_UP_DAYS: f64 = 7.0;
/// Ticket metadata: the subject's cost when its ticket was filed, and the cost seven days
/// after it landed.
const BEFORE_KEY: &str = "measure_before";
const AFTER_KEY: &str = "measure_after";

/// What one evaluation did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evaluation {
    /// The tickets filed.
    pub filed: Vec<String>,
    /// The findings stored for the dashboard without a ticket.
    pub reported: Vec<Finding>,
    /// The landed improvements that got their measure after seven days.
    pub measured: Vec<String>,
}

/// Evaluates every rule if the last successful evaluation was an hour or more ago.
pub fn evaluate_if_due(
    conn: &mut Connection,
    config: &config::Rules,
    now: Timestamp,
) -> Result<Option<Evaluation>> {
    let last: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [LAST_KEY], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(last) = last.and_then(|text| Timestamp::parse(&text).ok())
        && now.seconds_since(last) < EVERY_SECONDS
    {
        return Ok(None);
    }
    // Last week's use is too large a read for the dispatch transaction, so it is taken here.
    super::backlog::refresh(conn, now)?;
    // The time is recorded only after a successful evaluation, so a failed one is tried again
    // when the next records arrive instead of waiting an hour.
    let evaluation = evaluate(conn, &super::all(config), config, now)?;
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        params![LAST_KEY, now.to_string()],
    )?;
    Ok(Some(evaluation))
}

/// Evaluates `rules` now: files tickets, stores the other findings and takes the measures of
/// improvements that landed seven days ago.
pub fn evaluate(
    conn: &mut Connection,
    rules: &[Box<dyn Rule>],
    config: &config::Rules,
    now: Timestamp,
) -> Result<Evaluation> {
    let mut evaluation = Evaluation::default();
    // The findings without a ticket are rebuilt at the end in one transaction, so a failure
    // part way leaves the dashboard's earlier rows in place instead of a half-built set.
    let mut unticketed: Vec<rows::Finding> = Vec::new();
    for rule in rules {
        for finding in rule.find(conn, now)? {
            let report_only = rule.report_only() || config.report_only.contains(&finding.rule);
            let held = held_ticket(conn, &finding)?;
            if !findings::may_file(conn, &finding.rule, &finding.subject, now)? {
                if let Some((ticket, closed_at)) = held {
                    findings::upsert(conn, &row(&finding, Some(ticket), closed_at))?;
                }
                continue;
            }
            let enough = minimum(&finding.unit).is_some_and(|least| finding.saving >= least);
            if report_only || !enough {
                unticketed.push(row(&finding, None, None));
                evaluation.reported.push(finding);
                continue;
            }
            let ticket = file(conn, &finding, now)?;
            findings::upsert(conn, &row(&finding, Some(ticket.clone()), None))?;
            evaluation.filed.push(ticket);
        }
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("DELETE FROM findings WHERE ticket IS NULL", [])?;
    for finding in &unticketed {
        findings::upsert(&tx, finding)?;
    }
    tx.commit()?;
    evaluation.measured = follow_up(conn, rules, now)?;
    Ok(evaluation)
}

fn row(finding: &Finding, ticket: Option<String>, closed_at: Option<String>) -> rows::Finding {
    rows::Finding {
        rule: finding.rule.clone(),
        subject: finding.subject.clone(),
        unit: finding.unit.clone(),
        cost: finding.cost,
        saving: finding.saving,
        ticket,
        closed_at,
    }
}

/// The ticket already filed for the finding's rule and subject, with when it closed.
fn held_ticket(conn: &Connection, finding: &Finding) -> Result<Option<(String, Option<String>)>> {
    Ok(conn
        .query_row(
            "SELECT ticket, closed_at FROM findings
             WHERE rule = ?1 AND subject = ?2 AND ticket IS NOT NULL",
            params![finding.rule, finding.subject],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

/// Files the finding's ticket and records its estimated saving; returns its ID.
fn file(conn: &mut Connection, finding: &Finding, now: Timestamp) -> Result<String> {
    let create = Operation::Create {
        fields: TicketFields {
            title: Some(finding.title.clone()),
            // Nothing is scheduled until a person releases it, improvements included.
            status: Some(Status::PendingApproval),
            body: Some(finding.body.clone()),
            ticket_type: Some(TicketType::Task),
            priority: Some(2),
            labels: Some(vec![
                "improvement".to_owned(),
                format!("rule:{}", finding.rule),
            ]),
            metadata: Some([(BEFORE_KEY.to_owned(), json!(finding.cost))].into()),
            ..TicketFields::default()
        },
        parent: None,
        deps: Vec::new(),
        request_key: Some(format!(
            "improvement:{}:{}:{}",
            finding.rule,
            finding.subject,
            now.day()
        )),
    };
    let id = apply(conn, vec![create], now)?;
    let saving = Saving {
        rule: finding.rule.clone(),
        subject: finding.subject.clone(),
        unit: finding.unit.clone(),
        amount: finding.saving,
    };
    let saving = serde_json::to_value(saving).map_err(|e| StoreError::Invalid(e.to_string()))?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    set_columns(&tx, &id, ACTOR, &[("saving", saving)], now)?;
    tx.commit()?;
    Ok(id)
}

/// Applies the operations as the rules' actor; returns the first operation's ticket.
fn apply(conn: &mut Connection, ops: Vec<Operation>, now: Timestamp) -> Result<String> {
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    match batch::apply_automatic(conn, &request, ACTOR, "", now)? {
        BatchOutcome::Applied(reply) => Ok(reply.results[0].id.to_string()),
        BatchOutcome::Conflict(conflict) => Err(StoreError::Invalid(format!(
            "the rules' batch conflicted: {conflict:?}"
        ))),
    }
}

/// Comments on each improvement that landed seven or more days ago and has no measure after
/// yet; returns the tickets commented on.
fn follow_up(
    conn: &mut Connection,
    rules: &[Box<dyn Rule>],
    now: Timestamp,
) -> Result<Vec<String>> {
    let due = now.plus_seconds(-FOLLOW_UP_DAYS * 86_400.0).to_string();
    let landed: Vec<(String, String, String)> = {
        let mut statement = conn.prepare(
            "SELECT id, saving, metadata FROM tickets
             WHERE saving IS NOT NULL AND landed_at IS NOT NULL AND landed_at <= ?1
               AND json_extract(metadata, '$.' || ?2) IS NULL",
        )?;
        statement
            .query_map(params![due, AFTER_KEY], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut measured = Vec::new();
    for (id, saving, metadata) in landed {
        let Ok(saving) = serde_json::from_str::<Saving>(&saving) else {
            continue;
        };
        // A dashboard finding's rule is not one of Ignite's own; the efficiency rule measures it.
        let Some(rule) = rules
            .iter()
            .find(|rule| rule.name() == saving.rule)
            .or_else(|| rules.iter().find(|rule| rule.name() == "efficiency"))
        else {
            continue;
        };
        let after = rule.measure(conn, &saving.subject, now)?;
        let before = serde_json::from_str::<Value>(&metadata)
            .ok()
            .and_then(|meta| meta.get(BEFORE_KEY).and_then(Value::as_f64));
        let before = before.map_or_else(
            || "unknown".to_owned(),
            |cost| format!("{cost:.1} {}", saving.unit),
        );
        let text = format!(
            "Seven days after landing, `{}` measures {} at {after:.1} {} a week. Before this \
             ticket: {before} a week.",
            saving.rule, saving.subject, saving.unit
        );
        let ops = vec![
            Operation::Comment {
                id: id.clone(),
                text,
            },
            Operation::Update {
                id: id.clone(),
                set: None,
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: [(AFTER_KEY.to_owned(), json!(after))].into(),
                unset_meta: Vec::new(),
            },
        ];
        apply(conn, ops, now)?;
        measured.push(id);
    }
    Ok(measured)
}
