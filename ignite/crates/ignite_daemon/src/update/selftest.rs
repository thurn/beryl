//! Checking a new release before the daemon restarts into it.
//!
//! A release that fails at start is otherwise noticed only after it has crash-looped (see
//! `rollback`), and each of those starts cuts running work. `ignite internal selftest`, run by
//! the new executable itself, does what the start of a daemon would do first, against the live
//! state but without changing it:
//!
//! - parses `ignite.toml` and every check manifest in the main checkout, as `Daemon::start`
//!   does, and finds this machine's table in `ignite.toml`;
//! - copies `daemon.db` to a temporary folder and applies the new executable's migrations to
//!   the copy, so a migration that fails on this machine's data, or a database that is newer
//!   than the build, shows up here;
//! - reads every row of the CLI tables (`cli_slots`, `cli_turns`, `cli_messages`, `cli_holds`)
//!   in the migrated copy through the build's own row types, so a build that cannot decode what
//!   an older build wrote is rejected before a restart strands every running worker;
//! - prints the host protocol versions the build serves.
//!
//! It exits 0 when everything works, and otherwise 1 with the reason on standard error. The
//! daemon runs it through a launcher ([`SupervisedSelftest`], on the express cores where the
//! machine has them) before pointing `bin/current` at the build; a failure records the commit
//! as rejected and is reported to the justiciar (see `main/selfupdate.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::protocol::Class;
use ignite_core::traits::{CommandSpec, Error, Launcher, Result};

use super::{Home, tail};

/// The versions of the host protocol this build serves.
pub const HOST_PROTOCOL_VERSIONS: &[u32] = &[1, 2];

/// A selftest that takes longer than this is ended and counts as a failure.
pub const HANG_LIMIT_S: u64 = 10 * 60;

/// Runs the selftest of a stored executable.
pub trait Selftest: Send + Sync {
    /// `Ok` with what the selftest printed, or `Err` with why the build must not be used.
    fn run(&self, executable: &Path) -> Result<String>;
}

/// Runs `<executable> internal selftest` as a command of the daemon's launcher, in the main
/// checkout, so the scheduler accounts for it and a supervisor owns the process.
pub struct SupervisedSelftest {
    pub launcher: Arc<dyn Launcher>,
    pub repo: PathBuf,
}

impl Selftest for SupervisedSelftest {
    fn run(&self, executable: &Path) -> Result<String> {
        let result = self.launcher.run(&CommandSpec {
            args: vec![
                executable.display().to_string(),
                "internal".to_owned(),
                "selftest".to_owned(),
            ],
            cwd: self.repo.clone(),
            env: Vec::new(),
            class: Class::Agent,
            cores: 1,
            resources: Vec::new(),
            grows: false,
            tree: None,
            ticket: None,
            hang_limit_s: Some(HANG_LIMIT_S),
            reserve: None,
        });
        let text = std::fs::read_to_string(&result.output).unwrap_or_default();
        if result.hung {
            return Err(Error::Io(format!(
                "the selftest ran longer than {HANG_LIMIT_S} seconds and was ended:\n{}",
                tail(&text)
            )));
        }
        if result.exit != Some(0) {
            return Err(Error::Io(format!(
                "the selftest failed ({}):\n{}",
                result
                    .exit
                    .map_or_else(|| "ended by a signal".to_owned(), |c| format!("exit {c}")),
                tail(&text)
            )));
        }
        Ok(text)
    }
}

/// The hidden `ignite internal selftest` subcommand for the checkout around `cwd` (or the one
/// `ignite install` recorded). Returns the exit code.
pub fn command(cwd: &Path) -> i32 {
    let home = Home::from_env();
    let repo = crate::main::checkout(cwd, &home);
    let hostname = crate::doctor::Host::hostname(&crate::doctor::NativeHost);
    match check(&repo, &home, &hostname) {
        Ok(()) => {
            let versions: Vec<String> = HOST_PROTOCOL_VERSIONS
                .iter()
                .map(ToString::to_string)
                .collect();
            println!("host protocol versions: {}", versions.join(" "));
            println!("selftest passed");
            0
        }
        Err(reason) => {
            eprintln!("ignite selftest: {reason}");
            1
        }
    }
}

/// What a starting daemon would read, without starting one.
fn check(repo: &Path, home: &Home, hostname: &str) -> std::result::Result<(), String> {
    let config = crate::main::read_config(repo).map_err(|e| e.to_string())?;
    crate::main::read_manifest(repo, &config)?;
    if config.machine(hostname).is_none() {
        return Err(format!("ignite.toml declares no [machine.{hostname}]"));
    }
    migrate_a_copy(&home.root.join("daemon.db"))
}

/// Applies this build's migrations to a copy of the database at `live`, which is not changed.
/// `VACUUM INTO` makes a consistent copy while the daemon writes to the live file, including
/// what is still in its write-ahead log. A machine with no database yet migrates a new one.
fn migrate_a_copy(live: &Path) -> std::result::Result<(), String> {
    let folder = ScratchFolder::new().map_err(|e| format!("no temporary folder: {e}"))?;
    let copy = folder.path.join("daemon.db");
    if live.is_file() {
        let source = rusqlite::Connection::open_with_flags(
            live,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("{}: {e}", live.display()))?;
        source
            .busy_timeout(std::time::Duration::from_secs(30))
            .map_err(|e| e.to_string())?;
        source
            .execute("VACUUM INTO ?1", [copy.to_string_lossy().as_ref()])
            .map_err(|e| format!("cannot copy {}: {e}", live.display()))?;
    }
    let conn = ignite_store::local::open(&copy)
        .map_err(|e| format!("the migrations fail on a copy of {}: {e}", live.display()))?;
    ignite_store::local::cli_slots::verify_decodable(&conn)
        .map_err(|e| format!("the CLI tables of a copy of {}: {e}", live.display()))
}

/// A folder in the temporary directory that is removed when dropped.
struct ScratchFolder {
    path: PathBuf,
}

impl ScratchFolder {
    fn new() -> std::io::Result<Self> {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let path = ignite_core::test_files::temp_dir().join(format!(
            "ignite-selftest-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for ScratchFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
#[path = "selftest_tests.rs"]
mod tests;
