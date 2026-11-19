//! A Codex dispatcher's worker slots: each free slot takes a ticket, which runs in a new chat
//! that holds the slot until its ticket ends.

use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::Clock;

use super::restart_tests::DbFile;
use super::tests::{
    Rig, START, detail, handoff_to, lock, request, rig, rig_on, session, ticket_id,
};
use super::*;
use crate::pool::PoolVerb;
use crate::server::Verb;

const CODEX: &str = "local_codex";

/// Registers `dispatcher` as the machine's Codex dispatcher with `slots` slots.
fn codex(rig: &Rig, dispatcher: &str, slots: u32) -> Reply {
    let count = slots.to_string();
    let reply = rig.pool(
        &["add", "--app", "codex", "--dispatcher", "--slots", &count],
        Some(dispatcher),
    );
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    reply
}

/// The hub reserves `ticket` for whichever slot the daemon offered last.
fn reserve_for_slot(rig: &Rig, ticket: &str, slot: &str) {
    rig.hub.insert_ticket(detail(ticket, "open", None, &[], ""));
    rig.hub.queue_reservation(Reservation {
        ticket: ticket_id(ticket),
        machine: rig.ctx.machine.clone(),
        session: session(slot),
        expires_at: rig.clock.now().plus_seconds(600.0),
        code: "7f3a".to_owned(),
    });
}

/// The ticket a slot's hand-off starts. It names no session, and clears nothing.
fn slot_ticket(instruction: &DispatchInstruction) -> String {
    assert_eq!(instruction.action, DispatchAction::Handoff);
    assert_eq!(instruction.session, None);
    assert!(!instruction.clear);
    let prompt = instruction.prompt.as_deref().unwrap();
    let command = prompt.split('`').nth(1).unwrap();
    assert!(command.ends_with("--reservation 7f3a"), "{command}");
    command.split_whitespace().nth(2).unwrap().to_owned()
}

fn step_for(rig: &Rig, dispatcher: &str) -> Option<DispatchInstruction> {
    rig.dispatcher.step(&rig.ctx, Some(&session(dispatcher)))
}

/// The slots the daemon offered the hub the last time it asked.
fn offered(rig: &Rig) -> Vec<String> {
    let asked = rig.hub.asked();
    let last = asked.last().expect("the hub was asked");
    last.idle_workers
        .iter()
        .map(|w| w.session.as_str().to_owned())
        .collect()
}

#[test]
fn two_slots_take_two_of_three_tickets_and_the_third_after_one_lands() {
    let rig = rig(&[]);
    let reply = codex(&rig, CODEX, 2);
    assert!(reply.text.contains("2 slots"), "{}", reply.text);
    // The fake hub hands out what it was given in order, so the third ready ticket is given to
    // it only once a slot is free; the hub not being asked again shows no third slot is offered.
    reserve_for_slot(&rig, "T.1", "codex-slot-1");
    reserve_for_slot(&rig, "T.2", "codex-slot-2");
    let first = step_for(&rig, CODEX).unwrap();
    let second = step_for(&rig, CODEX).unwrap();
    assert_eq!([slot_ticket(&first), slot_ticket(&second)], ["T.1", "T.2"]);
    // The hub is asked once per ticket, and not again once both slots are taken.
    let asks = rig.hub.asked().len();
    assert_eq!(asks, 2);
    assert!(step_for(&rig, CODEX).is_none());
    assert_eq!(rig.hub.asked().len(), asks);

    // The dispatcher opens a chat for each and reports its thread.
    let ack = rig.ack(first.id, "ok", "thread-a");
    assert!(ack.text.contains("thread-a takes T.1"), "{}", ack.text);
    rig.ack(second.id, "ok", "thread-b");
    rig.hold("thread-a", "T.1");
    rig.hold("thread-b", "T.2");
    assert!(step_for(&rig, CODEX).is_none());
    assert_eq!(
        rig.hub.asked().len(),
        asks,
        "no slot is free while both chats work"
    );
    assert_eq!(rig.state_of("thread-a"), "working");

    // T.1 lands: its chat leaves the pool and its slot is offered to the hub at once.
    rig.finish("thread-a");
    assert!(step_for(&rig, CODEX).is_none());
    assert_eq!(rig.hub.asked().len(), asks + 1);
    assert_eq!(offered(&rig), ["codex-slot-2"]);
    assert!(
        rig.dispatcher
            .pool()
            .member(&session("thread-a"))
            .unwrap()
            .is_none(),
        "a chat is never handed a second ticket"
    );
    reserve_for_slot(&rig, "T.3", "codex-slot-2");
    let third = step_for(&rig, CODEX).unwrap();
    assert_eq!(slot_ticket(&third), "T.3");
}

#[test]
fn the_chat_reported_in_the_ack_holds_the_ticket_and_is_checked_when_silent() {
    let rig = rig(&[]);
    codex(&rig, CODEX, 1);
    reserve_for_slot(&rig, "T.1", "codex-slot-1");
    let handoff = step_for(&rig, CODEX).unwrap();
    rig.ack(handoff.id, "ok", "thread-a");
    let chat = rig
        .dispatcher
        .pool()
        .member(&session("thread-a"))
        .unwrap()
        .expect("the chat joins the pool");
    assert!(chat.single_use);
    assert_eq!(chat.parent, Some(session(CODEX)));
    assert_eq!(rig.state_of("thread-a"), "reserved");

    rig.hold("thread-a", "T.1");
    lock(&rig.host.commands).insert(
        session("thread-a"),
        SessionCommand {
            command: "start T.1".to_owned(),
            since: rig.clock.now(),
            running: false,
        },
    );
    assert!(step_for(&rig, CODEX).is_none());
    rig.clock.advance(SILENT_S + 1.0);
    let check = step_for(&rig, CODEX).expect("a silent chat is checked");
    assert_eq!(check.action, DispatchAction::Check);
    assert_eq!(check.session, Some(session("thread-a")));
}

#[test]
fn an_ack_without_a_thread_keeps_the_slot_until_the_reservation_lapses() {
    let rig = rig(&[]);
    codex(&rig, CODEX, 1);
    reserve_for_slot(&rig, "T.1", "codex-slot-1");
    let handoff = step_for(&rig, CODEX).unwrap();
    let ack = rig.ack(handoff.id, "ok", "");
    assert!(ack.text.contains("no thread ID"), "{}", ack.text);
    assert_eq!(rig.state_of("codex-slot-1"), "reserved");
    rig.clock.advance(601.0);
    assert!(step_for(&rig, CODEX).is_none(), "a slot is never checked");
    assert_eq!(rig.state_of("codex-slot-1"), "idle");
}

#[test]
fn claude_and_codex_dispatchers_get_only_their_own_handoffs_within_the_free_trees() {
    let rig = rig(&[]);
    rig.pool(
        &["add", "--app", "claude", "--dispatcher"],
        Some("local_dispatcher"),
    );
    rig.pool(
        &["add", "local_w1", "--app", "claude"],
        Some("local_dispatcher"),
    );
    codex(&rig, CODEX, 2);
    *lock(&rig.host.trees) = 2;
    rig.reserve("T.1", "local_w1", &[]);
    reserve_for_slot(&rig, "T.2", "codex-slot-1");
    reserve_for_slot(&rig, "T.3", "codex-slot-2");

    let codex_handoff = step_for(&rig, CODEX).unwrap();
    assert_eq!(slot_ticket(&codex_handoff), "T.2");
    assert!(
        step_for(&rig, CODEX).is_none(),
        "the trees are all reserved"
    );
    let claude_handoff = step_for(&rig, "local_dispatcher").unwrap();
    assert_eq!(handoff_to(&claude_handoff), ("local_w1", "T.1"));
    assert!(step_for(&rig, "local_dispatcher").is_none());
    let asked = rig.hub.asked();
    let trees: Vec<u32> = asked.iter().map(|a| a.free_ticket_trees).collect();
    assert_eq!(trees, [2, 1], "two trees, so two tickets and no third ask");
    let first: Vec<&str> = asked[0]
        .idle_workers
        .iter()
        .map(|w| w.session.as_str())
        .collect();
    assert_eq!(first, ["local_w1", "codex-slot-1", "codex-slot-2"]);
}

#[test]
fn a_rotated_dispatcher_keeps_the_slot_count_and_the_tickets_in_progress() {
    let rig = rig(&[]);
    codex(&rig, CODEX, 2);
    reserve_for_slot(&rig, "T.1", "codex-slot-1");
    let handoff = step_for(&rig, CODEX).unwrap();
    rig.ack(handoff.id, "ok", "thread-a");
    rig.hold("thread-a", "T.1");
    assert!(step_for(&rig, CODEX).is_none());

    // `/ignite pool 0` from the new chat registers it with 0, which keeps the count.
    let rotated = codex(&rig, "local_codex2", 0);
    assert!(rotated.text.contains("2 slots"), "{}", rotated.text);
    let pool = rig.dispatcher.pool();
    assert!(pool.member(&session(CODEX)).unwrap().is_none());
    assert_eq!(
        pool.member(&session("local_codex2"))
            .unwrap()
            .unwrap()
            .slots,
        2
    );
    let chat = pool.member(&session("thread-a")).unwrap().unwrap();
    assert_eq!(chat.parent, Some(session("local_codex2")));
    let view = rig.dispatcher.view().unwrap();
    let states: Vec<(&str, &str)> = view
        .iter()
        .filter(|w| w.role == WorkerRole::Worker)
        .map(|w| (w.session.as_str(), w.state.as_str()))
        .collect();
    assert_eq!(states, [("thread-a", "working"), ("codex-slot-2", "idle")]);

    // A new dispatcher with a count of its own starts afresh, and the old chats go with it.
    let replaced = codex(&rig, "local_codex3", 3);
    assert!(replaced.text.contains("thread-a"), "{}", replaced.text);
    assert!(pool.member(&session("thread-a")).unwrap().is_none());
}

#[test]
fn slots_are_only_for_a_codex_dispatcher() {
    let rig = rig(&[]);
    let reply = rig.pool(
        &["add", "--app", "claude", "--dispatcher", "--slots", "2"],
        Some("local_dispatcher"),
    );
    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
    let pool = PoolVerb::new(rig.dispatcher.clone());
    let reply = pool.run(
        &rig.ctx,
        &request(
            "pool",
            &["add", "--app", "codex", "--dispatcher", "--slots", "x"],
            Some(CODEX),
        ),
    );
    assert_eq!(reply.exit, Exit::Usage, "{}", reply.text);
}

#[test]
fn a_restart_keeps_the_slots_and_their_chats() {
    let file = DbFile::new("slots");
    let first = rig_on(&[], file.open(), Timestamp::from_millis(START));
    codex(&first, CODEX, 2);
    reserve_for_slot(&first, "T.1", "codex-slot-1");
    reserve_for_slot(&first, "T.2", "codex-slot-2");
    let one = step_for(&first, CODEX).unwrap();
    let two = step_for(&first, CODEX).unwrap();
    first.ack(one.id, "ok", "thread-a");
    first.hold("thread-a", "T.1");
    assert!(step_for(&first, CODEX).is_none());

    let now = first.clock.now().plus_seconds(5.0);
    let restarted = rig_on(&[], file.open(), now);
    restarted.hold("thread-a", "T.1");
    // The hand-off given to the second slot and not acknowledged is given again.
    let again = step_for(&restarted, CODEX).expect("given again");
    assert_eq!(again, two);
    restarted.ack(again.id, "ok", "thread-b");
    assert_eq!(restarted.state_of("thread-a"), "working");
    assert_eq!(restarted.state_of("thread-b"), "reserved");
    assert_eq!(
        restarted
            .dispatcher
            .pool()
            .member(&session(CODEX))
            .unwrap()
            .unwrap()
            .slots,
        2
    );
    assert!(step_for(&restarted, CODEX).is_none());
}
