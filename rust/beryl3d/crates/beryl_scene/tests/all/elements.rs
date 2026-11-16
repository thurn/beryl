//! The flat layer's elements: matching a new tree to the kept one by role, name and place, grid
//! and absolute layout, and elements anchored to a card that follow it as it flies.

use beryl_protocol::prompt::{ButtonId, Click, ClickRequest};
use beryl_protocol::script::Subject;
use beryl_scene::command::SessionCommand;
use beryl_scene::element::{
    Anchor, Chord, Display, Element, Overflow, Position, Press, Track, column, row, text,
};
use beryl_scene::frame::{ElementId, Frame, FrameBuilder, Role, StateBits};
use beryl_scene::input::{Input, Key, KeyCode, KeyPhase, Modifiers};
use beryl_scene::layout::{Layer, centre_of};
use beryl_scene::pose::Curve;
use beryl_scene::table::Table;
use glam::Vec2;

use crate::common::{MS, before, empty_frame, show, three_beats, window};
use crate::flat_layer::{client, deciding, decision, prompt};

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

/// A key pressed with `modifiers`.
fn key(code: KeyCode, modifiers: Modifiers) -> Input {
    Input::Key(Key {
        code,
        phase: KeyPhase::Down,
        modifiers,
    })
}

/// A layer laid out in a 1440 by 900 window at two pixels to the point.
fn layer() -> Layer {
    let mut layer = Layer::new();
    layer.resize(window(1440.0, 900.0));
    layer
}

#[test]
fn scrolling_moves_the_rows_without_layout_and_only_the_visible_ones_take_presses() {
    let mut layer = layer();
    let rows = (0..30).map(|index| text(&format!("Row {index}")).on_press(Press::Concede));
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
    let screen = |opacity: f32| region("screen").child(text("Fading").opacity(opacity));
    layer.show(Some(screen(1.0)));
    layer.tick(0);
    assert!(!layer.tick(1_000 * MS), "the fade in has ended");
    layer.show(Some(screen(0.0)));
    assert!(
        layer.tick(2_000 * MS),
        "the fade out begins at the next frame"
    );
    let half = 2_000 * MS + 90 * MS;
    assert!(layer.tick(half));
    let frame = write(&mut layer);
    let opacity = frame.resolved[find_index(&frame, "Fading")].opacity;
    let want = 1.0 - Curve::EASE_OUT.ease(0.5);
    assert!(close(opacity, want), "{opacity} halfway, not {want}");
    assert!(!layer.tick(2_000 * MS + 180 * MS));
    let frame = write(&mut layer);
    assert!(close(
        frame.resolved[find_index(&frame, "Fading")].opacity,
        0.0
    ));
}

#[test]
fn tab_moves_the_focus_to_the_next_button_and_return_presses_it() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.frame(0);
    client.handle(key(KeyCode::Tab, Modifiers::default()));
    client.handle(key(KeyCode::Tab, Modifiers::default()));
    let frame = client.frame(MS);
    let cancel = &frame.resolved[find_index(frame, "Cancel")];
    assert!(
        cancel.state.has(StateBits::FOCUSED),
        "Tab went to Auto Pay, then Cancel"
    );
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
            client.handle(Input::Platform { mac });
        }
        client.handle(key(KeyCode::B, modifiers));
        let frame = client.frame(0);
        (frame.resolved.iter()).any(|element| frame.name(element.name) == "bug report")
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
