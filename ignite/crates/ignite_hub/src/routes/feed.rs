//! `GET /feed?since=<seq>&wait=<seconds>`: returns as soon as anything after `since` exists, or
//! empty when the wait ends.
//!
//! This long poll replaces the ticket service's WebSocket feed. A waiting request sleeps on a
//! process-wide condition variable that [`notify`] wakes after every write the hub serves, and
//! looks at the database again at least once a second, so that a write made outside a request
//! (by upkeep, say) is seen too.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::json;

use ignite_core::ids::Seq;
use ignite_core::protocol::tickets::{FeedBatch, FeedChange};
use ignite_core::time::Timestamp;
use ignite_store::hub::feed;

use super::{Request, Response};

/// The longest wait a client may ask for, in seconds. A longer one is shortened to this.
pub const MAX_WAIT_S: u64 = 120;

/// The most requests that may wait at once. Each holds a thread and a database connection, so a
/// request beyond this, and a request on the public listener without a valid token, is answered
/// at once without waiting.
pub const MAX_WAITERS: usize = 16;

static WAITERS: AtomicUsize = AtomicUsize::new(0);

/// A place among the waiting requests, given back when dropped.
struct Waiter;

impl Waiter {
    fn take() -> Option<Waiter> {
        if WAITERS.fetch_add(1, Ordering::SeqCst) >= MAX_WAITERS {
            WAITERS.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Waiter)
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        WAITERS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The most changes one reply carries (one position's changes always come whole).
const LIMIT: u32 = 1000;

/// How long a waiting request sleeps before it looks at the database again.
const RECHECK: Duration = Duration::from_secs(1);

/// Counts writes, so a waiting request knows whether one happened since it last looked.
static WRITES: Mutex<u64> = Mutex::new(0);
static WRITTEN: Condvar = Condvar::new();

/// Wakes every waiting feed request. The server calls it after each request that may have
/// written.
pub fn notify() {
    let mut writes = WRITES.lock().unwrap_or_else(PoisonError::into_inner);
    *writes += 1;
    WRITTEN.notify_all();
}

fn writes() -> u64 {
    *WRITES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sleeps until a write after `seen`, or for `timeout`.
fn wait_for_write(seen: u64, timeout: Duration) {
    let writes = WRITES.lock().unwrap_or_else(PoisonError::into_inner);
    let _unused = WRITTEN
        .wait_timeout_while(writes, timeout, |writes| *writes == seen)
        .unwrap_or_else(PoisonError::into_inner);
}

/// The response if this module owns the request's path. `private` says the request came on the
/// private listener, which only the tailnet reaches. There a request may wait without a token,
/// because the dashboard's pages follow the feed and carry none; answered at once, their script
/// would ask again at once, hundreds of times a second.
pub fn handle(
    conn: &mut Connection,
    request: &Request,
    private: bool,
    _now: Timestamp,
) -> Option<Response> {
    if request.path != "/feed" {
        return None;
    }
    if request.method != "GET" {
        return Some(failure(405, "use GET for /feed"));
    }
    let since = match request.param("since").map(str::parse::<u64>) {
        Some(Ok(since)) => Seq(since),
        _ => return Some(failure(400, "since must be a feed position")),
    };
    let wait_s = match request.param("wait").map(str::parse::<u64>) {
        None => 0,
        Some(Ok(wait_s)) => wait_s.min(MAX_WAIT_S),
        Some(Err(_)) => return Some(failure(400, "wait must be a number of seconds")),
    };
    let waiter = if wait_s > 0 && (private || request.writer.is_some()) {
        Waiter::take()
    } else {
        None
    };
    let wait_s = if waiter.is_some() { wait_s } else { 0 };
    Some(match poll(conn, since, Duration::from_secs(wait_s)) {
        Ok(batch) => Response::json(200, &json!(batch)),
        Err(error) => failure(500, &error.to_string()),
    })
}

/// The changes after `since`, waiting up to `wait` for the first one.
fn poll(conn: &Connection, since: Seq, wait: Duration) -> ignite_store::Result<FeedBatch> {
    let deadline = Instant::now() + wait;
    loop {
        // Read the count before the database, so a write between the two still wakes us.
        let seen = writes();
        let batch = feed::since(conn, since, LIMIT)?;
        if since > batch.seq {
            // The client counts from a database other than this one (a restored backup, say).
            return Ok(FeedBatch {
                seq: batch.seq,
                changes: vec![FeedChange {
                    seq: batch.seq,
                    kind: feed::RESYNC.to_owned(),
                    ticket: None,
                    name: None,
                }],
            });
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if !batch.changes.is_empty() || left.is_zero() {
            return Ok(batch);
        }
        wait_for_write(seen, left.min(RECHECK));
    }
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}
