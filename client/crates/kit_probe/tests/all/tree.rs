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
        count: 1,
        annotations: Default::default(),
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

#[test]
fn tree_and_locator_use_the_scenes_registered_labels() {
    use kit_frame::frame::Labels;
    use kit_probe::agent::{line::Line, locate};

    const GAUGE: Role = Role::Custom("gauge");
    const ALERT: u32 = 1;
    const LABELS: Labels = {
        let mut labels = Labels::DEFAULT.with_state(ALERT, "alert");
        labels.roles = &[GAUGE];
        labels.count = Some("count");
        labels.annotations[0] = Some("detail");
        labels
    };

    let mut frame = Frame::with_capacity(1, 0, 64);
    frame.labels = &LABELS;
    let mut builder = FrameBuilder::new(
        &mut frame,
        Tier::Baseline,
        crate::test_data::data().card.size(),
    );
    let element = Resolved {
        place: Place::named("rack.upper"),
        state: StateBits(StateBits::PASSES | ALERT),
        count: 3,
        ..rectangle(4, u32::MAX, GAUGE, Some(Pose::default()), Vec2::ZERO)
    };
    let index = builder.resolved("pressure", element);
    builder.annotate(index, 0, "rising");
    let frame = builder.finish();
    let line = kit_probe::agent::tree::line(frame, index);
    assert_eq!(
        line,
        r#"gauge "pressure" #4 rack.upper 0,0 16x8 alert count(3) detail("rising")"#
    );
    assert_eq!(locate(frame, &line).unwrap(), index);
    let Line::Element { element, .. } = Line::parse_with_labels(&line, &LABELS).unwrap() else {
        panic!("an element line");
    };
    assert_eq!(element.states, ["alert", "count(3)", r#"detail("rising")"#]);
}
