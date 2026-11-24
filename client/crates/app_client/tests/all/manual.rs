//! The manual table, on views built by hand: which commands apply to a permanent, the context
//! menu a right click or a long press opens on one (a bottom sheet on a compact), the keys that
//! change a life total and draw a card, the command list, the ask prompt, the token search, the
//! library order tray and the log.

use app_client::Input;
use app_client::command::{Reply, Request, SessionCommand};
use app_client::{Client, Config};
use beryl_card_records::{TokenMakers, card_records_among};
use beryl_protocol::ipc::decks::{CardSearchHit, CardTokenLink};
use beryl_protocol::ipc::{TableActReply, TableRefusalReason, TableRefused};
use beryl_protocol::protocol::{LogLine, ObjectId, Side, TableAction, TableArrow, ZoneName};
use beryl_protocol::script::{Act, Arrow, Span as Edge, Subject};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, script, step, table, timing};
use game_manual::manual::bindings::Bindings;
use game_manual::manual::commands::{AppliesTo, commands_for};
use game_manual::manual::{Context, Target, target_at};
use glam::Vec2;
use kit_frame::frame::{Frame, Role, StateBits, Wake};
use kit_scene::hit::{self, PointerHit};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Size,
    TextChange,
};

/// A sample view at a solo manual table, at rest.
pub(super) fn table_view() -> beryl_protocol::PlayerView {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let mut view = builder.build();
    view.table = Some(table(&view));
    view
}

/// A client on the Mac in a 1600 by 900 window that has been given `view` and drawn it.
pub(super) fn client_at_table(view: &beryl_protocol::PlayerView) -> (Client, Frame) {
    client_in(view, 1600.0, 900.0)
}

/// A client on the Mac in a `width` by `height` window that has been given `view` and drawn
/// it.
pub(super) fn client_in(
    view: &beryl_protocol::PlayerView,
    width: f32,
    height: f32,
) -> (Client, Frame) {
    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 2048,
            draws: 2048,
            name_bytes: 65_536,
        },
    );
    client.handle(Input::Device(DeviceInput::Platform { mac: true }));
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size { width, height },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::SessionId(1),
        view: view.clone(),
    })));
    let frame = client.frame(0).clone();
    (client, frame)
}

/// The middle of the first element of `frame` with `role` whose name starts with `name`.
pub(super) fn middle(frame: &Frame, role: Role, name: &str) -> Vec2 {
    let element = (frame.resolved.iter())
        .find(|element| element.role == role && frame.name(element.name).starts_with(name))
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    element.corners.iter().copied().sum::<Vec2>() / 4.0
}

pub(super) fn pointer(phase: PointerPhase, position: Vec2, button: Button) -> Input {
    Input::Device(DeviceInput::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase,
        position,
        button,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    }))
}

fn touch(phase: PointerPhase, position: Vec2) -> Input {
    Input::Device(DeviceInput::Pointer(Pointer {
        id: 1,
        kind: PointerKind::Touch,
        phase,
        position,
        button: Button::Primary,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    }))
}

/// Nanoseconds in a millisecond, the unit of `Client::frame`'s clock.
const MS: u64 = 1_000_000;

/// The hold time of a long press, `long_press_ms` in `data/client/pointer.json`.
const LONG_PRESS: u64 = 500 * MS;

/// The names of the menu rows `frame` shows.
fn menu_rows(frame: &Frame) -> Vec<&str> {
    (frame.resolved.iter())
        .filter(|element| element.role == Role::MenuItem)
        .map(|element| frame.name(element.name))
        .collect()
}

pub(super) fn key(code: KeyCode, modifiers: Modifiers) -> Input {
    Input::Device(DeviceInput::Key(Key {
        code,
        phase: KeyPhase::Down,
        modifiers,
    }))
}

/// The table actions the client has queued, in order.
pub(super) fn acts(client: &mut Client) -> Vec<TableAction> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::TableAct(action) => Some(action),
            _ => None,
        })
        .collect()
}

#[test]
fn the_commands_for_each_kind_of_target_are_the_files_in_order() {
    let text = include_str!("../../../../../data/table-commands.json");
    let file: Vec<serde_json::Value> = serde_json::from_str(text).unwrap();
    let kinds = [
        (AppliesTo::Permanent, "permanent"),
        (AppliesTo::TheirPermanent, "their_permanent"),
        (AppliesTo::HandCard, "hand_card"),
        (AppliesTo::StackEntry, "stack_entry"),
        (AppliesTo::PileCard, "pile_card"),
        (AppliesTo::Library, "library"),
        (AppliesTo::Graveyard, "graveyard"),
        (AppliesTo::Exile, "exile"),
        (AppliesTo::Sideboard, "sideboard"),
        (AppliesTo::Hand, "hand"),
        (AppliesTo::Life, "life"),
        (AppliesTo::Player, "player"),
        (AppliesTo::Table, "table"),
        (AppliesTo::Lobby, "lobby"),
        (AppliesTo::ViewerCard, "viewer_card"),
    ];
    let view = table_view();
    let bindings = Bindings::new();
    let context = Context::new(true, &bindings);
    for (kind, word) in kinds {
        // `commandsFor` in ui/src/table/commands.ts, with every assist off and no card drawn,
        // so that a command of an assist or with a condition is not offered: each command of
        // the table's scope for which `word` is its kind or one of its others, with a menu row
        // where its own kind has one, or its other entry is `true` and it has one, or its
        // other entry is a placement.
        let expected: Vec<(&str, bool)> = (file.iter())
            .filter(|command| {
                command["scope"] == "table"
                    && command.get("assist").is_none()
                    && command.get("when").is_none()
            })
            .filter_map(|command| {
                let has_menu = command.get("menu").is_some();
                let menu = if command["applies_to"] == word {
                    has_menu
                } else {
                    match command.get("also_applies_to")?.get(word)? {
                        serde_json::Value::Bool(true) => has_menu,
                        serde_json::Value::Bool(false) => false,
                        _ => true,
                    }
                };
                Some((command["id"].as_str().unwrap(), menu))
            })
            .collect();
        let found: Vec<(&str, bool)> = commands_for(kind, &view, &context)
            .iter()
            .map(|entry| (entry.command.id, entry.menu.is_some()))
            .collect();
        assert_eq!(found, expected, "the commands for {word}");
    }
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
    client.handle(Input::Device(DeviceInput::Key(Key {
        code: KeyCode::D,
        phase: KeyPhase::Repeat,
        modifiers: command,
    })));
    assert_eq!(acts(&mut client), Vec::new());
}

#[test]
fn a_touch_held_on_a_permanent_opens_its_menu() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let angel = middle(&frame, Role::Card, "Serra Angel");
    client.handle(touch(PointerPhase::Down, angel));
    // The hold is timed from the first frame after the touch, which wakes the client again
    // no later than when it is due. It may wake sooner for another timer: before the first
    // screen stood whole from its first frame, its fade in kept the client awake.
    client.frame(10 * MS);
    let due = 10 * MS + LONG_PRESS;
    assert!(
        matches!(client.wake(), Wake::Now) || matches!(client.wake(), Wake::At(at) if at <= due),
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
    // The moved touch picked the card up. Once it is let go and has fallen back, nothing waits
    // for a hold to open a menu.
    client.handle(touch(PointerPhase::Up, angel + Vec2::new(8.0, 0.0)));
    client.frame(10 * MS + 3 * LONG_PRESS);
    let frame = client.frame(10 * MS + 6 * LONG_PRESS).clone();
    assert!(
        menu_rows(&frame).is_empty(),
        "lifting the finger opened the menu"
    );
    assert_eq!(client.wake(), Wake::Never);
}

#[test]
fn a_long_press_on_a_compact_opens_a_bottom_sheet_with_the_card() {
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
fn a_submenu_row_on_the_compact_sheet_shows_its_rows_with_back() {
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
pub(super) fn cmd() -> Modifiers {
    Modifiers {
        command: true,
        ..Modifiers::default()
    }
}

/// Types `words` into the focused field.
pub(super) fn type_in(client: &mut Client, words: &str) {
    client.handle(Input::Device(DeviceInput::Text(TextChange::Insert(
        words.to_owned(),
    ))));
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
pub(super) fn commands(client: &mut Client) -> Vec<SessionCommand> {
    std::iter::from_fn(|| client.next_command()).collect()
}

/// The action `json` names, as a table action.
pub(super) fn action(json: serde_json::Value) -> TableAction {
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
fn shift_with_a_deck_token_key_makes_the_token_face_down() {
    let mut view = table_view();
    view.table.as_mut().unwrap().deck_tokens = vec![CardTokenLink {
        name: "Spirit".to_owned(),
        scryfall_id: "spirit-1".to_owned(),
    }];
    let (mut client, _) = client_at_table(&view);
    let option = |shift| Modifiers {
        alt: true,
        shift,
        ..Modifiers::default()
    };
    let token = |face_down: bool| {
        let mut token = serde_json::json!({
            "card": { "token": { "name": "Spirit", "scryfall_id": "spirit-1" } },
            "count": 1,
            "attached_to": null,
            "note": null,
        });
        if face_down {
            token["face_down"] = true.into();
        }
        action(serde_json::json!({ "token": token }))
    };
    client.handle(key(KeyCode::Digit1, option(false)));
    assert_eq!(acts(&mut client), vec![token(false)]);
    client.handle(key(KeyCode::Digit1, option(true)));
    assert_eq!(acts(&mut client), vec![token(true)]);
}

#[test]
fn a_card_in_the_library_viewer_is_a_viewer_card() {
    let mut view = table_view();
    let slot = view.objects[0].slot;
    view.objects[0].zone = ZoneName::Library;
    assert_eq!(Target::Card(slot).applies_to(&view), AppliesTo::ViewerCard);
    let bindings = Bindings::new();
    let context = Context::new(true, &bindings);
    let ids = |kind| {
        commands_for(kind, &view, &context)
            .iter()
            .map(|entry| entry.command.id)
            .collect::<Vec<_>>()
    };
    assert!(ids(AppliesTo::ViewerCard).contains(&"hand_card.reveal"));
    assert!(!ids(AppliesTo::PileCard).contains(&"hand_card.reveal"));
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
pub(super) fn press(client: &mut Client, now: u64, role: Role, name: &str) {
    let at = middle(&client.frame(now).clone(), role, name);
    client.handle(pointer(PointerPhase::Down, at, Button::Primary));
    client.handle(pointer(PointerPhase::Up, at, Button::Primary));
}

/// The settings the client last asked to save, if it asked.
fn saved(client: &mut Client) -> Option<app_client::settings::Settings> {
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

/// The slot of the card named `name` in `side`'s `zone` of `view`, the first one if several.
fn slot_of(view: &beryl_protocol::PlayerView, zone: ZoneName, side: Side, name: &str) -> ObjectId {
    (view.objects.iter())
        .find(|object| {
            object.zone == zone && object.controller == side && object.name.as_deref() == Some(name)
        })
        .unwrap_or_else(|| panic!("no {name:?} in {side:?}'s {zone:?}"))
        .slot
}

/// The corners of the card element of `slot` in `frame`.
fn corners_of(frame: &Frame, slot: ObjectId) -> [Vec2; 4] {
    (frame.resolved.iter())
        .find(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0))
        .unwrap_or_else(|| panic!("no card element for slot {}", slot.0))
        .corners
}

/// The table actions as JSON, for reading their fields.
fn json(actions: &[TableAction]) -> Vec<serde_json::Value> {
    (actions.iter())
        .map(|action| serde_json::to_value(action).unwrap())
        .collect()
}

/// A mouse drag from `from` to `to` with the primary button, in eight steps with a frame after
/// each, ending with a frame at `now` that takes the release.
fn drag(client: &mut Client, from: Vec2, to: Vec2, now: u64) {
    client.handle(pointer(PointerPhase::Down, from, Button::Primary));
    client.frame(now);
    for step in 1..=8_u64 {
        let at = from.lerp(to, f32::from(u8::try_from(step).unwrap()) / 8.0);
        client.handle(pointer(PointerPhase::Move, at, Button::None));
        client.frame(now + step * 16 * MS);
    }
    client.handle(pointer(PointerPhase::Up, to, Button::Primary));
    client.frame(now + 9 * 16 * MS);
}

/// Sweeps the three local lands from clear table space on the desktop.
fn sweep_three_lands() -> (Client, Frame, Vec<ObjectId>) {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let lands: Vec<ObjectId> = (view.objects.iter())
        .filter(|object| {
            object.zone == ZoneName::Battlefield
                && object.controller == Side::You
                && matches!(object.name.as_deref(), Some("Island" | "Plains"))
        })
        .map(|object| object.slot)
        .collect();
    assert_eq!(lands.len(), 3);
    let (low, high) = lands
        .iter()
        .flat_map(|&slot| corners_of(&frame, slot))
        .fold(
            (Vec2::INFINITY, Vec2::NEG_INFINITY),
            |(low, high), corner| (low.min(corner), high.max(corner)),
        );
    let margin = Vec2::splat(4.0);
    let low = low - margin;
    let high = high + margin;
    // The upper-left corner is over the white mana counter in the native desktop layout.
    let start = Vec2::new(high.x, low.y);
    let end = Vec2::new(low.x, high.y);
    let empty_table_at = |client: &mut Client, frame: &Frame, point| {
        let layer = client.layer_mut();
        let target = hit::pointer(frame, layer, point, true, None, |_| true, |_| false);
        matches!(target, PointerHit::Empty)
    };
    assert!(
        empty_table_at(&mut client, &frame, start),
        "the sweep press at {start} needs empty table"
    );
    drag(&mut client, start, end, MS);
    let frame = client.frame(500 * MS).clone();
    assert!(
        empty_table_at(&mut client, &frame, end),
        "the cursor at {end} should be over empty table after the sweep"
    );
    (client, frame, lands)
}

#[test]
fn a_rectangle_over_three_permanents_selects_them_and_tap_acts_on_all_three() {
    let (mut client, frame, lands) = sweep_three_lands();

    // Each land wears the selected mark, and nothing else does.
    let selected: Vec<u32> = (frame.resolved.iter())
        .filter(|element| element.role == Role::Card && element.state.0 & StateBits::SELECTED != 0)
        .map(|element| element.id.0)
        .collect();
    let mut expected: Vec<u32> = lands.iter().map(|slot| u32::from(slot.0)).collect();
    let mut found = selected.clone();
    expected.sort_unstable();
    found.sort_unstable();
    assert_eq!(found, expected, "the sweep selected {selected:?}");

    // Tap on one of them taps all three in one action.
    let island = middle(&frame, Role::Card, "Island");
    client.handle(pointer(PointerPhase::Down, island, Button::Secondary));
    let frame = client.frame(600 * MS).clone();
    let tap = middle(&frame, Role::MenuItem, "Tap");
    client.handle(pointer(PointerPhase::Down, tap, Button::Primary));
    client.handle(pointer(PointerPhase::Up, tap, Button::Primary));
    let sent = json(&acts(&mut client));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["tap"]["objects"].as_array().map(Vec::len), Some(3));

    // Escape empties the selection.
    client.frame(700 * MS);
    client.handle(key(KeyCode::Escape, Modifiers::default()));
    let frame = client.frame(800 * MS).clone();
    assert!(
        (frame.resolved.iter())
            .filter(|element| element.role == Role::Card)
            .all(|element| element.state.0 & StateBits::SELECTED == 0),
        "Escape left a card selected"
    );
}

#[test]
fn a_cmd_click_adds_a_card_to_the_selection() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let angel = middle(&frame, Role::Card, "Serra Angel");
    let cmd = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    let down = |phase| {
        Input::Device(DeviceInput::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position: angel,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: cmd,
        }))
    };
    client.handle(down(PointerPhase::Down));
    client.handle(down(PointerPhase::Up));
    client.frame(MS);
    let frame = client.frame(100 * MS).clone();
    let slot = slot_of(&view, ZoneName::Battlefield, Side::You, "Serra Angel");
    let element = (frame.resolved.iter())
        .find(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0))
        .unwrap();
    assert_ne!(
        element.state.0 & StateBits::SELECTED,
        0,
        "the card is not selected"
    );
}

#[test]
fn dropping_a_hand_card_on_the_graveyard_sends_the_move() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let counterspell = slot_of(&view, ZoneName::Hand, Side::You, "Counterspell");
    let from = corners_of(&frame, counterspell).iter().sum::<Vec2>() / 4.0;
    let graveyard = slot_of(&view, ZoneName::Graveyard, Side::You, "Brainstorm");
    let to = corners_of(&frame, graveyard).iter().sum::<Vec2>() / 4.0;
    drag(&mut client, from, to, MS);
    let sent = json(&acts(&mut client));
    assert_eq!(sent.len(), 1, "{sent:?}");
    let moved = &sent[0]["move"];
    assert_eq!(
        moved["to"],
        serde_json::json!({ "graveyard": 0 }),
        "{sent:?}"
    );
    assert_eq!(moved["in"], serde_json::json!({ "hand": 0 }));
    assert_eq!(moved["objects"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_card_drag_crosses_a_panel_and_keeps_the_original_card() {
    use kit_ui::element::{Anchor, row, text};
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let card = slot_of(&view, ZoneName::Hand, Side::You, "Counterspell");
    let from = corners_of(&frame, card).iter().sum::<Vec2>() / 4.0;
    let graveyard = slot_of(&view, ZoneName::Graveyard, Side::You, "Brainstorm");
    let to = corners_of(&frame, graveyard).iter().sum::<Vec2>() / 4.0;
    let panel = from.lerp(to, 0.5) - Vec2::new(0.0, 130.0);
    let mut control = text("Panel")
        .width(100.0)
        .height(50.0)
        .on_press(app_screens::screens::settings::Message::Open);
    control.anchor = Some(Anchor::Point(panel, Vec2::splat(0.5)));
    client.layer_mut().show(Some(row().child(control)));
    client.frame(MS);
    client.handle(pointer(PointerPhase::Down, from, Button::Primary));
    client.handle(pointer(PointerPhase::Move, panel, Button::None));
    assert_eq!(client.pointer().held(), Some(usize::from(card.0)));
    client.frame(2 * MS);
    assert!(acts(&mut client).is_empty());

    client.handle(pointer(PointerPhase::Move, to, Button::None));
    client.handle(pointer(PointerPhase::Up, to, Button::Primary));
    let sent = json(&acts(&mut client));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["move"]["to"], serde_json::json!({ "graveyard": 0 }));
    assert_eq!(sent[0]["move"]["in"], serde_json::json!({ "hand": 0 }));
}

#[test]
fn the_graveyard_viewer_lists_its_cards_with_their_commands() {
    let mut builder = ViewBuilder::sample();
    builder.card(ZoneName::Graveyard, Side::You, "Counterspell");
    builder.script(ScriptBuilder::reset().build());
    let mut view = builder.build();
    view.table = Some(table(&view));
    let (mut client, frame) = client_at_table(&view);
    let brainstorm = slot_of(&view, ZoneName::Graveyard, Side::You, "Brainstorm");
    let pile = corners_of(&frame, brainstorm).iter().sum::<Vec2>() / 4.0;
    assert_eq!(
        target_at(&frame, pile),
        Target::Pile(Side::You, ZoneName::Graveyard)
    );
    client.handle(pointer(PointerPhase::Down, pile, Button::Secondary));
    let frame = client.frame(MS).clone();
    let row = middle(&frame, Role::MenuItem, "View graveyard");
    client.handle(pointer(PointerPhase::Down, row, Button::Primary));
    client.handle(pointer(PointerPhase::Up, row, Button::Primary));
    client.frame(2 * MS);
    // The cards fly into the viewer's boxes.
    let frame = client.frame(2_000 * MS).clone();
    for name in ["Brainstorm", "Counterspell"] {
        assert!(
            (frame.resolved.iter()).any(|element| element.role == Role::Card
                && frame.name(element.name).starts_with(name)
                && element.corners.iter().all(|corner| corner.y < 800.0)),
            "the viewer does not show {name}"
        );
    }

    // A right click on a card in the viewer opens that card's menu.
    let card = corners_of(&frame, brainstorm).iter().sum::<Vec2>() / 4.0;
    client.handle(pointer(PointerPhase::Down, card, Button::Secondary));
    let frame = client.frame(2_100 * MS).clone();
    assert!(
        (frame.resolved.iter())
            .any(|element| element.role == Role::List && frame.name(element.name) == "Brainstorm"),
        "the menu is not titled for the card"
    );
    let rows = menu_rows(&frame);
    for entry in commands_for(
        AppliesTo::PileCard,
        &view,
        &Context::new(true, &Bindings::new()),
    ) {
        let Some(placement) = entry.menu else {
            continue;
        };
        let row = placement.submenu.unwrap_or(entry.command.label);
        assert!(rows.contains(&row), "no row {row:?} in {rows:?}");
    }
}

#[test]
fn an_arrow_of_the_view_is_drawn_between_its_two_objects() {
    let view = table_view();
    let (mut client, _) = client_at_table(&view);
    let from = slot_of(&view, ZoneName::Battlefield, Side::You, "Serra Angel");
    let to = slot_of(&view, ZoneName::Battlefield, Side::Opponent, "Serra Angel");
    let (from_subject, to_subject) = (Subject::Object { slot: from }, Subject::Object { slot: to });
    let mut next = view.clone();
    if let Some(table) = &mut next.table {
        table.arrows.push(TableArrow {
            arrow: Arrow::HostileTarget,
            from: from_subject,
            to: to_subject,
            own: true,
        });
    }
    let point = Act::Point {
        span: Edge::Begin,
        arrow: Arrow::HostileTarget,
        from: from_subject,
        to: to_subject,
    };
    next.script = Some(script(None, vec![vec![step(point)]], vec![timing(200.0)]));
    client.handle(Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::SessionId(1),
        view: next,
    })));
    client.frame(MS);
    let frame = client.frame(1_000 * MS).clone();
    let arrow = (frame.resolved.iter())
        .find(|element| element.role == Role::Arrow)
        .expect("no arrow is drawn");
    let near = |point: Vec2, slot| {
        let corners = corners_of(&frame, slot);
        let centre = corners.iter().sum::<Vec2>() / 4.0;
        point.distance(centre) <= corners[0].distance(corners[2])
    };
    assert!(
        near(arrow.corners[0], from),
        "the arrow does not leave its source"
    );
    assert!(
        near(arrow.corners[3], to),
        "the arrow does not reach its target"
    );
}

#[test]
fn the_arrow_command_sends_an_arrow_to_the_card_clicked_next() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let angel = middle(&frame, Role::Card, "Serra Angel");
    let theirs = slot_of(&view, ZoneName::Battlefield, Side::Opponent, "Serra Angel");
    let target = corners_of(&frame, theirs).iter().sum::<Vec2>() / 4.0;
    client.handle(pointer(PointerPhase::Move, angel, Button::None));
    let alt = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    client.handle(key(KeyCode::A, alt));
    client.frame(MS);
    // An arrow runs from the card to the mouse while the command waits.
    client.handle(pointer(PointerPhase::Move, target, Button::None));
    let frame = client.frame(2 * MS).clone();
    assert!(
        (frame.resolved.iter()).any(|element| element.role == Role::Arrow),
        "no arrow follows the mouse"
    );
    client.handle(pointer(PointerPhase::Down, target, Button::Primary));
    client.handle(pointer(PointerPhase::Up, target, Button::Primary));
    let sent = json(&acts(&mut client));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(
        sent[0]["arrow"]["to"]["object"]["id"]["handle"],
        serde_json::json!(theirs.0),
        "{sent:?}"
    );
}

#[test]
fn a_key_over_the_table_acts_on_the_selected_cards() {
    let (mut client, _, _) = sweep_three_lands();
    assert!(
        acts(&mut client).is_empty(),
        "the sweep sent a table action"
    );
    // Option+L peeks at the three selected lands while the cursor rests on empty table.
    let option = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    client.handle(key(KeyCode::L, option));
    let sent = json(&acts(&mut client));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0]["peek"]["objects"].as_array().map(Vec::len), Some(3));
}

#[test]
fn cmd_d_over_the_opponents_life_draws_for_you() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let life = middle(&frame, Role::Player, "Opponent");
    assert_eq!(target_at(&frame, life), Target::Life(Side::Opponent));
    client.handle(pointer(PointerPhase::Move, life, Button::None));
    client.handle(key(KeyCode::D, cmd()));
    let expected = action(serde_json::json!({ "draw": { "seat": 0, "count": 1 } }));
    assert_eq!(acts(&mut client), vec![expected]);
}

#[test]
fn a_refused_action_plays_the_refusal_sound_once() {
    let view = table_view();
    let (mut client, _) = client_at_table(&view);
    client.handle(key(KeyCode::D, cmd()));
    assert_eq!(acts(&mut client).len(), 1);
    client.handle(Input::Reply(Reply::TableAct(TableActReply::Refused(
        TableRefused {
            refused: TableRefusalReason::Permission,
            blocked_by: None,
        },
    ))));
    let no = game_play::sound::id("no");
    let sounds = |client: &Client| {
        client
            .sounds()
            .iter()
            .map(|sound| sound.id)
            .collect::<Vec<_>>()
    };
    client.frame(MS);
    assert_eq!(sounds(&client), vec![no]);
    client.frame(2 * MS);
    assert_eq!(sounds(&client), Vec::new());
}

#[test]
fn a_drop_of_cards_from_two_zones_sends_one_batch() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let centre = |slot| corners_of(&frame, slot).iter().sum::<Vec2>() / 4.0;
    let angel = centre(slot_of(
        &view,
        ZoneName::Battlefield,
        Side::You,
        "Serra Angel",
    ));
    let counterspell = centre(slot_of(&view, ZoneName::Hand, Side::You, "Counterspell"));
    let graveyard = centre(slot_of(&view, ZoneName::Graveyard, Side::You, "Brainstorm"));
    let cmd_click = |at| {
        move |phase| {
            Input::Device(DeviceInput::Pointer(Pointer {
                id: 0,
                kind: PointerKind::Mouse,
                phase,
                position: at,
                button: Button::Primary,
                wheel: Vec2::ZERO,
                modifiers: cmd(),
            }))
        }
    };
    for (at, now) in [(angel, MS), (counterspell, 2 * MS)] {
        let click = cmd_click(at);
        client.handle(click(PointerPhase::Down));
        client.handle(click(PointerPhase::Up));
        client.frame(now);
    }
    drag(&mut client, counterspell, graveyard, 10 * MS);
    // One move for the permanent and one for the hand card, sent as one batch.
    let sent = json(&acts(&mut client));
    assert_eq!(sent.len(), 1, "{sent:?}");
    let moves = sent[0]["batch"]["actions"].as_array().expect("a batch");
    assert_eq!(moves.len(), 2, "{sent:?}");
    assert!(
        moves
            .iter()
            .all(|step| step["move"]["to"] == serde_json::json!({ "graveyard": 0 }))
    );
}
