//! The journal: the process's structured log, one JSON record per line. Each record says when
//! (wall clock and milliseconds since the process began), how serious, which part of the
//! program (`source`), what happened and any data that would reproduce it. It goes to three
//! places:
//!
//! - a ring of the latest [`CAPACITY`] records, which a bug report copies ([`recent`]);
//! - the file [`open`] names, if any (the app's is `~/Library/Logs/ca.thurn.beryl/beryl.jsonl`),
//!   flushed record by record so a crash loses nothing, and rotated to `<name>.1` past
//!   [`ROTATE_BYTES`];
//! - standard error, for warnings and errors, as `beryl: <source>: <message>`.
//!
//! [`install_panic_hook`] journals every panic with its thread and backtrace before the default
//! hook runs, so a game thread that dies says why.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// How many records the ring keeps.
pub const CAPACITY: usize = 4000;
/// The size past which the file is rotated.
pub const ROTATE_BYTES: u64 = 16 * 1024 * 1024;

/// How serious a record is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Routine traffic: each view, each command.
    Debug,
    /// Milestones: a game began or ended, a report was written.
    Info,
    /// Something went wrong that the program recovered from.
    Warn,
    /// Something went wrong that lost work or state.
    Error,
}

/// One record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// Wall clock, as UTC `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub time: String,
    /// Milliseconds since the process began: orders records even if the wall clock jumps.
    pub uptime_ms: u64,
    pub level: Level,
    /// The part of the program: `app`, `session`, `ui`, `panic`.
    pub source: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

struct Journal {
    started: Instant,
    ring: Mutex<VecDeque<Record>>,
    sink: Mutex<Option<Sink>>,
}

struct Sink {
    path: PathBuf,
    file: File,
    written: u64,
}

static JOURNAL: LazyLock<Journal> = LazyLock::new(|| Journal {
    started: Instant::now(),
    ring: Mutex::new(VecDeque::with_capacity(CAPACITY)),
    sink: Mutex::new(None),
});

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Journals to `path` from now on, appending, after the records so far (which the ring still
/// holds). Its directory is made if missing.
pub fn open(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let written = file.metadata().map_or(0, |m| m.len());
    *lock(&JOURNAL.sink) = Some(Sink {
        path: path.to_owned(),
        file,
        written,
    });
    info(
        "journal",
        "opened",
        serde_json::json!({
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
        }),
    );
    Ok(())
}

/// The file the journal writes to, if any.
pub fn path() -> Option<PathBuf> {
    lock(&JOURNAL.sink).as_ref().map(|s| s.path.clone())
}

/// Journals a record.
pub fn record(level: Level, source: &str, message: impl Into<String>, data: Value) {
    let journal = &*JOURNAL;
    let record = Record {
        time: utc(SystemTime::now()),
        uptime_ms: u64::try_from(journal.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        level,
        source: source.to_owned(),
        message: message.into(),
        thread: std::thread::current().name().map(str::to_owned),
        data,
    };
    if level >= Level::Warn {
        if record.data.is_null() {
            eprintln!("beryl: {}: {}", record.source, record.message);
        } else {
            eprintln!(
                "beryl: {}: {} {}",
                record.source, record.message, record.data
            );
        }
    }
    let line = serde_json::to_string(&record).expect("a record serializes");
    {
        let mut ring = lock(&journal.ring);
        if ring.len() == CAPACITY {
            ring.pop_front();
        }
        ring.push_back(record);
    }
    let mut sink = lock(&journal.sink);
    if let Some(s) = sink.as_mut()
        && let Err(e) = s.write(&line)
    {
        // Journaling about the journal would recurse: say it once and stop writing.
        eprintln!(
            "beryl: journal: {}: {e}; no longer writing it",
            s.path.display()
        );
        *sink = None;
    }
}

impl Sink {
    fn write(&mut self, line: &str) -> std::io::Result<()> {
        if self.written > ROTATE_BYTES {
            let old = self.path.with_extension("jsonl.1");
            std::fs::rename(&self.path, &old)?;
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.written = 0;
        }
        writeln!(self.file, "{line}")?;
        self.file.flush()?;
        self.written += line.len() as u64 + 1;
        Ok(())
    }
}

pub fn debug(source: &str, message: impl Into<String>, data: Value) {
    record(Level::Debug, source, message, data);
}

pub fn info(source: &str, message: impl Into<String>, data: Value) {
    record(Level::Info, source, message, data);
}

pub fn warn(source: &str, message: impl Into<String>, data: Value) {
    record(Level::Warn, source, message, data);
}

pub fn error(source: &str, message: impl Into<String>, data: Value) {
    record(Level::Error, source, message, data);
}

/// The latest `n` records, oldest first.
pub fn recent(n: usize) -> Vec<Record> {
    let ring = lock(&JOURNAL.ring);
    ring.iter()
        .skip(ring.len().saturating_sub(n))
        .cloned()
        .collect()
}

/// Journals every panic, with its thread, place and a backtrace, then runs the hook that was
/// installed before.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic with no message".to_owned());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        record(
            Level::Error,
            "panic",
            message,
            serde_json::json!({ "location": location, "backtrace": backtrace }),
        );
        previous(info);
    }));
}

/// `t` as UTC `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn utc(t: SystemTime) -> String {
    let since = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (date, secs) = civil(since.as_secs());
    format!(
        "{date}T{:02}:{:02}:{:02}.{:03}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        since.subsec_millis()
    )
}

/// The UTC date `YYYY-MM-DD` of `unix` seconds, and the seconds into that day. Howard
/// Hinnant's `civil_from_days`.
pub(crate) fn civil(unix: u64) -> (String, u64) {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (format!("{year:04}-{month:02}-{day:02}"), secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_formats_the_wall_clock() {
        let t = UNIX_EPOCH + Duration::from_millis(1_794_148_212_345);
        assert_eq!(utc(t), "2026-11-08T14:30:12.345Z");
        assert_eq!(utc(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn the_ring_keeps_the_latest_records_and_the_file_gets_each() {
        let dir = std::env::temp_dir().join(format!("beryl-journal-{}", std::process::id()));
        let path = dir.join("test.jsonl");
        open(&path).unwrap();
        debug("test", "first", Value::Null);
        info("test", "second", serde_json::json!({ "n": 2 }));
        // Other tests journal from their threads meanwhile.
        let ours: Vec<Record> = recent(CAPACITY)
            .into_iter()
            .filter(|r| r.source == "test")
            .collect();
        assert_eq!(ours[ours.len() - 2].message, "first");
        assert_eq!(ours[ours.len() - 1].data["n"], 2);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.lines().any(|l| {
                serde_json::from_str::<Record>(l).is_ok_and(|r| r.message == "second")
            })
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
