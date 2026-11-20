//! The manual table's controls beside the table itself, on views built by hand: the step track
//! and the Next button, the life totals, mana and player counters a click changes, the common
//! counter names of Add counter, and the question Quit asks.

use beryl_protocol::protocol::{Side, TablePhase};
use beryl_scene::frame::Role;
use beryl_scene::input::{Button, PointerPhase};

use super::manual::{
    action, acts, client_at_table, client_in, cmd, commands, key, middle, pointer, press,
    table_view,
};

#[test]
fn next_advances_to_the_phase_it_names() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let caption = (frame.resolved.iter()).find(|element| {
        element.role == Role::Text && frame.name(element.name) == "Beginning of combat"
    });
    assert!(
        caption.is_some(),
        "Next names the phase after the first main phase"
    );
    press(&mut client, 0, Role::Button, "Next");
    let phase = action(serde_json::json!({ "phase": { "phase": "beginning_of_combat" } }));
    assert_eq!(acts(&mut client), vec![phase]);
}

#[test]
fn a_step_icon_sets_the_phase_and_the_lit_draw_icon_draws() {
    let mut view = table_view();
    let (mut client, _) = client_at_table(&view);
    press(&mut client, 0, Role::Toggle, "Opponent's end step");
    let end = action(serde_json::json!({ "phase": { "phase": "end" } }));
    assert_eq!(acts(&mut client), vec![end]);

    if let Some(table) = view.table.as_mut() {
        table.phase = TablePhase::Draw;
    }
    view.turn.active = Side::You;
    let (mut client, _) = client_at_table(&view);
    press(&mut client, 0, Role::Toggle, "Your draw");
    let draw = action(serde_json::json!({ "draw": { "seat": 0, "count": 1 } }));
    assert_eq!(acts(&mut client), vec![draw]);
}

#[test]
fn cmd_q_asks_before_quitting_and_cancel_keeps_the_table() {
    use beryl_scene::command::{Request, SessionCommand};
    use beryl_scene::input::KeyCode;

    let view = table_view();
    let (mut client, _) = client_at_table(&view);
    client.handle(key(KeyCode::Q, cmd()));
    let frame = client.frame(0).clone();
    assert!(
        (frame.resolved.iter()).any(|element| frame.name(element.name) == "Quit Beryl?"),
        "Cmd+Q asks first"
    );
    assert!(
        !commands(&mut client).contains(&SessionCommand::TableLeave),
        "Cmd+Q does not leave the table at once"
    );
    press(&mut client, 0, Role::Button, "close");
    let frame = client.frame(0).clone();
    assert!(!(frame.resolved.iter()).any(|element| frame.name(element.name) == "Quit Beryl?"));
    let requests: Vec<Request> = std::iter::from_fn(|| client.next_request()).collect();
    assert!(
        !requests.contains(&Request::Quit),
        "Cancel keeps the table open"
    );

    client.handle(key(KeyCode::Q, cmd()));
    press(&mut client, 0, Role::Button, "Quit");
    let requests: Vec<Request> = std::iter::from_fn(|| client.next_request()).collect();
    assert!(requests.contains(&Request::Quit), "{requests:?}");
}

#[test]
fn a_click_taps_a_permanent_and_draws_from_the_library() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let island = middle(&frame, Role::Card, "Island");
    client.handle(pointer(PointerPhase::Down, island, Button::Primary));
    client.handle(pointer(PointerPhase::Up, island, Button::Primary));
    client.frame(1_000_000);
    let sent = acts(&mut client);
    let json = serde_json::to_value(&sent).unwrap();
    assert_eq!(json[0]["tap"]["tapped"], true, "{json}");
    assert_eq!(json[0]["tap"]["objects"].as_array().map(Vec::len), Some(1));

    let library = middle(&frame, Role::Pile, "Your library");
    client.handle(pointer(PointerPhase::Down, library, Button::Primary));
    client.handle(pointer(PointerPhase::Up, library, Button::Primary));
    client.frame(2_000_000);
    let draw = action(serde_json::json!({ "draw": { "seat": 0, "count": 1 } }));
    assert_eq!(acts(&mut client), vec![draw]);
}

#[test]
fn a_click_on_a_life_total_opens_set_life() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let life = middle(&frame, Role::Player, "Opponent ");
    client.handle(pointer(PointerPhase::Down, life, Button::Primary));
    client.handle(pointer(PointerPhase::Up, life, Button::Primary));
    let frame = client.frame(0).clone();
    assert!(
        (frame.resolved.iter()).any(|element| frame.name(element.name) == "Set life"),
        "the number prompt asks for the life total"
    );
}

#[test]
fn a_click_on_a_mana_symbol_adds_one_and_a_right_click_removes_one() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let blue = middle(&frame, Role::Button, "U counter: 0");
    client.handle(pointer(PointerPhase::Down, blue, Button::Primary));
    client.handle(pointer(PointerPhase::Up, blue, Button::Primary));
    let add = action(serde_json::json!({ "player_counter": { "seat": 0, "name": "U", "add": 1 } }));
    assert_eq!(acts(&mut client), vec![add]);
    client.handle(pointer(PointerPhase::Down, blue, Button::Secondary));
    let remove =
        action(serde_json::json!({ "player_counter": { "seat": 0, "name": "U", "add": -1 } }));
    assert_eq!(acts(&mut client), vec![remove]);
}

#[test]
fn a_common_counter_name_adds_that_counter() {
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let angel = middle(&frame, Role::Card, "Serra Angel");
    client.handle(pointer(PointerPhase::Down, angel, Button::Secondary));
    press(&mut client, 0, Role::MenuItem, "Add counter");
    press(&mut client, 0, Role::Button, "charge");
    let sent = serde_json::to_value(acts(&mut client)).unwrap();
    assert_eq!(sent[0]["counter"]["name"], "charge", "{sent}");
    assert_eq!(sent[0]["counter"]["add"], 1, "{sent}");
}

#[test]
fn a_refused_click_shakes_the_card_clicked_and_an_accepted_one_does_not() {
    use beryl_protocol::ipc::{TableActReply, TableRefusalReason, TableRefused};
    use beryl_scene::command::Reply;
    use beryl_scene::input::Input;

    const MS: u64 = 1_000_000;
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let island = middle(&frame, Role::Card, "Island");
    let across = |client: &mut beryl_scene::Client, now: u64| {
        let frame = client.frame(now).clone();
        middle(&frame, Role::Card, "Island").x - island.x
    };
    let click = |client: &mut beryl_scene::Client| {
        client.handle(pointer(PointerPhase::Down, island, Button::Primary));
        client.handle(pointer(PointerPhase::Up, island, Button::Primary));
        client.frame(MS);
        assert_eq!(acts(client).len(), 1, "the click taps the Island");
    };

    click(&mut client);
    client.handle(Input::Reply(Reply::TableAct(TableActReply::Refused(
        TableRefused {
            refused: TableRefusalReason::Permission,
            blocked_by: None,
        },
    ))));
    // The shake starts at the first frame after the reply.
    assert!(across(&mut client, 10 * MS).abs() < 0.01);
    let shaken: Vec<f32> = (1..12)
        .map(|ms| across(&mut client, (10 + 10 * ms) * MS))
        .collect();
    assert!(
        shaken.iter().any(|x| *x < -2.0) && shaken.iter().any(|x| *x > 1.0),
        "the Island shakes left and right: {shaken:?}"
    );
    assert!(
        across(&mut client, 400 * MS).abs() < 0.01,
        "and comes to rest where it was"
    );

    click(&mut client);
    client.handle(Input::Reply(Reply::TableAct(TableActReply::Ok {
        ok: true,
    })));
    let still: Vec<f32> = (0..12)
        .map(|ms| across(&mut client, (500 + 10 * ms) * MS))
        .collect();
    assert!(
        still.iter().all(|x| x.abs() < 0.01),
        "an accepted click shakes nothing: {still:?}"
    );
}

#[test]
fn a_refused_counter_press_shakes_that_button_and_an_accepted_one_does_not() {
    use beryl_protocol::ipc::{TableActReply, TableRefusalReason, TableRefused};
    use beryl_scene::command::Reply;
    use beryl_scene::input::Input;

    const MS: u64 = 1_000_000;
    let view = table_view();
    let (mut client, frame) = client_at_table(&view);
    let blue = middle(&frame, Role::Button, "U counter: 0");
    let green = middle(&frame, Role::Button, "G counter: 0");
    let across = |client: &mut beryl_scene::Client, now: u64, name: &str| {
        let frame = client.frame(now).clone();
        middle(&frame, Role::Button, name).x
    };
    let click = |client: &mut beryl_scene::Client| {
        client.handle(pointer(PointerPhase::Down, blue, Button::Primary));
        client.handle(pointer(PointerPhase::Up, blue, Button::Primary));
        client.frame(MS);
        assert_eq!(acts(client).len(), 1, "the click adds blue mana");
    };

    click(&mut client);
    client.handle(Input::Reply(Reply::TableAct(TableActReply::Refused(
        TableRefused {
            refused: TableRefusalReason::Permission,
            blocked_by: None,
        },
    ))));
    // The shake starts at the first frame after the reply.
    assert!((across(&mut client, 10 * MS, "U counter: 0") - blue.x).abs() < 0.01);
    let shaken: Vec<f32> = (1..12)
        .map(|ms| across(&mut client, (10 + 10 * ms) * MS, "U counter: 0") - blue.x)
        .collect();
    assert!(
        shaken.iter().any(|x| *x < -2.0) && shaken.iter().any(|x| *x > 1.0),
        "the blue button shakes left and right: {shaken:?}"
    );
    assert!(
        (across(&mut client, 300 * MS, "G counter: 0") - green.x).abs() < 0.01,
        "the buttons beside it stay still"
    );
    assert!(
        (across(&mut client, 400 * MS, "U counter: 0") - blue.x).abs() < 0.01,
        "and it comes to rest where it was"
    );

    click(&mut client);
    client.handle(Input::Reply(Reply::TableAct(TableActReply::Ok {
        ok: true,
    })));
    let still: Vec<f32> = (0..12)
        .map(|ms| across(&mut client, (500 + 10 * ms) * MS, "U counter: 0") - blue.x)
        .collect();
    assert!(
        still.iter().all(|x| x.abs() < 0.01),
        "an accepted press shakes nothing: {still:?}"
    );
}

#[test]
fn on_a_phone_the_mana_button_opens_a_sheet_that_adds_and_removes_mana() {
    let view = table_view();
    let (mut client, frame) = client_in(&view, 393.0, 759.0);
    let names = |frame: &beryl_scene::frame::Frame| {
        (frame.resolved.iter())
            .map(|element| frame.name(element.name).to_owned())
            .collect::<Vec<_>>()
    };
    assert!(
        !names(&frame).iter().any(|name| name == "U counter: 0"),
        "the phone shows no row of counters"
    );
    press(&mut client, 0, Role::Button, "Your mana: 0");
    let frame = client.frame(0).clone();
    assert!(
        names(&frame).contains(&"Your mana and counters".to_owned()),
        "the Mana button opens the sheet"
    );
    press(&mut client, 0, Role::Button, "U counter: 0");
    let add = action(serde_json::json!({ "player_counter": { "seat": 0, "name": "U", "add": 1 } }));
    assert_eq!(acts(&mut client), vec![add]);
    press(&mut client, 0, Role::Button, "Remove U");
    let remove =
        action(serde_json::json!({ "player_counter": { "seat": 0, "name": "U", "add": -1 } }));
    assert_eq!(acts(&mut client), vec![remove]);
    press(&mut client, 0, Role::Button, "Done");
    let frame = client.frame(0).clone();
    assert!(
        !names(&frame).contains(&"Your mana and counters".to_owned()),
        "Done closes the sheet"
    );
}
