//! Ticket owners (see `ignite_core::owner`):
//!
//! - `GET /owners` lists the owners a person may pick, every declared machine with every app the
//!   pool runs, and the default owner in force.
//! - `POST /owners/default` with `{"owner": "<machine>/<app>"}` makes that the default owner,
//!   which every ticket without an owner of its own or an ancestor's then follows. The dashboard's
//!   settings menu sends it as the Tailscale user (see `auth`); a machine may send it with its
//!   token.

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::json;

use ignite_core::owner::Owner;
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::owners;

use super::{Request, Response};

/// The path of the default owner's write, which the dashboard may send without a token.
pub const DEFAULT_PATH: &str = "/owners/default";

#[derive(Deserialize)]
struct DefaultOwnerBody {
    owner: Owner,
}

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/owners") => Some(list(conn).unwrap_or_else(|response| response)),
        ("POST", DEFAULT_PATH) => Some(set_default(conn, request, now).unwrap_or_else(|r| r)),
        (_, "/owners" | DEFAULT_PATH) => {
            Some(failure(405, "use GET /owners or POST /owners/default"))
        }
        _ => None,
    }
}

fn list(conn: &Connection) -> Result<Response, Response> {
    let routing = owners::routing(conn).map_err(|e| store_failure(&e))?;
    let default = owners::default_owner(conn, &routing).map_err(|e| store_failure(&e))?;
    let picked = owners::picked_default(conn).map_err(|e| store_failure(&e))?;
    Ok(Response::json(
        200,
        &json!({
            "choices": routing.choices(),
            "default": default.map(|owner| owner.resolved(&routing)),
            "picked": picked,
        }),
    ))
}

fn set_default(conn: &Connection, request: &Request, now: Timestamp) -> Result<Response, Response> {
    let Some(writer) = &request.writer else {
        return Err(failure(401, "changing the default owner needs a writer"));
    };
    let body: DefaultOwnerBody =
        serde_json::from_slice(&request.body).map_err(|e| failure(400, &e.to_string()))?;
    owners::set_default_owner(conn, &body.owner, &writer.actor, now).map_err(|e| match e {
        StoreError::Rejected(message) => failure(400, &message),
        other => store_failure(&other),
    })?;
    list(conn)
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

fn store_failure(error: &StoreError) -> Response {
    failure(500, &error.to_string())
}
