//! Standalone flat-layer layout, clipping, scrolling and transitions.

use glam::{Affine3A, Vec2};
use kit_base::geometry::{Window, plane::ScreenPlane};
use kit_base::pose::Curve;
use kit_frame::frame::{ElementId, Frame, FrameBuilder, Role};
use kit_ui::element::{Anchor, Display, Element, Overflow, Position, Track, column, row, text};
use kit_ui::input::Tier;
use kit_ui::layout::Layer;

const MS: u64 = 1_000_000;
const SCALE: f32 = 2.0;

/// The frame `layer` writes on its own, with the layout work done since the last one.
fn write(layer: &mut Layer) -> Frame {
    let mut frame = Frame::with_capacity(64, 256, 4096);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    let (layout_nodes, text_runs) = layer.take_work();
    builder.layout_work(layout_nodes, text_runs);
    let plane = ScreenPlane::new(Affine3A::IDENTITY, window(), 1.0, 1.0);
    layer.write(&mut builder, &plane, true);
    builder.finish();
    frame
}

/// A 1440 by 900 point window at two pixels per point.
fn window() -> Window {
    Window {
        scale: SCALE,
        ..Window::default()
    }
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
    layer.show(Some(screen("First", "Second")));
    let before = write(&mut layer);
    layer.show(Some(screen("Second", "First")));
    let after = write(&mut layer);

    for name in ["First", "Second"] {
        assert_eq!(
            find(&before, Role::Text, name).0,
            find(&after, Role::Text, name).0,
            "{name} keeps its id"
        );
    }
    assert!(
        find(&after, Role::Text, "Second").1[0] < find(&after, Role::Text, "First").1[0],
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

/// A layer laid out in a 1440 by 900 window at two pixels to the point.
fn layer() -> Layer {
    let mut layer = Layer::new();
    layer.resize(window());
    layer
}

#[test]
fn scrolling_moves_the_rows_without_layout_and_only_the_visible_ones_take_presses() {
    let mut layer = layer();
    let rows = (0..30).map(|index| text(&format!("Row {index}")).on_press(1_u8));
    layer.show(Some(
        column()
            .name("box")
            .overflow(Overflow::Scroll)
            .height(200.0)
            .children(rows),
    ));
    let before = write(&mut layer);
    let [x, y, ..] = find(&before, Role::Region, "box").1;
    let scroller = (layer.scroller_at(Vec2::new(x + 5.0, y + 5.0), true)).expect("the box scrolls");
    assert!(layer.scroll(scroller, Vec2::new(0.0, 300.0)));
    let after = write(&mut layer);

    assert_eq!((after.work.layout_nodes, after.work.text_runs), (0, 0));
    let top = |frame: &Frame, row: usize| find(frame, Role::Text, &format!("Row {row}")).1[1];
    for row in [0, 15, 29] {
        assert!(
            close(top(&before, row) - top(&after, row), 300.0),
            "row {row} moved by 300"
        );
    }
    for element in after.resolved.iter().filter(|e| e.role == Role::Text) {
        let (low, high) = (element.corners[2].y, element.corners[0].y);
        let shown = high > y && low < y + 200.0;
        assert_eq!(element.interactive, shown, "{}", after.name(element.name));
        assert!(element.clip.is_some());
    }
    // The wheel cannot scroll past the last row.
    assert!(layer.scroll(scroller, Vec2::new(0.0, 10_000.0)));
    assert!(!layer.scroll(scroller, Vec2::new(0.0, 10.0)));
}

#[test]
fn nested_clips_clip_to_their_intersection() {
    let mut layer = layer();
    layer.show(Some(
        region("outer")
            .overflow(Overflow::Clip)
            .width(200.0)
            .height(100.0)
            .child(
                region("inner")
                    .overflow(Overflow::Clip)
                    .margin([50.0, 0.0, 0.0, 150.0])
                    .width(100.0)
                    .height(100.0)
                    .shrink(0.0)
                    .child(text("Clipped")),
            ),
    ));
    let frame = write(&mut layer);
    let [ox, oy, ..] = find(&frame, Role::Region, "outer").1;
    let cut = frame.resolved[find_index(&frame, "Clipped")]
        .clip
        .expect("the text is clipped");
    let want = [ox + 150.0, oy + 50.0, 50.0, 50.0];
    assert!(
        [cut.x, cut.y, cut.width, cut.height]
            .iter()
            .zip(want)
            .all(|(&a, b)| close(a, b)),
        "{cut:?}"
    );
    // The frame's clips are in physical pixels.
    let inner = frame.clips[2];
    assert!(
        close(inner.x, 2.0 * want[0]) && close(inner.width, 100.0),
        "{inner:?}"
    );
}

/// The index in `frame.resolved` of the element named `name`.
fn find_index(frame: &Frame, name: &str) -> usize {
    (frame.resolved.iter())
        .position(|element| frame.name(element.name) == name)
        .unwrap_or_else(|| panic!("no element named {name:?}"))
}

#[test]
fn a_fading_node_is_partway_through_its_transition() {
    let mut layer = layer();
    let screen = |opacity: f32| {
        kit_ui::widgets::screen::with_transitions(
            region("screen").child(text("Fading").opacity(opacity)),
        )
    };
    layer.show(Some(screen(1.0)));
    layer.tick(0);
    assert!(!layer.tick(1_000 * MS).moving, "the fade in has ended");
    layer.show(Some(screen(0.0)));
    let begun = layer.tick(2_000 * MS);
    assert!(begun.moving, "the fade out begins at the next frame");
    assert!(begun.began, "the frame that begins it says so");
    let half = 2_000 * MS + 90 * MS;
    let halfway = layer.tick(half);
    assert!(halfway.moving && !halfway.began);
    let frame = write(&mut layer);
    let opacity = frame.resolved[find_index(&frame, "Fading")].opacity;
    let want = 1.0 - Curve::EASE_OUT.ease(0.5);
    assert!(close(opacity, want), "{opacity} halfway, not {want}");
    assert!(!layer.tick(2_000 * MS + 180 * MS).moving);
    let frame = write(&mut layer);
    assert!(close(
        frame.resolved[find_index(&frame, "Fading")].opacity,
        0.0
    ));
}

#[test]
fn a_screen_built_again_before_its_first_frame_glides_nothing() {
    // The start screen is built before the host's agents arrive and again with them, both
    // before any frame draws it. The rows push the title above them to a new place, but no
    // frame ever drew the title at its old one, so it must not glide there. A screen shown
    // before the first frame stands whole from that frame, as the web's first page does, so
    // nothing moves at all.
    let mut layer = layer();
    let screen = |rows: usize| {
        let rows = (0..rows).map(|at| region(&format!("row {at}")).width(100.0).height(50.0));
        column()
            .name("screen")
            .width(200.0)
            .child(text("Title"))
            .child(column().name("rows").children(rows))
    };
    layer.show(Some(screen(0)));
    layer.show(Some(screen(2)));
    layer.tick(0);
    assert!(
        !layer.tick(10 * MS).moving,
        "the first screen is not fading in"
    );
    let (_, first) = find(&write(&mut layer), Role::Text, "Title");
    layer.tick(1_000 * MS);
    let (_, rest) = find(&write(&mut layer), Role::Text, "Title");
    assert!(
        close(first[1], rest[1]),
        "the title stands at {first:?} on the first frames, not where it rests, {rest:?}"
    );
}

/// A scrolling column of `count` rows named "Row 0" on, 200 points tall.
fn rows_box(count: usize) -> Element {
    let rows = (0..count).map(|index| text(&format!("Row {index}")).on_press(1_u8));
    column()
        .name("box")
        .overflow(Overflow::Scroll)
        .height(200.0)
        .children(rows)
}

#[test]
fn a_list_that_loses_rows_scrolls_back_to_its_new_end() {
    let mut layer = layer();
    layer.show(Some(rows_box(30)));
    let frame = write(&mut layer);
    let [x, y, ..] = find(&frame, Role::Region, "box").1;
    let scroller = (layer.scroller_at(Vec2::new(x + 5.0, y + 5.0), true)).expect("the box scrolls");
    layer.scroll(scroller, Vec2::new(0.0, 10_000.0));

    // Ten rows still pass the box, by less than the thirty did: the last row now ends at the
    // box's bottom, with no gap after it.
    layer.show(Some(rows_box(10)));
    let frame = write(&mut layer);
    let last = find(&frame, Role::Text, "Row 9").1;
    assert!(
        close(last[1] + last[3], y + 200.0),
        "{last:?} in a box at {y}"
    );
    // With every row inside the box, nothing is scrolled off its top.
    layer.show(Some(rows_box(2)));
    let frame = write(&mut layer);
    assert!(close(find(&frame, Role::Text, "Row 0").1[1], y));
}

#[test]
fn an_anchored_element_in_a_scrolling_list_is_drawn_and_pressed_outside_it() {
    let mut layer = layer();
    let mut beside = text("Beside").on_press(1_u8);
    beside.anchor = Some(Anchor::Point(Vec2::new(900.0, 100.0), Vec2::ZERO));
    layer.show(Some(rows_box(30).child(beside)));
    let frame = write(&mut layer);
    let element = &frame.resolved[find_index(&frame, "Beside")];
    assert!(element.clip.is_none(), "{:?}", element.clip);
    assert!(element.interactive);
    let (id, ..) = layer
        .press_at(Vec2::new(905.0, 105.0), true)
        .expect("the anchored element takes the press");
    assert_eq!(id, element.id);
}

#[test]
fn a_child_overflowing_a_box_that_does_not_clip_takes_presses_where_it_is_drawn() {
    let mut layer = layer();
    layer.show(Some(
        region("narrow").width(20.0).height(20.0).child(
            text("Wide")
                .on_press(1_u8)
                .margin([0.0, 0.0, 0.0, 60.0])
                .shrink(0.0),
        ),
    ));
    let frame = write(&mut layer);
    let (id, [x, y, width, height]) = find(&frame, Role::Text, "Wide");
    assert!(frame.resolved[find_index(&frame, "Wide")].interactive);
    let centre = Vec2::new(x + width / 2.0, y + height / 2.0);
    let (pressed, ..) = layer.press_at(centre, true).expect("the press reaches it");
    assert_eq!(pressed, id);
}
