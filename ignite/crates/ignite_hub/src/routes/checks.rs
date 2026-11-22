//! The result cache, check and record routes: `/results/lookup`, `/results/store`, `/flaky`, `/flaky/batch`,
//! `/checks/stats`, `/checks/fix-outcomes`, `/tips/claim`, `/tips/verified`, `/landings/lookup`, `/regressions`, `/reverted`,
//! `/records`, `/metrics/import`, and the reports that file tickets: `/incomplete`, `/audits`,
//! `/finds` and `/tips/waiting`.
//! `/results/store` also files a ticket when a pass replaces another machine's failure.
//!
//! The tickets a report files are written by `crate::upkeep`, which also matches them against
//! earlier tickets.
//!
//! Each route decodes its body with the types of `ignite_core::protocol::hub`, calls the store in
//! `ignite_store::hub` and encodes the reply. Every route except `/results/lookup`,
//! `/landings/lookup` and the check history reads writes to the database and needs an
//! authorized writer (HTTP 401 otherwise).

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::de::DeserializeOwned;
use serde_json::json;

use ignite_core::fleet::ProviderSample;
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    AuditReport, ExplorationFind, FlakyReport, IncompleteReport, JusticiarReport, LandingsReply,
    LandingsRequest, LookupReply, LookupRequest, MetricsImport, NewResult, OpenRegression,
    OpenRegressions, RecordBatch, RegressionReport, ReportFiled, RevertFiled, Reverted,
    StoreRequest, TipClaim, TipVerified, TipWaiting, TipWaitingFiled,
};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::{
    checks, fix_outcomes, provider_usage, records, results, ticket_spend, tip_coverage, tips,
};

use super::{Request, Response};
use crate::upkeep::filing::Author;
use crate::upkeep::{explore, justiciar, machines, regressions, reverted, stuck};

/// Instructions are stored one by one; checks are costed in billions of them.
const BILLION: f64 = 1e9;

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    let (method, writes) = match request.path.as_str() {
        "/results/lookup" | "/landings/lookup" => ("POST", false),
        "/checks/stats"
        | "/checks/fix-outcomes"
        | "/regressions/open"
        | "/tips/candidate"
        | "/tips/coverage" => ("GET", false),
        "/results/store"
        | "/flaky"
        | "/flaky/batch"
        | "/tips/claim"
        | "/tips/verified"
        | "/regressions"
        | "/reverted"
        | "/records"
        | "/metrics/import"
        | "/incomplete"
        | "/audits"
        | "/finds"
        | "/justiciar"
        | "/tips/waiting"
        | "/tips/candidate/update"
        | "/tips/coverage/update" => ("POST", true),
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
        "/results/store" => store(conn, request, &body(request)?, now),
        "/flaky" => record_flakes(conn, &[body(request)?], now),
        "/flaky/batch" => record_flakes(conn, &body::<Vec<FlakyReport>>(request)?, now),
        "/checks/stats" => stats(conn, request),
        "/checks/fix-outcomes" => fixes(conn, request, now),
        "/regressions/open" => open_regressions(conn),
        "/tips/candidate" => {
            let candidate = tips::candidate(conn).map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!(candidate)))
        }
        "/tips/candidate/update" => {
            let updated = tips::update_candidate(conn, &body(request)?, now)
                .map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!(updated)))
        }
        "/tips/coverage" => {
            let coverage = tip_coverage::get(conn).map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!(coverage)))
        }
        "/tips/coverage/update" => {
            let updated =
                tip_coverage::update(conn, &body(request)?).map_err(|error| match error {
                    StoreError::Rejected(reason) => failure(409, &reason),
                    other => store_failure(&other),
                })?;
            Ok(Response::json(200, &json!(updated)))
        }
        "/tips/claim" => {
            let claim: TipClaim = body(request)?;
            let reply = tips::claim(conn, &claim, now).map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!(reply)))
        }
        "/tips/verified" => {
            let verified: TipVerified = body(request)?;
            tips::verified(conn, &verified, now).map_err(|e| store_failure(&e))?;
            if verified.state == ignite_core::protocol::hub::TipState::Verified {
                regressions::settle_after_change(conn, now);
            }
            Ok(done())
        }
        "/landings/lookup" => {
            let request: LandingsRequest = body(request)?;
            let commits =
                tips::landed_commits(conn, &request.commits).map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!(LandingsReply { commits })))
        }
        "/tips/waiting" => tip_waiting(conn, request, &body(request)?, now),
        "/regressions" => regression(conn, request, &body(request)?, now),
        "/reverted" => reverted(conn, request, &body(request)?, now),
        "/records" => {
            let batch: RecordBatch = body(request)?;
            let ids = records::insert(conn, &batch.records).map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!({ "ids": ids })))
        }
        "/metrics/import" => import_metrics(conn, &body(request)?, now),
        "/incomplete" | "/audits" | "/finds" | "/justiciar" => report(conn, request, now),
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
fn record_flakes(
    conn: &mut Connection,
    reports: &[FlakyReport],
    now: Timestamp,
) -> Result<Response, Response> {
    // Upkeep must see every flake in the command together to identify a machine fault.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| store_failure(&e.into()))?;
    for report in reports {
        checks::register(&tx, report.check.as_str()).map_err(|e| store_failure(&e))?;
        checks::record_flaky(&tx, report, now).map_err(|e| store_failure(&e))?;
    }
    tx.commit().map_err(|e| store_failure(&e.into()))?;
    Ok(done())
}

fn lookup(
    conn: &mut Connection,
    request: &LookupRequest,
    now: Timestamp,
) -> Result<Response, Response> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| store_failure(&e.into()))?;
    let found = results::lookup(&tx, &request.keys, now).map_err(|e| store_failure(&e))?;
    tx.commit().map_err(|e| store_failure(&e.into()))?;
    Ok(Response::json(200, &json!(LookupReply { results: found })))
}

/// Stores the results, then counts each new pass or failure as a run in the request's areas and
/// adds its cost. A result already stored from the same record (a batch sent twice) counts once.
/// A pass that replaces another machine's failure files a ticket for that machine (see
/// `upkeep::machines`); failing to file it never changes what was stored.
fn store(
    conn: &mut Connection,
    http: &Request,
    request: &StoreRequest,
    now: Timestamp,
) -> Result<Response, Response> {
    let mut fresh: Vec<&NewResult> = Vec::new();
    let mut replaced: Vec<(&NewResult, Stored)> = Vec::new();
    for result in &request.results {
        if !matches!(result.outcome, Outcome::Pass | Outcome::Fail) {
            continue;
        }
        let stored = stored(conn, result).map_err(|e| store_failure(&e))?;
        // A batch sent twice repeats the outcome and machine of the entry it already stored;
        // a different outcome or machine is a new run.
        let repeat = stored.as_ref().is_some_and(|s| {
            s.outcome == outcome_text(result.outcome) && s.machine == result.machine.as_str()
        });
        if !repeat {
            fresh.push(result);
        }
        if let Some(stored) = stored
            && result.outcome == Outcome::Pass
            && stored.outcome == "fail"
            && stored.machine != result.machine.as_str()
        {
            replaced.push((result, stored));
        }
    }
    results::store(conn, &request.results, now).map_err(|e| store_failure(&e))?;
    for result in &request.results {
        if matches!(result.outcome, Outcome::Pass | Outcome::Fail) {
            checks::register(conn, result.key.check.as_str()).map_err(|e| store_failure(&e))?;
        }
    }
    let writer = http.writer.as_ref().expect("checked in handle");
    let author = Author {
        actor: &writer.actor,
        machine: writer.machine.as_str(),
    };
    for (result, stored) in replaced {
        let seen = machines::Disagreement {
            key: &result.key,
            failed_on: &stored.machine,
            failed_output: stored.output.as_deref(),
            passed_on: result.machine.as_str(),
        };
        if let Err(error) = machines::disagreement(conn, author, &seen, now) {
            eprintln!("ignite hub: filing a machine disagreement: {error}");
        }
    }
    // Each family's start-up, estimated once for the request.
    let mut startups: BTreeMap<&str, checks::Startup> = BTreeMap::new();
    for result in fresh {
        let check = result.key.check.as_str();
        let failed = result.outcome == Outcome::Fail;
        let areas = if result.areas.is_empty() {
            &request.areas
        } else {
            &result.areas
        };
        checks::count_run(conn, check, areas, failed).map_err(|e| store_failure(&e))?;
        if let Some((cost, unit)) = record_cost(conn, result).map_err(|e| store_failure(&e))? {
            let family = result.key.check.family();
            let startup = if let Some(startup) = startups.get(family) {
                *startup
            } else {
                let startup = checks::startup(conn, family).map_err(|e| store_failure(&e))?;
                *startups.entry(family).or_insert(startup)
            };
            // A cost the result carries is its share of one command, start-up and all.
            let cost = checks::without_startup(cost, startup.in_unit(unit), result.cost.shared_by);
            checks::add_cost(conn, check, cost, unit).map_err(|e| store_failure(&e))?;
        }
    }
    Ok(done())
}

/// The cached entry for a result's key.
struct Stored {
    outcome: String,
    machine: String,
    output: Option<String>,
}

/// The cached entry for this result's key, if there is one.
fn outcome_text(outcome: Outcome) -> &'static str {
    if outcome == Outcome::Pass {
        "pass"
    } else {
        "fail"
    }
}

fn stored(conn: &Connection, result: &NewResult) -> Result<Option<Stored>, StoreError> {
    let platform = result.key.platform.map_or("", |os| os.as_str());
    let stored = conn
        .query_row(
            "SELECT outcome, machine, output FROM results
             WHERE check_name = ?1 AND hash = ?2 AND platform = ?3",
            params![
                result.key.check.as_str(),
                result.key.hash.as_str(),
                platform
            ],
            |row| {
                Ok(Stored {
                    outcome: row.get(0)?,
                    machine: row.get(1)?,
                    output: row.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(stored)
}

/// What the result says the check cost: billions of instructions where the machine counts
/// them, otherwise CPU seconds. A result with no cost of its own falls back to the hub's record
/// it names, if there is one; `None` when neither says.
fn record_cost(
    conn: &Connection,
    result: &NewResult,
) -> Result<Option<(f64, &'static str)>, StoreError> {
    #[allow(clippy::cast_precision_loss)]
    if let Some(count) = result.cost.instructions {
        return Ok(Some((count as f64 / BILLION, "instructions")));
    }
    #[allow(clippy::cast_precision_loss)]
    if result.cost.cpu_ms > 0 {
        return Ok(Some((result.cost.cpu_ms as f64 / 1000.0, "cpu_s")));
    }
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
    let mut reply = checks::stats(conn, family).map_err(|e| store_failure(&e))?;
    // Each `area` parameter names an area whose runs and failures to send; with none, every
    // area's are sent. The gate needs only the areas its change touches, and the `rust`
    // family's record in every area was 10 MB, which took 14 s to send.
    let areas: Vec<&str> = request
        .query
        .iter()
        .filter(|(key, _)| key == "area")
        .map(|(_, value)| value.as_str())
        .collect();
    if !areas.is_empty() {
        for check in &mut reply.checks {
            check.areas.retain(|a| areas.contains(&a.area.as_str()));
        }
    }
    Ok(Response::json(200, &json!(reply)))
}

/// The recent outcomes of one check by name, or of all checks for scoring.
fn fixes(conn: &Connection, request: &Request, now: Timestamp) -> Result<Response, Response> {
    let check = request
        .query
        .iter()
        .find(|(key, _)| key == "check")
        .map(|(_, value)| value.parse())
        .transpose()
        .map_err(|error| failure(400, &format!("{error}")))?;
    let products = request
        .query
        .iter()
        .any(|(key, value)| key == "outcome" && value == "product");
    let fixes = if products && check.is_none() {
        fix_outcomes::products(conn)
    } else {
        let since = now.plus_seconds(-fix_outcomes::HISTORY_DAYS * 86_400.0);
        fix_outcomes::between(conn, since, now, check.as_ref())
    }
    .map_err(|error| store_failure(&error))?;
    Ok(Response::json(200, &json!(fixes)))
}

/// Every check with an open regression and the open ticket that holds it.
fn open_regressions(conn: &Connection) -> Result<Response, Response> {
    let rows = tips::open_regressions(conn).map_err(|e| store_failure(&e))?;
    let regressions = rows
        .into_iter()
        .map(|(check, ticket)| {
            Ok(OpenRegression {
                check: check
                    .parse()
                    .map_err(|e| failure(500, &format!("check name {check:?}: {e}")))?,
                ticket: ticket
                    .parse()
                    .map_err(|e| failure(500, &format!("ticket ID {ticket:?}: {e}")))?,
            })
        })
        .collect::<Result<Vec<_>, Response>>()?;
    Ok(Response::json(200, &json!(OpenRegressions { regressions })))
}

/// Records the regression and returns the ticket for it (see `upkeep::regressions`).
fn regression(
    conn: &mut Connection,
    request: &Request,
    report: &RegressionReport,
    now: Timestamp,
) -> Result<Response, Response> {
    let writer = request.writer.as_ref().expect("checked in handle");
    let author = Author {
        actor: &writer.actor,
        machine: writer.machine.as_str(),
    };
    let (ticket, _) = regressions::file(conn, author, report, now).map_err(|e| match e {
        StoreError::Rejected(message) => failure(409, &message),
        other => store_failure(&other),
    })?;
    filed_reply(&ticket)
}

/// Reopens the ticket of a reverted landing (see `upkeep::reverted`).
fn reverted(
    conn: &mut Connection,
    request: &Request,
    report: &Reverted,
    now: Timestamp,
) -> Result<Response, Response> {
    let writer = request.writer.as_ref().expect("checked in handle");
    let author = Author {
        actor: &writer.actor,
        machine: writer.machine.as_str(),
    };
    let ticket = reverted::file(conn, author, report, now).map_err(|e| match e {
        StoreError::Rejected(message) => failure(409, &message),
        other => store_failure(&other),
    })?;
    let ticket = ticket
        .parse()
        .map_err(|error| failure(500, &format!("ticket ID {ticket:?}: {error}")))?;
    Ok(Response::json(200, &json!(RevertFiled { ticket })))
}

/// Files the tickets naming the machines a tip has waited an hour for.
fn tip_waiting(
    conn: &mut Connection,
    request: &Request,
    waiting: &TipWaiting,
    now: Timestamp,
) -> Result<Response, Response> {
    let writer = request.writer.as_ref().expect("checked in handle");
    let author = Author {
        actor: &writer.actor,
        machine: writer.machine.as_str(),
    };
    let filed = stuck::tip_waiting(conn, author, waiting, now).map_err(|e| store_failure(&e))?;
    let tickets = filed
        .iter()
        .map(|f| {
            f.ticket
                .parse()
                .map_err(|error| failure(500, &format!("ticket ID {:?}: {error}", f.ticket)))
        })
        .collect::<Result<_, _>>()?;
    Ok(Response::json(200, &json!(TipWaitingFiled { tickets })))
}

/// Files the ticket for one cause of incomplete checks, a cache audit that disagreed or an exploration
/// find, and returns it.
fn report(conn: &mut Connection, request: &Request, now: Timestamp) -> Result<Response, Response> {
    let writer = request.writer.as_ref().expect("checked in handle");
    let author = Author {
        actor: &writer.actor,
        machine: writer.machine.as_str(),
    };
    let filed = match request.path.as_str() {
        "/incomplete" => stuck::incomplete(conn, author, &body::<IncompleteReport>(request)?, now),
        "/audits" => stuck::audit(conn, author, &body::<AuditReport>(request)?, now),
        "/justiciar" => justiciar::report(conn, author, &body::<JusticiarReport>(request)?, now),
        _ => explore::find(conn, author, &body::<ExplorationFind>(request)?, now),
    }
    .map_err(|e| store_failure(&e))?;
    filed_reply(&filed.ticket)
}

fn filed_reply(ticket: &str) -> Result<Response, Response> {
    let ticket = ticket
        .parse()
        .map_err(|error| failure(500, &format!("ticket ID {ticket:?}: {error}")))?;
    Ok(Response::json(200, &json!(ReportFiled { ticket })))
}

/// Stores each row once per machine and source, however often a batch is sent.
fn import_metrics(
    conn: &mut Connection,
    request: &MetricsImport,
    now: Timestamp,
) -> Result<Response, Response> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| store_failure(&e.into()))?;
    let mut stored = 0_usize;
    if request.source == ticket_spend::SOURCE {
        for row in &request.rows {
            if ticket_spend::record(&tx, request.machine.as_str(), row, now)
                .map_err(|e| store_failure(&e))?
            {
                stored += 1;
            }
        }
        tx.commit().map_err(|e| store_failure(&e.into()))?;
        return Ok(Response::json(200, &json!({ "stored": stored })));
    }
    {
        let mut insert = tx
            .prepare_cached(
                "INSERT OR IGNORE INTO metrics (machine, source, data, at, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(|e| store_failure(&e.into()))?;
        for row in &request.rows {
            if request.source == "plan_usage"
                && let Some(sample) = provider_sample(request, row)
            {
                provider_usage::record(&tx, &sample).map_err(|e| store_failure(&e))?;
            }
            stored += insert
                .execute(params![
                    request.machine.as_str(),
                    request.source,
                    row.to_string(),
                    now.to_string(),
                    ignite_store::hub::metrics::kind_of(row)
                ])
                .map_err(|e| store_failure(&e.into()))?;
        }
    }
    tx.commit().map_err(|e| store_failure(&e.into()))?;
    Ok(Response::json(200, &json!({ "stored": stored })))
}

/// Older daemons only measured Claude. Their synthetic limit rows did not identify the
/// provider that ran out, so keep those rows in metrics without starting a hold.
fn provider_sample(request: &MetricsImport, row: &serde_json::Value) -> Option<ProviderSample> {
    let provider = row["provider"].as_str();
    let note = row["note"].as_str();
    let status = row["status"].as_str()?;
    if status != "limit" && note == Some("a CLI turn reached the plan's limit") {
        return None;
    }
    Some(ProviderSample {
        provider: provider.unwrap_or("claude").to_owned(),
        at: Timestamp::parse(row["at"].as_str()?).ok()?,
        machine: request.machine.clone(),
        status: status.to_owned(),
        label: row["window"].as_str().map(str::to_owned),
        percent_used: row["percent_used"].as_f64(),
        resets_at: row["resets_at"].as_str().map(str::to_owned),
        note: note.map(str::to_owned),
    })
}

#[cfg(test)]
#[path = "checks_tests.rs"]
mod checks_tests;
