//! The manual table's lobby, on lobbies built by hand: the start screen's 'Solo table' opens
//! it, its controls send the session lobby changes, and the lobby gives way to the table when
//! its game starts and comes back when a game ends. The deck picker loads a saved deck or a
//! pasted decklist, and the sideboard swap moves cards between the main deck and sideboard.

use app_client::Input;
use app_client::command::{Reply, Request, SessionCommand};
use app_client::{Client, Config};
use beryl_protocol::SessionError;
use beryl_protocol::ipc::decks::{DeckImportReply, DeckImportRequest, SavedDeck};
use beryl_protocol::ipc::{LobbyChange, LobbyUpdate, TableRefusalReason, TableRefused};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, lobby, table};
use glam::Vec2;
use kit_frame::frame::{Frame, Role};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Size,
};

/// A client on the Mac in a 1600 by 900 window, on the start screen.
fn client_on_start() -> Client {
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
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Agents(Vec::new()));
    client
}

/// The frame the client draws next.
fn frame(client: &mut Client) -> Frame {
    // Each frame is a second after the last, so that sections a change moves on the window have
    // finished gliding to their new places before a test presses one.
    thread_local!(static NOW: std::cell::Cell<u64> = const { std::cell::Cell::new(0) });
    let now = NOW.with(|now| {
        now.set(now.get() + 1_000_000_000);
        now.get()
    });
    client.frame(now).clone()
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
        client.handle(Input::Device(DeviceInput::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position: at,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        })));
    }
}

/// The session commands the client has queued, in order.
fn commands(client: &mut Client) -> Vec<SessionCommand> {
    std::iter::from_fn(|| client.next_command()).collect()
}

/// The lobby changes the client has queued, in order.
fn changes(client: &mut Client) -> Vec<LobbyChange> {
    (commands(client).into_iter())
        .filter_map(|command| match command {
            SessionCommand::TableLobby(change) => Some(change),
            _ => None,
        })
        .collect()
}

/// A deck saved in the library as `name`: 36 Islands and 4 Counterspells, with 8 Negates in
/// its sideboard.
fn saved(name: &str) -> SavedDeck {
    SavedDeck {
        deck: imported(name),
        main_count: 40,
        side_count: 8,
    }
}

/// The deck the session reads a decklist as and saves as `name`.
fn imported(name: &str) -> DeckImportReply {
    DeckImportReply {
        name: name.to_owned(),
        main: vec![("Island".to_owned(), 36), ("Counterspell".to_owned(), 4)],
        side: vec![("Negate".to_owned(), 8)],
        unknown: Vec::new(),
        skipped: Vec::new(),
    }
}

/// Focuses the field named `name` and pastes `words` into it, as the host hands back the
/// clipboard.
fn paste(client: &mut Client, name: &str, words: &str) {
    press(client, Role::Field, name);
    client.handle(Input::Device(DeviceInput::Paste(words.to_owned())));
}

/// A client in a solo table's lobby with a deck loaded whose main deck holds `main` and whose
/// sideboard holds `side`.
fn client_with_deck(main: &[(&str, u16)], side: &[(&str, u16)]) -> Client {
    let own = |cards: &[(&str, u16)]| {
        (cards.iter())
            .map(|(name, count)| ((*name).to_owned(), *count))
            .collect()
    };
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Solo table");
    let mut update = lobby(true);
    let view = update.view.as_mut().expect("a lobby with a state");
    view.main = own(main);
    view.side = own(side);
    client.handle(Input::Lobby(Box::new(update)));
    commands(&mut client);
    client
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
    assert_eq!(commands(&mut client), vec![SessionCommand::DeckList]);
    client.handle(Input::Reply(Reply::DeckList(Ok(vec![saved("Control")]))));
    press(&mut client, Role::Button, "Control");
    let [LobbyChange::LoadDeck { main, side }] = &changes(&mut client)[..] else {
        panic!("picking a saved deck did not load it");
    };
    assert_eq!(main, &saved("Control").deck.main);
    assert_eq!(side, &saved("Control").deck.side);

    client.handle(Input::Lobby(Box::new(lobby(true))));
    let shown = frame(&mut client);
    assert!(
        reads(&shown, "Control"),
        "the lobby shows the loaded deck's name"
    );
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
fn copy_puts_the_full_invite_on_the_clipboard() {
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Host a table");
    paste(&mut client, "table name", "Host");
    press(&mut client, Role::Button, "Host");

    let invite = format!("beryl:{}", "g".repeat(150));
    let mut update = lobby(false);
    let view = update.view.as_mut().unwrap();
    view.invite = Some(invite.clone());
    let mut guest = view.seats[0].clone();
    guest.seat.0 = 1;
    guest.name = None;
    view.seats.push(guest);
    client.handle(Input::Lobby(Box::new(update)));
    client.handle(Input::Reply(Reply::TableOpen(Ok(()))));

    while client.next_request().is_some() {}
    press(&mut client, Role::Button, "Copy");
    assert_eq!(client.next_request(), Some(Request::Copy(invite)));
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

#[test]
fn a_solo_table_that_does_not_open_says_so_on_the_start_screen() {
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Solo table");
    client.handle(Input::Reply(Reply::TableLobby(Err(TableRefused {
        refused: TableRefusalReason::Invalid,
        blocked_by: None,
    }))));
    let shown = frame(&mut client);
    assert!(has(&shown, Role::Region, "start"));
    assert!(reads(&shown, "Could not open the table."));

    // Trying again clears the message while the lobby is awaited.
    press(&mut client, Role::Button, "Solo table");
    assert!(!reads(&frame(&mut client), "Could not open the table."));
}

#[test]
fn a_pasted_decklist_is_saved_and_loaded_under_its_name() {
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Solo table");
    client.handle(Input::Lobby(Box::new(lobby(false))));
    commands(&mut client);
    press(&mut client, Role::Button, "Change");
    client.handle(Input::Reply(Reply::DeckList(Ok(Vec::new()))));
    press(&mut client, Role::Button, "Paste decklist");
    paste(&mut client, "deck name", "Islands");
    paste(
        &mut client,
        "decklist",
        "36 Island\n4 Counterspell\n\n8 Negate",
    );
    commands(&mut client);
    press(&mut client, Role::Button, "Import");
    assert_eq!(
        commands(&mut client),
        vec![SessionCommand::DeckImport(DeckImportRequest {
            name: "Islands".to_owned(),
            text: Some("36 Island\n4 Counterspell\n\n8 Negate".to_owned()),
            cod: None,
        })]
    );

    // A decklist the library cannot save says why, and can be imported again.
    client.handle(Input::Reply(Reply::DeckImport(Err(SessionError::Io {
        reason: "The disk is full.".to_owned(),
    }))));
    assert!(reads(&frame(&mut client), "The disk is full."));
    press(&mut client, Role::Button, "Import");
    commands(&mut client);
    client.handle(Input::Reply(Reply::DeckImport(Ok(imported("Islands")))));
    let deck = imported("Islands");
    assert_eq!(
        changes(&mut client),
        vec![LobbyChange::LoadDeck {
            main: deck.main,
            side: deck.side,
        }]
    );
    client.handle(Input::Lobby(Box::new(lobby(true))));
    let shown = frame(&mut client);
    assert!(reads(&shown, "Islands"));
    assert!(!has(&shown, Role::Field, "decklist"), "the picker closed");
}

#[test]
fn the_paste_key_opens_the_paste_field() {
    let mut client = client_on_start();
    press(&mut client, Role::Button, "Solo table");
    client.handle(Input::Lobby(Box::new(lobby(false))));
    let modifiers = Modifiers {
        command: true,
        shift: true,
        ..Modifiers::default()
    };
    for phase in [KeyPhase::Down, KeyPhase::Up] {
        client.handle(Input::Device(DeviceInput::Key(Key {
            code: KeyCode::V,
            phase,
            modifiers,
        })));
    }
    assert!(has(&frame(&mut client), Role::Field, "decklist"));
}

#[test]
fn the_sideboard_swap_moves_one_copy_each_way() {
    let mut client = client_with_deck(&[("Island", 36), ("Counterspell", 4)], &[("Negate", 8)]);
    press(&mut client, Role::Button, "Sideboard");
    press(&mut client, Role::Button, "Move Counterspell to sideboard");
    press(&mut client, Role::Button, "Move Negate to main deck");
    assert_eq!(
        changes(&mut client),
        vec![
            LobbyChange::Sideboard {
                into_main: Vec::new(),
                into_side: vec![("Counterspell".to_owned(), 1)],
            },
            LobbyChange::Sideboard {
                into_main: vec![("Negate".to_owned(), 1)],
                into_side: Vec::new(),
            },
        ]
    );
    press(&mut client, Role::Button, "Done");
    assert!(!has(
        &frame(&mut client),
        Role::Button,
        "Move Negate to main deck"
    ));
}
