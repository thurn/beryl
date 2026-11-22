//! The game screen's furniture: the step stops about each player's orb, and the commands a
//! press on one sends; and the game log, which folds to a button when a permanent reaches into
//! its column.

use app_client::Input;
use app_client::command::{Request, SessionCommand};
use beryl_protocol::StopsSetting;
use beryl_protocol::protocol::{CardType, GameEvent, SessionId, Side, Step, ZoneName};
use beryl_protocol::testing::ViewBuilder;
use kit_frame::frame::{Frame, Role, StateBits};
use kit_ui::input::PointerPhase;

use beryl_protocol::prompt::ButtonId;

use super::flat_layer::{centre, client, deciding, edges, find, pointer, prompt, prompt_button};

/// Whether `frame` has an element of `role` named `name`.
fn has(frame: &Frame, role: Role, name: &str) -> bool {
    (frame.resolved.iter()).any(|element| element.role == role && frame.name(element.name) == name)
}

#[test]
fn pressing_the_opponents_draw_step_sets_that_stop_and_draws_it_filled() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    let draw = find(frame, Role::Toggle, "Opponent's draw");
    assert!(!frame.resolved[draw].state.has(StateBits::SELECTED));
    let at = centre(frame, draw);
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));

    let mut want = StopsSetting::SMART;
    want.theirs = beryl_protocol::protocol::StepSet::of(&[
        Step::DeclareAttackers.into(),
        Step::DeclareBlockers.into(),
        Step::End.into(),
        Step::Draw.into(),
    ]);
    assert_eq!(
        client.next_command(),
        Some(SessionCommand::SetStops(SessionId(1), want))
    );
    let saved = std::iter::from_fn(|| client.next_request()).find_map(|request| match request {
        Request::SaveSettings(settings) => Some(settings.stops),
        _ => None,
    });
    assert_eq!(saved, Some(want));

    let frame = client.frame(1_000_000);
    let draw = find(frame, Role::Toggle, "Opponent's draw");
    assert!(frame.resolved[draw].state.has(StateBits::SELECTED));
    // The viewer's own draw step is a stop of its own, and stays unset.
    let yours = find(frame, Role::Toggle, "Your draw");
    assert!(!frame.resolved[yours].state.has(StateBits::SELECTED));
}

#[test]
fn the_damage_icon_sets_both_combat_damage_steps() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    let at = centre(frame, find(frame, Role::Toggle, "Your damage"));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    let Some(SessionCommand::SetStops(_, stops)) = client.next_command() else {
        panic!("no stops sent");
    };
    assert!(stops.yours.has(Step::FirstStrikeDamage.into()));
    assert!(stops.yours.has(Step::CombatDamage.into()));
}

/// The sample table while the opponent acts: the viewer has no decision open.
fn waiting() -> Input {
    Input::Update(Box::new(ViewBuilder::sample().update()))
}

/// Presses the toggle named `name`, as the last frame drew it.
fn press_toggle(client: &mut app_client::Client, now: u64, name: &str) {
    let frame = client.frame(now);
    let at = centre(frame, find(frame, Role::Toggle, name));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
}

/// The stops commands the client has queued.
fn stops_sent(client: &mut app_client::Client) -> Vec<StopsSetting> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::SetStops(_, stops) => Some(stops),
            _ => None,
        })
        .collect()
}

#[test]
fn a_stop_set_while_the_opponent_acts_is_sent_at_the_next_decision() {
    // The session takes stops only at the viewer's decision, and refuses them otherwise.
    let mut client = client();
    client.handle(waiting());
    press_toggle(&mut client, 0, "Opponent's draw");
    assert_eq!(stops_sent(&mut client), []);

    client.handle(deciding(prompt("Pay {1}{U}")));
    let sent = stops_sent(&mut client);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].theirs.has(Step::Draw.into()));
    // Frames between updates do not resend the command.
    client.frame(1_000_000);
    assert_eq!(stops_sent(&mut client), []);
}

#[test]
fn a_new_game_takes_up_the_saved_stops_at_its_first_decision() {
    let mut client = client();
    let mut settings = app_client::settings::Settings::default();
    settings.stops.full_control = true;
    client.handle(Input::Settings(settings));
    assert_eq!(stops_sent(&mut client), []);
    client.handle(deciding(prompt("Pay {1}{U}")));
    let sent = stops_sent(&mut client);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].full_control);
}

#[test]
fn a_stop_set_and_cleared_in_one_decision_sends_both_changes() {
    // The latest view still holds the stops from before the first press, so the second press,
    // which matches them, must still be sent: the game holds the first press's.
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    press_toggle(&mut client, 0, "Opponent's draw");
    press_toggle(&mut client, 1_000_000, "Opponent's draw");
    assert_eq!(
        stops_sent(&mut client).last(),
        Some(&StopsSetting::SMART),
        "the game is told the stop is cleared again"
    );
}

#[test]
fn an_earlier_stop_acknowledgement_keeps_the_later_change_pending() {
    let decision = || deciding(prompt("Pay {1}{U}"));
    let mut client = client();
    client.handle(decision());
    press_toggle(&mut client, 0, "Opponent's draw");
    press_toggle(&mut client, 1_000_000, "Opponent's draw");
    let sent = stops_sent(&mut client);
    assert_eq!(sent.len(), 2);

    for stops in sent {
        let Input::Update(mut update) = decision() else {
            unreachable!("a decision is sent in a game update");
        };
        update.view.stops = stops;
        client.handle(Input::Update(update));
        assert_eq!(stops_sent(&mut client), []);
    }
}

#[test]
fn untap_and_cleanup_show_an_icon_but_no_toggle() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    for whose in ["Your", "Opponent's"] {
        for step in ["untap", "cleanup"] {
            let name = format!("{whose} {step}");
            assert!(has(frame, Role::Icon, &name), "{name} has an icon");
            assert!(!has(frame, Role::Toggle, &name), "{name} is no toggle");
        }
    }
    // The other ten steps of each player are toggles.
    let toggles = (frame.resolved.iter())
        .filter(|element| element.role == Role::Toggle)
        .count();
    assert_eq!(toggles, 20);
}

/// A view of `lands` lands on each side of the table, besides a creature each, whose events say
/// the viewer gained 2 life.
fn lands(lands: usize) -> Input {
    let mut builder = ViewBuilder::new();
    for side in [Side::You, Side::Opponent] {
        for _ in 0..lands {
            builder.card(ZoneName::Battlefield, side, "Plains").types = vec![CardType::Land];
        }
        builder
            .card(ZoneName::Battlefield, side, "Serra Angel")
            .types = vec![CardType::Creature];
    }
    let mut update = builder.update();
    update.view.events = vec![GameEvent::LifeGain {
        player: Side::You,
        amount: 2,
    }];
    Input::Update(Box::new(update))
}

#[test]
fn the_log_is_a_button_until_it_is_opened() {
    let mut client = client();
    client.handle(lands(0));
    let frame = client.frame(0);
    assert!(!has(frame, Role::List, "Game log"), "the log starts open");
    let at = centre(frame, find(frame, Role::Button, "Show log"));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    let frame = client.frame(100_000_000);
    assert!(has(frame, Role::Button, "Hide log"));
    assert!(has(frame, Role::Text, "You gained 2 life"));
}

#[test]
fn the_recording_driver_reports_a_features_live_state() {
    use app_client::driver::{Page, Recording};
    use beryl_protocol::ipc::FixtureMessage;
    use kit_ui::input::Size;

    let Input::Update(update) = lands(0) else {
        unreachable!("lands are sent in a view update");
    };
    let recording = Recording(vec![FixtureMessage::game_update(&update)]);
    let mut page = Page::new(
        crate::test_data::data(),
        recording.into(),
        Side::You,
        Size {
            width: 1440.0,
            height: 900.0,
        },
    )
    .expect("the view can be read");
    assert!(page.feature_state("unknown").is_none());
    let closed = page.feature_state("game-log").expect("registered game log");
    page.press("button \"Show log\"");
    let opened = page.feature_state("game-log").expect("registered game log");
    assert_ne!(closed, opened);
    page.press("button \"Hide log\"");
    assert_eq!(page.feature_state("game-log"), Some(closed));
}

/// A deciding view whose prompt has a turn button and, if `pass_on`, the round button that passes
/// on, with full control on in the stops the game holds.
fn full_control_on(pass_on: bool) -> Input {
    let mut view = prompt("");
    view.buttons = vec![prompt_button(ButtonId::Decline, "Next", true)];
    if pass_on {
        view.buttons
            .push(prompt_button(ButtonId::PassOn, "To End Turn", false));
    }
    let Input::Update(mut update) = deciding(view) else {
        unreachable!("deciding is an update")
    };
    update.view.stops.full_control = true;
    Input::Update(update)
}

#[test]
fn full_controls_sign_stands_left_of_the_turn_button_and_moves_over_for_pass_on() {
    let mut client = client();
    let frame = client.frame(0);
    assert!(
        !has(frame, Role::Icon, "Full control"),
        "no sign before a game"
    );
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(1_000_000);
    assert!(
        !has(frame, Role::Icon, "Full control"),
        "no sign while full control is off"
    );

    // As the web's `.full-control` stands it: 12 points left of the turn button, centred on it.
    client.handle(full_control_on(false));
    let frame = client.frame(2_000_000);
    let next = edges(frame, find(frame, Role::Button, "Next"));
    let sign = edges(frame, find(frame, Role::Icon, "Full control"));
    assert!(
        (next.0 - sign.2 - 12.0).abs() < 0.01,
        "{sign:?} by {next:?}"
    );
    assert!(
        (sign.1 - next.1 - 10.0).abs() < 0.01,
        "{sign:?} by {next:?}"
    );

    // With the round pass-on button left of the turn button, the sign stands 12 points left of
    // that instead.
    client.handle(full_control_on(true));
    let frame = client.frame(3_000_000);
    let pass_on = edges(frame, find(frame, Role::Button, "To End Turn"));
    let sign = edges(frame, find(frame, Role::Icon, "Full control"));
    assert!(
        (pass_on.0 - sign.2 - 12.0).abs() < 0.01,
        "{sign:?} by {pass_on:?}"
    );
}
