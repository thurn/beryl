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

#[test]
fn every_crowded_pick_and_put_back_card_keeps_a_press_target() {
    use beryl_protocol::prompt::{Marked, Pile, PlaceName};
    use beryl_protocol::script::{Mark, Subject};
    use kit_ui::input::PointerKind;

    for (size, kind) in [
        (SIZE, PointerKind::Mouse),
        (
            Size {
                width: 375.0,
                height: 647.0,
            },
            PointerKind::Touch,
        ),
    ] {
        for put_back in [false, true] {
            let mut builder = ViewBuilder::sample();
            builder.script(ScriptBuilder::reset().build());
            let dealt: Vec<_> = (0..if put_back { 128 } else { 64 })
                .map(|index| {
                    builder
                        .card(
                            if put_back {
                                ZoneName::Hand
                            } else {
                                ZoneName::Graveyard
                            },
                            Side::You,
                            &format!("Card {index}"),
                        )
                        .slot
                })
                .collect();
            // A picked card is in the first pile but still lies in dealt order in
            // the fan. A put-back has two independently scrolling drop areas.
            let (into, from) = dealt.split_at(if put_back { 64 } else { 1 });
            let ids = |cards: &[ObjectId]| {
                cards
                    .iter()
                    .map(|slot| u32::from(slot.0))
                    .collect::<Vec<_>>()
            };
            let body = ShapeBody::Split {
                piles: vec![
                    Pile {
                        label: if put_back { "On top" } else { "Hand" }.to_owned(),
                        place: Some(if put_back {
                            PlaceName::OnTop
                        } else {
                            PlaceName::Hand
                        }),
                        cards: into.to_vec(),
                        exactly: Some(into.len() as u8),
                        closed: false,
                    },
                    Pile {
                        label: if put_back { "Hand" } else { "Library" }.to_owned(),
                        place: (!put_back).then_some(PlaceName::Top),
                        cards: from.to_vec(),
                        exactly: None,
                        closed: false,
                    },
                ],
            };
            let mut chosen = prompt(Purpose::Move, "Choose cards.", Vec::new(), body);
            chosen.marks = dealt
                .iter()
                .map(|&slot| Marked {
                    on: Subject::Object { slot },
                    mark: Mark::Eligible,
                })
                .collect();
            let mut client = client();
            client.handle(Input::Surface {
                size,
                scale: 1.0,
                safe_area: Insets::default(),
            });
            client.handle(Input::Update(Box::new(builder.update())));
            client.frame(0);
            client.handle(deciding(&builder, chosen, Vec::new()));
            client.frame(MS);
            client.frame(5_000 * MS);
            let mut now = 10_000 * MS;
            let side = client.table().profile().data().checks.target_points;
            let press_target = Size {
                width: side,
                height: side,
            };
            let groups = if put_back {
                vec![(ids(into), into[into.len() - 1]), (ids(from), from[0])]
            } else {
                vec![(ids(&dealt), from[0])]
            };

            for (group, first) in groups {
                assert_tray_press_targets(&mut client, &mut now, press_target, group, first, kind);
            }
            if put_back {
                assert_scrolled_reorder(&mut client, now, press_target, into, kind);
            }
        }
    }
}

fn assert_tray_press_targets(
    client: &mut Client,
    now: &mut u64,
    press_target: Size,
    group: Vec<u32>,
    first: ObjectId,
    kind: kit_ui::input::PointerKind,
) {
    use kit_scene::hit::{Overlaps, bounds, has_free_rect};

    let frame = client.frame(*now);
    let first = *frame
        .resolved
        .iter()
        .find(|item| item.role == Role::Card && item.id.0 == u32::from(first.0))
        .expect("the first card is shown");
    let clip = first
        .clip
        .expect("the crowded group has a scrolling viewport");
    let at = Vec2::new(clip.x + clip.width / 2.0, clip.y + clip.height / 2.0);
    let mut waiting: std::collections::BTreeSet<_> = group.into_iter().collect();
    for step in 0..320 {
        let frame = client.frame(*now);
        let overlaps = Overlaps::new(frame);
        for (index, item) in frame.resolved.iter().enumerate() {
            if item.role == Role::Card
                && waiting.contains(&item.id.0)
                && has_free_rect(frame, &overlaps, index as u32, press_target)
            {
                waiting.remove(&item.id.0);
            }
        }
        if waiting.is_empty() {
            roll_tray(client, at, 100_000.0);
            *now += MS;
            return;
        }
        if step == 0 && kind == kit_ui::input::PointerKind::Touch {
            let rect = bounds(&first);
            let start = Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            let end = start - Vec2::Y * press_target.height;
            tray_pointer(client, kind, PointerPhase::Down, start);
            tray_pointer(client, kind, PointerPhase::Move, end);
            tray_pointer(client, kind, PointerPhase::Cancel, end);
            *now += MS;
            let frame = client.frame(*now);
            assert!(
                frame
                    .resolved
                    .iter()
                    .find(|item| item.id == first.id)
                    .is_none_or(|item| bounds(item).y < rect.y)
            );
        } else {
            roll_tray(client, at, press_target.height);
        }
        *now += MS;
    }
    panic!("no full press square while scrolling for {waiting:?}");
}

fn roll_tray(client: &mut Client, at: Vec2, distance: f32) {
    let Input::Pointer(mut wheel) = pointer(PointerPhase::Wheel, at) else {
        unreachable!("a pointer event");
    };
    wheel.wheel = Vec2::new(0.0, -distance);
    client.handle(Input::Pointer(wheel));
}

fn assert_scrolled_reorder(
    client: &mut Client,
    now: u64,
    press_target: Size,
    into: &[ObjectId],
    kind: kit_ui::input::PointerKind,
) {
    use beryl_protocol::script::Subject;
    use kit_scene::hit::{bounds, free_rect};

    let frame = client.frame(now);
    let card = |slot: ObjectId| {
        frame
            .resolved
            .iter()
            .position(|item| item.role == Role::Card && item.id.0 == u32::from(slot.0))
            .unwrap()
    };
    let source = free_rect(frame, card(into[0]) as u32, press_target)
        .expect("the top card has a press square after scrolling");
    let target = bounds(&frame.resolved[card(into[2])]);
    let start = Vec2::new(
        source.x + source.width / 2.0,
        source.y + source.height / 2.0,
    );
    let end = Vec2::new(
        target.x + target.width / 2.0,
        target.y + target.height / 2.0,
    );
    // A sideways touch picks up the card. A vertical swipe scrolls.
    for (phase, position) in [
        (PointerPhase::Down, start),
        (PointerPhase::Move, start + Vec2::new(30.0, 2.0)),
        (PointerPhase::Move, end),
        (PointerPhase::Up, end),
    ] {
        tray_pointer(client, kind, phase, position);
    }
    client.frame(now + MS);
    let command = client.next_command();
    assert!(
        matches!(command, Some(SessionCommand::Click(ClickRequest {
        click: Click::Drop { on: Subject::Object { slot }, pile: 0, at: 3 },
        ..
    })) if slot == into[0]),
        "{kind:?} drop queued {command:?}"
    );
}

fn tray_pointer(
    client: &mut Client,
    kind: kit_ui::input::PointerKind,
    phase: PointerPhase,
    position: Vec2,
) {
    let Input::Pointer(mut event) = pointer(phase, position) else {
        unreachable!("a pointer event");
    };
    event.kind = kind;
    client.handle(Input::Pointer(event));
}
