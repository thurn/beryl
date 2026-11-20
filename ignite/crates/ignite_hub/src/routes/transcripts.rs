//! The transcript digest routes of CLI mode (see "CLI mode" in `docs/ignite.md`):
//!
//! - `POST /transcripts/append` stores the digest lines a registered machine's daemon sends,
//!   keyed by session, turn and index, so sending a line twice stores it once;
//! - `GET /transcripts/<session>?after=<seq>&wait=<seconds>` answers the lines stored after
//!   `seq`. With `wait`, the request is held open until such a line exists or the wait ends, the
//!   way `GET /feed` waits (see `feed`): it sleeps on the same condition variable, which every
//!   append wakes, and takes one of the same [`feed::MAX_WAITERS`] places. The session page
//!   keeps one such request open, so a line shows as soon as the hub stores it.
//!
//! A transcript can hold secrets, so both routes answer only on the private listener. The public
//! read-only copy answers 404 for them, as it does for the dashboard's session page. Only the
//! tailnet reaches the private listener, so there a request may wait without a token, as the
//! feed's may.

use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::json;

use ignite_core::protocol::cli::{TranscriptAppend, TranscriptAppendReply, TranscriptPage};
use ignite_core::time::Timestamp;
use ignite_store::hub::transcripts;

use super::feed::{self, Waiter};
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
    let wait_s = match request.param("wait").map(str::parse::<u64>) {
        None => 0,
        Some(Ok(wait_s)) => wait_s.min(feed::MAX_WAIT_S),
        Some(Err(_)) => return failure(400, "wait must be a number of seconds"),
    };
    // Past the limit of waiting requests, the request is answered at once.
    let waiter = if wait_s > 0 { Waiter::take() } else { None };
    let wait = Duration::from_secs(if waiter.is_some() { wait_s } else { 0 });
    let deadline = Instant::now() + wait;
    loop {
        // Read the count of writes before the database, so a write between the two still wakes
        // the request.
        let seen = feed::writes();
        let lines = match transcripts::after(conn, session, after) {
            Ok(lines) => lines,
            Err(error) => return failure(500, &error.to_string()),
        };
        let left = deadline.saturating_duration_since(Instant::now());
        if !lines.is_empty() || left.is_zero() {
            return Response::json(200, &json!(TranscriptPage { session: id, lines }));
        }
        feed::wait_for_write(seen, left.min(feed::RECHECK));
    }
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}
