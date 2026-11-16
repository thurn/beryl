//! The manual table, on views built by hand: which commands apply to a permanent, the context
//! menu a right click or a long press opens on one (a bottom sheet on a phone), the keys that
//! change a life total and draw a card, the command list, the ask prompt, the token search, the
//! library order tray and the log.

use beryl_card_records::{TokenMakers, card_records_among};
use beryl_protocol::ipc::decks::{CardSearchHit, CardTokenLink};
use beryl_protocol::ipc::{TableActReply, TableRefusalReason, TableRefused};
use beryl_protocol::protocol::{LogLine, Side, TableAction, ZoneName};
use beryl_scene::command::{Reply, Request, SessionCommand};
use beryl_scene::frame::{Frame, Role, StateBits, Wake};
use beryl_scene::input::{
    Button, Input, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase,
    Size, TextChange,
};
use beryl_scene::manual::bindings::Bindings;
use beryl_scene::manual::commands::{AppliesTo, commands_for};
use beryl_scene::manual::{Context, Target, target_at};
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
    let ids: Vec<&str> = commands_for(
        AppliesTo::Permanent,
        &view,
        &Context::new(true, &Bindings::new()),
    )
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
    for entry in commands_for(
        AppliesTo::Permanent,
        &view,
        &Context::new(true, &Bindings::new()),
    ) {
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

#[test]
fn a_submenu_row_on_the_phone_sheet_shows_its_rows_with_back() {
    let view = table_view();
    let (mut client, _) = client_in(&view, 393.0, 759.0);
    client.handle(Input::Cards(Box::new(card_records_among(
        ["Island"],
        TokenMakers::NONE,
    ))));
    let frame = client.frame(MS).clone();
    let island = middle(&frame, Role::Pile, "Your library");
    client.handle(touch(PointerPhase::Down, island));
    client.frame(10 * MS);
    let frame = client.frame(10 * MS + LONG_PRESS).clone();
    let first: Vec<String> = menu_rows(&frame).into_iter().map(str::to_owned).collect();
    let (folder, submenu_rows): (&str, Vec<&str>) = {
        let name = commands_for(
            AppliesTo::Library,
            &view,
            &Context::new(true, &Bindings::new()),
        )
        .iter()
        .find_map(|entry| entry.menu.and_then(|placement| placement.submenu))
        .expect("a library has a submenu");
        let inside = commands_for(
            AppliesTo::Library,
            &view,
            &Context::new(true, &Bindings::new()),
        )
        .iter()
        .filter(|entry| entry.menu.and_then(|placement| placement.submenu) == Some(name))
        .map(|entry| entry.command.label)
        .collect();
        (name, inside)
    };
    assert!(first.iter().any(|row| row == folder));
    let press = |client: &mut Client, frame: &Frame, name: &str| {
        let at = middle(frame, Role::MenuItem, name);
        client.handle(touch(PointerPhase::Down, at));
        client.handle(touch(PointerPhase::Up, at));
        client.frame(20 * MS + LONG_PRESS).clone()
    };
    let frame = press(&mut client, &frame, folder);
    let rows = menu_rows(&frame);
    assert_eq!(rows[0], "Back", "{rows:?}");
    for row in &submenu_rows {
        assert!(rows.contains(row), "no {row:?} in {rows:?}");
    }
    assert!(
        (frame.resolved.iter())
            .all(|element| element.role != Role::List || frame.name(element.name) != folder),
        "the submenu opened as a list beside the sheet"
    );
    let frame = press(&mut client, &frame, "Back");
    assert_eq!(menu_rows(&frame), first);
}

/// Cmd held, as the table's keys are written for the Mac.
fn cmd() -> Modifiers {
    Modifiers {
        command: true,
        ..Modifiers::default()
    }
}

/// Types `words` into the focused field.
fn type_in(client: &mut Client, words: &str) {
    client.handle(Input::Text(TextChange::Insert(words.to_owned())));
}

/// The names of the menu items `client` shows now, top to bottom.
fn rows(client: &mut Client, now: u64) -> Vec<String> {
    let frame = client.frame(now);
    (frame.resolved.iter())
        .filter(|element| element.role == Role::MenuItem)
        .map(|element| frame.name(element.name).to_owned())
        .collect()
}

/// Every command the client has queued, in order.
fn commands(client: &mut Client) -> Vec<SessionCommand> {
    std::iter::from_fn(|| client.next_command()).collect()
}

/// The action `json` names, as a table action.
fn action(json: serde_json::Value) -> TableAction {
    serde_json::from_value(json).unwrap()
}

#[test]
fn typing_shuf_in_the_command_list_puts_shuffle_first() {
    let view = table_view();
    let (mut client, _) = client_at_table(&view);
    client.handle(key(KeyCode::K, cmd()));
    assert!(
        rows(&mut client, 1).len() > 20,
        "the list offers every command"
    );
    type_in(&mut client, "shuf");
    let shown = rows(&mut client, 2);
    assert_eq!(
        shown.first().map(String::as_str),
        Some("Shuffle"),
        "{shown:?}"
    );
    // Return runs it, on the viewer's library.
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    let expected = action(serde_json::json!({ "shuffle": { "seat": 0, "portion": "all" } }));
    assert_eq!(acts(&mut client), vec![expected]);
    assert!(rows(&mut client, 3).is_empty(), "the list closed");
}

#[test]
fn a_signed_life_entry_changes_life_and_a_plain_one_sets_it() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let life = middle(&frame, Role::Player, "You ");
    client.handle(pointer(PointerPhase::Move, life, Button::None));
    for (typed, expected) in [
        ("+3", serde_json::json!({ "life": { "seat": 0, "add": 3 } })),
        ("7", serde_json::json!({ "life": { "seat": 0, "set": 7 } })),
    ] {
        client.handle(key(KeyCode::L, cmd()));
        // The field opens on the current total, selected, so typing replaces it.
        type_in(&mut client, typed);
        client.handle(key(KeyCode::Enter, Modifiers::default()));
        assert_eq!(acts(&mut client), vec![action(expected)], "typed {typed:?}");
    }
}

#[test]
fn two_slash_three_sets_power_and_toughness() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let angel = middle(&frame, Role::Card, "Serra Angel");
    client.handle(pointer(PointerPhase::Move, angel, Button::None));
    client.handle(key(KeyCode::P, cmd()));
    type_in(&mut client, "2/3");
    let frame = client.frame(1);
    assert!(
        (frame.resolved.iter()).any(|element| element.role == Role::Field
            && frame.name(element.name) == "Set power and toughness"),
        "the ask prompt is open in the dock"
    );
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    let [
        TableAction::Stats {
            stats: Some((2, 3)),
            ..
        },
    ] = acts(&mut client)[..]
    else {
        panic!("2/3 did not set the card's power and toughness");
    };
}

#[test]
fn the_token_search_lists_the_decks_tokens_before_searching() {
    let mut view = table_view();
    let token = |name: &str, id: &str| CardTokenLink {
        name: name.to_owned(),
        scryfall_id: id.to_owned(),
    };
    let table = view.table.as_mut().unwrap();
    table.deck_tokens = vec![token("Spirit", "spirit-1"), token("Soldier", "soldier-1")];
    let (mut client, _) = client_at_table(&view);
    client.handle(key(KeyCode::T, cmd()));
    assert_eq!(rows(&mut client, 1), ["Spirit", "Soldier", "Custom token"]);
    assert_eq!(
        commands(&mut client),
        Vec::new(),
        "nothing was searched yet"
    );

    type_in(&mut client, "spi");
    assert_eq!(
        commands(&mut client),
        vec![SessionCommand::CardSearch("spi".to_owned())]
    );
    let hit = |name: &str, id: &str| CardSearchHit {
        name: name.to_owned(),
        scryfall_id: id.to_owned(),
        type_line: "Token Creature".to_owned(),
        mana_cost: None,
        power: Some("1".to_owned()),
        toughness: Some("1".to_owned()),
        image: String::new(),
    };
    let hits = vec![
        hit("Spirit", "spirit-1"),
        hit("Spirit Warrior", "warrior-1"),
    ];
    client.handle(Input::Reply(Reply::CardSearch("spi".to_owned(), hits)));
    // The deck's token stays first, and is listed once.
    assert_eq!(
        rows(&mut client, 2),
        ["Spirit", "Spirit Warrior", "Custom token"]
    );
    client.handle(key(KeyCode::ArrowDown, Modifiers::default()));
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    let expected = action(serde_json::json!({ "token": {
        "card": { "token": { "name": "Spirit Warrior", "scryfall_id": "warrior-1" } },
        "count": 1,
        "attached_to": null,
        "note": null,
    }}));
    assert_eq!(acts(&mut client), vec![expected]);
}

#[test]
fn undo_on_a_log_line_sends_its_undo() {
    let mut view = table_view();
    let line = |n: u32, text: &str, undoable: bool| LogLine {
        n,
        by: "You".to_owned(),
        text: text.to_owned(),
        struck: false,
        undoable,
        blocked_by: Vec::new(),
    };
    view.table.as_mut().unwrap().log = vec![
        line(1, "Shuffled your library.", false),
        line(2, "Drew a card.", true),
    ];
    let (mut client, frame) = client_at_table(&view);
    // The log starts folded to a button, which opens it.
    let show = middle(&frame, Role::Button, "Show log");
    client.handle(pointer(PointerPhase::Down, show, Button::Primary));
    client.handle(pointer(PointerPhase::Up, show, Button::Primary));
    let frame = client.frame(1).clone();
    let undo = middle(&frame, Role::Button, "Undo");
    client.handle(pointer(PointerPhase::Down, undo, Button::Primary));
    client.handle(pointer(PointerPhase::Up, undo, Button::Primary));
    let expected = action(serde_json::json!({ "undo": { "entry": 2 } }));
    assert_eq!(acts(&mut client), vec![expected]);
}

#[test]
fn the_library_order_puts_the_first_card_on_top() {
    let view = table_view();
    let (mut client, _) = client_at_table(&view);
    let hand: Vec<_> = (view.objects.iter())
        .filter(|object| object.zone == ZoneName::Hand && object.owner == Side::You)
        .map(|object| object.slot)
        .take(2)
        .collect();
    assert_eq!(hand.len(), 2, "the sample view has two cards in hand");
    client.order_library(Side::You, hand.clone());
    let frame = client.frame(1).clone();
    let second = (frame.resolved.iter())
        .filter(|element| {
            frame
                .name(element.name)
                .ends_with(&format!("#{}", hand[1].0))
        })
        .find(|element| element.role == Role::Region)
        .expect("the tray shows the second card");
    let at = second.corners.iter().copied().sum::<Vec2>() / 4.0;
    client.handle(pointer(PointerPhase::Down, at, Button::Primary));
    client.handle(pointer(PointerPhase::Up, at, Button::Primary));
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    // The second card was moved to the top, so it is put on the library last.
    let moved: Vec<u32> = acts(&mut client)
        .iter()
        .map(|action| match action {
            TableAction::Move { selection, .. } => {
                serde_json::to_value(selection).unwrap()["objects"][0]["handle"]
                    .as_u64()
                    .and_then(|handle| u32::try_from(handle).ok())
                    .unwrap()
            }
            other => panic!("not a move: {other:?}"),
        })
        .collect();
    assert_eq!(moved, [u32::from(hand[0].0), u32::from(hand[1].0)]);
}

/// Presses the first element drawn at `now` with `role` whose name starts with `name`.
fn press(client: &mut Client, now: u64, role: Role, name: &str) {
    let at = middle(&client.frame(now).clone(), role, name);
    client.handle(pointer(PointerPhase::Down, at, Button::Primary));
    client.handle(pointer(PointerPhase::Up, at, Button::Primary));
}

/// The settings the client last asked to save, if it asked.
fn saved(client: &mut Client) -> Option<beryl_scene::settings::Settings> {
    std::iter::from_fn(|| client.next_request())
        .filter_map(|request| match request {
            Request::SaveSettings(settings) => Some(settings),
            _ => None,
        })
        .last()
}

#[test]
fn option_return_binds_a_key_and_reset_shortcuts_takes_it_back() {
    let view = table_view();
    let (mut client, _) = client_at_table(&view);
    let shuffle = action(serde_json::json!({ "shuffle": { "seat": 0, "portion": "all" } }));
    client.handle(key(KeyCode::K, cmd()));
    type_in(&mut client, "shuf");
    let option = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    client.handle(key(KeyCode::Enter, option));
    // F9 is Second main phase's own key, which it loses to Shuffle.
    client.handle(key(KeyCode::F9, Modifiers::default()));
    let frame = client.frame(1).clone();
    assert!(
        (frame.resolved.iter())
            .any(|element| frame.name(element.name) == "Second main phase lost its shortcut."),
        "the list says which command lost its key"
    );
    assert!(saved(&mut client).is_some(), "the binding is saved");
    let shown = rows(&mut client, 2);
    assert_eq!(shown.first().map(String::as_str), Some("Shuffle"));
    client.handle(key(KeyCode::Escape, Modifiers::default()));
    client.handle(key(KeyCode::F9, Modifiers::default()));
    assert_eq!(acts(&mut client), vec![shuffle.clone()], "F9 now shuffles");

    // Shortcuts opens the list with Reset shortcuts at its top, which puts F9 back.
    client.handle(key(KeyCode::K, cmd()));
    type_in(&mut client, "shortcuts");
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    assert_eq!(
        rows(&mut client, 3).first().map(String::as_str),
        Some("Reset shortcuts")
    );
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    let reset = saved(&mut client).expect("the reset is saved");
    assert!(reset.bindings.overrides().is_empty());
    client.handle(key(KeyCode::Escape, Modifiers::default()));
    client.handle(key(KeyCode::F9, Modifiers::default()));
    let phase = action(serde_json::json!({ "phase": { "phase": "second_main" } }));
    assert_eq!(acts(&mut client), vec![phase]);
}

#[test]
fn the_custom_token_form_takes_a_type_line_colors_stats_and_a_note() {
    let view = table_view();
    let (mut client, _) = client_at_table(&view);
    client.handle(key(KeyCode::T, cmd()));
    assert_eq!(
        rows(&mut client, 1).last().map(String::as_str),
        Some("Custom token")
    );
    client.handle(key(KeyCode::ArrowUp, Modifiers::default()));
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    // The name is focused as the form opens.
    type_in(&mut client, "Germ");
    for (field, words) in [
        ("Type line", "Creature — Phyrexian Germ"),
        ("Power", "0"),
        ("Toughness", "0"),
        ("Note", "Equipped"),
    ] {
        press(&mut client, 2, Role::Field, field);
        type_in(&mut client, words);
    }
    press(&mut client, 3, Role::Toggle, "Black");
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    let expected = action(serde_json::json!({ "token": {
        "card": { "custom": {
            "name": "Germ", "types": "Creature — Phyrexian Germ", "colors": ["B"], "stats": [0, 0]
        } },
        "count": 1,
        "attached_to": null,
        "note": "Equipped",
    }}));
    assert_eq!(acts(&mut client), vec![expected]);
}

#[test]
fn create_another_token_makes_the_last_token_the_table_took() {
    let mut view = table_view();
    view.table.as_mut().unwrap().deck_tokens = vec![CardTokenLink {
        name: "Spirit".to_owned(),
        scryfall_id: "spirit-1".to_owned(),
    }];
    let (mut client, _) = client_at_table(&view);
    client.handle(key(KeyCode::T, cmd()));
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    let made = acts(&mut client);
    assert_eq!(made.len(), 1, "the Spirit was asked for");
    // Until the table takes it, there is no last token to make again.
    client.handle(key(KeyCode::G, cmd()));
    assert!(acts(&mut client).is_empty());
    client.handle(Input::Reply(Reply::TableAct(TableActReply::Ok {
        ok: true,
    })));
    client.handle(key(KeyCode::G, cmd()));
    assert_eq!(acts(&mut client), made);
    client.handle(key(KeyCode::K, cmd()));
    assert!(
        rows(&mut client, 1)
            .iter()
            .any(|row| row == "Create another token")
    );
}

#[test]
fn a_refused_undo_opens_the_log_on_the_lines_that_block_it() {
    let mut view = table_view();
    let line = |n: u32, undoable: bool, blocked_by: Vec<u32>| LogLine {
        n,
        by: "You".to_owned(),
        text: "Drew a card.".to_owned(),
        struck: false,
        undoable,
        blocked_by,
    };
    view.table.as_mut().unwrap().log = vec![line(1, false, vec![2]), line(2, true, Vec::new())];
    let (mut client, _) = client_at_table(&view);
    press(&mut client, 1, Role::Button, "Show log");
    press(&mut client, 2, Role::Button, "Undo");
    assert_eq!(
        acts(&mut client),
        vec![action(serde_json::json!({ "undo": { "entry": 1 } }))]
    );
    press(&mut client, 3, Role::Button, "Hide log");
    client.handle(Input::Reply(Reply::TableAct(TableActReply::Refused(
        TableRefused {
            refused: TableRefusalReason::Invalid,
            blocked_by: Some(vec![2]),
        },
    ))));
    let frame = client.frame(4);
    let selected = |name: &str| {
        (frame.resolved.iter())
            .find(|element| frame.name(element.name) == name)
            .unwrap_or_else(|| panic!("the log shows {name}"))
            .state
            .has(StateBits::SELECTED)
    };
    assert!(selected("line 2"), "the blocking line is highlighted");
    assert!(!selected("line 1"));
}

#[test]
fn the_log_stays_folded_to_a_button_until_opened() {
    let mut view = table_view();
    view.table.as_mut().unwrap().log = vec![LogLine {
        n: 1,
        by: "You".to_owned(),
        text: "Shuffled your library.".to_owned(),
        struck: false,
        undoable: false,
        blocked_by: Vec::new(),
    }];
    // The sample board has no permanent near the log's column, and still the log is folded,
    // as the web's desktop table always folds it.
    let (_, frame) = client_at_table(&view);
    let names: Vec<&str> = frame
        .resolved
        .iter()
        .map(|element| frame.name(element.name))
        .collect();
    assert!(names.contains(&"Show log"), "{names:?}");
    assert!(!names.contains(&"line 1"));
}
