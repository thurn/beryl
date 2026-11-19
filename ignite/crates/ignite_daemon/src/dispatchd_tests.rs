use std::path::PathBuf;

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::MachineId;
use ignite_core::protocol::client::Request;
use ignite_core::protocol::hub::Reservation;
use ignite_core::protocol::tickets::Operation;
use ignite_core::traits::Clock;

use super::*;
use crate::pool::PoolVerb;
use crate::server::Verb;
use ignite_core::protocol::hub::WorkerState;

#[derive(Default)]
pub(super) struct FakeHost {
    pub(super) trees: Mutex<u32>,
    cores: Mutex<f64>,
    admit: Mutex<f64>,
    holders: Mutex<BTreeMap<SessionId, TicketId>>,
    pub(super) commands: Mutex<BTreeMap<SessionId, SessionCommand>>,
    pub(super) quiet: Mutex<BTreeMap<SessionId, Timestamp>>,
    interrupts: Mutex<Vec<(SessionId, String)>>,
    pub(super) releases: Mutex<Vec<(SessionId, TicketId)>>,
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Host for FakeHost {
    fn free_ticket_trees(&self) -> u32 {
        *lock(&self.trees)
    }

    fn free_cores(&self) -> f64 {
        *lock(&self.cores)
    }

    fn admit_cores(&self) -> f64 {
        *lock(&self.admit)
    }

    fn holders(&self) -> BTreeMap<SessionId, TicketId> {
        lock(&self.holders).clone()
    }

    fn command(&self, session: &SessionId) -> Option<SessionCommand> {
        lock(&self.commands).get(session).cloned()
    }

    fn quiet_since(&self, session: &SessionId) -> Option<Timestamp> {
        lock(&self.quiet).get(session).copied()
    }

    fn interrupt(&self, session: &SessionId, reply: Reply) -> bool {
        lock(&self.interrupts).push((session.clone(), reply.text));
        true
    }

    /// Releasing frees the session's tree, as the real release does.
    fn release(&self, session: &SessionId, ticket: &TicketId, _reason: &str) -> Result<()> {
        lock(&self.holders).remove(session);
        lock(&self.releases).push((session.clone(), ticket.clone()));
        Ok(())
    }
}

pub(super) struct Rig {
    pub(super) ctx: Context,
    pub(super) clock: Arc<FakeClock>,
    pub(super) hub: Arc<FakeHub>,
    pub(super) host: Arc<FakeHost>,
    pub(super) dispatcher: Arc<Dispatcher>,
}

pub(super) const START: i64 = 1_800_000_000_000;

pub(super) fn rig(workers: &[&str]) -> Rig {
    rig_on(
        workers,
        ignite_store::local::open_in_memory().unwrap(),
        Timestamp::from_millis(START),
    )
}

/// A dispatcher over `db` with its clock at `now`; a second one over the same database file is a
/// restarted daemon.
pub(super) fn rig_on(workers: &[&str], db: rusqlite::Connection, now: Timestamp) -> Rig {
    let text = r#"
        [repository]
        remote = "origin"
        staging = "staging"
        master = "master"
        trees = "trees"
        checks = "checks"
        [hub]
        url = "http://localhost:1"
        project = "test"
        [gate]
        threshold = 0.01
        cap = 10
        [machine.local]
        ticket_trees = 4
        landing_trees = 1
    "#;
    let clock = Arc::new(FakeClock::new(now));
    let hub = Arc::new(FakeHub::default());
    let ctx = Context {
        config: Config::parse("ignite.toml", text).unwrap(),
        repo: PathBuf::from("/repo"),
        machine: MachineId::new("m-1").unwrap(),
        clock: clock.clone(),
        hub: hub.clone(),
        git: Arc::new(FakeGit::default()),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    };
    let host = Arc::new(FakeHost::default());
    *lock(&host.trees) = 8;
    *lock(&host.cores) = 10.0;
    *lock(&host.admit) = 15.0;
    let pool = Pool::new(db);
    let setup = Setup {
        os: Os::Macos,
        families: Vec::new(),
    };
    let dispatcher = Arc::new(Dispatcher::new(pool, host.clone(), setup));
    let rig = Rig {
        ctx,
        clock,
        hub,
        host,
        dispatcher,
    };
    if !workers.is_empty() {
        let mut args = vec!["add"];
        args.extend_from_slice(workers);
        args.extend_from_slice(&["--app", "claude"]);
        let reply = rig.pool(&args, Some("local_dispatcher"));
        assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    }
    rig
}

pub(super) fn session(name: &str) -> SessionId {
    SessionId::new(name).unwrap()
}

pub(super) fn ticket_id(name: &str) -> TicketId {
    TicketId::new(name).unwrap()
}

pub(super) fn request(verb: &str, args: &[&str], session: Option<&str>) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        cwd: PathBuf::from("/repo"),
        session: session.map(|s| SessionId::new(s).unwrap()),
        wait_s: 0,
        pid: None,
    }
}

/// A ticket for the fake hub, from the fields that matter here.
pub(super) fn detail(
    id: &str,
    status: &str,
    holder: Option<&str>,
    labels: &[&str],
    body: &str,
) -> TicketDetail {
    serde_json::from_value(serde_json::json!({
        "id": id, "parent": null, "next_child": 1, "title": format!("Ticket {id}"),
        "body": body, "type": "task", "status": status, "priority": 2, "assignee": holder,
        "labels": labels, "defer_until": null, "close_reason": null,
        "created_at": "2027-01-01T00:00:00.000Z", "created_by": "test",
        "updated_at": "2027-01-01T00:00:00.000Z", "closed_at": null, "version": 1,
        "session": holder, "edges": [], "children": [], "ready": false, "blocked_by": [],
    }))
    .unwrap()
}

impl Rig {
    pub(super) fn pool(&self, args: &[&str], session: Option<&str>) -> Reply {
        PoolVerb::new(self.dispatcher.clone()).run(&self.ctx, &request("pool", args, session))
    }

    pub(super) fn tell(&self, args: &[&str]) -> Reply {
        TellVerb.run(&self.ctx, &request("tell", args, None))
    }

    pub(super) fn tell_as(&self, session: &str, args: &[&str]) -> Reply {
        TellVerb.run(&self.ctx, &request("tell", args, Some(session)))
    }

    pub(super) fn step(&self) -> Option<DispatchInstruction> {
        self.dispatcher.step(&self.ctx, None)
    }

    pub(super) fn ack(&self, id: u64, result: &str, reason: &str) -> Reply {
        self.dispatcher.ack(&self.ctx, id, result, reason)
    }

    pub(super) fn reserve(&self, ticket: &str, worker: &str, labels: &[&str]) {
        self.hub
            .insert_ticket(detail(ticket, "open", None, labels, ""));
        self.hub.queue_reservation(Reservation {
            ticket: ticket_id(ticket),
            machine: self.ctx.machine.clone(),
            session: session(worker),
            expires_at: self.clock.now().plus_seconds(600.0),
            code: "7f3a".to_owned(),
        });
    }

    pub(super) fn hold(&self, worker: &str, ticket: &str) {
        lock(&self.host.holders).insert(session(worker), ticket_id(ticket));
    }

    pub(super) fn finish(&self, worker: &str) {
        lock(&self.host.holders).remove(&session(worker));
    }

    pub(super) fn state_of(&self, worker: &str) -> String {
        let view = self.dispatcher.view().unwrap();
        view.into_iter()
            .find(|w| w.session == session(worker))
            .unwrap()
            .state
    }
}

pub(super) fn handoff_to(instruction: &DispatchInstruction) -> (&str, &str) {
    assert_eq!(instruction.action, DispatchAction::Handoff);
    let prompt = instruction.prompt.as_deref().unwrap();
    let command = prompt.split('`').nth(1).unwrap();
    let words: Vec<&str> = command.split_whitespace().collect();
    assert_eq!(words[..2], ["ignite", "start"]);
    assert_eq!(words[3..], ["--reservation", "7f3a"], "{command}");
    let ticket = words[2];
    (instruction.session.as_ref().unwrap().as_str(), ticket)
}

#[test]
fn four_idle_workers_take_four_of_six_tickets_and_the_rest_as_they_finish() {
    let rig = rig(&["local_w1", "local_w2", "local_w3", "local_w4"]);
    for (ticket, worker) in [
        ("T.1", "local_w1"),
        ("T.2", "local_w2"),
        ("T.3", "local_w3"),
        ("T.4", "local_w4"),
        ("T.5", "local_w1"),
        ("T.6", "local_w2"),
    ] {
        rig.reserve(ticket, worker, &["tier:opus"]);
    }
    let first: Vec<DispatchInstruction> = std::iter::from_fn(|| rig.step()).collect();
    let pairs: Vec<(&str, &str)> = first.iter().map(handoff_to).collect();
    assert_eq!(
        pairs,
        [
            ("local_w1", "T.1"),
            ("local_w2", "T.2"),
            ("local_w3", "T.3"),
            ("local_w4", "T.4")
        ]
    );
    assert!(
        first
            .iter()
            .all(|i| i.clear && i.model.as_deref() == Some("opus"))
    );
    for instruction in &first {
        assert_eq!(rig.ack(instruction.id, "ok", "").exit, Exit::Success);
    }
    assert_eq!(rig.state_of("local_w1"), "reserved");
    for (worker, ticket) in [
        ("local_w1", "T.1"),
        ("local_w2", "T.2"),
        ("local_w3", "T.3"),
        ("local_w4", "T.4"),
    ] {
        rig.hold(worker, ticket);
    }
    assert!(rig.step().is_none());
    assert_eq!(rig.state_of("local_w1"), "working");
    rig.finish("local_w1");
    rig.finish("local_w2");
    let later: Vec<DispatchInstruction> = std::iter::from_fn(|| rig.step()).collect();
    let pairs: Vec<(&str, &str)> = later.iter().map(handoff_to).collect();
    assert_eq!(pairs, [("local_w1", "T.5"), ("local_w2", "T.6")]);
}

#[test]
fn an_unacknowledged_handoff_is_given_twice_more_then_the_dispatcher_is_down() {
    let rig = rig(&["local_w1", "local_w2"]);
    rig.reserve("T.1", "local_w1", &[]);
    let id = rig.step().unwrap().id;
    assert!(rig.step().is_none(), "nothing more until two minutes pass");
    for _ in 0..2 {
        rig.clock.advance(ACK_WAIT_S);
        assert_eq!(rig.step().map(|i| i.id), Some(id));
    }
    assert!(!rig.dispatcher.is_down());
    rig.clock.advance(ACK_WAIT_S);
    rig.reserve("T.2", "local_w2", &[]);
    let asked = rig.hub.asked().len();
    assert!(rig.step().is_none());
    assert!(rig.dispatcher.is_down());
    assert_eq!(
        rig.hub.asked().len(),
        asked,
        "a down dispatcher gets no new hand-offs"
    );
    assert!(rig.pool(&["list"], None).text.contains("down"));
}

#[test]
fn nothing_is_handed_off_without_a_free_tree_or_admission() {
    let rig = rig(&["local_w1"]);
    *lock(&rig.host.trees) = 0;
    rig.reserve("T.1", "local_w1", &[]);
    assert!(rig.step().is_none());
    assert!(
        rig.hub.asked().is_empty(),
        "no tree free: the hub is not asked"
    );

    // The hub decides admission from the cores the daemon reports, and answers nothing. The
    // daemon reports the scheduler's free cores at the moment and the cores it admits against.
    let rig = super::tests::rig(&["local_w1"]);
    *lock(&rig.host.cores) = 0.5;
    assert!(rig.step().is_none());
    let asked = rig.hub.asked();
    assert_eq!(asked.len(), 1);
    assert!((asked[0].free_cores - 0.5).abs() < f64::EPSILON);
    assert_eq!(asked[0].admit_cores, Some(15.0));
    assert_eq!(asked[0].idle_workers[0].models, ["opus", "sonnet", "light"]);
    assert_eq!(rig.state_of("local_w1"), "idle");
}

#[test]
fn a_refused_clear_moves_the_handoff_and_rests_the_first_worker_a_minute() {
    let rig = rig(&["local_w1", "local_w2"]);
    rig.reserve("T.1", "local_w1", &["tier:sonnet"]);
    let first = rig.step().unwrap();
    assert_eq!(handoff_to(&first), ("local_w1", "T.1"));
    let reply = rig.ack(first.id, "failed", "clear refused: still finishing a turn");
    assert!(reply.text.contains("goes to local_w2"), "{}", reply.text);
    let moved = rig.step().unwrap();
    assert_eq!(handoff_to(&moved), ("local_w2", "T.1"));
    assert_eq!(moved.model.as_deref(), Some("sonnet"));

    rig.reserve("T.2", "local_w1", &[]);
    assert!(rig.step().is_none(), "the first worker rests");
    rig.clock.advance(REFUSED_S);
    assert_eq!(handoff_to(&rig.step().unwrap()), ("local_w1", "T.2"));
}

#[test]
fn a_model_that_reads_back_wrong_fails_the_handoff_without_a_substitute() {
    let rig = rig(&["local_w1"]);
    rig.reserve("T.1", "local_w1", &["tier:opus"]);
    let handoff = rig.step().unwrap();
    assert_eq!(handoff.model.as_deref(), Some("opus"));
    rig.ack(handoff.id, "failed", "model read back as sonnet");
    assert_eq!(rig.state_of("local_w1"), "idle");
    assert!(rig.step().is_none(), "never sent again with another model");
    rig.clock.advance(REFUSED_S);
    let again = rig.step().unwrap();
    assert_eq!(
        (handoff_to(&again), again.model.as_deref()),
        (("local_w1", "T.1"), Some("opus"))
    );
}

#[test]
fn a_handoff_waits_until_its_tickets_model_can_be_read() {
    let rig = rig(&["local_w1"]);
    rig.hub.queue_reservation(Reservation {
        ticket: ticket_id("T.1"),
        machine: rig.ctx.machine.clone(),
        session: session("local_w1"),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    });
    assert!(rig.step().is_none(), "the ticket cannot be read yet");
    assert_eq!(rig.state_of("local_w1"), "reserved");
    rig.hub
        .insert_ticket(detail("T.1", "open", None, &["tier:opus"], ""));
    let handoff = rig.step().unwrap();
    assert_eq!(
        (handoff_to(&handoff), handoff.model.as_deref()),
        (("local_w1", "T.1"), Some("opus"))
    );
}

#[test]
fn a_paused_pool_hands_off_nothing_until_resumed() {
    let rig = rig(&["local_w1"]);
    rig.reserve("T.1", "local_w1", &[]);
    assert_eq!(rig.pool(&["pause"], None).exit, Exit::Success);
    assert!(rig.step().is_none());
    assert!(rig.hub.asked().is_empty());
    assert_eq!(rig.hub.pauses()[0].machine, Some(rig.ctx.machine.clone()));
    assert!(rig.pool(&["list"], None).text.contains("paused"));
    rig.pool(&["resume"], None);
    assert_eq!(handoff_to(&rig.step().unwrap()), ("local_w1", "T.1"));

    rig.pool(&["pause", "--all"], None);
    assert_eq!(rig.hub.pauses().last().unwrap().machine, None);
}

#[test]
fn resuming_every_machine_ends_the_emergency_here_at_once() {
    use crate::emergency::{Emergencies, EmergencyVerb};
    let rig = rig(&[]);
    // The fake hub declares emergencies at the epoch.
    rig.clock.set(Timestamp::EPOCH);
    let emergencies = Arc::new(Emergencies::new(rig.clock.clone(), None));
    let declare = request("emergency", &["declare", "TJUS1"], Some("local_j"));
    let declared = EmergencyVerb(emergencies.clone()).run(&rig.ctx, &declare);
    assert_eq!(declared.exit, Exit::Success, "{}", declared.text);
    assert!(emergencies.in_force().is_some());
    let pool = PoolVerb::new(rig.dispatcher.clone()).ending(emergencies.clone());
    let resumed = pool.run(
        &rig.ctx,
        &request("pool", &["resume", "--all"], Some("local_j")),
    );
    assert!(
        resumed.text.contains("ended    emergency TJUS1"),
        "{}",
        resumed.text
    );
    assert!(emergencies.in_force().is_none());
}

#[test]
fn a_draining_daemon_asks_the_hub_for_nothing_until_it_stops() {
    let rig = rig(&["local_w1"]);
    rig.reserve("T.1", "local_w1", &[]);
    rig.dispatcher.set_draining(true);
    assert!(rig.step().is_none());
    assert!(rig.hub.asked().is_empty());
    rig.dispatcher.set_draining(false);
    assert_eq!(handoff_to(&rig.step().unwrap()), ("local_w1", "T.1"));
}

#[test]
fn a_check_answered_unknown_changes_nothing() {
    let rig = rig(&["local_w1"]);
    rig.hold("local_w1", "T.1");
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: "test x".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    assert!(rig.step().is_none());
    rig.clock.advance(SILENT_S);
    let check = rig.step().unwrap();
    assert_eq!(check.action, DispatchAction::Check);
    rig.ack(check.id, "unknown", "");
    assert_eq!(rig.state_of("local_w1"), "working");
    assert!(rig.step().is_none());
    assert!(lock(&rig.host.releases).is_empty());
}

#[test]
fn an_idle_worker_is_reminded_once_then_released_and_a_gone_one_at_once() {
    let rig = rig(&["local_w1", "local_w2"]);
    rig.hold("local_w1", "T.1");
    rig.hold("local_w2", "T.2");
    for worker in ["local_w1", "local_w2"] {
        lock(&rig.host.commands).insert(
            session(worker),
            SessionCommand {
                command: "sync".to_owned(),
                since: rig.clock.now(),
                running: false,
            },
        );
    }
    assert!(rig.step().is_none());
    rig.clock.advance(SILENT_S);
    let checks: Vec<DispatchInstruction> = std::iter::from_fn(|| rig.step()).collect();
    assert_eq!(checks.len(), 2);
    rig.ack(checks[0].id, "idle", "");
    rig.ack(checks[1].id, "gone", "");
    assert_eq!(rig.state_of("local_w2"), "down");
    let reminder = rig.step().unwrap();
    assert_eq!(
        (reminder.action, reminder.prompt.as_deref()),
        (DispatchAction::Message, Some(CONTINUE))
    );
    rig.ack(reminder.id, "ok", "");
    assert_eq!(rig.state_of("local_w1"), "stalled");
    rig.clock.advance(IDLE_AFTER_MESSAGE_S);
    let recheck = rig.step().unwrap();
    rig.ack(recheck.id, "idle", "");
    let releases = lock(&rig.host.releases).clone();
    assert_eq!(
        releases,
        [
            (session("local_w2"), ticket_id("T.2")),
            (session("local_w1"), ticket_id("T.1"))
        ]
    );
}

#[test]
fn tell_on_a_held_ticket_messages_its_session_once_and_ends_its_wait() {
    let rig = rig(&["local_w1"]);
    rig.hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), &[], ""));
    rig.hold("local_w1", "T.1");
    assert!(rig.step().is_none());
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    let reply = rig.tell(&["T.1", "Land when the gate passes."]);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    let message = rig.step().unwrap();
    assert_eq!(message.action, DispatchAction::Message);
    assert_eq!(message.session, Some(session("local_w1")));
    let text = "Message about T.1: Land when the gate passes.";
    assert_eq!(message.prompt.as_deref(), Some(text));
    // The wait's reply is tool output, so it names the message without carrying its text.
    assert_eq!(
        lock(&rig.host.interrupts).clone(),
        [(
            session("local_w1"),
            "T.1 has a message for you from the dispatcher. End your turn to receive it."
                .to_owned()
        )]
    );
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    assert!(rig.step().is_none());

    // Told through the session that holds it.
    assert_eq!(rig.tell(&["local_w1", "Again."]).exit, Exit::Success);
    assert_eq!(rig.tell(&["local_w9", "Nobody."]).exit, Exit::Usage);
}

#[test]
fn tell_names_the_sending_session_in_the_comment() {
    let rig = rig(&[]);
    rig.hub.insert_ticket(detail("T.1", "open", None, &[], ""));
    let reply = rig.tell_as("local_j1", &["T.1", "Stop building."]);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let ops = &rig.hub.batches()[0].ops;
    let [Operation::Comment { text, .. }] = ops.as_slice() else {
        panic!("{ops:?}");
    };
    assert_eq!(text, "from local_j1: Stop building.");
}

#[test]
fn tell_all_comments_on_every_ticket_in_progress_in_one_batch() {
    let rig = rig(&[]);
    rig.hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), &[], ""));
    rig.hub
        .insert_ticket(detail("T.2", "in_progress", Some("local_w2"), &[], ""));
    rig.hub.insert_ticket(detail("T.3", "open", None, &[], ""));
    assert_eq!(
        rig.tell(&["--all", "Skip oracle checks."]).exit,
        Exit::Success
    );
    let batches = rig.hub.batches();
    assert_eq!(batches.len(), 1);
    let commented: Vec<&str> = batches[0]
        .ops
        .iter()
        .filter_map(|op| match op {
            Operation::Comment { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(commented, ["T.1", "T.2"]);
}

#[test]
fn tell_on_a_ticket_nobody_holds_is_only_a_comment() {
    let rig = rig(&["local_w1"]);
    rig.hub.insert_ticket(detail("T.1", "open", None, &[], ""));
    assert!(rig.step().is_none());
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    let reply = rig.tell(&["T.1", "Use the new helper."]);
    assert!(reply.text.contains("nobody holds it"), "{}", reply.text);
    let ops = &rig.hub.batches()[0].ops;
    assert!(matches!(ops.as_slice(), [Operation::Comment { .. }]));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    assert!(rig.step().is_none());
    assert!(lock(&rig.host.interrupts).is_empty());
}

#[test]
fn the_pool_shows_the_command_each_worker_runs() {
    let rig = rig(&["local_w1"]);
    let since = rig.clock.now();
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: "wait L-5521".to_owned(),
            since,
            running: true,
        },
    );
    let reply = rig.pool(&["list"], None);
    assert!(reply.text.contains("`wait L-5521`"), "{}", reply.text);
    let view: Vec<WorkerState> = serde_json::from_value(reply.json.unwrap()).unwrap();
    assert_eq!(view[0].command.as_deref(), Some("wait L-5521"));
    assert_eq!(view[0].command_since, Some(since));
    let member = rig
        .dispatcher
        .pool()
        .member(&session("local_w1"))
        .unwrap()
        .unwrap();
    assert_eq!(member.parent, Some(session("local_dispatcher")));
}

#[test]
fn a_regression_goes_to_the_idle_worker_whose_ticket_caused_it() {
    let rig = rig(&["local_w1"]);
    rig.hold("local_w1", "T.1");
    assert!(rig.step().is_none());
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    rig.finish("local_w1");
    *lock(&rig.host.trees) = 0;
    assert!(rig.step().is_none());
    let body = "caused-by T.1 (commit 5e1b0aa)\nfirst bad 5e1b0aa, last good 77c2d14";
    rig.hub
        .insert_ticket(detail("R", "open", None, &["regression"], body));
    rig.hub.push_change("ticket", Some(ticket_id("R")));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    let message = rig.step().unwrap();
    assert_eq!(message.action, DispatchAction::Message);
    assert!(message.prompt.unwrap().contains("ignite start R"));
    assert_eq!(rig.state_of("local_w1"), "reserved");
}

#[test]
fn a_codex_dispatcher_opens_a_new_chat_after_many_instructions() {
    let rig = rig(&[]);
    rig.pool(
        &["add", "local_codex", "--app", "codex", "--dispatcher"],
        None,
    );
    rig.dispatcher
        .state()
        .given_to
        .insert(session("local_codex"), ROTATE_AFTER);
    let rotate = rig
        .dispatcher
        .step(&rig.ctx, Some(&session("local_codex")))
        .unwrap();
    assert_eq!(rotate.action, DispatchAction::Rotate);
}

#[test]
fn dispatch_prints_the_instruction_then_the_loop() {
    let rig = rig(&["local_w1"]);
    let verb = DispatchVerb::new(rig.dispatcher.clone());
    let idle = verb.run(&rig.ctx, &request("dispatch", &["--wait", "0"], None));
    let lines: Vec<&str> = idle.text.lines().collect();
    assert_eq!(
        lines,
        [
            r#"{"id":0,"do":"idle","session":null,"clear":false,"model":null,"effort":null,"prompt":null}"#,
            LOOP
        ]
    );
    rig.reserve("T.1", "local_w1", &[]);
    let handoff = verb.run(&rig.ctx, &request("dispatch", &["--wait", "0"], None));
    let id = handoff.json.unwrap()["id"].as_u64().unwrap();
    let ack = verb.run(
        &rig.ctx,
        &request("dispatch", &["ack", &id.to_string(), "ok"], None),
    );
    assert_eq!(ack.exit, Exit::Success);
    let again = verb.run(
        &rig.ctx,
        &request("dispatch", &["ack", &id.to_string(), "ok"], None),
    );
    assert_eq!(again.exit, Exit::Usage);
}

#[test]
fn a_new_dispatcher_replaces_the_old_one_and_its_workers() {
    let rig = rig(&["local_w1"]);
    let members = |rig: &Rig| -> Vec<String> {
        let mut names: Vec<String> = rig
            .dispatcher
            .pool()
            .members()
            .unwrap()
            .iter()
            .map(|m| m.session.as_str().to_owned())
            .collect();
        names.sort();
        names
    };
    // With no session named, `--dispatcher` registers the caller.
    let first = rig.pool(
        &["add", "--app", "claude", "--dispatcher"],
        Some("local_dispatcher"),
    );
    assert_eq!(first.exit, Exit::Success, "{}", first.text);
    assert!(!first.text.contains("removed"), "{}", first.text);
    assert_eq!(members(&rig), ["local_dispatcher", "local_w1"]);
    // Registering again is safe.
    rig.pool(
        &["add", "--app", "claude", "--dispatcher"],
        Some("local_dispatcher"),
    );
    assert_eq!(members(&rig), ["local_dispatcher", "local_w1"]);
    // A Codex dispatcher leaves the Claude one alone.
    rig.pool(
        &["add", "--app", "codex", "--dispatcher"],
        Some("local_codex"),
    );
    assert_eq!(
        members(&rig),
        ["local_codex", "local_dispatcher", "local_w1"]
    );
    // A new Claude dispatcher cannot drive the old one's workers, so both go.
    let second = rig.pool(
        &["add", "--app", "claude", "--dispatcher"],
        Some("local_new"),
    );
    assert!(second.text.contains("removed"), "{}", second.text);
    assert_eq!(members(&rig), ["local_codex", "local_new"]);
}

#[test]
fn a_replaced_dispatchers_worker_holding_a_ticket_is_still_checked_and_released() {
    let rig = rig(&[]);
    rig.pool(
        &["add", "--app", "claude", "--dispatcher"],
        Some("local_dispatcher"),
    );
    rig.pool(
        &["add", "local_w1", "--app", "claude"],
        Some("local_dispatcher"),
    );
    rig.hold("local_w1", "T.1");
    lock(&rig.host.commands).insert(
        session("local_w1"),
        SessionCommand {
            command: "start T.1".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    assert!(rig.step().is_none());

    // A new dispatcher removes the old one's workers from the pool. The worker still holds its
    // ticket, so the new dispatcher is asked whether it is still there.
    let replaced = rig.pool(
        &["add", "--app", "claude", "--dispatcher"],
        Some("local_new"),
    );
    assert!(replaced.text.contains("removed"), "{}", replaced.text);
    let check = rig.step().expect("the removed worker is checked");
    assert_eq!(check.action, DispatchAction::Check);
    assert_eq!(check.session, Some(session("local_w1")));
    rig.ack(check.id, "gone", "");
    assert_eq!(
        lock(&rig.host.releases).clone(),
        [(session("local_w1"), ticket_id("T.1"))]
    );
}
