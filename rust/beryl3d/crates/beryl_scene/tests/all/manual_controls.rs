//! The manual table's controls beside the table itself, on views built by hand: the step track
//! and the Next button, the life totals, mana and player counters a click changes, the common
//! counter names of Add counter, and the question Quit asks.

use beryl_protocol::protocol::{Side, TablePhase};
use beryl_scene::frame::Role;

use super::manual::{action, acts, client_at_table, cmd, commands, key, press, table_view};

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
    press(&mut client, 0, Role::Button, "Opponent's end step");
    let end = action(serde_json::json!({ "phase": { "phase": "end" } }));
    assert_eq!(acts(&mut client), vec![end]);

    if let Some(table) = view.table.as_mut() {
        table.phase = TablePhase::Draw;
    }
    view.turn.active = Side::You;
    let (mut client, _) = client_at_table(&view);
    press(&mut client, 0, Role::Button, "Your draw");
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
