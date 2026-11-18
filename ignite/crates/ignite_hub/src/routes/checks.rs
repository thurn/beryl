//! The result cache, check and record routes: `/results/lookup`, `/results/store`, `/flaky`,
//! `/checks/stats`, `/tips/claim`, `/tips/verified`, `/regressions`, `/records` and
//! `/metrics/import`.
//!
//! Each route decodes its body with the types of `ignite_core::protocol::hub`, calls the store in
//! `ignite_store::hub` and encodes the reply. Every route except `/results/lookup` and
//! `/checks/stats` changes the database, so it needs an authorized writer (HTTP 401 otherwise).

use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde_json::json;

use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    FlakyReport, LookupReply, LookupRequest, MetricsImport, NewResult, RecordBatch,
    RegressionFiled, RegressionReport, StoreRequest, TipClaim, TipVerified,
};
use ignite_core::protocol::tickets::{
    BatchRequest, Dependency, EdgeKind, Operation, TicketFields, TicketType,
};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{checks, records, results, tips};

use super::{Request, Response};

/// Instructions are stored one by one; checks are costed in billions of them.
const BILLION: f64 = 1e9;

/// The lines of a regression's output kept in its ticket.
const OUTPUT_LINES: usize = 40;

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    let (method, writes) = match request.path.as_str() {
        "/results/lookup" => ("POST", false),
        "/checks/stats" => ("GET", false),
        "/results/store" | "/flaky" | "/tips/claim" | "/tips/verified" | "/regressions"
        | "/records" | "/metrics/import" => ("POST", true),
        _ => return None,
    };
    if request.method != method {
        return Some(failure(405, &format!("use {method} for {}", request.path)));
    }
    if writes && request.writer.is_none() {
        return Some(failure(401, "writes need a registered machine's token"));
    }
    Some(route(conn, request, now).unwrap_or_else(|response| response))
}

fn route(conn: &mut Connection, request: &Request, now: Timestamp) -> Result<Response, Response> {
    match request.path.as_str() {
        "/results/lookup" => lookup(conn, &body(request)?, now),
        "/results/store" => store(conn, &body(request)?, now),
        "/flaky" => {
            let report: FlakyReport = body(request)?;
            checks::record_flaky(conn, &report, now).map_err(|e| store_failure(&e))?;
            Ok(done())
        }
        "/checks/stats" => stats(conn, request),
        "/tips/claim" => {
            let claim: TipClaim = body(request)?;
            let reply = tips::claim(conn, &claim, now).map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!(reply)))
        }
        "/tips/verified" => {
            let verified: TipVerified = body(request)?;
            tips::verified(conn, &verified, now).map_err(|e| store_failure(&e))?;
            Ok(done())
        }
        "/regressions" => regression(conn, request, &body(request)?, now),
        "/records" => {
            let batch: RecordBatch = body(request)?;
            let ids = records::insert(conn, &batch.records).map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!({ "ids": ids })))
        }
        "/metrics/import" => import_metrics(conn, &body(request)?, now),
        other => Err(failure(404, &format!("no route {other}"))),
    }
}

fn done() -> Response {
    Response::json(200, &json!({ "ok": true }))
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

fn store_failure(error: &StoreError) -> Response {
    failure(500, &error.to_string())
}

fn body<T: DeserializeOwned>(request: &Request) -> Result<T, Response> {
    serde_json::from_slice(&request.body)
        .map_err(|error| failure(400, &format!("bad request body: {error}")))
}

/// Answers every key in one transaction, so a request of thousands of keys is one commit.
fn lookup(
    conn: &mut Connection,
    request: &LookupRequest,
    now: Timestamp,
) -> Result<Response, Response> {
    let tx = conn.transaction().map_err(|e| store_failure(&e.into()))?;
    let found = results::lookup(&tx, &request.keys, now).map_err(|e| store_failure(&e))?;
    tx.commit().map_err(|e| store_failure(&e.into()))?;
    Ok(Response::json(200, &json!(LookupReply { results: found })))
}

/// Stores the results, then counts each new pass or failure as a run in the request's areas and
/// adds its cost. A result already stored from the same record (a batch sent twice) counts once.
fn store(
    conn: &mut Connection,
    request: &StoreRequest,
    now: Timestamp,
) -> Result<Response, Response> {
    let mut fresh: Vec<&NewResult> = Vec::new();
    for result in &request.results {
        if !matches!(result.outcome, Outcome::Pass | Outcome::Fail) {
            continue;
        }
        let stored = stored_record(conn, result).map_err(|e| store_failure(&e))?;
        if stored != Some(result.record.0) {
            fresh.push(result);
        }
    }
    results::store(conn, &request.results, now).map_err(|e| store_failure(&e))?;
    for result in fresh {
        let check = result.key.check.as_str();
        let failed = result.outcome == Outcome::Fail;
        checks::count_run(conn, check, &request.areas, failed).map_err(|e| store_failure(&e))?;
        if let Some((cost, unit)) = record_cost(conn, result).map_err(|e| store_failure(&e))? {
            checks::add_cost(conn, check, cost, unit).map_err(|e| store_failure(&e))?;
        }
    }
    Ok(done())
}

/// The record behind the cached entry for this key, if there is one.
fn stored_record(conn: &Connection, result: &NewResult) -> Result<Option<u64>, StoreError> {
    let platform = result.key.platform.map_or("", |os| os.as_str());
    let record: Option<i64> = conn
        .query_row(
            "SELECT record FROM results WHERE check_name = ?1 AND hash = ?2 AND platform = ?3",
            params![
                result.key.check.as_str(),
                result.key.hash.as_str(),
                platform
            ],
            |row| row.get(0),
        )
        .optional()?;
    Ok(record.and_then(|id| u64::try_from(id).ok()))
}

/// What the result's record says the check cost: billions of instructions where the machine
/// counts them, otherwise CPU seconds. `None` when the hub holds no such record.
fn record_cost(
    conn: &Connection,
    result: &NewResult,
) -> Result<Option<(f64, &'static str)>, StoreError> {
    let id = i64::try_from(result.record.0).unwrap_or(i64::MAX);
    let cost: Option<(Option<f64>, f64)> = conn
        .query_row(
            "SELECT instructions, cpu_s FROM records WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(cost.map(|(instructions, cpu_s)| match instructions {
        Some(count) => (count / BILLION, "instructions"),
        None => (cpu_s, "cpu_s"),
    }))
}

fn stats(conn: &Connection, request: &Request) -> Result<Response, Response> {
    let family = request
        .query
        .iter()
        .find(|(key, _)| key == "family")
        .map(|(_, value)| value.as_str())
        .ok_or_else(|| failure(400, "missing the family parameter"))?;
    let reply = checks::stats(conn, family).map_err(|e| store_failure(&e))?;
    Ok(Response::json(200, &json!(reply)))
}

/// Records the regression and returns the ticket for it. A check with no regression gets a new P0
/// ticket; one whose regression was fixed gets its old ticket reopened with the new output; one
/// that is already open keeps its ticket and nothing changes.
fn regression(
    conn: &mut Connection,
    request: &Request,
    report: &RegressionReport,
    now: Timestamp,
) -> Result<Response, Response> {
    let writer = request.writer.as_ref().expect("checked in handle");
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT ticket, state FROM regressions WHERE check_name = ?1",
            params![report.check.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| store_failure(&e.into()))?;
    let ops = match &existing {
        Some((_, state)) if state != tips::FIXED => Vec::new(),
        Some((ticket, _)) => vec![
            Operation::Reopen { id: ticket.clone() },
            Operation::Comment {
                id: ticket.clone(),
                text: details(report),
            },
        ],
        None => vec![create_operation(report)],
    };
    let mut ticket = existing.map(|(ticket, _)| ticket);
    if !ops.is_empty() {
        let batch = BatchRequest {
            preconditions: Vec::new(),
            ops,
        };
        let outcome = batch::apply(conn, &batch, &writer.actor, writer.machine.as_str(), now)
            .map_err(|e| store_failure(&e))?;
        match outcome {
            BatchOutcome::Applied(reply) => {
                ticket = Some(reply.results[0].id.to_string());
            }
            BatchOutcome::Conflict(conflict) => {
                return Err(Response::json(409, &json!(conflict)));
            }
        }
    }
    let ticket = ticket.expect("a ticket exists or was just filed");
    tips::regression(conn, report, &ticket).map_err(|e| store_failure(&e))?;
    let filed = RegressionFiled {
        ticket: ticket
            .parse()
            .map_err(|error| failure(500, &format!("ticket ID {ticket:?}: {error}")))?,
    };
    Ok(Response::json(200, &json!(filed)))
}

fn create_operation(report: &RegressionReport) -> Operation {
    let family = report
        .check
        .as_str()
        .split_once(':')
        .map_or(report.check.as_str(), |(family, _)| family);
    let deps = report
        .caused_by
        .iter()
        .map(|ticket| Dependency {
            on: ticket.to_string(),
            kind: Some(EdgeKind::DiscoveredFrom),
        })
        .collect();
    Operation::Create {
        fields: TicketFields {
            title: Some(format!("Regression: {}", report.check)),
            body: Some(details(report)),
            ticket_type: Some(TicketType::Bug),
            priority: Some(0),
            labels: Some(vec!["regression".to_owned(), format!("check:{family}")]),
            ..TicketFields::default()
        },
        parent: None,
        deps,
        request_key: Some(format!("regression:{}:{}", report.check, report.first_bad)),
    }
}

/// The ticket text: what failed, between which commits, and the end of its output.
fn details(report: &RegressionReport) -> String {
    let lines: Vec<&str> = report.output.lines().collect();
    let tail = lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n");
    let caused_by = report
        .caused_by
        .as_ref()
        .map_or_else(String::new, |ticket| format!("caused-by {ticket}\n"));
    format!(
        "{caused_by}first bad {}, last good {}\nrerun: ignite check {}\n\noutput (last {OUTPUT_LINES} lines):\n{tail}",
        report.first_bad, report.last_good, report.check
    )
}

/// Stores each row once per machine and source, however often a batch is sent.
fn import_metrics(
    conn: &mut Connection,
    request: &MetricsImport,
    now: Timestamp,
) -> Result<Response, Response> {
    let tx = conn.transaction().map_err(|e| store_failure(&e.into()))?;
    let mut stored = 0_usize;
    {
        let mut insert = tx
            .prepare_cached(
                "INSERT OR IGNORE INTO metrics (machine, source, data, at) VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(|e| store_failure(&e.into()))?;
        for row in &request.rows {
            stored += insert
                .execute(params![
                    request.machine.as_str(),
                    request.source,
                    row.to_string(),
                    now.to_string()
                ])
                .map_err(|e| store_failure(&e.into()))?;
        }
    }
    tx.commit().map_err(|e| store_failure(&e.into()))?;
    Ok(Response::json(200, &json!({ "stored": stored })))
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod checks_tests;
