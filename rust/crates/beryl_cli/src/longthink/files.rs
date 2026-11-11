//! The run directory's crash-safe files: append-only JSONL, each line flushed and fsynced
//! before the next is written, whose torn last line (a power cut mid-write) is dropped when
//! read; files rewritten whole through a temporary file and a rename; the lock that keeps
//! one runner per run; and the stamp that tells when a file was replaced.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// A JSONL file open for appending.
pub struct Log {
    file: File,
}

impl Log {
    /// Opens `path` for appending, creating it if missing.
    pub fn open(path: &Path) -> Result<Log, String> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Log { file })
    }

    /// Appends `value` as one line, durably: on disk before this returns.
    pub fn append<T: Serialize>(&mut self, value: &T) -> Result<(), String> {
        self.write(value, true)
    }

    /// Appends `value` as one line, which survives the process but, unless `sync`, perhaps
    /// not a power cut: for a line cheaper to redo than to sync (a sync costs milliseconds),
    /// since a later sync covers it.
    pub fn write<T: Serialize>(&mut self, value: &T, sync: bool) -> Result<(), String> {
        let mut line = serde_json::to_string(value).map_err(|e| e.to_string())?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| if sync { self.file.sync_data() } else { Ok(()) })
            .map_err(|e| format!("appending a line: {e}"))
    }

    /// Puts every line written so far on disk.
    pub fn sync(&mut self) -> Result<(), String> {
        self.file
            .sync_data()
            .map_err(|e| format!("syncing a log: {e}"))
    }
}

/// The whole lines of JSONL file `path`, none if it is missing. A last line without its
/// newline is being written, or was torn by a stop mid-write: it is not returned.
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    let at = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(at(&e)),
    };
    let whole = text.rfind('\n').map_or(0, |i| i + 1);
    text[..whole]
        .lines()
        .enumerate()
        .map(|(i, line)| {
            serde_json::from_str(line).map_err(|e| at(&format!("line {}: {e}", i + 1)))
        })
        .collect()
}

/// Cuts a torn last line from JSONL file `path`, so the next append starts a line of its
/// own. Only the process holding the run's lock may: another may be mid-write.
pub fn repair(path: &Path) -> Result<(), String> {
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let text = match fs::read(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(at(e)),
    };
    let whole = text.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    if whole < text.len() {
        let file = OpenOptions::new().write(true).open(path).map_err(at)?;
        file.set_len(whole as u64)
            .and_then(|()| file.sync_data())
            .map_err(at)?;
    }
    Ok(())
}

/// Cuts JSONL file `path` after its first `keep` lines, if it has more: the lines of a game
/// that was not committed. A missing file is left missing.
pub fn keep_lines(path: &Path, keep: usize) -> Result<(), String> {
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(at(e)),
    };
    let mut text = Vec::new();
    file.read_to_end(&mut text).map_err(at)?;
    let mut ends = text
        .iter()
        .enumerate()
        .filter(|&(_, &b)| b == b'\n')
        .map(|(i, _)| i + 1);
    let end = match keep {
        0 => 0,
        keep => match ends.nth(keep - 1) {
            Some(end) => end,
            // No more lines than that: nothing to cut.
            None => return Ok(()),
        },
    };
    if end < text.len() {
        file.set_len(end as u64)
            .and_then(|()| file.sync_data())
            .map_err(at)?;
    }
    Ok(())
}

/// Replaces `path` with `contents` all at once: a reader sees the old file or the new one,
/// never part of either.
pub fn write_whole(path: &Path, contents: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = File::create(&tmp).map_err(at)?;
    file.write_all(contents)
        .and_then(|()| file.sync_data())
        .map_err(at)?;
    fs::rename(&tmp, path).map_err(at)?;
    // The rename is the directory's to keep: synced before anything that follows is
    // committed. Best effort, as not every file system syncs a directory.
    if let Some(dir) = path.parent() {
        let _ = File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

/// The run's lock, held while this process plays it.
pub struct Lock {
    file: File,
    /// The file the lock is on, where that is not `file` ([`GUARD`]).
    _guard: Option<File>,
}

impl Lock {
    /// Adds `note` to the lock file, under the holder's process id, for others to read
    /// ([`holder`]).
    pub fn note(&mut self, note: &str) -> Result<(), String> {
        writeln!(self.file, "{note}").map_err(|e| format!("noting the lock: {e}"))
    }
}

/// On Windows a file locked by one process cannot be read by another, so there the lock is
/// taken on a file of its own, leaving `lock`'s text readable; elsewhere on `lock` itself.
const GUARD: Option<&str> = if cfg!(windows) {
    Some("lock.guard")
} else {
    None
};

fn open_rw(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// Takes the exclusive lock on run directory `dir`, or names the process that holds it.
pub fn lock(dir: &Path) -> Result<Lock, String> {
    let path = dir.join("lock");
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = open_rw(&path).map_err(at)?;
    let guard = match GUARD {
        Some(name) => Some(open_rw(&dir.join(name)).map_err(at)?),
        None => None,
    };
    match guard.as_ref().unwrap_or(&file).try_lock() {
        Ok(()) => {
            file.set_len(0).map_err(at)?;
            file.seek(SeekFrom::Start(0)).map_err(at)?;
            writeln!(file, "{}", std::process::id()).map_err(at)?;
            Ok(Lock {
                file,
                _guard: guard,
            })
        }
        Err(TryLockError::WouldBlock) => {
            let mut holder = String::new();
            file.read_to_string(&mut holder).map_err(at)?;
            Err(format!(
                "{} is already being played by process {}",
                dir.display(),
                holder.lines().next().unwrap_or_default()
            ))
        }
        Err(TryLockError::Error(e)) => Err(at(e)),
    }
}

/// The lock file of run directory `dir` while a process holds it: its process id, then the
/// notes it added. None while no process does.
pub fn holder(dir: &Path) -> Result<Option<String>, String> {
    let path = dir.join("lock");
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(at(e)),
    };
    let guard = match GUARD {
        Some(name) => match File::open(dir.join(name)) {
            Ok(guard) => Some(guard),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(at(e)),
        },
        None => None,
    };
    match guard.as_ref().unwrap_or(&file).try_lock() {
        // Free: closing the file lets it go again.
        Ok(()) => Ok(None),
        Err(TryLockError::WouldBlock) => {
            let mut text = String::new();
            file.read_to_string(&mut text).map_err(at)?;
            Ok(Some(text))
        }
        Err(TryLockError::Error(e)) => Err(at(e)),
    }
}

/// Which file is at a path, and which version of it: a file renamed into place over
/// another, or rewritten, has a new stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// The file itself: its device and inode, or on Windows its volume and file index.
    id: (u64, u64),
    len: u64,
    mtime: Option<std::time::SystemTime>,
}

/// The stamp of the file at `path`, none if there is none.
pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    Some(Stamp {
        id: file_id(path, &m)?,
        len: m.len(),
        mtime: m.modified().ok(),
    })
}

#[cfg(unix)]
#[allow(clippy::unnecessary_wraps)] // Windows' reading can fail: one signature for both.
fn file_id(_: &Path, m: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((m.dev(), m.ino()))
}

/// Std reads no file index from metadata on Windows: it takes a handle.
#[cfg(windows)]
fn file_id(path: &Path, _: &fs::Metadata) -> Option<(u64, u64)> {
    let handle = winapi_util::Handle::from_path_any(path).ok()?;
    let info = winapi_util::file::information(&handle).ok()?;
    Some((info.volume_serial_number(), info.file_index()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("beryl-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_torn_last_line_is_dropped_and_repaired() {
        let dir = scratch("torn");
        let path = dir.join("a.jsonl");
        let mut log = Log::open(&path).unwrap();
        log.append(&1).unwrap();
        log.append(&2).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"[3, 4")
            .unwrap();
        assert_eq!(read::<u32>(&path).unwrap(), [1, 2]);
        repair(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "1\n2\n");
        Log::open(&path).unwrap().append(&5).unwrap();
        assert_eq!(read::<u32>(&path).unwrap(), [1, 2, 5]);
        assert!(read::<u32>(&dir.join("missing")).unwrap().is_empty());
    }

    #[test]
    fn keep_lines_cuts_what_follows() {
        let dir = scratch("keep");
        let path = dir.join("a.jsonl");
        fs::write(&path, "1\n2\n3\n").unwrap();
        keep_lines(&path, 5).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "1\n2\n3\n");
        keep_lines(&path, 2).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "1\n2\n");
        keep_lines(&path, 0).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        keep_lines(&dir.join("missing"), 0).unwrap();
    }

    /// Retries for a forked child that has not yet exec'd to let go of the lock's file.
    const RELEASE_RETRIES: usize = 1_000_000;

    /// A second lock names the first holder; the holder's notes read back while it holds
    /// the lock, and nothing once it lets go.
    #[test]
    fn a_second_lock_names_the_first_holder() {
        let dir = scratch("lock");
        assert_eq!(holder(&dir).unwrap(), None);
        let mut held = lock(&dir).unwrap();
        held.note("a note").unwrap();
        let e = lock(&dir).err().unwrap();
        let pid = std::process::id().to_string();
        assert!(e.ends_with(&format!("process {pid}")), "{e}");
        assert_eq!(holder(&dir).unwrap(), Some(format!("{pid}\na note\n")));
        drop(held);
        // A flock belongs to the open file description, not the Lock: a subprocess another
        // test in this binary is forking holds a copy until it execs (std marks the fd
        // CLOEXEC, which closes it at exec, not at fork), and under load that window is long.
        // So wait for it by a count of retries, not by the clock.
        let released = (0..RELEASE_RETRIES).any(|_| {
            std::thread::yield_now();
            holder(&dir).unwrap().is_none()
        });
        assert!(released, "the lock outlived its holder");
        lock(&dir).unwrap();
    }

    /// A file renamed into place over another changes the stamp, even with the same bytes.
    #[test]
    fn a_replaced_file_has_a_new_stamp() {
        let dir = scratch("stamp");
        let path = dir.join("bin");
        assert_eq!(stamp(&path), None);
        fs::write(&path, "same").unwrap();
        let first = stamp(&path).unwrap();
        assert_eq!(stamp(&path), Some(first));
        write_whole(&path, b"same").unwrap();
        assert_ne!(stamp(&path), Some(first));
    }
}
