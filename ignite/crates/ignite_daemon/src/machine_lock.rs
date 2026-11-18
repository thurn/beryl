//! One daemon per machine.
//!
//! The daemon holds an exclusive lock on `daemon.lock` in the machine's Ignite home for as long
//! as it runs. The operating system frees the lock when the process ends, however it ends, and
//! no other process can free it: the file is created once and never deleted, truncated or
//! replaced, so a second daemon always locks the same file as the first. A second daemon that
//! finds the lock held exits instead of serving.
//!
//! The socket listener's own lock (`ignite_os::ipc`) is not enough for this. It is taken only
//! when the daemon starts listening, after it has opened its database and started its threads,
//! and it is named after the socket's path rather than the machine's home.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::PathBuf;

use crate::update::Home;

/// The file in the machine's Ignite home that the running daemon holds locked.
pub const LOCK_FILE: &str = "daemon.lock";

/// The running daemon's lock, held until this value is dropped or the process ends.
#[derive(Debug)]
pub struct DaemonLock {
    /// Held, never read: the open file keeps the lock.
    _file: File,
    pub path: PathBuf,
}

/// Takes the machine's daemon lock, creating the home folder and the lock file if they are
/// missing. Fails with [`io::ErrorKind::AddrInUse`], and a message saying so, while another
/// process holds it.
pub fn hold(home: &Home) -> io::Result<DaemonLock> {
    home.create()?;
    let path = home.root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(DaemonLock { _file: file, path }),
        Err(TryLockError::WouldBlock) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "another ignite daemon is running on this machine: it holds {}",
                path.display()
            ),
        )),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(test)]
#[path = "machine_lock_tests.rs"]
mod tests;
