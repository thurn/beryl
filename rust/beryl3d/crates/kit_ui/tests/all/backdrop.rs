use glam::Affine3A;
use kit_base::geometry::{Rect, Window, plane::ScreenPlane};
use kit_base::tokens::color;
use kit_frame::frame::{Frame, FrameBuilder};
use kit_ui::element::{Filter, Position, holder, row};
use kit_ui::input::{Insets, Size, Tier};
use kit_ui::layout::Layer;

fn frame(tier: Tier) -> Frame {
    let window = Window {
        size: Size {
            width: 240.0,
            height: 160.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    };
    let panel = row()
        .position(Position::Absolute)
        .inset([Some(30.0), None, None, Some(20.0)])
        .width(80.0)
        .height(40.0)
        .radius(6.0)
        .backdrop_filter([Filter::Blur(4.0)].to_vec())
        .child(
            row()
                .width(20.0)
                .height(20.0)
                .background([0, 128, 255, 255]),
        );
    let mut layer = Layer::new();
    layer.resize(window);
    let root = holder().position(Position::Absolute).inset([Some(0.0); 4]);
    layer.show(Some(root.child(panel)));
    let mut frame = Frame::with_capacity(8, 8, 32);
    let mut builder = FrameBuilder::new(&mut frame, tier);
    layer.write(
        &mut builder,
        &ScreenPlane::new(Affine3A::IDENTITY, window, 1.0, 1.0),
        true,
    );
    builder.finish();
    frame
}

#[test]
fn backdrop_uses_the_same_box_in_both_tiers() {
    let enhanced = frame(Tier::Enhanced);
    let backdrop = enhanced
        .backdrop
        .expect("the Enhanced frame schedules a blur");
    let panel = backdrop.panel.expect("the blur has a box");
    assert_eq!(
        panel.rect,
        Rect {
            x: 40.0,
            y: 60.0,
            width: 160.0,
            height: 80.0
        }
    );
    assert_eq!(panel.clip, panel.rect);
    assert!((panel.radius - 12.0).abs() < f32::EPSILON);
    assert!((backdrop.blur - 8.0).abs() < f32::EPSILON);
    assert_eq!(backdrop.at, enhanced.layer, "the blur replaces the fill");

    let baseline = frame(Tier::Baseline);
    assert!(baseline.backdrop.is_none());
    let [fallback, child] = baseline.draws.as_slice() else {
        panic!("expected a fallback fill and the child");
    };
    assert_eq!(fallback.tint, color::BACKDROP_FILTER_FALLBACK);
    assert_eq!(child.tint, enhanced.draws[backdrop.at as usize + 1].tint);
    assert_eq!(enhanced.resolved[1].corners, baseline.resolved[1].corners);
}
