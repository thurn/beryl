//! The hub's backups: an hourly snapshot of the database and a daily export of the tickets, as
//! the ticket service made them.
//!
//! A snapshot is a directory named after its time (`2026-11-17T14-00-00.000Z`) holding the
//! compressed database and `manifest.json` with the time, feed position and database SHA-256.
//! The database is checked with `PRAGMA integrity_check` before compression. The directory is
//! published only after the archive decompresses to the same checksum. To restore a copy into a
//! scratch directory, run `zstd -d hub.sqlite.zst -o hub.sqlite` and compare that database's
//! SHA-256 with the manifest. The newest 6 snapshots are kept, plus the newest of each day for
//! 7 days. A 48 GiB budget drops extra hourly copies before daily copies and keeps the newest
//! snapshot. Older raw SQLite backups are compressed by upkeep and keep their manifests.
//! Every daemon's ticket mirror is the longer record. A staging directory that a restart left
//! behind is removed with the old snapshots.
//!
//! An export is `exports/<day>.json`: every ticket with its comments and history. The newest 30
//! are kept.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufRead as _, BufReader, Write as _};
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

/// The compressed database inside a completed snapshot.
pub(super) const ARCHIVE: &str = "hub.sqlite.zst";
const MANIFEST: &str = "manifest.json";
const SNAPSHOTS_KEPT: usize = 6;
const DAILY_DAYS: f64 = 7.0;
const SNAPSHOT_BUDGET_BYTES: u64 = 48 * 1024 * 1024 * 1024;
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
    prune_snapshots(dir, now, SNAPSHOT_BUDGET_BYTES)?;
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
    let digest =
        archive_database(staging, None).map_err(|e| io_error("compressing a snapshot", &e))?;
    let manifest = json!({ "timestamp": now.to_string(), "seq": seq.0, "sha256": digest });
    fs::write(staging.join(MANIFEST), format!("{manifest}\n"))
        .map_err(|e| io_error("writing a snapshot manifest", &e))
}

/// Upgrades raw snapshots independently so one damaged copy does not hold up the others.
pub(super) fn compress_existing(dir: &Path) -> Result<()> {
    let mut first_error = None;
    for (_, path) in snapshots(dir)? {
        if let Err(error) = compress_existing_snapshot(&path) {
            first_error.get_or_insert_with(|| {
                io_error(&format!("compressing snapshot {}", path.display()), &error)
            });
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn compress_existing_snapshot(path: &Path) -> io::Result<()> {
    if path.join(DATABASE).try_exists()? {
        let manifest: serde_json::Value =
            serde_json::from_reader(File::open(path.join(MANIFEST))?)?;
        let expected = manifest["sha256"].as_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "missing snapshot checksum")
        })?;
        archive_database(path, Some(expected))?;
    }
    Ok(())
}

/// Publishes the archive before removing the raw database. The raw file survives a failed copy
/// or checksum and is picked up again on the next pass.
fn archive_database(dir: &Path, expected: Option<&str>) -> io::Result<String> {
    let database = dir.join(DATABASE);
    let temporary = dir.join(".hub.sqlite.zst.partial");
    let result = write_archive(&database, &temporary, expected).and_then(|checksum| {
        fs::rename(&temporary, dir.join(ARCHIVE))?;
        #[cfg(unix)]
        File::open(dir)?.sync_all()?;
        fs::remove_file(&database)?;
        Ok(checksum)
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_archive(database: &Path, archive: &Path, expected: Option<&str>) -> io::Result<String> {
    let mut reader = BufReader::with_capacity(64 * 1024, File::open(database)?);
    let mut encoder = zstd::stream::write::Encoder::new(File::create(archive)?, 1)?;
    let mut digest = Sha256::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        digest.update(chunk);
        encoder.write_all(chunk)?;
        let len = chunk.len();
        reader.consume(len);
    }
    let checksum = super::hex(&digest.finalize());
    if expected.is_some_and(|expected| expected != checksum) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "database checksum differs from its manifest",
        ));
    }
    encoder.finish()?.sync_all()?;

    let mut decoder = zstd::stream::read::Decoder::new(File::open(archive)?)?;
    let mut restored_digest = Sha256::new();
    io::copy(&mut decoder, &mut restored_digest)?;
    if super::hex(&restored_digest.finalize()) != checksum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "archive checksum differs from its database",
        ));
    }
    Ok(checksum)
}

/// Applies the count, age and byte limits to completed backups. Only one hub writes snapshots,
/// and it has renamed its own staging directory by the time it removes abandoned ones.
fn prune_snapshots(dir: &Path, now: Timestamp, budget_bytes: u64) -> Result<()> {
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
    let mut days = HashSet::new();
    let mut hourly_copies = Vec::new();
    let mut daily_copies = Vec::new();
    let mut total_bytes = 0;
    for (index, (at, path)) in snapshots(dir)?.into_iter().enumerate() {
        let daily = days.insert(at.day()) && now.seconds_since(at) < DAILY_DAYS * 86_400.0;
        if index >= SNAPSHOTS_KEPT && !daily {
            fs::remove_dir_all(&path).map_err(|e| io_error("removing an old snapshot", &e))?;
            continue;
        }
        let bytes = snapshot_bytes(&path).map_err(|e| io_error("measuring a snapshot", &e))?;
        total_bytes += bytes;
        if index == 0 {
            continue;
        }
        if daily {
            daily_copies.push((path, bytes));
        } else {
            hourly_copies.push((path, bytes));
        }
    }
    for (path, bytes) in hourly_copies
        .into_iter()
        .rev()
        .chain(daily_copies.into_iter().rev())
    {
        if total_bytes <= budget_bytes {
            break;
        }
        fs::remove_dir_all(&path).map_err(|e| io_error("removing an old snapshot", &e))?;
        total_bytes -= bytes;
    }
    Ok(())
}

/// Counts stored bytes, including any raw database left by an upgrade that could not finish.
fn snapshot_bytes(path: &Path) -> io::Result<u64> {
    let mut bytes = 0;
    for entry in fs::read_dir(path)? {
        let metadata = entry?.metadata()?;
        if metadata.is_file() {
            bytes += metadata.len();
        }
    }
    Ok(bytes)
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

    #[test]
    fn checksum_failure_keeps_the_raw_backup() {
        let dir = ignite_core::test_files::temp_dir().join("backup-checksum-failure");
        let _ = fs::remove_dir_all(&dir);
        let at = Timestamp::parse("2026-11-21T12:30:00Z").unwrap();
        let path = dir.join(snapshot_name(at));
        fs::create_dir_all(&path).unwrap();
        let database = path.join(DATABASE);
        fs::write(&database, b"changed since the snapshot").unwrap();
        fs::write(path.join(MANIFEST), br#"{"sha256": "original"}"#).unwrap();

        let error = compress_existing(&dir).unwrap_err();
        assert!(error.to_string().contains("differs from its manifest"));
        assert_eq!(fs::read(database).unwrap(), b"changed since the snapshot");
        assert!(!path.join(ARCHIVE).exists());
        assert!(!path.join(".hub.sqlite.zst.partial").exists());
        fs::remove_dir_all(dir).unwrap();
    }

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

    #[test]
    fn pruning_by_bytes_drops_oldest_hourly_copies_before_daily_copies() {
        let dir = ignite_core::test_files::temp_dir().join("backup-byte-budget");
        fs::create_dir(&dir).unwrap();
        let newest = Timestamp::parse("2026-11-21T12:30:00Z").unwrap();
        let recent_hourly = newest.plus_seconds(-3_600.0);
        let older_hourly = newest.plus_seconds(-7_200.0);
        let yesterday = newest.plus_seconds(-86_400.0);
        let two_days_ago = newest.plus_seconds(-172_800.0);
        for (at, database_bytes) in [
            (newest, 48),
            (recent_hourly, 48),
            (older_hourly, 32),
            (yesterday, 40),
            (two_days_ago, 32),
        ] {
            let path = dir.join(snapshot_name(at));
            fs::create_dir(&path).unwrap();
            fs::write(path.join(DATABASE), vec![0; database_bytes]).unwrap();
            fs::write(path.join("manifest.json"), b"manifest").unwrap();
        }
        let kept = || {
            snapshots(&dir)
                .unwrap()
                .into_iter()
                .map(|(at, _)| at)
                .collect::<Vec<_>>()
        };

        prune_snapshots(&dir, newest, 200).unwrap();
        assert_eq!(kept(), [newest, recent_hourly, yesterday, two_days_ago]);

        prune_snapshots(&dir, newest, 104).unwrap();
        assert_eq!(kept(), [newest, yesterday]);

        prune_snapshots(&dir, newest, 1).unwrap();
        assert_eq!(kept(), [newest]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
