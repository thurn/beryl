//! Starting a CLI turn exactly once, and finding it again after a daemon restart or a reboot.
//!
//! A turn is one run of the CLI program (`claude -p` or `codex exec`) for one CLI session, as a
//! supervised command (see [`crate::supervise`]), so it outlives the daemon. Starting it has two
//! phases:
//!
//! 1. [`TurnLauncher::start`] records the turn as starting, with its prompt and the messages it
//!    delivers, in one transaction (`ignite_store::local::cli_slots::record_turn_start`). A turn
//!    already recorded is refused there, so a second start of the same turn runs nothing.
//! 2. It then starts the supervisor, without the scheduler, with [`TURN_VARIABLE`] set to
//!    `<session>:<turn>` in the run's `spec.json` and `IGNITE_SESSION` set to the session. The
//!    supervisor's command is `ignite internal cli-turn` ([`cli_turn`]), run in the main
//!    checkout, which takes the session's turn lock and then replaces itself with the CLI
//!    program.
//!
//! A daemon that dies between the two phases finds no run naming the turn and starts it then;
//! one that finds the run follows it ([`recover`], [`TurnLauncher::ensure`]). [`cli_turn`] also
//! records in the session's folder the last turn it ran, and refuses to run a turn whose number
//! is not above it, so even two supervisors started for one turn run the program once.
//!
//! The session's folder in the Ignite home is `sessions/<session>`, and its `lock` is held by
//! the running turn's process for as long as it lives, as a verb process holds its lock (see
//! [`crate::verbproc`]). The CLI keeps one transcript per session, so two turns of one session
//! can never run at once whatever a daemon decides: the second waits for the lock and says so in
//! its output.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::Connection;

use ignite_core::ids::SessionId;
use ignite_core::protocol::Class;
use ignite_core::traits::{CommandSpec, Error, Result};
use ignite_store::local::cli_slots;
use ignite_store::local::rows::CliTurn;

use crate::supervise::{Run, Started, SupervisedLauncher, keep};

/// The variable in a turn's supervised command that names its session and turn, as
/// `<session>:<turn>`, so a restarted daemon finds the run of each recorded turn.
pub const TURN_VARIABLE: &str = "IGNITE_CLI_TURN";

/// The variable that names the Ignite session to every `ignite` command the CLI runs.
pub const SESSION_VARIABLE: &str = "IGNITE_SESSION";

/// The variables in which the desktop apps name their own sessions. A turn's CLI program runs
/// without them, so that its `ignite` commands are recorded under `IGNITE_SESSION` alone and
/// never under a desktop session that the daemon happened to inherit.
pub const APP_SESSION_VARIABLES: &[&str] = &["CLAUDE_CODE_HOST_SESSION_ID", "CODEX_THREAD_ID"];

/// The file in a session's folder that holds the number of the last turn [`cli_turn`] ran.
const LAST_TURN: &str = "turn";

/// What the runs in the Ignite home `home` say about turn `turn` of `session`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No run names the turn: it was recorded and never started, so it is started now.
    NotStarted,
    /// A run names it and its turn lock is held.
    Running,
    /// A run names it and wrote its result, with this exit code.
    Ended { exit: Option<i32> },
    /// A run names it, its lock is free and it wrote no result, as after a reboot.
    Interrupted,
}

/// The folder of `session` in the Ignite home `home`, which holds its turn lock.
pub fn session_folder(home: &Path, session: &SessionId) -> PathBuf {
    home.join("sessions").join(session.as_str())
}

/// Whether a turn of `session` runs now: some process holds its turn lock. A lock file that does
/// not exist is free.
pub fn turn_running(home: &Path, session: &SessionId) -> bool {
    File::open(session_folder(home, session).join("lock"))
        .is_ok_and(|file| matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock)))
}

/// The value of [`TURN_VARIABLE`] for turn `turn` of `session`.
fn turn_value(session: &SessionId, turn: u32) -> String {
    format!("{session}:{turn}")
}

/// The session and turn a value of [`TURN_VARIABLE`] names.
fn parse_turn_value(value: &str) -> Option<(SessionId, u32)> {
    let (session, turn) = value.rsplit_once(':')?;
    Some((SessionId::new(session).ok()?, turn.parse().ok()?))
}

/// The run directories under `home` whose `spec.json` names turn `turn` of `session`, oldest
/// first. A directory whose supervisor never took its lock is left out: its command never
/// started, so it says nothing about the turn.
fn runs_of(home: &Path, session: &SessionId, turn: u32) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(home.join("runs")) else {
        return Vec::new();
    };
    let wanted = turn_value(session, turn);
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|dir| dir.join("lock").exists() && names_turn(dir, &wanted))
        .collect();
    dirs.sort();
    dirs
}

/// Whether the run in `dir` was started with [`TURN_VARIABLE`] set to `wanted`.
fn names_turn(dir: &Path, wanted: &str) -> bool {
    let Ok(bytes) = fs::read(dir.join("spec.json")) else {
        return false;
    };
    // Most runs are builds and checks; a substring test skips them without parsing.
    if !bytes
        .windows(TURN_VARIABLE.len())
        .any(|window| window == TURN_VARIABLE.as_bytes())
    {
        return false;
    }
    serde_json::from_slice::<crate::supervise::SpecFile>(&bytes).is_ok_and(|spec| {
        spec.env
            .iter()
            .any(|(name, value)| name == TURN_VARIABLE && value == wanted)
    })
}

/// The run of turn `turn` of `session` under `home`, if one was started: the oldest, should
/// there be more than one.
pub fn find_run(home: &Path, session: &SessionId, turn: u32) -> Option<Run> {
    runs_of(home, session, turn).into_iter().next().map(Run::at)
}

/// Finds the run of turn `turn` of `session` among the supervised runs under `home`. A run whose
/// lock is free and that wrote no result is interrupted: its supervisor died, as in a reboot, and
/// anything still running in its command's group is ended here first, so the turn that follows
/// cannot overlap it.
pub fn recover(home: &Path, session: &SessionId, turn: u32) -> Recovery {
    let Some(run) = find_run(home, session, turn) else {
        return Recovery::NotStarted;
    };
    let dir = run.dir();
    if let Some(result) = fs::read(dir.join("result.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<crate::supervise::ResultFile>(&bytes).ok())
    {
        return Recovery::Ended { exit: result.exit };
    }
    let held = File::open(dir.join("lock"))
        .is_ok_and(|file| matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock)));
    if held {
        return Recovery::Running;
    }
    if let Err(error) = run.kill() {
        eprintln!(
            "ignite: cannot end what is left of the interrupted turn in {}: {error}",
            dir.display()
        );
    }
    Recovery::Interrupted
}

/// Starts CLI turns as supervised commands.
pub struct TurnLauncher {
    launcher: Arc<SupervisedLauncher>,
    home: PathBuf,
    /// The command that runs [`cli_turn`], to which the CLI program and its arguments are added.
    cli_turn: Vec<String>,
    /// The main checkout, where every turn runs.
    checkout: PathBuf,
}

impl TurnLauncher {
    /// A launcher whose turns run `<ignite> internal cli-turn --home <home> -- <program...>` in
    /// `checkout`, under supervisors `launcher` starts.
    pub fn new(
        launcher: Arc<SupervisedLauncher>,
        ignite: &Path,
        home: PathBuf,
        checkout: PathBuf,
    ) -> TurnLauncher {
        let cli_turn = vec![
            ignite.display().to_string(),
            "internal".to_owned(),
            "cli-turn".to_owned(),
            "--home".to_owned(),
            home.display().to_string(),
            "--".to_owned(),
        ];
        TurnLauncher::with_command(launcher, cli_turn, home, checkout)
    }

    /// A launcher whose turns run `cli_turn` followed by the CLI program and its arguments. For
    /// tests, whose `cli_turn` is the test binary.
    pub fn with_command(
        launcher: Arc<SupervisedLauncher>,
        cli_turn: Vec<String>,
        home: PathBuf,
        checkout: PathBuf,
    ) -> TurnLauncher {
        TurnLauncher {
            launcher,
            home,
            cli_turn,
            checkout,
        }
    }

    /// Records `turn` as starting, marking the messages `deliver` as carried by it, then starts
    /// its supervisor with `command`, the CLI program and its arguments. Fails, starting
    /// nothing, when the turn is recorded already. A failure to start the supervisor after the
    /// record leaves a turn that [`TurnLauncher::ensure`] starts later.
    ///
    /// The supervisor is this process's child: the caller waits for it ([`Started::wait`]) or
    /// leaves it to be found again with [`find_run`].
    pub fn start(
        &self,
        conn: &mut Connection,
        turn: &CliTurn,
        deliver: &[i64],
        command: &[String],
    ) -> Result<Started> {
        let session = SessionId::new(turn.session.as_str())
            .map_err(|e| Error::Invalid(format!("the turn's session: {e}")))?;
        let number = u32::try_from(turn.turn)
            .map_err(|_| Error::Invalid(format!("turn number {}", turn.turn)))?;
        cli_slots::record_turn_start(conn, turn, deliver).map_err(|e| {
            Error::Conflict(format!(
                "turn {number} of {session} is not recorded again: {e}"
            ))
        })?;
        self.launch(&session, number, command)
            .map_err(|e| Error::Io(format!("turn {number} of {session} did not start: {e}")))
    }

    /// Makes sure turn `turn` of `session`, recorded already, has been started once: starts it
    /// with `command` when no run names it, and otherwise says what its run shows. Returns what
    /// [`recover`] found, and the supervisor when this call started it.
    pub fn ensure(
        &self,
        session: &SessionId,
        turn: u32,
        command: &[String],
    ) -> io::Result<(Recovery, Option<Started>)> {
        match recover(&self.home, session, turn) {
            Recovery::NotStarted => {
                let started = self.launch(session, turn, command)?;
                Ok((Recovery::NotStarted, Some(started)))
            }
            found => Ok((found, None)),
        }
    }

    /// The second phase: starts the supervisor of turn `turn` of `session`, and marks the run's
    /// output to be kept as long as a judged run's (see [`keep`]), so a turn's log stays long
    /// after its slot is free.
    fn launch(&self, session: &SessionId, turn: u32, command: &[String]) -> io::Result<Started> {
        if command.is_empty() {
            return Err(io::Error::other("a turn needs a program"));
        }
        let mut args = self.cli_turn.clone();
        args.extend(command.iter().cloned());
        let spec = CommandSpec {
            args,
            cwd: self.checkout.clone(),
            env: Vec::new(),
            class: Class::Agent,
            cores: 0,
            resources: Vec::new(),
            grows: false,
            tree: None,
            ticket: None,
            // The daemon judges a hung turn by its output, not by its length.
            hang_limit_s: None,
            reserve: None,
        };
        let env = vec![
            (SESSION_VARIABLE.to_owned(), session.to_string()),
            (TURN_VARIABLE.to_owned(), turn_value(session, turn)),
        ];
        let started = self.launcher.start(&spec, env, 0.0)?;
        keep(&started.run().dir().join("output.log"))?;
        Ok(started)
    }
}

/// The hidden `ignite internal cli-turn` subcommand: takes the turn lock of the session that
/// [`TURN_VARIABLE`] names, in the Ignite home `home`, waiting while another process holds it,
/// then replaces itself with `command` (on Windows, runs it as a child in the same job and
/// waits) without the variables in [`APP_SESSION_VARIABLES`]. Returns the process's exit code
/// only if it could not run the program: 3 when the turn has run already, 1 otherwise.
pub fn cli_turn(home: &Path, command: &[String]) -> i32 {
    match prepare(home, command) {
        Ok(Prepared::Run(lock)) => {
            let Some((program, args)) = command.split_first() else {
                return 1;
            };
            let error =
                ignite_os::proc::become_program(program, args, APP_SESSION_VARIABLES, &[&lock]);
            eprintln!("ignite internal cli-turn: cannot run {program}: {error}");
            1
        }
        Ok(Prepared::AlreadyRan(message)) => {
            eprintln!("ignite internal cli-turn: {message}");
            3
        }
        Err(error) => {
            eprintln!("ignite internal cli-turn: {error}");
            1
        }
    }
}

/// What [`prepare`] found.
enum Prepared {
    /// The turn lock is held and the turn recorded as run: run the program.
    Run(File),
    /// The session's last turn is this one or a later one.
    AlreadyRan(String),
}

/// Takes the turn lock and records the turn as the session's last, unless it is not above the
/// last one.
fn prepare(home: &Path, command: &[String]) -> io::Result<Prepared> {
    if command.is_empty() {
        return Err(io::Error::other("no program to run"));
    }
    let value = std::env::var(TURN_VARIABLE)
        .map_err(|_| io::Error::other(format!("{TURN_VARIABLE} is not set")))?;
    let (session, turn) = parse_turn_value(&value)
        .ok_or_else(|| io::Error::other(format!("{TURN_VARIABLE} is {value:?}")))?;
    if session.as_str().contains(['/', '\\']) || session.as_str().starts_with('.') {
        return Err(io::Error::other(format!(
            "{session} cannot name a folder in the Ignite home"
        )));
    }
    let folder = session_folder(home, &session);
    fs::create_dir_all(&folder)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(folder.join("lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            eprintln!(
                "ignite internal cli-turn: another turn of {session} holds its lock; turn {turn} \
                 waits for it"
            );
            lock.lock()?;
        }
        Err(TryLockError::Error(error)) => return Err(error),
    }
    let last: Option<u32> = fs::read_to_string(folder.join(LAST_TURN))
        .ok()
        .and_then(|text| text.trim().parse().ok());
    if let Some(last) = last.filter(|&last| last >= turn) {
        return Ok(Prepared::AlreadyRan(format!(
            "turn {turn} of {session} is not run: turn {last} ran already"
        )));
    }
    let partial = folder.join(format!("{LAST_TURN}.partial"));
    fs::write(&partial, turn.to_string())?;
    fs::rename(partial, folder.join(LAST_TURN))?;
    Ok(Prepared::Run(lock))
}

#[cfg(test)]
#[path = "clid_start_tests.rs"]
mod clid_start_tests;
