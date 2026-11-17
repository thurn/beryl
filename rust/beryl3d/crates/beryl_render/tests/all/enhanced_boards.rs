//! Every gated board draws in the Enhanced tier, multisampled and with bloom, on a device with
//! wgpu's default limits. wgpu panics on a validation error nobody captures, so a pass, target
//! or pipeline the Enhanced tier gets wrong on a real board fails the test.

use beryl_inspect::gate::{BOARDS, Playback, TARGET, rank};
use beryl_render::{Limits, Renderer, Target};
use beryl_scene::input::Tier;

#[test]
fn every_gated_board_draws_in_enhanced_on_a_full_device() {
    let mut renderer = Renderer::headless(Limits::Default).expect("a GPU adapter and device");
    assert!(
        renderer.enhanced_available(),
        "the adapter {:?} has no Enhanced support",
        renderer.adapter()
    );
    let [width, height] = TARGET;
    let mut target = Target::offscreen(&renderer, width, height);
    for name in BOARDS {
        // The frames with the most work, which hold the most records.
        let ranked = rank(name).expect("a gated board");
        let mut playback = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Enhanced);
        let mut drawn = 0;
        while let Some(moment) = playback.moment() {
            playback.handle();
            let frame = playback.frame();
            if ranked.contains(&moment.frame) {
                assert_eq!(frame.tier, Tier::Enhanced, "{name}: the frame's tier");
                renderer.render(frame, &mut target);
                drawn += 1;
            }
        }
        renderer.finish();
        assert_eq!(drawn, ranked.len(), "{name}: every ranked frame was drawn");
    }
}
