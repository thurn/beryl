//! The choice prompts (`prompts::choose`): a yes or no holds the card it is about up in the
//! tray and answers with its buttons, a number steps through the values it offers, and the
//! choice among a card's ways draws the card once a way over the table.

use crate::reference;
use app_client::Input;
use app_client::command::SessionCommand;
use app_client::{Client, Config};
use beryl_card_records::{TokenMakers, card_records_among};
use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::{
    ButtonId, Click, ClickRequest, PartName, PromptButton, PromptView, Purpose, ShapeBody, Words,
};
use beryl_protocol::protocol::{
    Candidate, CandidateView, DecisionView, Kind, ObjectId, SessionId, Side, Status, ZoneName,
};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use glam::Vec2;
use kit_frame::frame::{Frame, Role, StateBits};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, PointerPhase, Size};

use super::flat_layer::{centre, client, decision, find, pointer};

/// A millisecond, in the nanoseconds `Client::frame` takes.
const MS: u64 = 1_000_000;

/// The window `flat_layer::client` and the ways test draw.
const SIZE: Size = Size {
    width: 1440.0,
    height: 900.0,
};

fn button(id: ButtonId, label: &str, primary: bool, enabled: bool) -> PromptButton {
    PromptButton {
        id,
        label: label.to_owned(),
        caption: String::new(),
        enabled,
        primary,
    }
}

fn prompt(
    purpose: Purpose,
    title: &str,
    buttons: Vec<PromptButton>,
    body: ShapeBody,
) -> PromptView {
    PromptView {
        revision: 2,
        purpose,
        part: PartName::Pick,
        title: title.to_owned(),
        words: Words::default(),
        count: None,
        about: Vec::new(),
        from: None,
        marks: Vec::new(),
        previews: Vec::new(),
        buttons,
        body,
    }
}

/// `builder`'s view, with the viewer deciding `prompt` among `candidates`.
fn deciding(builder: &ViewBuilder, prompt: PromptView, candidates: Vec<CandidateView>) -> Input {
    let mut view = builder.build();
    view.status = Status::Deciding {
        decision: DecisionView {
            id: decision(),
            kind: Kind::Priority,
            candidates,
            resolving: None,
            prompt: Box::new(prompt),
            pass_to: None,
            pass_on: None,
            source: None,
            halt: false,
            cancellable: false,
            undo: 0,
            undo_depth: 0,
            refused: Vec::new(),
            acting_for: None,
        },
    };
    Input::Update(Box::new(GameUpdate {
        session: SessionId(1),
        view,
    }))
}

/// Presses the element at `index` of `frame` and returns the click it queued, if any.
fn press(client: &mut Client, at: Vec2) -> Option<Click> {
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    match client.next_command() {
        Some(SessionCommand::Click(ClickRequest { click, .. })) => Some(click),
        None => None,
        Some(other) => panic!("a press queues a click, not {other:?}"),
    }
}

/// The centre of the card in `slot` as `frame` draws it.
fn card_centre(frame: &Frame, slot: ObjectId) -> Vec2 {
    let index = (frame.resolved.iter())
        .position(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0))
        .expect("the card is drawn");
    centre(frame, index)
}

#[test]
fn a_yes_or_no_holds_its_card_up_and_its_yes_sends_the_first_option() {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let card = builder
        .card(ZoneName::Graveyard, Side::You, "Brainstorm")
        .slot;
    let question = prompt(
        Purpose::Move,
        "Return Brainstorm to your hand?",
        vec![
            button(ButtonId::Option { index: 0 }, "Yes", true, true),
            button(ButtonId::Option { index: 1 }, "No", false, true),
        ],
        ShapeBody::YesNo {
            about: vec![card],
            places: None,
        },
    );
    let mut client = client();
    client.handle(Input::Update(Box::new(builder.update())));
    client.frame(0);
    client.handle(deciding(&builder, question, Vec::new()));
    client.frame(MS);
    client.frame(5_000 * MS);
    let frame = client.frame(10_000 * MS);
    let (tray, _) = reference().tray.on_window(SIZE);
    let held = card_centre(frame, card);
    assert!(
        (tray.x..tray.x + tray.width).contains(&held.x)
            && (tray.y..tray.y + tray.height).contains(&held.y),
        "the card stands in the tray {tray:?}, not at {held}"
    );
    let yes = centre(frame, find(frame, Role::Button, "Yes"));
    assert_eq!(
        press(&mut client, yes),
        Some(Click::Button {
            button: ButtonId::Option { index: 0 }
        })
    );
}

#[test]
fn a_numbers_steps_send_the_values_beside_it() {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let number = prompt(
        Purpose::X,
        "Choose X.",
        vec![button(ButtonId::Confirm, "Done", true, true)],
        ShapeBody::Number {
            values: vec![0, 1, 2, 4],
            value: 2,
            detail: Some("{2}{U}".to_owned()),
        },
    );
    let mut client = client();
    client.handle(deciding(&builder, number, Vec::new()));
    let frame = client.frame(0);
    find(frame, Role::Text, "2");
    find(frame, Role::Text, "{2}{U}");
    let more = centre(frame, find(frame, Role::Button, "More"));
    let less = centre(frame, find(frame, Role::Button, "Less"));
    assert_eq!(
        press(&mut client, more),
        Some(Click::Number { n: 4, on: None })
    );
    assert_eq!(
        press(&mut client, less),
        Some(Click::Number { n: 1, on: None })
    );
}

#[test]
fn a_number_at_its_highest_value_cannot_step_up() {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let number = prompt(
        Purpose::X,
        "Choose X.",
        vec![button(ButtonId::Confirm, "Done", true, true)],
        ShapeBody::Number {
            values: vec![0, 1],
            value: 1,
            detail: None,
        },
    );
    let mut client = client();
    client.handle(deciding(&builder, number, Vec::new()));
    let frame = client.frame(0);
    let more = find(frame, Role::Button, "More");
    assert!(frame.resolved[more].state.has(StateBits::DISABLED));
    let at = centre(frame, more);
    assert_eq!(press(&mut client, at), None);
}

#[test]
fn the_ways_to_use_a_card_draw_one_face_each_and_a_disabled_way_sends_nothing() {
    const NAME: &str = "Teferi, Time Raveler";
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let teferi = builder.card(ZoneName::Battlefield, Side::You, NAME).slot;
    let ways = prompt(
        Purpose::Act,
        "",
        vec![
            button(ButtonId::Option { index: 0 }, "+1", false, true),
            button(ButtonId::Ability { nth: 1 }, "−3", false, false),
            button(ButtonId::Decline, "Cancel", false, true),
        ],
        ShapeBody::Mode {
            about: vec![teferi],
        },
    );
    let candidates = vec![CandidateView {
        label: "+1".to_owned(),
        option: Candidate::Activate {
            slot: teferi,
            ability: 1,
            granted: false,
            merged: None,
        },
        payments: Vec::new(),
        blocks: Vec::new(),
        nth: Some(0),
        text: None,
        targets: Vec::new(),
    }];
    let view = builder.build();
    let names = view.objects.iter().filter_map(|o| o.name.as_deref());
    let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
    client.handle(Input::Device(DeviceInput::Surface {
        size: SIZE,
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Cards(Box::new(card_records_among(
        names,
        TokenMakers::NONE,
    ))));
    client.handle(deciding(&builder, ways, candidates));
    let frame = client.frame(0);
    let faces = (frame.resolved.iter())
        .filter(|element| element.role == Role::Region && frame.name(element.name) == "face")
        .count();
    assert_eq!(faces, 2, "one face for each way");
    // The ways stand over the table, not in the prompt bar, which keeps its other buttons.
    find(frame, Role::Button, "Cancel");
    let disabled = centre(frame, find(frame, Role::Button, "−3"));
    let enabled = centre(frame, find(frame, Role::Button, "+1"));
    assert_eq!(press(&mut client, disabled), None);
    assert_eq!(
        press(&mut client, enabled),
        Some(Click::Button {
            button: ButtonId::Option { index: 0 }
        })
    );
}

/// A catalog of the five colors, with the entry `chosen`, if any.
fn colors(chosen: Option<u16>) -> PromptView {
    let entries = ["White", "Blue", "Black", "Red", "Green"];
    prompt(
        Purpose::Choose,
        "Choose a color.",
        Vec::new(),
        ShapeBody::Catalog {
            entries: entries.map(str::to_owned).to_vec(),
            chosen,
        },
    )
}

#[test]
fn a_catalog_lists_its_entries_in_order_and_lights_the_one_chosen() {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let mut client = client();
    client.handle(deciding(&builder, colors(None), Vec::new()));
    let frame = client.frame(0);
    let tops: Vec<f32> = ["White", "Blue", "Black", "Red", "Green"]
        .map(|name| frame.resolved[find(frame, Role::Button, name)].corners[0].y)
        .to_vec();
    assert!(
        tops.windows(2).all(|pair| pair[0] < pair[1]),
        "the entries stand top to bottom in the session's order: {tops:?}"
    );
    let black = centre(frame, find(frame, Role::Button, "Black"));
    assert_eq!(press(&mut client, black), Some(Click::Entry { index: 2 }));

    client.handle(deciding(&builder, colors(Some(2)), Vec::new()));
    let frame = client.frame(MS);
    let lit = |name| {
        let index = find(frame, Role::Button, name);
        frame.resolved[index].state.has(StateBits::SELECTED)
    };
    assert!(lit("Black"), "the reply lights the entry chosen");
    assert!(!lit("Blue"), "and no other");
}

#[test]
fn no_card_takes_a_press_under_a_catalogs_shade() {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let card = (builder.card(ZoneName::Battlefield, Side::You, "Island")).slot;
    let mut client = client();
    client.handle(deciding(&builder, colors(None), Vec::new()));
    // The table covers itself for a prompt only once it is open: from the frame after the
    // one in which it settles.
    client.frame(0);
    let frame = client.frame(MS);
    let index = (frame.resolved.iter())
        .position(|element| element.role == Role::Card && element.id.0 == u32::from(card.0))
        .expect("the card is drawn");
    assert!(
        !frame.resolved[index].interactive,
        "a card under the shade takes no press"
    );
}

#[test]
fn a_crowded_tray_scrolls_to_its_last_card_and_clips_the_first_row() {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let tray: Vec<_> = (0..60)
        .map(|index| {
            builder
                .card(ZoneName::Graveyard, Side::You, &format!("Card {index}"))
                .slot
        })
        .collect();
    let first = u32::from(tray[0].0);
    let last = u32::from(tray[59].0);
    let choose = prompt(
        Purpose::Move,
        "Choose a card.",
        Vec::new(),
        ShapeBody::Pick {
            tray,
            least: 1,
            most: 1,
            picked: Vec::new(),
            warned: Vec::new(),
        },
    );
    let mut client = client();
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 375.0,
            height: 647.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(builder.update())));
    client.frame(0);
    client.handle(deciding(&builder, choose, Vec::new()));
    client.frame(MS);
    client.frame(5_000 * MS);
    let frame = client.frame(10_000 * MS);
    let card = |frame: &Frame, id| {
        frame
            .resolved
            .iter()
            .copied()
            .find(|card| card.role == Role::Card && card.id.0 == id)
    };
    let first_card = card(frame, first).expect("the first row starts in view");
    assert!(
        card(frame, last).is_none(),
        "the last row starts below the viewport"
    );
    assert!((0..frame.resolved.len()).any(|at| frame.shown_by(at as u32) == Some(first_card.id)));
    let rect = kit_scene::hit::bounds(&first_card);
    let clip = first_card.clip.expect("the tray clips to its viewport");
    let roll = |distance: f32| {
        Input::Device(DeviceInput::Pointer(kit_ui::input::Pointer {
            id: 0,
            kind: kit_ui::input::PointerKind::Mouse,
            phase: PointerPhase::Wheel,
            position: Vec2::new(rect.x + rect.width / 2.0, clip.y + clip.height / 2.0),
            button: kit_ui::input::Button::None,
            wheel: Vec2::new(0.0, -distance),
            modifiers: kit_ui::input::Modifiers::default(),
        }))
    };
    client.handle(roll(rect.height / 4.0));
    let frame = client.frame(10_001 * MS);
    let clipped = card(frame, first).expect("part of the first row is still visible");
    let point = Vec2::new(rect.x + rect.width / 2.0, clip.y - 1.0);
    assert_eq!(clipped.clip, Some(clip));
    assert!(kit_scene::hit::bounds(&clipped).y < point.y);
    assert!(
        kit_scene::hit::hit(frame, point)
            .is_none_or(|at| frame.resolved[at as usize].id != clipped.id)
    );

    client.handle(roll(10_000.0));
    let frame = client.frame(10_002 * MS);
    assert!(card(frame, first).is_none());
    let reached = card(frame, last).expect("the last card scrolls into view");
    assert!(reached.interactive);
    let at = reached.corners.iter().copied().sum::<Vec2>() / 4.0;
    assert_eq!(
        kit_scene::hit::hit(frame, at).map(|index| frame.resolved[index as usize].id),
        Some(reached.id)
    );
}
