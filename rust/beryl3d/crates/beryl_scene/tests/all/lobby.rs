//! The manual table's lobby, on lobbies built by hand: the start screen's 'Solo table' opens
//! it, its controls send the session lobby changes, and the lobby gives way to the table when
//! its game starts and comes back when a game ends.

use beryl_protocol::ipc::{LobbyChange, LobbyUpdate, TableRefusalReason, TableRefused};
use beryl_scene::command::{Reply, SessionCommand};
use beryl_scene::frame::{Frame, Role};
use beryl_scene::input::{
    Button, Input, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size,
};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, lobby, table};
use beryl_scene::{Client, Config};
use glam::Vec2;

/// A client on the Mac in a 1600 by 900 window, on the start screen.
fn client_on_start() -> Client {
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
    client.handle(Input::Agents(Vec::new()));
    client
}

/// The frame the client draws next.
fn frame(client: &mut Client) -> Frame {
    client.frame(0).clone()
}

/// Whether `frame` has an element named `name` with `role`.
fn has(frame: &Frame, role: Role, name: &str) -> bool {
    (frame.resolved.iter()).any(|element| element.role == role && frame.name(element.name) == name)
}

/// Whether `frame` holds a text that reads `words`.
fn reads(frame: &Frame, words: &str) -> bool {
    (frame.resolved.iter()).any(|element| frame.name(element.name) == words)
}

/// Presses the element named `name` with `role` in the client's next frame.
fn press(client: &mut Client, role: Role, name: &str) {
    let frame = frame(client);
    let element = (frame.resolved.iter())
        .find(|element| element.role == role && frame.name(element.name) == name)
        .unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
    let at = element.corners.iter().copied().sum::<Vec2>() / 4.0;
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        client.handle(Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position: at,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
    }
}

/// The lobby changes the client has queued, in order.
fn changes(client: &mut Client) -> Vec<LobbyChange> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::TableLobby(change) => Some(change),
            _ => None,
        })
        .collect()
}

/// A view of the solo table, as the session sends one when its game has started.
fn table_update() -> Input {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let mut view = builder.build();
    view.table = Some(table(&view));
    Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::SessionId(1),
        view,
    }))
}

#[test]
fn solo_table_opens_a_lobby_that_loads_the_deck_and_starts_the_table() {
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Solo table");
    assert_eq!(changes(&mut client), vec![LobbyChange::Solo]);
    assert!(has(&frame(&mut client), Role::Region, "lobby"));

    // The table sends its view while it is still in its lobby, and then the lobby: both keep
    // the lobby screen.
    client.handle(table_update());
    client.handle(Input::Lobby(Box::new(lobby(false))));
    let shown = frame(&mut client);
    assert!(has(&shown, Role::Region, "lobby"));
    assert!(reads(&shown, "No deck loaded"));

    press(&mut client, Role::Button, "Change");
    let [LobbyChange::LoadDeck { main, side }] = &changes(&mut client)[..] else {
        panic!("'Change' did not load a deck");
    };
    assert_eq!(main.iter().map(|(_, count)| count).sum::<u16>(), 40);
    assert!(!side.is_empty());

    client.handle(Input::Lobby(Box::new(lobby(true))));
    let shown = frame(&mut client);
    assert!(reads(&shown, "Deck loaded"));
    assert!(reads(&shown, "40 main · 8 sideboard"));
    press(&mut client, Role::Button, "Start");
    assert_eq!(changes(&mut client), vec![LobbyChange::Start]);

    // The game started: the lobby gives way to the table.
    client.handle(table_update());
    client.handle(Input::Lobby(Box::new(LobbyUpdate {
        view: None,
        game: None,
    })));
    assert!(has(&frame(&mut client), Role::Region, "table"));

    // A game that ends there brings the lobby back.
    client.handle(Input::Lobby(Box::new(lobby(true))));
    assert!(has(&frame(&mut client), Role::Region, "lobby"));
}

#[test]
fn start_waits_for_a_deck() {
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Solo table");
    client.handle(Input::Lobby(Box::new(lobby(false))));
    changes(&mut client);
    press(&mut client, Role::Button, "Start");
    assert_eq!(changes(&mut client), Vec::new());
}

#[test]
fn the_assists_turn_on_and_off() {
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Solo table");
    client.handle(Input::Lobby(Box::new(lobby(true))));
    changes(&mut client);
    press(&mut client, Role::Toggle, "Turn upkeep");
    press(&mut client, Role::Toggle, "Combat");
    let changes: Vec<serde_json::Value> = (changes(&mut client).iter())
        .map(|change| serde_json::to_value(change).unwrap())
        .collect();
    assert_eq!(
        changes,
        [
            serde_json::json!({ "kind": "set_assist", "assist": "turn_upkeep", "on": false }),
            serde_json::json!({ "kind": "set_assist", "assist": "combat", "on": true }),
        ]
    );
}

#[test]
fn a_refused_change_says_so_and_leave_goes_back_to_the_start_screen() {
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Solo table");
    client.handle(Input::Lobby(Box::new(lobby(true))));
    client.handle(Input::Reply(Reply::TableLobby(Err(TableRefused {
        refused: TableRefusalReason::Permission,
        blocked_by: None,
    }))));
    assert!(reads(&frame(&mut client), "Could not update the lobby."));

    press(&mut client, Role::Button, "Leave table");
    let left = std::iter::from_fn(|| client.next_command())
        .any(|command| command == SessionCommand::TableLeave);
    assert!(left, "'Leave table' did not leave the table");
    assert!(has(&frame(&mut client), Role::Region, "start"));
}
