//! The game screen's furniture: the step stops about each player's orb, and the commands a
//! press on one sends.

use beryl_protocol::StopsSetting;
use beryl_protocol::protocol::{SessionId, Step};
use beryl_scene::command::{Request, SessionCommand};
use beryl_scene::frame::{Frame, Role, StateBits};
use beryl_scene::input::PointerPhase;

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
