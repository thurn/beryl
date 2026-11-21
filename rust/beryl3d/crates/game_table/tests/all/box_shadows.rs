//! CSS box shadows in the flat layer: outer shadows are written before the element's panel and
//! inset ones after it, as CSS paints them, each quad the box moved by the offset and grown by
//! the spread and three deviations of its blur.

#![expect(
    clippy::float_cmp,
    reason = "the record copies the box's size and the shadow's numbers, so they are exact"
)]

use game_table::table::Table;
use kit_frame::frame::{Frame, FrameBuilder, Material};
use kit_ui::element::{Position, Shadow, holder, row};
use kit_ui::input::Tier;
use kit_ui::layout::Layer;

#[test]
fn outer_shadows_go_under_the_panel_and_inset_ones_over_it() {
    let mut panel = row()
        .position(Position::Absolute)
        .inset([Some(100.0), None, None, Some(200.0)])
        .width(200.0)
        .height(80.0)
        .background([33, 31, 28, 255]);
    panel.style.box_shadow = vec![
        Shadow::new(0.0, 0.0, 20.0, [233, 173, 74, 89]).inset(),
        Shadow::new(0.0, 6.0, 16.0, [0, 0, 0, 128]),
        Shadow::new(0.0, 0.0, 0.0, [228, 162, 66, 255]).spread(3.0),
    ];
    let mut layer = Layer::new();
    layer.show(Some(holder().child(panel)));
    let mut frame = Frame::with_capacity(16, 16, 64);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    layer.write(
        &mut builder,
        &Table::new(
            crate::test_data::data(),
            game_table::table::DESKTOP,
            game_table::table::Window::default(),
        )
        .camera()
        .screen_plane(),
        true,
    );
    let frame = builder.finish();
    let drawn: Vec<(u8, u16)> = (frame.draws.iter())
        .filter(|draw| {
            draw.material == Material::BoxShadow as u8 || draw.material == Material::Panel as u8
        })
        .map(|draw| (draw.material, draw.layer))
        .collect();
    let (shadow, panel) = (Material::BoxShadow as u8, Material::Panel as u8);
    // The outer shadows last first (the ring was listed last, so it is drawn under the drop
    // shadow), then the panel, then the inset shadow.
    assert_eq!(drawn, [(shadow, 0), (shadow, 0), (panel, 0), (shadow, 1)]);
    let ring = frame
        .draws
        .iter()
        .find(|draw| draw.material == shadow)
        .expect("a shadow");
    // A spread of 3 points and no blur: the box's own size, with no shape radius.
    assert_eq!(ring.params, [200.0, 80.0, 0.0, 0.0]);
    assert_eq!(ring.uv[2], 32_768 + 3 * 16);
}
