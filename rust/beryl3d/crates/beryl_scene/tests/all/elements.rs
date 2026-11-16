//! The flat layer's elements: matching a new tree to the kept one by role, name and place, grid
//! and absolute layout, and elements anchored to a card that follow it as it flies.

use beryl_protocol::script::Subject;
use beryl_scene::element::{Anchor, Display, Element, Position, Track, row, text};
use beryl_scene::frame::{ElementId, Frame, FrameBuilder, Role};
use beryl_scene::layout::{Layer, centre_of};
use beryl_scene::table::Table;
use glam::Vec2;

use crate::common::{MS, before, empty_frame, show, three_beats};

/// The frame `layer` writes on its own, with the layout work done since the last one.
fn write(layer: &mut Layer) -> Frame {
    let mut frame = Frame::with_capacity(64, 256, 4096);
    let mut builder = FrameBuilder::new(&mut frame);
    let (layout_nodes, text_runs) = layer.take_work();
    builder.layout_work(layout_nodes, text_runs);
    layer.write(&mut builder, Table::default().camera(), true);
    builder.finish().clone()
}

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

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn region(name: &str) -> Element {
    row().name(name)
}

#[test]
fn swapped_siblings_keep_their_ids_and_lay_out_only_their_parent() {
    let mut layer = Layer::new();
    let screen = |first: &str, second: &str| {
        row()
            .name("screen")
            .child(text(first).padding(4.0))
            .child(text(second).padding(4.0))
    };
    layer.show(Some(screen("Keep", "Mulligan")));
    let before = write(&mut layer);
    layer.show(Some(screen("Mulligan", "Keep")));
    let after = write(&mut layer);

    for name in ["Keep", "Mulligan"] {
        assert_eq!(
            find(&before, Role::Text, name).0,
            find(&after, Role::Text, name).0,
            "{name} keeps its id"
        );
    }
    assert!(
        find(&after, Role::Text, "Mulligan").1[0] < find(&after, Role::Text, "Keep").1[0],
        "the two changed places"
    );
    // The window's root and the row whose children moved, and neither text.
    assert_eq!(after.work.layout_nodes, 2);
    assert_eq!(after.work.text_runs, 0);
}

#[test]
fn a_grid_track_of_points_leaves_the_rest_to_a_fraction() {
    let mut layer = Layer::new();
    layer.show(Some(
        region("grid")
            .display(Display::Grid)
            .grid_template_columns(vec![Track::Points(100.0), Track::Fraction(1.0)])
            .width(400.0)
            .height(50.0)
            .child(region("first"))
            .child(region("second")),
    ));
    let frame = write(&mut layer);
    let [grid_x, ..] = find(&frame, Role::Region, "grid").1;
    let [x, _, width, height] = find(&frame, Role::Region, "second").1;
    assert!(
        close(x - grid_x, 100.0),
        "the second column starts at {}",
        x - grid_x
    );
    assert!(close(width, 300.0), "the second column is {width} wide");
    assert!(close(height, 50.0), "a cell fills its row, not {height}");
}

#[test]
fn an_absolute_child_stands_at_its_insets() {
    let mut layer = Layer::new();
    layer.show(Some(
        region("parent").width(300.0).height(200.0).child(
            region("corner")
                .position(Position::Absolute)
                .inset([None, Some(10.0), Some(20.0), None])
                .width(50.0)
                .height(30.0),
        ),
    ));
    let frame = write(&mut layer);
    let [px, py, pw, ph] = find(&frame, Role::Region, "parent").1;
    let [x, y, width, height] = find(&frame, Role::Region, "corner").1;
    assert!(
        close(px + pw - (x + width), 10.0),
        "{} from the right",
        px + pw - x - width
    );
    assert!(
        close(py + ph - (y + height), 20.0),
        "{} from the bottom",
        py + ph - y - height
    );
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
        badge.anchor = Some(Anchor::Object(land, Vec2::new(0.5, 0.5)));
        badge
    })));
    layer.take_work();

    let mut frame = empty_frame();
    let mut centres = Vec::new();
    let mut now = 0;
    while now <= 600 * MS {
        let mut builder = FrameBuilder::new(&mut frame);
        show.frame(now, &mut builder);
        let card = builder.centre(ElementId(u32::from(cards.land.0)));
        layer.follow(|subject| centre_of(&builder, show.table(), subject));
        let (layout_nodes, text_runs) = layer.take_work();
        assert_eq!((layout_nodes, text_runs), (0, 0), "no layout at {now}");
        layer.write(&mut builder, show.table().camera(), true);
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
        badge.anchor = Some(Anchor::Object(Subject::Object { slot }, Vec2::ZERO));
        badge
    })));
    layer.follow(|_| None);
    let frame = write(&mut layer);
    assert!(
        !(frame.resolved.iter()).any(|element| frame.name(element.name) == "Gone"),
        "no element for a card that is not shown"
    );
    assert_eq!(layer.press_at(Vec2::ZERO, true), None);
}

#[test]
fn an_element_that_gains_an_anchor_stands_at_its_point() {
    let mut layer = Layer::new();
    let screen = |anchor: Option<Anchor>| {
        let mut badge = region("badge").width(40.0).height(20.0);
        badge.anchor = anchor;
        region("screen").padding(30.0).child(badge)
    };
    layer.show(Some(screen(None)));
    write(&mut layer);
    layer.show(Some(screen(Some(Anchor::Point(
        Vec2::new(200.0, 100.0),
        Vec2::new(0.5, 0.5),
    )))));
    let frame = write(&mut layer);
    let [x, y, width, height] = find(&frame, Role::Region, "badge").1;
    let want = [180.0, 90.0, 40.0, 20.0];
    assert!(
        [x, y, width, height]
            .iter()
            .zip(want)
            .all(|(&a, b)| close(a, b)),
        "the badge's box is {:?}",
        [x, y, width, height]
    );
}
