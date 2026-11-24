use beryl_session::ipc::{self, FixtureMessage};
use beryl_session::prompt::{ButtonId, Click, ClickReply, ClickRequest};
use beryl_session::script::Subject;
use beryl_session::{GameUpdate, Status};

use crate::common::TEST;
use beryl_session::prompt::ShapeBody;

/// A recording keeps each click with the session's reply to it, and after a click that
/// answered, the views up to the human's next decision: what a replay answers the UI's
/// own clicks from.
#[test]
fn a_recording_holds_its_clicks_and_what_they_led_to() {
    let def: beryl_cli::fixture::Definition = serde_json::from_value(serde_json::json!({
        "name": "t",
        "board": "shape-pick",
        "until": "first priority decision for seat 0",
        "clicks": ["Counterspell", "Island", "press decline"],
    }))
    .unwrap();
    let messages = beryl_cli::fixture::record_game(&TEST, &def, false).unwrap();
    let replies: Vec<(ClickRequest, ClickReply)> = (messages.iter())
        .filter_map(|message| match message {
            FixtureMessage::Response {
                command,
                request: Some(request),
                value,
            } if command == ipc::CLICK => Some((
                serde_json::from_value(request.clone()).unwrap(),
                serde_json::from_value(value.clone()).unwrap(),
            )),
            _ => None,
        })
        .collect();
    let [(nothing, same), (land, played), (pass, passed)] = &replies[..] else {
        panic!("three clicks, not {replies:?}");
    };
    // A card with nothing to do leaves the prompt as it was; a land is played by the
    // click; and the button passes, each on the decision then open.
    assert!(matches!(same, ClickReply::Open { prompt } if prompt.revision == nothing.revision));
    assert!(matches!(nothing.click, Click::On { .. }));
    assert_eq!(*played, ClickReply::Answered);
    assert_eq!(nothing.decision, land.decision);
    assert_eq!(*passed, ClickReply::Answered);
    assert_ne!(pass.decision, land.decision);
    assert_eq!(
        pass.click,
        Click::Button {
            button: ButtonId::Decline
        }
    );
    let Some(FixtureMessage::Event { payload, .. }) = messages.last() else {
        panic!("a recording ends on a view");
    };
    let update: GameUpdate = serde_json::from_value(payload.clone()).unwrap();
    assert!(matches!(update.view.status, Status::Deciding { .. }));
}

/// A board's own play may lead on from its scripts to the stop, the stand-in answering
/// what the human is asked on the way (a cost to pay, a pass): the clicks begin at the
/// stop, and a drop there is the session's to place.
#[test]
fn clicks_begin_at_the_stop_a_boards_play_leads_to() {
    let def: beryl_cli::fixture::Definition = serde_json::from_value(serde_json::json!({
        "name": "t",
        "board": "shape-split",
        "until": "first chooseUpTo decision for seat 0",
        "clicks": ["drop 0 0 Island", "press confirm"],
    }))
    .unwrap();
    let messages = beryl_cli::fixture::record_game(&TEST, &def, false).unwrap();
    let replies: Vec<(ClickRequest, ClickReply)> = (messages.iter())
        .filter_map(|message| match message {
            FixtureMessage::Response {
                command,
                request: Some(request),
                value,
            } if command == ipc::CLICK => Some((
                serde_json::from_value(request.clone()).unwrap(),
                serde_json::from_value(value.clone()).unwrap(),
            )),
            _ => None,
        })
        .collect();
    let [(drop, sorted), (_, done)] = &replies[..] else {
        panic!("two clicks, not {replies:?}");
    };
    let Click::Drop { on, pile: 0, at: 0 } = drop.click else {
        panic!("a drop into the first pile, not {:?}", drop.click);
    };
    let ClickReply::Open { prompt } = sorted else {
        panic!("the scry stays open, not {sorted:?}");
    };
    let ShapeBody::Split { piles } = &prompt.body else {
        panic!("a split, not {:?}", prompt.body);
    };
    let Subject::Object { slot } = on else {
        panic!("a card, not {on:?}");
    };
    assert_eq!(piles[0].cards, [slot]);
    assert_eq!(*done, ClickReply::Answered);
}

/// A seat its board's scripts still play is not the recording's to click for.
#[test]
fn clicks_wait_for_the_boards_scripts_to_end() {
    let def: beryl_cli::fixture::Definition = serde_json::from_value(serde_json::json!({
        "name": "t",
        "board": "shape-pick-theirs",
        "until": "first priority decision for seat 0",
        "clicks": ["press decline"],
    }))
    .unwrap();
    let refused = beryl_cli::fixture::record_game(&TEST, &def, false).unwrap_err();
    assert!(refused.contains("scripts must end"), "{refused}");
}
