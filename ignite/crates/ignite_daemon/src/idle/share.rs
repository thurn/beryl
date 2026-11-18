//! File locks that let several processes use the idle state file: the idle job runs in a verb
//! process of its own, beside the daemon and any `ignite rebuild` or `ignite audit` process.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// A lock on a file beside the state file, held until this is dropped.
pub(super) struct FileLock {
    /// Held, never read: the open file keeps the lock.
    _file: File,
}

/// The file next to `state` that holds the lock named `suffix`: `idle.json.<suffix>`.
fn beside(state: &Path, suffix: &str) -> PathBuf {
    let mut name = state.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

/// Waits for the lock named `suffix` beside `state` and takes it, exclusively. `None` when the
/// lock file cannot be opened or locked: the caller goes on with the in-process guards alone,
/// as the daemon did before idle jobs had a process of their own.
pub(super) fn lock(state: &Path, suffix: &str) -> Option<FileLock> {
    let path = beside(state, suffix);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .and_then(|file| file.lock().map(|()| file));
    match file {
        Ok(file) => Some(FileLock { _file: file }),
        Err(error) => {
            eprintln!("ignite: cannot lock {}: {error}", path.display());
            None
        }
    }
}
