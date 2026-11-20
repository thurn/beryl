//! The daemon's justiciar: crash records and stuck loops reported to the hub, a justiciar
//! prompt handed to a local worker while the hub is down, and `ignite justiciar`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ignite_core::protocol::client::Exit;
use ignite_core::traits::Clock;

use super::tests::{Rig, request, rig, session};
use super::*;
use crate::crash::{self, CrashRecord};
use crate::justiciar::{HUB_DOWN_S, JusticiarVerb, Reporter};
use crate::loop_health::LoopHealth;
use crate::server::Verb;

fn home(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-justiciar-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn reporter(rig: &Rig, home: &Path) -> (Reporter, LoopHealth) {
    let health = LoopHealth::new(rig.clock.clone());
    let reporter = Reporter::new(
        home.to_path_buf(),
        "jarvis".to_owned(),
        "abc1234".to_owned(),
        health.clone(),
        rig.dispatcher.clone(),
    );
    (reporter, health)
}

fn panic_record() -> CrashRecord {
    CrashRecord {
        at: "2026-11-18T01:00:00Z".to_owned(),
        commit: "abc1234".to_owned(),
        thread: "ignite-tip".to_owned(),
        message: "index out of bounds".to_owned(),
        location: "src/tip.rs:12".to_owned(),
        backtrace: "0: frame".to_owned(),
    }
}

#[test]
fn a_crash_record_is_reported_once_and_marked() {
    let rig = rig(&[]);
    let home = home("crash");
    crash::write(&home, &panic_record()).unwrap();
    let (reporter, _) = reporter(&rig, &home);
    reporter.tick(&rig.ctx, true);
    reporter.tick(&rig.ctx, true);
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].subject, "crash:ignite-tip:src/tip.rs:12");
    assert!(!reports[0].on_machine);
    assert!(reports[0].details.contains("index out of bounds"));
    assert!(crash::pending(&home).is_empty());
}

#[test]
fn a_crash_record_waits_while_the_hub_is_down() {
    let rig = rig(&[]);
    let home = home("crash-down");
    crash::write(&home, &panic_record()).unwrap();
    let (reporter, _) = reporter(&rig, &home);
    rig.hub.set_unreachable(true);
    reporter.tick(&rig.ctx, true);
    assert_eq!(crash::pending(&home).len(), 1);
    rig.hub.set_unreachable(false);
    reporter.tick(&rig.ctx, true);
    assert!(crash::pending(&home).is_empty());
}

#[test]
fn a_stuck_loop_is_reported_once_for_this_machine() {
    let rig = rig(&[]);
    let home = home("loop");
    let (reporter, health) = reporter(&rig, &home);
    let _ticker = health.register("sender", Duration::from_secs(600));
    rig.clock.advance(601.0);
    reporter.tick(&rig.ctx, true);
    rig.clock.advance(60.0);
    reporter.tick(&rig.ctx, true);
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].subject, "loop:sender");
    assert!(reports[0].on_machine);
}

/// On 2026-11-20 cygnus filed a P0 justiciar ticket because its idle loop had waited ten
/// minutes in the scheduler's queue for 12 cores while a tip round and an agent's build held all
/// 18. That is how idle work is meant to behave, so a round waiting for cores is not reported;
/// the same round is reported once it is stuck on anything else.
#[test]
fn a_loop_waiting_for_cores_is_not_reported_until_it_waits_on_something_else() {
    use std::sync::{Arc, Barrier};

    let rig = rig(&[]);
    let home = home("loop-cores");
    let (reporter, health) = reporter(&rig, &home);
    let idle = health.register("ignite-idle", Duration::from_secs(600));
    let barriers: Vec<Arc<Barrier>> = (0..4).map(|_| Arc::new(Barrier::new(2))).collect();
    let round = {
        let barriers = barriers.clone();
        std::thread::spawn(move || {
            idle.round(|| {
                crate::steps::step("building cli in tree-01");
                let cores = crate::steps::waiting_for_cores("12 cores for idle work");
                barriers[0].wait();
                barriers[1].wait();
                drop(cores);
                let _lease = crate::steps::waiting("the tree-01 lease");
                barriers[2].wait();
                barriers[3].wait();
                Ok::<_, String>(())
            })
        })
    };
    barriers[0].wait();
    rig.clock.advance(601.0);
    reporter.tick(&rig.ctx, true);
    assert!(rig.hub.justiciar_reports().is_empty());
    barriers[1].wait();
    barriers[2].wait();
    reporter.tick(&rig.ctx, true);
    barriers[3].wait();
    round.join().unwrap();
    let reports = rig.hub.justiciar_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].subject, "loop:ignite-idle");
}

#[test]
fn a_hub_down_for_fifteen_minutes_gets_a_local_justiciar() {
    let rig = rig(&["local_w1", "local_w2"]);
    let home = home("hub-down");
    let (reporter, _) = reporter(&rig, &home);
    reporter.tick(&rig.ctx, false);
    assert!(rig.dispatcher.state().pending.is_empty());
    rig.clock.advance(HUB_DOWN_S);
    reporter.tick(&rig.ctx, false);
    let given = rig.step().expect("a hand-off");
    assert_eq!(given.action, DispatchAction::Handoff);
    assert!(given.clear);
    assert_eq!(given.model.as_deref(), Some("opus"));
    assert!(given.prompt.unwrap().contains("ignite guide justiciar"));
    let worker = given.session.unwrap();

    // Only one justiciar at a time, and it is kept from tickets.
    rig.clock.advance(60.0);
    reporter.tick(&rig.ctx, false);
    assert!(rig.dispatcher.state().pending.is_empty());
    let members = rig.dispatcher.pool().members().unwrap();
    let state = rig.dispatcher.state();
    let idle = Dispatcher::idle_workers(&state, &members, rig.clock.now());
    assert!(idle.iter().all(|m| m.session != worker), "{idle:?}");
    assert!(
        idle.iter()
            .any(|m| m.session == session("local_w2") || m.session == session("local_w1"))
    );
}

#[test]
fn asking_for_a_justiciar_files_it_and_keeps_it_while_the_hub_is_down() {
    let rig = rig(&["local_w1"]);
    let home = home("verb");
    let verb = JusticiarVerb {
        home: home.clone(),
        host: "jarvis".to_owned(),
        dispatcher: rig.dispatcher.clone(),
    };
    let ask = request(
        "justiciar",
        &["the remote rejects every push"],
        Some("local_w9"),
    );
    let reply = verb.run(&rig.ctx, &ask);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(rig.hub.justiciar_reports()[0].subject, "request:local_w9");

    rig.hub.set_unreachable(true);
    let reply = verb.run(&rig.ctx, &ask);
    assert_eq!(reply.exit, Exit::Unreachable, "{}", reply.text);
    assert!(
        rig.step()
            .is_some_and(|i| i.action == DispatchAction::Handoff)
    );
    rig.hub.set_unreachable(false);
    let (reporter, _) = reporter(&rig, &home);
    reporter.tick(&rig.ctx, true);
    assert_eq!(rig.hub.justiciar_reports().len(), 2);
    assert!(!home.join("justiciar-request.json").exists());
}

#[test]
fn a_refused_justiciar_hand_off_releases_the_hold() {
    let rig = rig(&["local_w1", "local_w2"]);
    let first = rig
        .dispatcher
        .hand_off_justiciar(&rig.ctx, "repair the hub")
        .expect("a worker");
    let given = rig.step().expect("the hand-off");
    assert_eq!(rig.ack(given.id, "failed", "busy").exit, Exit::Success);
    let now = rig.clock.now();
    assert!(!justiciar::holds(&rig.dispatcher.state(), &first, now));
    // The prompt can go to another worker straight away.
    let second = rig
        .dispatcher
        .hand_off_justiciar(&rig.ctx, "repair the hub")
        .expect("another worker");
    assert_ne!(second, first);
}
