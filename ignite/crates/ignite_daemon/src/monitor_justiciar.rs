//! The monitor's headless justiciar: when the daemon stays down after the monitor has asked for
//! a repair, `ignite monitor` starts a repair session itself, as one headless CLI turn.
//!
//! The monitor writes `justiciar-needed.json` after 15 minutes of failed kickstarts. On every run
//! after that in which the daemon still does not answer, and on a machine that runs at least one
//! app in CLI mode (see "CLI mode" in the design document), this module:
//!
//! - starts one turn of the first such app in the main checkout, on the `opus` tier at `high`
//!   effort, with `IGNITE_SESSION=cli-monitor-justiciar` and a prompt made of `ignite guide
//!   justiciar` and the evidence: the monitor's request, the newest crash record, and the last 50
//!   lines of the daemon's log and of `starts.jsonl`;
//! - runs the turn as a detached process, `ignite internal justiciar-turn`, which holds
//!   `monitor/justiciar.lock` in the Ignite home while the CLI runs and writes the CLI's output to
//!   `monitor/justiciar-<n>.log`;
//! - on a later run, when the lock is free and the daemon still does not answer, resumes the
//!   transcript the newest log names, for at most [`MAX_TURNS`] turns per incident. The incident
//!   and the transcript ID are kept in `monitor/justiciar.json`.
//!
//! Once the daemon has answered in [`ANSWERED_RUNS_TO_END`] runs in a row the incident ends and
//! `monitor/justiciar.json` is deleted; a turn still running is left to finish. A machine with no
//! app in CLI mode gets nothing from this module, as before it existed.
//!
//! As in the rest of the monitor, [`decide`] is a function of what was observed, and every
//! observation and action outside the home folder goes through [`super::Machine`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ignite_core::cli::{TurnSpec, adapter};
use ignite_core::config::CliProgram;
use ignite_core::time::Timestamp;
use serde::{Deserialize, Serialize};

use super::Machine;
use crate::update::Home;

/// Turns one incident may start, the first included.
pub const MAX_TURNS: u32 = 6;
/// Runs in a row in which the daemon answered that end an incident.
pub const ANSWERED_RUNS_TO_END: u32 = 2;
/// The Ignite session every turn runs as.
pub const SESSION: &str = "cli-monitor-justiciar";
/// The model tier and effort of every turn.
pub const TIER: &str = "opus";
pub const EFFORT: &str = "high";
/// Lines of the daemon's log and of `starts.jsonl` the prompt quotes.
pub const LOG_LINES: usize = 50;
/// The apps' own session variables, which a turn must not inherit: they would make the turn's
/// `ignite` commands speak for another session.
pub const REMOVED_VARIABLES: &[&str] = &[
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CODEX_THREAD_ID",
    "CLAUDECODE",
];

/// The app the turns use, as the machine reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JusticiarApp {
    /// The app's name, such as `claude`.
    pub name: String,
    /// `[pool.cli.<app>]`.
    pub program: CliProgram,
    /// The model ID the app takes for the `opus` tier.
    pub model: String,
    /// The main checkout, where each turn runs.
    pub checkout: PathBuf,
}

/// One turn for [`Machine::start_justiciar`] to start, detached, holding `lock`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JusticiarTurn {
    /// The CLI program and every argument.
    pub command: Vec<String>,
    pub cwd: PathBuf,
    /// Held by the turn's process while the CLI runs.
    pub lock: PathBuf,
    /// Where the CLI's output goes.
    pub log: PathBuf,
    /// The value of `IGNITE_SESSION`.
    pub session: String,
}

/// What the monitor remembers about the current incident: `monitor/justiciar.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Incident {
    /// When its first turn started; `None` while there is no incident.
    pub started_at: Option<Timestamp>,
    /// The app its turns use.
    pub app: Option<String>,
    /// Turns started so far.
    pub turns: u32,
    /// The CLI's transcript ID, from the newest turn's output.
    pub transcript: Option<String>,
    /// Runs in a row in which the daemon answered.
    pub answered_runs: u32,
    /// Whether running out of turns has been reported.
    pub exhausted_reported: bool,
}

/// What one run saw, for [`decide`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// Whether the daemon answered.
    pub answered: bool,
    /// Whether `justiciar-needed.json` exists.
    pub needed: bool,
    /// Whether a turn holds `monitor/justiciar.lock`.
    pub turn_running: bool,
    /// The first app this machine runs in CLI mode; `None` when there is none.
    pub app: Option<String>,
    /// The transcript ID the newest turn's output names, read while no turn runs.
    pub transcript: Option<String>,
}

/// What a run does about the incident.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Start turn `turn` with a new transcript.
    Start { turn: u32 },
    /// Start turn `turn` by resuming `transcript`.
    Resume { turn: u32, transcript: String },
    /// Report once that the incident has used all its turns.
    Exhausted,
    /// The daemon answers again: the incident is over.
    End,
}

/// Updates `incident` for one run and says what to do. The incident already counts a returned
/// start as done.
pub fn decide(incident: &mut Incident, seen: &Seen, now: Timestamp) -> Option<Step> {
    let active = incident.started_at.is_some();
    if seen.answered {
        if !active {
            return None;
        }
        incident.answered_runs += 1;
        if incident.answered_runs >= ANSWERED_RUNS_TO_END {
            *incident = Incident::default();
            return Some(Step::End);
        }
        return None;
    }
    incident.answered_runs = 0;
    if !seen.turn_running && seen.transcript.is_some() {
        incident.transcript.clone_from(&seen.transcript);
    }
    let app = seen.app.as_ref()?;
    if !seen.needed || seen.turn_running {
        return None;
    }
    if incident.turns >= MAX_TURNS {
        if incident.exhausted_reported {
            return None;
        }
        incident.exhausted_reported = true;
        return Some(Step::Exhausted);
    }
    if incident.app.as_ref() != Some(app) {
        // Another app cannot resume this app's transcript.
        incident.app = Some(app.clone());
        incident.transcript = None;
    }
    incident.started_at.get_or_insert(now);
    incident.turns += 1;
    let turn = incident.turns;
    Some(match &incident.transcript {
        Some(transcript) => Step::Resume {
            turn,
            transcript: transcript.clone(),
        },
        None => Step::Start { turn },
    })
}

fn folder(home: &Home) -> PathBuf {
    home.root.join("monitor")
}

/// `monitor/justiciar.lock`.
pub fn lock_path(home: &Home) -> PathBuf {
    folder(home).join("justiciar.lock")
}

/// `monitor/justiciar-<turn>.log`.
pub fn log_path(home: &Home, turn: u32) -> PathBuf {
    folder(home).join(format!("justiciar-{turn}.log"))
}

fn state_path(home: &Home) -> PathBuf {
    folder(home).join("justiciar.json")
}

fn load(home: &Home) -> Incident {
    fs::read(state_path(home))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Writes `incident`, or deletes the file when there is none, so a machine that never had an
/// incident never gets one.
fn save(home: &Home, incident: &Incident) {
    if *incident == Incident::default() {
        let _ = fs::remove_file(state_path(home));
        return;
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(incident) {
        let _ = fs::create_dir_all(folder(home));
        let _ = fs::write(state_path(home), bytes);
    }
}

/// The last `lines` lines of `text`.
fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// The newest crash record in the home's crash folder, with its path.
fn newest_crash(home: &Home) -> Option<(PathBuf, String)> {
    let mut records: Vec<PathBuf> = fs::read_dir(crate::crash::record_dir(&home.root))
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    // Each name starts with the record's time in milliseconds, all of one width.
    records.sort();
    let newest = records.pop()?;
    let text = fs::read_to_string(&newest).ok()?;
    Some((newest, text))
}

/// The facts a turn starts from.
fn evidence(home: &Home, machine: &dyn Machine) -> String {
    let request = fs::read_to_string(home.root.join("justiciar-needed.json"))
        .unwrap_or_else(|_| "(none)".to_owned());
    let crash = newest_crash(home).map_or_else(
        || "(none)".to_owned(),
        |(path, text)| format!("{}\n{text}", path.display()),
    );
    let starts = fs::read_to_string(home.root.join("starts.jsonl")).unwrap_or_default();
    format!(
        "The monitor's request (justiciar-needed.json):\n{request}\n\nThe newest crash record:\n\
         {crash}\n\nThe last {LOG_LINES} lines of the daemon's log:\n{}\n\nThe last {LOG_LINES} \
         lines of starts.jsonl in {}:\n{}",
        machine.daemon_log_tail(LOG_LINES),
        home.root.display(),
        tail(&starts, LOG_LINES),
    )
}

/// The prompt of turn `turn`.
pub fn prompt(turn: u32, resumed: bool, evidence: &str) -> String {
    if resumed {
        return format!(
            "The Ignite daemon still does not answer. This is turn {turn} of at most \
             {MAX_TURNS}. Continue the repair from what you found so far. Fresh evidence:\n\n\
             {evidence}"
        );
    }
    let guide = ignite_core::guide::topic("justiciar").map_or("", |topic| topic.text);
    format!(
        "You are this machine's justiciar, started by `ignite monitor` because the Ignite daemon \
         does not answer and kickstarting it did not help. You run headless in the main \
         checkout, with no person watching and no ticket held. Repair the daemon so that it \
         answers again. `ignite monitor` runs every two minutes: while the daemon stays down it \
         resumes you, for at most {MAX_TURNS} turns, and it stops once the daemon answers. When \
         the hub answers, record what you did with `ignite ticket create` and close that \
         ticket.\n\nThis is what `ignite guide justiciar` says:\n\n{guide}\n\nThe evidence:\n\n\
         {evidence}"
    )
}

/// The transcript ID that turn `turn`'s log names, if it has been written.
fn transcript_of(home: &Home, dialect: &str, turn: u32) -> Option<String> {
    let text = fs::read_to_string(log_path(home, turn)).ok()?;
    adapter(dialect)?.read(&text).transcript
}

/// This run's part for the justiciar, after the rest of the monitor has run: `answered` says
/// whether the daemon answered. Returns what it did, one line per action.
pub fn run(home: &Home, machine: &dyn Machine, answered: bool, now: Timestamp) -> Vec<String> {
    let mut incident = load(home);
    let lock = lock_path(home);
    let turn_running = machine.locked(&lock);
    // Asked only while the daemon is down: a run that answered needs no app.
    let app = if answered {
        None
    } else {
        machine
            .justiciar_app()
            .filter(|app| adapter(app.program.dialect(&app.name)).is_some())
    };
    let transcript = app.as_ref().and_then(|app| {
        (!turn_running && incident.turns > 0 && incident.app.as_ref() == Some(&app.name))
            .then(|| transcript_of(home, app.program.dialect(&app.name), incident.turns))
            .flatten()
    });
    let seen = Seen {
        answered,
        needed: home.root.join("justiciar-needed.json").exists(),
        turn_running,
        app: app.as_ref().map(|app| app.name.clone()),
        transcript,
    };
    let step = decide(&mut incident, &seen, now);
    let mut done = Vec::new();
    match step {
        None => {}
        Some(Step::End) => done.push(
            "the daemon answered twice in a row; the headless justiciar's incident is over"
                .to_owned(),
        ),
        Some(Step::Exhausted) => {
            let text = format!(
                "the headless justiciar used its {MAX_TURNS} turns and the daemon still does not \
                 answer"
            );
            machine.notify("Ignite needs a person", &text);
            done.push(text);
        }
        Some(Step::Start { turn }) => {
            done.push(start(home, machine, app.as_ref(), turn, None, &lock));
        }
        Some(Step::Resume { turn, transcript }) => {
            done.push(start(
                home,
                machine,
                app.as_ref(),
                turn,
                Some(transcript),
                &lock,
            ));
        }
    }
    save(home, &incident);
    done
}

fn start(
    home: &Home,
    machine: &dyn Machine,
    app: Option<&JusticiarApp>,
    turn: u32,
    transcript: Option<String>,
    lock: &Path,
) -> String {
    let Some((app, adapter)) =
        app.and_then(|app| Some((app, adapter(app.program.dialect(&app.name))?)))
    else {
        return "no CLI app to start the headless justiciar with".to_owned();
    };
    let resumed = transcript.is_some();
    let spec = TurnSpec {
        app: app.name.clone(),
        program: app.program.program.clone(),
        args: app.program.args.clone(),
        model: app.model.clone(),
        effort: Some(EFFORT.to_owned()),
        transcript,
        new_transcript: None,
        prompt: prompt(turn, resumed, &evidence(home, machine)),
    };
    let launch = JusticiarTurn {
        command: adapter.command(&spec),
        cwd: app.checkout.clone(),
        lock: lock.to_path_buf(),
        log: log_path(home, turn),
        session: SESSION.to_owned(),
    };
    let verb = if resumed { "resumed" } else { "started" };
    match fs::create_dir_all(folder(home)).and_then(|()| machine.start_justiciar(&launch)) {
        Ok(()) => format!(
            "{verb} the headless justiciar with {}, turn {turn} of {MAX_TURNS}",
            app.name
        ),
        Err(error) => format!("cannot start the headless justiciar: {error}"),
    }
}

/// `ignite internal justiciar-turn <lock> <log> <command...>`: takes `lock`, runs `command` with
/// its output in `log` and waits for it, so the lock is held exactly while the CLI runs. Refuses
/// when another process holds the lock, so two turns never run at once. Returns the exit code.
pub fn run_turn(lock: &Path, log: &Path, command: &[String]) -> i32 {
    let Some((program, args)) = command.split_first() else {
        eprintln!("ignite: justiciar-turn needs a command");
        return 1;
    };
    let file = match fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock)
    {
        Ok(file) => file,
        Err(error) => {
            eprintln!("ignite: cannot open {}: {error}", lock.display());
            return 1;
        }
    };
    match file.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            eprintln!(
                "ignite: another headless justiciar turn holds {}",
                lock.display()
            );
            return 3;
        }
        Err(fs::TryLockError::Error(error)) => {
            eprintln!("ignite: cannot lock {}: {error}", lock.display());
            return 1;
        }
    }
    let output = match fs::File::create(log) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("ignite: cannot write {}: {error}", log.display());
            return 1;
        }
    };
    let status = output.try_clone().and_then(|errors| {
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(output)
            .stderr(errors)
            .status()
    });
    match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("ignite: cannot run {program}: {error}");
            1
        }
    }
}

/// Starts `turn` as a detached `ignite internal justiciar-turn` process of this executable, in
/// a process group of its own so that the login service does not end it with the monitor.
pub fn spawn_detached(turn: &JusticiarTurn) -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let mut command = std::process::Command::new(executable);
    command
        .args(["internal", "justiciar-turn"])
        .arg(&turn.lock)
        .arg(&turn.log)
        .args(&turn.command)
        .current_dir(&turn.cwd)
        .env("IGNITE_SESSION", &turn.session)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for name in REMOVED_VARIABLES {
        command.env_remove(name);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS`.
        const DETACHED: u32 = 0x0000_0200 | 0x0000_0008;
        command.creation_flags(DETACHED);
    }
    command.spawn().map(drop)
}

/// Whether another process holds `lock`. A lock file that cannot be opened is not held.
pub fn lock_held(lock: &Path) -> bool {
    let Ok(file) = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock)
    else {
        return false;
    };
    matches!(file.try_lock(), Err(fs::TryLockError::WouldBlock))
}

/// The first configured CLI provider with a justiciar model.
pub fn first_cli_app(
    config: &ignite_core::config::Config,
    checkout: &Path,
) -> Option<JusticiarApp> {
    config.pool.cli.iter().find_map(|(name, program)| {
        Some(JusticiarApp {
            name: name.clone(),
            program: program.clone(),
            model: config.pool.model_id(name, TIER)?,
            checkout: checkout.to_path_buf(),
        })
    })
}

#[cfg(test)]
#[path = "monitor_justiciar_tests.rs"]
mod tests;
