//! The app's shortcuts and its flat layer's anchors to game objects.

use app_client::Input;
use app_client::command::{Request, SessionCommand};
use beryl_protocol::prompt::{ButtonId, Click, ClickRequest};
use beryl_protocol::script::Subject;
use game_hud::hud::anchors::{anchor_target, spot_of};
use game_table::table::Table;
use glam::Vec2;
use kit_frame::frame::{ElementId, Frame, FrameBuilder, Role, StateBits};
use kit_ui::element::{Anchor, Chord, row, text};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Key, KeyCode, KeyPhase, Modifiers, Tier};
use kit_ui::layout::Layer;

use crate::common::{MS, before, empty_frame, show, three_beats};
use crate::flat_layer::{client, deciding, decision, prompt};

/// The resolved element of `role` named `name`: its id and its box as (left, top, width,
/// height).
fn find(frame: &Frame, role: Role, name: &str) -> (ElementId, [f32; 4]) {
    let element = (frame.resolved.iter())
        .find(|element| element.role == role && frame.name(element.name) == name)
        .unwrap_or_else(|| panic!("no {role:?} named {name:?} in the frame"));
    let min = element.corners.into_iter().reduce(Vec2::min).unwrap();
    let max = element.corners.into_iter().reduce(Vec2::max).unwrap();
    (element.id, [min.x, min.y, max.x - min.x, max.y - min.y])
}

#[test]
fn an_element_anchored_to_a_card_stays_on_it_while_it_flies() {
    let (start, cards) = before();
    let (flight, timings) = three_beats(&start, cards);
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(flight), timings);
    let land = Subject::Object { slot: cards.land };
    let mut layer = Layer::new();
    layer.show(Some(row().name("screen").child({
        let mut badge = text("Plains").padding(4.0);
        badge.anchor = Some(Anchor::Object(anchor_target(land), Vec2::new(0.5, 0.5)));
        badge
    })));
    layer.take_work();

    let mut frame = empty_frame();
    let mut centres = Vec::new();
    let mut now = 0;
    while now <= 600 * MS {
        let mut builder = FrameBuilder::new(
            &mut frame,
            Tier::Baseline,
            crate::test_data::data().card.size(),
        );
        show.frame(now, &mut builder);
        let card = builder.centre(ElementId(u32::from(cards.land.0)));
        layer.follow(|subject| spot_of(&builder, show.table(), subject));
        let (layout_nodes, text_runs) = layer.take_work();
        assert_eq!((layout_nodes, text_runs), (0, 0), "no layout at {now}");
        layer.write(&mut builder, &show.table().camera().screen_plane(), true);
        let frame = builder.finish();
        let card = card.expect("the table draws the Plains");
        let [x, y, width, height] = find(frame, Role::Text, "Plains").1;
        let badge = Vec2::new(x + width / 2.0, y + height / 2.0);
        assert!(
            badge.distance(card) < 0.01,
            "{badge} off the card at {card}"
        );
        centres.push(card);
        now += 16 * MS;
    }
    let travelled = centres.first().unwrap().distance(*centres.last().unwrap());
    assert!(travelled > 100.0, "the Plains flew only {travelled} points");
}

#[test]
fn an_element_anchored_to_a_card_the_table_does_not_show_is_not_drawn() {
    let mut layer = Layer::new();
    layer.show(Some(row().name("screen").child({
        let mut badge = text("Gone");
        let slot = beryl_protocol::protocol::ObjectId(40);
        badge.anchor = Some(Anchor::Object(
            anchor_target(Subject::Object { slot }),
            Vec2::ZERO,
        ));
        badge
    })));
    layer.follow(|_| None);
    let mut frame = Frame::with_capacity(64, 256, 4096);
    let mut builder = FrameBuilder::new(
        &mut frame,
        Tier::Baseline,
        crate::test_data::data().card.size(),
    );
    let table = Table::new(
        crate::test_data::data(),
        game_table::table::Window::default(),
    );
    layer.write(&mut builder, &table.camera().screen_plane(), true);
    let frame = builder.finish();
    assert!(
        !(frame.resolved.iter()).any(|element| frame.name(element.name) == "Gone"),
        "no element for a card that is not shown"
    );
    assert_eq!(layer.press_at(Vec2::ZERO, true), None);
}

/// A key pressed with `modifiers`.
fn key(code: KeyCode, modifiers: Modifiers) -> Input {
    Input::Device(DeviceInput::Key(Key {
        code,
        phase: KeyPhase::Down,
        modifiers,
    }))
}

/// The index in `frame.resolved` of the element named `name`.
fn find_index(frame: &Frame, name: &str) -> usize {
    (frame.resolved.iter())
        .position(|element| frame.name(element.name) == name)
        .unwrap_or_else(|| panic!("no element named {name:?}"))
}

#[test]
fn tab_moves_the_focus_to_the_next_button_and_return_presses_it() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.frame(0);
    // The dock stacks Cancel over the turn button, Auto Pay, and Tab follows the dock's order
    // from the top, as it follows the web's.
    client.handle(key(KeyCode::Tab, Modifiers::default()));
    let frame = client.frame(MS);
    let cancel = &frame.resolved[find_index(frame, "Cancel")];
    assert!(cancel.state.has(StateBits::FOCUSED), "Tab went to Cancel");
    assert!(
        !frame.resolved[find_index(frame, "Auto Pay")]
            .state
            .has(StateBits::FOCUSED)
    );
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    assert_eq!(
        client.next_command(),
        Some(SessionCommand::Click(ClickRequest {
            decision: decision(),
            revision: 3,
            click: Click::Button {
                button: ButtonId::Decline
            },
        }))
    );
}

#[test]
fn cmd_b_opens_the_bug_report_and_ctrl_b_does_off_the_mac() {
    let command = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    let control = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    let opens = |platform: Option<bool>, modifiers: Modifiers| {
        let mut client = client();
        if let Some(mac) = platform {
            client.handle(Input::Device(DeviceInput::Platform { mac }));
        }
        client.handle(key(KeyCode::B, modifiers));
        // The report opens by freezing the table and asking the host for its picture.
        std::iter::from_fn(|| client.next_request()).any(|request| request == Request::FreezeFrame)
    };
    assert!(opens(None, command), "Cmd+B on the Mac");
    assert!(!opens(None, control), "Ctrl+B is not the Mac's chord");
    assert!(opens(Some(false), control), "Ctrl+B elsewhere");
    assert!(!opens(Some(false), command));
    assert_eq!(
        Chord::parse("Cmd+Shift+L", true),
        Some(Chord {
            key: KeyCode::L,
            modifiers: Modifiers {
                shift: true,
                command: true,
                ..Modifiers::default()
            },
        })
    );
    assert_eq!(
        Chord::parse("Delete", true).map(|c| c.key),
        Some(KeyCode::Backspace)
    );
    assert_eq!(Chord::parse("Hyper+X", true), None);
}
