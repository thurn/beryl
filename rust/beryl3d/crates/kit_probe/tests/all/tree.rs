use glam::Vec2;
use kit_base::{atlas::Tier, pose::Pose};
use kit_frame::frame::{ElementId, Frame, FrameBuilder, NameRef, Place, Resolved, Role, StateBits};
use kit_probe::agent::{TreeOptions, tree};

fn rectangle(id: u32, parent: u32, role: Role, pose: Option<Pose>, origin: Vec2) -> Resolved {
    Resolved {
        id: ElementId(id),
        parent,
        role,
        name: NameRef::default(),
        place: Place::Layer,
        pose,
        corners: [
            origin,
            origin + Vec2::new(16.0, 0.0),
            origin + Vec2::new(16.0, 8.0),
            origin + Vec2::new(0.0, 8.0),
        ],
        depth: 0.0,
        opacity: 1.0,
        clip: None,
        interactive: false,
        state: StateBits(StateBits::PASSES),
        face: None,
        copies: 1,
        really: NameRef::default(),
        note: NameRef::default(),
    }
}

#[test]
fn prints_table_and_screen_nodes_and_their_children() {
    let mut frame = Frame::with_capacity(3, 0, 32);
    let mut builder = FrameBuilder::new(
        &mut frame,
        Tier::Baseline,
        crate::test_data::data().card.size(),
    );
    let panel = builder.resolved(
        "panel",
        rectangle(1, u32::MAX, Role::Region, None, Vec2::new(16.0, 16.0)),
    );
    builder.resolved(
        "shape",
        rectangle(2, u32::MAX, Role::Region, Some(Pose::default()), Vec2::ZERO),
    );
    builder.resolved(
        "label",
        rectangle(3, panel, Role::Text, None, Vec2::new(16.0, 16.0)),
    );

    let printed = tree(builder.finish(), &TreeOptions::default()).unwrap();
    assert_eq!(
        printed,
        concat!(
            "game\n",
            "  table\n",
            "    region \"shape\" #2 0,0 16x8\n",
            "  layer\n",
            "    region \"panel\" #1 16,16 16x8\n",
            "      text \"label\" #3 16,16 16x8\n",
        )
    );
}
