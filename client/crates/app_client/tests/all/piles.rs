//! The library, graveyard and exile piles: their real height, the few records a tall pile
//! draws, their elements and counts, a card leaving from inside one, and the tray of cards a
//! prompt holds up.

use crate::common::{MS, empty_frame, frame, show, window};
use crate::reference;
use app_client::Input;
use app_client::{Client, Config};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Side, Why, ZoneName};
use beryl_protocol::script::Act;
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, object, timing};
use game_table::table::{Camera, Held, Table, TrayKind, landing, tray, tray_boxes};
use glam::{Vec2, Vec3};
use kit_base::pose::{Pose, Segment};
use kit_frame::frame::{Frame, Material, Role};
use kit_space::motion::pose_at;
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size};

fn client() -> Client {
    Client::new(
        crate::test_data::data(),
        Config {
            elements: 2048,
            draws: 2048,
            name_bytes: 65_536,
        },
    )
}

/// A view whose viewer's library holds `cards` cards and which has nothing else.
fn library(cards: usize) -> ViewBuilder {
    let mut builder = ViewBuilder::new();
    for _ in 0..cards {
        builder.card(ZoneName::Library, Side::You, "Island");
    }
    builder
}

/// The frame a client draws for `view` at rest in a 1600 by 900 window.
fn frame_of(view: &ViewBuilder) -> Frame {
    let mut client = client();
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    let mut view = view.clone();
    view.script(ScriptBuilder::reset().build());
    client.handle(Input::Update(Box::new(view.update())));
    client.frame(0).clone()
}

/// How many records the table draws, the cloth among them: those before the flat layer's.
fn table_records(frame: &Frame) -> usize {
    frame.draws[..frame.layer as usize].len()
}

/// The table at rest in `view`, in a 1600 by 900 window.
fn table_of(view: &ViewBuilder) -> Table {
    let mut table = Table::new(crate::test_data::data(), window(1600.0, 900.0));
    table.apply(&view.build());
    table
}

/// The height of the viewer's library: from under its bottom card to the top of its top card.
/// Its cards lean back toward the camera as far as makes them as tall on screen as the web
/// interface's, which stands each a little off the table, by the same height for every card.
fn library_height(table: &Table, view: &ViewBuilder) -> f32 {
    let library = view.build().players.you.library;
    let height = |slot: ObjectId| table.rest(usize::from(slot.0)).expect("placed").position.z;
    let (bottom, top) = (library[0], *library.last().expect("a card"));
    height(top) - height(bottom) + crate::test_data::data().card.layer
}

#[test]
fn a_tall_library_is_its_real_height_and_draws_few_records() {
    let empty = table_records(&frame_of(&library(0)));
    let sixty = library(60);
    let height = library_height(&table_of(&sixty), &sixty);
    assert!(
        (height - 18.0).abs() < 1e-3,
        "60 cards stand {height} mm tall"
    );
    assert_eq!(
        table_records(&frame_of(&sixty)) - empty,
        18,
        "the top card and the one under it, each with its soft edge, two dark records of the rims beneath, a light line for \
         each of the 11 cards beneath them that steps further than its line is wide, and one \
         light band for the 47 deeper cards"
    );
    // The card and its soft edge.
    assert_eq!(table_records(&frame_of(&library(1))) - empty, 2);
}

#[test]
fn a_library_falls_by_one_layer_when_a_card_is_drawn() {
    let full = library(20);
    let mut drawn = full.clone();
    drawn.card(ZoneName::Hand, Side::You, "Plains");
    let mut view = drawn.build();
    let top = view.players.you.library.pop().expect("a card");
    view.objects[usize::from(top.0)] = object(top, ZoneName::Hand, Side::You, Some("Island"));
    let mut table = table_of(&full);
    let height =
        |table: &Table, slot: ObjectId| table.rest(usize::from(slot.0)).expect("placed").position.z;
    let before = height(&table, top);
    table.apply(&view);
    let new_top = *view.players.you.library.last().expect("a card");
    let after = height(&table, new_top);
    // The cards of a taller pile are drawn a hair smaller, so that they are as large on screen as
    // a shorter pile's, which moves where their near edge stands by far less than a layer.
    assert!(
        (before - after - crate::test_data::data().card.layer).abs() < 1e-2,
        "{before} became {after}"
    );
}

#[test]
fn the_opponents_graveyard_face_is_in_front_of_the_cards_and_rims_below_it() {
    let mut view = ViewBuilder::new();
    let mut slots = Vec::new();
    for name in [
        "Damn",
        "Counterspell",
        "Fractured Identity",
        "Plains",
        "Snapcaster Mage",
    ] {
        slots.push(view.card(ZoneName::Graveyard, Side::Opponent, name).slot);
    }
    view.script(ScriptBuilder::reset().build());

    let mut client = client();
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 1728.0,
            height: 1117.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(view.update())));
    let frame = client.frame(0);
    let pose = |index: usize| {
        frame
            .resolved
            .iter()
            .find(|element| element.role == Role::Card && element.id.0 == u32::from(slots[index].0))
            .and_then(|element| element.pose)
            .expect("a visible card in the pile")
    };
    let (top, under) = (pose(4), pose(3));
    let normal = top.rotation * Vec3::Z;
    let separation = crate::test_data::data().card.layer / 2.0;
    let rims = frame.draws.iter().filter(|draw| {
        draw.material == Material::Table as u8
            && draw.params.iter().all(|value| value.abs() < f32::EPSILON)
    });
    let half = crate::test_data::data().card.half();
    let mut count = 0;
    for draw in rims {
        count += 1;
        let [xx, xy, _, x, yx, yy, _, y, zx, zy, _, z] = draw.model;
        let (across, along) = (Vec3::new(xx, yx, zx), Vec3::new(xy, yy, zy));
        for x_side in [-half.x, half.x] {
            for y_side in [-half.y, half.y] {
                let corner = Vec3::new(x, y, z) + across * x_side + along * y_side;
                assert!(
                    (under.position - corner).dot(normal) > separation,
                    "the rim at {corner:?} must lie behind the second card"
                );
            }
        }
    }
    assert!(count > 2, "the pile draws dark filling and stripes");
    assert!(
        (top.position - under.position).dot(normal) > separation,
        "the second card must lie behind the top face"
    );
}

#[test]
fn each_pile_is_named_with_its_count_and_an_empty_one_has_neither() {
    let frame = frame_of(&ViewBuilder::sample());
    let named = |role: Role, name: &str| {
        frame
            .resolved
            .iter()
            .any(|element| element.role == role && frame.name(element.name) == name)
    };
    for (pile, count) in [
        ("Your library", 5),
        ("Your graveyard", 1),
        ("Your exile", 1),
        ("Opponent's library", 5),
        ("Opponent's graveyard", 1),
        ("Opponent's exile", 1),
    ] {
        assert!(named(Role::Pile, pile), "{pile} has an element");
        // The count shows only while the pointer is over the pile, as the web's tip does.
        assert!(
            !named(Role::Text, &format!("{pile} {count}")),
            "{pile} shows its count at rest"
        );
    }
    let frame = frame_of(&library(3));
    let piles: Vec<&str> = (frame.resolved.iter())
        .filter(|element| element.role == Role::Pile)
        .map(|element| frame.name(element.name))
        .collect();
    assert_eq!(piles, ["Your library"], "only a pile with a card has one");
    let counts = (frame.resolved.iter())
        .filter(|element| {
            element.role == Role::Text && frame.name(element.name).starts_with("Opponent's")
        })
        .count();
    assert_eq!(counts, 0, "an empty pile shows no count");
}

#[test]
fn a_card_taken_from_inside_a_pile_starts_where_it_lay() {
    let before = library(10);
    let slot = ObjectId(4);
    let mut after = before.build();
    after.players.you.library.retain(|&card| card != slot);
    let card = object(slot, ZoneName::Hand, Side::You, Some("Island"));
    after.objects[usize::from(slot.0)] = card.clone();
    after.script = Some(
        ScriptBuilder::default()
            .beat([Act::Fly {
                from: ZoneName::Library,
                why: Why::Draw,
                object: card,
                at: None,
                item: None,
                origin: None,
            }])
            .build(),
    );
    let mut before = before;
    before.script(ScriptBuilder::reset().build());
    // The show's table is 1440 by 900, and a pile's cards are as large as the web's in the
    // window: lay the library out in the same window.
    let mut table = Table::new(crate::test_data::data(), window(1440.0, 900.0));
    table.apply(&before.build());
    let lay = table
        .rest(usize::from(slot.0))
        .expect("placed in the library");
    let mut show = show();
    show.record();
    show.push(Box::new(before.update()), Vec::new());
    let after = GameUpdate {
        session: before.update().session,
        view: after,
    };
    show.push(Box::new(after), vec![timing(400.0)]);
    let mut scratch = empty_frame();
    let (mut now, mut drawn) = (0, false);
    while !show.settled() && now < 2_000 * MS {
        let frame = frame(&mut show, &mut scratch, now);
        drawn |= frame
            .resolved
            .iter()
            .any(|element| element.role == Role::Card && element.id.0 == u32::from(slot.0));
        now += 16 * MS;
    }
    let segments: Vec<Segment> = (show.history().last().expect("recorded"))
        .track(usize::from(slot.0))
        .iter()
        .map(|record| record.segment)
        .collect();
    let start: Pose = pose_at(&segments, 0);
    assert!(
        (start.position.z - lay.position.z).abs() < 1e-4,
        "it starts at the height it lay at: {start:?} against {lay:?}"
    );
    assert!(
        drawn,
        "it is drawn on its way, though it was not drawn in the pile"
    );
}

/// The corners on screen of a card resting at `pose`, as the smallest and largest points.
fn screen_box(camera: &Camera, pose: &Pose) -> (Vec2, Vec2) {
    let transform = pose.transform();
    let (x, y) = (31.5, 44.0);
    let points = [(-x, -y), (x, -y), (x, y), (-x, y)]
        .map(|(x, y)| camera.to_screen(transform.transform_point3(Vec3::new(x, y, 0.0))));
    let low = points.iter().copied().reduce(Vec2::min).expect("four");
    let high = points.iter().copied().reduce(Vec2::max).expect("four");
    (low, high)
}

#[test]
fn a_tray_lies_inside_the_window_with_every_card_seen() {
    let sizes = [
        (1440.0, 900.0),
        (1600.0, 900.0),
        (1920.0, 1080.0),
        (2560.0, 1080.0),
    ];
    let trays: [&[usize]; 5] = [&[1], &[5], &[12], &[3, 4], &[4, 3]];
    for (width, height) in sizes {
        let camera = Camera::fit(reference().camera, window(width, height));
        for counts in trays {
            let piles: Vec<Vec<usize>> = counts.iter().map(|&count| (0..count).collect()).collect();
            let piles: Vec<&[usize]> = piles.iter().map(Vec::as_slice).collect();
            let poses = tray(&piles, reference().tray, &camera);
            assert_eq!(poses.len(), counts.iter().sum::<usize>());
            let boxes: Vec<(Vec2, Vec2)> = poses.iter().map(|p| screen_box(&camera, p)).collect();
            for (index, (low, high)) in boxes.iter().enumerate() {
                let label = format!("card {index} of {counts:?} at {width}x{height}");
                assert!(
                    low.x >= 0.0 && low.y >= 0.0 && high.x <= width && high.y <= height,
                    "{label} lies inside the window: {low} to {high}"
                );
                let centre = (*low + *high) / 2.0;
                let covered = boxes[index + 1..].iter().any(|(l, h)| {
                    centre.x > l.x && centre.x < h.x && centre.y > l.y && centre.y < h.y
                });
                assert!(!covered, "{label} has its centre covered by a later card");
            }
        }
    }
}

#[test]
fn a_card_let_go_in_a_box_lands_before_the_first_card_whose_middle_is_past_it() {
    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    let space = reference().tray;
    let held = Held {
        piles: vec![vec![ObjectId(1)], vec![ObjectId(2), ObjectId(3)]],
        rooms: vec![None, None],
        kind: TrayKind::Boxes,
    };
    let boxes = tray_boxes(&held, space, size);
    assert_eq!(boxes.len(), 2, "a box for each pile");
    let second = boxes[1];
    let y = second.y + second.height / 2.0;
    assert_eq!(
        landing(
            &held,
            space,
            size,
            Vec2::new(second.x + 2.0, y),
            ObjectId(1)
        ),
        Some((1, 0)),
        "at the second box's left edge: before its first card"
    );
    assert_eq!(
        landing(
            &held,
            space,
            size,
            Vec2::new(second.x + second.width - 2.0, y),
            ObjectId(1)
        ),
        Some((1, 2)),
        "at its right edge: at its end"
    );
    assert_eq!(
        landing(&held, space, size, Vec2::new(10.0, 10.0), ObjectId(1)),
        None,
        "outside both boxes: nowhere"
    );
    let one = Held {
        piles: vec![vec![ObjectId(1), ObjectId(2)]],
        rooms: Vec::new(),
        kind: TrayKind::Boxes,
    };
    assert!(
        tray_boxes(&one, space, size).is_empty(),
        "one pile has no box"
    );
    let centre = Vec2::new(size.width / 2.0, size.height / 2.0);
    assert_eq!(
        landing(&one, space, size, centre, ObjectId(1)),
        None,
        "nor any order to give"
    );
}

/// Builds a shared-deck view with one library, one graveyard and one exile pile.
fn shared_view(
    known_top: bool,
    top_owner: Side,
    castable_owner: Side,
) -> beryl_protocol::PlayerView {
    use beryl_protocol::protocol::{Facing, SharedPiles};

    let mut builder = ViewBuilder::new();
    let hidden = builder.card(ZoneName::Library, Side::You, "Island").slot;
    let top = builder.card(ZoneName::Library, top_owner, "Dandan").slot;
    let gy_plain = builder
        .card(ZoneName::Graveyard, Side::You, "Brainstorm")
        .slot;
    let gy_cast = builder
        .card(ZoneName::Graveyard, castable_owner, "Memory Lapse")
        .slot;
    let host = builder
        .card(ZoneName::Battlefield, Side::You, "Dandan")
        .slot;
    let exiled_plain = builder.card(ZoneName::Exile, Side::You, "Predict").slot;
    let exiled_tucked = builder
        .card(ZoneName::Exile, Side::Opponent, "Telling Time")
        .slot;
    builder.script(ScriptBuilder::reset().build());
    let mut view = builder.build();
    for object in &mut view.objects {
        if object.slot == hidden {
            object.facing = Facing::Down;
            object.name = None;
            object.zone_side = None;
        } else if object.slot == top {
            object.zone_side = None;
            if known_top {
                object.facing = Facing::Up;
                object.name = Some("Dandan".to_owned());
            } else {
                object.facing = Facing::Down;
                object.name = None;
            }
        } else if object.slot == gy_plain {
            object.zone_side = None;
        } else if object.slot == gy_cast {
            object.zone_side = None;
            object.castable_from_graveyard = true;
        } else if object.slot == exiled_plain {
            object.zone_side = None;
        } else if object.slot == exiled_tucked {
            object.zone_side = None;
            object.exiled_by = Some(host);
        }
    }
    view.players.you.library.clear();
    view.players.opponent.library.clear();
    view.players.you.graveyard.clear();
    view.players.opponent.graveyard.clear();
    view.shared = Some(SharedPiles {
        library: vec![hidden, top],
        graveyard: vec![gy_plain, gy_cast],
    });
    view
}

// Generic story checks verify layout and reachability, but cannot assert that a castable card
// owned by the opponent in the shared graveyard is placed in the viewer's hand fan, that shared
// exile omits cards tucked under hosts, or that a known shared-library top card is face up only
// for the viewer who knows it.
#[test]
fn shared_piles_counts_browsers_castable_graveyard_and_known_top() {
    use game_hud::hud::piles::pile_cards;
    use game_table::table::Place;
    use kit_frame::frame::StateBits;

    let known = shared_view(true, Side::Opponent, Side::Opponent);
    let unknown = shared_view(false, Side::Opponent, Side::Opponent);

    // Shared exile browser omits the card tucked under the battlefield host.
    let exile_slots = pile_cards(&known, None, ZoneName::Exile);
    assert_eq!(exile_slots.len(), 1, "only untucked exile card is listed");
    assert_eq!(pile_cards(&known, None, ZoneName::Library).len(), 2);
    assert_eq!(pile_cards(&known, None, ZoneName::Graveyard).len(), 2);

    let render = |view: &beryl_protocol::PlayerView| -> Frame {
        let mut client = client();
        client.handle(Input::Device(DeviceInput::Surface {
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 2.0,
            safe_area: Insets::default(),
        }));
        client.handle(Input::Update(Box::new(GameUpdate {
            session: beryl_protocol::SessionId(1),
            view: view.clone(),
        })));
        client.frame(0).clone()
    };

    // Known top card (even owned by Opponent) is face up; unknown top card is face down.
    let known_frame = render(&known);
    let top_slot = known.shared.as_ref().unwrap().library[1];
    let top_known = known_frame
        .resolved
        .iter()
        .find(|el| el.role == Role::Card && el.id.0 == u32::from(top_slot.0))
        .expect("top card drawn");
    assert!(
        !top_known.state.has(StateBits::FACE_DOWN),
        "known shared library top card should be face up"
    );

    let unknown_frame = render(&unknown);
    let top_unknown = unknown_frame
        .resolved
        .iter()
        .find(|el| el.role == Role::Card && el.id.0 == u32::from(top_slot.0))
        .expect("unknown top card drawn");
    assert!(
        top_unknown.state.has(StateBits::FACE_DOWN),
        "unknown shared library top card should be face down"
    );

    // A card castable from the shared graveyard (even owned by Opponent) is offered in the
    // viewer's hand fan rather than on the shared graveyard pile.
    let castable_slot = known.shared.as_ref().unwrap().graveyard[1];
    let castable_el = known_frame
        .resolved
        .iter()
        .find(|el| el.role == Role::Card && el.id.0 == u32::from(castable_slot.0))
        .expect("castable graveyard card drawn");
    assert_eq!(
        Place::in_frame(castable_el.place),
        Some(Place::Card {
            zone: ZoneName::Hand,
            side: Side::You,
            row: None,
        }),
        "castable shared graveyard card belongs in the viewer's hand fan"
    );
}

/// Builds a view with an open `Order Cards` second-part split prompt whose first `"Top"` pile
/// is closed and empty and whose second `"Top"` pile holds three looked-at library cards.
fn order_cards_view() -> (ViewBuilder, beryl_protocol::PlayerView, ObjectId) {
    use beryl_protocol::SessionId;
    use beryl_protocol::prompt::{
        ButtonId, Lies, Marked, PartName, Pile, PlaceName, PromptButton, Purpose, ShapeBody,
    };
    use beryl_protocol::protocol::{DecisionId, DecisionView, Facing, Kind, Status};
    use beryl_protocol::script::{Mark, Subject};
    use beryl_protocol::testing;

    let mut builder = ViewBuilder::new();
    builder.script(ScriptBuilder::reset().build());
    let island = builder.card(ZoneName::Library, Side::You, "Island").slot;
    let plains = builder.card(ZoneName::Library, Side::You, "Plains").slot;
    let swamp = builder.card(ZoneName::Library, Side::You, "Swamp").slot;
    let cards = [island, plains, swamp];
    let mut view = builder.build();
    for object in &mut view.objects {
        if let Some((_, name)) = [(island, "Island"), (plains, "Plains"), (swamp, "Swamp")]
            .iter()
            .find(|(slot, _)| *slot == object.slot)
        {
            object.facing = Facing::Up;
            object.name = Some((*name).to_owned());
        }
    }
    let mut prompt = testing::prompt(
        Purpose::Arrange,
        "Order Cards",
        vec![PromptButton {
            id: ButtonId::Confirm,
            label: "Done".to_owned(),
            caption: String::new(),
            enabled: true,
            primary: true,
        }],
        ShapeBody::Split {
            piles: vec![
                Pile {
                    label: "Top".to_owned(),
                    place: Some(PlaceName::Top),
                    cards: Vec::new(),
                    exactly: None,
                    closed: true,
                },
                Pile {
                    label: "Top".to_owned(),
                    place: Some(PlaceName::Top),
                    cards: cards.to_vec(),
                    exactly: None,
                    closed: false,
                },
            ],
        },
    );
    prompt.part = PartName::Second;
    "Done".clone_into(&mut prompt.words.confirm);
    prompt.words.piles = ["Top".to_owned(), "Top".to_owned()];
    prompt.count = Some(3);
    prompt.from = Some(Lies {
        player: Side::You,
        zone: ZoneName::Library,
    });
    prompt.marks = cards
        .iter()
        .map(|&slot| Marked {
            on: Subject::Object { slot },
            mark: Mark::Eligible,
        })
        .collect();
    view.status = Status::Deciding {
        decision: DecisionView {
            id: DecisionId {
                session: SessionId(1),
                seq: 1,
            },
            kind: Kind::ChooseObject,
            candidates: Vec::new(),
            resolving: None,
            prompt: Box::new(prompt),
            pass_to: None,
            pass_on: None,
            source: None,
            acting_for: None,
            halt: false,
            cancellable: false,
            undo: 0,
            undo_depth: 0,
            refused: Vec::new(),
        },
    };
    (builder, view, island)
}

#[test]
fn a_split_with_a_closed_empty_first_pile_shows_one_box_and_drops_into_the_open_pile() {
    use app_client::command::SessionCommand;
    use beryl_protocol::SessionId;
    use beryl_protocol::prompt::{Click, ClickRequest};
    use beryl_protocol::script::Subject;
    use kit_scene::hit::bounds;
    use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};

    let size = Size {
        width: 1440.0,
        height: 900.0,
    };
    let (builder, view, island) = order_cards_view();
    let mut client = client();
    client.handle(Input::Device(DeviceInput::Surface {
        size,
        scale: 1.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(builder.update())));
    client.frame(0);
    client.handle(Input::Update(Box::new(GameUpdate {
        session: SessionId(1),
        view,
    })));
    client.frame(MS);
    client.frame(5_000 * MS);
    let settled = 10_000 * MS;
    let frame = client.frame(settled);

    let top_regions: Vec<_> = frame
        .resolved
        .iter()
        .filter(|el| el.role == Role::Region && frame.name(el.name) == "Top")
        .collect();
    assert_eq!(
        top_regions.len(),
        1,
        "only the open pile gets a Top box, not the closed empty pile"
    );
    let top_box = bounds(top_regions[0]);
    let island_el = frame
        .resolved
        .iter()
        .find(|el| el.role == Role::Card && el.id.0 == u32::from(island.0))
        .expect("Island card drawn in tray");
    let island_rect = bounds(island_el);
    let start = Vec2::new(
        island_rect.x + island_rect.width / 2.0,
        island_rect.y + island_rect.height / 2.0,
    );
    let end = Vec2::new(
        top_box.x + top_box.width - 4.0,
        top_box.y + top_box.height / 2.0,
    );
    for (phase, position) in [
        (PointerPhase::Down, start),
        (PointerPhase::Move, start + Vec2::new(30.0, 2.0)),
        (PointerPhase::Move, end),
        (PointerPhase::Up, end),
    ] {
        client.handle(Input::Device(DeviceInput::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        })));
    }
    client.frame(settled + MS);
    let command = client.next_command();
    assert!(
        matches!(
            command,
            Some(SessionCommand::Click(ClickRequest {
                click: Click::Drop {
                    on: Subject::Object { slot },
                    pile: 1,
                    at: 3,
                },
                ..
            })) if slot == island
        ),
        "dragging in the single open box maps to prompt pile 1, got {command:?}"
    );
}
