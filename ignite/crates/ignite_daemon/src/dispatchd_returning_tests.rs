//! A machine that comes back after the hub released its claims for silence acts on the hub's
//! answer for each ticket its workers still hold trees for.

use ignite_core::protocol::returning::{RELEASED_OFFLINE_KEY, Reconciled, ReturnOutcome};
use ignite_core::protocol::tickets::Operation;

use super::Host as _;
use super::cli_tests::cli;
use super::tests::{detail, rig, session, ticket_id};

const WORKER: &str = "cli-claude-00000001";

fn answer(outcome: ReturnOutcome) -> Reconciled {
    Reconciled {
        session: session(WORKER),
        ticket: ticket_id("T.1"),
        outcome,
    }
}

#[test]
fn a_ticket_given_back_keeps_the_worker_and_its_tree() {
    let rig = rig(&[]);
    let pool = cli(&rig, 4);
    rig.hold(WORKER, "T.1");

    rig.headless_dispatch
        .reconciled(&rig.ctx, &[answer(ReturnOutcome::Reclaimed)]);

    assert!(pool.retired().is_empty());
    assert_eq!(
        rig.host.holders().get(&session(WORKER)),
        Some(&ticket_id("T.1"))
    );
    assert!(rig.hub.batches().is_empty());
}

#[test]
fn a_lost_ticket_saves_the_tree_retires_the_worker_and_says_where_the_work_went() {
    let rig = rig(&[]);
    let pool = cli(&rig, 4);
    rig.hold(WORKER, "T.1");
    // Another machine's worker took the ticket while this one was away.
    rig.hub.insert_ticket(detail(
        "T.1",
        "in_progress",
        Some("cli-codex-00000002"),
        &[],
        "",
    ));

    rig.headless_dispatch
        .reconciled(&rig.ctx, &[answer(ReturnOutcome::Lost)]);

    assert_eq!(pool.retired(), [session(WORKER)]);
    assert!(rig.host.holders().is_empty(), "the tree was freed");
    let batches = rig.hub.batches();
    let [batch] = batches.as_slice() else {
        panic!("expected one batch, found {batches:?}");
    };
    let comment = batch.ops.iter().find_map(|op| match op {
        Operation::Comment { text, .. } => Some(text.clone()),
        _ => None,
    });
    let comment = comment.expect("a comment on the ticket");
    assert!(
        comment.contains(WORKER) && comment.contains("saved/T.1"),
        "{comment}"
    );
    assert!(batch.ops.iter().any(|op| matches!(
        op,
        Operation::Update { unset_meta, .. } if unset_meta == &[RELEASED_OFFLINE_KEY.to_owned()]
    )));
}
