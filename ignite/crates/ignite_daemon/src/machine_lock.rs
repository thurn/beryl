//! One daemon per machine.
//!
//! The daemon holds an exclusive lock on `daemon.lock` in the machine's Ignite home for as long
//! as it runs. The operating system frees the lock when the process ends, however it ends, and
//! no other process can free it: the file is created once and never deleted, truncated or
//! replaced, so a second daemon always locks the same file as the first. A second daemon that
//! finds the lock held exits instead of serving.
//!
//! A daemon that finds the lock held waits a little before it gives up, because the holder may
//! be on its way out:
//!
//! - On Windows a restart starts the new executable and then exits (see
//!   `crate::update::ExecRestart`), so the new process can reach the lock while the old one still
//!   holds it. The old process names itself in [`RESTART_FROM`], and the new one waits up to
//!   [`RESTART_WAIT`] for the lock instead of exiting with no daemon left running.
//! - Every other start waits [`START_GRACE`], so a client that tries the lock for a moment to
//!   see whether a daemon runs ([`daemon_starting`]) never makes a daemon fail to start.
//!
//! The socket listener's own lock (`ignite_os::ipc`) is not enough for this. It is named after
//! the socket's path rather than the machine's home, and on Windows a pipe has no lock file.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ignite_core::time::Timestamp;

use crate::update::Home;

/// The file in the machine's Ignite home that the running daemon holds locked.
pub const LOCK_FILE: &str = "daemon.lock";

/// The environment variable a restarting daemon sets for the process that replaces it: its own
/// process ID.
pub const RESTART_FROM: &str = "IGNITE_RESTART_FROM";

/// How long a daemon started by a restart waits for its predecessor to free the lock.
pub const RESTART_WAIT: Duration = Duration::from_secs(30);

/// How long any other start waits for the lock: long enough for a client probing the lock
/// ([`daemon_starting`]) or a failing daemon that is about to exit to free it.
pub const START_GRACE: Duration = Duration::from_secs(5);

/// How recent a start in `starts.jsonl` must be for a client to treat a missing socket as a
/// daemon that is starting.
pub const RECENT_START_S: f64 = 60.0;

/// The pause between two tries of a held lock.
const RETRY_PAUSE: Duration = Duration::from_millis(50);

/// The running daemon's lock, held until this value is dropped or the process ends.
#[derive(Debug)]
pub struct DaemonLock {
    /// Held, never read: the open file keeps the lock.
    _file: File,
    pub path: PathBuf,
}

/// Takes the machine's daemon lock, creating the home folder and the lock file if they are
/// missing. While another process holds it, waits [`RESTART_WAIT`] if this process was started
/// by a restart of another ([`RESTART_FROM`]), else [`START_GRACE`], then fails with
/// [`io::ErrorKind::AddrInUse`] and a message saying so.
pub fn hold(home: &Home) -> io::Result<DaemonLock> {
    let from = std::env::var(RESTART_FROM).ok();
    hold_waiting(home, start_wait(from.as_deref(), std::process::id()), || {})
}

/// How long a starting daemon waits for the lock, given the value of [`RESTART_FROM`] and its
/// own process ID. A restart on macOS replaces the process image and keeps the ID, so a value
/// naming this process is no predecessor to wait for.
pub fn start_wait(restart_from: Option<&str>, own: u32) -> Duration {
    match restart_from.and_then(|pid| pid.trim().parse::<u32>().ok()) {
        Some(pid) if pid != own => RESTART_WAIT,
        _ => START_GRACE,
    }
}

/// [`hold`], waiting up to `wait` while the lock is held. `busy` is called once, the first time
/// the lock is found held.
pub fn hold_waiting(home: &Home, wait: Duration, mut busy: impl FnMut()) -> io::Result<DaemonLock> {
    home.create()?;
    let path = home.root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))?;
    let started = Instant::now();
    let mut told = false;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(DaemonLock { _file: file, path }),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(error)) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", path.display()),
                ));
            }
        }
        if !told {
            told = true;
            busy();
        }
        if started.elapsed() >= wait {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "another ignite daemon is running on this machine: it holds {} (waited {} s)",
                    path.display(),
                    wait.as_secs()
                ),
            ));
        }
        std::thread::sleep(RETRY_PAUSE);
    }
}

/// Whether some process holds the machine's daemon lock: a daemon is running or starting. The
/// lock is tried for a moment and freed at once; a daemon starting meanwhile waits for it (see
/// [`START_GRACE`]).
pub fn held(home: &Home) -> bool {
    let Ok(file) = OpenOptions::new()
        .write(true)
        .open(home.root.join(LOCK_FILE))
    else {
        return false;
    };
    matches!(file.try_lock(), Err(TryLockError::WouldBlock))
}

/// Whether a client that found no daemon socket should wait for one: a process holds the daemon
/// lock, or a daemon start was logged in `starts.jsonl` in the last [`RECENT_START_S`] seconds.
/// A restart on Windows starts the new process before the old one exits and its pipe goes away,
/// and the new process logs its start first of all, so one of the two is true throughout.
pub fn daemon_starting(home: &Home, now: Timestamp) -> bool {
    held(home) || started_recently(home, now)
}

/// Whether `starts.jsonl` logged a start in the last [`RECENT_START_S`] seconds before `now`.
pub fn started_recently(home: &Home, now: Timestamp) -> bool {
    crate::update::start_log(home)
        .read()
        .last()
        .is_some_and(|start| now.seconds_since(start.at) <= RECENT_START_S)
}

#[cfg(test)]
#[path = "machine_lock_tests.rs"]
mod tests;
