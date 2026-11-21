//! Hub requests run outside the CLI dispatch state lock. Pause and backlog limit new starts.

use std::sync::TryLockError;

use super::cli_tests::{cli, reserve, tick};
use super::tests::{Rig, detail, lock, rig};
use super::*;
use crate::testhub::SpyHub;
use ignite_core::dispatch_pressure::BACKLOG_SECONDS;
use ignite_core::fleet::MachineState;
use ignite_core::protocol::hub::HeartbeatReply;
use ignite_core::traits::Hub;

#[test]
fn no_hub_request_is_made_while_the_state_is_locked() {
    let mut rig = rig(&[]);
    cli(&rig, 1);
    let offences = Arc::new(Mutex::new(Vec::<String>::new()));
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let dispatcher = rig.dispatcher.clone();
    let spy = {
        let (offences, calls) = (offences.clone(), calls.clone());
        SpyHub::new(rig.hub.clone()).spying(move |name| {
            if matches!(dispatcher.state.try_lock(), Err(TryLockError::WouldBlock)) {
                lock(&offences).push(name.to_owned());
            }
            lock(&calls).push(name.to_owned());
        })
    };
    rig.ctx.hub = Arc::new(spy);

    // Asking for a ticket and reading it.
    reserve(&rig, "T.1", "cli-claude-slot-1", &[]);
    tick(&rig);
    // A session finishing its ticket, whose landed files are read.
    rig.hub.insert_ticket(detail(
        "T.1",
        "in_progress",
        Some("cli-claude-00000001"),
        &[],
        "",
    ));
    rig.hold("cli-claude-00000001", "T.1");
    tick(&rig);
    // The feed, and a message to the holder.
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    rig.tell(&["T.1", "Hello."]);
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    rig.finish("cli-claude-00000001");
    tick(&rig);

    let calls = lock(&calls).clone();
    for expected in ["next", "ticket", "feed"] {
        assert!(calls.iter().any(|c| c == expected), "{expected}: {calls:?}");
    }
    assert!(
        lock(&offences).is_empty(),
        "hub requests with the state locked: {:?}",
        lock(&offences)
    );
}

#[test]
fn the_feed_cursor_follows_a_hub_whose_journal_restarted_lower() {
    let rig = rig(&[]);
    rig.hub.push_change("ticket", None);
    rig.hub.push_change("ticket", None);
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    assert_eq!(rig.dispatcher.state().feed, Some(Seq(2)));
    // The hub's database was restored from an older backup.
    rig.dispatcher.state().feed = Some(Seq(99));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    assert_eq!(rig.dispatcher.state().feed, Some(Seq(2)));
}

fn hear_fleet_state(rig: &Rig) {
    crate::fleet_notice::heard(
        rig.dispatcher.pool(),
        &HeartbeatReply {
            fleet: rig.hub.fleet_notice(&rig.ctx.machine),
            paused_everywhere: rig.hub.pauses().last().is_some_and(|pause| pause.paused),
            hub_version: "test".to_owned(),
            emergency: None,
        },
    )
    .unwrap();
}

#[test]
fn a_paused_pool_asks_only_for_tickets_holding_master_back() {
    use crate::doctor::StatusSection;
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    let status = crate::pool::PauseSection {
        dispatcher: rig.dispatcher.clone(),
        hub: rig.hub.clone(),
        clock: rig.clock.clone(),
    };
    assert!(status.lines().is_empty(), "nothing to say while not paused");
    assert_eq!(
        rig.pool(&["pause"], Some("local_operator")).exit,
        Exit::Success
    );
    assert!(
        rig.hub.pauses().is_empty(),
        "single-machine pause uses the fleet API"
    );
    assert_eq!(
        rig.hub.fleet_notice(&rig.ctx.machine).unwrap().state,
        MachineState::ChecksOnly
    );
    hear_fleet_state(&rig);
    // The hub answers a paused machine only with a ticket that holds `master` back.
    reserve(&rig, "T.1", "cli-claude-slot-1", &["regression"]);
    tick(&rig);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "T.1");
    let asked = rig.hub.asked();
    assert!(
        asked
            .iter()
            .all(|free| free.master_only && !free.justiciar_only),
        "{asked:?}"
    );
    assert!(
        rig.pool(&["list"], None)
            .text
            .contains("machine is checks-only (set by local_operator)")
    );
    let lines = status.lines();
    assert!(
        lines[0] == "machine is checks-only (set by local_operator)",
        "{lines:?}"
    );
    assert!(lines[1].starts_with("master held back by T.1"), "{lines:?}");
    assert_eq!(rig.pool(&["resume"], None).exit, Exit::Success);
    assert_eq!(
        rig.hub.fleet_notice(&rig.ctx.machine).unwrap().state,
        MachineState::Active
    );
    hear_fleet_state(&rig);
    let before = rig.hub.asked().len();
    tick(&rig);
    assert!(
        rig.hub.asked()[before..]
            .iter()
            .any(|free| !free.master_only)
    );

    // A pause of every machine asks for nothing at all.
    rig.pool(&["pause", "--all"], None);
    assert!(rig.hub.pauses().last().unwrap().machine.is_none());
    hear_fleet_state(&rig);
    let before = rig.hub.asked().len();
    tick(&rig);
    assert_eq!(rig.hub.asked().len(), before);
    assert!(rig.pool(&["list"], None).text.contains("every machine"));
    assert!(status.lines()[0].starts_with("paused on every machine"));
}

#[test]
fn dispatch_stops_and_resumes_on_heartbeat_without_starting_a_new_daemon() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    let set = |state| {
        rig.hub
            .set_fleet_machine_state(&rig.ctx.machine, state, Some("operator"))
            .unwrap();
        hear_fleet_state(&rig);
    };
    reserve(&rig, "T.1", "cli-claude-slot-1", &[]);

    set(MachineState::Off);
    tick(&rig);
    assert!(rig.hub.asked().is_empty());
    assert!(fake.begun().is_empty());
    assert!(
        rig.pool(&["list"], None)
            .text
            .contains("machine is off (set by operator)")
    );

    set(MachineState::Active);
    tick(&rig);
    assert!(!rig.hub.asked()[0].master_only);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "T.1");
}

/// On 2026-11-20 jarvis kept starting workers while 16 to 18 commands waited for its cores, and
/// every command took many times its unloaded time. While ticket work has waited two minutes or
/// more for cores, the dispatcher asks only for the tickets that hold `master` back, as a paused
/// pool does, and `ignite status` says why its workers are idle.
#[test]
fn a_long_wait_for_cores_asks_only_for_tickets_holding_master_back() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    *lock(&rig.host.waited) = f64::from(BACKLOG_SECONDS) + 30.0;
    tick(&rig);
    let asked = rig.hub.asked();
    assert!(
        asked
            .iter()
            .all(|free| free.master_only && !free.justiciar_only),
        "{asked:?}"
    );
    let (idle, why) = rig.dispatcher.idle_reason().unwrap();
    assert_eq!(idle, 3);
    assert!(
        why.starts_with("commands have waited 150 s for cores"),
        "{why}"
    );

    // A regression still starts.
    reserve(&rig, "R", "cli-claude-slot-1", &["regression"]);
    tick(&rig);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "R");

    // Once the wait is short again, ordinary tickets start.
    *lock(&rig.host.waited) = f64::from(BACKLOG_SECONDS) - 1.0;
    reserve(&rig, "T.1", "cli-claude-slot-2", &[]);
    let before = rig.hub.asked().len();
    tick(&rig);
    assert_eq!(fake.begun()[1].0.ticket.as_str(), "T.1");
    assert!(
        rig.hub.asked()[before..]
            .iter()
            .any(|free| !free.master_only)
    );
}
