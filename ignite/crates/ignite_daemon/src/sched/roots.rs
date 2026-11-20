//! Ignite's processes outside the daemon's process tree, whose CPU the scheduler counts as
//! Ignite's rather than as load from outside it ([`super::Scheduler::count_as_ignite`]).
//!
//! Two kinds of Ignite process run outside the daemon's tree:
//!
//! - the hub, on the hub's machine, which its own login service starts with launchd as its
//!   parent. Nothing else records its PID, so `ignite hub` writes it to `hub/pid` in the Ignite
//!   home and holds `hub/lock` for its whole life ([`hold_hub`]). A free lock means the PID in
//!   the file names no hub, whatever process it names now.
//! - a verb process that a previous daemon started (see [`crate::verbproc`]). Verb processes
//!   outlive restarts of the daemon: when the daemon exits, launchd adopts them, so after a
//!   restart into a new release they are no longer in the new daemon's tree. A verb process
//!   records its PID in its handle's row and holds its handle's lock for its whole life, so the
//!   PID of every running handle whose lock is held names a live verb process.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignite_core::ids::Handle;
use rusqlite::Connection;

use ignite_store::local::handles as handle_store;

use super::IgniteRoots;

/// The folder in the Ignite home that holds the hub's PID and lock.
fn hub_folder(home: &Path) -> PathBuf {
    home.join("hub")
}

/// The lock the hub holds for its whole life, open and locked.
pub struct HubLock {
    /// Held, never read: the open file keeps the lock.
    _file: File,
}

/// Records this process as the hub of Ignite home `home`: takes `hub/lock` and writes this
/// process's PID to `hub/pid`. The hub keeps the returned lock until it exits. `None` when
/// another process holds the lock, as a second hub would.
pub fn hold_hub(home: &Path) -> io::Result<Option<HubLock>> {
    let dir = hub_folder(home);
    fs::create_dir_all(&dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("lock"))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(error)) => return Err(error),
    }
    fs::write(dir.join("pid"), format!("{}\n", std::process::id()))?;
    Ok(Some(HubLock { _file: file }))
}

/// The PID of the hub that serves from Ignite home `home`, or `None` when no hub holds its lock
/// there, as on every machine but the hub's.
pub fn hub_pid(home: &Path) -> Option<u32> {
    let dir = hub_folder(home);
    let lock = File::open(dir.join("lock")).ok()?;
    if !matches!(lock.try_lock_shared(), Err(TryLockError::WouldBlock)) {
        return None;
    }
    fs::read_to_string(dir.join("pid"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The PIDs of the verb processes running for Ignite home `home`, from the handles in `db`
/// that are running, record a PID and whose lock is held.
pub fn verb_pids(home: &Path, db: &Connection) -> Vec<u32> {
    handle_store::running(db)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| {
            let pid = u32::try_from(row.pid?).ok()?;
            let handle = Handle::new(row.id.as_str()).ok()?;
            crate::verbproc::alive(home, &handle).then_some(pid)
        })
        .collect()
}

/// Lists the verb processes and the hub of Ignite home `home`, reading handles from `db`, for
/// [`super::Scheduler::count_as_ignite`].
pub fn native(home: PathBuf, db: Connection) -> Arc<IgniteRoots> {
    let db = Mutex::new(db);
    Arc::new(move || {
        let mut pids = verb_pids(
            &home,
            &db.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        pids.extend(hub_pid(&home));
        pids
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scheduler finds the hub through the file it writes, and only while it holds its lock:
    /// a PID left behind by a hub that exited may name another process by now.
    #[test]
    fn the_hub_is_found_while_it_holds_its_lock() {
        let home = ignite_core::test_files::temp_dir()
            .join(format!("ignite-hub-lock-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        assert_eq!(hub_pid(&home), None);
        let lock = hold_hub(&home).unwrap().unwrap();
        assert_eq!(hub_pid(&home), Some(std::process::id()));
        assert!(
            hold_hub(&home).unwrap().is_none(),
            "a second hub does not take the lock"
        );
        drop(lock);
        assert_eq!(hub_pid(&home), None);
        let _ = fs::remove_dir_all(&home);
    }
}
