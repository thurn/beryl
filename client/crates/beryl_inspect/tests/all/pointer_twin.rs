//! Playing or casting a non-offered hand twin with a distinct Secret Lair printing, from either
//! end of the fan and on both desktop and phone layouts.

use app_client::Input;
use app_client::command::SessionCommand;
use app_client::driver::{Responses, plays_on};
use app_client::{Client, Config};
use beryl_inspect::pages::recording;
use beryl_protocol::card_records::PrintingIx;
use beryl_protocol::protocol::{ObjectId, PlayerView, Side, Why};
use beryl_protocol::script::Act;
use glam::Vec2;
use kit_frame::frame::{Frame, Resolved, Role, StateBits, Wake};
use kit_scene::hit::uncovered;
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size};

const MILLISECOND: u64 = 1_000_000;
const DRAG: Vec2 = Vec2::new(180.0, -240.0);

fn crossing([a, b, c, d]: [Vec2; 4]) -> Vec2 {
    let (r, s) = (c - a, d - b);
    let t = (b - a).perp_dot(s) / r.perp_dot(s);
    a + r * t
}

fn settle(client: &mut Client, now: &mut u64) {
    while {
        client.frame(*now);
        client.wake() != Wake::Never
    } {
        *now += 16 * MILLISECOND;
    }
}

fn card_slot(frame: &Frame, slot: ObjectId) -> Resolved {
    *frame
        .resolved
        .iter()
        .find(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0))
        .unwrap_or_else(|| panic!("no card in slot {}", slot.0))
}

fn press_point(frame: &Frame, slot: ObjectId) -> Vec2 {
    let index = frame
        .resolved
        .iter()
        .position(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0))
        .expect("the card is drawn");
    let visible = uncovered(frame, u32::try_from(index).unwrap()).expect("the card can be pressed");
    Vec2::new(
        visible.x + visible.width / 2.0,
        visible.y + visible.height / 2.0,
    )
}

fn send_pointer(client: &mut Client, kind: PointerKind, phase: PointerPhase, position: Vec2) {
    client.handle(Input::Device(DeviceInput::Pointer(Pointer {
        id: 0,
        kind,
        phase,
        position,
        button: Button::Primary,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    })));
}

fn take_commands(client: &mut Client) -> Vec<SessionCommand> {
    std::iter::from_fn(|| client.next_command()).collect()
}

fn art(view: &PlayerView, slot: ObjectId) -> Option<PrintingIx> {
    view.objects
        .iter()
        .find(|o| o.slot == slot)
        .expect("slot present in view")
        .printing
}

/// Playing a hand twin whose Secret Lair printing differs from the engine's offered candidate
/// (Forgetful Fish turn 4) starts the flight from the dragged or clicked card and keeps the
/// other Islands where they were, on both desktop and phone layouts.
#[test]
fn playing_a_hand_twin_with_different_printing_preserves_pose_and_art() {
    let [offered, chosen] = [ObjectId(7), ObjectId(272)];
    let layouts = [
        (
            "desktop",
            Size {
                width: 1600.0,
                height: 900.0,
            },
            PointerKind::Mouse,
            DRAG,
        ),
        (
            "phone",
            Size {
                width: 393.0,
                height: 759.0,
            },
            PointerKind::Touch,
            Vec2::new(0.0, -280.0),
        ),
    ];
    for (label, size, kind, drag_delta) in layouts {
        for drag in [true, false] {
            let rec = recording("hand-pair-twin-printings", Side::You)
                .unwrap_or_else(|why| panic!("{why}"));
            let views = rec.views().unwrap_or_else(|why| panic!("{why}"));
            let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
            client.record();
            client.handle(Input::Device(DeviceInput::Surface {
                size,
                scale: 1.0,
                safe_area: Insets::default(),
            }));
            let at_prompt = views.len() - 2;
            for update in &views[..=at_prompt] {
                client.handle(Input::Update(Box::new(update.clone())));
            }
            let mut now = 0;
            settle(&mut client, &mut now);

            let offered_art = art(&views[at_prompt].view, offered);
            let chosen_art = art(&views[at_prompt].view, chosen);
            assert!(offered_art.is_some() && chosen_art.is_some() && offered_art != chosen_art);

            let _ = take_commands(&mut client);
            let frame = client.frame(now);
            let other = crossing(card_slot(frame, offered).corners);
            let start = press_point(frame, chosen);
            send_pointer(&mut client, kind, PointerPhase::Down, start);
            let flight_from = if drag {
                let at = start + drag_delta;
                send_pointer(&mut client, kind, PointerPhase::Move, at);
                assert!(
                    card_slot(client.frame(now), chosen)
                        .state
                        .has(StateBits::HELD),
                    "{label}: dragged Island is held"
                );
                send_pointer(&mut client, kind, PointerPhase::Up, at);
                at
            } else {
                let from = crossing(card_slot(client.frame(now), chosen).corners);
                send_pointer(&mut client, kind, PointerPhase::Up, start);
                from
            };
            client.frame(now);
            let commands = take_commands(&mut client);
            let [command] = &commands[..] else {
                panic!("{label} (drag={drag}): expected one click, got {commands:?}");
            };
            let answer = Responses::new(&rec)
                .answer(command)
                .unwrap_or_else(|why| panic!("{why}"));
            assert!(answer.plays_on);
            client.handle(Input::Reply(answer.reply));
            for update in &views[at_prompt + 1..] {
                client.handle(Input::Update(Box::new(update.clone())));
            }
            let frame = client.frame(now);
            let flying = card_slot(frame, offered);
            assert!(flying.state.has(StateBits::MOVING), "{label} (drag={drag})");
            assert!(crossing(flying.corners).distance(flight_from) < 0.01);
            assert!(crossing(card_slot(frame, chosen).corners).distance(other) < 0.01);

            let after = &views[at_prompt + 1].view;
            assert_eq!(art(after, offered), chosen_art);
            assert_eq!(art(after, chosen), offered_art);
        }
    }
}

/// Dragging a non-offered twin from the left end of the fan, and casting a non-offered instant
/// twin (`Why::Cast`), both start the flight from the dragged card even with distinct printings.
#[test]
fn playing_a_left_end_twin_and_casting_an_instant_twin_start_from_the_handled_card() {
    let [offered, chosen] = [ObjectId(0), ObjectId(2)];
    for cast_instant in [false, true] {
        let rec = recording("hand-pair-twin", Side::You).unwrap_or_else(|why| panic!("{why}"));
        let mut views = rec.views().unwrap_or_else(|why| panic!("{why}"));
        for update in &mut views {
            for o in &mut update.view.objects {
                if o.slot == offered {
                    o.printing = Some(PrintingIx(6));
                    if cast_instant {
                        o.name = Some("Brainstorm".to_owned());
                    }
                } else if o.slot == chosen {
                    o.printing = Some(PrintingIx(7));
                    if cast_instant {
                        o.name = Some("Brainstorm".to_owned());
                    }
                }
            }
        }
        if cast_instant {
            for update in &mut views[1..] {
                if let Some(script) = &mut update.view.script {
                    for step in script.beats.iter_mut().flatten() {
                        if let Act::Fly { why, .. } = &mut step.act {
                            *why = Why::Cast;
                        }
                    }
                }
            }
        }
        let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
        client.record();
        client.handle(Input::Device(DeviceInput::Surface {
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
        }));
        if !cast_instant {
            // Put `chosen` in hand first so `offered` arrives after it at the right end of the
            // fan, placing the non-offered twin `chosen` at the left end.
            let mut first = views[0].clone();
            first.view.objects.retain(|o| o.slot != offered);
            client.handle(Input::Update(Box::new(first)));
        }
        client.handle(Input::Update(Box::new(views[0].clone())));
        let mut now = 0;
        settle(&mut client, &mut now);

        let frame = client.frame(now);
        if !cast_instant {
            assert!(
                crossing(card_slot(frame, chosen).corners).x
                    < crossing(card_slot(frame, offered).corners).x
            );
        }
        let other = crossing(card_slot(frame, offered).corners);
        let start = press_point(frame, chosen);
        let at = start + DRAG;
        send_pointer(&mut client, PointerKind::Mouse, PointerPhase::Down, start);
        send_pointer(&mut client, PointerKind::Mouse, PointerPhase::Move, at);
        assert!(
            card_slot(client.frame(now), chosen)
                .state
                .has(StateBits::HELD)
        );
        send_pointer(&mut client, PointerKind::Mouse, PointerPhase::Up, at);
        client.frame(now);
        let commands = take_commands(&mut client);
        let [command] = &commands[..] else {
            panic!("dropping the twin sends one click");
        };
        let answer = Responses::new(&rec)
            .answer(command)
            .unwrap_or_else(|why| panic!("{why}"));
        client.handle(Input::Reply(answer.reply));
        for update in &views[1..] {
            client.handle(Input::Update(Box::new(update.clone())));
            if !plays_on(update) {
                break;
            }
        }
        let frame = client.frame(now);
        let flying = card_slot(frame, offered);
        assert!(flying.state.has(StateBits::MOVING));
        assert!(crossing(flying.corners).distance(at) < 0.01);
        assert!(crossing(card_slot(frame, chosen).corners).distance(other) < 0.01);
    }
}
