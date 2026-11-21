//! Every gated board draws on a device with wgpu's downlevel limits, the OpenGL ES 3.0 and WebGL
//! 2 feature set of the floor (docs/beryl-3d.md, "The Baseline tier": clamped limits in the
//! automated checks). wgpu panics on a validation error nobody captures, so a buffer, texture or
//! shader the floor could not take fails the test.

use beryl_inspect::gate::{BOARDS, Playback, rank};
use beryl_render::{Limits, Renderer, Target};
use kit_base::geometry::Size;

/// The largest window the downlevel limits allow a target for: their 2D textures are at most
/// 2,048 pixels on a side, so the gates' own 2880 by 1800 target cannot be made.
const WINDOW: Size = Size {
    width: 1920.0,
    height: 1080.0,
};

#[test]
fn every_gated_board_draws_within_the_floors_limits() {
    let mut renderer = Renderer::headless(Limits::Downlevel).expect("a GPU adapter and device");
    let mut target = Target::offscreen(&renderer, WINDOW.width as u32, WINDOW.height as u32);
    for name in BOARDS {
        // The frames with the most work, which hold the most records.
        let ranked = rank(name).expect("a gated board");
        let mut playback = Playback::in_window(name, WINDOW, 1.0).expect("a gated board");
        let mut drawn = 0;
        while let Some(moment) = playback.moment() {
            playback.handle();
            let frame = playback.frame();
            if ranked.contains(&moment.frame) {
                renderer.render(frame, &mut target);
                drawn += 1;
            }
        }
        renderer.finish();
        assert_eq!(drawn, ranked.len(), "{name}: every ranked frame was drawn");
    }
}
