//! The outbox: records and results queue in the local database and are sent to the hub in order
//! whenever it is reachable. An entry leaves the queue only once the hub has accepted it, so
//! nothing is lost while the hub is down or across a restart of the daemon.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;

use ignite_core::protocol::cli::TranscriptAppend;
use ignite_core::protocol::hub::{FlakyReport, MetricsImport, NewResult, Record};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Hub, Result};
use ignite_store::local::{outbox as store, rows};

use crate::hubclient::too_large;

const RECORD: &str = "record";
const RESULT: &str = "result";
const FLAKY: &str = "flaky";
const METRICS: &str = "metrics";
const TRANSCRIPT: &str = "transcript";

/// How many entries one flush reads at a time.
const BATCH: u32 = 200;

pub fn push_record(conn: &Connection, record: &Record, at: Timestamp) -> Result<()> {
    push(conn, RECORD, record, at)
}

pub fn push_result(conn: &Connection, result: &NewResult, at: Timestamp) -> Result<()> {
    push(conn, RESULT, result, at)
}

pub fn push_flaky(conn: &Connection, report: &FlakyReport, at: Timestamp) -> Result<()> {
    push(conn, FLAKY, report, at)
}

/// Queues rows for the hub's `POST /metrics/import`, such as a CLI turn's cost.
pub fn push_metrics(conn: &Connection, import: &MetricsImport, at: Timestamp) -> Result<()> {
    push(conn, METRICS, import, at)
}

/// Queues digest lines of a CLI worker's transcript. The hub stores a line once whatever
/// the number of times it is sent, so a line queued again after a restart does no harm.
pub fn push_transcript(conn: &Connection, append: &TranscriptAppend, at: Timestamp) -> Result<()> {
    push(conn, TRANSCRIPT, append, at)
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
///
/// When the hub refuses a run as too large, the run is sent again in halves until each piece
/// fits. A single entry too large on its own is moved aside, into a `.bad` file in the
/// `outbox` directory beside the database ([`set_aside`]), and reported on standard error, so
/// that it holds back nothing behind it.
pub fn flush(conn: &Connection, hub: &dyn Hub) -> Result<u32> {
    let mut sent = 0;
    let mut limit = BATCH;
    loop {
        let entries = store::peek(conn, limit).map_err(|e| Error::Io(e.to_string()))?;
        let Some(first) = entries.first() else {
            return Ok(sent);
        };
        let run: Vec<&rows::OutboxEntry> = entries
            .iter()
            .take_while(|entry| entry.kind == first.kind)
            .collect();
        let last = run.last().map_or(first.seq, |entry| entry.seq);
        match send(hub, &first.kind, &run) {
            Ok(()) => {
                sent += u32::try_from(run.len()).unwrap_or(u32::MAX);
                limit = BATCH;
            }
            Err(error) if too_large(&error) && run.len() > 1 => {
                limit = u32::try_from(run.len() / 2).unwrap_or(BATCH);
                continue;
            }
            Err(error) if too_large(&error) => set_aside(conn, first, &error),
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

/// Writes an entry the hub refused as too large to `<seq>-<kind>.bad` in the `outbox`
/// directory beside the database, so that it can be looked at or sent again by hand, and says
/// so on standard error. The entry then leaves the queue. A database with no file (one in
/// memory) has no directory to write to, so its entry is only reported.
fn set_aside(conn: &Connection, entry: &rows::OutboxEntry, error: &Error) {
    let kept = bad_file(conn, entry).and_then(|path| {
        let dir = path.parent()?;
        fs::create_dir_all(dir).ok()?;
        fs::write(&path, &entry.body).ok()?;
        Some(path)
    });
    match kept {
        Some(path) => eprintln!(
            "ignite: outbox entry {} ({}, {} bytes) is too large for the hub and was moved to \
             {}: {error}",
            entry.seq,
            entry.kind,
            entry.body.len(),
            path.display()
        ),
        None => eprintln!(
            "ignite: dropped outbox entry {} ({}, {} bytes), which is too large for the hub \
             and could not be kept in a file: {error}",
            entry.seq,
            entry.kind,
            entry.body.len()
        ),
    }
}

/// Where [`set_aside`] keeps `entry`, or `None` for a database with no file.
fn bad_file(conn: &Connection, entry: &rows::OutboxEntry) -> Option<PathBuf> {
    let database = Path::new(conn.path().filter(|path| !path.is_empty())?);
    let name = format!("{}-{}.bad", entry.seq, entry.kind);
    Some(database.parent()?.join("outbox").join(name))
}

fn send(hub: &dyn Hub, kind: &str, run: &[&rows::OutboxEntry]) -> Result<()> {
    match kind {
        RECORD => hub.records(&parse::<Record>(run)?),
        RESULT => hub.store(&parse::<NewResult>(run)?),
        FLAKY => parse::<FlakyReport>(run)?
            .iter()
            .try_for_each(|report| hub.flaky(report)),
        METRICS => parse::<MetricsImport>(run)?
            .iter()
            .try_for_each(|import| hub.metrics(import)),
        TRANSCRIPT => {
            // One request for the run: its entries are appends of one machine, in order.
            let appends = parse::<TranscriptAppend>(run)?;
            let Some(first) = appends.first() else {
                return Ok(());
            };
            hub.transcripts(&TranscriptAppend {
                machine: first.machine.clone(),
                lines: appends.into_iter().flat_map(|a| a.lines).collect(),
            })
        }
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
/// `beat`. A failure of either is reported on standard error and tried again next round, and
/// the round's result names it.
///
/// `conn` is the sender's own connection to the local database, so that a slow hub never holds
/// up the connection that answers requests.
pub fn tick(
    conn: &Connection,
    hub: &dyn Hub,
    beat: &dyn Fn() -> Result<()>,
) -> std::result::Result<(), String> {
    let mut failures = Vec::new();
    if let Err(error) = flush(conn, hub) {
        crate::hubclient::HUB_WRITES.failed("outbox not sent", &error);
        failures.push(format!("outbox not sent: {error}"));
    }
    if let Err(error) = beat() {
        crate::hubclient::HUB_WRITES.failed("heartbeat not sent", &error);
        failures.push(format!("heartbeat not sent: {error}"));
    } else {
        crate::hubclient::HUB_WRITES.succeeded();
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
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
        let _ = tick(conn, hub, beat);
        std::thread::sleep(period);
    }
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
