//! The manual table, on views built by hand: which commands apply to a permanent, the context
//! menu a right click opens on one, and the keys that change a life total and draw a card.

use beryl_protocol::protocol::{Side, TableAction, ZoneName};
use beryl_scene::command::SessionCommand;
use beryl_scene::frame::{Frame, Role};
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
    let mut client = Client::new(Config {
        elements: 2048,
        draws: 2048,
        name_bytes: 65_536,
    });
    client.handle(Input::Platform { mac: true });
    client.handle(Input::Surface {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
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
