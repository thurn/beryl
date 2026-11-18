//! The fleet and hand-off routes: `/machines/register`, `/machines/heartbeat`, `/next`, `/claims`,
//! `/landed` and `/release`.
//!
//! A ticket moves through them like this:
//!
//! 1. `POST /next`: a daemon with an idle worker says what its machine has free. The hub picks a
//!    ticket with `ignite_core::dispatch` and reserves it for that machine and worker in the same
//!    `IMMEDIATE` transaction, so two machines asking at once never get one ticket. The reply is
//!    the [`Reservation`], or `null`.
//! 2. `POST /claims`: the worker's `ignite start` turns the reservation into a claim held by its
//!    session and tree. A session may also claim a ticket nobody reserved for it, or file and
//!    claim an `adhoc` ticket from a summary. The reply is the ticket's brief with what each of
//!    its dependencies left behind.
//! 3. `POST /landed` records the commits, files and note, closes the ticket and frees the
//!    worker; the reply reserves the next ticket of a `lane:` chain for the same session.
//!    `POST /release` gives the ticket back to the queue with a comment.
//!
//! A reservation nobody claimed in 10 minutes lapses at the next `/next` or `/claims`. A ticket
//! held by another session answers 409, and so does `/landed` or `/release` from a session whose
//! claim was lost: the client prints that as exit code 3.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use ignite_core::dispatch::label;
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::hub::{
    AdhocClaim, Claim, DependencyNote, Heartbeat, HeartbeatReply, Landed, LandedReply, MachineFree,
    MachineRegister, MachineRegistered, Release, TicketBrief,
};
use ignite_core::protocol::tickets::{
    BatchRequest, Operation, OperationResult, Precondition, Status, Ticket, TicketFields,
};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{edges, feed, findings, fleet, tickets, tokens};

use super::{Request, Response};

#[path = "dispatch_queue.rs"]
mod queue;

/// The label on a ticket filed by `ignite start "<summary>"`.
const ADHOC: &str = "adhoc";

/// The `meta` key holding the Ignite commit the hub runs, which the server sets when it starts.
pub const HUB_VERSION: &str = "hub_version";

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    let writes = match request.path.as_str() {
        // A machine registering has no token yet; registration is how it gets one.
        "/machines/register" => false,
        "/machines/heartbeat" | "/next" | "/claims" | "/landed" | "/release" => true,
        _ => return None,
    };
    if request.method != "POST" {
        return Some(failure(405, &format!("use POST for {}", request.path)));
    }
    if writes && request.writer.is_none() {
        return Some(failure(401, "writes need a registered machine's token"));
    }
    Some(route(conn, request, now).unwrap_or_else(|response| response))
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

fn route(conn: &mut Connection, request: &Request, now: Timestamp) -> Answer {
    match request.path.as_str() {
        "/machines/register" => register(conn, &body(request)?, now),
        "/machines/heartbeat" => heartbeat(conn, &body(request)?, now),
        "/next" => next(conn, &body(request)?, now),
        "/claims" => {
            let value: Value = body(request)?;
            if value.get("summary").is_some() {
                adhoc(conn, &parse(value)?, now)
            } else {
                claim(conn, &parse(value)?, now)
            }
        }
        "/landed" => landed(conn, &body(request)?, now),
        "/release" => release(conn, &body(request)?, now),
        other => Err(failure(404, &format!("no route {other}"))),
    }
}

/// Adds or updates the machine and issues it a new write token.
fn register(conn: &mut Connection, machine: &MachineRegister, now: Timestamp) -> Answer {
    let tx = conn.transaction().map_err(sql_failure)?;
    fleet::register(&tx, machine, now).map_err(|e| store_failure(&e))?;
    let token = tokens::issue(&tx, &machine.machine).map_err(|e| store_failure(&e))?;
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(
        200,
        &json!(MachineRegistered {
            machine: machine.machine.clone(),
            token,
        }),
    ))
}

fn heartbeat(conn: &mut Connection, beat: &Heartbeat, now: Timestamp) -> Answer {
    fleet::heartbeat(conn, beat, now).map_err(|error| match error {
        StoreError::Invalid(message) => failure(404, &message),
        other => store_failure(&other),
    })?;
    let hub_version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [HUB_VERSION],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_failure)?;
    Ok(Response::json(
        200,
        &json!(HeartbeatReply {
            // A hub that never recorded its version asks no machine to update.
            hub_version: hub_version.unwrap_or_else(|| beat.version.clone()),
        }),
    ))
}

fn next(conn: &mut Connection, free: &MachineFree, now: Timestamp) -> Answer {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    fleet::lapse(&tx, now).map_err(|e| store_failure(&e))?;
    let reservation = queue::next(&tx, free, now).map_err(|e| store_failure(&e))?;
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(reservation)))
}

/// Claims `claim.ticket` for its session and tree: the reservation made for that session, or a
/// ticket nobody else holds or has reserved.
fn claim(conn: &mut Connection, claim: &Claim, now: Timestamp) -> Answer {
    let id = claim.ticket.as_str();
    {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_failure)?;
        fleet::lapse(&tx, now).map_err(|e| store_failure(&e))?;
        let ticket = load(&tx, id)?;
        if ticket.status == Status::Closed {
            return Err(conflict(&format!("{id} is closed"), Some(&ticket)));
        }
        let reserved_for: Option<String> = tx
            .query_row(
                "SELECT session FROM reservations WHERE ticket = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_failure)?;
        if reserved_for.is_some_and(|session| session != claim.session.as_str()) {
            return Err(conflict(
                &format!("{id} is reserved for another session"),
                Some(&ticket),
            ));
        }
        tx.commit().map_err(sql_failure)?;
    }
    let ops = vec![Operation::Claim { id: id.to_owned() }];
    apply(conn, Vec::new(), ops, &claim.session, &claim.machine, now)?;
    take(conn, &claim.ticket, claim, now)
}

/// Files a ticket from the summary, labelled `adhoc`, and claims it in the same batch.
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
    let results = apply(
        conn,
        Vec::new(),
        ops,
        &request.session,
        &request.machine,
        now,
    )?;
    let claim = Claim {
        ticket: results[0].id.clone(),
        machine: request.machine.clone(),
        session: request.session.clone(),
        tree: request.tree.clone(),
    };
    take(conn, &claim.ticket, &claim, now)
}

/// After the batch claimed the ticket: records the session and tree working it, drops its
/// reservation and answers with its brief.
fn take(conn: &mut Connection, id: &TicketId, claim: &Claim, now: Timestamp) -> Answer {
    let tx = conn.transaction().map_err(sql_failure)?;
    set_columns(
        &tx,
        id.as_str(),
        claim.session.as_str(),
        &[
            ("session", Value::from(claim.session.as_str())),
            ("tree", Value::from(claim.tree.as_str())),
        ],
        now,
    )
    .map_err(|e| store_failure(&e))?;
    fleet::release_reservation(&tx, id.as_str()).map_err(|e| store_failure(&e))?;
    let brief = brief(&tx, id.as_str())?;
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(brief)))
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
    Ok(TicketBrief { ticket, needs })
}

/// Records what landed, closes the ticket, frees the worker and reserves the next ticket of
/// its chain for the same session. A repeat of a landing that already happened answers as the
/// first did, without the reservation, so a daemon may resend it.
fn landed(conn: &mut Connection, landed: &Landed, now: Timestamp) -> Answer {
    let id = landed.ticket.as_str();
    {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_failure)?;
        let ticket = load(&tx, id)?;
        if ticket.status == Status::Closed && ticket.session.as_ref() == Some(&landed.session) {
            return Ok(Response::json(200, &json!(LandedReply { next: None })));
        }
        holds(&ticket, &landed.session)?;
        let work = serde_json::to_value(&landed.work).map_err(|e| failure(500, &e.to_string()))?;
        set_columns(
            &tx,
            id,
            landed.session.as_str(),
            &[("landed", work), ("landed_at", json!(now))],
            now,
        )
        .map_err(|e| store_failure(&e))?;
        tx.commit().map_err(sql_failure)?;
    }
    let reason = landed.work.note.lines().next().unwrap_or("").trim();
    let reason = if reason.is_empty() { "landed" } else { reason };
    let ops = vec![Operation::Close {
        id: id.to_owned(),
        reason: reason.to_owned(),
        include_ready: false,
    }];
    apply(
        conn,
        held_by(&landed.ticket, &landed.session),
        ops,
        &landed.session,
        &landed.machine,
        now,
    )?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    free_worker(
        &tx,
        &landed.machine,
        &landed.session,
        Some(&landed.work.files),
    )?;
    findings::close_ticket(&tx, id, now).map_err(|e| store_failure(&e))?;
    let ticket = load(&tx, id)?;
    let next = match label(&ticket.labels, "lane:") {
        Some(lane) => queue::chain_next(&tx, lane, &landed.machine, &landed.session, now)
            .map_err(|e| store_failure(&e))?,
        None => None,
    };
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(LandedReply { next })))
}

/// Gives a ticket back to the queue with the reason as a comment. A session may also give back a
/// reservation it has not claimed yet.
fn release(conn: &mut Connection, release: &Release, now: Timestamp) -> Answer {
    let id = release.ticket.as_str();
    let ticket = load(conn, id)?;
    if ticket.assignee.as_deref() != Some(release.session.as_str()) {
        let dropped = conn
            .execute(
                "DELETE FROM reservations WHERE ticket = ?1 AND session = ?2",
                params![id, release.session.as_str()],
            )
            .map_err(sql_failure)?;
        if dropped > 0 {
            return Ok(done());
        }
    }
    holds(&ticket, &release.session)?;
    let ops = vec![
        Operation::Update {
            id: id.to_owned(),
            set: Some(TicketFields {
                status: Some(Status::Open),
                assignee: Some(None),
                ..TicketFields::default()
            }),
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: std::collections::BTreeMap::new(),
            unset_meta: Vec::new(),
        },
        Operation::Comment {
            id: id.to_owned(),
            text: format!("Released: {}", release.reason),
        },
    ];
    apply(
        conn,
        held_by(&release.ticket, &release.session),
        ops,
        &release.session,
        &release.machine,
        now,
    )?;
    let tx = conn.transaction().map_err(sql_failure)?;
    let cleared = ["session", "tree", "machine", "claimed_at"].map(|column| (column, Value::Null));
    set_columns(&tx, id, release.session.as_str(), &cleared, now).map_err(|e| store_failure(&e))?;
    free_worker(&tx, &release.machine, &release.session, None)?;
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

fn held_by(ticket: &TicketId, session: &SessionId) -> Vec<Precondition> {
    vec![Precondition::Assignee {
        id: ticket.clone(),
        assignee: Some(session.as_str().to_owned()),
    }]
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

/// Applies a batch as `session` on `machine`, mapping a conflict to 409 and an unknown ticket to
/// 404.
fn apply(
    conn: &mut Connection,
    preconditions: Vec<Precondition>,
    ops: Vec<Operation>,
    session: &SessionId,
    machine: &MachineId,
    now: Timestamp,
) -> Result<Vec<OperationResult>, Response> {
    let request = BatchRequest { preconditions, ops };
    match batch::apply(conn, &request, session.as_str(), machine.as_str(), now) {
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
