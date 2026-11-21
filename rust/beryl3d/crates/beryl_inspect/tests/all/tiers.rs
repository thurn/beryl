//! The two visual tiers differ only in shading (docs/beryl-3d.md, "The Enhanced tier"): on
//! every gated board, each frame resolves the same elements and draws the same records in
//! both, but for the records only one tier writes, contact shadows in Baseline and particles in
//! Enhanced.

use beryl_inspect::gate::{BOARDS, Playback};
use kit_frame::frame::{Draw, Frame, Material};
use kit_ui::input::Tier;

/// The records of `frame` other than those of `material`.
fn records_but(frame: &Frame, material: Material) -> Vec<Draw> {
    (frame.draws.iter())
        .filter(|draw| draw.material != material as u8)
        .copied()
        .collect()
}

#[test]
fn every_gated_board_resolves_and_draws_the_same_in_both_tiers() {
    for name in BOARDS {
        let mut baseline = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Baseline);
        let mut enhanced = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Enhanced);
        while let Some(moment) = baseline.moment() {
            assert_eq!(enhanced.moment(), Some(moment), "{name}");
            baseline.handle();
            enhanced.handle();
            let frame = moment.frame;
            let (base, enhanced_frame) = (baseline.frame(), enhanced.frame());
            assert_eq!(base.tier, Tier::Baseline);
            assert_eq!(enhanced_frame.tier, Tier::Enhanced);
            assert!(
                base.resolved == enhanced_frame.resolved,
                "{name} frame {frame}: the resolved elements differ"
            );
            assert!(
                base.names == enhanced_frame.names,
                "{name} frame {frame}: the names differ"
            );
            let base_draws = records_but(base, Material::ContactShadow);
            let enhanced_draws = records_but(enhanced_frame, Material::Particle);
            if let Some(index) = (0..base_draws.len().max(enhanced_draws.len()))
                .find(|&index| base_draws.get(index) != enhanced_draws.get(index))
            {
                panic!(
                    "{name} frame {frame}: record {index} differs\n  Baseline: {:?}\n  Enhanced: {:?}",
                    base_draws.get(index),
                    enhanced_draws.get(index),
                );
            }
        }
        assert_eq!(enhanced.moment(), None, "{name}");
    }
}

/// A step down to Baseline in the middle of play moves nothing (docs/beryl-3d.md, "Choosing a
/// tier"): on each gated board with particles, a playback that steps down on the first frame
/// with particles, while a `vanish` is under way, draws every frame from then on exactly as a
/// playback in Baseline from the start draws it.
#[test]
fn a_step_down_in_the_middle_of_a_vanish_draws_what_baseline_draws() {
    let mut stepped_any = false;
    for name in BOARDS {
        let mut baseline = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Baseline);
        let mut stepped = Playback::new(name)
            .expect("a gated board")
            .with_tier(Tier::Enhanced);
        let mut stepped_at = None;
        while let Some(moment) = baseline.moment() {
            assert_eq!(stepped.moment(), Some(moment), "{name}");
            baseline.handle();
            stepped.handle();
            let frame = moment.frame;
            let base = baseline.frame();
            let drawn = stepped.frame();
            if stepped_at.is_some() {
                assert_eq!(drawn.tier, Tier::Baseline, "{name} frame {frame}");
                assert!(
                    base.resolved == drawn.resolved && base.names == drawn.names,
                    "{name} frame {frame}: the elements differ after the step down"
                );
                assert!(
                    base.draws == drawn.draws,
                    "{name} frame {frame}: the records differ after the step down"
                );
            } else if drawn.load.particles > 0 {
                stepped_at = Some(frame);
                stepped.set_tier(Tier::Baseline);
            }
        }
        stepped_any |= stepped_at.is_some();
    }
    assert!(stepped_any, "no gated board has a frame with particles");
}
