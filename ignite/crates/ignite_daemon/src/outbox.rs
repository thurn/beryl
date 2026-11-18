//! The outbox: records and results queue in the local database and are sent to the hub in order
//! whenever it is reachable. An entry leaves the queue only once the hub has accepted it, so
//! nothing is lost while the hub is down or across a restart of the daemon.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;

use ignite_core::protocol::hub::{NewResult, Record};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Hub, Result};
use ignite_store::local::{outbox as store, rows};

const RECORD: &str = "record";
const RESULT: &str = "result";

/// How many entries one flush reads at a time.
const BATCH: u32 = 200;

pub fn push_record(conn: &Connection, record: &Record, at: Timestamp) -> Result<()> {
    push(conn, RECORD, record, at)
}

pub fn push_result(conn: &Connection, result: &NewResult, at: Timestamp) -> Result<()> {
    push(conn, RESULT, result, at)
}

fn push(conn: &Connection, kind: &str, body: &impl Serialize, at: Timestamp) -> Result<()> {
    let body = serde_json::to_string(body).map_err(|e| Error::Invalid(e.to_string()))?;
    store::push(conn, kind, &body, &at.to_string()).map_err(|e| Error::Io(e.to_string()))?;
    Ok(())
}

/// Sends what is queued, oldest first, with one hub request per run of entries of one kind.
/// Returns how many entries were sent. On an error, the entries sent before it stay sent and
/// the rest stay queued.
///
/// An entry the hub rejects as malformed, or that no longer parses, is dropped with a message
/// on standard error: keeping it would hold back every entry behind it for good.
pub fn flush(conn: &Connection, hub: &dyn Hub) -> Result<u32> {
    let mut sent = 0;
    loop {
        let entries = store::peek(conn, BATCH).map_err(|e| Error::Io(e.to_string()))?;
        let Some(first) = entries.first() else {
            return Ok(sent);
        };
        let run: Vec<&rows::OutboxEntry> = entries
            .iter()
            .take_while(|entry| entry.kind == first.kind)
            .collect();
        let last = run.last().map_or(first.seq, |entry| entry.seq);
        match send(hub, &first.kind, &run) {
            Ok(()) => sent += u32::try_from(run.len()).unwrap_or(u32::MAX),
            Err(Error::Invalid(message)) => {
                eprintln!(
                    "ignite: dropped {} outbox entries up to {last}: {message}",
                    run.len()
                );
            }
            Err(error) => return Err(error),
        }
        store::ack(conn, last).map_err(|e| Error::Io(e.to_string()))?;
    }
}

fn send(hub: &dyn Hub, kind: &str, run: &[&rows::OutboxEntry]) -> Result<()> {
    match kind {
        RECORD => hub.records(&parse::<Record>(run)?),
        RESULT => hub.store(&parse::<NewResult>(run)?),
        other => Err(Error::Invalid(format!("unknown outbox kind {other:?}"))),
    }
}

fn parse<T: serde::de::DeserializeOwned>(run: &[&rows::OutboxEntry]) -> Result<Vec<T>> {
    run.iter()
        .map(|entry| {
            serde_json::from_str(&entry.body)
                .map_err(|e| Error::Invalid(format!("entry {}: {e}", entry.seq)))
        })
        .collect()
}

/// One round of the daemon's sender: flush the outbox, then send the machine's heartbeat with
/// `beat`. A failure of either is reported on standard error and tried again next round.
///
/// `conn` is the sender's own connection to the local database, so that a slow hub never holds
/// up the connection that answers requests.
pub fn tick(conn: &Connection, hub: &dyn Hub, beat: &dyn Fn() -> Result<()>) {
    if let Err(error) = flush(conn, hub) {
        eprintln!("ignite: outbox not sent: {error}");
    }
    if let Err(error) = beat() {
        eprintln!("ignite: heartbeat not sent: {error}");
    }
}

/// Runs [`tick`] every `period` until `stop` is set.
pub fn run(
    conn: &Connection,
    hub: &dyn Hub,
    beat: &dyn Fn() -> Result<()>,
    period: Duration,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Relaxed) {
        tick(conn, hub, beat);
        std::thread::sleep(period);
    }
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
