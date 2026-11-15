//! The table at rest: the camera's fit, and where each zone's cards are placed.

use beryl_protocol::protocol::{CardType, ObjectId, PlayerView, RowName, Side, ZoneName};
use beryl_scene::frame::{Frame, Place, Role};
use beryl_scene::input::{Input, Insets, Size};
use beryl_scene::table::{CARD_HEIGHT, CARD_WIDTH, Camera, DESKTOP, Window, place};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};
use beryl_scene::{Client, Config};
use glam::{Vec2, Vec3};

fn window(width: f32, height: f32) -> Window {
    Window {
        size: Size { width, height },
        scale: 2.0,
        safe_area: Insets::default(),
    }
}

fn client() -> Client {
    Client::new(Config {
        elements: 2048,
        draws: 2048,
        name_bytes: 65_536,
    })
}

/// The frame a client draws for `view` in a 1440 by 900 window.
fn frame_of(client: &mut Client, view: &ViewBuilder) -> Frame {
    client.handle(Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
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
    assert_eq!(cards, view.objects.len(), "every card is drawn");
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
    // The opponent's library is the viewer's turned half a turn about the table's centre.
    let camera = Camera::fit(DESKTOP.camera, window(1440.0, 900.0));
    let placement = place(&view, &DESKTOP, &camera, &Default::default());
    let library = |side: Side| {
        let player = match side {
            Side::You => &view.players.you,
            Side::Opponent => &view.players.opponent,
        };
        placement.get(player.library[0]).unwrap().pose.position
    };
    assert_eq!(
        library(Side::Opponent).truncate(),
        -library(Side::You).truncate()
    );
}

#[test]
fn placement_is_the_same_for_the_same_view_and_window() {
    let view = ViewBuilder::sample().build();
    let camera = Camera::fit(DESKTOP.camera, window(1440.0, 900.0));
    let first = place(&view, &DESKTOP, &camera, &Default::default());
    let again = place(&view, &DESKTOP, &camera, &Default::default());
    assert_eq!(first, again);
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
    let camera = Camera::fit(DESKTOP.camera, window(1440.0, 900.0));
    let placement = place(&view, &DESKTOP, &camera, &Default::default());
    let [low, high] = DESKTOP.camera.required;
    let mut drawn = 0;
    for object in &view.objects {
        let rest = placement.get(object.slot).unwrap();
        if !rest.visible {
            continue;
        }
        drawn += 1;
        let p = rest.pose.position;
        let half = if object.tapped {
            CARD_HEIGHT
        } else {
            CARD_WIDTH
        } / 2.0;
        assert!(p.x - half >= low.x && p.x + half <= high.x, "{p}");
        assert!(
            p.y - CARD_HEIGHT / 2.0 >= low.y && p.y + CARD_HEIGHT / 2.0 <= high.y,
            "{p}"
        );
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
        frame.resolved.len(),
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
                pair[1] - pair[0] >= DESKTOP.strip - 0.01,
                "{row:?}: {pair:?}"
            );
        }
    }
}

/// A point of the table on screen, computed from the camera's definition without its
/// matrices: the eye stands back from the target along the pitched line of sight, and a point
/// lands on screen by similar triangles.
fn by_hand(point: Vec3, camera: &Camera) -> Vec2 {
    let data = DESKTOP.camera;
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
    let camera = Camera::fit(DESKTOP.camera, window(1440.0, 900.0));
    let (x, y) = (CARD_WIDTH / 2.0, CARD_HEIGHT / 2.0);
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
fn resizing_keeps_the_required_rectangle_in_view() {
    for (width, height) in [(1440.0, 900.0), (2560.0, 1080.0)] {
        let camera = Camera::fit(DESKTOP.camera, window(width, height));
        let [low, high] = DESKTOP.camera.required;
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
    let camera = Camera::fit(DESKTOP.camera, window(1440.0, 900.0));
    let placement = place(&view, &DESKTOP, &camera, &Default::default());
    let mut slots: Vec<u16> = view.objects.iter().map(|object| object.slot.0).collect();
    slots.sort_by(|&a, &b| {
        let x = |slot: u16| placement.get(ObjectId(slot)).unwrap().pose.position.x;
        x(a).total_cmp(&x(b))
    });
    assert_eq!(slots, [2, 1, 4, 0, 3]);
}

#[test]
fn desktop_camera_is_the_data_in_space_json() {
    let space: serde_json::Value = serde_json::from_str(beryl_scene::data::SPACE).unwrap();
    let pitch = space["layout"]["desktop"]["camera"]["pitch_degrees"]
        .as_f64()
        .unwrap();
    assert!((f64::from(DESKTOP.camera.pitch_degrees) - pitch).abs() < 1e-6);
}
