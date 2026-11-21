//! Tests of the CLI pool and the `ignite-cli` loop against `FakeCliApp`, `ScriptedLauncher`
//! and a `decide` that answers with the action each test chooses.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ignite_core::cli::{Action, Facts, HoldReason, Slot, SlotState, TurnEnd, TurnEvents};
use ignite_core::config::CliProgram;
use ignite_core::fakes::{FakeCliApp, FakeClock, ScriptedLauncher, ScriptedTurn};
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::client::Reply;
use ignite_core::protocol::hub::ReleaseCause;
use ignite_core::protocol::hub::Reservation;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Launcher, Result, RunResult};
use ignite_store::local::cli_slots as store;

use super::read::{read_text, wrong_model};
use super::*;
use crate::dispatchd::{RunningVerb, SessionCommand};

#[path = "clid_actions_tests.rs"]
mod actions;
#[path = "clid_paths_tests.rs"]
mod paths;
#[path = "clid_resume_tests.rs"]
mod resume;
#[path = "clid_transcript_tests.rs"]
mod transcripts;

const START: i64 = 1_800_000_000_000;
const PID: u32 = 77;

/// Turns run by a [`ScriptedLauncher`]: each start plays the next queued stream at once. A turn
/// whose number is in `keep` stays running, with its lines readable, until it is ended.
#[derive(Default)]
struct FakeTurns {
    launcher: ScriptedLauncher,
    runs: Mutex<BTreeMap<(String, u32), RunResult>>,
    keep: Mutex<BTreeSet<u32>>,
    live: Mutex<BTreeSet<(String, u32)>>,
    ended: Mutex<Vec<(String, u32)>>,
    starts: Mutex<Vec<(String, u32, CommandSpec)>>,
    fail_starts: Mutex<u32>,
    grew_at: Mutex<Option<Timestamp>>,
}

impl FakeTurns {
    fn starts(&self) -> Vec<(String, u32, CommandSpec)> {
        lock(&self.starts).clone()
    }

    fn prompts(&self) -> Vec<String> {
        self.starts()
            .iter()
            .map(|(_, _, c)| c.args.last().cloned().unwrap_or_default())
            .collect()
    }
}

impl Turns for FakeTurns {
    fn start(&self, session: &SessionId, turn: u32, command: &CommandSpec) -> Result<()> {
        let mut fail = lock(&self.fail_starts);
        if *fail > 0 {
            *fail -= 1;
            return Err(Error::Io("the supervisor did not start".to_owned()));
        }
        let key = (session.to_string(), turn);
        lock(&self.starts).push((key.0.clone(), turn, command.clone()));
        let result = self.launcher.run(command);
        if lock(&self.keep).contains(&turn) {
            lock(&self.live).insert(key.clone());
        }
        lock(&self.runs).insert(key, result);
        Ok(())
    }

    fn recover(&self, session: &SessionId, turn: u32) -> Recovery {
        let key = (session.to_string(), turn);
        if lock(&self.live).contains(&key) {
            return Recovery::Running;
        }
        match lock(&self.runs).get(&key) {
            None => Recovery::NotStarted,
            Some(run) if run.hung => Recovery::Interrupted,
            Some(run) => Recovery::Ended { exit: run.exit },
        }
    }

    fn output(&self, session: &SessionId, turn: u32) -> Option<String> {
        let runs = lock(&self.runs);
        let run = runs.get(&(session.to_string(), turn))?;
        self.launcher.output(&run.output)
    }

    fn grew_at(&self, _session: &SessionId, _turn: u32) -> Option<Timestamp> {
        *lock(&self.grew_at)
    }

    fn process(&self, _session: &SessionId, _turn: u32) -> Option<u32> {
        Some(PID)
    }

    fn end(&self, session: &SessionId, turn: u32) -> Result<()> {
        let key = (session.to_string(), turn);
        lock(&self.live).remove(&key);
        if let Some(run) = lock(&self.runs).get_mut(&key) {
            run.exit = None;
        }
        lock(&self.ended).push(key);
        Ok(())
    }
}

#[derive(Default)]
struct TestHost {
    holders: Mutex<BTreeMap<SessionId, TicketId>>,
    verbs: Mutex<BTreeMap<SessionId, Vec<RunningVerb>>>,
    quiet: Mutex<BTreeMap<SessionId, Timestamp>>,
    children: Mutex<Option<Vec<u32>>>,
    releases: Mutex<Vec<(SessionId, TicketId, String)>>,
    /// The replies `ignite wait`s were ended with.
    interrupts: Mutex<Vec<String>>,
}

impl Host for TestHost {
    fn free_ticket_trees(&self) -> u32 {
        1
    }
    fn free_cores(&self) -> f64 {
        8.0
    }
    fn admit_cores(&self) -> f64 {
        8.0
    }
    fn ticket_work_wait(&self) -> f64 {
        0.0
    }
    fn holders(&self) -> BTreeMap<SessionId, TicketId> {
        lock(&self.holders).clone()
    }
    fn command(&self, _session: &SessionId) -> Option<SessionCommand> {
        None
    }
    fn quiet_since(&self, session: &SessionId) -> Option<Timestamp> {
        lock(&self.quiet).get(session).copied()
    }
    fn running_verbs(&self, session: &SessionId) -> Vec<RunningVerb> {
        lock(&self.verbs).get(session).cloned().unwrap_or_default()
    }
    fn interrupt(&self, _session: &SessionId, reply: Reply) -> bool {
        lock(&self.interrupts).push(reply.text);
        false
    }
    fn release(
        &self,
        session: &SessionId,
        ticket: &TicketId,
        reason: &str,
        _cause: ReleaseCause,
    ) -> Result<()> {
        lock(&self.holders).remove(session);
        lock(&self.releases).push((session.clone(), ticket.clone(), reason.to_owned()));
        Ok(())
    }
    fn title(&self, _session: &SessionId) -> Option<String> {
        None
    }
    fn check_sightings(&self, _since: Timestamp) -> Vec<ignite_store::local::rows::CheckSighting> {
        Vec::new()
    }
    fn green(&self) -> Option<String> {
        None
    }
    fn running_under(&self, root: u32, _asker: Option<u32>) -> Option<Vec<u32>> {
        assert_eq!(root, PID);
        lock(&self.children).clone()
    }
}

#[derive(Default)]
struct Recorded {
    usage: Mutex<Vec<String>>,
    ends: Mutex<Vec<TurnEnd>>,
    holds: Mutex<Vec<(String, HoldReason, String)>>,
    reports: Mutex<Vec<String>>,
}

impl Effects for Recorded {
    fn usage(&self, _slot: &Slot, samples: &[ignite_core::cli::UsageSample]) {
        lock(&self.usage).extend(samples.iter().map(|s| s.window.clone()));
    }
    fn ended(&self, _slot: &Slot, end: &TurnEnd, _events: &TurnEvents) {
        lock(&self.ends).push(end.clone());
    }
    fn hold(&self, app: &str, reason: HoldReason, message: &str, _version: Option<&str>) {
        lock(&self.holds).push((app.to_owned(), reason, message.to_owned()));
    }
    fn report(&self, _slot: &Slot, report: &str) {
        lock(&self.reports).push(report.to_owned());
    }
}

struct Rig {
    daemon: CliDaemon,
    turns: Arc<FakeTurns>,
    host: Arc<TestHost>,
    clock: Arc<FakeClock>,
    effects: Arc<Recorded>,
    pool: Arc<Pool>,
    /// The actions `decide` answers with, in order; `Park` once they run out.
    actions: Arc<Mutex<VecDeque<Action>>>,
    /// What `decide` was asked.
    asked: Arc<Mutex<Vec<(TurnEnd, Facts)>>>,
}

impl Rig {
    fn new() -> Rig {
        let turns = Arc::new(FakeTurns::default());
        let host = Arc::new(TestHost::default());
        let clock = Arc::new(FakeClock::new(Timestamp::from_millis(START)));
        let effects = Arc::new(Recorded::default());
        let pool = Arc::new(Pool::new(ignite_store::local::open_in_memory().unwrap()));
        let actions = Arc::new(Mutex::new(VecDeque::new()));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (queue, log) = (actions.clone(), asked.clone());
        let rules = Rules {
            decide: Box::new(move |_slot: &Slot, end: &TurnEnd, facts: &Facts| {
                lock(&log).push((end.clone(), facts.clone()));
                lock(&queue).pop_front().unwrap_or(Action::Park {
                    report: "no action scripted".to_owned(),
                })
            }),
            prompt: Box::new(|messages: &[String]| {
                (!messages.is_empty()).then(|| messages.join("\n\n"))
            }),
        };
        let setup = CliSetup {
            checkout: PathBuf::from("/checkout"),
            apps: BTreeMap::from([(
                "fake".to_owned(),
                CliAppSetup {
                    app: Arc::new(FakeCliApp::default()),
                    program: CliProgram {
                        program: "fake-cli".to_owned(),
                        args: vec!["--yes".to_owned()],
                        dialect: None,
                    },
                    default_model: Some("default-model".to_owned()),
                },
            )]),
        };
        let daemon = CliDaemon::new(
            setup,
            pool.clone(),
            host.clone(),
            clock.clone(),
            turns.clone(),
        )
        .with_effects(effects.clone())
        .with_rules(rules);
        Rig {
            daemon,
            turns,
            host,
            clock,
            effects,
            pool,
            actions,
            asked,
        }
    }

    fn push(&self, turn: ScriptedTurn) {
        self.turns.launcher.push(turn);
    }

    fn then(&self, action: Action) {
        lock(&self.actions).push_back(action);
    }

    fn keep_running(&self, turn: u32) {
        lock(&self.turns.keep).insert(turn);
    }

    /// Begins T.1 on slot 1 with the model `m-1`, as if `ignite start` claimed it, so the
    /// session holds its ticket.
    fn begin(&self) -> SessionId {
        let session = self.try_begin().unwrap();
        self.host
            .holders
            .lock()
            .unwrap()
            .insert(session.clone(), ticket());
        session
    }

    fn try_begin(&self) -> Result<SessionId> {
        self.daemon
            .begin(&reservation("T.1", 1), "m-1", Some("high"), None, false)
    }

    fn slot(&self) -> Slot {
        self.daemon.slots("fake")[0].clone()
    }

    fn row(&self, session: &SessionId) -> CliSlot {
        self.pool
            .with_database(|conn| store::by_session(conn, session.as_str()))
            .unwrap()
            .unwrap()
    }

    fn asked(&self) -> Vec<(TurnEnd, Facts)> {
        lock(&self.asked).clone()
    }
}

fn ticket() -> TicketId {
    TicketId::new("T.1").unwrap()
}

fn reservation(ticket: &str, slot: u32) -> Reservation {
    Reservation {
        master_blocker: false,
        provider: None,
        ticket: TicketId::new(ticket).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        session: SessionId::new(format!("cli-fake-slot-{slot}")).unwrap(),
        expires_at: "2026-11-20T12:00:00.000Z".parse().unwrap(),
        code: "c0de".to_owned(),
    }
}

/// A turn that started transcript `t-1` on `m-1`, said `text`, and finished.
fn clean(text: &str) -> ScriptedTurn {
    ScriptedTurn::clean(vec![
        FakeCliApp::init_line("t-1", "m-1", "1.0"),
        FakeCliApp::text_line(text),
        FakeCliApp::result_line(0.25),
    ])
}

fn killed() -> ScriptedTurn {
    ScriptedTurn::killed(vec![FakeCliApp::init_line("t-1", "m-1", "1.0")])
}

#[test]
fn begin_starts_the_first_turn_with_the_hand_off_prompt() {
    let rig = Rig::new();
    rig.keep_running(1);
    let session = rig.begin();
    assert!(session.as_str().starts_with("cli-fake-"));
    assert_eq!(session.as_str().len(), "cli-fake-".len() + 8);
    let starts = rig.turns.starts();
    assert_eq!(starts.len(), 1);
    let (_, turn, command) = &starts[0];
    assert_eq!(*turn, 1);
    assert_eq!(
        command.args,
        [
            "fake-cli",
            "--yes",
            "--model",
            "m-1",
            "Run `ignite start T.1 --reservation c0de` and follow what it prints."
        ]
    );
    assert!(
        command
            .env
            .contains(&("IGNITE_SESSION".to_owned(), session.to_string()))
    );
    assert!(
        command
            .env
            .contains(&("IGNITE_CLI_TURN".to_owned(), format!("{session}:1")))
    );
    assert_eq!(command.cwd, PathBuf::from("/checkout"));
    assert_eq!(rig.slot().state, SlotState::Running);
    assert_eq!(rig.daemon.check(&session), SessionCheck::Running);
    // The dispatcher counts busy sessions, not slot numbers, so a placeholder can name a busy
    // slot: the ticket takes the lowest free one.
    // The same reservation again, as a restarted dispatcher that had not saved the session
    // asks, returns the session and starts no turn.
    assert_eq!(rig.try_begin().unwrap(), session);
    assert_eq!(rig.turns.starts().len(), 1);
    let second = rig
        .daemon
        .begin(&reservation("T.2", 1), "m-1", Some("high"), None, false)
        .unwrap();
    let slots = rig.daemon.slots("fake");
    assert_eq!(slots[1].index, 2);
    assert_eq!(slots[1].session.as_ref(), Some(&second));
    // A placeholder of an app not in CLI mode is refused.
    let mut other = reservation("T.3", 1);
    other.session = SessionId::new(format!("cli-{}-slot-1", "codex")).unwrap();
    assert!(rig.daemon.begin(&other, "m-1", None, None, false).is_err());
}

#[test]
fn reading_the_same_log_twice_changes_nothing() {
    let rig = Rig::new();
    rig.keep_running(1);
    rig.push(ScriptedTurn::clean(vec![
        FakeCliApp::init_line("t-9", "m-1", "2.0"),
        "not json".to_owned(),
        FakeCliApp::usage_line("five_hour", 0.5, "allowed"),
        FakeCliApp::text_line("working"),
    ]));
    let session = rig.begin();
    rig.daemon.step().unwrap();
    rig.daemon.step().unwrap();
    rig.daemon.step().unwrap();
    assert_eq!(lock(&rig.effects.usage).clone(), ["five_hour".to_owned()]);
    let turn = rig
        .pool
        .with_database(|conn| store::turns(conn, session.as_str()))
        .unwrap()
        .remove(0);
    assert_eq!(turn.lines_applied, 4);
    assert_eq!(
        (turn.transcript.as_deref(), turn.cli_version.as_deref()),
        (Some("t-9"), Some("2.0"))
    );
    assert_eq!(rig.slot().transcript.as_deref(), Some("t-9"));
    assert!(rig.asked().is_empty());
    assert_eq!(rig.slot().state, SlotState::Running);
}

#[test]
fn only_whole_lines_are_read() {
    let app = FakeCliApp::default();
    let half = format!(
        "{}\n{{\"type\": \"te",
        FakeCliApp::init_line("t-1", "m-1", "1.0")
    );
    let events = read_text(&app, &half);
    assert_eq!(events.lines, 1);
    assert_eq!(events.transcript.as_deref(), Some("t-1"));
}

#[test]
fn a_turn_on_the_wrong_model_is_ended_and_its_ticket_released() {
    let rig = Rig::new();
    rig.keep_running(1);
    rig.push(ScriptedTurn::clean(vec![FakeCliApp::init_line(
        "t-1", "m-2", "1.0",
    )]));
    let session = rig.begin();
    rig.daemon.step().unwrap();
    assert_eq!(lock(&rig.turns.ended).clone(), [(session.to_string(), 1)]);
    let releases = lock(&rig.host.releases).clone();
    assert_eq!(releases.len(), 1);
    assert!(releases[0].2.contains("m-2"));
    assert!(rig.daemon.slots("fake").is_empty());
    assert!(rig.asked().is_empty());
    assert_eq!(rig.daemon.hidden_models("fake"), ["m-1".to_owned()]);
    rig.clock.advance(HIDE_MODEL_S);
    assert!(rig.daemon.hidden_models("fake").is_empty());
    // The long-context suffix is the same model, and a turn that names none is not checked.
    assert!(!wrong_model(Some("m-1"), Some("m-1[1m]")));
    assert!(!wrong_model(Some("m-1"), None));
    assert!(wrong_model(Some("m-1"), Some("m-2")));
}

#[test]
fn a_hung_turn_is_ended_and_treated_as_interrupted() {
    let rig = Rig::new();
    rig.keep_running(1);
    rig.push(ScriptedTurn::clean(vec![FakeCliApp::init_line(
        "t-1", "m-1", "1.0",
    )]));
    *lock(&rig.turns.grew_at) = Some(Timestamp::from_millis(START));
    let session = rig.begin();
    *lock(&rig.host.children) = Some(Vec::new());
    rig.clock.advance(read::HANG_S - 1.0);
    rig.daemon.step().unwrap();
    assert!(lock(&rig.turns.ended).is_empty());

    rig.clock.advance(1.0);
    // A running verb of the session keeps the turn alive.
    let verb = RunningVerb {
        verb: "test".to_owned(),
        started: rig.clock.now(),
        step: None,
    };
    rig.host
        .verbs
        .lock()
        .unwrap()
        .insert(session.clone(), vec![verb]);
    rig.daemon.step().unwrap();
    rig.host.verbs.lock().unwrap().clear();
    // So does a live child of the CLI, and a process list that cannot be read.
    *lock(&rig.host.children) = Some(vec![4321]);
    rig.daemon.step().unwrap();
    *lock(&rig.host.children) = None;
    rig.daemon.step().unwrap();
    assert!(lock(&rig.turns.ended).is_empty());

    *lock(&rig.host.children) = Some(Vec::new());
    rig.then(Action::Park {
        report: "hung".to_owned(),
    });
    rig.daemon.step().unwrap();
    assert_eq!(lock(&rig.turns.ended).clone(), [(session.to_string(), 1)]);
    assert_eq!(rig.asked()[0].0, TurnEnd::Interrupted);
    assert_eq!(rig.asked()[0].1.interrupted_in_row, 1);
}

#[test]
fn a_cli_handoff_without_a_model_uses_the_configured_default() {
    let rig = Rig::new();
    rig.keep_running(1);
    let session = rig
        .daemon
        .begin(&reservation("T.1", 1), "", None, None, false)
        .unwrap();
    assert_eq!(rig.row(&session).model.as_deref(), Some("default-model"));
    assert_eq!(
        rig.turns.starts()[0].2.args[2..4],
        ["--model", "default-model"]
    );
}

#[test]
fn a_retry_uses_the_configured_default_when_the_saved_model_is_empty() {
    let rig = Rig::new();
    *lock(&rig.turns.fail_starts) = 1;
    let session = rig.try_begin().unwrap();
    let mut row = rig.row(&session);
    row.model = Some(String::new());
    rig.pool
        .with_database(|conn| store::put(conn, &row))
        .unwrap();

    rig.keep_running(1);
    rig.daemon.step().unwrap();

    assert_eq!(
        rig.turns.starts()[0].2.args[2..4],
        ["--model", "default-model"]
    );
}

#[test]
fn a_turn_recorded_and_never_started_is_started_once() {
    let rig = Rig::new();
    *lock(&rig.turns.fail_starts) = 1;
    // The supervisor fails to start after the turn was recorded, as when the daemon dies
    // between the two phases.
    // The session exists once its turn is recorded, so the hand-off succeeds.
    assert!(rig.try_begin().is_ok());
    let slot = rig.slot();
    assert_eq!((slot.state, slot.turn), (SlotState::Starting, 1));
    assert!(rig.turns.starts().is_empty());
    rig.keep_running(1);
    rig.daemon.step().unwrap();
    rig.daemon.step().unwrap();
    assert_eq!(rig.turns.starts().len(), 1);
    assert_eq!(rig.slot().state, SlotState::Running);
}

#[test]
fn interrupted_ends_count_until_an_ignite_command_runs() {
    let rig = Rig::new();
    rig.push(killed());
    rig.push(killed());
    rig.push(killed());
    let session = rig.begin();
    for _ in 0..3 {
        rig.then(Action::Resume {
            prompt: "Your process was interrupted.".to_owned(),
        });
    }
    rig.daemon.step().unwrap();
    rig.clock.advance(5.0);
    rig.daemon.step().unwrap();
    // An `ignite` command during the third turn starts the count again.
    rig.clock.advance(5.0);
    rig.host
        .quiet
        .lock()
        .unwrap()
        .insert(session.clone(), rig.clock.now());
    rig.clock.advance(1.0);
    rig.daemon.step().unwrap();
    let counts: Vec<u32> = rig
        .asked()
        .iter()
        .map(|(_, f)| f.interrupted_in_row)
        .collect();
    assert_eq!(counts, [1, 2, 1]);
    assert!(rig.asked().iter().all(|(e, _)| *e == TurnEnd::Interrupted));
}

#[test]
fn a_second_clean_end_knows_the_block_text_was_sent() {
    let rig = Rig::new();
    rig.push(clean("done?"));
    rig.push(clean("done."));
    rig.begin();
    rig.then(Action::Resume {
        prompt: "turn-ended block".to_owned(),
    });
    rig.then(Action::Release {
        note: "done.".to_owned(),
    });
    rig.daemon.step().unwrap();
    rig.daemon.step().unwrap();
    let asked = rig.asked();
    assert_eq!(asked[0].1.previous, ignite_core::cli::PreviousAnswer::Other);
    assert_eq!(
        asked[1].1.previous,
        ignite_core::cli::PreviousAnswer::BlockSent
    );
    assert_eq!(lock(&rig.host.releases).len(), 1);
}

#[test]
fn messages_reach_the_next_turn_and_an_urgent_one_ends_the_running_turn() {
    let rig = Rig::new();
    rig.keep_running(1);
    rig.push(clean("working"));
    let session = rig.begin();
    rig.daemon
        .message(&session, "Message about T.1: rebase first", false)
        .unwrap();
    assert!(lock(&rig.turns.ended).is_empty());
    rig.daemon
        .message(&session, "Message about T.1: stop now", true)
        .unwrap();
    assert_eq!(lock(&rig.turns.ended).clone(), [(session.to_string(), 1)]);
    // `decide` already put the first message in its prompt; the second is added after it.
    rig.then(Action::Resume {
        prompt: "Your process was interrupted.\n\nMessage about T.1: rebase first".to_owned(),
    });
    rig.keep_running(2);
    rig.daemon.step().unwrap();
    assert_eq!(rig.asked()[0].1.messages.len(), 2);
    assert_eq!(
        rig.turns.prompts()[1],
        "Your process was interrupted.\n\nMessage about T.1: rebase first\n\n\
         Message about T.1: stop now"
    );
    let waiting = rig
        .pool
        .with_database(|conn| store::undelivered(conn, session.as_str()))
        .unwrap();
    assert!(waiting.is_empty());
}

#[test]
fn the_next_ticket_of_a_lane_keeps_the_session_and_resumes_its_transcript() {
    for fresh in [false, true] {
        check_lane_transcript(fresh);
    }
}

fn check_lane_transcript(fresh: bool) {
    let rig = Rig::new();
    rig.keep_running(1);
    rig.push(clean("landed"));
    let session = rig.begin();
    let mut next = reservation("T.2", 1);
    next.session = session.clone();
    // While its turn runs, the session cannot take the next ticket.
    assert!(matches!(
        rig.daemon.begin(&next, "m-1", None, None, false),
        Err(Error::Conflict(_))
    ));
    lock(&rig.turns.live).clear();
    rig.host.holders.lock().unwrap().clear();
    rig.then(Action::Free);
    rig.daemon.step().unwrap();
    rig.keep_running(2);
    assert_eq!(
        rig.daemon.begin(&next, "m-1", None, None, fresh).unwrap(),
        session
    );
    let starts = rig.turns.starts();
    assert_eq!(starts[1].1, 2);
    let args = &starts[1].2.args;
    assert_eq!(args.iter().any(|arg| arg == "--resume"), !fresh);
    if !fresh {
        assert!(args.windows(2).any(|pair| pair == ["--resume", "t-1"]));
    }
    assert_eq!(
        args.last().unwrap(),
        "Run `ignite start T.2 --reservation c0de` and follow what it prints."
    );
    let slot = rig.slot();
    assert_eq!(slot.ticket, Some(TicketId::new("T.2").unwrap()));
    // A session that never ran a turn here is not a CLI slot.
    next.session = SessionId::new("cli-fake-0badc0de").unwrap();
    "f00d".clone_into(&mut next.code);
    assert!(matches!(
        rig.daemon.begin(&next, "m-1", None, None, false),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn a_justiciar_session_holds_no_ticket() {
    let rig = Rig::new();
    rig.push(clean("the hub is back"));
    let session = rig
        .daemon
        .begin_justiciar("fake", "You are the justiciar.", "m-1", "high")
        .unwrap();
    assert_eq!(rig.turns.prompts(), ["You are the justiciar.".to_owned()]);
    assert_eq!(rig.turns.starts()[0].2.ticket, None);
    let row = rig.row(&session);
    assert_eq!((row.ticket, row.effort.as_deref()), (None, Some("high")));
    rig.then(Action::Free);
    rig.daemon.step().unwrap();
    assert!(!rig.asked()[0].1.ticket_held);
    assert!(rig.daemon.slots("fake").is_empty());
}
