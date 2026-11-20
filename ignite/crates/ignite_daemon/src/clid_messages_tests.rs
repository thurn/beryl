//! Messages reach a CLI session's next turn in order, come back after a turn that printed
//! nothing, and an urgent one ends the running turn unless a land's push has begun.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use ignite_core::cli::CliApp;
use ignite_core::fakes::{FakeCliApp, ScriptedLauncher, ScriptedTurn};
use ignite_core::protocol::Class;
use ignite_core::traits::{CommandSpec, Launcher, RunResult};
use ignite_store::local::rows::CliSlot;

use super::*;

const SESSION: &str = "cli-claude-0a1b2c3d";

fn session() -> SessionId {
    SessionId::new(SESSION).unwrap()
}

fn now() -> Timestamp {
    Timestamp::from_millis(1_800_000_000_000)
}

/// What a test's daemon did when asked: the runs it ended and the replies it ended waits with.
#[derive(Default)]
struct FakeDeliverer {
    waiting: Cell<bool>,
    pushing: Cell<bool>,
    ended: RefCell<Vec<PathBuf>>,
    replies: RefCell<Vec<String>>,
}

impl Deliverer for FakeDeliverer {
    fn interrupt(&self, _session: &SessionId, reply: Reply) -> bool {
        if self.waiting.replace(false) {
            self.replies.borrow_mut().push(reply.text);
            true
        } else {
            false
        }
    }

    fn land_pushing(&self, _session: &SessionId) -> bool {
        self.pushing.get()
    }

    fn end_run(&self, run: &Path) -> io::Result<()> {
        self.ended.borrow_mut().push(run.to_owned());
        Ok(())
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir().join(format!(
        "ignite-clid-messages-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A database with one slot of `SESSION` holding ticket T.1, between turns.
fn database() -> Connection {
    let conn = ignite_store::local::open_in_memory().unwrap();
    cli_slots::put(
        &conn,
        &CliSlot {
            app: "claude".to_owned(),
            slot: 1,
            session: Some(SESSION.to_owned()),
            ticket: Some("T.1".to_owned()),
            state: "running".to_owned(),
            turn: 0,
            transcript: None,
            model: Some("opus".to_owned()),
            effort: Some("high".to_owned()),
            since: None,
            reservation: None,
            next_prompt: None,
            wait_handle: None,
            resume_at: None,
            park_reason: None,
            interrupted_in_row: 0,
            block_sent: 0,
            backed_off: 0,
            fallback_prompt: None,
        },
    )
    .unwrap();
    conn
}

/// Records turn `n` with `prompt` as starting and the pending messages as delivered, then marks
/// the slot running in run directory `run`, as the turn starter does once the supervisor runs.
fn record(conn: &mut Connection, n: u32, prompt: &str, run: Option<&Path>) {
    let ids = pending(conn, &session()).unwrap().ids;
    let turn = CliTurn {
        session: SESSION.to_owned(),
        turn: i64::from(n),
        app: "claude".to_owned(),
        slot: 1,
        ticket: Some("T.1".to_owned()),
        prompt: prompt.to_owned(),
        state: "starting".to_owned(),
        run: run.map(|r| r.display().to_string()),
        started_at: Some(now().to_string()),
        ended_at: None,
        exit: None,
        end_kind: None,
        transcript: None,
        model: None,
        cli_version: None,
        cost_usd: None,
        lines_applied: 0,
    };
    cli_slots::record_turn_start(conn, &turn, &ids).unwrap();
    conn.execute(
        "UPDATE cli_slots SET state = 'running' WHERE session = ?1",
        [SESSION],
    )
    .unwrap();
}

/// Plays `turn` as the CLI's answer to a turn with `prompt`, and returns the command it ran
/// and its result.
fn play(
    app: &FakeCliApp,
    launcher: &ScriptedLauncher,
    prompt: &str,
    turn: ScriptedTurn,
) -> (Vec<String>, RunResult) {
    launcher.push(turn);
    let args = app.command(&ignite_core::cli::TurnSpec {
        app: "fake".to_owned(),
        program: "fake-cli".to_owned(),
        args: Vec::new(),
        model: "opus".to_owned(),
        effort: None,
        transcript: Some("t-1".to_owned()),
        new_transcript: None,
        prompt: prompt.to_owned(),
    });
    let result = launcher.run(&CommandSpec {
        args: args.clone(),
        cwd: PathBuf::from("/checkout"),
        env: Vec::new(),
        class: Class::Agent,
        cores: 0,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
        reserve: None,
    });
    (args, result)
}

#[test]
fn two_messages_arrive_in_order_in_one_prompt() {
    let mut conn = database();
    let deliverer = FakeDeliverer::default();
    for text in ["Message about T.1: first", "Message about T.1: second"] {
        let delivery = message(&conn, &deliverer, &session(), text, false, now()).unwrap();
        assert_eq!(delivery, Delivery::Queued);
    }
    let waiting = pending(&conn, &session()).unwrap();
    let text = prompt(&waiting.texts).unwrap();
    assert_eq!(
        text,
        "Message about T.1: first\n\nMessage about T.1: second"
    );

    record(&mut conn, 1, &text, None);
    let app = FakeCliApp::default();
    let launcher = ScriptedLauncher::default();
    let (args, _) = play(
        &app,
        &launcher,
        &text,
        ScriptedTurn::clean(vec![FakeCliApp::result_line(0.1)]),
    );
    assert_eq!(args.last(), Some(&text));
    assert_eq!(pending(&conn, &session()).unwrap(), Pending::default());
    assert_eq!(prompt(&[]), None);
    assert!(deliverer.ended.borrow().is_empty());
}

#[test]
fn a_turn_that_printed_nothing_delivers_its_messages_again() {
    let mut conn = database();
    let deliverer = FakeDeliverer::default();
    message(&conn, &deliverer, &session(), "one", false, now()).unwrap();
    message(&conn, &deliverer, &session(), "two", false, now()).unwrap();
    let text = prompt(&pending(&conn, &session()).unwrap().texts).unwrap();
    record(&mut conn, 1, &text, None);
    assert!(pending(&conn, &session()).unwrap().texts.is_empty());

    let app = FakeCliApp::default();
    let launcher = ScriptedLauncher::default();
    let (_, result) = play(
        &app,
        &launcher,
        &text,
        ScriptedTurn {
            lines: Vec::new(),
            exit: Some(1),
            hung: false,
        },
    );
    let events = app.read(&launcher.output(&result.output).unwrap());
    assert_eq!(
        requeue_if_silent(&conn, &session(), 1, events.lines).unwrap(),
        2
    );
    assert_eq!(pending(&conn, &session()).unwrap().texts, ["one", "two"]);

    // The next turn printed a line, so the worker saw its prompt and nothing comes back.
    let text = prompt(&pending(&conn, &session()).unwrap().texts).unwrap();
    record(&mut conn, 2, &text, None);
    let (args, result) = play(
        &app,
        &launcher,
        &text,
        ScriptedTurn::clean(vec![FakeCliApp::init_line("t-1", "opus", "1.0")]),
    );
    assert_eq!(args.last().map(String::as_str), Some("one\n\ntwo"));
    let events = app.read(&launcher.output(&result.output).unwrap());
    assert_eq!(
        requeue_if_silent(&conn, &session(), 2, events.lines).unwrap(),
        0
    );
    assert!(pending(&conn, &session()).unwrap().texts.is_empty());
}

#[test]
fn now_ends_a_running_turn_and_the_next_prompt_holds_the_text() {
    let mut conn = database();
    let run = scratch("now");
    record(&mut conn, 1, "Run `ignite start T.1`", Some(&run));
    let deliverer = FakeDeliverer::default();
    let order = "Message about T.1: from local_j: stop and release";

    let delivery = message(&conn, &deliverer, &session(), order, true, now()).unwrap();
    assert_eq!(delivery, Delivery::TurnEnded(run.display().to_string()));
    assert_eq!(*deliverer.ended.borrow(), std::slice::from_ref(&run));
    // Asking again ends nothing more: the turn was already ended for the message.
    assert_eq!(
        press(&conn, &deliverer, &session()).unwrap(),
        Delivery::Queued
    );
    assert_eq!(deliverer.ended.borrow().len(), 1);

    // The ended turn printed no result, so `decide` resumes with the interrupted text and the
    // waiting messages; the cut-off mark makes the first line say why.
    let app = FakeCliApp::default();
    let launcher = ScriptedLauncher::default();
    let (_, result) = play(
        &app,
        &launcher,
        "Run `ignite start T.1`",
        ScriptedTurn::killed(vec![FakeCliApp::init_line("t-1", "opus", "1.0")]),
    );
    let events = app.read(&launcher.output(&result.output).unwrap());
    assert_eq!(
        app.classify(&events, result.exit),
        ignite_core::cli::TurnEnd::Interrupted
    );
    let waiting = pending(&conn, &session()).unwrap();
    let chosen = format!("{INTERRUPTED_TEXT}\n\n{}", waiting.texts.join("\n\n"));
    let next = resume_prompt(&chosen, Some(&run));
    assert_eq!(next, format!("{CUT_OFF_TEXT}\n\n{order}"));

    record(&mut conn, 2, &next, None);
    let (args, _) = play(
        &app,
        &launcher,
        &next,
        ScriptedTurn::clean(vec![FakeCliApp::result_line(0.1)]),
    );
    assert!(args.last().unwrap().contains(order));
    assert!(pending(&conn, &session()).unwrap().texts.is_empty());

    // A turn interrupted for any other reason keeps the interrupted text.
    let other = scratch("crashed");
    assert_eq!(resume_prompt(&chosen, Some(&other)), chosen);
}

#[test]
fn a_begun_push_delays_an_urgent_message() {
    let mut conn = database();
    let run = scratch("push");
    record(&mut conn, 1, "go", Some(&run));
    let deliverer = FakeDeliverer::default();
    deliverer.pushing.set(true);

    let delivery = message(&conn, &deliverer, &session(), "now", true, now()).unwrap();
    assert_eq!(delivery, Delivery::HeldForPush);
    assert!(deliverer.ended.borrow().is_empty());
    assert!(!was_cut_off(&run));

    // The land's push finished; the CLI loop's next pass ends the turn.
    deliverer.pushing.set(false);
    assert_eq!(
        press(&conn, &deliverer, &session()).unwrap(),
        Delivery::TurnEnded(run.display().to_string())
    );
    assert!(was_cut_off(&run));
}

#[test]
fn an_urgent_message_between_turns_waits_for_the_next_one() {
    let mut conn = database();
    let run = scratch("finished");
    record(&mut conn, 1, "go", Some(&run));
    fs::write(run.join("result.json"), b"{}").unwrap();
    let deliverer = FakeDeliverer::default();
    deliverer.waiting.set(true);

    let delivery = message(&conn, &deliverer, &session(), "now", true, now()).unwrap();
    // The turn already ended, so nothing is ended; a session in `ignite wait` is told.
    assert_eq!(delivery, Delivery::WaitEnded);
    assert!(deliverer.ended.borrow().is_empty());
    assert_eq!(
        *deliverer.replies.borrow(),
        ["T.1 has a message for you. End your turn to receive it."]
    );
    assert_eq!(pending(&conn, &session()).unwrap().texts, ["now"]);
}

#[test]
fn a_session_without_a_slot_gets_no_message() {
    let conn = database();
    let stranger = SessionId::new("cli-claude-ffffffff").unwrap();
    let deliverer = FakeDeliverer::default();
    assert!(message(&conn, &deliverer, &stranger, "hello", false, now()).is_err());
    assert!(pending(&conn, &stranger).unwrap().texts.is_empty());
}
