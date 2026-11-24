//! The last build that is known to work, and the decision to fall back to it.
//!
//! A daemon that has stayed up for [`LAST_GOOD_AFTER_S`] marks its own build with `bin/last-good`
//! ([`ExecutableStore::mark_last_good`]), a link of the same kind as `bin/current`. Every start
//! of a daemon is appended to `starts.jsonl` (see `starts`). A build that starts
//! [`CRASH_STARTS`] times within [`CRASH_WINDOW_S`], not counting starts whose run stopped
//! cleanly because it was asked to, is in a crash loop: the login service keeps
//! restarting it, so it is replaced by the last-good build ([`roll_back`]), recorded as bad so
//! that no update installs that exact commit again, and described in a record in `crash/`.
//! Both the starting daemon and the monitor make this decision, so it works whichever of them
//! notices the loop first.

use std::fs;
use std::io;
use std::path::PathBuf;

use ignite_core::ids::Commit;
use ignite_core::time::Timestamp;

use super::rollback::{LOG, REJECTED};
use super::starts::StartRecord;
use super::{CRASH_WINDOW_S, ExecutableStore, Home};

/// Seconds a daemon must stay up before its build counts as the last good one.
pub const LAST_GOOD_AFTER_S: f64 = 600.0;
/// This many starts of one build within [`CRASH_WINDOW_S`] are a crash loop.
pub const CRASH_STARTS: usize = 3;

const LAST_GOOD: &str = "last-good";

/// The Windows launcher at `bin/last-good.cmd`: it runs the executable whose path is the text of
/// `bin/last-good`, beside it, with the launcher's arguments. It never changes once written, which
/// matters because `cmd` rereads a batch file line by line while running it.
#[cfg(windows)]
pub const LAST_GOOD_CMD: &str = "@for /f \"usebackq delims=\" %%E in (\"%~dp0last-good\") do @\"%%E\" %*\r\n\
                                 @exit /b %ERRORLEVEL%\r\n";

impl ExecutableStore {
    /// What runs the last-good build: `bin/last-good` (a symbolic link on Unix), or on Windows
    /// the launcher [`LAST_GOOD_CMD`].
    pub fn last_good_link(&self) -> PathBuf {
        #[cfg(unix)]
        return self.root.join(LAST_GOOD);
        #[cfg(windows)]
        return self.root.join("last-good.cmd");
    }

    /// The last-good build, if one is marked and still stored.
    pub fn last_good(&self) -> Option<Commit> {
        #[cfg(unix)]
        let path = self.root.join(LAST_GOOD);
        #[cfg(windows)]
        let path = PathBuf::from(fs::read_to_string(self.root.join(LAST_GOOD)).ok()?.trim());
        Self::version_of(&path).filter(|commit| self.has(commit))
    }

    /// Ensures the Windows launcher `bin/last-good.cmd` exists beside `bin/last-good`.
    #[cfg(windows)]
    fn ensure_last_good_launcher(&self) -> io::Result<()> {
        let launcher = self.last_good_link();
        if fs::read_to_string(&launcher).ok().as_deref() != Some(LAST_GOOD_CMD) {
            fs::write(&launcher, LAST_GOOD_CMD)?;
        }
        Ok(())
    }

    /// Marks `commit`, which must be stored, as the last good build.
    pub fn mark_last_good(&self, commit: &Commit) -> io::Result<()> {
        if !self.has(commit) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the build of {commit} is not stored"),
            ));
        }
        #[cfg(windows)]
        self.ensure_last_good_launcher()?;
        self.link(LAST_GOOD, commit)
    }

    /// What the monitor's login service entry runs: the last-good build if there is one, else
    /// the current one, so the monitor itself runs a build known to work.
    pub fn monitor_program(&self) -> PathBuf {
        if self.last_good().is_some() {
            // A machine whose `bin/last-good` was written by an older build may lack
            // `bin/last-good.cmd`; write it before registering the scheduled task.
            #[cfg(windows)]
            if self.ensure_last_good_launcher().is_err() {
                return self.current_link();
            }
            self.last_good_link()
        } else {
            self.current_link()
        }
    }

    /// Records that `commit` crash-looped, so that no update installs it again. A newer commit
    /// is not affected.
    pub fn reject(&self, commit: &Commit) {
        let _ = fs::create_dir_all(&self.root);
        self.append(REJECTED, commit.as_str());
    }
}

/// A build that keeps crashing at startup, and the build to go back to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashLoop {
    pub bad: Commit,
    pub to: Commit,
    /// How many starts of `bad` that did not stop cleanly fall in the window.
    pub starts: usize,
}

/// Whether `bad`, the build the login service starts, crash-looped at `now` according to the
/// recorded `starts` whose runs did not stop cleanly, and the last-good build to go back to. There is nothing to go back to
/// when no build is marked or the marked one is `bad` itself.
pub fn detect_crash_loop(
    starts: &[StartRecord],
    bad: &Commit,
    last_good: Option<Commit>,
    now: Timestamp,
) -> Option<CrashLoop> {
    let to = last_good.filter(|good| good != bad)?;
    let count = starts
        .iter()
        .filter(|start| start.commit == bad.as_str() && start.counts())
        .filter(|start| {
            let age = (now.millis() - start.at.millis()) as f64 / 1000.0;
            (0.0..CRASH_WINDOW_S).contains(&age)
        })
        .count();
    (count >= CRASH_STARTS).then(|| CrashLoop {
        bad: bad.clone(),
        to,
        starts: count,
    })
}

/// Points `bin/current` at the build to go back to, records the bad build and writes the
/// crash record. The caller restarts the daemon.
pub fn roll_back(home: &Home, looped: &CrashLoop, now: Timestamp) -> io::Result<()> {
    let store = home.store();
    if !store.has(&looped.to) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("the build of {} is not stored", looped.to),
        ));
    }
    store.flip(&looped.to)?;
    store.reject(&looped.bad);
    let message = format!(
        "crash loop: {} starts in {} minutes; rolled back to {}",
        looped.starts,
        (CRASH_WINDOW_S / 60.0) as u32,
        looped.to
    );
    store.append(LOG, &format!("{now} {} {message}", looped.bad));
    let dir = home.root.join("crash");
    fs::create_dir_all(&dir)?;
    let record = serde_json::json!({
        "at": now,
        "commit": looped.bad,
        "thread": "startup",
        "message": message,
        "location": "",
        "backtrace": "",
    });
    fs::write(
        dir.join(format!("{}-startup.json", now.millis())),
        serde_json::to_vec_pretty(&record)?,
    )
}
