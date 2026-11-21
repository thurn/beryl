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
        child.show = Some(Show {
            object: target.clone(),
        });
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
        assert_eq!(child.show.as_ref().unwrap().object, anchor.to);
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
    ]
    .into_iter();
    let mut record = |step, _: &mut Tree| assert_eq!(Some(step), expected.next());
    tree.solve(SolvePhase::Handle, &mut record);
    tree.solve(SolvePhase::Frame, &mut record);
    tree.solve(SolvePhase::Frame, &mut record);
    assert_eq!(expected.next(), None);
}
