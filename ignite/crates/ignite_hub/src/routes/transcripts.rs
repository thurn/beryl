//! The transcript digest routes of CLI mode (see "CLI mode" in `docs/ignite.md`):
//!
//! - `POST /transcripts/append` stores the digest lines a registered machine's daemon sends,
//!   keyed by session, turn and index, so sending a line twice stores it once;
//! - `GET /transcripts/<session>?after=<seq>` answers the lines stored after `seq`.
//!
//! A transcript can hold secrets, so both routes answer only on the private listener. The public
//! read-only copy answers 404 for them, as it does for the dashboard's session page.

use rusqlite::Connection;
use serde_json::json;

use ignite_core::protocol::cli::{TranscriptAppend, TranscriptAppendReply, TranscriptPage};
use ignite_core::time::Timestamp;
use ignite_store::hub::transcripts;

use super::{Request, Response};

/// The response if this module owns the request's path. `private` says the request came on the
/// private listener.
pub fn handle(
    conn: &mut Connection,
    request: &Request,
    private: bool,
    now: Timestamp,
) -> Option<Response> {
    let rest = request.path.strip_prefix("/transcripts/")?;
    if !private {
        return Some(failure(404, "Unknown route"));
    }
    if rest == "append" {
        return Some(if request.method == "POST" {
            append(conn, request, now)
        } else {
            failure(405, "use POST for /transcripts/append")
        });
    }
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    if request.method != "GET" {
        return Some(failure(405, "use GET for /transcripts/<session>"));
    }
    Some(page(conn, request, rest))
}

fn append(conn: &Connection, request: &Request, now: Timestamp) -> Response {
    let Some(writer) = &request.writer else {
        return failure(401, "writes need a registered machine's token");
    };
    let body: TranscriptAppend = match serde_json::from_slice(&request.body) {
        Ok(body) => body,
        Err(error) => return failure(400, &format!("invalid transcript lines: {error}")),
    };
    if body.machine != writer.machine {
        return failure(403, "the token belongs to another machine");
    }
    match transcripts::append(conn, &body.machine, &body.lines, now) {
        Ok(stored) => Response::json(200, &json!(TranscriptAppendReply { stored })),
        Err(error) => failure(500, &error.to_string()),
    }
}

fn page(conn: &Connection, request: &Request, session: &str) -> Response {
    let Ok(id) = ignite_core::ids::SessionId::new(session) else {
        return failure(400, "invalid session");
    };
    let after = match request.param("after").map(str::parse::<i64>) {
        None => 0,
        Some(Ok(after)) => after,
        Some(Err(_)) => return failure(400, "after must be a number"),
    };
    match transcripts::after(conn, session, after) {
        Ok(lines) => Response::json(200, &json!(TranscriptPage { session: id, lines })),
        Err(error) => failure(500, &error.to_string()),
    }
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}
