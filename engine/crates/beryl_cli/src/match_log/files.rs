//! The run directory's crash-safe files: append-only JSONL, each line flushed and fsynced
//! before the next is written, whose torn last line (a power cut mid-write) is dropped when
//! read; files rewritten whole through a temporary file and a rename; and the lock that
//! keeps one process per run.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// A JSONL file open for appending. It is not opened in append mode, since Windows then
/// refuses to truncate it: each write goes to the end because this is the file's one writer
/// ([`lock`]) and its position stays there.
pub struct Log {
    file: File,
}

impl Log {
    /// Opens `path` for appending, creating it if missing. A torn last line is cut off, as
    /// [`read`] drops it, so the next line starts on a line of its own.
    pub fn open(path: &Path) -> Result<Log, String> {
        let at = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut file = open_rw(path).map_err(at)?;
        cut_torn_line(&mut file)
            .and_then(|()| file.seek(SeekFrom::End(0)))
            .map_err(at)?;
        Ok(Log { file })
    }

    /// Empties the log.
    pub fn clear(&mut self) -> Result<(), String> {
        self.file
            .set_len(0)
            .and_then(|()| self.file.sync_data())
            .and_then(|()| self.file.seek(SeekFrom::Start(0)))
            .map(drop)
            .map_err(|e| format!("emptying a log: {e}"))
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

/// Truncates `file` after its last newline, reading back from its end a block at a time.
fn cut_torn_line(file: &mut File) -> std::io::Result<()> {
    let len = file.metadata()?.len();
    let mut block = [0u8; 8192];
    let mut end = len;
    let keep = loop {
        if end == 0 {
            break 0;
        }
        let start = end.saturating_sub(block.len() as u64);
        let n = (end - start) as usize;
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut block[..n])?;
        if let Some(i) = block[..n].iter().rposition(|&b| b == b'\n') {
            break start + i as u64 + 1;
        }
        end = start;
    };
    truncate(file, keep)
}

/// Cuts `file` to its first `len` bytes, durably, if it is longer.
fn truncate(file: &File, len: u64) -> std::io::Result<()> {
    if len < file.metadata()?.len() {
        file.set_len(len)?;
        file.sync_data()?;
    }
    Ok(())
}

/// Cuts JSONL file `path` before its last lines that `drop` says to, and before a torn last
/// line: for lines that will be written again.
pub fn cut_last<T: DeserializeOwned>(path: &Path, drop: impl Fn(&T) -> bool) -> Result<(), String> {
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(at(e)),
    };
    let mut keep = text.rfind('\n').map_or(0, |i| i + 1);
    while keep > 0 {
        let start = text[..keep - 1].rfind('\n').map_or(0, |i| i + 1);
        match serde_json::from_str::<T>(&text[start..keep - 1]) {
            Ok(line) if !drop(&line) => break,
            _ => keep = start,
        }
    }
    truncate(&open_rw(path).map_err(at)?, keep as u64).map_err(at)
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
    _file: File,
    /// The file the lock is on, where that is not `file` ([`guard`]).
    _guard: Option<File>,
}

/// On Windows a file locked by one process cannot be read by another, so there the lock is
/// taken on a file of its own beside it (`lock.guard` for `lock`), leaving the locked file's
/// text, the holder's process id, readable; elsewhere on the file itself.
fn guard(path: &Path) -> Option<std::path::PathBuf> {
    cfg!(windows).then(|| path.with_extension("guard"))
}

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
    lock_file(&dir.join("lock"))?.map_err(|holder| {
        format!(
            "{} is already being played by process {holder}",
            dir.display()
        )
    })
}

/// Takes the exclusive lock on the file at `path`, writing this process's id into it. The
/// inner error is the id of the process that holds it.
pub fn lock_file(path: &Path) -> Result<Result<Lock, String>, String> {
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = open_rw(path).map_err(at)?;
    let guard = match guard(path) {
        Some(g) => Some(open_rw(&g).map_err(at)?),
        None => None,
    };
    match guard.as_ref().unwrap_or(&file).try_lock() {
        Ok(()) => {
            file.set_len(0).map_err(at)?;
            file.seek(SeekFrom::Start(0)).map_err(at)?;
            writeln!(file, "{}", std::process::id()).map_err(at)?;
            Ok(Ok(Lock {
                _file: file,
                _guard: guard,
            }))
        }
        Err(TryLockError::WouldBlock) => {
            let mut holder = String::new();
            file.read_to_string(&mut holder).map_err(at)?;
            Ok(Err(holder.lines().next().unwrap_or_default().to_owned()))
        }
        Err(TryLockError::Error(e)) => Err(at(e)),
    }
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
    fn a_torn_last_line_is_dropped() {
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
        assert!(read::<u32>(&dir.join("missing")).unwrap().is_empty());
        // Opened again, the log continues after the last whole line.
        Log::open(&path).unwrap().append(&5).unwrap();
        assert_eq!(read::<u32>(&path).unwrap(), [1, 2, 5]);
    }

    /// Retries for a forked child that has not yet exec'd to let go of the lock's file.
    const RELEASE_RETRIES: usize = 1_000_000;

    /// A second lock names the first holder, and the lock is free again once it lets go.
    #[test]
    fn a_second_lock_names_the_first_holder() {
        let dir = scratch("lock");
        let held = lock(&dir).unwrap();
        let e = lock(&dir).err().unwrap();
        let pid = std::process::id().to_string();
        assert!(e.ends_with(&format!("process {pid}")), "{e}");
        drop(held);
        // A flock belongs to the open file description, not the Lock: a subprocess another
        // test in this binary is forking holds a copy until it execs (std marks the fd
        // CLOEXEC, which closes it at exec, not at fork), and under load that window is long.
        // So wait for it by a count of retries, not by the clock.
        let released = (0..RELEASE_RETRIES).any(|_| {
            std::thread::yield_now();
            lock(&dir).is_ok()
        });
        assert!(released, "the lock outlived its holder");
    }
}
