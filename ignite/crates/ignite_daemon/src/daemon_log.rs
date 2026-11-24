//! The diagnostic log shared by the Windows daemon and its justiciar monitor.
//!
//! A scheduled task has no persistent stderr. Write failures as they happen so a later
//! successful round or a new daemon process cannot erase them from the monitor's evidence.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ignite_core::time::{SystemClock, Timestamp};
use ignite_core::traits::Clock;

const MAX_BYTES: u64 = 256 * 1024;
const MAX_RECORD_BYTES: usize = 8 * 1024;

fn path(home: &Path) -> PathBuf {
    home.join("ignite.daemon.log")
}

pub(crate) fn read(home: &Path) -> io::Result<Vec<u8>> {
    let path = path(home);
    let lock = File::open(path.with_extension("log.lock"))?;
    lock.lock_shared()?;
    fs::read(path)
}

/// Prints a lifecycle diagnostic and also records it on Windows.
pub(crate) fn report(home: &Path, message: fmt::Arguments<'_>) {
    record(home, SystemClock.now(), "daemon", message);
    eprintln!("ignite: {message}");
}

/// The log is best effort so a local I/O failure cannot stop the daemon.
pub(crate) fn record(home: &Path, at: Timestamp, source: &str, message: fmt::Arguments<'_>) {
    if cfg!(windows)
        && let Err(error) = append(home, at, source, message)
    {
        let _ = writeln!(
            io::stderr(),
            "ignite: cannot write {}: {error}",
            path(home).display()
        );
    }
}

fn append(home: &Path, at: Timestamp, source: &str, message: fmt::Arguments<'_>) -> io::Result<()> {
    let mut line = format!("{at} [{source}] (pid {}) {message}", std::process::id())
        .replace('\r', "\\r")
        .replace('\n', "\\n");
    line.truncate(line.floor_char_boundary(MAX_RECORD_BYTES - 1));
    line.push('\n');

    let path = path(home);
    // During a Windows restart, the old process may still be logging when its successor
    // starts. Lock a separate file so replacing the log cannot make the lock ineffective.
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path.with_extension("log.lock"))?;
    lock.lock()?;

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&path)?;
    let size = file.metadata()?.len();
    if size + line.len() as u64 <= MAX_BYTES {
        return file.write_all(line.as_bytes());
    }

    // Keep the newest half, starting at a complete line. Replace the file in one step so a
    // daemon that stops during compaction leaves its previous log on disk.
    let keep = MAX_BYTES / 2;
    let offset = size.saturating_sub(keep);
    file.seek(SeekFrom::Start(offset.saturating_sub(1)))?;
    let mut recent = Vec::new();
    file.read_to_end(&mut recent)?;
    drop(file);
    let start = if offset == 0 {
        0
    } else {
        recent
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(recent.len(), |i| i + 1)
    };
    let partial = path.with_extension("log.partial");
    let mut compacted = File::create(&partial)?;
    compacted.write_all(&recent[start..])?;
    compacted.write_all(line.as_bytes())?;
    drop(compacted);
    fs::rename(partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_keeps_recent_complete_records_within_the_byte_limit() {
        let home = ignite_core::test_files::temp_dir().join("daemon-log-retention");
        fs::create_dir_all(&home).unwrap();
        let path = path(&home);
        let oversized = "é".repeat(MAX_RECORD_BYTES);
        let records = MAX_BYTES as usize / MAX_RECORD_BYTES + 4;
        for i in 0..records {
            append(
                &home,
                Timestamp::EPOCH,
                "ignite-main",
                format_args!("round {i}: {oversized}"),
            )
            .unwrap();
            assert!(fs::metadata(&path).unwrap().len() <= MAX_BYTES);
        }

        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("round 0:"));
        assert!(text.contains(&format!("round {}:", records - 1)));
        assert!(
            text.lines()
                .all(|line| line.starts_with("1970-01-01T00:00:00.000Z [ignite-main]"))
        );
    }
}
