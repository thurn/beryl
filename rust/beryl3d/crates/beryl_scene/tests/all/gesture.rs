//! What the primary pointer does between going down and coming up: a drag that begins on a
//! slider keeps setting it wherever it goes, a finger that scrolls a list presses none of its
//! rows, and a carry cut short by Cancel leaves nothing behind for the next press.

use beryl_protocol::prompt::{ButtonId, Click, ClickRequest, PromptButton, ShapeBody};
use beryl_protocol::script::Subject;
use beryl_scene::Client;
use beryl_scene::Input;
use beryl_scene::Press;
use beryl_scene::command::{Request, SessionCommand};
use beryl_scene::element::Action;
use beryl_scene::element::{row, text};
use beryl_scene::frame::{Role, StateBits};
use beryl_scene::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};
use beryl_scene::screens::Sheet;
use beryl_scene::widgets;
use glam::Vec2;

use super::flat_layer::{centre, client, deciding, edges, find, pointer, prompt};

/// A millisecond, in the nanoseconds `Client::frame` takes.
const MS: u64 = 1_000_000;

/// What pressing the prompt's Cancel button sends.
const DECLINE: Press = Press::Prompt(Click::Button {
    button: ButtonId::Decline,
});

/// A finger at `position`.
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

/// The buttons each click the client queued pressed, in order.
fn clicks(client: &mut Client) -> Vec<Click> {
    std::iter::from_fn(|| client.next_command())
        .map(|command| match command {
            SessionCommand::Click(ClickRequest { click, .. }) => click,
            other => panic!("a press queues a click, not {other:?}"),
        })
        .collect()
}

#[test]
fn a_drag_from_a_slider_let_go_on_a_button_sets_the_slider_and_presses_nothing() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.press(&Action::new(Press::Sheet(Some(Sheet::Settings))));
    let frame = client.frame(1);
    let slider = find(frame, Role::Slider, "Music");
    let (left, _, right, _) = edges(frame, slider);
    let y = centre(frame, slider).y;
    let close = centre(frame, find(frame, Role::Button, "close"));
    assert!(
        close.x > right,
        "the close button stands right of the slider's end"
    );
    client.handle(pointer(PointerPhase::Down, Vec2::new(left + 60.0, y)));
    client.handle(pointer(PointerPhase::Move, close));
    client.handle(pointer(PointerPhase::Up, close));
    let saved = std::iter::from_fn(|| client.next_request())
        .filter_map(|request| match request {
            Request::SaveSettings(settings) => Some(settings.music),
            _ => None,
        })
        .last();
    assert_eq!(saved, Some(100), "the move past the slider's end sets 100");
    let frame = client.frame(2);
    let state = frame.resolved[find(frame, Role::Slider, "Music")].state;
    assert!(
        !state.has(StateBits::HELD),
        "the released slider is not held"
    );
    assert_eq!(clicks(&mut client), Vec::new());
}

#[test]
fn a_finger_that_scrolls_a_list_presses_no_row_when_it_lifts() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.frame(0);
    let rows = (0..50).map(|n| text(&format!("Entry {n}")).on_press(DECLINE));
    (client.layer_mut()).show(Some(
        row()
            .name("log panel")
            .child(widgets::list("Log", rows).height(200.0).width(300.0)),
    ));
    let frame = client.frame(MS);
    let at = centre(frame, find(frame, Role::Text, "Entry 2"));
    // A tap on the row presses it, which shows that the row takes presses.
    client.handle(touch(PointerPhase::Down, at));
    client.handle(touch(PointerPhase::Up, at));
    assert_eq!(clicks(&mut client).len(), 1);
    client.frame(2 * MS);
    // The finger moves the list two points up, so the row stays under it.
    let moved = at - Vec2::new(0.0, 2.0);
    client.handle(touch(PointerPhase::Down, at));
    client.handle(touch(PointerPhase::Move, moved));
    client.handle(touch(PointerPhase::Up, moved));
    assert_eq!(clicks(&mut client), Vec::new(), "a scroll presses nothing");
}

#[test]
fn a_carry_ended_by_cancel_leaves_the_next_press_to_press_once() {
    let mut view = prompt("Order the triggers");
    view.buttons = vec![PromptButton {
        id: ButtonId::Decline,
        label: "Cancel".to_owned(),
        caption: String::new(),
        enabled: true,
        primary: false,
    }];
    view.body = ShapeBody::Order {
        items: vec![Subject::Ability { id: 1 }, Subject::Ability { id: 2 }],
    };
    let mut client = client();
    client.handle(deciding(view));
    client.frame(0);
    let frame = client.frame(MS);
    let item = (frame.resolved.iter())
        .position(|element| element.role == Role::Ability && element.interactive)
        .expect("an item of the row takes presses");
    let from = centre(frame, item);
    let cancel = centre(frame, find(frame, Role::Button, "Cancel"));
    client.handle(pointer(PointerPhase::Down, from));
    client.handle(pointer(PointerPhase::Move, from + Vec2::new(-80.0, 0.0)));
    client.handle(pointer(PointerPhase::Cancel, from + Vec2::new(-80.0, 0.0)));
    let frame = client.frame(2 * MS);
    let items = (frame.resolved.iter())
        .filter(|element| element.role == Role::Ability && element.interactive)
        .count();
    assert_eq!(items, 2, "the carried item is back in the row");
    client.handle(pointer(PointerPhase::Down, cancel));
    client.handle(pointer(PointerPhase::Up, cancel));
    assert_eq!(
        clicks(&mut client),
        vec![Click::Button {
            button: ButtonId::Decline
        }]
    );
}
