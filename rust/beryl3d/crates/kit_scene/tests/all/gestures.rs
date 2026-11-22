//! Gesture routing over rectangles, with no game or app state.

use glam::Vec2;
use kit_base::data::pointer::PointerTuning;
use kit_base::geometry::Rect;
use kit_frame::frame::{ElementId, Frame, FrameBuilder, NameRef, Place, Resolved, Role, StateBits};
use kit_scene::input::{Direction, Routed, Router, Stage, Target, TouchPolicy};
use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase, Tier};

const HOME: Vec2 = Vec2::new(10.0, 10.0);
const AWAY: Vec2 = Vec2::new(110.0, 10.0);
const MS: u64 = 1_000_000;

type Node = Target<ElementId, u32>;

struct Scene {
    router: Router<Node>,
    frame: Frame,
    tuning: PointerTuning,
}

impl Scene {
    fn new() -> Self {
        let mut frame = Frame::with_capacity(4, 0, 64);
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline, crate::card());
        for (id, x) in [(1, 0.0), (2, 100.0), (3, 100.0)] {
            let rect = Rect {
                x,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            };
            builder.resolved(
                "item",
                Resolved {
                    id: ElementId(id),
                    parent: u32::MAX,
                    role: Role::Button,
                    name: NameRef::default(),
                    place: Place::Layer,
                    pose: None,
                    corners: [
                        Vec2::new(rect.x, rect.y),
                        Vec2::new(rect.x + rect.width, rect.y),
                        Vec2::new(rect.x + rect.width, rect.y + rect.height),
                        Vec2::new(rect.x, rect.y + rect.height),
                    ],
                    depth: 0.0,
                    opacity: 1.0,
                    clip: None,
                    interactive: true,
                    state: StateBits::default(),
                    face: None,
                    count: 0,
                    annotations: Default::default(),
                },
            );
        }
        builder.finish();
        Self {
            router: Router::default(),
            frame,
            tuning: PointerTuning::parse(include_str!(
                "../../../../../../data/beryl3d/pointer.json"
            ))
            .expect("repository pointer tuning"),
        }
    }

    fn point(&mut self, kind: PointerKind, phase: PointerPhase, at: Vec2) -> Routed<Node> {
        self.router.point(
            &self.frame,
            Pointer {
                id: u8::from(kind == PointerKind::Touch),
                kind,
                phase,
                position: at,
                button: Button::Primary,
                wheel: Vec2::ZERO,
                modifiers: Modifiers::default(),
            },
            0,
            &self.tuning,
            |hit| {
                (
                    hit.map(|node| Target {
                        node: node.id,
                        container: Some(u32::from(node.id.0 != 1)),
                    }),
                    TouchPolicy {
                        hold: true,
                        delayed_pickup: true,
                        ..TouchPolicy::default()
                    },
                )
            },
        )
    }
}

#[test]
fn a_drag_keeps_its_source_container_and_uses_the_frontmost_destination() {
    let mut scene = Scene::new();
    let kind = PointerKind::Mouse;
    let down = scene.point(kind, PointerPhase::Down, HOME).point.unwrap();
    assert_eq!(down.pressed.unwrap().node, ElementId(1));
    let start = scene
        .point(kind, PointerPhase::Move, AWAY)
        .point
        .unwrap()
        .drag
        .unwrap();
    assert_eq!(start.stage, Stage::Start);
    assert_eq!(start.direction, Direction::Horizontal);
    assert_eq!(start.source.unwrap().container, Some(0));
    assert_eq!(start.destination.unwrap().node, ElementId(3));
    assert_eq!(start.destination.unwrap().container, Some(1));

    let mut carried = scene.frame.resolved[0];
    carried.corners = carried.corners.map(|corner| corner + Vec2::new(100.0, 0.0));
    carried.state.0 |= StateBits::HELD;
    scene.frame.resolved[2] = carried;

    let back = scene
        .point(kind, PointerPhase::Move, HOME + Vec2::Y)
        .point
        .unwrap()
        .drag
        .unwrap();
    assert_eq!(back.stage, Stage::Move);
    assert_eq!(back.direction, Direction::Horizontal);

    let up = scene.point(kind, PointerPhase::Up, AWAY).point.unwrap();
    assert_eq!(up.drag.unwrap().stage, Stage::Drop);
    assert_eq!(up.drag.unwrap().source, start.source);
    assert_eq!(up.drag.unwrap().destination.unwrap().node, ElementId(2));
    assert!(up.released_by(|target| target.node).is_none());
}

#[test]
fn a_touch_can_wait_to_pick_up_or_hold_the_captured_node() {
    let mut scene = Scene::new();
    let kind = PointerKind::Touch;
    scene.point(kind, PointerPhase::Down, HOME);
    assert!(scene.router.advance(MS, &scene.tuning).is_none());
    let hold = scene
        .router
        .advance((1 + scene.tuning.long_press_ms) * MS, &scene.tuning);
    assert_eq!(
        hold.map(|(node, at)| (node.unwrap().node, at)),
        Some((ElementId(1), HOME))
    );
    assert!(scene.router.advance(u64::MAX, &scene.tuning).is_none());

    scene.point(kind, PointerPhase::Cancel, HOME);
    scene.point(kind, PointerPhase::Down, HOME);
    let early = scene
        .point(kind, PointerPhase::Move, AWAY)
        .point
        .unwrap()
        .drag
        .unwrap();
    assert!(!early.pickup_ready);
    assert!(scene.router.advance(u64::MAX, &scene.tuning).is_none());

    scene.point(kind, PointerPhase::Cancel, AWAY);
    scene.point(kind, PointerPhase::Down, HOME);
    assert!(scene.router.advance(MS, &scene.tuning).is_none());
    assert!(
        scene
            .router
            .advance((1 + scene.tuning.touch.pickup_ms) * MS, &scene.tuning)
            .is_none()
    );
    let ready = scene
        .point(kind, PointerPhase::Move, AWAY)
        .point
        .unwrap()
        .drag
        .unwrap();
    assert!(ready.pickup_ready);
}

#[test]
fn claiming_a_press_for_scrolling_does_not_lose_the_drag_source() {
    let mut scene = Scene::new();
    let kind = PointerKind::Mouse;
    scene.point(kind, PointerPhase::Down, HOME);
    scene.point(kind, PointerPhase::Move, AWAY);
    scene.router.suppress_press();
    let up = scene.point(kind, PointerPhase::Up, HOME).point.unwrap();
    assert!(up.released_by(|target| target.node).is_none());
    assert_eq!(up.drag.unwrap().source.unwrap().node, ElementId(1));
}

#[test]
fn a_focused_control_can_consume_a_key_without_activating_a_shortcut() {
    use kit_scene::input::{KeyResponse, key};

    let mut calls = 0;
    let response = key(
        &mut calls,
        |calls| {
            *calls += 1;
            Some(KeyResponse::Handled)
        },
        |calls| {
            *calls += 10;
            Some(KeyResponse::Action("shortcut"))
        },
    );
    assert!(matches!(response, Some(KeyResponse::Handled)));
    assert_eq!(calls, 1);
    let response = key(
        &mut (),
        |()| None,
        |()| Some(KeyResponse::Action("shortcut")),
    );
    assert!(matches!(response, Some(KeyResponse::Action("shortcut"))));
}
