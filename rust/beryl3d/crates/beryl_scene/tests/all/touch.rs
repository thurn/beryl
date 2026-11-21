//! A touch screen (the phone's gestures in docs/ui-conventions.md, "The phone"), driven through
//! `Client::handle` on an explicit clock: a tap uses a card the prompt can use and shows any
//! other whole, a finger held still shows its card whole, a hand card can be carried off at
//! once while a manual-table card waits for the finger to rest, a drag scrolls a row instead of
//! touching a card, a second finger cancels the first only if it lands with it, and a double
//! tap on bare felt or with two fingers opens the bug report.

use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::prompt::{ButtonId, Click, ClickRequest, Marked};
use beryl_protocol::protocol::{CardType, ObjectId, Side, Status, ZoneName};
use beryl_protocol::script::{Mark, Subject};
use beryl_scene::Input;
use beryl_scene::agent::{hit, uncovered};
use beryl_scene::command::{Request, SessionCommand};
use beryl_scene::driver::Recording;
use beryl_scene::frame::{
    ElementId, Frame, FrameBuilder, NameRef, Place, Resolved, Role, StateBits,
};
use beryl_scene::input::{Button, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Tier};
use beryl_scene::input::{Rect, Size};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, table};
use beryl_scene::{Client, Config};
use glam::Vec2;

use super::flat_layer::{centre, deciding, find, prompt};

/// A millisecond, in the nanoseconds `Client::frame` takes.
const MS: u64 = 1_000_000;

/// The window. A touch means the same in any layout; the phone's own table is laid out by other
/// code, and the sample table in a window this large leaves every card a strip to touch.
const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// A client given `update`, with the card records of its cards, drawn at time 0.
fn client(update: GameUpdate) -> Client {
    let message = fixture_message(&[FixtureMessage::game_update(&update)], TokenMakers::NONE);
    let records = Recording(vec![message])
        .card_records()
        .expect("the records can be read")
        .expect("the recording holds records");
    let mut client = Client::new(Config {
        elements: 2048,
        draws: 4096,
        name_bytes: 65_536,
    });
    client.handle(Input::Surface {
        size: WINDOW,
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(Input::Cards(Box::new(records)));
    client.handle(Input::Update(Box::new(update)));
    client.frame(0);
    client
}

/// The sample table, with the viewer paying a cost that may use the Counterspell in hand.
fn paying() -> Client {
    let Input::Update(mut update) = deciding(prompt("Pay {1}{U}")) else {
        unreachable!("`deciding` is an update")
    };
    let slot = (update.view.objects.iter())
        .position(|object| object.name.as_deref() == Some("Counterspell"))
        .expect("the sample has a Counterspell");
    if let Status::Deciding { decision } = &mut update.view.status {
        decision.prompt.marks = vec![Marked {
            on: object(slot),
            mark: Mark::Eligible,
        }];
    }
    client(*update)
}

fn object(slot: usize) -> Subject {
    Subject::Object {
        slot: ObjectId(u16::try_from(slot).expect("few cards")),
    }
}

/// Finger `id` at `at`.
fn finger(id: u8, phase: PointerPhase, at: Vec2) -> Input {
    Input::Pointer(Pointer {
        id,
        kind: PointerKind::Touch,
        phase,
        position: at,
        button: Button::Primary,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    })
}

/// A tap of finger `id` at `at`.
fn tap(client: &mut Client, id: u8, at: Vec2) {
    client.handle(finger(id, PointerPhase::Down, at));
    client.handle(finger(id, PointerPhase::Up, at));
}

/// The clicks the client queued since the last call, in order.
fn clicks(client: &mut Client) -> Vec<Click> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::Click(ClickRequest { click, .. }) => Some(click),
            _ => None,
        })
        .collect()
}

/// How many times the bug report was opened since the last call: each opening first freezes
/// the frame.
fn reports(client: &mut Client) -> usize {
    std::iter::from_fn(|| client.next_request())
        .filter(|request| *request == Request::FreezeFrame)
        .count()
}

/// The name of the card the inspector shows in `frame`, if it shows one.
fn inspected(frame: &Frame) -> Option<String> {
    (frame.resolved.iter()).find_map(|element| {
        let name = frame.name(element.name);
        (element.role == Role::Region)
            .then(|| name.strip_suffix(", inspected"))
            .flatten()
            .map(str::to_owned)
    })
}

/// The index and slot of the first card named `name` in `frame`.
fn card(frame: &Frame, name: &str) -> (u32, usize) {
    let index = (frame.resolved.iter())
        .position(|element| element.role == Role::Card && frame.name(element.name) == name)
        .unwrap_or_else(|| panic!("no card {name:?}"));
    let slot = frame.resolved[index].id.0 as usize;
    (u32::try_from(index).expect("few elements"), slot)
}

/// A point where a touch reaches the card named `name`: the middle of its uncovered part.
fn on(frame: &Frame, name: &str) -> Vec2 {
    let (index, _) = card(frame, name);
    let rect = uncovered(frame, index).expect("part of the card is uncovered");
    Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// A point of bare felt in `frame`: one where nothing is drawn, nearest the window's middle row.
fn felt(frame: &Frame) -> Vec2 {
    let steps = (0..40).flat_map(|row| (0..20).map(move |column| (row, column)));
    steps
        .map(|(row, column)| {
            let y =
                WINDOW.height / 2.0 + (if row % 2 == 0 { 1.0 } else { -1.0 }) * 9.0 * row as f32;
            Vec2::new(40.0 + 76.0 * column as f32, y)
        })
        .find(|point| hit(frame, *point).is_none())
        .expect("some of the table is bare felt")
}

#[test]
fn a_tap_uses_a_card_the_prompt_can_use_and_shows_any_other_whole() {
    let mut client = paying();
    let frame = client.frame(16 * MS).clone();
    let (_, counterspell) = card(&frame, "Counterspell");
    tap(&mut client, 1, on(&frame, "Counterspell"));
    client.frame(32 * MS);
    assert_eq!(
        clicks(&mut client),
        vec![Click::On {
            on: object(counterspell)
        }]
    );

    tap(&mut client, 1, on(&frame, "Serra Angel"));
    client.frame(48 * MS);
    let shown = client.frame(64 * MS).clone();
    assert_eq!(
        clicks(&mut client),
        Vec::new(),
        "the angel is no use to the prompt"
    );
    assert_eq!(inspected(&shown).as_deref(), Some("Serra Angel"));

    // The next touch only puts the card away; the one after shows a card of the hand.
    tap(&mut client, 1, on(&frame, "Plains"));
    let put_away = client.frame(80 * MS).clone();
    assert_eq!(inspected(&put_away), None);
    tap(&mut client, 1, on(&frame, "Plains"));
    client.frame(96 * MS);
    let shown = client.frame(112 * MS).clone();
    assert_eq!(inspected(&shown).as_deref(), Some("Plains"));
    tap(&mut client, 1, felt(&frame));
    let put_away = client.frame(128 * MS).clone();
    assert_eq!(inspected(&put_away), None);
    assert_eq!(clicks(&mut client), Vec::new());
}

#[test]
fn a_finger_held_still_shows_its_card_once_and_its_lift_does_nothing() {
    let mut client = paying();
    let frame = client.frame(16 * MS).clone();
    let at = on(&frame, "Counterspell");
    client.handle(finger(1, PointerPhase::Down, at));
    // The hold is timed from the first frame after the finger went down.
    client.frame(20 * MS);
    let early = client.frame(519 * MS).clone();
    assert_eq!(inspected(&early), None, "shown before the hold time");
    let held = client.frame(520 * MS).clone();
    assert_eq!(inspected(&held).as_deref(), Some("Counterspell"));
    let later = client.frame(1200 * MS).clone();
    assert_eq!(inspected(&later).as_deref(), Some("Counterspell"));
    client.handle(finger(1, PointerPhase::Up, at));
    let lifted = client.frame(1216 * MS).clone();
    assert_eq!(clicks(&mut client), Vec::new(), "the lift clicked the card");
    assert_eq!(inspected(&lifted).as_deref(), Some("Counterspell"));
}

#[test]
fn a_finger_that_moves_shows_and_clicks_nothing() {
    let mut client = paying();
    let frame = client.frame(16 * MS).clone();
    let angel = on(&frame, "Serra Angel");
    // Moving past the drag threshold before the hold time ends the hold.
    client.handle(finger(1, PointerPhase::Down, angel));
    client.frame(20 * MS);
    client.handle(finger(1, PointerPhase::Move, angel + Vec2::new(8.0, 0.0)));
    let after = client.frame(1020 * MS).clone();
    assert_eq!(inspected(&after), None, "a moved finger held");
    client.handle(finger(1, PointerPhase::Up, angel + Vec2::new(8.0, 0.0)));
    let lifted = client.frame(1036 * MS).clone();
    assert_eq!(inspected(&lifted), None, "a moved finger tapped");
    // A lift far from where the finger landed, with no move between, is a drag too.
    client.handle(finger(1, PointerPhase::Down, angel));
    client.handle(finger(1, PointerPhase::Up, angel + Vec2::new(20.0, 0.0)));
    client.frame(1052 * MS);
    let lifted = client.frame(1068 * MS).clone();
    assert_eq!(inspected(&lifted), None, "a lift far away tapped");
    assert_eq!(clicks(&mut client), Vec::new());
}

#[test]
fn a_hand_card_is_carried_as_soon_as_the_finger_moves() {
    let mut client = paying();
    let frame = client.frame(16 * MS).clone();
    let (_, counterspell) = card(&frame, "Counterspell");
    let at = on(&frame, "Counterspell");
    let out = at - Vec2::new(0.0, 200.0);
    client.handle(finger(1, PointerPhase::Down, at));
    client.handle(finger(1, PointerPhase::Move, out));
    let carried = client.frame(32 * MS).clone();
    let (index, _) = card(&carried, "Counterspell");
    assert!(carried.resolved[index as usize].state.has(StateBits::HELD));
    // Let go over the table, it is played.
    client.handle(finger(1, PointerPhase::Up, out));
    client.frame(48 * MS);
    assert_eq!(
        clicks(&mut client),
        vec![Click::On {
            on: object(counterspell)
        }]
    );
    // Let go back over the hand, it returns and nothing is played.
    client.frame(400 * MS);
    let frame = client.frame(416 * MS).clone();
    let at = on(&frame, "Counterspell");
    client.handle(finger(1, PointerPhase::Down, at));
    client.handle(finger(1, PointerPhase::Move, at - Vec2::new(0.0, 200.0)));
    client.frame(432 * MS);
    client.handle(finger(1, PointerPhase::Move, at + Vec2::new(10.0, 0.0)));
    client.handle(finger(1, PointerPhase::Up, at + Vec2::new(10.0, 0.0)));
    client.frame(448 * MS);
    assert_eq!(clicks(&mut client), Vec::new());
}

#[test]
fn on_the_manual_table_a_finger_rests_before_it_picks_a_card_up() {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let mut update = builder.update();
    update.view.table = Some(table(&update.view));
    let mut client = client(update);
    let frame = client.frame(16 * MS).clone();
    let angel = on(&frame, "Serra Angel");
    let held = |frame: &Frame| {
        let (index, _) = card(frame, "Serra Angel");
        frame.resolved[index as usize].state.has(StateBits::HELD)
    };
    client.handle(finger(1, PointerPhase::Down, angel));
    client.handle(finger(1, PointerPhase::Move, angel + Vec2::new(30.0, 0.0)));
    let early = client.frame(32 * MS).clone();
    assert!(!held(&early), "picked up before the finger rested");
    client.handle(finger(1, PointerPhase::Up, angel + Vec2::new(30.0, 0.0)));
    client.frame(400 * MS);

    client.handle(finger(1, PointerPhase::Down, angel));
    client.frame(416 * MS);
    let rested = client.frame(566 * MS).clone();
    assert!(!held(&rested), "a finger at rest holds nothing");
    client.handle(finger(1, PointerPhase::Move, angel + Vec2::new(30.0, 0.0)));
    let carried = client.frame(582 * MS).clone();
    assert!(held(&carried), "not picked up after the finger rested");
}

#[test]
fn a_finger_dragged_across_a_scrolling_row_scrolls_it_and_touches_no_card() {
    let mut builder = ViewBuilder::new();
    for index in 0..30 {
        builder
            .card(ZoneName::Battlefield, Side::You, &format!("Land {index}"))
            .types = vec![CardType::Land];
    }
    builder.script(ScriptBuilder::reset().build());
    let mut client = client(builder.update());
    let frame = client.frame(16 * MS).clone();
    let first_in_view = |frame: &Frame| {
        (frame.resolved.iter())
            .filter(|element| element.role == Role::Card && element.opacity > 0.0)
            .min_by(|a, b| a.corners[0].x.total_cmp(&b.corners[0].x))
            .map(|element| frame.name(element.name).to_owned())
    };
    let before = first_in_view(&frame);
    let at = on(&frame, before.as_deref().expect("lands in view"));
    client.handle(finger(1, PointerPhase::Down, at));
    for step in 1..=4 {
        let x = at.x - 40.0 * step as f32;
        client.handle(finger(1, PointerPhase::Move, Vec2::new(x, at.y)));
    }
    client.handle(finger(1, PointerPhase::Up, at - Vec2::new(160.0, 0.0)));
    client.frame(32 * MS);
    let after = client.frame(2000 * MS).clone();
    assert_ne!(first_in_view(&after), before, "the row did not scroll");
    assert_eq!(inspected(&after), None);
    assert_eq!(clicks(&mut client), Vec::new());
}

#[test]
fn a_second_finger_cancels_the_first_only_when_it_lands_with_it() {
    let mut client = paying();
    let frame = client.frame(16 * MS).clone();
    let (_, counterspell) = card(&frame, "Counterspell");
    let at = on(&frame, "Counterspell");
    let elsewhere = felt(&frame);
    // A thumb landing 100 ms later is ignored, and the first finger's tap goes on.
    client.handle(finger(1, PointerPhase::Down, at));
    client.frame(116 * MS);
    tap(&mut client, 2, elsewhere);
    client.handle(finger(1, PointerPhase::Up, at));
    client.frame(132 * MS);
    assert_eq!(
        clicks(&mut client),
        vec![Click::On {
            on: object(counterspell)
        }]
    );
    // A finger landing with it makes a two-finger tap, which clicks and shows nothing.
    client.frame(1000 * MS);
    client.handle(finger(1, PointerPhase::Down, at));
    client.handle(finger(2, PointerPhase::Down, elsewhere));
    client.handle(finger(1, PointerPhase::Up, at));
    client.handle(finger(2, PointerPhase::Up, elsewhere));
    client.frame(1016 * MS);
    let after = client.frame(1032 * MS).clone();
    assert_eq!(clicks(&mut client), Vec::new());
    assert_eq!(inspected(&after), None);
    // A card the first finger was already carrying falls back when the second lands with it.
    client.frame(2000 * MS);
    let out = at - Vec2::new(0.0, 200.0);
    client.handle(finger(1, PointerPhase::Down, at));
    client.handle(finger(1, PointerPhase::Move, out));
    client.handle(finger(2, PointerPhase::Down, elsewhere));
    let dropped = client.frame(2016 * MS).clone();
    let (index, _) = card(&dropped, "Counterspell");
    assert!(!dropped.resolved[index as usize].state.has(StateBits::HELD));
    client.handle(finger(1, PointerPhase::Up, out));
    client.handle(finger(2, PointerPhase::Up, elsewhere));
    client.frame(2032 * MS);
    assert_eq!(
        clicks(&mut client),
        Vec::new(),
        "the dropped card was played"
    );
}

/// A tap with two fingers landing together at `at`.
fn two_finger_tap(client: &mut Client, at: Vec2) {
    let beside = at + Vec2::new(12.0, 0.0);
    client.handle(finger(1, PointerPhase::Down, at));
    client.handle(finger(2, PointerPhase::Down, beside));
    client.handle(finger(1, PointerPhase::Up, at));
    client.handle(finger(2, PointerPhase::Up, beside));
}

/// A double tap: what it is called, where its fingers land, whether two land each time, and
/// how many reports and which clicks follow it.
type Case = (&'static str, fn(&Frame) -> Vec2, bool, usize, Vec<Click>);

#[test]
fn a_double_tap_on_bare_felt_or_with_two_fingers_opens_the_bug_report() {
    let decline = Click::Button {
        button: ButtonId::Decline,
    };
    // Each case taps twice 100 ms apart, with two fingers or one, and says how many reports and
    // clicks follow.
    let cases: [Case; 6] = [
        ("one finger on felt", felt, false, 1, Vec::new()),
        (
            "one finger on a card",
            |f| on(f, "Serra Angel"),
            false,
            0,
            Vec::new(),
        ),
        (
            "one finger on a button",
            |f| centre(f, find(f, Role::Button, "Cancel")),
            false,
            0,
            vec![decline, decline],
        ),
        ("two fingers on felt", felt, true, 1, Vec::new()),
        (
            "two fingers on a card",
            |f| on(f, "Counterspell"),
            true,
            1,
            Vec::new(),
        ),
        (
            "two fingers on a button",
            |f| centre(f, find(f, Role::Button, "Cancel")),
            true,
            1,
            Vec::new(),
        ),
    ];
    for (case, place, two, opened, clicked) in cases {
        let mut client = paying();
        let frame = client.frame(16 * MS).clone();
        reports(&mut client);
        let at = place(&frame);
        for now in [32, 132] {
            if two {
                two_finger_tap(&mut client, at);
            } else {
                tap(&mut client, 1, at);
            }
            client.frame(now * MS);
        }
        assert_eq!(reports(&mut client), opened, "{case}");
        assert_eq!(clicks(&mut client), clicked, "{case}");
    }
    // Taps 400 ms apart are two taps, not a double tap.
    let mut client = paying();
    let frame = client.frame(16 * MS).clone();
    reports(&mut client);
    let at = felt(&frame);
    tap(&mut client, 1, at);
    client.frame(416 * MS);
    tap(&mut client, 1, at);
    client.frame(432 * MS);
    assert_eq!(reports(&mut client), 0, "slow taps");
}

/// A card element at `x`, 96 by 134 points, at `depth` (nearer is smaller).
fn strip_card(id: u32, x: f32, depth: f32) -> Resolved {
    Resolved {
        id: ElementId(id),
        parent: u32::MAX,
        role: Role::Card,
        name: NameRef::default(),
        place: Place::Card {
            zone: ZoneName::Hand,
            side: Side::You,
            row: None,
        },
        corners: [
            Vec2::new(x, 534.0),
            Vec2::new(x + 96.0, 534.0),
            Vec2::new(x + 96.0, 400.0),
            Vec2::new(x, 400.0),
        ],
        depth,
        opacity: 1.0,
        clip: None,
        interactive: true,
        state: StateBits::default(),
        face: Some(1),
        copies: 1,
        really: NameRef::default(),
        note: NameRef::default(),
    }
}

/// Two cards of a fan, the second laid over the first with a 44-point strip of the first
/// uncovered: a finger in the strip reaches the first card, and one just past it the second.
/// Placement leaves that strip, and a touch hits through the frame's resolved geometry as the
/// mouse does.
#[test]
fn a_finger_in_a_44_point_strip_reaches_the_card_under_the_one_over_it() {
    let mut frame = Frame::with_capacity(2, 0, 64);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    builder.resolved("Island", strip_card(0, 100.0, 2.0));
    builder.resolved("Plains", strip_card(1, 144.0, 1.0));
    builder.finish();
    assert_eq!(
        uncovered(&frame, 0),
        Some(Rect {
            x: 100.0,
            y: 400.0,
            width: 44.0,
            height: 134.0,
        })
    );
    for x in [100.5, 122.0, 143.5] {
        assert_eq!(hit(&frame, Vec2::new(x, 467.0)), Some(0), "at {x}");
    }
    for x in [144.5, 200.0] {
        assert_eq!(hit(&frame, Vec2::new(x, 467.0)), Some(1), "at {x}");
    }
}
