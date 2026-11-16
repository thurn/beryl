//! The game screen's furniture: the step stops about each player's orb, and the commands a
//! press on one sends; and the game log, which folds to a button when a permanent reaches into
//! its column.

use beryl_protocol::StopsSetting;
use beryl_protocol::protocol::{CardType, GameEvent, SessionId, Side, Step, ZoneName};
use beryl_scene::command::{Request, SessionCommand};
use beryl_scene::frame::{Frame, Role, StateBits};
use beryl_scene::input::{Input, PointerPhase};
use beryl_scene::testing::ViewBuilder;

use super::flat_layer::{centre, client, deciding, find, pointer, prompt};

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
fn press_toggle(client: &mut beryl_scene::Client, now: u64, name: &str) {
    let frame = client.frame(now);
    let at = centre(frame, find(frame, Role::Toggle, name));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
}

/// The stops commands the client has queued.
fn stops_sent(client: &mut beryl_scene::Client) -> Vec<StopsSetting> {
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
    // Sent once: a later view of the same game, still holding the old stops because the
    // session has not drawn one since, sends them again only at that view.
    client.frame(1_000_000);
    assert_eq!(stops_sent(&mut client), []);
}

#[test]
fn a_new_game_takes_up_the_saved_stops_at_its_first_decision() {
    let mut client = client();
    let mut settings = beryl_scene::settings::Settings::default();
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
fn the_log_folds_to_a_button_when_a_permanent_enters_its_column() {
    let mut client = client();
    client.handle(lands(0));
    let frame = client.frame(0);
    assert!(has(frame, Role::Text, "You gained 2 life"));
    assert!(!has(frame, Role::Button, "Show log"));

    // A long row of lands runs from the table's left edge into the log's column.
    client.handle(lands(14));
    let frame = client.frame(10_000_000_000);
    assert!(
        !has(frame, Role::List, "Game log"),
        "the folded log is open"
    );
    let at = centre(frame, find(frame, Role::Button, "Show log"));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    let frame = client.frame(10_100_000_000);
    assert!(has(frame, Role::Button, "Hide log"));
    assert!(has(frame, Role::Text, "You gained 2 life"));
}
