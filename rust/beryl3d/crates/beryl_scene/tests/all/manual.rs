//! The manual table, on views built by hand: which commands apply to a permanent, the context
//! menu a right click or a long press opens on one (a bottom sheet on a phone), and the keys
//! that change a life total and draw a card.

use beryl_card_records::{TokenMakers, card_records_among};
use beryl_protocol::protocol::{Side, TableAction, ZoneName};
use beryl_scene::command::SessionCommand;
use beryl_scene::frame::{Frame, Role, Wake};
use beryl_scene::input::{
    Button, Input, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase,
    Size,
};
use beryl_scene::manual::commands::{AppliesTo, commands_for};
use beryl_scene::manual::{Target, target_at};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, table};
use beryl_scene::{Client, Config};
use glam::Vec2;

/// A sample view at a solo manual table, at rest.
fn table_view() -> beryl_protocol::PlayerView {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let mut view = builder.build();
    view.table = Some(table(&view));
    view
}

/// A client on the Mac in a 1600 by 900 window that has been given `view` and drawn it.
fn client_at_table(view: &beryl_protocol::PlayerView) -> (Client, Frame) {
    client_in(view, 1600.0, 900.0)
}

/// A client on the Mac in a `width` by `height` window that has been given `view` and drawn
/// it.
fn client_in(view: &beryl_protocol::PlayerView, width: f32, height: f32) -> (Client, Frame) {
    let mut client = Client::new(Config {
        elements: 2048,
        draws: 2048,
        name_bytes: 65_536,
    });
    client.handle(Input::Platform { mac: true });
    client.handle(Input::Surface {
        size: Size { width, height },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::SessionId(1),
        view: view.clone(),
    })));
    let frame = client.frame(0).clone();
    (client, frame)
}

/// The middle of the first element of `frame` with `role` whose name starts with `name`.
fn middle(frame: &Frame, role: Role, name: &str) -> Vec2 {
    let element = (frame.resolved.iter())
        .find(|element| element.role == role && frame.name(element.name).starts_with(name))
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    element.corners.iter().copied().sum::<Vec2>() / 4.0
}

fn pointer(phase: PointerPhase, position: Vec2, button: Button) -> Input {
    Input::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase,
        position,
        button,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    })
}

fn touch(phase: PointerPhase, position: Vec2) -> Input {
    Input::Pointer(Pointer {
        id: 1,
        kind: PointerKind::Touch,
        phase,
        position,
        button: Button::Primary,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    })
}

/// Nanoseconds in a millisecond, the unit of `Client::frame`'s clock.
const MS: u64 = 1_000_000;

/// The hold time of a long press, `pointer.long_press_ms` in `data/beryl3d/space.json`.
const LONG_PRESS: u64 = 500 * MS;

/// The names of the menu rows `frame` shows.
fn menu_rows(frame: &Frame) -> Vec<&str> {
    (frame.resolved.iter())
        .filter(|element| element.role == Role::MenuItem)
        .map(|element| frame.name(element.name))
        .collect()
}

fn key(code: KeyCode, modifiers: Modifiers) -> Input {
    Input::Key(Key {
        code,
        phase: KeyPhase::Down,
        modifiers,
    })
}

/// The table actions the client has queued, in order.
fn acts(client: &mut Client) -> Vec<TableAction> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::TableAct(action) => Some(action),
            _ => None,
        })
        .collect()
}

#[test]
fn the_commands_for_a_permanent_are_the_files_permanent_commands_in_order() {
    let text = include_str!("../../../../../../data/table-commands.json");
    let file: Vec<serde_json::Value> = serde_json::from_str(text).unwrap();
    // With every assist off and no card drawn, a command of an assist or with a condition is
    // not offered.
    let expected: Vec<&str> = (file.iter())
        .filter(|command| {
            let applies = command["applies_to"] == "permanent"
                || command["also_applies_to"].get("permanent").is_some();
            applies
                && command["scope"] == "table"
                && command.get("assist").is_none()
                && command.get("when").is_none()
        })
        .map(|command| command["id"].as_str().unwrap())
        .collect();
    let view = table_view();
    let ids: Vec<&str> = commands_for(AppliesTo::Permanent, &view)
        .iter()
        .map(|entry| entry.command.id)
        .collect();
    assert_eq!(ids, expected);
    assert!(ids.contains(&"permanent.tap"));
}

#[test]
fn right_clicking_a_permanent_opens_its_menu() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let angel = middle(&frame, Role::Card, "Serra Angel");
    let Target::Card(slot) = target_at(&frame, angel) else {
        panic!("the pointer is not on the card");
    };
    let object = view
        .objects
        .iter()
        .find(|object| object.slot == slot)
        .unwrap();
    assert_eq!(
        (object.zone, object.controller),
        (ZoneName::Battlefield, Side::You)
    );
    client.handle(pointer(PointerPhase::Down, angel, Button::Secondary));
    let frame = client.frame(1).clone();
    let rows: Vec<&str> = (frame.resolved.iter())
        .filter(|element| element.role == Role::MenuItem)
        .map(|element| frame.name(element.name))
        .collect();
    // Every placed command outside a submenu is a row, and every submenu a row of its own.
    for entry in commands_for(AppliesTo::Permanent, &view) {
        let Some(placement) = entry.menu else {
            continue;
        };
        let row = placement.submenu.unwrap_or(entry.command.label);
        assert!(rows.contains(&row), "no row {row:?} in {rows:?}");
    }
    assert!(
        (frame.resolved.iter())
            .any(|element| element.role == Role::List && frame.name(element.name) == "Serra Angel"),
        "the menu is not titled for the card"
    );

    // Its first row taps the card.
    let tap = middle(&frame, Role::MenuItem, "Tap");
    client.handle(pointer(PointerPhase::Down, tap, Button::Primary));
    client.handle(pointer(PointerPhase::Up, tap, Button::Primary));
    let [TableAction::Tap { tapped: true, .. }] = acts(&mut client)[..] else {
        panic!("the Tap row did not tap the card");
    };
}

#[test]
fn f12_over_your_life_total_gains_one_life() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let life = middle(&frame, Role::Player, "You ");
    assert_eq!(target_at(&frame, life), Target::Life(Side::You));
    client.handle(pointer(PointerPhase::Move, life, Button::None));
    client.handle(key(KeyCode::F12, Modifiers::default()));
    let expected: TableAction =
        serde_json::from_value(serde_json::json!({ "life": { "seat": 0, "add": 1 } })).unwrap();
    assert_eq!(acts(&mut client), vec![expected]);
}

#[test]
fn cmd_d_draws_a_card() {
    let view = table_view();
    let (mut client, _) = client_at_table(&view);
    let command = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    client.handle(key(KeyCode::D, command));
    let expected: TableAction =
        serde_json::from_value(serde_json::json!({ "draw": { "seat": 0, "count": 1 } })).unwrap();
    assert_eq!(acts(&mut client), vec![expected]);
    // A held key's repeats draw no more.
    client.handle(Input::Key(Key {
        code: KeyCode::D,
        phase: KeyPhase::Repeat,
        modifiers: command,
    }));
    assert_eq!(acts(&mut client), Vec::new());
}

#[test]
fn a_touch_held_on_a_permanent_opens_its_menu() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let angel = middle(&frame, Role::Card, "Serra Angel");
    client.handle(touch(PointerPhase::Down, angel));
    // The hold is timed from the first frame after the touch, which wakes the client again
    // when it is due.
    client.frame(10 * MS);
    let due = 10 * MS + LONG_PRESS;
    assert!(
        matches!(client.wake(), Wake::Now) || client.wake() == Wake::At(due),
        "the client sleeps past the hold: {:?}",
        client.wake()
    );
    let frame = client.frame(10 * MS + LONG_PRESS - MS).clone();
    assert!(
        menu_rows(&frame).is_empty(),
        "the menu opened before the hold time"
    );
    let frame = client.frame(10 * MS + LONG_PRESS).clone();
    assert!(menu_rows(&frame).contains(&"Tap"), "no menu after the hold");
    assert!(
        (frame.resolved.iter())
            .any(|element| element.role == Role::List && frame.name(element.name) == "Serra Angel"),
        "the menu is not titled for the card"
    );
    // Lifting the finger gives no command and leaves the menu open.
    client.handle(touch(PointerPhase::Up, angel));
    let frame = client.frame(11 * MS + LONG_PRESS).clone();
    assert_eq!(acts(&mut client), Vec::new());
    assert!(
        menu_rows(&frame).contains(&"Tap"),
        "lifting the finger closed the menu"
    );
}

#[test]
fn a_touch_that_moves_before_the_hold_time_opens_no_menu() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let angel = middle(&frame, Role::Card, "Serra Angel");
    client.handle(touch(PointerPhase::Down, angel));
    client.frame(10 * MS);
    client.handle(touch(PointerPhase::Move, angel + Vec2::new(8.0, 0.0)));
    let frame = client.frame(10 * MS + 2 * LONG_PRESS).clone();
    assert!(
        menu_rows(&frame).is_empty(),
        "a moved touch opened the menu"
    );
    assert_eq!(client.wake(), Wake::Never);
}

#[test]
fn a_long_press_on_a_phone_opens_a_bottom_sheet_with_the_card() {
    let view = table_view();
    let (width, height) = (393.0, 759.0);
    let (mut client, _) = client_in(&view, width, height);
    client.handle(Input::Cards(Box::new(card_records_among(
        ["Island"],
        TokenMakers::NONE,
    ))));
    let frame = client.frame(MS).clone();
    // The spell on the stack covers both Serra Angels on a phone; the first Island is the
    // viewer's land.
    let island = middle(&frame, Role::Card, "Island");
    let Target::Card(slot) = target_at(&frame, island) else {
        panic!("the pointer is not on the Island");
    };
    assert!(
        (view.objects.iter())
            .any(|object| object.slot == slot && object.zone == ZoneName::Battlefield)
    );
    client.handle(touch(PointerPhase::Down, island));
    client.frame(10 * MS);
    let frame = client.frame(10 * MS + LONG_PRESS).clone();
    let sheet = (frame.resolved.iter())
        .find(|element| element.role == Role::List && frame.name(element.name) == "Island")
        .unwrap_or_else(|| {
            let names: Vec<_> = (frame.resolved.iter())
                .map(|element| (element.role, frame.name(element.name)))
                .collect();
            panic!("no sheet for the card in {names:?}")
        });
    let xs = sheet.corners.map(|corner| corner.x);
    let ys = sheet.corners.map(|corner| corner.y);
    let (left, right) = (
        xs.into_iter().fold(f32::MAX, f32::min),
        xs.into_iter().fold(0.0, f32::max),
    );
    let bottom = ys.into_iter().fold(0.0, f32::max);
    // It stands along the bottom of the window, across nearly all of it.
    assert!(height - bottom < 16.0, "the sheet's bottom is at {bottom}");
    assert!(
        right - left > width - 24.0,
        "the sheet is {} wide",
        right - left
    );
    assert!(
        (frame.resolved.iter()).any(|element| frame.name(element.name) == "preview"),
        "the sheet shows no card"
    );
}
