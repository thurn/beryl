//! Switching a check on or off by hand: `POST /checks/state` (`ignite flaky on|off`), which needs
//! an authorized writer, and the read routes for `ignite flaky list` and
//! `ignite flaky evidence`.
//!
//! A switch records who made it, and comments on the check's flaky ticket if it has one, in the
//! same transaction as the change.

use rusqlite::{Connection, TransactionBehavior};
use serde_json::json;

use ignite_core::protocol::hub::{
    CheckState, CheckStateChange, CheckStateChanged, FlakyChecksReply,
};
use ignite_core::protocol::tickets::{BatchRequest, Operation};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::batch::{self, BatchOutcome};
use ignite_store::hub::{check_state, checks, tickets};

use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    let method = match request.path.as_str() {
        "/checks/flaky" | "/checks/flaky/evidence" => "GET",
        "/checks/state" => "POST",
        _ => return None,
    };
    if request.method != method {
        return Some(failure(405, &format!("use {method} for {}", request.path)));
    }
    let reply = match request.path.as_str() {
        "/checks/flaky" => list(conn),
        "/checks/flaky/evidence" => evidence(conn, request),
        _ => switch(conn, request, now),
    };
    Some(reply.unwrap_or_else(|response| response))
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

fn store_failure(error: &StoreError) -> Response {
    failure(500, &error.to_string())
}

fn list(conn: &Connection) -> Result<Response, Response> {
    let checks = check_state::flaky_checks(conn).map_err(|e| store_failure(&e))?;
    Ok(Response::json(200, &json!(FlakyChecksReply { checks })))
}

fn evidence(conn: &Connection, request: &Request) -> Result<Response, Response> {
    let ticket = request
        .param("ticket")
        .ok_or_else(|| failure(400, "name a ticket"))?;
    let ticket = tickets::resolve(conn, ticket)
        .map_err(|e| store_failure(&e))?
        .ok_or_else(|| failure(404, &format!("no ticket named {ticket}")))?;
    let reports = checks::filed_flakes(conn, ticket.as_str()).map_err(|e| store_failure(&e))?;
    Ok(Response::json(200, &json!(reports)))
}

fn word(state: CheckState) -> &'static str {
    match state {
        CheckState::On => "on",
        CheckState::Off => "off",
        CheckState::Uncached => "uncached",
    }
}

fn switch(conn: &mut Connection, request: &Request, now: Timestamp) -> Result<Response, Response> {
    let Some(writer) = request.writer.as_ref() else {
        return Err(failure(401, "writes need a registered machine's token"));
    };
    let change: CheckStateChange = serde_json::from_slice(&request.body)
        .map_err(|error| failure(400, &format!("bad request body: {error}")))?;
    if change.state == CheckState::Uncached {
        return Err(failure(
            400,
            "a check can be switched on or off; only an audit makes it uncached",
        ));
    }
    let check = change.check.as_str();
    let mut tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| store_failure(&e.into()))?;
    let ticket = check_state::flaky_ticket(&tx, check).map_err(|e| store_failure(&e))?;
    let switch = check_state::Switch {
        check,
        state: change.state,
        actor: &writer.actor,
        machine: writer.machine.as_str(),
        note: &change.note,
        ticket: ticket.as_deref(),
        at: now,
    };
    let Some(previous) = check_state::switch(&tx, &switch).map_err(|e| store_failure(&e))? else {
        return Err(failure(404, &format!("no check named {check}")));
    };
    let recorded = if previous == change.state {
        None
    } else if let Some(ticket) = &ticket {
        comment(&mut tx, ticket, &switch, previous)?;
        Some(ticket.clone())
    } else {
        None
    };
    tx.commit().map_err(|e| store_failure(&e.into()))?;
    if change.state == CheckState::Off && previous != change.state {
        crate::upkeep::regressions::settle_after_change(conn, now);
    }
    Ok(Response::json(
        200,
        &json!(CheckStateChanged {
            check: change.check,
            previous,
            state: change.state,
            ticket: recorded,
        }),
    ))
}

/// Says on the check's flaky ticket who switched it, from where, and why.
fn comment(
    tx: &mut rusqlite::Transaction,
    ticket: &str,
    switch: &check_state::Switch,
    previous: CheckState,
) -> Result<(), Response> {
    let mut text = format!(
        "{} on machine {} switched {} {} by hand (it was {}).",
        switch.actor,
        switch.machine,
        switch.check,
        word(switch.state),
        word(previous)
    );
    if !switch.note.is_empty() {
        text.push_str("\n\n");
        text.push_str(switch.note);
    }
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Comment {
            id: ticket.to_owned(),
            text,
        }],
    };
    match batch::apply_in(tx, &request, switch.actor, switch.machine, switch.at) {
        Ok(BatchOutcome::Applied(_)) => Ok(()),
        Ok(BatchOutcome::Conflict(_)) => Err(failure(
            409,
            &format!("the comment on {ticket} conflicted; try again"),
        )),
        Err(error) => Err(store_failure(&error)),
    }
}

#[cfg(test)]
#[path = "check_state_tests.rs"]
mod tests;
