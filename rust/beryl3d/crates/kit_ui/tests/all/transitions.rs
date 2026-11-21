use glam::{Affine3A, Vec2};
use kit_base::geometry::{Window, plane::ScreenPlane};
use kit_base::pose::Curve;
use kit_frame::frame::{Frame, FrameBuilder, Resolved};
use kit_ui::element::{Element, Position, Presence, Property, Timing, Transition, row, text};
use kit_ui::input::Tier;
use kit_ui::layout::Layer;

const MS: u64 = 1_000_000;
const LINEAR: Timing = Timing::new(100.0, 0.0, Curve::LINEAR);
const DELAYED: Timing = Timing::new(100.0, 20.0, Curve::LINEAR);
const FADE: Presence = Presence::fade(LINEAR);
const OPACITY: &[Transition] = &[Transition::new(Property::Opacity, DELAYED)];
const WIDTH: &[Transition] = &[Transition::new(Property::Width, LINEAR)];
const PAINT: &[Transition] = &[
    Transition::new(Property::Color, LINEAR),
    Transition::new(Property::Background, DELAYED),
];

fn draw(layer: &Layer) -> Frame {
    let mut frame = Frame::with_capacity(32, 256, 256);
    frame.looking = true;
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline, crate::card());
    let camera = ScreenPlane::new(Affine3A::IDENTITY, Window::default(), 1.0, 1.0);
    layer.write(&mut builder, &camera, true);
    builder.finish();
    frame
}

fn element<'a>(frame: &'a Frame, name: &str) -> &'a Resolved {
    frame
        .resolved
        .iter()
        .find(|element| frame.name(element.name) == name)
        .expect("the element is drawn")
}

fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

fn started(tree: Element) -> Layer {
    let mut layer = Layer::new();
    layer.show(Some(tree));
    layer.tick(0);
    layer.tick(MS);
    layer.take_work();
    layer
}

fn label(opacity: f32) -> Element {
    text("Label").opacity(opacity).transition(OPACITY)
}

#[test]
fn opacity_changes_after_its_delay_without_laying_out_text() {
    let mut layer = started(label(1.0));
    layer.show(Some(label(0.0)));
    assert_eq!(layer.take_work(), (0, 0), "only the text's opacity changed");

    let begun = layer.tick(10 * MS);
    assert!(begun.began && begun.moving);
    assert_eq!(begun.length, 120 * MS);
    layer.tick(30 * MS);
    close(element(&draw(&layer), "Label").opacity, 1.0);
    layer.tick(80 * MS);
    close(element(&draw(&layer), "Label").opacity, 0.5);
    assert!(!layer.tick(130 * MS).moving);
    close(element(&draw(&layer), "Label").opacity, 0.0);
    assert_eq!(layer.take_work(), (0, 0));
}

#[test]
fn a_new_target_uses_the_last_drawn_opacity_as_its_start() {
    let mut layer = started(label(1.0));
    layer.show(Some(label(0.0)));
    layer.tick(5 * MS);
    layer.tick(50 * MS);
    let before = element(&draw(&layer), "Label").opacity;
    close(before, 0.75);

    layer.show(Some(label(0.5)));
    layer.tick(60 * MS);
    close(element(&draw(&layer), "Label").opacity, before);
    layer.tick(130 * MS);
    close(element(&draw(&layer), "Label").opacity, 0.625);
}

#[test]
fn a_removed_subtree_is_painted_during_its_leave_and_cannot_take_presses() {
    let tree = |child| {
        row()
            .name("Screen")
            .width(200.0)
            .height(120.0)
            .children(child)
    };
    let child = row()
        .name("Leaving")
        .width(80.0)
        .height(50.0)
        .background([40, 40, 40, 255])
        .child(text("Action").on_press(1_u32))
        .leave(&FADE);
    let mut layer = started(tree(Some(child)));
    let frame = draw(&layer);
    let action = element(&frame, "Action");
    let point = (action.corners[0] + action.corners[2]) / 2.0;
    assert!(layer.press_at(point, true).is_some());

    layer.show(Some(tree(None)));
    layer.take_work();
    layer.tick(10 * MS);
    layer.tick(60 * MS);
    let fading = draw(&layer);
    let orphan = element(&fading, "Action");
    assert_eq!(orphan.id, action.id);
    assert_eq!(orphan.corners, action.corners);
    close(orphan.opacity, 0.5);
    assert!(!orphan.interactive);
    assert!(layer.press_at(point, true).is_none());
    assert!(!layer.tick(110 * MS).moving);
    assert_eq!(draw(&layer).resolved.len(), 1);
    assert_eq!(layer.take_work(), (0, 0));
}

#[test]
fn translation_and_scale_on_enter_move_the_contents_and_hit_boxes_together() {
    const ENTER: Presence = Presence {
        translation: Some(Vec2::new(100.0, 0.0)),
        scale: Some(0.5),
        ..Presence::new(LINEAR)
    };
    let initial = || row().name("Screen").width(200.0).height(120.0);
    let mut layer = started(initial());
    layer.show(Some(
        initial().child(
            row()
                .name("Panel")
                .width(80.0)
                .height(50.0)
                .enter(&ENTER)
                .child(
                    row()
                        .name("Action")
                        .width(80.0)
                        .height(50.0)
                        .on_press(1_u32),
                ),
        ),
    ));
    layer.tick(10 * MS);
    let entering = draw(&layer);
    layer.tick(110 * MS);
    let resting = draw(&layer);
    let before = element(&entering, "Action").corners;
    let after = element(&resting, "Action").corners;
    close(before[0].x - after[0].x, 120.0);
    close(before[1].x - before[0].x, 40.0);
    let centre = (before[0] + before[2]) / 2.0;
    assert!(
        layer.press_at(centre, true).is_none(),
        "the action has moved"
    );
    let centre = (after[0] + after[2]) / 2.0;
    assert!(layer.press_at(centre, true).is_some());
}

#[test]
fn an_absolute_box_interpolates_its_width_without_reflowing_its_sibling() {
    let screen = |width| {
        row()
            .name("Screen")
            .width(300.0)
            .height(200.0)
            .child(
                row()
                    .name("Panel")
                    .position(Position::Absolute)
                    .width(width)
                    .height(50.0)
                    .background([40, 40, 40, 255])
                    .transition(WIDTH),
            )
            .child(text("Sibling"))
    };
    let mut layer = started(screen(40.0));
    let before = draw(&layer);
    layer.show(Some(screen(100.0)));
    layer.take_work();
    layer.tick(10 * MS);
    layer.tick(60 * MS);
    let halfway = draw(&layer);
    let corners = element(&halfway, "Panel").corners;
    close(corners[1].x - corners[0].x, 70.0);
    assert_eq!(
        element(&before, "Sibling").corners,
        element(&halfway, "Sibling").corners
    );
    assert_eq!(layer.take_work(), (0, 0));
}

#[test]
fn foreground_and_background_have_independent_timing() {
    let panel = |ink, background| {
        text("Label")
            .color(ink)
            .background(background)
            .transition(PAINT)
    };
    let mut layer = started(panel([0, 0, 0, 255], [40, 40, 40, 255]));
    layer.show(Some(panel([100, 0, 0, 255], [140, 40, 40, 255])));
    layer.tick(5 * MS);
    layer.tick(55 * MS);
    let frame = draw(&layer);
    let look = &frame.looks.elements[0];
    assert_eq!(look.color, [50, 0, 0, 255]);
    assert_eq!(look.background, Some([70, 40, 40, 255]));
    assert_eq!(layer.take_work(), (0, 0));
}
