//! Shedding build tickets from a machine that holds more than its limit, by `ignite pool shed`
//! and by the daemon itself, without any help from the workers.

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::Exit;
use ignite_core::protocol::hub::shed::{AUTO_SHED_AFTER_S, BuildHoldings, HeldBuild, ShedOrder};
use ignite_core::protocol::hub::{HeartbeatReply, ReleaseCause};
use ignite_core::time::Timestamp;

use super::cli_tests::cli;
use super::tests::{Rig, lock, rig, session, ticket_id};
use super::{SessionCommand, shed::AUTOMATIC};
use crate::server::Verb as _;

fn worker(n: usize) -> String {
    format!("cli-claude-0000000{n}")
}

/// A machine holding `count` build tickets `T.1` (claimed first) to `T.<count>` against a
/// limit of 3, each with a tree here, as the hub reports them: newest claim first.
fn hold_builds(rig: &Rig, count: usize) -> BuildHoldings {
    let mut tickets = Vec::new();
    for n in (1..=count).rev() {
        rig.hold(&worker(n), &format!("T.{n}"));
        tickets.push(HeldBuild {
            ticket: ticket_id(&format!("T.{n}")),
            session: Some(session(&worker(n))),
            since: Some(Timestamp::from_millis(
                super::tests::START + n as i64 * 1000,
            )),
            master_blocker: false,
        });
    }
    BuildHoldings { limit: 3, tickets }
}

/// A heartbeat reply carrying `holdings` and `order`.
fn reply(holdings: Option<BuildHoldings>, order: Option<ShedOrder>) -> HeartbeatReply {
    HeartbeatReply {
        hub_version: "test".to_owned(),
        emergency: None,
        paused_everywhere: false,
        fleet: None,
        reconciled: Vec::new(),
        build_tickets: holdings,
        shed: order,
    }
}

fn released(rig: &Rig) -> Vec<TicketId> {
    lock(&rig.host.releases)
        .iter()
        .map(|(_, ticket)| ticket.clone())
        .collect()
}

fn sessions(numbers: &[usize]) -> Vec<SessionId> {
    numbers.iter().map(|n| session(&worker(*n))).collect()
}

#[test]
fn shedding_to_three_releases_the_newest_and_keeps_the_landing_one() {
    let rig = rig(&[]);
    let pool = cli(&rig, 8);
    let holdings = hold_builds(&rig, 6);
    rig.headless_dispatch
        .heard_builds(&rig.ctx, &reply(Some(holdings), None));
    // T.5's worker is landing it.
    lock(&rig.host.commands).insert(
        session(&worker(5)),
        SessionCommand {
            command: "land".to_owned(),
            since: Timestamp::from_millis(super::tests::START),
            running: true,
        },
    );
    assert!(
        rig.pool(&[], None)
            .text
            .contains("held 6 of limit 3 build tickets"),
        "`ignite pool list` shows the count"
    );

    let reply = rig.pool(&["shed", "--to", "3"], None);

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        released(&rig),
        [ticket_id("T.6"), ticket_id("T.4"), ticket_id("T.3")]
    );
    assert_eq!(pool.retired(), sessions(&[6, 4, 3]), "their sessions end");
    for (reason, cause) in lock(&rig.host.release_reasons).iter() {
        assert_eq!(
            *cause,
            ReleaseCause::Shed,
            "a shed is not counted against the ticket"
        );
        assert!(
            reason.contains("Shed by the operator") && reason.contains("limit of 3"),
            "the comment says who shed it and why: {reason}"
        );
    }
    let dones = rig.hub.shed_dones();
    let [done] = dones.as_slice() else {
        panic!("one shed recorded in the fleet's changes, found {dones:?}");
    };
    assert!(done.summary.contains("T.6, T.4, T.3"), "{}", done.summary);
    assert!(
        rig.pool(&[], None)
            .text
            .contains("held 3 of limit 3 build tickets"),
        "the count drops at once"
    );
    assert!(released(&rig).len() == 3 && rig.pool(&["shed"], None).text.contains("nothing"));
}

#[test]
fn holding_more_than_twice_the_limit_for_five_minutes_sheds_by_itself() {
    let rig = rig(&[]);
    let pool = cli(&rig, 8);
    let far_over = hold_builds(&rig, 7);
    rig.headless_dispatch
        .heard_builds(&rig.ctx, &reply(Some(far_over.clone()), None));
    rig.clock.advance(AUTO_SHED_AFTER_S - 1.0);
    rig.headless_dispatch
        .heard_builds(&rig.ctx, &reply(Some(far_over.clone()), None));
    assert!(released(&rig).is_empty(), "not before five minutes");

    rig.clock.advance(1.0);
    rig.headless_dispatch
        .heard_builds(&rig.ctx, &reply(Some(far_over), None));

    assert_eq!(
        released(&rig),
        [
            ticket_id("T.7"),
            ticket_id("T.6"),
            ticket_id("T.5"),
            ticket_id("T.4")
        ]
    );
    assert_eq!(pool.retired(), sessions(&[7, 6, 5, 4]));
    let dones = rig.hub.shed_dones();
    assert!(
        dones.len() == 1 && dones[0].summary.contains(AUTOMATIC),
        "the automatic shed is in the fleet's changes: {dones:?}"
    );
}

#[test]
fn holding_less_than_twice_the_limit_never_sheds_by_itself() {
    let rig = rig(&[]);
    let _pool = cli(&rig, 8);
    let over = hold_builds(&rig, 5);
    for _ in 0..4 {
        rig.headless_dispatch
            .heard_builds(&rig.ctx, &reply(Some(over.clone()), None));
        rig.clock.advance(AUTO_SHED_AFTER_S);
    }
    assert!(released(&rig).is_empty());
}

#[test]
fn a_worker_blocked_in_wait_is_shed_without_its_help() {
    let rig = rig(&[]);
    let pool = cli(&rig, 8);
    let holdings = hold_builds(&rig, 4);
    let blocked = session(&worker(4));
    lock(&rig.host.commands).insert(
        blocked.clone(),
        SessionCommand {
            command: "wait L-17".to_owned(),
            since: Timestamp::from_millis(super::tests::START),
            running: true,
        },
    );
    // The order arrives with the heartbeat reply, as `ignite pool shed --machine` sends it.
    let order = ShedOrder {
        machine: "m-1".to_owned(),
        to: None,
        by: "cli-claude-incident".to_owned(),
    };
    rig.headless_dispatch
        .heard_builds(&rig.ctx, &reply(Some(holdings), Some(order)));

    assert_eq!(released(&rig), [ticket_id("T.4")]);
    assert_eq!(pool.retired(), std::slice::from_ref(&blocked));
    assert_eq!(
        *lock(&rig.host.stopped),
        std::slice::from_ref(&blocked),
        "its verbs were stopped"
    );
    let interrupts = lock(&rig.host.interrupts);
    assert!(
        interrupts
            .iter()
            .any(|(s, text)| *s == blocked && text.contains("shed")),
        "its `ignite wait` ended: {interrupts:?}"
    );
}

#[test]
fn only_the_operator_and_emergency_holders_shed_and_a_named_machine_gets_an_order() {
    let rig = rig(&[]);
    let mut ctx = rig.ctx.clone();
    ctx.session = Some(session("cli-claude-00000009"));
    let refused = crate::pool::PoolVerb::new(rig.headless_dispatch.clone()).run(
        &ctx,
        &super::tests::request("pool", &["shed"], Some("cli-claude-00000009")),
    );
    assert_eq!(refused.exit, Exit::Conflict, "{}", refused.text);

    let ordered = rig.pool(&["shed", "--machine", "jarvis", "--to", "2"], None);
    assert_eq!(ordered.exit, Exit::Success, "{}", ordered.text);
    assert_eq!(
        rig.hub.sheds(),
        [ShedOrder {
            machine: "jarvis".to_owned(),
            to: Some(2),
            by: "the operator".to_owned(),
        }]
    );
}
