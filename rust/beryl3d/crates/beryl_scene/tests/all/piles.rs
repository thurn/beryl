//! The library, graveyard and exile piles: their real height, the few records a tall pile
//! draws, their elements and counts, a card leaving from inside one, and the tray of cards a
//! prompt holds up.

use crate::common::{MS, empty_frame, frame, show, window};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Side, Why, ZoneName};
use beryl_protocol::script::Act;
use beryl_scene::frame::{Frame, Role};
use beryl_scene::input::{Input, Insets, Size};
use beryl_scene::motion::pose_at;
use beryl_scene::pose::{Pose, Segment};
use beryl_scene::table::{
    Camera, DESKTOP, Held, LAYER, Table, TrayKind, TraySpace, landing, tray, tray_boxes,
};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, object, timing};
use beryl_scene::{Client, Config};
use glam::{Vec2, Vec3};

fn client() -> Client {
    Client::new(Config {
        elements: 2048,
        draws: 2048,
        name_bytes: 65_536,
    })
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
    client.handle(Input::Surface {
        size: Size {
            width: 1600.0,
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

/// How many records the table draws, the cloth among them: those before the flat layer's.
fn table_records(frame: &Frame) -> usize {
    frame.draws[..frame.layer as usize].len()
}

/// The table at rest in `view`, in a 1600 by 900 window.
fn table_of(view: &ViewBuilder) -> Table {
    let mut table = Table::new(DESKTOP, window(1600.0, 900.0));
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
    height(top) - height(bottom) + LAYER
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
        16,
        "the top card, the one under it, two dark records of the rims beneath, a light line for \
         each of the 11 cards beneath them that steps further than its line is wide, and one \
         light band for the 47 deeper cards"
    );
    assert_eq!(table_records(&frame_of(&library(1))) - empty, 1);
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
        (before - after - LAYER).abs() < 1e-2,
        "{before} became {after}"
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
    let mut table = Table::new(DESKTOP, window(1440.0, 900.0));
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
        let camera = Camera::fit(DESKTOP.camera, window(width, height));
        for counts in trays {
            let piles: Vec<Vec<usize>> = counts.iter().map(|&count| (0..count).collect()).collect();
            let piles: Vec<&[usize]> = piles.iter().map(Vec::as_slice).collect();
            let poses = tray(&piles, TraySpace::DESKTOP, &camera);
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
    let space = TraySpace::DESKTOP;
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
