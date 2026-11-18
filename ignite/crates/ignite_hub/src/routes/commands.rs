//! `POST /commands/rejected`: a command line the `ignite` executable refused to parse. The
//! `rejected-command` improvement rule reads what this stores.

use rusqlite::Connection;
use serde_json::json;

use ignite_core::protocol::hub::RejectedCommand;
use ignite_core::time::Timestamp;
use ignite_store::hub::rejected;

use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    if request.path != "/commands/rejected" {
        return None;
    }
    if request.method != "POST" {
        return Some(failure(405, "use POST for /commands/rejected"));
    }
    let Some(writer) = &request.writer else {
        return Some(failure(401, "writes need a registered machine's token"));
    };
    let command: RejectedCommand = match serde_json::from_slice(&request.body) {
        Ok(command) => command,
        Err(error) => return Some(failure(400, &format!("bad request body: {error}"))),
    };
    Some(
        match rejected::insert(conn, writer.machine.as_str(), &command, now) {
            Ok(()) => Response::json(200, &json!({"stored": true})),
            Err(error) => failure(500, &error.to_string()),
        },
    )
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}
