//! Fleet efficiency reads on the private listener. Send the registered machine's bearer token
//! and `X-Actor`, as for the other authenticated hub APIs.
//!
//! - `GET /efficiency/records?after=<id>&limit=<rows>` returns `records` and `next_id`.
//! - `GET /efficiency/metrics?cursor=<token>&limit=<rows>` returns `rows`, `headers` and
//!   `next_cursor`. Persist the cursor as returned. The rows and headers carry their original
//!   JSON lines as strings in `data`.
//! - `GET /efficiency/state` returns the compact hub state used by efficiency reports.
//!
//! Omit the position for the first page. An empty page returns the current position, which can
//! be used again to pick up later arrivals. Pages default to 1,000 rows and allow up to 5,000.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_store::hub::efficiency;

use super::{Request, Response};

const DEFAULT_PAGE: i64 = 1_000;
const MAX_PAGE: i64 = 5_000;
const CURSOR_VERSION: &str = "v1-";

pub fn handle(conn: &mut Connection, request: &Request, private: bool) -> Option<Response> {
    let read = match request.path.as_str() {
        "/efficiency/records" => records,
        "/efficiency/metrics" => metrics,
        "/efficiency/state" => state,
        _ => return None,
    };
    let response = if !private {
        failure(404, "Unknown route")
    } else if request.method != "GET" {
        failure(405, "use GET for efficiency reads")
    } else if request.writer.is_none() {
        failure(
            401,
            "efficiency reads need a registered machine's token and X-Actor",
        )
    } else {
        match read(conn, request) {
            Ok(value) => Response::json(200, &value),
            Err(error) => error,
        }
    };
    Some(response.with_header("Cache-Control", "no-store".to_owned()))
}

fn records(conn: &mut Connection, request: &Request) -> Result<Value, Response> {
    let after = match request.param("after") {
        None => 0,
        Some(value) => value
            .parse::<i64>()
            .ok()
            .filter(|id| *id >= 0)
            .ok_or_else(|| failure(400, "after must be a nonnegative record ID"))?,
    };
    let records =
        efficiency::records(conn, after, limit(request)?).map_err(|e| store_failure(&e))?;
    let next = records.last().map_or(after, |row| row.record.id);
    Ok(json!({ "records": records, "next_id": next }))
}

fn metrics(conn: &mut Connection, request: &Request) -> Result<Value, Response> {
    let after = match request.param("cursor") {
        None => 0,
        Some(value) => value
            .strip_prefix(CURSOR_VERSION)
            .and_then(|id| i64::from_str_radix(id, 16).ok())
            .filter(|id| *id >= 0)
            .ok_or_else(|| failure(400, "invalid metrics cursor"))?,
    };
    let page = efficiency::metrics(conn, after, limit(request)?).map_err(|e| store_failure(&e))?;
    Ok(json!({
        "rows": page.rows,
        "headers": page.headers,
        "next_cursor": format!("{CURSOR_VERSION}{:x}", page.last_id),
    }))
}

fn state(conn: &mut Connection, _: &Request) -> Result<Value, Response> {
    Ok(json!(
        efficiency::state(conn).map_err(|e| store_failure(&e))?
    ))
}

fn limit(request: &Request) -> Result<i64, Response> {
    match request.param("limit") {
        None => Ok(DEFAULT_PAGE),
        Some(value) => value
            .parse::<i64>()
            .ok()
            .filter(|limit| (1..=MAX_PAGE).contains(limit))
            .ok_or_else(|| failure(400, "limit must be between 1 and 5000")),
    }
}

fn store_failure(error: &ignite_store::StoreError) -> Response {
    eprintln!("ignite hub: reading fleet efficiency: {error}");
    failure(500, "could not read fleet efficiency")
}

fn failure(status: u16, error: &str) -> Response {
    Response::json(status, &json!({ "error": error }))
}
