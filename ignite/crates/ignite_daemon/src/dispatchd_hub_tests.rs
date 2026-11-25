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
    let headless_dispatch = rig.headless_dispatch.clone();
    let spy = {
        let (offences, calls) = (offences.clone(), calls.clone());
        SpyHub::new(rig.hub.clone()).spying(move |name| {
            if matches!(
                headless_dispatch.state.try_lock(),
                Err(TryLockError::WouldBlock)
            ) {
                lock(&offences).push(name.to_owned());
            }
            lock(&calls).push(name.to_owned());
        })
    };
    rig.ctx.hub = Arc::new(spy);

    // Asking for a ticket and reading it.
    reserve(&rig, "T.1", "reservation-1", &[]);
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
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    rig.tell(&["T.1", "Hello."]);
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
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
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    assert_eq!(rig.headless_dispatch.state().feed, Some(Seq(2)));
    // The hub's database was restored from an older backup.
    rig.headless_dispatch.state().feed = Some(Seq(99));
    rig.headless_dispatch.poll_feed(&rig.ctx, 0).unwrap();
    assert_eq!(rig.headless_dispatch.state().feed, Some(Seq(2)));
}

fn hear_fleet_state(rig: &Rig) {
    crate::fleet_notice::heard(
        rig.headless_dispatch.pool(),
        &HeartbeatReply {
            fleet: rig.hub.fleet_notice(&rig.ctx.machine),
            paused_everywhere: rig.hub.pauses().last().is_some_and(|pause| pause.paused),
            hub_version: "test".to_owned(),
            emergency: None,
            reconciled: Vec::new(),
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
        headless_dispatch: rig.headless_dispatch.clone(),
        hub: rig.hub.clone(),
        clock: rig.clock.clone(),
    };
    assert_eq!(status.lines(), ["room to start 2 (worker ceiling 2)"]);
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
    reserve(&rig, "T.1", "reservation-1", &["regression"]);
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
    reserve(&rig, "T.1", "reservation-1", &[]);

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
/// every command took many times its unloaded time. With a busy host and a two-minute queue,
/// the daemon asks only for tickets that hold `master` back, and `ignite status` says why.
#[test]
fn a_long_wait_on_a_busy_host_asks_only_for_tickets_holding_master_back() {
    let rig = rig(&[]);
    let fake = cli(&rig, 2);
    *lock(&rig.host.waited) = f64::from(BACKLOG_SECONDS) + 30.0;
    *lock(&rig.host.cpu_percent) = Some(90);
    tick(&rig);
    let asked = rig.hub.asked();
    assert!(
        asked
            .iter()
            .all(|free| free.master_only && !free.justiciar_only),
        "{asked:?}"
    );
    let (idle, why) = rig.headless_dispatch.idle_reason().unwrap();
    assert_eq!(idle, 2);
    assert!(
        why.starts_with("commands have waited 150 s for cores"),
        "{why}"
    );

    // A regression still starts.
    reserve(&rig, "R", "reservation-1", &["regression"]);
    tick(&rig);
    assert_eq!(fake.begun()[0].0.ticket.as_str(), "R");

    // Once CPU headroom returns, an ordinary ticket can start during the same long wait.
    *lock(&rig.host.cpu_percent) = Some(89);
    reserve(&rig, "T.1", "reservation-2", &[]);
    let before = rig.hub.asked().len();
    tick(&rig);
    assert_eq!(fake.begun()[1].0.ticket.as_str(), "T.1");
    let asked = rig.hub.asked();
    let request = &asked[before];
    assert!(!request.master_only);
    assert_eq!(request.pressure.as_ref().unwrap().queue_seconds, 150);
}
