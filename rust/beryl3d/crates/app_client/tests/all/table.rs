//! The table at rest: the camera's fit, and where each zone's cards are placed.

use crate::reference;
use app_client::Input;
use app_client::{Client, Config};
use beryl_protocol::protocol::{
    CardType, ObjectId, PlayerView, RowName, Side, StackItem, TargetRef, ZoneName,
};
use beryl_protocol::script::Reset;
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, script};
use game_table::table::{ABILITY_IDS, Camera, Placement, Scroll, Window};
use glam::{Vec2, Vec3};
use kit_base::pose::Pose;
use kit_frame::frame::{Frame, Place, Role};
use kit_probe::agent::Context;
use kit_probe::agent::checks::{duplicates, reachable};
use kit_probe::agent::tree::{TreeOptions, tree};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size};

fn place(
    view: &PlayerView,
    abilities: &[u32],
    layout: &game_table::table::Layout,
    camera: &Camera,
    scroll: &Scroll,
) -> Placement {
    let data = crate::test_data::data();
    game_table::table::place(view, abilities, layout, camera, scroll, &data.verbs.lift)
}

fn window(width: f32, height: f32) -> Window {
    Window {
        size_class: None,
        size: Size { width, height },
        scale: 2.0,
        safe_area: Insets::default(),
    }
}

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

/// The frame a client draws for `view` in a 1440 by 900 window.
fn frame_of(client: &mut Client, view: &ViewBuilder) -> Frame {
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 1440.0,
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

#[test]
fn every_zone_of_both_sides_gets_its_cards() {
    let builder = ViewBuilder::sample();
    let view = builder.build();
    let frame = frame_of(&mut client(), &builder);
    let cards = frame
        .resolved
        .iter()
        .filter(|r| r.role == Role::Card)
        .count();
    // Each side's library of five draws only its top two cards.
    assert_eq!(
        cards,
        view.objects.len() - 2 * 3,
        "every card but those under the top two of a library is drawn"
    );
    for side in [Side::You, Side::Opponent] {
        let places = [
            (ZoneName::Library, None),
            (ZoneName::Hand, None),
            (ZoneName::Graveyard, None),
            (ZoneName::Exile, None),
            (ZoneName::Battlefield, Some(RowName::Creatures)),
            (ZoneName::Battlefield, Some(RowName::Lands)),
            (ZoneName::Battlefield, Some(RowName::Others)),
        ];
        for (zone, row) in places {
            let place = Place::Card { zone, side, row };
            assert!(
                frame.resolved.iter().any(|r| r.place == place),
                "{side:?} {zone:?} {row:?} has a card"
            );
        }
    }
    let stack = Place::Card {
        zone: ZoneName::Stack,
        side: Side::You,
        row: None,
    };
    assert!(frame.resolved.iter().any(|r| r.place == stack));
    // Each side's library lies in its own corner of the window, as the web interface's piles
    // do: the viewer's at the bottom left, the opponent's at the top right.
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let placement = place(&view, &[], reference(), &camera, &Scroll::default());
    let library = |side: Side| {
        let player = match side {
            Side::You => &view.players.you,
            Side::Opponent => &view.players.opponent,
        };
        camera.to_screen(placement.get(player.library[0]).unwrap().pose.position)
    };
    let (you, opponent) = (library(Side::You), library(Side::Opponent));
    assert!(you.x < 1440.0 / 4.0 && you.y > 900.0 * 3.0 / 4.0, "{you}");
    assert!(
        opponent.x > 1440.0 * 3.0 / 4.0 && opponent.y < 900.0 / 4.0,
        "{opponent}"
    );
}

#[test]
fn placement_is_the_same_for_the_same_view_and_window() {
    let view = ViewBuilder::sample().build();
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let first = place(&view, &[], reference(), &camera, &Scroll::default());
    let again = place(&view, &[], reference(), &camera, &Scroll::default());
    assert_eq!(first, again);
}

#[test]
fn plain_twins_pile_in_an_uncrowded_row_with_the_lowest_slot_on_top() {
    let mut builder = ViewBuilder::new();
    let top = builder.card(ZoneName::Battlefield, Side::You, "Map Token");
    top.token = true;
    let top = top.slot;
    let copy = builder.card(ZoneName::Battlefield, Side::You, "Map Token");
    copy.token = true;
    let copy = copy.slot;
    let frame = frame_of(&mut client(), &builder);
    let tree = tree(&frame, &TreeOptions::default()).unwrap();
    assert!(
        tree.lines()
            .any(|line| line.contains("Map Token") && line.contains("pile(2)"))
    );
    let mut view = builder.build();
    view.battlefield.reverse();
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let placement = place(&view, &[], reference(), &camera, &Scroll::default());
    let top = placement.get(top).unwrap();
    let copy = placement.get(copy).unwrap();
    assert_eq!((top.copies, copy.copies), (2, 0));
    assert!(top.pose.position.z > copy.pose.position.z);
}

#[test]
fn tapped_and_untapped_twins_stand_apart() {
    let mut builder = ViewBuilder::new();
    let untapped = builder.card(ZoneName::Battlefield, Side::You, "Map Token");
    untapped.token = true;
    let untapped = untapped.slot;
    let tapped = builder.card(ZoneName::Battlefield, Side::You, "Map Token");
    tapped.token = true;
    tapped.tapped = true;
    let tapped = tapped.slot;
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let placement = place(
        &builder.build(),
        &[],
        reference(),
        &camera,
        &Scroll::default(),
    );
    assert_eq!(placement.get(tapped).unwrap().copies, 1);
    assert_eq!(placement.get(untapped).unwrap().copies, 1);
}

#[test]
fn the_lands_row_piles_at_seven_lands_including_lands_in_other_rows() {
    let mut builder = ViewBuilder::new();
    for name in ["Island", "Island", "Plains", "Swamp", "Mountain", "Forest"] {
        builder.card(ZoneName::Battlefield, Side::You, name).types = vec![CardType::Land];
    }
    builder
        .card(ZoneName::Battlefield, Side::Opponent, "Forest")
        .types = vec![CardType::Land];
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let sizes = |view: &PlayerView| {
        let placement = place(view, &[], reference(), &camera, &Scroll::default());
        [ObjectId(0), ObjectId(1)].map(|slot| placement.get(slot).unwrap().copies)
    };
    assert_eq!(sizes(&builder.build()), [1, 1]);
    builder
        .card(ZoneName::Battlefield, Side::You, "Dryad Arbor")
        .types = vec![CardType::Creature, CardType::Land];
    assert_eq!(sizes(&builder.build()), [2, 0]);
}

#[test]
fn a_permanent_targeted_on_the_stack_stands_apart_from_its_twins() {
    let mut builder = ViewBuilder::new();
    let first = builder
        .card(ZoneName::Battlefield, Side::You, "Map Token")
        .slot;
    let targeted = builder
        .card(ZoneName::Battlefield, Side::You, "Map Token")
        .slot;
    builder.card(ZoneName::Stack, Side::Opponent, "Disenchant");
    let mut view = builder.build();
    let StackItem::Spell { targets, .. } = &mut view.stack[0] else {
        panic!("expected a spell");
    };
    targets.push(TargetRef::Object { slot: targeted });
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let placement = place(&view, &[], reference(), &camera, &Scroll::default());
    assert_eq!(placement.get(first).unwrap().copies, 1);
    assert_eq!(placement.get(targeted).unwrap().copies, 1);
}

/// 255 permanents of different names, a third of them in each of the viewer's rows.
fn crowded() -> PlayerView {
    let mut builder = ViewBuilder::new();
    for index in 0..255 {
        let types = match index % 3 {
            0 => vec![CardType::Creature],
            1 => vec![CardType::Land],
            _ => vec![CardType::Artifact],
        };
        builder
            .card(
                ZoneName::Battlefield,
                Side::You,
                &format!("Permanent {index}"),
            )
            .types = types;
    }
    builder.build()
}

#[test]
fn a_crowded_battlefield_keeps_its_cards_in_the_table_with_their_strips_uncovered() {
    let view = crowded();
    let layout = game_table::profiles::Profile::for_window(
        &crate::test_data::data().profiles,
        window(1440.0, 900.0),
    )
    .layout();
    let camera = layout.fit(window(1440.0, 900.0));
    let placement = place(&view, &[], &layout, &camera, &Scroll::default());
    let [low, high] = reference().camera.required;
    let mut drawn = 0;
    for object in &view.objects {
        let rest = placement.get(object.slot).unwrap();
        if !rest.visible {
            continue;
        }
        drawn += 1;
        let p = rest.pose.position;
        let half = if object.tapped {
            crate::test_data::data().card.height
        } else {
            crate::test_data::data().card.width
        } / 2.0
            * rest.pose.scale;
        assert!(p.x - half >= low.x && p.x + half <= high.x, "{p}");
        let long = crate::test_data::data().card.height / 2.0 * rest.pose.scale;
        assert!(p.y - long >= low.y && p.y + long <= high.y, "{p}");
    }
    assert!(drawn < 255, "some cards are scrolled out of view");

    let mut builder = ViewBuilder::new();
    for object in &view.objects {
        builder
            .card(
                ZoneName::Battlefield,
                Side::You,
                object.name.as_deref().unwrap(),
            )
            .types
            .clone_from(&object.types);
    }
    let frame = frame_of(&mut client(), &builder);
    assert_eq!(
        (frame.resolved.iter())
            .filter(|element| element.role == Role::Card)
            .count(),
        drawn,
        "only the cards in view are drawn"
    );
    for row in [RowName::Creatures, RowName::Lands, RowName::Others] {
        let place = Place::Card {
            zone: ZoneName::Battlefield,
            side: Side::You,
            row: Some(row),
        };
        let mut lefts: Vec<f32> = frame
            .resolved
            .iter()
            .filter(|r| r.place == place)
            .map(|r| r.corners.iter().map(|c| c.x).fold(f32::INFINITY, f32::min))
            .collect();
        assert!(lefts.len() > 1, "{row:?} is crowded");
        lefts.sort_by(f32::total_cmp);
        for pair in lefts.windows(2) {
            assert!(
                pair[1] - pair[0] >= reference().strip - 0.01,
                "{row:?}: {pair:?}"
            );
        }
    }
}

/// A point of the table on screen, computed from the camera's definition without its
/// matrices: the eye stands back from the target along the pitched line of sight, and a point
/// lands on screen by similar triangles.
fn by_hand(point: Vec3, camera: &Camera) -> Vec2 {
    let data = reference().camera;
    let pitch = data.pitch_degrees.to_radians();
    let forward = Vec3::new(0.0, pitch.sin(), -pitch.cos());
    let eye = data.target - forward * camera.distance;
    // The camera never turns sideways, so its right is the table's x, and its up is
    // perpendicular to both.
    let right = Vec3::X;
    let up = right.cross(forward);
    let v = point - eye;
    let (x, y, z) = (v.dot(right), v.dot(up), v.dot(forward));
    let size = camera.window.size;
    let tan_half = (data.fov_degrees.to_radians() / 2.0).tan();
    let ndc = Vec2::new(
        x / (z * tan_half * size.width / size.height),
        y / (z * tan_half),
    );
    Vec2::new(
        f32::midpoint(ndc.x, 1.0) * size.width,
        f32::midpoint(1.0, -ndc.y) * size.height,
    )
}

#[test]
fn a_card_at_the_tables_centre_projects_where_a_hand_calculation_says() {
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let (x, y) = (
        crate::test_data::data().card.width / 2.0,
        crate::test_data::data().card.height / 2.0,
    );
    for corner in [
        Vec3::new(-x, -y, 0.0),
        Vec3::new(x, -y, 0.0),
        Vec3::new(x, y, 0.0),
        Vec3::new(-x, y, 0.0),
    ] {
        let expected = by_hand(corner, &camera);
        let projected = camera.to_screen(corner);
        assert!(
            projected.distance(expected) < 0.01,
            "{corner}: {projected} by the camera, {expected} by hand"
        );
    }
}

#[test]
fn the_reference_camera_is_the_one_fitted_to_arena() {
    let camera = crate::test_data::data()
        .profiles
        .for_size(window(1440.0, 900.0).size)
        .table
        .camera;
    assert_eq!(
        reference().camera.pitch_degrees.to_bits(),
        camera.pitch_degrees.to_bits()
    );
    assert_eq!(
        reference().camera.fov_degrees.to_bits(),
        camera.fov_degrees.to_bits()
    );
}

/// The desktop windows every recording is checked in (`beryl_inspect::pages::REFERENCE_WINDOWS`,
/// which this crate cannot depend on).
const REFERENCE_WINDOWS: [(f32, f32); 4] = [
    (1440.0, 900.0),
    (1600.0, 900.0),
    (1920.0, 1080.0),
    (2560.0, 1080.0),
];

#[test]
fn resizing_keeps_the_required_rectangle_in_view() {
    for (width, height) in REFERENCE_WINDOWS {
        assert!(Camera::fit(reference().camera, window(width, height)).required_fits());
        let camera = Camera::fit(reference().camera, window(width, height));
        let [low, high] = reference().camera.required;
        for corner in [
            low,
            Vec2::new(high.x, low.y),
            high,
            Vec2::new(low.x, high.y),
        ] {
            let p = camera.to_screen(corner.extend(0.0));
            assert!(
                (-0.01..=width + 0.01).contains(&p.x) && (-0.01..=height + 0.01).contains(&p.y),
                "{width}x{height}: {corner} lands at {p}"
            );
        }
    }
}

#[test]
fn lands_stand_untapped_first_with_each_name_together_in_first_appearance_order() {
    let mut builder = ViewBuilder::new();
    for (name, tapped) in [
        ("Island", true),
        ("Plains", false),
        ("Island", false),
        ("Swamp", true),
        ("Plains", false),
    ] {
        let land = builder.card(ZoneName::Battlefield, Side::You, name);
        land.types = vec![CardType::Land];
        land.tapped = tapped;
    }
    let view = builder.build();
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let placement = place(&view, &[], reference(), &camera, &Scroll::default());
    let mut slots: Vec<u16> = view.objects.iter().map(|object| object.slot.0).collect();
    slots.sort_by(|&a, &b| {
        let x = |slot: u16| placement.get(ObjectId(slot)).unwrap().pose.position.x;
        x(a).total_cmp(&x(b))
    });
    assert_eq!(slots, [2, 1, 4, 0, 3]);
}

#[test]
fn abilities_on_the_stack_are_drawn_as_cards_above_the_spell_beneath_them() {
    // A spell and then two abilities, which the reset numbers 3 and 4: a view's stack names
    // no ability, so the numbers come from the script.
    let mut builder = ViewBuilder::new();
    let angel = builder.card(ZoneName::Battlefield, Side::You, "Serra Angel");
    angel.types = vec![CardType::Creature];
    let angel = angel.slot;
    builder.card(ZoneName::Stack, Side::You, "Lightning Bolt");
    let mut update = builder.update();
    let ability = |controller| StackItem::Ability {
        source: Some(angel),
        ability: 0,
        granted: false,
        name: "Serra Angel".to_owned(),
        triggered: true,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller,
        manual: None,
        targets: Vec::new(),
        x: 0,
    };
    update.view.stack.push(ability(Side::You));
    update.view.stack.push(ability(Side::Opponent));
    update.view.script = Some(script(
        Some(Reset {
            abilities: vec![3, 4],
        }),
        Vec::new(),
        Vec::new(),
    ));
    let view = update.view.clone();
    let mut client = client();
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(update)));
    let frame = client.frame(0).clone();

    let abilities: Vec<_> = frame
        .resolved
        .iter()
        .filter(|r| r.role == Role::Ability)
        .collect();
    let ids: Vec<u32> = abilities.iter().map(|r| r.id.0).collect();
    assert_eq!(ids, [ABILITY_IDS + 3, ABILITY_IDS + 4]);
    assert_eq!(frame.name(abilities[0].name), "Serra Angel");
    let sides: Vec<Place> = abilities.iter().map(|r| r.place).collect();
    let on_stack = |side| Place::Card {
        zone: ZoneName::Stack,
        side,
        row: None,
    };
    assert_eq!(sides, [on_stack(Side::You), on_stack(Side::Opponent)]);

    // The spell is the stack's bottom item and the abilities its second and third.
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let placement = place(&view, &[3, 4], reference(), &camera, &Scroll::default());
    let spell = view.objects.iter().find(|o| o.zone == ZoneName::Stack);
    let spell = placement.get(spell.expect("a spell").slot).expect("placed");
    let depths = [
        spell.pose.position,
        placement.ability(3).expect("placed").pose.position,
        placement.ability(4).expect("placed").pose.position,
    ]
    .map(|position| camera.depth(position));
    assert!(
        depths[0] > depths[1] && depths[1] > depths[2],
        "each item stands nearer the eye than the one beneath it: {depths:?}"
    );

    // `just b3d tree` prints a line for each, and no two elements share an id.
    let printed = tree(&frame, &TreeOptions::default()).expect("a tree");
    let lines = printed.lines().filter(|line| line.contains("ability"));
    assert_eq!(lines.count(), 2, "{printed}");
    let context = Context {
        recording: "abilities".to_owned(),
        seat: Side::You,
        layout: crate::REGULAR,
        audit: crate::test_data::data().audit,
        checks: crate::REGULAR.checks(
            &crate::test_data::data().profiles,
            kit_base::geometry::Size::default(),
        ),
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        safe_area: Insets::default(),
        at: 0,
    };
    assert!(duplicates(&frame, &context).is_empty());
}

/// A view with `count` cards in each player's hand.
fn hands(count: usize) -> PlayerView {
    let mut builder = ViewBuilder::new();
    for index in 0..count {
        builder.card(ZoneName::Hand, Side::You, &format!("Mine {index}"));
        builder.card(ZoneName::Hand, Side::Opponent, &format!("Theirs {index}"));
    }
    builder.build()
}

/// The corners on screen of a card resting at `pose`, in points: its face's bottom left, bottom
/// right, top right and top left.
fn corners(pose: &Pose, camera: &Camera) -> [Vec2; 4] {
    let (x, y) = (
        crate::test_data::data().card.width / 2.0,
        crate::test_data::data().card.height / 2.0,
    );
    let transform = pose.transform();
    [(-x, -y), (x, -y), (x, y), (-x, y)]
        .map(|(x, y)| camera.to_screen(transform.transform_point3(Vec3::new(x, y, 0.0))))
}

/// The poses of `side`'s hand in `view`, in the order the view lists the cards.
fn hand_poses(view: &PlayerView, side: Side, camera: &Camera) -> Vec<Pose> {
    let placement = place(view, &[], reference(), camera, &Scroll::default());
    view.objects
        .iter()
        .filter(|object| {
            object.zone == ZoneName::Hand && object.zone_side.unwrap_or(object.owner) == side
        })
        .map(|object| placement.get(object.slot).expect("placed").pose)
        .collect()
}

/// The middle of the edge of a card nearest the screen's middle, on screen: the top edge of the
/// viewer's cards, the bottom edge of the opponent's, which stand upside down.
fn inner_edge(corners: [Vec2; 4], side: Side) -> Vec2 {
    let [a, b] = inner_corners(corners, side);
    (a + b) / 2.0
}

/// The two corners of `inner_edge`, the left one first.
fn inner_corners(corners: [Vec2; 4], side: Side) -> [Vec2; 2] {
    let mut sorted = corners;
    sorted.sort_by(|a, b| a.y.total_cmp(&b.y));
    let [a, b] = match side {
        Side::You => [sorted[0], sorted[1]],
        Side::Opponent => [sorted[2], sorted[3]],
    };
    if a.x < b.x { [a, b] } else { [b, a] }
}

#[test]
fn hands_of_any_size_lie_inside_the_window() {
    for count in [1, 2, 5, 7, 10, 20, 60] {
        let view = hands(count);
        for (width, height) in REFERENCE_WINDOWS {
            let camera = Camera::fit(reference().camera, window(width, height));
            for side in [Side::You, Side::Opponent] {
                let cards: Vec<[Vec2; 4]> = hand_poses(&view, side, &camera)
                    .iter()
                    .map(|pose| corners(pose, &camera))
                    .collect();
                assert_eq!(cards.len(), count);
                let label = format!("{count} cards in {side:?}'s hand at {width}x{height}");
                for corners in &cards {
                    assert!(
                        corners.iter().all(|c| (-0.5..=width + 0.5).contains(&c.x)),
                        "{label}: {corners:?} runs past the window's sides"
                    );
                    let edge = inner_edge(*corners, side);
                    assert!(
                        (0.0..=height).contains(&edge.y),
                        "{label}: the card's inner edge, at {edge}, is out of view"
                    );
                }
            }
        }
    }
}

/// Every card of the viewer's hand has a square `TARGET_POINTS` on a side that a press reaches,
/// however many it holds: a hand too large for one row of strips across the window, such as 60
/// cards, fills a second row behind the first.
#[test]
fn every_card_of_a_hand_of_any_size_can_be_pressed() {
    for count in [7, 20, 40, 60] {
        let mut builder = ViewBuilder::new();
        for index in 0..count {
            builder.card(ZoneName::Hand, Side::You, &format!("Mine {index}"));
        }
        for (width, height) in REFERENCE_WINDOWS {
            let size = Size { width, height };
            let mut client = client();
            client.handle(Input::Device(DeviceInput::Surface {
                size,
                scale: 2.0,
                safe_area: Insets::default(),
            }));
            let mut view = builder.clone();
            view.script(ScriptBuilder::reset().build());
            client.handle(Input::Update(Box::new(view.update())));
            let frame = client.frame(0).clone();
            let context = Context {
                recording: format!("{count} cards"),
                seat: Side::You,
                layout: crate::REGULAR,
                audit: crate::test_data::data().audit,
                checks: crate::REGULAR.checks(&crate::test_data::data().profiles, size),
                size,
                safe_area: Insets::default(),
                at: 0,
            };
            let faults = reachable(&frame, &context);
            assert!(
                faults.is_empty(),
                "{count} cards at {width}x{height}: {faults:#?}"
            );
        }
    }
}

#[test]
fn a_hands_cards_lie_on_one_circle_and_turn_more_the_farther_from_the_middle() {
    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    // Twenty cards, the most of these that one row holds in this window.
    for count in [7, 20] {
        let view = hands(count);
        for (side, data, down) in [
            (Side::You, reference().hand, 1.0),
            (Side::Opponent, reference().opponent_hand, -1.0),
        ] {
            let poses = hand_poses(&view, side, &camera);
            let cards: Vec<[Vec2; 4]> = poses.iter().map(|pose| corners(pose, &camera)).collect();
            let edges: Vec<Vec2> = cards
                .iter()
                .map(|corners| inner_edge(*corners, side))
                .collect();
            let wide = cards[0][0].distance(cards[0][1]);
            // One millimetre of the hand's plane, in points.
            let millimetre = wide / (crate::test_data::data().card.width * poses[0].scale);
            let radius = data.fan_radius * wide;
            let middle = (edges[0] + edges[count - 1]) / 2.0;
            // The circle's centre lies below the middle of the viewer's fan, above the
            // opponent's, by a radius from the middle card's edge.
            let top = edges
                .iter()
                .map(|edge| edge.y * down)
                .fold(f32::MAX, f32::min)
                * down;
            let centre = Vec2::new(middle.x, top + down * radius);
            for edge in &edges {
                let off = (edge.distance(centre) - radius).abs() / millimetre;
                assert!(
                    off < 0.5,
                    "{count} cards in {side:?}'s hand: an edge {off} mm off the circle"
                );
            }
            // The turn of each card on screen, from its inner edge, against the middle's.
            let turns: Vec<f32> = cards
                .iter()
                .map(|corners| {
                    let [a, b] = inner_corners(*corners, side);
                    (b.y - a.y).atan2(b.x - a.x)
                })
                .collect();
            let lean = data.bias_degrees.to_radians();
            let turn = |at: usize| (turns[at] - lean * down).abs();
            // Outward from the middle on each side, each card turns more than the one before.
            let half = count / 2;
            let left = (0..half).rev();
            let right = count - half..count;
            for side_of_fan in [left.collect::<Vec<_>>(), right.collect()] {
                for pair in side_of_fan.windows(2) {
                    assert!(
                        turn(pair[1]) > turn(pair[0]),
                        "{count} cards in {side:?}'s hand: card {} turns no more than {}",
                        pair[1],
                        pair[0]
                    );
                }
            }
            let ends = (turns[0] - lean * down).abs();
            assert!(
                ends > 0.01,
                "{count} cards in {side:?}'s hand: the end card turns"
            );
        }
    }
}

#[test]
fn stacks_of_any_size_stay_on_screen_with_the_newest_item_nearest() {
    for count in [1, 3, 10, 50] {
        let mut builder = ViewBuilder::new();
        for index in 0..count {
            builder.card(ZoneName::Stack, Side::You, &format!("Spell {index}"));
        }
        let view = builder.build();
        for (width, height) in REFERENCE_WINDOWS {
            let camera = Camera::fit(reference().camera, window(width, height));
            let placement = place(&view, &[], reference(), &camera, &Scroll::default());
            let poses: Vec<Pose> = view
                .stack
                .iter()
                .map(|item| match item {
                    StackItem::Spell { slot, .. } => placement.get(*slot).expect("placed").pose,
                    StackItem::Ability { .. } => unreachable!("only spells"),
                })
                .collect();
            let label = format!("a stack of {count} at {width}x{height}");
            for pose in &poses {
                let corners = corners(pose, &camera);
                assert!(
                    corners
                        .iter()
                        .all(|c| (0.0..=width).contains(&c.x) && (0.0..=height).contains(&c.y)),
                    "{label}: {corners:?} is off screen"
                );
            }
            for pair in poses.windows(2) {
                assert!(
                    camera.depth(pair[1].position) < camera.depth(pair[0].position),
                    "{label}: each item stands nearer the eye than the one beneath it"
                );
            }
        }
    }
}

#[test]
fn every_item_of_a_stack_of_any_size_can_be_pressed() {
    // A stack too tall to lean with a wide enough strip at each item's left stands in rows.
    for count in [2, 7, 8, 12, 25, 50] {
        let mut builder = ViewBuilder::new();
        for index in 0..count {
            builder.card(ZoneName::Stack, Side::You, &format!("Spell {index}"));
        }
        builder.script(ScriptBuilder::reset().build());
        for (width, height) in REFERENCE_WINDOWS {
            let size = Size { width, height };
            let mut client = client();
            client.handle(Input::Device(DeviceInput::Surface {
                size,
                scale: 2.0,
                safe_area: Insets::default(),
            }));
            client.handle(Input::Update(Box::new(builder.update())));
            let context = Context {
                recording: "stack".to_owned(),
                seat: Side::You,
                layout: crate::REGULAR,
                audit: crate::test_data::data().audit,
                checks: crate::REGULAR.checks(&crate::test_data::data().profiles, size),
                size,
                safe_area: Insets::default(),
                at: 0,
            };
            let faults = reachable(client.frame(0), &context);
            let messages: Vec<&str> = faults.iter().map(|f| f.message.as_str()).collect();
            assert!(
                messages.is_empty(),
                "a stack of {count} at {width}x{height}: {messages:#?}"
            );
        }
    }
}

#[test]
fn a_spell_that_exiled_itself_rests_in_exile_while_the_view_still_lists_it() {
    // A spell that exiles itself as it resolves and then asks a player to choose: the view
    // drawn at the choice still lists the spell on its stack, though its card is in exile.
    let mut builder = ViewBuilder::new();
    let spell = builder
        .card(ZoneName::Stack, Side::Opponent, "Time Spiral")
        .slot;
    let mut view = builder.build();
    assert!(
        view.stack
            .iter()
            .any(|item| matches!(item, StackItem::Spell { slot, .. } if *slot == spell)),
        "the view lists the spell on its stack"
    );
    let object = view.objects.iter_mut().find(|o| o.slot == spell);
    object.expect("the spell's card").zone = ZoneName::Exile;

    let camera = Camera::fit(reference().camera, window(1440.0, 900.0));
    let placement = place(&view, &[], reference(), &camera, &Scroll::default());
    let rest = placement.get(spell).expect("placed");
    assert_eq!(
        rest.place,
        Place::Card {
            zone: ZoneName::Exile,
            side: Side::Opponent,
            row: None,
        }
    );
}
