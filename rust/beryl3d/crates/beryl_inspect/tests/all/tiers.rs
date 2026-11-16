//! The two visual tiers differ only in shading (docs/beryl-3d.md, "The Enhanced tier"): on
//! every gated board, each frame resolves the same elements and draws the same records in
//! both, but for the records only one tier writes, contact shadows in Baseline and particles in
//! Enhanced.

use beryl_inspect::gate::{BOARDS, Playback};
use beryl_scene::frame::{Draw, Frame, Material};
use beryl_scene::input::Tier;

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
