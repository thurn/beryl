//! The hub's backups: an hourly snapshot of the database and a daily export of the tickets, as
//! the ticket service made them.
//!
//! A snapshot is a directory named after its time (`2026-11-17T14-00-00.000Z`) holding the
//! database file and `manifest.json` with the time, the feed position and the file's SHA-256.
//! It is written under a temporary name, checked with `PRAGMA integrity_check`, and renamed
//! only when complete. The newest 6 snapshots are kept, and beyond them the newest of each day
//! for 7 days. A snapshot is a whole copy of the database, several gigabytes on a busy hub, and
//! keeping 48 hourly and 30 daily copies took over 100 GB of the hub's disk, which the build
//! trees on the same disk needed. Every daemon's ticket mirror is the longer record. A staging
//! directory that a restart left behind is removed with the old snapshots.
//!
//! An export is `exports/<day>.json`: every ticket with its comments and history. The newest 30
//! are kept.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde_json::json;
use sha2::{Digest, Sha256};

use ignite_core::protocol::tickets::TicketQuery;
use ignite_core::time::Timestamp;
use ignite_store::hub::{feed, tickets};
use ignite_store::{Result, StoreError};

/// The file name of the database inside a snapshot.
pub const DATABASE: &str = "hub.sqlite";

const SNAPSHOTS_KEPT: usize = 6;
const DAILY_DAYS: f64 = 7.0;
const EXPORTS_KEPT: usize = 30;

fn io_error(what: &str, error: &io::Error) -> StoreError {
    StoreError::Invalid(format!("{what}: {error}"))
}

/// The directory name of a snapshot taken at `at`.
fn snapshot_name(at: Timestamp) -> String {
    at.to_string().replace(':', "-")
}

/// The time a snapshot directory is named after, if the name is one.
fn snapshot_time(name: &str) -> Option<Timestamp> {
    let (day, time) = name.split_once('T')?;
    Timestamp::parse(&format!("{day}T{}", time.replace('-', ":"))).ok()
}

/// The completed snapshots in `dir`, newest first.
pub fn snapshots(dir: &Path) -> Result<Vec<(Timestamp, PathBuf)>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error("listing backups", &error)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_error("listing backups", &e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(at) = snapshot_time(&name).filter(|_| entry.path().is_dir()) {
            found.push((at, entry.path()));
        }
    }
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    Ok(found)
}

/// Writes a snapshot of the database into `dir`, then removes the snapshots no longer kept.
/// Returns the new snapshot's directory.
pub fn snapshot(conn: &Connection, dir: &Path, now: Timestamp) -> Result<PathBuf> {
    fs::create_dir_all(dir).map_err(|e| io_error("creating the backup directory", &e))?;
    let name = snapshot_name(now);
    let staging = dir.join(format!(".snapshot-{name}"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir(&staging).map_err(|e| io_error("creating a snapshot", &e))?;
    let written = write_snapshot(conn, &staging, now);
    if let Err(error) = written {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let destination = dir.join(&name);
    fs::rename(&staging, &destination).map_err(|e| io_error("completing a snapshot", &e))?;
    prune_snapshots(dir, now)?;
    Ok(destination)
}

fn write_snapshot(conn: &Connection, staging: &Path, now: Timestamp) -> Result<()> {
    let file = staging.join(DATABASE);
    // VACUUM INTO writes a consistent copy while other connections keep writing.
    conn.execute("VACUUM INTO ?1", params![file.to_string_lossy()])?;
    let copy = Connection::open(&file)?;
    let integrity: String = copy.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(StoreError::Invalid(format!(
            "the snapshot failed its integrity check: {integrity}"
        )));
    }
    let seq = feed::current(&copy)?;
    drop(copy);
    let bytes = fs::read(&file).map_err(|e| io_error("reading a snapshot", &e))?;
    let digest = super::hex(&Sha256::digest(&bytes));
    let manifest = json!({ "timestamp": now.to_string(), "seq": seq.0, "sha256": digest });
    fs::write(staging.join("manifest.json"), format!("{manifest}\n"))
        .map_err(|e| io_error("writing a snapshot manifest", &e))
}

/// Keeps the newest 6 snapshots, and of the older ones the newest of each day for 7 days. Removes
/// every staging directory: a daemon restarted while `VACUUM INTO` was writing one left it
/// behind, and nothing else ever removed it. Only one hub writes snapshots, and it has renamed
/// its own staging directory by the time it prunes.
fn prune_snapshots(dir: &Path, now: Timestamp) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|e| io_error("listing backups", &e))? {
        let entry = entry.map_err(|e| io_error("listing backups", &e))?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with(".snapshot-")
        {
            fs::remove_dir_all(entry.path())
                .map_err(|e| io_error("removing an abandoned snapshot", &e))?;
        }
    }
    let mut days = std::collections::HashSet::new();
    for (index, (at, path)) in snapshots(dir)?.into_iter().enumerate() {
        let daily = days.insert(at.day()) && now.seconds_since(at) < DAILY_DAYS * 86_400.0;
        if index >= SNAPSHOTS_KEPT && !daily {
            fs::remove_dir_all(&path).map_err(|e| io_error("removing an old snapshot", &e))?;
        }
    }
    Ok(())
}

/// Writes today's export into `dir/exports`, then removes the exports no longer kept. Returns
/// the export's path.
pub fn export(conn: &Connection, dir: &Path, now: Timestamp) -> Result<PathBuf> {
    let exports = dir.join("exports");
    fs::create_dir_all(&exports).map_err(|e| io_error("creating the export directory", &e))?;
    let query = TicketQuery {
        all: true,
        ..TicketQuery::default()
    };
    let mut rows = Vec::new();
    for ticket in tickets::list(conn, &query, now)? {
        let id = ticket.id.to_string();
        rows.push(json!({
            "ticket": ticket,
            "comments": tickets::comments(conn, &id)?,
            "history": tickets::history(conn, &id)?,
        }));
    }
    let document = json!({
        "timestamp": now.to_string(),
        "seq": feed::current(conn)?.0,
        "tickets": rows,
    });
    let path = exports.join(format!("{}.json", now.day()));
    let partial = exports.join(format!(".{}.json", now.day()));
    fs::write(&partial, format!("{document}\n")).map_err(|e| io_error("writing an export", &e))?;
    fs::rename(&partial, &path).map_err(|e| io_error("completing an export", &e))?;
    let mut kept: Vec<PathBuf> = fs::read_dir(&exports)
        .map_err(|e| io_error("listing exports", &e))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json") && !hidden(p))
        .collect();
    kept.sort();
    if kept.len() > EXPORTS_KEPT {
        for old in &kept[..kept.len() - EXPORTS_KEPT] {
            fs::remove_file(old).map_err(|e| io_error("removing an old export", &e))?;
        }
    }
    Ok(path)
}

fn hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

/// The day of the newest export in `dir/exports`, if any.
pub fn last_export_day(dir: &Path) -> Option<String> {
    fs::read_dir(dir.join("exports"))
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .filter_map(|name| name.strip_suffix(".json").map(str::to_owned))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Backups filled the hub's disk: 48 hourly copies of a database of several gigabytes, and
    /// staging directories that restarts left behind and nothing removed.
    #[test]
    fn pruning_keeps_six_recent_snapshots_and_one_a_day_for_a_week() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-backup-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let now = Timestamp::parse("2026-11-21T12:30:00Z").unwrap();
        // Hourly snapshots for the last ten days, and a staging directory from a restart.
        for hour in 1..=240 {
            let at = now.plus_seconds(-3_600.0 * f64::from(hour));
            fs::create_dir(dir.join(snapshot_name(at))).unwrap();
        }
        let abandoned = dir.join(".snapshot-2026-11-20T03-08-19.597Z");
        fs::create_dir(&abandoned).unwrap();
        let conn = ignite_store::hub::open_in_memory().unwrap();

        snapshot(&conn, &dir, now).unwrap();

        let kept = snapshots(&dir).unwrap();
        // The new snapshot and five more from the last hours, then the newest of each older day
        // within the week: today's earlier hours are already covered by the recent six.
        assert_eq!(kept.len(), 6 + 7, "{kept:?}");
        assert!(
            kept.iter()
                .all(|(at, _)| now.seconds_since(*at) < 7.0 * 86_400.0)
        );
        assert!(!abandoned.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
