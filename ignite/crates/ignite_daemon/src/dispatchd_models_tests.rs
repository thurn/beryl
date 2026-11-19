//! The model ID and reasoning effort a hand-off carries: tier labels are looked up in
//! `[pool.models]`, a tier the app has no ID for is not offered to its workers, and the effort
//! defaults to medium except for a justiciar.

use ignite_core::protocol::client::DispatchAction;

use super::tests::{Rig, handoff_to, rig};

/// A rig whose Claude table maps `opus` and `sonnet` to IDs, and has no entry for `light`.
fn mapped_rig(workers: &[&str]) -> Rig {
    let mut rig = rig(workers);
    let table = [
        ("opus", "claude-opus-test"),
        ("sonnet", "claude-sonnet-test"),
    ];
    rig.ctx.config.pool.models.insert(
        "claude".to_owned(),
        table
            .iter()
            .map(|(tier, id)| ((*tier).to_owned(), (*id).to_owned()))
            .collect(),
    );
    rig
}

#[test]
fn a_tier_opus_handoff_carries_the_id_from_the_claude_table() {
    let rig = mapped_rig(&["local_w1"]);
    rig.reserve("T.1", "local_w1", &["tier:opus"]);
    let handoff = rig.step().unwrap();
    assert_eq!(handoff_to(&handoff), ("local_w1", "T.1"));
    assert_eq!(handoff.model.as_deref(), Some("claude-opus-test"));
    // A ticket with no tier runs on `sonnet`.
    let rig = mapped_rig(&["local_w1"]);
    rig.reserve("T.2", "local_w1", &[]);
    assert_eq!(
        rig.step().unwrap().model.as_deref(),
        Some("claude-sonnet-test")
    );
}

#[test]
fn a_tier_with_no_id_is_not_offered_to_a_claude_worker() {
    let rig = mapped_rig(&["local_w1"]);
    assert!(rig.step().is_none());
    let asked = rig.hub.asked();
    assert_eq!(asked[0].idle_workers[0].models, ["opus", "sonnet"]);
}

#[test]
fn a_reservation_for_a_tier_that_lost_its_id_leaves_the_worker_idle() {
    let rig = mapped_rig(&["local_w1"]);
    rig.reserve("T.1", "local_w1", &["tier:light"]);
    assert!(rig.step().is_none());
    assert_eq!(rig.state_of("local_w1"), "idle");
}

#[test]
fn a_handoff_is_medium_effort_unless_the_ticket_says_otherwise() {
    let rig = rig(&["local_w1", "local_w2"]);
    rig.reserve("T.1", "local_w1", &[]);
    rig.reserve("T.2", "local_w2", &["effort:xhigh"]);
    let first = rig.step().unwrap();
    let second = rig.step().unwrap();
    let (plain, tuned) = if handoff_to(&first).1 == "T.1" {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(plain.effort.as_deref(), Some("medium"));
    assert_eq!(tuned.effort.as_deref(), Some("xhigh"));
}

#[test]
fn a_justiciar_handoff_is_high_effort_on_the_mapped_model() {
    let rig = mapped_rig(&["local_w1", "local_w2"]);
    let worker = rig.dispatcher.hand_off_justiciar(&rig.ctx, "repair");
    assert!(worker.is_some());
    let given = rig.step().unwrap();
    assert_eq!(given.action, DispatchAction::Handoff);
    assert_eq!(given.model.as_deref(), Some("claude-opus-test"));
    assert_eq!(given.effort.as_deref(), Some("high"));
}
