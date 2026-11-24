use glam::Affine3A;
use kit_base::geometry::{Window, plane::ScreenPlane};
use kit_frame::frame::{Frame, FrameBuilder, Material, RADIAL};
use kit_ui::element::{BackgroundBox, Border, Extent, Gradient, Length, Stop, holder, row};
use kit_ui::input::Tier;
use kit_ui::layout::Layer;

#[test]
fn a_padding_box_gradient_leaves_the_border_box_gradient_visible() {
    let gold = [255, 195, 63, 255];
    let stone = [53, 50, 48, 255];
    let fill = Gradient::linear(180.0, &[Stop::new(stone)]);
    let rim = Gradient::radial(
        false,
        Extent::FarthestCorner,
        (Length::Percent(50.0), Length::Percent(50.0)),
        &[Stop::new(gold)],
    );
    let panel = row()
        .width(100.0)
        .height(50.0)
        .radius(10.0)
        .border(Border::uniform([0; 4], 4.0))
        .background_image(vec![fill, rim])
        .background_boxes(vec![BackgroundBox::Padding, BackgroundBox::Border]);
    let mut layer = Layer::new();
    layer.show(Some(holder().child(panel)));

    let plane = ScreenPlane::new(Affine3A::IDENTITY, Window::default(), 1.0, 1.0);
    let mut frame = Frame::with_capacity(4, 4, 16);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline, crate::card());
    layer.write(&mut builder, &plane, true);
    let frame = builder.finish();

    let [rim, fill] = frame.draws.as_slice() else {
        panic!("expected two gradient draws, got {}", frame.draws.len());
    };
    for draw in [rim, fill] {
        assert_eq!(draw.material, Material::Gradient as u8);
    }
    assert_ne!(rim.layer & RADIAL, 0);
    assert_eq!(fill.layer & RADIAL, 0);
    for (params, expected) in [
        (rim.params, [100.0, 50.0, 10.0]),
        (fill.params, [92.0, 42.0, 6.0]),
    ] {
        assert!(
            params
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < f32::EPSILON)
        );
    }
}
