use glam::{Vec2, Vec3};
use kit_base::{geometry::Rect, pose::Pose};
use kit_frame::frame::{ElementId, Role};
use kit_scene::{
    Anchor, Band, KeepClear, Node, NodeQuery, NodeRef, Show, SlotRef, SolvePhase, SolveStep, Space,
    Tree,
};

const BOX: Rect = Rect {
    x: 8.0,
    y: 12.0,
    width: 32.0,
    height: 16.0,
};

fn rectangle(
    id: u32,
    name: &'static str,
    parent: Option<u32>,
    space: Space,
    band: Band,
) -> Node<'static> {
    Node {
        id: ElementId(id),
        role: Role::Region,
        name: name.into(),
        parent,
        space,
        band,
        anchor: None,
        decoration: None,
        keep_clear: None,
        show: None,
    }
}

fn named(parent: Option<NodeRef>, name: &str) -> NodeRef {
    NodeRef::Named {
        parent: parent.map(Box::new),
        role: Role::Region,
        name: name.into(),
    }
}

#[test]
fn references_survive_rebuilding_a_mixed_scene() {
    let target = NodeRef::Id(ElementId(42));
    let label = named(Some(named(None, "panel")), "label");
    for (panel_id, label_id, reverse) in [(9, 12, false), (28, 29, true)] {
        let mut tree = Tree::default();
        let shape = rectangle(
            42,
            "shape",
            None,
            Space::Table {
                pose: Pose {
                    position: Vec3::new(1.0, 2.0, 3.0),
                    ..Pose::default()
                },
                bounds: BOX,
            },
            Band::Table,
        );
        if !reverse {
            tree.push(shape.clone());
        }
        let screen = Space::Screen { bounds: BOX };
        let mut panel = rectangle(panel_id, "panel", None, screen, Band::ScreenPanels);
        panel.keep_clear = Some(KeepClear {
            of: vec![NodeQuery::Node(target.clone())],
            places: vec![SlotRef(1), SlotRef(2)],
        });
        let parent = tree.push(panel);
        let mut child = rectangle(label_id, "label", Some(parent), screen, Band::ShownObjects);
        child.anchor = Some(Anchor {
            to: target.clone(),
            at: Vec2::splat(0.5),
            align: Vec2::ZERO,
            offset: Vec2::Y,
        });
        child.show = Some(Show::object(ElementId(42)));
        let index = tree.push(child);
        if reverse {
            tree.push(shape);
        }

        assert_eq!(tree.resolve(&label), Some(index));
        assert_eq!(tree.resolve(&named(None, "label")), None);
        let child = &tree.nodes()[index as usize];
        let anchor = child.anchor.as_ref().unwrap();
        let object = tree.resolve(&anchor.to).unwrap();
        assert!(matches!(
            tree.nodes()[object as usize].space,
            Space::Table { .. }
        ));
        assert_eq!(child.show.unwrap().object_id(), ElementId(42));
        let keep_clear = tree.nodes()[parent as usize].keep_clear.as_ref().unwrap();
        assert_eq!(keep_clear.of, [NodeQuery::Node(target.clone())]);
        assert_eq!(keep_clear.places, [SlotRef(1), SlotRef(2)]);
    }
}

#[test]
fn an_ambiguous_root_does_not_resolve_to_an_arbitrary_child() {
    let mut tree = Tree::default();
    let screen = Space::Screen { bounds: BOX };
    let parent = tree.push(rectangle(1, "panel", None, screen, Band::ScreenPanels));
    tree.push(rectangle(2, "label", Some(parent), screen, Band::Overlays));
    tree.push(rectangle(3, "panel", None, screen, Band::ScreenPanels));

    assert_eq!(
        tree.resolve(&named(Some(named(None, "panel")), "label")),
        None
    );
    assert_eq!(
        tree.resolve(&named(Some(NodeRef::Id(ElementId(1))), "label")),
        Some(1)
    );
}

#[test]
fn animation_places_anchors_without_repeating_layout() {
    let mut tree = Tree::default();
    let mut expected = [
        SolveStep::Arrange,
        SolveStep::Layout,
        SolveStep::KeepClear,
        SolveStep::Anchor,
        SolveStep::Anchor,
        SolveStep::Anchor,
    ]
    .into_iter();
    let mut record = |step, _: &mut Tree| assert_eq!(Some(step), expected.next());
    tree.solve(SolvePhase::Handle, &mut record);
    tree.solve(SolvePhase::Frame, &mut record);
    tree.solve(SolvePhase::Frame, &mut record);
    assert_eq!(expected.next(), None);
}

#[test]
fn anchored_rectangles_follow_their_targets_and_leave_with_them() {
    let mut tree = Tree::default();
    let mut label = Node::screen(ElementId(1), "label", BOX);
    label.anchor = Some(Anchor::new(
        ElementId(2),
        Vec2::new(1.0, 0.5),
        Vec2::new(0.0, 0.5),
        Vec2::X * 4.0,
    ));
    let label = tree.push(label);
    let mut detail = Node::screen(ElementId(3), "detail", BOX);
    detail.parent = Some(label);
    detail.anchor = Some(Anchor::new(ElementId(1), Vec2::Y, Vec2::ZERO, Vec2::ZERO));
    let detail = tree.push(detail);
    let target = tree.push(Node::screen(ElementId(2), "target", BOX));
    let other = tree.push(Node::screen(ElementId(4), "other", BOX));
    let mut child = Node::screen(ElementId(5), "child", BOX);
    child.parent = Some(other);
    tree.push(child);

    for delta in [0.0, 24.0] {
        tree.solve(SolvePhase::Frame, |_, tree| {
            tree.set_space(
                target,
                Space::Screen {
                    bounds: Rect {
                        x: BOX.x + delta,
                        y: BOX.y + delta,
                        ..BOX
                    },
                },
            );
        });
        let label_box = tree.nodes()[label as usize].space.screen_bounds();
        assert_eq!(
            label_box,
            Rect {
                x: 44.0 + delta,
                y: 12.0 + delta,
                ..BOX
            }
        );
        assert_eq!(
            tree.nodes()[detail as usize].space.screen_bounds(),
            Rect {
                y: label_box.y + label_box.height,
                ..label_box
            },
        );
    }

    tree.remove(target);
    tree.solve(SolvePhase::Frame, |_, _| {});
    assert_eq!(tree.nodes().len(), 2);
    assert_eq!(tree.nodes()[0].id, ElementId(4));
    assert_eq!(tree.nodes()[1].parent, Some(0));
}

#[test]
fn a_press_follows_paint_order_across_the_table_and_screen() {
    use kit_base::atlas::Tier;
    use kit_frame::frame::{Frame, FrameBuilder, NameRef, Place, Resolved, StateBits};
    use kit_scene::hit::hit;

    let point = Vec2::new(16.0, 16.0);
    let corners = [
        Vec2::new(BOX.x, BOX.y),
        Vec2::new(BOX.x + BOX.width, BOX.y),
        Vec2::new(BOX.x + BOX.width, BOX.y + BOX.height),
        Vec2::new(BOX.x, BOX.y + BOX.height),
    ];
    let node = |id, depth, state, projected: bool| Resolved {
        id: ElementId(id),
        parent: u32::MAX,
        role: if projected { Role::Card } else { Role::Button },
        name: NameRef::default(),
        place: if projected {
            Place::named("preview")
        } else {
            Place::Layer
        },
        pose: projected.then(Pose::default),
        corners,
        depth,
        opacity: 1.0,
        clip: None,
        interactive: true,
        state: StateBits(state),
        face: None,
        count: 1,
        annotations: Default::default(),
    };
    let mut frame = Frame::with_capacity(6, 0, 0);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline, crate::card());
    let dragged = builder.resolved("dragged", node(5, 7.0, StateBits::HELD, true));
    let moving = builder.resolved(
        "moving into preview",
        node(1, 5.0, StateBits::MOVING | StateBits::SHOWN, true),
    );
    let far = builder.resolved("far", node(0, 3.0, 0, true));
    let near = builder.resolved("near", node(2, 1.0, 0, true));
    let panel = builder.resolved("panel", node(3, 0.0, 0, false));
    let mut shown = node(4, 6.0, StateBits::SHOWN, true);
    shown.clip = Some(Rect {
        x: 20.0,
        width: 20.0,
        ..BOX
    });
    let shown = builder.resolved("shown", shown);
    builder.finish();
    assert_eq!(
        frame.paint_order().collect::<Vec<_>>(),
        [far, near, moving, panel, shown, dragged]
    );
    assert_eq!(hit(&frame, point), Some(dragged));

    // Transparent nodes don't catch a press. The clipped preview only catches presses on
    // the visible half, and the moving card is in front of the nearer resting card.
    frame.resolved[dragged as usize].opacity = 0.0;
    assert_eq!(hit(&frame, Vec2::new(24.0, 16.0)), Some(shown));
    assert_eq!(hit(&frame, point), Some(panel));
    frame.resolved[panel as usize].opacity = 0.0;
    assert_eq!(hit(&frame, point), Some(moving));
}

#[test]
fn keep_clear_uses_the_first_slot_that_clears_the_queried_children() {
    let mut tree = Tree::default();
    let group = tree.push(Node::screen(ElementId(1), "cards", Rect::default()));
    let mut card = Node::screen(ElementId(2), "card", BOX);
    card.parent = Some(group);
    tree.push(card);
    let clear = Rect {
        x: BOX.x + BOX.width,
        ..BOX
    };
    // Overlapping a node outside the queried group doesn't block this slot.
    tree.push(Node::screen(ElementId(3), "other", clear));
    let blocked = tree.add_slot(BOX.into());
    let preferred = tree.add_slot(clear.into());
    let spare = tree.add_slot(
        Rect {
            y: BOX.y + BOX.height,
            ..BOX
        }
        .into(),
    );
    let mut panel = Node::screen(ElementId(4), "panel", Rect::default());
    panel.keep_clear = Some(KeepClear {
        of: vec![NodeQuery::Children(NodeRef::Id(ElementId(1)))],
        places: vec![blocked, preferred, spare],
    });
    let index = tree.push(panel);
    tree.solve(SolvePhase::Handle, |_, _| {});
    assert_eq!(tree.selected_slot(index), Some(preferred));
    assert_eq!(tree.nodes()[index as usize].space.screen_bounds(), clear);
}
