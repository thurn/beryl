//! File locks that let several processes use the idle state file: the idle job runs in a verb
//! process of its own, beside the daemon and any `ignite rebuild` or `ignite audit` process.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ignite_core::traits::{Error, Result};

use crate::steps;

/// A lock on a file beside the state file, held until this is dropped.
pub(super) struct FileLock {
    /// Held, never read: the open file keeps the lock.
    _file: File,
}

pub(super) struct JobLock {
    _file: File,
    owner: PathBuf,
}

impl Drop for JobLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.owner);
    }
}

/// The file next to `state` that holds the lock named `suffix`: `idle.json.<suffix>`.
fn beside(state: &Path, suffix: &str) -> PathBuf {
    let mut name = state.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

fn open(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

/// Waits for the lock named `suffix` beside `state` and takes it, exclusively. `None` when the
/// lock file cannot be opened or locked: the caller goes on with the in-process guards alone,
/// as the daemon did before idle jobs had a process of their own.
pub(super) fn lock(state: &Path, suffix: &str) -> Option<FileLock> {
    let path = beside(state, suffix);
    let file = open(&path).and_then(|file| file.lock().map(|()| file));
    match file {
        Ok(file) => Some(FileLock { _file: file }),
        Err(error) => {
            eprintln!("ignite: cannot lock {}: {error}", path.display());
            None
        }
    }
}

/// Waits for the previous job to release its lock, checking for a stop even when the
/// previous job lives in another process or under an earlier daemon. The owner file is
/// for status only. The kernel lock is the authority, including after a process dies.
pub(super) fn lock_job(state: &Path) -> Result<JobLock> {
    let path = beside(state, "job-lock");
    let owner = beside(state, "job-owner");
    let file = open(&path)?;
    let activity = steps::current();
    let mut waiting = None;
    loop {
        if steps::stop_requested() {
            return Err(Error::Conflict(
                "stopped waiting for the idle job lock".to_owned(),
            ));
        }
        match file.try_lock() {
            Ok(()) => {
                let name = activity
                    .as_ref()
                    .and_then(|activity| activity.handle())
                    .map_or_else(
                        || format!("PID {}", std::process::id()),
                        |handle| handle.to_string(),
                    );
                if let Err(error) = fs::write(&owner, name) {
                    eprintln!("ignite: cannot record the idle job owner: {error}");
                }
                return Ok(JobLock { _file: file, owner });
            }
            Err(TryLockError::WouldBlock) => {
                let reason = fs::read_to_string(&owner)
                    .ok()
                    .filter(|name| !name.is_empty())
                    .map_or_else(
                        || "the idle job lock".to_owned(),
                        |name| format!("{name} to release the idle job lock"),
                    );
                if waiting.as_ref().is_none_or(|(old, _)| old != &reason) {
                    waiting = Some((reason.clone(), steps::waiting(&reason)));
                }
                let poll = Duration::from_millis(250);
                if let Some(activity) = &activity {
                    activity.wait_stop_requested(poll);
                } else {
                    std::thread::sleep(poll);
                }
            }
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}
