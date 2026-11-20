//! How the dispatcher deals with the hub: no request is made with the state locked, a hand-off
//! whose reservation lapsed is dropped, `dispatch` ends before its caller's wait does, and the
//! feed cursor follows a hub whose journal restarted, and a paused pool asks the hub only for
//! the tickets that hold `master` back, as does a pool whose commands wait long for cores.

use std::sync::TryLockError;

use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::tests::{detail, handoff_to, lock, request, rig, session};
use super::work::BACKLOG_SECONDS;
use super::*;
use crate::server::Verb;
use crate::testhub::SpyHub;
use ignite_core::protocol::client::Reply;

#[test]
fn no_hub_request_is_made_while_the_state_is_locked() {
    let mut rig = rig(&["local_w1"]);
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
    rig.reserve("T.1", "local_w1", &[]);
    let handoff = rig.step().unwrap();
    rig.handed(&handoff);
    // A session finishing its ticket, whose landed files are read.
    rig.hub
        .insert_ticket(detail("T.1", "in_progress", Some("local_w1"), &[], ""));
    rig.hold("local_w1", "T.1");
    rig.step();
    // The feed, and a message to the holder.
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    rig.tell(&["T.1", "Hello."]);
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    rig.finish("local_w1");
    rig.step();

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
fn a_hand_off_queued_before_its_reservation_lapsed_is_not_given_after() {
    let rig = rig(&["local_w1"]);
    rig.reserve("T.1", "local_w1", &[]);
    let members = rig.dispatcher.pool().members().unwrap();
    rig.dispatcher.refresh(&rig.ctx, &members);
    assert_eq!(rig.dispatcher.state().pending.len(), 1);

    // The dispatcher was not asking, and the hub's ten minutes ran out.
    rig.clock.advance(601.0);
    let mut given = Vec::new();
    while let Some(instruction) = rig.step() {
        given.push(instruction.action);
    }
    assert!(!given.contains(&DispatchAction::Handoff), "{given:?}");
    assert_ne!(rig.state_of("local_w1"), "reserved");
    assert!(rig.dispatcher.state().reserved.is_empty());
}

#[test]
fn dispatch_answers_idle_before_the_requests_own_wait_ends() {
    let rig = rig(&["local_w1"]);
    let verb = DispatchVerb::new(rig.dispatcher.clone());
    let mut req = request("dispatch", &["--wait", "540"], None);
    req.wait_s = 3;
    let reply = verb.run(&rig.ctx, &req);
    assert_eq!(reply.json.unwrap()["do"], "idle");
}

#[test]
fn dispatch_gives_nothing_once_its_caller_has_stopped_waiting() {
    let mut rig = rig(&["local_w1"]);
    rig.hub.insert_ticket(detail("T.1", "open", None, &[], ""));
    let spy = {
        let (hub, clock) = (rig.hub.clone(), rig.clock.clone());
        let machine = rig.ctx.machine.clone();
        SpyHub::new(rig.hub.clone()).spying(move |name| {
            if name == "feed" {
                // The long poll runs past the request's wait, and a reservation arrives.
                clock.advance(30.0);
                hub.queue_reservation(Reservation {
                    ticket: "T.1".parse().unwrap(),
                    machine: machine.clone(),
                    session: session("local_w1"),
                    expires_at: clock.now().plus_seconds(600.0),
                    code: "7f3a".to_owned(),
                });
            }
        })
    };
    rig.ctx.hub = Arc::new(spy);
    let verb = DispatchVerb::new(rig.dispatcher.clone());
    let mut req = request("dispatch", &["--wait", "540"], None);
    req.wait_s = 20;
    let reply = verb.run(&rig.ctx, &req);
    assert_eq!(reply.json.unwrap()["do"], "idle");
    let state = rig.dispatcher.state();
    assert!(state.given.is_empty() && state.pending.is_empty());
}

#[test]
fn a_newer_wait_from_the_same_dispatcher_replaces_the_older_one() {
    let mut rig = rig(&["local_w1"]);
    let verb: Arc<Mutex<Option<Arc<DispatchVerb>>>> = Arc::new(Mutex::new(None));
    let second: Arc<Mutex<Option<Reply>>> = Arc::new(Mutex::new(None));
    let spy = {
        let (verb, second, clock, ctx) = (
            verb.clone(),
            second.clone(),
            rig.clock.clone(),
            rig.ctx.clone(),
        );
        SpyHub::new(rig.hub.clone()).spying(move |name| {
            if name != "feed" {
                return;
            }
            clock.advance(30.0);
            // While the first wait is polling the feed, the dispatcher starts another.
            let mut second = lock(&second);
            if second.is_none() {
                let verb = lock(&verb).clone().unwrap();
                let req = request("dispatch", &["--wait", "0"], Some("local_disp"));
                *second = Some(verb.run(&ctx, &req));
            }
        })
    };
    rig.ctx.hub = Arc::new(spy);
    let dispatch = Arc::new(DispatchVerb::new(rig.dispatcher.clone()));
    *lock(&verb) = Some(dispatch.clone());
    let first = dispatch.run(
        &rig.ctx,
        &request("dispatch", &["--wait", "540"], Some("local_disp")),
    );
    assert_eq!(first.text, verb::REPLACED);
    let second = lock(&second).take().unwrap();
    assert_eq!(second.json.unwrap()["do"], "idle");
    // A wait from another session is not replaced by this dispatcher's.
    let other = dispatch.run(
        &rig.ctx,
        &request("dispatch", &["--wait", "0"], Some("local_other")),
    );
    assert_eq!(other.json.unwrap()["do"], "idle");
}

#[test]
fn the_feed_cursor_follows_a_hub_whose_journal_restarted_lower() {
    let rig = rig(&["local_w1"]);
    rig.hub.push_change("ticket", None);
    rig.hub.push_change("ticket", None);
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    assert_eq!(rig.dispatcher.state().feed, Some(Seq(2)));
    // The hub's database was restored from an older backup.
    rig.dispatcher.state().feed = Some(Seq(99));
    rig.dispatcher.poll_feed(&rig.ctx, 0).unwrap();
    assert_eq!(rig.dispatcher.state().feed, Some(Seq(2)));
}

#[test]
fn a_paused_pool_asks_only_for_tickets_holding_master_back() {
    use crate::doctor::StatusSection;
    let rig = rig(&["local_w1", "local_w2"]);
    let status = crate::pool::PauseSection {
        dispatcher: rig.dispatcher.clone(),
        hub: rig.hub.clone(),
    };
    assert!(status.lines().is_empty(), "nothing to say while not paused");
    assert_eq!(rig.pool(&["pause"], None).exit, Exit::Success);
    assert_eq!(rig.hub.pauses()[0].machine, Some(rig.ctx.machine.clone()));
    // The hub answers a paused machine only with a ticket that holds `master` back.
    rig.reserve("T.1", "local_w1", &["regression"]);
    assert_eq!(handoff_to(&rig.step().unwrap()), ("local_w1", "T.1"));
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
            .contains("only tickets that hold master back")
    );
    let lines = status.lines();
    assert!(lines[0].starts_with("paused on this machine"), "{lines:?}");
    assert!(lines[1].starts_with("master held back by T.1"), "{lines:?}");
    rig.pool(&["resume"], None);
    rig.step();
    assert!(!rig.hub.asked().last().unwrap().master_only);

    // A pause of every machine asks for nothing at all.
    rig.pool(&["pause", "--all"], None);
    rig.pool(&["pause"], None);
    assert_eq!(
        rig.hub.pauses().last().unwrap().machine,
        Some(rig.ctx.machine.clone())
    );
    let before = rig.hub.asked().len();
    assert!(rig.step().is_none());
    assert_eq!(rig.hub.asked().len(), before);
    assert!(rig.pool(&["list"], None).text.contains("every machine"));
    assert!(status.lines()[0].starts_with("paused on every machine"));
}

/// On 2026-11-20 jarvis kept starting workers while 16 to 18 commands waited for its cores, and
/// every command took many times its unloaded time. While ticket work has waited two minutes or
/// more for cores, the dispatcher asks only for the tickets that hold `master` back, as a paused
/// pool does, and `ignite status` says why its workers are idle.
#[test]
fn a_long_wait_for_cores_asks_only_for_tickets_holding_master_back() {
    let rig = rig(&["local_w1", "local_w2"]);
    *lock(&rig.host.waited) = BACKLOG_SECONDS + 30.0;
    assert!(rig.step().is_none());
    let asked = rig.hub.asked();
    assert!(
        asked
            .iter()
            .all(|free| free.master_only && !free.justiciar_only),
        "{asked:?}"
    );
    let (idle, why) = rig.dispatcher.idle_reason().unwrap();
    assert_eq!(idle, 2);
    assert!(
        why.starts_with("commands have waited 150 s for cores"),
        "{why}"
    );

    // A regression still starts.
    rig.reserve("R", "local_w1", &["regression"]);
    assert_eq!(handoff_to(&rig.step().unwrap()), ("local_w1", "R"));

    // Once the wait is short again, ordinary tickets start.
    *lock(&rig.host.waited) = BACKLOG_SECONDS - 1.0;
    rig.reserve("T.1", "local_w2", &[]);
    assert_eq!(handoff_to(&rig.step().unwrap()), ("local_w2", "T.1"));
    assert!(!rig.hub.asked().last().unwrap().master_only);
}
