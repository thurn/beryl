//! An app in CLI mode: its slots take tickets through the CLI pool with no dispatcher, the extra
//! slot takes only tickets that hold `master` back, and messages and checks for its sessions go
//! to the CLI pool instead of the instruction queue.

use ignite_core::config::{CliProgram, MachinePool, PoolMode};
use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::tell_tests::claimed;
use super::tests::{Rig, detail, lock, rig, session, ticket_id};
use super::wake::WAKE;
use super::*;
use crate::clid::{FakeCliPool, SessionCheck};

/// Puts `rig`'s Claude app in CLI mode with `slots` slots, through a fake CLI pool.
pub(super) fn cli(rig: &Rig, slots: u32) -> Arc<FakeCliPool> {
    cli_with(rig, slots, Arc::new(FakeCliPool::default()))
}

/// [`cli`] through `fake`, which a restarted rig shares with the one before it, as a restarted
/// daemon finds the slots its CLI pool wrote to the database.
pub(super) fn cli_with(rig: &Rig, slots: u32, fake: Arc<FakeCliPool>) -> Arc<FakeCliPool> {
    let mut config = rig.ctx.config.clone();
    config.pool.cli.insert(
        "claude".to_owned(),
        CliProgram {
            program: "claude".to_owned(),
            args: Vec::new(),
        },
    );
    config.machine.get_mut("local").unwrap().pool.insert(
        "claude".to_owned(),
        MachinePool {
            mode: PoolMode::Cli,
            slots,
        },
    );
    rig.dispatcher.set_cli(fake.clone(), "local", &config);
    fake
}

/// The hub reserves `ticket`, labelled `labels`, for the slot `slot`.
pub(super) fn reserve(rig: &Rig, ticket: &str, slot: &str, labels: &[&str]) {
    rig.hub
        .insert_ticket(detail(ticket, "open", None, labels, ""));
    rig.hub.queue_reservation(Reservation {
        ticket: ticket_id(ticket),
        machine: rig.ctx.machine.clone(),
        session: session(slot),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    });
}

/// The workers of each request to the hub, and whether it asked only for tickets that hold
/// `master` back.
fn asks(rig: &Rig) -> Vec<(Vec<String>, bool)> {
    rig.hub
        .asked()
        .iter()
        .map(|free| {
            let workers = free
                .idle_workers
                .iter()
                .map(|w| w.session.as_str().to_owned())
                .collect();
            (workers, free.master_only)
        })
        .collect()
}

pub(super) fn tick(rig: &Rig) {
    rig.dispatcher.tick(&rig.ctx);
}

#[test]
fn an_app_in_cli_mode_hands_off_through_the_cli_pool_without_a_dispatcher() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    reserve(&rig, "T.1", "cli-claude-slot-1", &["tier:opus"]);
    tick(&rig);

    let begun = fake.begun();
    assert_eq!(begun.len(), 1);
    let (reservation, model, effort) = &begun[0];
    assert_eq!(
        (reservation.ticket.as_str(), reservation.session.as_str()),
        ("T.1", "cli-claude-slot-1")
    );
    assert_eq!(reservation.code, "7f3a");
    assert_eq!(
        (model.as_str(), effort.as_deref()),
        ("opus", Some("medium"))
    );
    assert!(
        rig.step().is_none(),
        "a CLI hand-off never enters the instruction queue"
    );

    // The new session joins the pool, holds the reservation, and fills a slot.
    let member = rig
        .dispatcher
        .pool()
        .member(&session("cli-claude-00000001"))
        .unwrap()
        .expect("the CLI session joins the pool");
    assert!(member.single_use);
    assert_eq!(rig.state_of("cli-claude-00000001"), "reserved");
    rig.hold("cli-claude-00000001", "T.1");
    assert_eq!(rig.state_of("cli-claude-00000001"), "working");
    let view = rig.dispatcher.view().unwrap();
    let idle: Vec<&str> = view
        .iter()
        .filter(|w| w.state == "idle")
        .map(|w| w.session.as_str())
        .collect();
    // As with a Codex dispatcher's slots, the sessions fill the lowest numbers first.
    assert_eq!(idle, ["cli-claude-slot-2", "cli-claude-slot-3"]);

    // The first request offered both slots and the extra one was asked for alone, for tickets
    // that hold master back.
    let asked = asks(&rig);
    assert_eq!(
        asked[0],
        (
            vec![
                "cli-claude-slot-1".to_owned(),
                "cli-claude-slot-2".to_owned()
            ],
            false
        )
    );
    assert!(asked.contains(&(vec!["cli-claude-slot-3".to_owned()], true)));
}

#[test]
fn codex_cli_handoffs_use_the_configured_effort_unless_the_ticket_overrides_it() {
    let fake = Arc::new(FakeCliPool::default());
    let mut rig = rig(&[]);
    rig.ctx.config.pool.default_effort = [("codex".to_owned(), "high".to_owned())].into();
    rig.ctx.config.pool.cli.insert(
        "codex".to_owned(),
        CliProgram {
            program: "codex".to_owned(),
            args: Vec::new(),
        },
    );
    rig.ctx
        .config
        .machine
        .get_mut("local")
        .unwrap()
        .pool
        .insert(
            "codex".to_owned(),
            MachinePool {
                mode: PoolMode::Cli,
                slots: 2,
            },
        );
    rig.dispatcher
        .set_cli(fake.clone(), "local", &rig.ctx.config);
    reserve(&rig, "T.1", "cli-codex-slot-1", &[]);
    reserve(&rig, "T.2", "cli-codex-slot-2", &["effort:medium"]);
    tick(&rig);

    let begun = fake.begun();
    assert_eq!(begun.len(), 2);
    let efforts: BTreeMap<_, _> = begun
        .iter()
        .map(|(reservation, _, effort)| (reservation.ticket.as_str(), effort.as_deref()))
        .collect();
    assert_eq!(
        efforts,
        [("T.1", Some("high")), ("T.2", Some("medium"))].into()
    );
}

#[test]
fn the_extra_slot_takes_only_a_ticket_that_holds_master_back() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    reserve(&rig, "T.1", "cli-claude-slot-1", &[]);
    reserve(&rig, "T.2", "cli-claude-slot-2", &[]);
    tick(&rig);
    rig.hold("cli-claude-00000001", "T.1");
    rig.hold("cli-claude-00000002", "T.2");
    let before = rig.hub.asked().len();
    tick(&rig);
    let asked = asks(&rig);
    assert_eq!(
        asked[before..],
        [(vec!["cli-claude-slot-3".to_owned()], true)],
        "with both slots busy only the extra one is offered, for master's tickets"
    );

    reserve(&rig, "R.1", "cli-claude-slot-3", &["regression"]);
    tick(&rig);
    assert_eq!(fake.begun()[2].0.session.as_str(), "cli-claude-slot-3");
    rig.hold("cli-claude-00000003", "R.1");
    let before = rig.hub.asked().len();
    tick(&rig);
    assert_eq!(rig.hub.asked().len(), before, "every slot is busy");
}

#[test]
fn a_tell_and_a_stall_check_for_a_cli_session_reach_the_cli_pool() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    reserve(&rig, "T.1", "cli-claude-slot-1", &[]);
    tick(&rig);
    let worker = "cli-claude-00000001";
    rig.hold(worker, "T.1");
    lock(&rig.host.commands).insert(
        session(worker),
        SessionCommand {
            command: "start T.1".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    tick(&rig);

    // A tell made after the claim is queued for the session's next turn.
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    rig.hub.insert_ticket(claimed(
        "T.1",
        worker,
        &[(1, "2027-01-02T00:00:00.000Z", "Use the new helper.")],
        "2027-01-01T00:00:00.000Z",
    ));
    rig.hub.push_change("ticket", Some(ticket_id("T.1")));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    let sent = fake.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].session, session(worker));
    assert_eq!(sent[0].text, "Message about T.1: Use the new helper.");
    assert!(!sent[0].urgent);

    // Silent for 30 minutes: the CLI pool answers the check, and an idle session is reminded.
    fake.set_check(&session(worker), SessionCheck::Running);
    rig.clock.advance(SILENT_S + 1.0);
    tick(&rig);
    assert_eq!(fake.sent().len(), 1, "a running session is left alone");
    fake.set_check(&session(worker), SessionCheck::Idle);
    rig.clock.advance(SILENT_S + 1.0);
    tick(&rig);
    assert_eq!(fake.sent()[1].text, CONTINUE);
    assert!(
        rig.step().is_none(),
        "no check or message entered the queue"
    );

    // Still idle 15 minutes after the reminder: the ticket is released, as for any worker.
    rig.clock.advance(IDLE_AFTER_MESSAGE_S + 1.0);
    tick(&rig);
    assert_eq!(
        *lock(&rig.host.releases),
        [(session(worker), ticket_id("T.1"))]
    );
}

#[test]
fn a_cli_session_is_woken_as_soon_as_its_verb_ends() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    reserve(&rig, "T.1", "cli-claude-slot-1", &[]);
    tick(&rig);
    let worker = session("cli-claude-00000001");
    rig.hold(worker.as_str(), "T.1");
    tick(&rig);
    lock(&rig.host.commands).insert(
        worker.clone(),
        SessionCommand {
            command: "test ignite".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    rig.dispatcher.note_turn_end(&worker, true, rig.clock.now());
    tick(&rig);
    let sent = fake.sent();
    assert_eq!(sent.len(), 1, "no two-minute wait for a CLI session");
    assert_eq!(sent[0].text, WAKE);
}

#[test]
fn pool_mode_switches_without_disturbing_a_held_ticket() {
    let rig = rig(&["local_w1", "local_w2"]);
    rig.reserve("T.1", "local_w1", &[]);
    let handoff = rig.step().expect("a desktop hand-off");
    rig.ack(handoff.id, "ok", "");
    rig.hold("local_w1", "T.1");
    let fake = cli(&rig, 1);

    // The file says desktop for codex and cli for claude, so claude already runs in CLI mode.
    let reply = rig.pool(&["mode", "claude", "desktop"], None);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("desktop mode"), "{}", reply.text);
    let reply = rig.pool(&["mode", "claude", "cli"], None);
    assert!(reply.text.contains("cli mode"), "{}", reply.text);
    let list = rig.pool(&["list"], None);
    assert!(
        list.text
            .contains("mode     claude cli (set by `ignite pool mode`)"),
        "{}",
        list.text
    );

    // The desktop worker keeps its ticket; only the CLI slots are offered.
    let before = rig.hub.asked().len();
    reserve(&rig, "T.2", "cli-claude-slot-1", &[]);
    rig.dispatcher.step(&rig.ctx, None);
    assert_eq!(
        asks(&rig)[before].0,
        ["cli-claude-slot-1"],
        "an idle desktop worker of an app in CLI mode is offered nothing"
    );
    assert_eq!(rig.state_of("local_w1"), "working");
    assert!(lock(&rig.host.releases).is_empty());
    assert_eq!(fake.begun().len(), 1);
    rig.hold("cli-claude-00000001", "T.2");

    // Back to desktop: the CLI session keeps its ticket and its checks still go to the pool.
    let reply = rig.pool(&["mode", "claude", "config"], None);
    assert!(reply.text.contains("cli mode"), "{}", reply.text);
    let reply = rig.pool(&["mode", "claude", "desktop"], None);
    assert!(reply.text.contains("desktop mode"), "{}", reply.text);
    let before = rig.hub.asked().len();
    rig.dispatcher.step(&rig.ctx, None);
    assert_eq!(asks(&rig)[before].0, ["local_w2"]);
    assert_eq!(rig.state_of("cli-claude-00000001"), "working");
    assert!(rig.view_has_no_cli_slot());
    let reply = rig.pool(&["mode", "claude", "tui"], None);
    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
}

impl Rig {
    fn view_has_no_cli_slot(&self) -> bool {
        let view = self.dispatcher.view().unwrap();
        !view.iter().any(|w| cli::is_cli_slot(&w.session))
    }
}

#[test]
fn a_dispatcher_that_is_down_does_not_stop_cli_hand_offs() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    rig.dispatcher.state().down = true;
    reserve(&rig, "T.1", "cli-claude-slot-1", &[]);
    tick(&rig);
    assert_eq!(fake.begun().len(), 1);
}

#[test]
fn a_start_that_fails_frees_the_slot_for_a_minute() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    fake.refuse(Some("the program is missing"));
    reserve(&rig, "T.1", "cli-claude-slot-1", &[]);
    tick(&rig);
    assert_eq!(rig.state_of("cli-claude-slot-1"), "idle");
    let before = rig.hub.asked().len();
    tick(&rig);
    let asked = asks(&rig);
    assert!(
        asked[before..]
            .iter()
            .all(|(workers, _)| !workers.contains(&"cli-claude-slot-1".to_owned())),
        "{asked:?}"
    );
    fake.refuse(None);
    rig.clock.advance(REFUSED_S + 1.0);
    reserve(&rig, "T.1", "cli-claude-slot-1", &[]);
    tick(&rig);
    assert_eq!(fake.begun().len(), 1);
}

#[test]
fn the_hub_down_justiciar_runs_as_a_cli_session() {
    let rig = rig(&["local_w1"]);
    let fake = cli(&rig, 1);
    let given = rig
        .dispatcher
        .hand_off_justiciar(&rig.ctx, "Repair the hub.")
        .expect("a CLI justiciar starts");
    let started = fake.justiciars();
    assert_eq!(started.len(), 1);
    assert_eq!(
        (
            started[0].app.as_str(),
            started[0].model.as_str(),
            started[0].effort.as_str()
        ),
        ("claude", "opus", "high")
    );
    assert_eq!(started[0].prompt, "Repair the hub.");
    assert!(cli::is_cli_session(&given));
    assert!(rig.step().is_none(), "no desktop worker is given it");
    assert!(
        rig.dispatcher
            .hand_off_justiciar(&rig.ctx, "Repair the hub.")
            .is_none(),
        "one justiciar at a time"
    );
}

/// Whether any request to the hub offered one of the CLI app's slots.
fn offered_cli_slots(rig: &Rig, from: usize) -> bool {
    asks(rig)[from..]
        .iter()
        .any(|(workers, _)| workers.iter().any(|w| w.starts_with("cli-claude-slot-")))
}

#[test]
fn an_unprobed_or_held_app_offers_no_slots() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    fake.set_probed("claude", false);
    tick(&rig);
    assert!(
        !offered_cli_slots(&rig, 0),
        "no slot before a probe passes: {:?}",
        asks(&rig)
    );

    fake.set_probed("claude", true);
    fake.set_hold(
        "claude",
        Some(ignite_core::cli::Hold {
            app: "claude".to_owned(),
            reason: ignite_core::cli::HoldReason::NotLoggedIn,
            since: rig.clock.now(),
            cli_version: Some("2.1.323".to_owned()),
            message: "Invalid API key".to_owned(),
        }),
    );
    let before = asks(&rig).len();
    tick(&rig);
    assert!(!offered_cli_slots(&rig, before), "{:?}", asks(&rig));
    assert!(fake.begun().is_empty());

    fake.set_hold("claude", None);
    let before = asks(&rig).len();
    tick(&rig);
    assert!(offered_cli_slots(&rig, before), "{:?}", asks(&rig));
}

#[test]
fn a_machine_whose_claude_runs_in_cli_mode_queues_no_usage() {
    let rig = super::tests::rig_sampling(
        &[],
        ignite_store::local::open_in_memory().unwrap(),
        Timestamp::from_millis(super::tests::START),
        true,
    );
    let reply = rig.pool(
        &["add", "local_dispatcher", "--app", "claude", "--dispatcher"],
        Some("local_dispatcher"),
    );
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    cli(&rig, 1);
    tick(&rig);
    let step = rig.step();
    assert!(
        step.as_ref()
            .is_none_or(|given| given.action != DispatchAction::Usage),
        "{step:?}"
    );
}

/// A regression reserved for a CLI slot, whose culprit's CLI session ended when it landed, asks
/// the CLI pool to resume the culprit's transcript with the regression's hand-off prompt.
#[test]
fn a_regression_for_a_cli_slot_names_its_culprit_to_the_cli_pool() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    let body = "caused-by T.1 (commit 5e1b0aa)\nfirst bad 5e1b0aa, last good 77c2d14";
    rig.hub.insert_ticket(detail(
        "R",
        "open",
        None,
        &["regression", "tier:opus"],
        body,
    ));
    rig.hub.queue_reservation(Reservation {
        ticket: ticket_id("R"),
        machine: rig.ctx.machine.clone(),
        session: session("cli-claude-slot-1"),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    });
    tick(&rig);

    assert_eq!(fake.begun().len(), 1);
    let resume = fake.resumes()[0].clone().expect("the culprit is named");
    assert_eq!(resume.culprit.as_str(), "T.1");
    assert!(
        resume
            .prompt
            .starts_with("R is a regression caused by your last ticket")
            && resume.prompt.contains("ignite start R --reservation 7f3a"),
        "{}",
        resume.prompt
    );
    // An ordinary ticket names no culprit.
    reserve(&rig, "T.2", "cli-claude-slot-2", &["tier:opus"]);
    tick(&rig);
    assert_eq!(fake.resumes().get(1), Some(&None));
}
