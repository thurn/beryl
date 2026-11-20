use super::*;
use crate::cli::{Hold, HoldReason, PreviousAnswer, Slot, SlotState, TurnEnd, Verb};
use crate::ids::SessionId;
use crate::time::Timestamp;

const NOW: Timestamp = Timestamp::from_millis(1_800_000_000_000);

fn ticket() -> TicketId {
    "TQ44S.9".parse().unwrap()
}

fn slot(state: SlotState) -> Slot {
    Slot {
        app: "claude".to_owned(),
        index: 2,
        session: Some(SessionId::new("cli-claude-0123abcd").unwrap()),
        ticket: Some(ticket()),
        state,
        turn: 3,
        transcript: Some("transcript".to_owned()),
        model: Some("model".to_owned()),
        effort: None,
        since: Some(NOW),
    }
}

/// A clean slate: the ticket held, nothing running, nothing waiting.
fn facts() -> Facts {
    Facts {
        now: NOW,
        ticket_held: true,
        running_verbs: Vec::new(),
        land_pushing: false,
        messages: Vec::new(),
        interrupted_in_row: 1,
        previous: PreviousAnswer::Other,
        hold: None,
        last_text: Some("Half done.".to_owned()),
    }
}

fn verb(command: &str, handle: &str) -> Verb {
    Verb {
        command: command.to_owned(),
        handle: handle.to_owned(),
    }
}

fn on_hold() -> Hold {
    Hold {
        app: "claude".to_owned(),
        reason: HoldReason::NotLoggedIn,
        since: NOW,
        cli_version: None,
        message: "not logged in".to_owned(),
    }
}

fn api_error() -> TurnEnd {
    TurnEnd::Hold {
        reason: HoldReason::ApiError,
        message: "529 overloaded".to_owned(),
    }
}

fn message() -> String {
    "Message about TQ44S.9: rebase first.".to_owned()
}

fn wait_on(command: &str, handle: &str) -> Action {
    Action::WaitForVerb {
        handle: handle.to_owned(),
        prompt: format!("`{command} {handle}` finished: run `ignite wait {handle}`."),
    }
}

fn resume(prompt: impl Into<String>) -> Action {
    Action::Resume {
        prompt: prompt.into(),
    }
}

/// What a row's action must be: exactly this, or only of this kind when its text is a report
/// or a note whose wording the row does not pin.
enum Expect {
    Is(Action),
    Parks,
    Releases(&'static str),
}

#[test]
#[allow(clippy::too_many_lines)] // One row per rule reads best as one table.
fn every_rule_and_boundary_chooses_its_action() {
    use Expect::{Is, Parks, Releases};
    let running = slot(SlotState::Running);
    let rows: Vec<(&str, Slot, TurnEnd, Facts, Expect)> = vec![
        (
            "clean end, ticket no longer held: free",
            running.clone(),
            TurnEnd::Clean,
            Facts {
                ticket_held: false,
                ..facts()
            },
            Is(Action::Free),
        ),
        (
            "interrupted end, ticket no longer held: free",
            running.clone(),
            TurnEnd::Interrupted,
            Facts {
                ticket_held: false,
                interrupted_in_row: 5,
                ..facts()
            },
            Is(Action::Free),
        ),
        (
            "clean end, a verb runs: wait for it",
            running.clone(),
            TurnEnd::Clean,
            Facts {
                running_verbs: vec![verb("test", "T-12")],
                messages: vec![message()],
                previous: PreviousAnswer::BlockSent,
                ..facts()
            },
            Is(wait_on("test", "T-12")),
        ),
        (
            "clean end, messages wait: resume with them",
            running.clone(),
            TurnEnd::Clean,
            Facts {
                messages: vec![message(), "Message about TQ44S.9: and test.".to_owned()],
                previous: PreviousAnswer::BlockSent,
                ..facts()
            },
            Is(resume(format!(
                "{}\n\nMessage about TQ44S.9: and test.",
                message()
            ))),
        ),
        (
            "first clean end with the ticket held: the block text",
            running.clone(),
            TurnEnd::Clean,
            facts(),
            Is(resume(block_text(&ticket()))),
        ),
        (
            "first clean end from a starting slot: the block text",
            slot(SlotState::Starting),
            TurnEnd::Clean,
            facts(),
            Is(resume(block_text(&ticket()))),
        ),
        (
            "second clean end with the ticket held: release with the last message",
            running.clone(),
            TurnEnd::Clean,
            Facts {
                previous: PreviousAnswer::BlockSent,
                ..facts()
            },
            Is(Action::Release {
                note: release_reason(Some("Half done.")),
            }),
        ),
        (
            "second clean end while a land pushes: wait for the land",
            running.clone(),
            TurnEnd::Clean,
            Facts {
                previous: PreviousAnswer::BlockSent,
                land_pushing: true,
                running_verbs: vec![verb("land", "L-5521")],
                ..facts()
            },
            Is(wait_on("land", "L-5521")),
        ),
        (
            "clean end with the ticket held but none on the slot: park",
            Slot {
                ticket: None,
                ..running.clone()
            },
            TurnEnd::Clean,
            facts(),
            Parks,
        ),
        (
            "4 interrupted ends in a row: resume",
            running.clone(),
            TurnEnd::Interrupted,
            Facts {
                interrupted_in_row: 4,
                ..facts()
            },
            Is(resume(INTERRUPTED_TEXT)),
        ),
        (
            "interrupted end with messages: resume with them after the note",
            running.clone(),
            TurnEnd::Interrupted,
            Facts {
                messages: vec![message()],
                ..facts()
            },
            Is(resume(format!("{INTERRUPTED_TEXT}\n\n{}", message()))),
        ),
        (
            "5 interrupted ends in a row: release",
            running.clone(),
            TurnEnd::Interrupted,
            Facts {
                interrupted_in_row: 5,
                ..facts()
            },
            Releases("interrupted 5 times in a row"),
        ),
        (
            "5 interrupted ends while a land pushes: wait for the land",
            running.clone(),
            TurnEnd::Interrupted,
            Facts {
                interrupted_in_row: 5,
                land_pushing: true,
                running_verbs: vec![verb("test", "T-1"), verb("land", "L-7")],
                ..facts()
            },
            Is(wait_on("land", "L-7")),
        ),
        (
            "a push has begun but no land is listed: park, never release",
            running.clone(),
            TurnEnd::PlanLimit { window: None },
            Facts {
                land_pushing: true,
                ..facts()
            },
            Parks,
        ),
        (
            "plan limit: release naming the window",
            running.clone(),
            TurnEnd::PlanLimit {
                window: Some("five_hour".to_owned()),
            },
            facts(),
            Releases("limit was reached in `five_hour`"),
        ),
        (
            "first API error: back off 30 s",
            running.clone(),
            api_error(),
            facts(),
            Is(Action::BackOff {
                at: NOW.plus_seconds(30.0),
                prompt: API_ERROR_TEXT.to_owned(),
            }),
        ),
        (
            "second API error in a row: hold",
            running.clone(),
            api_error(),
            Facts {
                previous: PreviousAnswer::BackedOff,
                ..facts()
            },
            Is(Action::Hold {
                reason: HoldReason::ApiError,
                message: "529 overloaded".to_owned(),
            }),
        ),
        (
            "API error while the app is on a hold: hold",
            running.clone(),
            api_error(),
            Facts {
                hold: Some(on_hold()),
                ..facts()
            },
            Is(Action::Hold {
                reason: HoldReason::ApiError,
                message: "529 overloaded".to_owned(),
            }),
        ),
        (
            "not logged in: hold at once",
            slot(SlotState::Starting),
            TurnEnd::Hold {
                reason: HoldReason::NotLoggedIn,
                message: "please log in".to_owned(),
            },
            facts(),
            Is(Action::Hold {
                reason: HoldReason::NotLoggedIn,
                message: "please log in".to_owned(),
            }),
        ),
        (
            "a hold's end holds even when the ticket is gone",
            running.clone(),
            TurnEnd::Hold {
                reason: HoldReason::ProgramMissing,
                message: "no such file".to_owned(),
            },
            Facts {
                ticket_held: false,
                ..facts()
            },
            Is(Action::Hold {
                reason: HoldReason::ProgramMissing,
                message: "no such file".to_owned(),
            }),
        ),
        (
            "interrupted end while the app is on a hold: park, start nothing",
            running.clone(),
            TurnEnd::Interrupted,
            Facts {
                hold: Some(on_hold()),
                ..facts()
            },
            Parks,
        ),
        (
            "a verb runs while the app is on a hold: still wait for it",
            running.clone(),
            TurnEnd::Clean,
            Facts {
                hold: Some(on_hold()),
                running_verbs: vec![verb("test", "T-3")],
                ..facts()
            },
            Is(wait_on("test", "T-3")),
        ),
        (
            "unknown end: park",
            running.clone(),
            TurnEnd::Unknown {
                why: "exit 139".to_owned(),
            },
            Facts {
                ticket_held: false,
                previous: PreviousAnswer::BlockSent,
                interrupted_in_row: 9,
                ..facts()
            },
            Parks,
        ),
        (
            "a turn ends while the slot is free: park",
            slot(SlotState::Free),
            TurnEnd::Clean,
            Facts {
                previous: PreviousAnswer::BlockSent,
                ..facts()
            },
            Parks,
        ),
        (
            "a turn ends while the slot waits on a verb: park",
            slot(SlotState::WaitingVerb),
            TurnEnd::Interrupted,
            Facts {
                interrupted_in_row: 5,
                ..facts()
            },
            Parks,
        ),
    ];
    for (name, slot, end, facts, expect) in rows {
        let action = decide(&slot, &end, &facts);
        match expect {
            Is(wanted) => assert_eq!(action, wanted, "{name}"),
            Parks => assert!(matches!(action, Action::Park { .. }), "{name}: {action:?}"),
            Releases(phrase) => match &action {
                Action::Release { note } => {
                    assert!(note.contains(phrase), "{name}: {note}");
                    assert!(
                        note.contains("Its last message: Half done."),
                        "{name}: {note}"
                    );
                }
                other => panic!("{name}: {other:?}"),
            },
        }
    }
}

#[test]
fn every_end_in_every_state_returns_an_action() {
    let ends = [
        TurnEnd::Clean,
        TurnEnd::Interrupted,
        TurnEnd::PlanLimit { window: None },
        TurnEnd::Hold {
            reason: HoldReason::ModelRefused,
            message: "refused".to_owned(),
        },
        api_error(),
        TurnEnd::Unknown {
            why: "?".to_owned(),
        },
    ];
    let variants = [
        facts(),
        Facts {
            ticket_held: false,
            ..facts()
        },
        Facts {
            previous: PreviousAnswer::BlockSent,
            land_pushing: true,
            interrupted_in_row: 0,
            hold: Some(on_hold()),
            last_text: None,
            ..facts()
        },
    ];
    for state in SlotState::ALL {
        for end in &ends {
            for facts in &variants {
                let action = decide(&slot(state), end, facts);
                let starts_now = matches!(action, Action::Resume { .. } | Action::BackOff { .. });
                let releases = matches!(action, Action::Release { .. });
                if !matches!(state, SlotState::Starting | SlotState::Running)
                    || matches!(end, TurnEnd::Unknown { .. })
                {
                    assert!(matches!(action, Action::Park { .. }), "{state} {end:?}");
                }
                if facts.hold.is_some() {
                    assert!(!starts_now, "{state} {end:?} started a turn during a hold");
                }
                if facts.land_pushing {
                    assert!(!releases, "{state} {end:?} released during a push");
                }
            }
        }
    }
}

#[test]
fn the_release_note_quotes_only_the_start_of_the_last_message() {
    let long = "x".repeat(NOTE_CHARS + 10);
    let reason = release_reason(Some(&long));
    assert!(reason.ends_with('…'));
    assert!(!release_reason(Some("  ")).contains("last message"));
}
