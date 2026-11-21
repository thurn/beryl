//! The manual table's guided start and its connection, on views built by hand: the play or
//! draw choice, keeping or mulliganing an opening hand and putting cards on the bottom, and the
//! input a dropped connection freezes until the table's next view.

use app_client::Input;
use app_client::command::SessionCommand;
use app_client::{Client, Config};
use beryl_protocol::protocol::{Answer, LinkState, TablePrompt};
use game_manual::manual::opening::prompt;
use game_probe::testing::{ScriptBuilder, ViewBuilder, table};
use glam::Vec2;
use kit_frame::frame::{Frame, Role};
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Size,
};

/// A sample view at a manual table, at rest, with `change` made to its table.
fn table_view(
    change: impl FnOnce(&mut beryl_protocol::protocol::TableView),
) -> beryl_protocol::PlayerView {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let mut view = builder.build();
    let mut shown = table(&view);
    change(&mut shown);
    view.table = Some(shown);
    view
}

/// A client on the Mac in a 1600 by 900 window that has been given `view`.
fn client_at(view: &beryl_protocol::PlayerView) -> Client {
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
    give(&mut client, view);
    client
}

fn give(client: &mut Client, view: &beryl_protocol::PlayerView) {
    client.handle(Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::SessionId(1),
        view: view.clone(),
    })));
}

/// The middle of the first element of `frame` with `role` named `name`.
fn middle(frame: &Frame, role: Role, name: &str) -> Option<Vec2> {
    let element = (frame.resolved.iter())
        .find(|element| element.role == role && frame.name(element.name) == name)?;
    Some(element.corners.iter().copied().sum::<Vec2>() / 4.0)
}

/// Clicks the element with `role` named `name` in the frame at `now`.
fn press(client: &mut Client, now: u64, role: Role, name: &str) {
    let frame = client.frame(now).clone();
    let at = middle(&frame, role, name).unwrap_or_else(|| panic!("no {role:?} {name:?}"));
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

/// The commands the client has queued, in order.
fn commands(client: &mut Client) -> Vec<SessionCommand> {
    std::iter::from_fn(|| client.next_command()).collect()
}

/// The answers among `commands`.
fn answers(commands: &[SessionCommand]) -> Vec<Answer> {
    (commands.iter())
        .filter_map(|command| match command {
            SessionCommand::TableAnswer(answer) => Some(answer.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn choosing_play_sends_the_pick_and_confirms_it() {
    let view = table_view(|table| {
        table.choosing_first = true;
        table.prompt = Some(TablePrompt::Choose {
            title: "You won the roll".to_owned(),
            options: vec!["Play".to_owned(), "Draw".to_owned()],
            selected: None,
        });
    });
    let mut client = client_at(&view);
    press(&mut client, 0, Role::Button, "Play");
    assert_eq!(
        answers(&commands(&mut client)),
        vec![Answer::Pick { option: 0 }, Answer::Done]
    );
}

#[test]
fn after_a_mulligan_a_card_goes_to_the_bottom_and_done_confirms() {
    let view = table_view(|table| {
        table.prompt = Some(TablePrompt::Opening {
            hand: table.orders.you.hand.clone(),
            mulligans: 1,
            to_bottom: Some(1),
            bottom: Vec::new(),
        });
    });
    let first = view.table.as_ref().unwrap().orders.you.hand[0];
    let name = (view.objects.iter())
        .find(|object| object.slot == first)
        .and_then(|object| object.name.clone())
        .unwrap();
    let mut client = client_at(&view);
    press(
        &mut client,
        0,
        Role::Region,
        &format!("{name} #{}", first.0),
    );
    assert_eq!(
        answers(&commands(&mut client)),
        vec![Answer::Bottom { card: first }]
    );
    press(&mut client, 0, Role::Button, "Done");
    assert_eq!(answers(&commands(&mut client)), vec![Answer::Done]);
}

#[test]
fn an_opening_hand_offers_keep_and_mulligan() {
    let view = table_view(|table| {
        table.prompt = Some(TablePrompt::Opening {
            hand: table.orders.you.hand.clone(),
            mulligans: 0,
            to_bottom: None,
            bottom: Vec::new(),
        });
    });
    let mut client = client_at(&view);
    press(&mut client, 0, Role::Button, "Mulligan");
    assert_eq!(answers(&commands(&mut client)), vec![Answer::Mulligan]);
    press(&mut client, 0, Role::Button, "Keep");
    assert_eq!(answers(&commands(&mut client)), vec![Answer::Keep]);
}

#[test]
fn a_dropped_connection_freezes_the_keys_until_the_link_returns() {
    let mut view = table_view(|table| {
        table.link = LinkState::Reconnecting;
        table.peer_name = Some("Alex".to_owned());
    });
    let mut client = client_at(&view);
    let frame = client.frame(0).clone();
    assert!(middle(&frame, Role::Text, "Alex is reconnecting").is_some());
    let draw = Input::Key(Key {
        code: KeyCode::D,
        phase: KeyPhase::Down,
        modifiers: Modifiers {
            command: true,
            ..Modifiers::default()
        },
    });
    client.handle(draw.clone());
    client.frame(0);
    assert!(
        !(commands(&mut client).iter())
            .any(|command| matches!(command, SessionCommand::TableAct(_))),
        "a frozen table draws no card"
    );
    view.table.as_mut().unwrap().link = LinkState::Connected {
        direct: true,
        round_trip_ms: None,
    };
    give(&mut client, &view);
    let frame = client.frame(MS).clone();
    assert!(middle(&frame, Role::Text, "Alex is reconnecting").is_none());
    client.handle(draw);
    assert!(
        (commands(&mut client).iter())
            .any(|command| matches!(command, SessionCommand::TableAct(_))),
        "the restored table draws a card"
    );
}

#[test]
fn a_table_the_other_player_left_offers_lobby() {
    let view = table_view(|table| {
        table.link = LinkState::Left;
        table.peer_name = Some("Alex".to_owned());
    });
    let mut client = client_at(&view);
    press(&mut client, 0, Role::Button, "Lobby");
    assert!(commands(&mut client).contains(&SessionCommand::TableLeave));
}

/// Nanoseconds in a millisecond, the unit of `Client::frame`'s clock.
const MS: u64 = 1_000_000;

#[test]
fn a_choice_that_is_not_the_start_is_no_opening() {
    let mut view = table_view(|table| {
        table.prompt = Some(TablePrompt::Choose {
            title: "Search".to_owned(),
            options: vec!["One".to_owned()],
            selected: None,
        });
    });
    assert!(prompt(&view).is_none());
    view.table.as_mut().expect("a table").choosing_first = true;
    assert!(prompt(&view).is_some());
}
