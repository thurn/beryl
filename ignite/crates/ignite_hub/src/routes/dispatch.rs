//! The fleet and hand-off routes, including reservations, claims and ticket completion.
//!
//! Reservations, claims and their release are transactional. A reservation can be claimed
//! only with its one-time code by its machine, and lapses if unclaimed after ten minutes.

use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use ignite_core::dispatch::label;
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::emergency::{EmergencyChange, EmergencyReply};
use ignite_core::protocol::hub::{
    AdhocClaim, Claim, Close, DependencyNote, FixOutcome, Heartbeat, Landed, LandedReply,
    MachineFree, NextReply, Pause, Release, SessionTitle, TicketBrief,
};
use ignite_core::protocol::tickets::{
    BatchRequest, CLI_LANDING_KEY, CliLanding, Operation, OperationResult, Status, Ticket,
    TicketFields, cli_provider,
};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::fix_outcomes::LandingFix;
use ignite_store::hub::{
    edges, emergency, feed, findings, fleet, machine_asks, session_titles, tickets,
};

use super::{Request, Response};
use crate::upkeep::regressions;

#[path = "dispatch_machines.rs"]
mod machines;

#[path = "dispatch_families.rs"]
mod families;
pub use families::publish as publish_check_families;

#[path = "dispatch_usage.rs"]
mod usage;

#[path = "dispatch_queue.rs"]
mod queue;

#[path = "dispatch_mix.rs"]
mod mix;

#[path = "dispatch_placement.rs"]
mod placement;

#[path = "dispatch_releases.rs"]
mod counting;

#[path = "dispatch_closing.rs"]
mod closing;
use closing::closing;

pub use queue::{named_files, resolve_named_file, view};

/// The label on a ticket filed by `ignite start "<summary>"`.
const ADHOC: &str = "adhoc";

/// The ticket metadata key holding the branch on the remote where a release saved the ticket's
/// work, until the ticket lands or closes.
pub const SAVED_BRANCH: &str = "saved_branch";

/// The `meta` key holding the Ignite commit the hub runs, which the server sets when it starts.
pub const HUB_VERSION: &str = "hub_version";

/// The response if this module owns the request's path.
pub fn handle(
    conn: &mut Connection,
    request: &Request,
    now: Timestamp,
    test_paths: &[String],
) -> Option<Response> {
    let writes = match request.path.as_str() {
        // A machine registering for the first time has no token yet; registration is how it
        // gets one. One that already has a token must present it (see `register`).
        "/machines/register" => false,
        "/machines/heartbeat"
        | "/next"
        | "/reservations/refuse"
        | "/claims"
        | "/landed"
        | "/release"
        | "/close"
        | "/pause"
        | "/session-titles"
        | "/emergency" => true,
        _ => return None,
    };
    if request.method != "POST" {
        return Some(failure(405, &format!("use POST for {}", request.path)));
    }
    if writes && request.writer.is_none() {
        return Some(failure(401, "writes need a registered machine's token"));
    }
    Some(route(conn, request, now, test_paths).unwrap_or_else(|response| response))
}

/// The ticket's ID, whichever form it was named in: with or without the leading `T`, or by an
/// alias, as `GET /tickets/<name>` accepts. 404 for a name that is no ticket.
fn canonical(conn: &Connection, name: &TicketId) -> Result<TicketId, Response> {
    tickets::resolve(conn, name.as_str())
        .map_err(|e| store_failure(&e))?
        .ok_or_else(|| failure(404, &format!("Unknown ticket {name}")))
}

/// 403 unless the machine a request names in its body is the machine whose token signed it, so
/// that one machine cannot claim, or act, as another.
fn same_machine(request: &Request, named: &MachineId) -> Result<(), Response> {
    match &request.writer {
        Some(writer) if &writer.machine == named => Ok(()),
        _ => Err(failure(
            403,
            &format!("this token does not belong to machine {named}"),
        )),
    }
}

/// Records the Ignite commit the hub runs, for heartbeat replies.
pub fn set_hub_version(conn: &Connection, version: &str) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![HUB_VERSION, version],
    )?;
    Ok(())
}

type Answer = Result<Response, Response>;

fn route(
    conn: &mut Connection,
    request: &Request,
    now: Timestamp,
    test_paths: &[String],
) -> Answer {
    match request.path.as_str() {
        "/machines/register" => machines::register(conn, request, &body(request)?, now),
        "/session-titles" => {
            let parsed: SessionTitle = body(request)?;
            same_machine(request, &parsed.machine)?;
            session_titles::record(conn, parsed.session.as_str(), &parsed.title, now)
                .map_err(|e| store_failure(&e))?;
            Ok(Response::json(200, &json!({})))
        }
        "/machines/heartbeat" => {
            let parsed: Heartbeat = body(request)?;
            same_machine(request, &parsed.machine)?;
            machines::heartbeat(conn, &parsed, now)
        }
        "/next" => {
            let parsed: MachineFree = body(request)?;
            same_machine(request, &parsed.machine)?;
            next(conn, &parsed, now)
        }
        "/reservations/refuse" => machines::refuse_reservation(conn, request, &body(request)?),
        "/claims" => {
            let value: Value = body(request)?;
            if value.get("summary").is_some() {
                let adhoc_claim: AdhocClaim = parse(value)?;
                same_machine(request, &adhoc_claim.machine)?;
                adhoc(conn, &adhoc_claim, now)
            } else {
                let mut ticket_claim: Claim = parse(value)?;
                same_machine(request, &ticket_claim.machine)?;
                ticket_claim.ticket = canonical(conn, &ticket_claim.ticket)?;
                claim(conn, &ticket_claim, now)
            }
        }
        "/landed" => {
            let mut parsed: Landed = body(request)?;
            same_machine(request, &parsed.machine)?;
            parsed.ticket = canonical(conn, &parsed.ticket)?;
            landed(conn, &parsed, now, test_paths)
        }
        "/release" => {
            let mut parsed: Release = body(request)?;
            same_machine(request, &parsed.machine)?;
            parsed.ticket = canonical(conn, &parsed.ticket)?;
            counting::release(conn, &parsed, now)
        }
        "/close" => {
            let mut parsed: Close = body(request)?;
            same_machine(request, &parsed.machine)?;
            parsed.ticket = canonical(conn, &parsed.ticket)?;
            close(conn, &parsed, now)
        }
        "/pause" => {
            // Any machine may pause every machine, as `ignite pool pause --all` does, but only
            // a machine itself may pause or resume it alone.
            let parsed: Pause = body(request)?;
            if let Some(machine) = &parsed.machine {
                same_machine(request, machine)?;
            }
            machines::pause(conn, &parsed, request, now)
        }
        "/emergency" => {
            // Tickets are stored by their canonical ID, which is how a daemon names the
            // ticket a tree holds.
            let mut parsed: EmergencyChange = body(request)?;
            match &mut parsed {
                EmergencyChange::Declare {
                    machine, ticket, ..
                } => {
                    same_machine(request, machine)?;
                    *ticket = canonical(conn, ticket)?;
                }
                EmergencyChange::Allow { allowance, .. } => {
                    allowance.ticket = canonical(conn, &allowance.ticket)?;
                }
            }
            let changed = emergency::change(conn, &parsed, now).map_err(|error| match error {
                StoreError::Rejected(message) => failure(409, &message),
                other => store_failure(&other),
            })?;
            Ok(Response::json(
                200,
                &json!(EmergencyReply {
                    emergency: Some(changed),
                }),
            ))
        }
        other => Err(failure(404, &format!("no route {other}"))),
    }
}

fn next(conn: &mut Connection, free: &MachineFree, now: Timestamp) -> Answer {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    fleet::lapse(&tx, now).map_err(|e| store_failure(&e))?;
    let mut free = free.clone();
    if let Some((name, os, families)) =
        fleet::dispatch_capabilities(&tx, free.machine.as_str()).map_err(|e| store_failure(&e))?
    {
        if os != free.os {
            return Err(failure(
                400,
                "request OS differs from the machine's registration",
            ));
        }
        free.name = Some(name);
        free.families = families;
    }
    if free.name.is_none() {
        free.name = Some(free.machine.to_string());
    }
    let mut answer = queue::next(&tx, &free, now).map_err(|e| store_failure(&e))?;
    if free.room.is_none()
        && let Ok(reservation) = &mut answer
    {
        reservation.provider = None;
    }
    let said = answer
        .as_ref()
        .map(|reservation| reservation.ticket.as_str())
        .map_err(|wait| wait.reason.as_str());
    machine_asks::record(&tx, &free, said, now).map_err(|e| store_failure(&e))?;
    let reply = match answer {
        Ok(reservation) => NextReply::Reserved(reservation),
        // Only a daemon that asked for the reason gets it; an older one reads `null`.
        Err(wait) => NextReply::Nothing(free.explain.then_some(wait.reason)),
    };
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(reply)))
}

/// Claims a ticket for the session and tree, or returns the existing brief on retry.
/// The checks, claim and reservation removal are one `IMMEDIATE` transaction.
fn claim(conn: &mut Connection, claim: &Claim, now: Timestamp) -> Answer {
    let id = claim.ticket.as_str();
    let mut tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    fleet::lapse(&tx, now).map_err(|e| store_failure(&e))?;
    let ticket = load(&tx, id)?;
    if ticket.status == Status::Closed {
        return Err(conflict(&format!("{id} is closed"), Some(&ticket)));
    }
    if ticket.status == Status::InProgress
        && ticket.assignee.as_deref() == Some(claim.session.as_str())
    {
        // A retry after a lost reply: the first claim already succeeded and took the
        // reservation, so answer as it did.
        let brief = brief(&tx, id)?;
        tx.commit().map_err(sql_failure)?;
        return Ok(Response::json(200, &json!(brief)));
    }
    if ticket.assignee.is_none()
        && let Some(passing) = regressions::passing(&tx, &ticket).map_err(|e| store_failure(&e))?
    {
        passing
            .close(&mut tx, &ticket, now)
            .map_err(|e| store_failure(&e))?;
        let closed = load(&tx, id)?;
        tx.commit().map_err(sql_failure)?;
        return Err(conflict(&format!("{id} is closed"), Some(&closed)));
    }
    let reservation = fleet::reservation_of(&tx, id).map_err(|e| store_failure(&e))?;
    let refused = match (&reservation, &claim.code) {
        (Some((machine, code, _)), Some(sent)) => (machine != claim.machine.as_str()
            || code != sent)
            .then_some("is reserved under another code"),
        (Some(_), None) => Some(
            "is reserved for a worker that is starting; nobody else can claim it, so take another ticket",
        ),
        (None, Some(_)) => Some("has no live reservation with that code"),
        (None, None) => None,
    };
    if let Some(refused) = refused {
        return Err(conflict(&format!("{id} {refused}"), Some(&ticket)));
    }
    // A batch claim without a reservation code cannot take a live reservation.
    // Consume the validated reservation here, in the same transaction as the claim.
    let provider = if let Some((_, _, provider)) = reservation {
        fleet::release_reservation(&tx, id).map_err(|e| store_failure(&e))?;
        provider
    } else {
        None
    };
    let ops = vec![Operation::Claim { id: id.to_owned() }];
    apply_in(&mut tx, ops, &claim.session, &claim.machine, now)?;
    let brief = take(&tx, claim, provider.as_deref(), now)?;
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(brief)))
}

/// Files a ticket from the summary, labelled `adhoc`, and claims it in the same transaction.
fn adhoc(conn: &mut Connection, request: &AdhocClaim, now: Timestamp) -> Answer {
    let fields = TicketFields {
        title: Some(request.summary.clone()),
        labels: Some(vec![ADHOC.to_owned()]),
        ..TicketFields::default()
    };
    let ops = vec![
        Operation::Create {
            fields,
            parent: None,
            deps: Vec::new(),
            request_key: None,
        },
        Operation::Claim { id: "$0".into() },
    ];
    let mut tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    let results = apply_in(&mut tx, ops, &request.session, &request.machine, now)?;
    let claim = Claim {
        ticket: results[0].id.clone(),
        machine: request.machine.clone(),
        session: request.session.clone(),
        tree: request.tree.clone(),
        code: None,
    };
    let brief = take(&tx, &claim, None, now)?;
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(brief)))
}

/// Records the session and tree working the ticket and returns its brief in the claim's
/// transaction.
fn take(
    tx: &Connection,
    claim: &Claim,
    provider: Option<&str>,
    now: Timestamp,
) -> Result<TicketBrief, Response> {
    let id = claim.ticket.as_str();
    set_columns(
        tx,
        id,
        claim.session.as_str(),
        &[
            ("session", Value::from(claim.session.as_str())),
            ("tree", Value::from(claim.tree.as_str())),
            ("provider", json!(provider)),
        ],
        now,
    )
    .map_err(|e| store_failure(&e))?;
    brief(tx, id)
}

/// The ticket and what each ticket it depends on left behind.
fn brief(conn: &Connection, id: &str) -> Result<TicketBrief, Response> {
    let ticket = load(conn, id)?;
    let mut needs = Vec::new();
    for edge in edges::from(conn, id).map_err(|e| store_failure(&e))? {
        let dependency = load(conn, edge.dst.as_str())?;
        needs.push(DependencyNote {
            ticket: dependency.id,
            title: dependency.title,
            landed: dependency.landed,
            close_reason: dependency.close_reason,
        });
    }
    let saved_branch = ticket
        .metadata
        .get(SAVED_BRANCH)
        .and_then(Value::as_str)
        .map(str::to_owned);
    let attachments = ignite_store::hub::attachments::list(conn, id);
    Ok(TicketBrief {
        ticket,
        needs,
        saved_branch,
        attachments: attachments.map_err(|e| store_failure(&e))?,
    })
}

/// Records what landed, closes the ticket, frees the worker and reserves the next ticket of
/// its chain for the same session, all in one `IMMEDIATE` transaction, so an interruption
/// leaves nothing half done. A repeat returns the same reservation while it can still be used.
/// It leaves the worker alone because the worker may already be working on another ticket.
fn landed(conn: &mut Connection, landed: &Landed, now: Timestamp, test_paths: &[String]) -> Answer {
    let id = landed.ticket.as_str();
    let mut tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    let ticket = load(&tx, id)?;
    if ticket.status == Status::Closed && ticket.session.as_ref() == Some(&landed.session) {
        if ticket.landed.as_ref() != Some(&landed.work)
            || queue::chain_machine(&tx, &landed.machine)
                .map_err(|e| store_failure(&e))?
                .is_none()
        {
            return Ok(Response::json(200, &json!(LandedReply { next: None })));
        }
        let next = fleet::reservation_for_landing(
            &tx,
            landed.machine.as_str(),
            landed.session.as_str(),
            id,
            now,
        )
        .map_err(|e| store_failure(&e))?;
        return Ok(Response::json(200, &json!(LandedReply { next })));
    }
    holds(&ticket, &landed.session)?;
    let provider: Option<String> = tx
        .query_row("SELECT provider FROM tickets WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .map_err(sql_failure)?;
    fleet::lapse(&tx, now).map_err(|e| store_failure(&e))?;
    let prior_commits = ticket.landed.map_or_else(Vec::new, |work| work.commits);
    let mut metadata = ticket.metadata;
    if let Some(app) = cli_provider(&landed.session) {
        metadata.insert(
            CLI_LANDING_KEY.to_owned(),
            json!(CliLanding {
                machine: landed.machine.clone(),
                provider: provider.as_deref().unwrap_or(app).to_owned(),
            }),
        );
    } else {
        metadata.remove(CLI_LANDING_KEY);
    }
    let work = serde_json::to_value(&landed.work).map_err(|e| failure(500, &e.to_string()))?;
    set_columns(
        &tx,
        id,
        landed.session.as_str(),
        &[
            ("landed", work),
            ("landed_at", json!(now)),
            ("metadata", json!(metadata)),
            ("provider", Value::Null),
        ],
        now,
    )
    .map_err(|e| store_failure(&e))?;
    let fix = LandingFix {
        outcome: FixOutcome::classify(&landed.work.files, test_paths),
        commit: landed.work.commits.last().cloned(),
        prior_commits,
    };
    close_after_landing(&mut tx, landed, now, &fix)?;
    free_worker(
        &tx,
        &landed.machine,
        &landed.session,
        Some(&landed.work.files),
    )?;
    findings::close_ticket(&tx, id, now).map_err(|e| store_failure(&e))?;
    let ticket = load(&tx, id)?;
    let lane = label(&ticket.labels, "lane:").filter(|_| !landed.defer_successor);
    let next = match lane {
        Some(lane) => queue::chain_next(
            &tx,
            lane,
            &landed.machine,
            &landed.session,
            provider.as_deref(),
            now,
            landed.pressure.as_ref(),
        )
        .map_err(|e| store_failure(&e))?,
        None => None,
    };
    if let Some(reservation) = &next {
        fleet::link_landing(&tx, reservation.ticket.as_str(), id).map_err(|e| store_failure(&e))?;
    }
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(LandedReply { next })))
}

fn close_after_landing(
    tx: &mut Transaction,
    landed: &Landed,
    now: Timestamp,
    fix: &LandingFix,
) -> Result<(), Response> {
    let id = landed.ticket.as_str();
    let reason = landed.work.note.lines().next().unwrap_or("").trim();
    let reason = if reason.is_empty() { "landed" } else { reason };
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: closing(&load(tx, id)?, reason),
    };
    batch_result(batch::apply_landing_in(
        tx,
        &request,
        landed.session.as_str(),
        landed.machine.as_str(),
        now,
        (id, fix),
    ))?;
    Ok(())
}

/// Closes a ticket that needs no landing (`ignite close`) and frees its worker, in one
/// transaction. A repeat of a close that already happened answers as the first did, and
/// completes whatever an interrupted first one left undone.
fn close(conn: &mut Connection, close: &Close, now: Timestamp) -> Answer {
    let id = close.ticket.as_str();
    let mut tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    let ticket = load(&tx, id)?;
    if !(ticket.status == Status::Closed && ticket.session.as_ref() == Some(&close.session)) {
        holds(&ticket, &close.session)?;
        let reason = close.reason.trim();
        let reason = if reason.is_empty() { "closed" } else { reason };
        set_columns(
            &tx,
            id,
            close.session.as_str(),
            &[("provider", Value::Null)],
            now,
        )
        .map_err(|e| store_failure(&e))?;
        apply_in(
            &mut tx,
            closing(&ticket, reason),
            &close.session,
            &close.machine,
            now,
        )?;
        if let Some(passing) = regressions::passing(&tx, &ticket).map_err(|e| store_failure(&e))? {
            passing.mark_fixed(&tx).map_err(|e| store_failure(&e))?;
        }
    }
    free_worker(&tx, &close.machine, &close.session, None)?;
    findings::close_ticket(&tx, id, now).map_err(|e| store_failure(&e))?;
    tx.commit().map_err(sql_failure)?;
    Ok(done())
}

/// 409 unless `session` holds the open ticket: the claim was lost to another session, or the
/// ticket was released or closed meanwhile.
fn holds(ticket: &Ticket, session: &SessionId) -> Result<(), Response> {
    if ticket.status != Status::Closed && ticket.assignee.as_deref() == Some(session.as_str()) {
        return Ok(());
    }
    Err(conflict(
        &format!("{} is not held by {session}", ticket.id),
        Some(ticket),
    ))
}

/// Marks the worker idle with no ticket, and remembers the files its ticket touched.
fn free_worker(
    conn: &Connection,
    machine: &MachineId,
    session: &SessionId,
    files: Option<&[String]>,
) -> Result<(), Response> {
    let files = files.map(|files| serde_json::to_string(files).unwrap_or_else(|_| "[]".to_owned()));
    conn.execute(
        "UPDATE workers SET ticket = NULL, state = 'idle', files = COALESCE(?3, files)
         WHERE machine = ?1 AND session = ?2",
        params![machine.as_str(), session.as_str(), files],
    )
    .map_err(sql_failure)?;
    Ok(())
}

/// Applies a batch as `session` on `machine` inside a transaction the caller holds, mapping a
/// conflict to 409 and an unknown ticket to 404. A failure leaves the batch undone and
/// the caller drops the transaction.
fn apply_in(
    tx: &mut Transaction,
    ops: Vec<Operation>,
    session: &SessionId,
    machine: &MachineId,
    now: Timestamp,
) -> Result<Vec<OperationResult>, Response> {
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops,
    };
    batch_result(batch::apply_in(
        tx,
        &request,
        session.as_str(),
        machine.as_str(),
        now,
    ))
}

fn batch_result(
    result: Result<BatchOutcome, StoreError>,
) -> Result<Vec<OperationResult>, Response> {
    match result {
        Ok(BatchOutcome::Applied(reply)) => Ok(reply.results),
        Ok(BatchOutcome::Conflict(found)) => Err(conflict(
            "another session holds the ticket",
            found.current.as_ref(),
        )),
        Err(StoreError::NotFound(message)) => Err(failure(404, &message)),
        Err(StoreError::Rejected(message)) => Err(failure(400, &message)),
        Err(other) => Err(store_failure(&other)),
    }
}

/// Writes ticket columns that the batch's fields do not cover (the session, tree and what
/// landed), as one change: the next feed position becomes the ticket's version, with a history
/// row of `{column: [old, new]}`, so that a reader caching on the feed position sees it.
pub(crate) fn set_columns(
    conn: &Connection,
    id: &str,
    actor: &str,
    columns: &[(&str, Value)],
    now: Timestamp,
) -> Result<(), StoreError> {
    let mut change = Map::new();
    for (column, new) in columns {
        let old: Option<String> = conn.query_row(
            &format!("SELECT {column} FROM tickets WHERE id = ?1"),
            [id],
            |row| row.get(0),
        )?;
        let old = old.map_or(Value::Null, |text| {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        });
        if old != *new {
            change.insert((*column).to_owned(), json!([old, new]));
        }
    }
    if change.is_empty() {
        return Ok(());
    }
    let seq = feed::next_seq(conn)?;
    for (column, new) in columns {
        let text = match new {
            Value::Null => None,
            Value::String(text) => Some(text.clone()),
            other => Some(other.to_string()),
        };
        conn.execute(
            &format!("UPDATE tickets SET {column} = ?1 WHERE id = ?2"),
            params![text, id],
        )?;
    }
    conn.execute(
        "UPDATE tickets SET updated_at = ?1, version = ?2 WHERE id = ?3",
        params![now.to_string(), seq.0 as i64, id],
    )?;
    conn.execute(
        "INSERT INTO history (ticket, seq, actor, at, change) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id,
            seq.0 as i64,
            actor,
            now.to_string(),
            Value::Object(change).to_string()
        ],
    )?;
    feed::record(
        conn,
        &ignite_core::protocol::tickets::FeedChange {
            seq,
            kind: "ticket".to_owned(),
            ticket: id.parse().ok(),
            name: None,
        },
    )
}

fn load(conn: &Connection, id: &str) -> Result<Ticket, Response> {
    tickets::load(conn, id)
        .map_err(|e| store_failure(&e))?
        .ok_or_else(|| failure(404, &format!("Unknown ticket {id}")))
}

fn conflict(message: &str, current: Option<&Ticket>) -> Response {
    Response::json(409, &json!({ "error": message, "current": current }))
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

#[allow(clippy::needless_pass_by_value)]
fn sql_failure(error: rusqlite::Error) -> Response {
    store_failure(&StoreError::Sqlite(error))
}

fn body<T: DeserializeOwned>(request: &Request) -> Result<T, Response> {
    serde_json::from_slice(&request.body)
        .map_err(|error| failure(400, &format!("bad request body: {error}")))
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, Response> {
    serde_json::from_value(value)
        .map_err(|error| failure(400, &format!("bad request body: {error}")))
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dispatch_security_tests.rs"]
mod security_tests;

#[cfg(test)]
#[path = "dispatch_allocation_tests.rs"]
mod allocation_tests;

#[cfg(test)]
#[path = "dispatch_hardening_tests.rs"]
mod hardening_tests;

#[cfg(test)]
#[path = "dispatch_pressure_tests.rs"]
mod pressure_tests;

#[cfg(test)]
#[path = "dispatch_emergency_tests.rs"]
mod emergency_tests;

#[cfg(test)]
#[path = "dispatch_pins_tests.rs"]
mod pins_tests;

#[cfg(test)]
#[path = "dispatch_release_count_tests.rs"]
mod release_count_tests;

#[cfg(test)]
#[path = "dispatch_usage_tests.rs"]
mod usage_tests;

#[cfg(test)]
#[path = "dispatch_mix_tests.rs"]
mod mix_tests;

#[cfg(test)]
#[path = "dispatch_placement_tests.rs"]
mod placement_tests;

#[cfg(test)]
#[path = "dispatch_fix_outcomes_tests.rs"]
mod fix_outcomes_tests;
