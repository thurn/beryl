//! What `ignite pool list` and `ignite pool watch` show for an app in CLI mode.

use ignite_core::cli::{Hold, HoldReason, Slot, SlotState};
use ignite_core::protocol::cli::{TranscriptKind, TranscriptLine};
use ignite_core::protocol::client::Exit;
use ignite_core::traits::Clock;

use super::cli_tests::cli;
use super::tests::{rig, session, ticket_id};
use super::*;
use crate::doctor::StatusSection;

fn slot(index: u32, state: SlotState, turn: u32, ticket: Option<&str>, since: Timestamp) -> Slot {
    Slot {
        app: "claude".to_owned(),
        index,
        session: ticket.map(|_| session(&format!("cli-claude-0000000{index}"))),
        ticket: ticket.map(ticket_id),
        state,
        turn,
        transcript: None,
        model: None,
        effort: None,
        since: Some(since),
    }
}

#[test]
fn pool_list_shows_a_running_a_parked_and_a_free_cli_slot() {
    let rig = rig(&[]);
    let fake = cli(&rig, 3);
    let now = rig.clock.now();
    fake.set_slots(
        "claude",
        vec![
            slot(
                1,
                SlotState::Running,
                3,
                Some("T.1"),
                now.plus_seconds(-250.0),
            ),
            slot(
                2,
                SlotState::Parked,
                5,
                Some("T.2"),
                now.plus_seconds(-7300.0),
            ),
            slot(3, SlotState::Free, 0, None, now),
        ],
    );
    fake.set_hold(
        "claude",
        Some(Hold {
            app: "claude".to_owned(),
            reason: HoldReason::NotLoggedIn,
            since: now.plus_seconds(-90.0),
            cli_version: None,
            message: "log in".to_owned(),
        }),
    );

    let reply = rig.pool(&["list"], None);
    assert_eq!(reply.exit, Exit::Success);
    let slots: Vec<&str> = reply
        .text
        .lines()
        .filter(|line| line.starts_with("slot ") || line.starts_with("hold "))
        .collect();
    assert_eq!(
        slots,
        [
            "hold     claude CLI hold for 2m (not_logged_in): log in",
            "slot     claude 1  turn 3 running for 4m  T.1  cli-claude-00000001",
            "slot     claude 2  parked for 2h after turn 5  T.2  cli-claude-00000002",
            "slot     claude 3  free",
        ]
    );
    assert!(!reply.text.lines().any(|line| line.starts_with("mode ")));

    let status = crate::pool::PauseSection {
        headless_dispatch: rig.headless_dispatch.clone(),
        hub: rig.hub.clone(),
        clock: rig.clock.clone(),
    };
    assert!(status.lines().iter().any(|line| line == slots[0]));
}

fn line(turn: u32, index: u32, kind: TranscriptKind, text: &str) -> TranscriptLine {
    TranscriptLine {
        session: session("cli-claude-00000001"),
        ticket: Some(ticket_id("T.1")),
        turn,
        index,
        kind,
        text: text.to_owned(),
        at: None,
        call: None,
    }
}

#[test]
fn pool_watch_prints_the_lines_after_the_ones_already_shown() {
    let rig = rig(&[]);
    let fake = cli(&rig, 1);
    let lines = vec![
        line(1, 0, TranscriptKind::Assistant, "starting"),
        line(1, 1, TranscriptKind::Tool, "Bash: ignite status"),
        line(1, 2, TranscriptKind::Result, "success: done"),
        line(2, 0, TranscriptKind::Assistant, "again"),
    ];
    fake.set_digest("T.1", &session("cli-claude-00000001"), lines);

    let first = rig.pool(&["watch", "T.1"], None);
    assert_eq!(
        first.text,
        "turn     1\nsay      starting\ntool     Bash: ignite status\nresult   success: done\n\
         turn     2\nsay      again"
    );
    assert_eq!(first.json.as_ref().unwrap()["next"], 4);

    let after = rig.pool(&["watch", "T.1", "--after", "3"], None);
    assert_eq!(after.text, "turn     2\nsay      again");
    let nothing_new = rig.pool(&["watch", "T.1", "--after", "4"], None);
    assert_eq!(nothing_new.exit, Exit::Success);
    assert_eq!(nothing_new.text, "");

    let unknown = rig.pool(&["watch", "T.9"], None);
    assert_ne!(unknown.exit, Exit::Success);
    assert!(unknown.text.contains("no CLI session"));
}
