//! Offscreen samples of the flat layer's CSS paint capabilities.

use beryl_render::{Renderer, Target};
use kit_base::geometry::{Insets, Size, Window};
use kit_frame::frame::{Frame, FrameBuilder};
use kit_space::camera::{Camera, DESKTOP_CAMERA};
use kit_ui::element::{Element, Filter, Gradient, Position, Stop, holder, row};
use kit_ui::input::Tier;
use kit_ui::layout::Layer;

use super::enhanced::{enhanced, within};
use super::render::Picture;

const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255; 4];
const RED: [u8; 4] = [200, 24, 12, 255];

fn box_at(x: f32, y: f32, width: f32, height: f32) -> Element {
    row()
        .position(Position::Absolute)
        .inset([Some(y), None, None, Some(x)])
        .width(width)
        .height(height)
}

fn picture(
    renderer: &mut Renderer,
    tier: Tier,
    elements: impl IntoIterator<Item = Element>,
) -> (Picture, u32) {
    let window = Window {
        size_class: None,
        size: Size {
            width: 200.0,
            height: 120.0,
        },
        scale: 1.0,
        safe_area: Insets::default(),
    };
    let mut layer = Layer::new();
    layer.resize(window);
    let root = holder().position(Position::Absolute).inset([Some(0.0); 4]);
    layer.show(Some(root.children(elements)));
    let mut frame = Frame::with_capacity(16, 16, 64);
    let mut builder = FrameBuilder::new(&mut frame, tier);
    let camera = Camera::fit(DESKTOP_CAMERA, window);
    builder.camera(camera.view_projection);
    layer.write(&mut builder, &camera.screen_plane(), true);
    let mut target = Target::offscreen(renderer, 200, 120);
    let drawn = renderer.render(builder.finish(), &mut target);
    (
        Picture {
            pixels: renderer.read(&target),
            width: 200,
        },
        drawn.draw_calls,
    )
}

#[test]
fn backdrop_blurs_only_the_panel_and_its_contents_draw_sharp() {
    let mut renderer = enhanced();
    for tier in [Tier::Enhanced, Tier::Baseline] {
        let panel = box_at(60.0, 30.0, 80.0, 60.0)
            .radius(10.0)
            .backdrop_filter(vec![Filter::Blur(6.0)])
            .child(box_at(10.0, 20.0, 8.0, 8.0).background(RED));
        let (picture, draws) = picture(
            &mut renderer,
            tier,
            [box_at(0.0, 0.0, 100.0, 120.0).background(WHITE), panel],
        );
        assert_eq!(
            picture.at(99, 16),
            [255; 3],
            "above the panel the edge is sharp"
        );
        assert_eq!(picture.at(100, 16), [0; 3]);
        assert_eq!(picture.at(60, 30), [255; 3], "the rounded corner is clear");
        assert!(within(picture.at(72, 52), RED[..3].try_into().unwrap(), 1));
        let before = picture.at(99, 60);
        let after = picture.at(100, 60);
        if tier == Tier::Enhanced {
            assert!((120..=150).contains(&before[0]), "{before:?}");
            assert!((105..=135).contains(&after[0]), "{after:?}");
            assert!(before[0] > after[0] && before[0] - after[0] < 25);
            assert!(draws <= 7, "the bounded blur took {draws} draw calls");
        } else {
            assert!(
                before[0] > after[0] + 45,
                "the fallback keeps the edge sharp"
            );
            assert!(after[0] > 0, "the fallback tint is visible");
            assert!(draws <= 3, "the fallback took {draws} draw calls");
        }
    }
}

#[test]
fn element_blur_mask_and_grayscale_change_pixels() {
    let mut renderer = enhanced();
    let mask = Gradient::linear(90.0, &[Stop::new([0; 4]), Stop::percent(BLACK, 70.0)]);
    let (picture, _) = picture(
        &mut renderer,
        Tier::Enhanced,
        [
            box_at(12.0, 30.0, 40.0, 60.0)
                .background(WHITE)
                .filter(vec![Filter::Blur(4.0)]),
            box_at(65.0, 30.0, 60.0, 60.0).background(WHITE).mask(mask),
            box_at(140.0, 30.0, 40.0, 60.0)
                .background(RED)
                .filter(vec![Filter::Grayscale(1.0)]),
        ],
    );
    assert_eq!(picture.at(26, 60), [255; 3]);
    assert!(
        (30..=240).contains(&picture.at(11, 60)[0]),
        "the blur spills beyond the box"
    );
    assert!(
        picture.at(65, 60)[0] < 5,
        "the mask starts nearly transparent"
    );
    assert!(picture.at(86, 60)[0] > 90 && picture.at(86, 60)[0] < 160);
    assert_eq!(picture.at(116, 60), [255; 3]);
    let gray = picture.at(160, 60);
    assert!(
        (50..65).contains(&gray[0]) && gray == [gray[0]; 3],
        "{gray:?}"
    );
}
