//! The fleet and hand-off routes: `/machines/register`, `/machines/heartbeat`, `/next`, `/claims`,
//! `/landed`, `/release`, `/close` and `/pause`.
//!
//! A ticket moves through them like this:
//!
//! 1. `POST /next`: a daemon with an idle worker says what its machine has free. The hub picks a
//!    ticket with `ignite_core::dispatch` and reserves it for that machine and worker in the same
//!    `IMMEDIATE` transaction, so two machines asking at once never get one ticket. The reply is
//!    the [`Reservation`] with its one-time code, or `null`. Only tickets under an active epic,
//!    or filed by the hub itself, are offered, and nothing while the machine is paused
//!    (`POST /pause`); `dispatch_queue.rs` says which.
//! 2. `POST /claims`: the worker's `ignite start` turns the reservation into a claim held by its
//!    session and tree. A reserved ticket is claimed only with its reservation's code, by any
//!    session of the machine it was reserved for, since a hand-off whose worker refuses moves to
//!    another worker there. A session may also claim by name a ticket nobody holds or has
//!    reserved, or file and claim an `adhoc` ticket from a summary. The reply is the ticket's
//!    brief with what each of its dependencies left behind and the branch an earlier release
//!    saved.
//! 3. `POST /landed` records the commits, files and note, closes the ticket and frees the
//!    worker; the reply reserves the next ticket of a `lane:` chain for the same session.
//!    `POST /release` gives the ticket back to the queue with the note as a comment, the saved
//!    branch and any `needs:` label. `POST /close` closes a ticket that needs no landing.
//!
//! A reservation nobody claimed in 10 minutes lapses at the next `/next` or `/claims`. A ticket
//! held by another session answers 409, and so does `/landed` or `/release` from a session whose
//! claim was lost: the client prints that as exit code 3.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use ignite_core::dispatch::label;
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::hub::{
    AdhocClaim, Claim, Close, DependencyNote, Heartbeat, HeartbeatReply, Landed, LandedReply,
    MachineFree, MachineRegister, MachineRegistered, Pause, Release, TicketBrief,
};
use ignite_core::protocol::tickets::{
    BatchRequest, Operation, OperationResult, Status, Ticket, TicketFields,
};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{edges, feed, findings, fleet, tickets, tokens};

use super::{Request, Response};

#[path = "dispatch_queue.rs"]
mod queue;

pub use queue::{named_files, resolve_named_file};

/// The label on a ticket filed by `ignite start "<summary>"`.
const ADHOC: &str = "adhoc";

/// The ticket metadata key holding the branch on the remote where a release saved the ticket's
/// work, until the ticket lands or closes.
pub const SAVED_BRANCH: &str = "saved_branch";

/// The `meta` key holding the Ignite commit the hub runs, which the server sets when it starts.
pub const HUB_VERSION: &str = "hub_version";

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    let writes = match request.path.as_str() {
        // A machine registering for the first time has no token yet; registration is how it
        // gets one. One that already has a token must present it (see `register`).
        "/machines/register" => false,
        "/machines/heartbeat"
        | "/next"
        | "/claims"
        | "/landed"
        | "/release"
        | "/close"
        | "/pause" => true,
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

fn route(conn: &mut Connection, request: &Request, now: Timestamp) -> Answer {
    match request.path.as_str() {
        "/machines/register" => register(conn, request, &body(request)?, now),
        "/machines/heartbeat" => {
            let parsed: Heartbeat = body(request)?;
            same_machine(request, &parsed.machine)?;
            heartbeat(conn, &parsed, now)
        }
        "/next" => {
            let parsed: MachineFree = body(request)?;
            same_machine(request, &parsed.machine)?;
            next(conn, &parsed, now)
        }
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
            landed(conn, &parsed, now)
        }
        "/release" => {
            let mut parsed: Release = body(request)?;
            same_machine(request, &parsed.machine)?;
            parsed.ticket = canonical(conn, &parsed.ticket)?;
            release(conn, &parsed, now)
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
            pause(conn, &parsed, now)
        }
        other => Err(failure(404, &format!("no route {other}"))),
    }
}

/// Adds or updates the machine and answers with its write token. A new machine ID needs no token
/// and is issued one. A machine ID that already has a token registers again only with that token
/// as the bearer, so that nobody who can reach the hub can replace another machine's token, and
/// it keeps that token: `ignite install` registers the machine again while the daemon and its
/// verb processes still send the token they read when they started, and a new token would make
/// the hub refuse each of their writes with 401 until they restarted.
fn register(
    conn: &mut Connection,
    request: &Request,
    machine: &MachineRegister,
    now: Timestamp,
) -> Answer {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    let mut kept = None;
    if tokens::has_token(&tx, &machine.machine).map_err(|e| store_failure(&e))? {
        let bearer = request
            .headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .and_then(|(_, value)| value.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, token)| token.trim());
        // `writer` is set when the request carried a valid token and an `X-Actor`; a client
        // that sends only the bearer (`ignite install`) is checked here.
        let owner = match (&request.writer, bearer) {
            (Some(writer), _) => Some(writer.machine.clone()),
            (None, Some(token)) => tokens::verify(&tx, token).map_err(|e| store_failure(&e))?,
            (None, None) => None,
        };
        if owner.as_ref() != Some(&machine.machine) {
            return Err(failure(
                401,
                &format!(
                    "{} is already registered; send its current token as the bearer",
                    machine.machine
                ),
            ));
        }
        // Keeps the bearer only once it is known to be the machine's current token.
        if let Some(token) = bearer
            && tokens::verify(&tx, token)
                .map_err(|e| store_failure(&e))?
                .as_ref()
                == Some(&machine.machine)
        {
            kept = Some(token.to_owned());
        }
    }
    fleet::register(&tx, machine, now).map_err(|e| store_failure(&e))?;
    let token = match kept {
        Some(token) => token,
        None => tokens::issue(&tx, &machine.machine).map_err(|e| store_failure(&e))?,
    };
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

/// Pauses or resumes hand-offs to one machine or to every machine.
fn pause(conn: &mut Connection, pause: &Pause, now: Timestamp) -> Answer {
    let machine = pause.machine.as_ref().map(MachineId::as_str);
    fleet::set_paused(conn, machine, pause.paused, now).map_err(|e| store_failure(&e))?;
    Ok(done())
}

fn next(conn: &mut Connection, free: &MachineFree, now: Timestamp) -> Answer {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    fleet::lapse(&tx, now).map_err(|e| store_failure(&e))?;
    let mut free = free.clone();
    if free.name.is_none() {
        let name = crate::upkeep::machines::name(&tx, free.machine.as_str())
            .map_err(|e| store_failure(&e))?;
        free.name = Some(name);
    }
    let reservation = queue::next(&tx, &free, now).map_err(|e| store_failure(&e))?;
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(reservation)))
}

/// Claims `claim.ticket` for its session and tree: a reserved ticket with its reservation's code
/// from the machine it was reserved for, or, with no code, a ticket nobody else holds or has
/// reserved. A wrong code, or a code whose reservation lapsed, is refused. A session that
/// already holds the ticket in progress gets its brief again, so a claim whose reply was lost
/// can be retried. The checks, the claim
/// and the record of session and tree are one `IMMEDIATE` transaction, so no other session can
/// take or drop the reservation in between.
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
    let reservation = fleet::reservation_of(&tx, id).map_err(|e| store_failure(&e))?;
    let refused = match (&reservation, &claim.code) {
        (Some((machine, code)), Some(sent)) => (machine != claim.machine.as_str() || code != sent)
            .then_some("is reserved under another code"),
        (Some(_), None) => Some("is reserved; claim it with its reservation's code"),
        (None, Some(_)) => Some("has no live reservation with that code"),
        (None, None) => None,
    };
    if let Some(refused) = refused {
        return Err(conflict(&format!("{id} {refused}"), Some(&ticket)));
    }
    let ops = vec![Operation::Claim { id: id.to_owned() }];
    apply_in(&mut tx, ops, &claim.session, &claim.machine, now)?;
    let brief = take(&tx, &claim.ticket, claim, now)?;
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
    let brief = take(&tx, &claim.ticket, &claim, now)?;
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(200, &json!(brief)))
}

/// After the batch claimed the ticket: records the session and tree working it, drops its
/// reservation and returns its brief. Runs in the claim's transaction.
fn take(
    tx: &Connection,
    id: &TicketId,
    claim: &Claim,
    now: Timestamp,
) -> Result<TicketBrief, Response> {
    set_columns(
        tx,
        id.as_str(),
        claim.session.as_str(),
        &[
            ("session", Value::from(claim.session.as_str())),
            ("tree", Value::from(claim.tree.as_str())),
        ],
        now,
    )
    .map_err(|e| store_failure(&e))?;
    fleet::release_reservation(tx, id.as_str()).map_err(|e| store_failure(&e))?;
    brief(tx, id.as_str())
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
    Ok(TicketBrief {
        ticket,
        needs,
        saved_branch,
    })
}

/// Records what landed, closes the ticket, frees the worker and reserves the next ticket of
/// its chain for the same session, all in one `IMMEDIATE` transaction, so an interruption
/// leaves nothing half done. A repeat of a landing that already happened answers as the first
/// did, without the reservation, so a daemon may resend it; it also frees the worker and marks
/// the ticket's finding closed again, which does nothing when they are already done.
fn landed(conn: &mut Connection, landed: &Landed, now: Timestamp) -> Answer {
    let id = landed.ticket.as_str();
    let mut tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    let ticket = load(&tx, id)?;
    if ticket.status == Status::Closed && ticket.session.as_ref() == Some(&landed.session) {
        free_worker(
            &tx,
            &landed.machine,
            &landed.session,
            Some(&landed.work.files),
        )?;
        findings::close_ticket(&tx, id, now).map_err(|e| store_failure(&e))?;
        tx.commit().map_err(sql_failure)?;
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
    let reason = landed.work.note.lines().next().unwrap_or("").trim();
    let reason = if reason.is_empty() { "landed" } else { reason };
    let ops = closing(&load(&tx, id)?, reason);
    apply_in(&mut tx, ops, &landed.session, &landed.machine, now)?;
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
        apply_in(
            &mut tx,
            closing(&ticket, reason),
            &close.session,
            &close.machine,
            now,
        )?;
    }
    free_worker(&tx, &close.machine, &close.session, None)?;
    findings::close_ticket(&tx, id, now).map_err(|e| store_failure(&e))?;
    tx.commit().map_err(sql_failure)?;
    Ok(done())
}

/// The operations that close `ticket` with `reason` and forget its saved branch, which the
/// closing session deletes from the remote.
fn closing(ticket: &Ticket, reason: &str) -> Vec<Operation> {
    let mut ops = Vec::new();
    if ticket.metadata.contains_key(SAVED_BRANCH) {
        ops.push(Operation::Update {
            id: ticket.id.as_str().to_owned(),
            set: None,
            add_labels: Vec::new(),
            remove_labels: Vec::new(),
            set_meta: BTreeMap::new(),
            unset_meta: vec![SAVED_BRANCH.to_owned()],
        });
    }
    ops.push(Operation::Close {
        id: ticket.id.as_str().to_owned(),
        reason: reason.to_owned(),
        include_ready: false,
    });
    ops
}

/// Gives a ticket back to the queue with the note as a comment, records the branch its work was
/// saved to and, with `needs`, labels it `needs:<needs>` in place of any earlier `needs:` label.
/// A session may also give back a reservation it has not claimed yet.
fn release(conn: &mut Connection, release: &Release, now: Timestamp) -> Answer {
    let id = release.ticket.as_str();
    let mut tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    let ticket = load(&tx, id)?;
    if ticket.assignee.as_deref() != Some(release.session.as_str()) {
        let dropped = tx
            .execute(
                "DELETE FROM reservations WHERE ticket = ?1 AND session = ?2",
                params![id, release.session.as_str()],
            )
            .map_err(sql_failure)?;
        if dropped > 0 {
            tx.commit().map_err(sql_failure)?;
            return Ok(done());
        }
    }
    holds(&ticket, &release.session)?;
    let (add_labels, remove_labels) = match &release.needs {
        Some(needs) => {
            let label = format!("needs:{needs}");
            let earlier = ticket
                .labels
                .iter()
                .filter(|l| l.starts_with("needs:") && **l != label)
                .cloned()
                .collect();
            (vec![label], earlier)
        }
        None => (Vec::new(), Vec::new()),
    };
    let set_meta = release
        .branch
        .iter()
        .map(|branch| (SAVED_BRANCH.to_owned(), Value::from(branch.as_str())))
        .collect();
    let ops = vec![
        Operation::Update {
            id: id.to_owned(),
            set: Some(TicketFields {
                status: Some(Status::Open),
                assignee: Some(None),
                ..TicketFields::default()
            }),
            add_labels,
            remove_labels,
            set_meta,
            unset_meta: Vec::new(),
        },
        Operation::Comment {
            id: id.to_owned(),
            text: format!("Released: {}", release.reason),
        },
    ];
    apply_in(&mut tx, ops, &release.session, &release.machine, now)?;
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
    match batch::apply_in(tx, &request, session.as_str(), machine.as_str(), now) {
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
#[path = "dispatch_hardening_tests.rs"]
mod hardening_tests;
