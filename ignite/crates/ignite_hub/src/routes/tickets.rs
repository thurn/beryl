//! The ticket routes: `GET /tickets`, `GET /tickets/<id>`, `GET /graph`, `POST /batch`,
//! `POST /leases/<name>`, `GET /leases`, `GET /export` and `POST /import`.
//!
//! They keep the ticket service's request and response shapes and status codes: 400 for a
//! malformed request, 401 for a write without a registered token, 404 for an unknown ticket, 409
//! for a failed precondition or a lease someone else holds, and 304 for an unchanged graph. Each
//! decodes its request, calls `ignite_store::hub` and encodes the reply.

mod transfer;

use std::fmt::Write as _;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Value, json};

use ignite_core::ids::TicketId;
use ignite_core::protocol::tickets::{
    BatchRequest, LeaseOp, LeaseReply, Operation, Status, TicketQuery,
};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{
    attachments, feed, filer_deps, fleet, leases, owners, releases, session_titles, tickets,
};

use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    let path = request.path.as_str();
    let (method, writes) = match path {
        "/tickets" | "/graph" | "/leases" | "/export" => ("GET", false),
        "/batch" | "/import" => ("POST", true),
        _ if path.strip_prefix("/tickets/").is_some_and(is_segment) => ("GET", false),
        _ if path.strip_prefix("/leases/").is_some_and(is_segment) => ("POST", true),
        _ => return None,
    };
    if request.method != method {
        return Some(failure(405, &format!("use {method} for {path}")));
    }
    if writes && request.writer.is_none() {
        return Some(failure(401, "Missing or unknown token"));
    }
    Some(route(conn, request, now).unwrap_or_else(|response| response))
}

/// One non-empty path segment.
fn is_segment(rest: &str) -> bool {
    !rest.is_empty() && !rest.contains('/')
}

fn route(conn: &mut Connection, request: &Request, now: Timestamp) -> Result<Response, Response> {
    let path = request.path.as_str();
    match path {
        "/tickets" => {
            let query = ticket_query(conn, request)?;
            ok(&tickets::list(conn, &query, now).map_err(|e| store_failure(&e))?)
        }
        "/graph" => graph(conn, request, now),
        "/leases" => ok(&leases::list(conn, now).map_err(|e| store_failure(&e))?),
        "/export" => {
            let ids: Vec<&str> = request
                .query
                .iter()
                .filter(|(key, _)| key == "id")
                .map(|(_, value)| value.as_str())
                .collect();
            transfer::export(conn, request.param("after"), &ids)
        }
        "/batch" => apply_batch(conn, request, now),
        "/import" => transfer::import(conn, &body(request)?),
        _ => {
            if let Some(id) = path.strip_prefix("/tickets/") {
                let comments = flag(request, "comments")?;
                let history = flag(request, "history")?;
                match tickets::get(conn, id, comments, history, now) {
                    Ok(Some(detail)) => with_releases(conn, &detail),
                    Ok(None) => Err(failure(404, &format!("Ticket {id} not found"))),
                    Err(error) => Err(store_failure(&error)),
                }
            } else {
                let name = path.strip_prefix("/leases/").unwrap_or_default();
                lease(conn, request, name, now)
            }
        }
    }
}

fn apply_batch(
    conn: &mut Connection,
    request: &Request,
    now: Timestamp,
) -> Result<Response, Response> {
    let mut batch: BatchRequest = body(request)?;
    let writer = request.writer.as_ref().expect("checked in handle");
    let raw = serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
    if raw.get(AUTOMATIC).and_then(Value::as_bool) == Some(true) {
        let machine =
            fleet::name_of(conn, writer.machine.as_str()).map_err(|e| store_failure(&e))?;
        owners::add_filing_machine(&mut batch, machine.as_deref());
    }
    // A follow-up filed by a session that holds unlanded work in the same line depends on that
    // work, unless the filer opts out or a program filed it; see `filer_deps`.
    let independent = raw.get(INDEPENDENT).and_then(Value::as_bool) == Some(true);
    let automatic = raw.get(AUTOMATIC).and_then(Value::as_bool) == Some(true);
    let added = if independent || automatic {
        Vec::new()
    } else {
        filer_deps::add_filer_dependencies(conn, &mut batch, &writer.actor)
            .map_err(|e| store_failure(&e))?
    };
    tickets::add_holds(conn, &mut batch).map_err(|e| store_failure(&e))?;
    tickets::add_releases(conn, &mut batch).map_err(|e| store_failure(&e))?;
    // The CLI sends the writing session's title beside the operations, so the dashboard can
    // name the session by its title; see `session_titles`.
    if let Some(title) = raw.get(ACTOR_TITLE).and_then(Value::as_str) {
        session_titles::record(conn, &writer.actor, title, now).map_err(|e| store_failure(&e))?;
    }
    match batch::apply(conn, &batch, &writer.actor, writer.machine.as_str(), now) {
        Ok(BatchOutcome::Applied(reply)) => {
            forget_releases(conn, &batch).map_err(|e| store_failure(&e))?;
            let mut value = json!(reply);
            if !added.is_empty() {
                value[FILER_DEPENDENCIES] = json!(added);
            }
            Ok(Response::json(200, &value))
        }
        Ok(BatchOutcome::Conflict(conflict)) => Ok(Response::json(409, &json!(conflict))),
        Err(error) => Err(store_failure(&error)),
    }
}

/// Forgets the counted releases of each ticket whose body or acceptance the batch changed, or
/// which it set `open` as `ignite ticket update <id> --release` does, so that a ticket held
/// back after repeated releases is offered again (see `releases`).
fn forget_releases(conn: &Connection, batch: &BatchRequest) -> Result<(), StoreError> {
    for op in &batch.ops {
        let Operation::Update {
            id, set: Some(set), ..
        } = op
        else {
            continue;
        };
        let changed = set.body.is_some() || set.acceptance.is_some();
        if !(changed || set.status == Some(Status::Open)) {
            continue;
        }
        // A ticket the batch created (`$0`) has no releases to forget.
        if let Some(id) = tickets::resolve(conn, id)? {
            releases::reset(conn, id.as_str())?;
        }
    }
    Ok(())
}

/// `GET /tickets/<id>`'s reply: the ticket, with its counted releases under `releases` when it
/// has any, why it is held back under `held_back` when it is, and its image attachments under
/// `attachments` when it has any. `ignite ticket show` prints all three.
fn with_releases(
    conn: &Connection,
    detail: &ignite_core::protocol::tickets::TicketDetail,
) -> Result<Response, Response> {
    let counted = releases::of(conn, detail.ticket.id.as_str()).map_err(|e| store_failure(&e))?;
    let mut value = json!(detail);
    if !counted.releases.is_empty() {
        value["releases"] = json!({
            "count": counted.releases.len(),
            "sessions": counted.sessions(),
        });
    }
    if let Some(reason) = counted.reason() {
        value["held_back"] = json!(reason);
    }
    let attached =
        attachments::list(conn, detail.ticket.id.as_str()).map_err(|e| store_failure(&e))?;
    if !attached.is_empty() {
        value["attachments"] = json!(attached);
    }
    Ok(Response::json(200, &value))
}

/// The key of `POST /batch`'s body that is `true` when the filer says its tickets do not need
/// its own unlanded work (`ignite ticket create --independent`), so the hub adds no dependency.
pub const INDEPENDENT: &str = "independent";

/// The key of `POST /batch`'s reply listing the dependencies the hub added on the filer's
/// unlanded work, each as `{"op": <index of the create>, "on": <ticket>}`. Left out when none.
pub const FILER_DEPENDENCIES: &str = "filer_dependencies";

/// The key of `POST /batch`'s body that is `true` when a program filed the batch's tickets, such
/// as a check (`ignite ticket create --automatic`). Each ticket it creates without an owner is
/// then owned by the writer's machine, as a ticket the hub files from a machine's report is; a
/// ticket a person or a session files follows its line's owner or the default owner.
pub const AUTOMATIC: &str = "automatic";

/// The key of `POST /batch`'s body holding the title of the session writing, if it has one.
pub const ACTOR_TITLE: &str = "actor_title";

fn lease(
    conn: &mut Connection,
    request: &Request,
    name: &str,
    now: Timestamp,
) -> Result<Response, Response> {
    let op: LeaseOp = body(request)?;
    let writer = request.writer.as_ref().expect("checked in handle");
    match leases::apply(conn, name, &op, writer.machine.as_str(), now) {
        Ok(LeaseReply::Held(lease)) => ok(&lease),
        Ok(LeaseReply::Busy(busy)) => Ok(Response::json(409, &json!(busy))),
        Err(error) => Err(store_failure(&error)),
    }
}

/// `GET /graph`, with an `ETag` that changes whenever the reply could: on every write (the feed
/// position) and when the earliest future `defer_until` of a live ticket passes, which makes
/// that ticket ready without any write. The long text fields are left out of each ticket, as the
/// ticket service did.
fn graph(conn: &Connection, request: &Request, now: Timestamp) -> Result<Response, Response> {
    let all = flag(request, "all")?;
    let seq = feed::current(conn).map_err(|e| store_failure(&e))?;
    let mut key = format!("{}-{}", if all { "all" } else { "live" }, seq.0);
    if let Some(defer) = next_defer(conn, now).map_err(|e| store_failure(&e))? {
        let _ = write!(key, "-{}", defer.millis());
    }
    let tag = format!("\"{key}\"");
    if request
        .header("if-none-match")
        .is_some_and(|header| matches_tag(header, &key))
    {
        return Ok(Response {
            status: 304,
            content_type: "application/json",
            body: Vec::new(),
            headers: vec![("ETag", tag)],
        });
    }
    let graph = tickets::graph(conn, all, now).map_err(|e| store_failure(&e))?;
    let mut value = json!(graph);
    if let Some(list) = value["tickets"].as_array_mut() {
        for ticket in list.iter_mut().filter_map(Value::as_object_mut) {
            for field in ["body", "design", "notes"] {
                ticket.remove(field);
            }
        }
    }
    Ok(Response::json(200, &value).with_header("ETag", tag))
}

/// Whether an `If-None-Match` header names the `ETag` `key`, weak or strong, or is `*`.
fn matches_tag(header: &str, key: &str) -> bool {
    header.split(',').any(|tag| {
        let tag = tag.trim();
        let tag = tag.strip_prefix("W/").unwrap_or(tag);
        tag == "*"
            || tag == key
            || tag.strip_prefix('"').and_then(|t| t.strip_suffix('"')) == Some(key)
    })
}

/// The earliest `defer_until` of a live ticket that is still in the future.
fn next_defer(conn: &Connection, now: Timestamp) -> Result<Option<Timestamp>, StoreError> {
    let mut statement = conn.prepare(
        "SELECT defer_until FROM tickets WHERE status != 'closed' AND defer_until IS NOT NULL",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut earliest: Option<Timestamp> = None;
    for text in rows {
        let Ok(at) = Timestamp::parse(&text?) else {
            continue;
        };
        if at > now && earliest.is_none_or(|earliest| at < earliest) {
            earliest = Some(at);
        }
    }
    Ok(earliest)
}

/// The query of `GET /tickets`. A parent or ancestor may be named by any alias. Parameters the
/// protocol does not know are ignored.
fn ticket_query(conn: &Connection, request: &Request) -> Result<TicketQuery, Response> {
    let mut query = TicketQuery {
        status: request
            .param("status")
            .map(|v| choice(v, "status"))
            .transpose()?,
        ticket_type: request
            .param("type")
            .map(|v| choice(v, "type"))
            .transpose()?,
        parent: request
            .param("parent")
            .map(|v| ticket(conn, v))
            .transpose()?,
        ancestor: request
            .param("ancestor")
            .map(|v| ticket(conn, v))
            .transpose()?,
        search: request.param("search").map(str::to_owned),
        assignee: request.param("assignee").map(str::to_owned),
        ready: flag(request, "ready")?,
        all: flag(request, "all")?,
        ..TicketQuery::default()
    };
    for (key, value) in &request.query {
        match key.as_str() {
            "label" => query.label.push(value.clone()),
            "label_any" => query.label_any.push(value.clone()),
            _ => {}
        }
    }
    if let Some(limit) = request.param("limit") {
        query.limit = Some(
            limit
                .parse()
                .ok()
                .filter(|_| limit.bytes().all(|b| b.is_ascii_digit()))
                .ok_or_else(|| failure(400, "limit must be a nonnegative integer"))?,
        );
    }
    Ok(query)
}

/// A value of one of the protocol's enums, such as a status, from its JSON spelling.
fn choice<T: serde::de::DeserializeOwned>(value: &str, field: &str) -> Result<T, Response> {
    serde_json::from_value(Value::String(value.to_owned()))
        .map_err(|_| failure(400, &format!("{field} {value:?} is not known")))
}

/// A ticket named by ID or alias. A name that is no ticket still filters (to nothing) if it is
/// a well-formed ID.
fn ticket(conn: &Connection, name: &str) -> Result<TicketId, Response> {
    match tickets::resolve(conn, name).map_err(|e| store_failure(&e))? {
        Some(id) => Ok(id),
        None => name
            .parse()
            .map_err(|_| failure(400, &format!("{name:?} is not a ticket ID"))),
    }
}

/// A `0` or `1` query parameter; absent is `0`.
fn flag(request: &Request, name: &str) -> Result<bool, Response> {
    match request.param(name) {
        None | Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(_) => Err(failure(400, &format!("{name} must be 0 or 1"))),
    }
}

fn body<T: serde::de::DeserializeOwned>(request: &Request) -> Result<T, Response> {
    serde_json::from_slice(&request.body)
        .map_err(|error| failure(400, &format!("Malformed request: {error}")))
}

#[allow(clippy::unnecessary_wraps)] // Every route arm returns a `Result`.
fn ok<T: Serialize>(value: &T) -> Result<Response, Response> {
    Ok(Response::json(200, &json!(value)))
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

/// A store error as its status: 404 for an unknown ticket, 400 for a refused request and 500
/// for anything else.
fn store_failure(error: &StoreError) -> Response {
    let status = match error {
        StoreError::NotFound(_) => 404,
        StoreError::Rejected(_) => 400,
        StoreError::Sqlite(_) | StoreError::Invalid(_) => 500,
    };
    failure(status, &error.to_string())
}
